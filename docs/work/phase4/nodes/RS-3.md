# RS-3 — `POST /openai/v1/responses` route, test harness, request side of the forward step, errors before the first byte, redaction

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.1 (first item; identity and error shape), K§6.4.2 (stage table rows identity, resolution, redaction, attribution), K§6.4.3 (the request side of Responses → Anthropic and Responses → chat), K§6.4.4, K§6.7 (Responses requests), K§11 items 12 (at the route), 14 (the before-the-first-byte half) and 20; K-D47, K-D50
**Files:** `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/src/pipeline.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/errors.rs`, `crates/zygo-proxy/src/redact_stage.rs`, `crates/zygo-proxy/src/resolve.rs` (planning: candidates of a Responses client), `crates/zygo-proxy/tests/translate.rs` (planning: `wire_openai_routes_not_in_v1` claims the route is 404), `crates/zygo-proxy/tests/support/responses.rs` (new), `crates/zygo-proxy/tests/responses_route.rs` (new), `docs/specs/2026-11-25-client-model-design.md` (review round 1: F1, F2), `crates/zygo-proxy/src/translate/responses_client.rs` (review round 2: `is_format_or_separator` made `pub(crate)` for the PC14 message; `translate/mod.rs` is back to the base)
**Consumes:** `check_fields` (RS-1), `request` (RS-1), `fixtures/codex/requests/` (PC-3), `fixtures/codex/errors/` (PC-3)
**Produces:** `pipeline::ModelRoute::Responses`, `support::Harness::start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness` (proxy on a `ManualClock`, one session of `agent`, one mock upstream of dialect `upstream`), `Harness::post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes)`, `Harness::get(&self, path: &str) -> (StatusCode, HeaderMap, Bytes)`, `Harness::upstream_bodies(&self) -> Vec<Value>`, `Harness::reply_with(&self, status: u16, content_type: &str, body: Vec<u8>)`, `Harness::usage_rows(&self) -> Vec<RequestRow>`; added in planning: `Harness::post_as(&self, path: &str, headers: &[(&str, &str)], body: &Value) -> (StatusCode, HeaderMap, Bytes)`, `Harness::token(&self) -> &str`, `Harness::session_id(&self) -> &str`, `Harness::upstream_paths(&self) -> Vec<String>`, `Harness::upstream_headers(&self) -> Vec<HeaderMap>`, `support::codex_request(name: &str) -> Value`, consts `support::{MODEL, UPSTREAM_MODEL, PROVIDER, MAX_OUTPUT}`; added in review round 1: `Harness::provider_key(&self) -> &str`
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_route -- --list 2>/dev/null | grep -c ': test$')" -ge 7 && cargo test -p zygo-proxy --locked` → at least 7 tests in `responses_route` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: a missing or wrong `X-Zygo-Session` or bearer → the identity error in the OpenAI error shape; an unknown and a refused top-level field → 400 in the OpenAI shape before any upstream request (K§11 item 12 at the route); a captured Codex request reaches an Anthropic mock as a Messages body and a chat mock as a chat body (seen by `upstream_bodies`); an upstream `529` before the first byte on a streaming request → `529`, an OpenAI error body and no SSE bytes (K§11 item 14, first half); an over-long-prompt upstream error reaches the client in the PC14 shape; redaction acts on a Responses `input` body: the planted secret is absent from the upstream body (K§11 item 20)

## Planning notes

- Route by dialect: a Responses request from any session is served (K-D47). Tests use a Claude Code session (K§6.4.2).
- Request side at forward time: `attempt_body` (`forward.rs:779-831`) translates when the candidate dialect differs from the client's. For this client it chains `request` (RS-1), then for a chat candidate the existing Messages → chat translator. The response side of a 2xx answer (the encoder, the stream order, the non-streaming form) is RS-4; until RS-4 merges, a 2xx answer on this route gives a `zygo_` error, and no test here claims it. The same-dialect body (Responses upstream) is RS-5. The tests of this node use upstream error answers, so the request a mock receives is seen without a 2xx path.
- Redaction already picks `Layout::Input` for `Dialect::Openai` (`redact_stage.rs:191`); this node proves it on the route and edits the stage only if the route needs it.
- `upstream_path(route, d)` (`forward.rs:182-188`) and `ModelRoute::client_path` / `upstream_path` (`:166-179`) gain the `Responses` arm.
- `tests/support/responses.rs` is included by each Responses test file with `#[path = "support/responses.rs"] mod support;`, the form of `crates/zygo-launch/tests/premise.rs:4-5`; `tests/support/` holds no `main.rs`, so Cargo builds no target from it. Later RS nodes that change it depend on this node through their edges.
- Test hooks: the proxy's `test-hooks` feature (`crates/zygo-proxy/Cargo.toml:40-43`) and `ManualClock`; no sleep, no wall-clock bound (Global constraints).

- Lead's decisions (2026-11-25): the Files and Produces changes of this plan are accepted. Usage-row wait: no new or widened wall-clock bound in CI tests. The usage writer has no explicit flush or drain inside Files (`UsageSink::wait` is reached only through `ProxyHandle::usage_written`, `server.rs:322`; `ProxyHandle::shutdown`, `server.rs:308`, consumes the handle), so `Harness::usage_rows` reuses `usage_written(n, Duration::from_secs(10))`, the helper and bound the existing tests use (`tests/translate.rs:1114`); no 60 s bound.
- Lead's decision (2026-11-25, RS-3 review round 1 F2): `openai` is no candidate of a Responses client; candidates are `anthropic`, then `openai-chat`; with neither, the existing `400 zygo_no_provider`. RS-5 re-enables the openai candidate with store:=false and Responses usage parsing.

## Plan

Base: `origin/phase-4` at fe18f3dd (RS-1 merged). Branch `phase-4--RS-3`. Every commit ends with the lines `Zygo-Session: <session>` and `Zygo-Work: phase4:RS-3`; no attribution. Builds use `CARGO_INCREMENTAL=0`.

### Premise check (2026-11-25, against fe18f3dd)

- RS-1 landed: `check_fields(req: &Value) -> Result<(), ZygoError>` (`translate/responses_client.rs:84`), `request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError>` (`:104`; it calls `check_fields` first, `:105`), `IGNORED` (`:30`), `REFUSED` (`:44`), `NO_STATE` (`:47`). `request` keeps `model` as sent, so `model_body` (`forward.rs:833`) still sets `upstream_model`.
- `max_tokens` with no client `max_output_tokens` comes only from `facts.max_output_tokens` (`responses_client.rs:225-232`); no captured body sends `max_output_tokens`, and every one has `"model": "gpt-mock"`, `"stream": true` (the record's `body` key). So the harness catalog row is `gpt-mock` with a `[model.facts]` table, else every captured body answers 400.
- **Drift 1 (blocks the node).** `resolve::upstream_dialect` (`resolve.rs:80-88`) returns `None` for a client dialect other than `Anthropic` when the provider does not speak it. With `ctx.dialect = Openai`, an `anthropic` or `openai-chat` provider is no candidate, and the request answers `400 zygo_no_provider` (`resolve.rs:65`). K§6.4.3 requires both pairs. Fixed in this node: `resolve.rs` is added to Files.
- **Drift 2 (blocks the node).** `tests/translate.rs:1384-1403` (`wire_openai_routes_not_in_v1`) asserts `POST /openai/v1/responses` → `404 zygo_route_not_in_v1`; the route this node builds breaks it, and the Acceptance runs every `zygo-proxy` target. `tests/translate.rs` is added to Files; one tuple is removed (step 9; a listed test-logic edit).
- Error shape: proxy errors use `IntoResponse for ZygoError` = Anthropic shape (`errors.rs:46-50`), reached from `routes.rs:257` (identity) and `forward.rs:311` (every later error). `ZygoError::body(Dialect::Openai)` gives `{"error":{"type":code,"message":m}}` (`errors.rs:37-43`).
- `run` sets `dialect: Dialect::Anthropic` (`forward.rs:338`); the stages key on it: resolution (`resolve.rs:100,105`), redaction layout (`redact_stage.rs:191`). Cache (`cache.rs:35`) and window (`window.rs:130`) act only on `ModelRoute::Messages`, so they skip this route with no edit.
- Upstream errors on a translated candidate: `translate_error` (`translate/mod.rs:67`) gives a Messages error; a context-limit refusal becomes `400 "prompt is too long: <n> tokens > <max> maximum"` only when `context_error` (`:118`) matches, which it does for OpenAI and llama forms but not for Anthropic's own `prompt is too long` message (that one passes through with its status and text). PC14 (`fixtures/codex/errors/context-400.json`): status 400, `{"error":{"message":…,"type":"invalid_request_error","param":"input","code":"context_length_exceeded"}}`; Codex recovers from it (`errors/result.txt`, `e1`).
- A 2xx from a translated candidate would reach `StreamMapper::new(c.dialect, …)` (`forward.rs:510`) or `translate_response` (`:518`), both of which read Responses or chat as the upstream form, never Messages, and emit Messages to the client. Until RS-4, step 6 answers it with `502 zygo_upstream_bad_response` (`forward.rs:194`) in the OpenAI shape.
- `outbound_headers` (`forward.rs:71-101`) forwards `anthropic-version` only when the client sent it; Codex sends none (`fixtures/codex/requests/*.json` `headers`), and the Anthropic API refuses a request without it. Step 6 sets it for this pair (K§10: the field mapping is the builder's).
- Redaction: `Layout::Input` for `Dialect::Openai` (`redact_stage.rs:191`), user `input_text` parts of `message` items (`zygo-redact/src/content.rs:195-205`); the secret pass walks every string. No stage edit is needed: `redact_stage.rs` stays unchanged.
- Test seams: `ManualClock` (`session.rs:64`), `StartOpts { clock, keys, usage_db, redact_limits, .. }` (`server.rs:159-205`), `ProxyHandle::create_session` (`server.rs:303`), `usage_written(n, limit)` (`server.rs:322-325`). The mock-upstream form is `tests/redaction_e2e.rs:32-71`; the key resolver form is `tests/translate.rs:962-983`; generous redaction limits `tests/translate.rs:1030-1037` (no redaction deadline can fire).
- `tests/support/` exists nowhere yet; `#[path]` inclusion as `crates/zygo-launch/tests/premise.rs:4-5`.

### Interfaces

Consumed (as at fe18f3dd):

- `pub fn check_fields(req: &Value) -> Result<(), ZygoError>`; `pub fn request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError>`; `pub const IGNORED: [&str; 10]`, `pub const REFUSED: [&str; 4]`, `pub const NO_STATE: &str` (`translate/responses_client.rs:30-104`)
- `pub fn translate_request(req: &Value, to: Dialect, target: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError>`, `pub fn translate_error(status: StatusCode, body: &[u8], estimate: u64, key: Option<&str>) -> (StatusCode, Value)` (`translate/mod.rs:41, 67`)
- `pub fn facts::apply(req: &mut Value, f: &Facts) -> Vec<Adjustment>`, `pub fn facts::write_effort(from: &Value, out: &mut Value, to: Dialect)` (`translate/facts.rs:15, 144`); `pub fn schema::rewrite` as called at `forward.rs:795`
- `fixtures/codex/requests/{first-turn,after-function-call-output,after-custom-tool-call-output,turn-with-reasoning}.json` (record; body under `body`); `fixtures/codex/errors/context-400.json`. Read-only.

Produced:

- `pipeline::ModelRoute::Responses`; `ModelRoute::dialect(self) -> Dialect` (`Responses` → `Openai`, else `Anthropic`)
- `errors::ZygoError::response(&self, d: Dialect) -> Response`
- `tests/support/responses.rs`: `Harness::start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness`, `post`, `post_as`, `get`, `token`, `session_id`, `upstream_bodies`, `upstream_paths`, `upstream_headers`, `reply_with`, `usage_rows`, `codex_request`, consts `MODEL = "gpt-mock"`, `UPSTREAM_MODEL = "up-rs"`, `PROVIDER = "p-rs"`, `MAX_OUTPUT: u64 = 32000` (signatures in the Produces line)

### Steps

- [x] 1. **Harness.** Create `crates/zygo-proxy/tests/support/responses.rs`, first line `#![allow(dead_code)]` (each test file uses part of it). Contents:
  - Mock upstream as `tests/redaction_e2e.rs:32-71` (127.0.0.1:0, own runtime thread, stop on drop), recording `(path with query, HeaderMap, body bytes)` per request and answering the current reply. The reply starts as status 400, `content-type: application/json`, body `{"type":"error","error":{"type":"invalid_request_error","message":"rs3 mock"}}` for `Dialect::Anthropic`, else `{"error":{"type":"invalid_request_error","message":"rs3 mock"}}`; `reply_with` replaces it for all later requests.
  - `start`: a `TempDir`; `config.toml` = the text below, then `extra_toml`; `load(&LoadContext { paths, repo: None, team: None })` with `Paths` as `tests/translate.rs:1010-1023`; `Proxy::start_local_with(cfg, StartOpts { clock: ManualClock::new(), keys: Arc::new(<one key "k-rs", value built at run time as translate.rs:981-983>), usage_db: Some(<tmp>/data/usage.db), redact_limits: Some(<generous: 60 s base, ZERO per MiB, 60 s content budget>), ..Default::default() })`; `create_session(SessionRequest { agent, agent_version: "2.1.328".into(), profile: None, passthrough: false, repo: None, work: None, stored_values: vec![], known_plugins: Default::default() })`. TOML (`{DIALECT}` = `anthropic` | `openai` | `openai-chat`; `{KEY}` = `key = "keychain:k-rs"` for a non-`anthropic` dialect, else empty):
    ```toml
    [role.code]
    model = "gpt-mock"

    [[model]]
    id = "gpt-mock"
    [model.facts]
    reasoning = "always"
    effort_levels = ["low", "medium", "high"]
    max_output_tokens = 32000
    context_window = 200000
    sampling = true
    vision = true
    structured_output = "native"
    forced_tool_choice = true

    [[model.provider]]
    name = "p-rs"
    base_url = "{URL}"
    dialects = ["{DIALECT}"]
    upstream_model = "up-rs"
    {KEY}
    ```
  - `post(path, body)`: headers `authorization: Bearer <token>`, `x-zygo-session: <id>`, `content-type: application/json` (the Codex form, PC1/PC2); counts one model post. `post_as(path, headers, body)`: exactly `headers` plus `content-type`; counts nothing. `get(path)`: the two identity headers. Each blocks on a current-thread runtime as `tests/translate.rs:1082-1098`, with a `no_proxy` client; `path` is joined to `ProxyHandle::url()`.
  - `upstream_bodies` parses each recorded body as JSON (panics on a non-JSON body); `upstream_paths`, `upstream_headers` return the recorded values in order.
  - `usage_rows`: `assert!(h.usage_written(<model posts>, Duration::from_secs(10)), "usage rows not written")` (the existing wait and bound of every other proxy test file, unchanged; lead's decision 2026-11-25), then `Store::open(<db>).requests_for_session(<id>)`. Doc: call it only when every `post` reached a model route.
  - `codex_request(name)`: `fixtures/codex/requests/<name>.json` from `CARGO_MANIFEST_DIR/../../`, its `body` value.
  - Check: step 2 compiles against it.

- [x] 2. **Failing tests.** Create `crates/zygo-proxy/tests/responses_route.rs` with `#[path = "support/responses.rs"] mod support;`, `const R: &str = "openai/v1/responses";`, and a helper `fn oai_err(b: &[u8]) -> Value` that parses the body, asserts it is an object with key `error` and no key `type`, and returns `error`. Tests (`A` = `Dialect::Anthropic`, `C` = `Dialect::OpenaiChat`, agent `Agent::ClaudeCode`, extra `""` unless stated):
  ```rust
  identity_missing_session_header_openai_shape   // A; post_as(R, [authorization Bearer token], first-turn) → 401, oai_err.type == "zygo_token_invalid"; upstream_bodies empty
  identity_wrong_bearer_or_session_openai_shape  // A; [Bearer "wrong-token", x-zygo-session id] → 401 "zygo_token_invalid"; [Bearer token, x-zygo-session "no-such-session"] → 401 "zygo_session_unknown"; upstream empty
  unknown_field_400_before_upstream              // A; first-turn + "rs3_field": 1 → 400, type "zygo_bad_request", message == "unknown field: rs3_field"; upstream empty
  refused_fields_400_before_upstream             // A; for f in REFUSED: first-turn + f: "x" → 400, message == format!("{f}: {NO_STATE}"); upstream empty
  proxy_error_after_identity_openai_shape        // A; first-turn with model "no-such-model" → 400, type "zygo_unknown_model"; upstream empty
  captured_requests_reach_anthropic_as_messages  // A; each of the 4 fixtures: status 400 (mock); last upstream path "/v1/messages"; body: model == UPSTREAM_MODEL, max_tokens == MAX_OUTPUT, messages non-empty array with messages[0].role == "user", no key "input", no key of IGNORED; last upstream headers: anthropic-version == "2023-06-01", no authorization, no x-zygo-*; reply x-zygo-adjustments (absent = empty) has no item "facts_missing" and none starting "blocks_dropped:"
  captured_requests_reach_chat_as_chat           // C; each fixture: last path "/v1/chat/completions"; body: model == UPSTREAM_MODEL, messages non-empty array, stream == true, no key "input", no key "system", stream_options == {"include_usage":true} (the chat translator's own field, `translate/chat.rs:81-83`; no fixture sends one; execution fix), no other key of IGNORED; authorization header present and != format!("Bearer {}", token); same adjustments check
  upstream_529_before_first_byte_streaming       // A; reply_with(529, "application/json", {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}); first-turn (stream true) → 529, content-type starts "application/json", oai_err == {"type":"overloaded_error","message":"Overloaded"}, body holds no b"event:" and no b"data:"
  over_long_prompt_anthropic_pc14                // A; reply 400 {"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 250001 tokens > 200000 maximum"}} → 400; oai_err keys == keys of context-400.json's error; type, param, code == the fixture's; message == "prompt is too long: 250001 tokens > 200000 maximum"; no SSE bytes
  over_long_prompt_chat_pc14                     // C; reply 400 {"error":{"message":"This model's maximum context length is 128000 tokens. However, your messages resulted in 130000 tokens.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}} → same checks, message == "prompt is too long: 130000 tokens > 128000 maximum"
  redaction_acts_on_input                        // A, extra "[redaction.content]\ndeny = [\"plimsoq\"]\n"; first-turn + input item {"type":"message","role":"user","content":[{"type":"input_text","text": format!("key {k} and plimsoq")}]}, k = ["sk-","ant-api03-",&"Ab1_".repeat(24)].concat(); the upstream body bytes hold neither k nor "plimsoq"; reply x-zygo-redacted == "1"
  attribution_row_for_responses_route            // A; first-turn with model "code" → upstream body model == UPSTREAM_MODEL; reply x-zygo-role == "code", x-zygo-model == MODEL; usage_rows() == one row with route "/openai/v1/responses", agent "claude-code", role_served Some("code"), model MODEL, provider PROVIDER, status 400
  ```
  Run `cargo test -p zygo-proxy --test responses_route`; pass = every test fails (the route answers `404 zygo_route_not_in_v1`). Commit `test(proxy): RS-3 Responses route tests and harness`.

- [x] 3. **Route and error shape.**
  - `pipeline.rs`: variant `ModelRoute::Responses`; `impl ModelRoute { pub fn dialect(self) -> Dialect }` (`Responses` → `Openai`, others → `Anthropic`).
  - `errors.rs`: `pub fn response(&self, d: Dialect) -> Response` = `(status, Json(self.body(d)))`.
  - `routes.rs`: `/openai/v1/responses` → `post(responses)`, `async fn responses(State(app): State<Arc<App>>, req: Request) -> Response` calling `model_route(app, ModelRoute::Responses, req)`; `model_route`'s `Err(e)` arm → `e.response(route.dialect())`.
  - `forward.rs`: `client_path` arm `"/openai/v1/responses"`; `ModelRoute::upstream_path` arm `"/v1/messages"` (its path on an `anthropic` upstream; `upstream_path(route, d)` keeps `/v1/responses` and `/v1/chat/completions` for the others); `handle`'s `Err(e)` arm → `e.response(route.dialect())`; `run`: `dialect: route.dialect()`, and for `ModelRoute::Responses` call `responses_client::check_fields(&body)?` right after the JSON parse, before the `model` read and the stages.
  - Check: the identity, field and `proxy_error_after_identity` tests pass. Commit `feat(proxy): RS-3 /openai/v1/responses route and OpenAI error shape`.

- [x] 4. **Candidates.** `resolve.rs` `upstream_dialect(p, client)`: the client's dialect when spoken; else for `Anthropic` as today; else for `Openai`: `Anthropic`, then `OpenaiChat` (Messages is the pivot; one pass before two, K-C2); `OpenaiChat` client → `None` as today. Update the doc line. Check: `cargo test -p zygo-proxy --test translate --test request_path` passes (the `anthropic` client order is unchanged). (Execution note: `wire_openai_routes_not_in_v1` fails from step 3 on, as the route is served; step 7 fixes it. No commit of its own: it goes with step 5.)

- [x] 5. **Request side.** In `forward.rs`:
  - `attempt_body`: when `client == Dialect::Openai && c.dialect != client`, return `responses_attempt(body, raw, c, rw, &rules)`.
  - `fn responses_attempt(body: &Value, raw: &Bytes, c: &Candidate, rw: &Rewrite, rules: &[SchemaRule]) -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError>`: `request(body, &Target::of(c), rw.facts)`; then on that Messages body the steps of today's translated branch (`forward.rs:791-796`): `facts::apply` (or `facts_missing`), `schema::rewrite`; for `OpenaiChat`: `translate_request(&m, OpenaiChat, &t)` and, with facts, `facts::write_effort(&m, &mut v, OpenaiChat)`; for `Anthropic` the Messages body is the upstream body; `model_body(&mut v, raw, true, c)`. Codes in the order `model_body`, `request`, facts, schema, translator. An `Openai` candidate never reaches it (same dialect: RS-5).
  - In the attempt loop, when `ctx.dialect == Openai && c.dialect == Anthropic` and the outbound map has no `anthropic-version`, insert `anthropic-version: 2023-06-01` (`const ANTHROPIC_VERSION`).
  - Check: `captured_requests_reach_*` and `redaction_acts_on_input` pass. Commit `feat(proxy): RS-3 Responses request side at forward time`.

- [x] 6. **Errors before the first byte.** In `forward.rs`:
  - `fn responses_error(status: StatusCode, body: &[u8], estimate: u64, key: Option<&str>) -> (StatusCode, Value)`: `translate_error(...)` gives `(s, m)`; when `s == 400` and `m.error.message` starts with `"prompt is too long"` → `(400, {"error":{"message":<that message>,"type":"invalid_request_error","param":"input","code":"context_length_exceeded"}})` (PC14); else `(s, {"error": m.error})`.
  - In `run`'s read-whole branch (`:513-527`) call it in place of `translate_error` when `ctx.dialect == Openai`.
  - Before the stream is built (after `:496`): `ctx.dialect == Openai && translated && status.is_success()` → `Err(read_failed(slot, c, &adjustments, upstream_bad_response()))` (removed by RS-4; no test claims it).
  - Check: `upstream_529_*`, `over_long_prompt_*`, `attribution_row_*` pass; all 12 pass. Commit `feat(proxy): RS-3 upstream errors before the first byte in the OpenAI shape`.

- [x] 7. **Existing test.** `tests/translate.rs` `wire_openai_routes_not_in_v1` (`:1384-1403`): remove the `("POST", "openai/v1/responses", …)` tuple from the loop (test-logic edit: the route is now served; `chat/completions` and `models` stay 404, the `GET openai/v1/responses` → 405 line stays). Check: `cargo test -p zygo-proxy --test translate` passes. Commit `test(proxy): RS-3 /openai/v1/responses is served`.

- [x] 8. **Acceptance and floor.** Run alone: `test "$(cargo test -q -p zygo-proxy --test responses_route -- --list 2>/dev/null | grep -c ': test$')" -ge 7 && cargo test -p zygo-proxy --locked` → exit 0 with 12 tests listed. Then `cargo test --workspace --locked` (CI form, `ci.yml:53`) passes. Push `phase-4--RS-3`.

### Author's checks (2026-11-25)

1. Spec coverage: K§6.4.1 identity and error shape → step 3, tests 1-2, 5; K§6.4.2 identity, resolution (role `code`), redaction, attribution → steps 3-4, tests 11-12; K§6.4.3 request side, field lists at the route (K§11 item 12) → steps 3, 5, tests 3-4, 6-7; K§6.4.4 and K§11 item 14 first half → step 6, tests 8-10; K§6.7 Responses requests (body limit and depth: `read_charged`/`key_scan` unchanged, fail-closed fields: step 3); K§11 item 20 → test 11; K-D47 (any session; Claude Code tests) → harness; K-D50 → step 6.
2. Step scan: each code step names its signature, file and fixed values; no step carries a body the tests decide, except the PC14 rule, which the spec leaves to the shape PC14 records.
3. Interface match: `check_fields`, `request` as RS-1 produced them (`request` takes `rw.facts`, `forward.rs:403`). RS-4 and RS-5 consume `Harness` and `ModelRoute::Responses`; RS-8 consumes the harness `get`. RS-2 consumes nothing from here.
4. Review focus 10 (graph) → tests 8-10.
5. Proportion: signatures, constants, TOML and assertions only.
6. Live code: every path and line above was opened at fe18f3dd; no existing function maps an upstream error to the OpenAI shape (`translate_error` gives Messages; `not_in_v1`, `routes.rs:65-68`, is a fixed reply).

## Review round 1 findings

Review of PR #157 at 75dc7869; fixed in round 2 (tests first: faa79017).

- **F1 (important).** Anthropic's `` input length and `max_tokens` exceed context limit: <a> + <b> > <c> `` text did not get the PC14 shape. Fix: `translate::context_error` recognises it (`n` = a + b, `max` = c), so `translate_error` gives `prompt is too long: …` and `forward::responses_error` (prefix `prompt is too long`, kept) gives the PC14 shape. Anthropic's own `prompt is too long` text passes through `translate_error` unchanged and keeps matching the prefix. Tests: `responses_route::over_long_prompt_anthropic_limit_text_pc14`; `translate::res_json_context_errors` (new row) and `translate::wire_context_error_anthropic_limit_text` (Messages route, translated candidate). Spec: K§6.4.4, K§12. Superseded by G1 (review round 2): `context_error` is back to its base form; the limit text is mapped in `forward::responses_error` only.
- **F2 (important).** A Responses client to an `openai` provider went through raw (`store` as sent) with no usage. Fix: `resolve::upstream_dialect` for an `openai` client: `anthropic`, then `openai-chat`; never `openai` (lead's decision, Planning notes). Test: `openai_only_provider_is_no_candidate` (400 `zygo_no_provider`, OpenAI shape, no upstream request). Spec: K§6.4.3.
- **F3 (nit).** The harness provider has `key = "keychain:k-rs"` for every dialect; `Harness::provider_key()` (added to Produces) gives its value. `captured_requests_reach_anthropic_as_messages` asserts `x-api-key` == the key and no `authorization`; `captured_requests_reach_chat_as_chat` asserts `authorization` == `Bearer <key>`, no `x-api-key`, no `anthropic-version`.
- **F4 (nit).** Tests `client_anthropic_version_kept` (the client's one value only) and `chat_candidate_gets_no_anthropic_version` (with and without a client value).
- **F5 (nit).** Tests `body_over_limit_openai_shape` (413 `zygo_body_too_large`), `non_object_body_openai_shape` (400 `zygo_bad_request`), `anthropic_route_errors_keep_anthropic_shape` (401 and 400 with top-level `type: "error"`).

## Review round 2 findings

Review at 1516c7ff; fixed in round 3 (tests first: 8283fffb).

- **G1 (important).** F1's `context_error` change rewrote Anthropic's `max_tokens` limit text for every translated client, which breaks Claude Code's retry with a smaller `max_tokens` (it parses that exact text). Fix: `translate/mod.rs` restored to the base (identical to origin/phase-4); `forward::responses_error` maps the full form to the PC14 shape (type `invalid_request_error`, param `input`, code `context_length_exceeded`) for the Responses client only. Tests: `translate::res_json_anthropic_limit_text_unchanged`, `translate::wire_anthropic_limit_text_unchanged_from_chat` (Messages route, chat upstream after a 503 fallover: the exact text, and the substring Claude Code's pattern reads), `translate::res_sse_in_stream_anthropic_limit_text_unchanged` (inside a chat stream: `api_error` with the text unchanged); `responses_route::over_long_prompt_anthropic_limit_text_pc14`. `wire_context_error_anthropic_limit_text` and the F1 row of `res_json_context_errors` are removed. Spec: K§6.4.4, K§12, change log.
- **G2 (nit).** `forward::anthropic_limit_form` matches only the full form (`… exceed context limit: \d+ \+ \d+ > \d+`). Test: `responses_route::bare_limit_phrase_is_ordinary_error` ("the tool said: we exceed context limit sometimes" passes as an ordinary 400 with no `param`/`code`).
- **G3 (nit).** No token count is made up: the message is the upstream text, `forward::pc14_message`: each Cc, Cf, Zl, Zp character replaced with `?`, at most 512 characters (lead's decision after round 3: the 64-character echo cut ended inside the second number). `over_long_prompt_anthropic_limit_text_pc14` asserts the full message. Spec: K§6.4.4 (covered by the round 3 change-log line).
