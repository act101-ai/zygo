# RS-8 — `GET /openai/v1/models` in the PC7 shape, other PC1 paths, chat route stays 404

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.1 (second and third items), K§3 (the chat/completions route stays 404), K§11 item 22
**Files:** `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/src/models.rs`, `crates/zygo-proxy/tests/responses_models.rs` (new), `crates/zygo-proxy/tests/translate.rs` (planning: `wire_openai_routes_not_in_v1` claims `GET /openai/v1/models` is 404), `crates/zygo-proxy/tests/support/responses.rs` (planning: `Harness::get_as` for the identity test), `docs/specs/2026-11-25-client-model-design.md` (lead: K§6.4.1 text on `base_instructions` and the change log)
**Consumes:** `Harness::start`, `Harness::get`, `Harness::post`, `Harness::post_as`, `Harness::upstream_paths`, `support::codex_request` (RS-3), `fixtures/codex/models/catalog-shape.json` and `request.json` (PC-3, PC7), `fixtures/codex/transport/paths.txt` (PC-3, PC1)
**Produces:** `models::list_openai(s: &SessionState) -> Value` (the session's role list in the shape PC7 records); `Harness::get_as(&self, path: &str, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Bytes)` (added in planning)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_models -- --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-proxy --locked` → at least 3 tests in `responses_models` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: `GET /openai/v1/models` gives the role list in the PC7 shape; `POST /openai/v1/chat/completions` stays `404 zygo_route_not_in_v1`; each other `/openai/v1/` path that PC1 records is served when K§6.4 gives its function, else `404` in the OpenAI error shape (K§11 item 22)

## Planning notes

- `models::list(s, cfg, d)` (`models.rs:21`) gives the Anthropic list shape for any dialect today; the route at `routes.rs:50` answers `not_in_v1`. The identity rule of the Anthropic models route (`routes.rs:273-277`) applies.
- Planning (author's call, 2026-11-26): `list_openai` takes no `cfg`. The list is roles and custom roles only (K§6.4.1 "the session's role list"; K§11 item 22 "the role list"; registry A-3.1 "The picker shows the session's role list", K§6.2.2). Catalog ids are left out: `Catalog::view` (`zygo-config/src/schema.rs:86-100`) records the client dialects a model serves only for an Anthropic client, so a `Dialect::Openai` filter would list the models with an `openai` provider, which RS-3 made no candidate of a Responses client (RS-3 Planning notes, lead's decision F2). No node consumes `list_openai`.
- Planning (author's call): entry values. Keys = the keys of `fixtures/codex/models/catalog-shape.json` `models[0]` (the shape Codex 0.172.0 listed, `models/result.txt` `b: … listed=yes`; the OpenAI `{"object":"list","data":[…]}` shape is not listed, `c: … listed=no`). `supported_reasoning_levels: []` and `default_reasoning_level: null` as in `models/catalog-patch.json`, which 0.172.0 accepted (PC-3 T4, `nodes/PC-3.md:165`); `priority: 1` for every entry, as served in run `b`, where the picker kept the served order (`models/picker-b.jsonl`: `gpt-mock`, `gpt-mock-listed`); the other fixed values as `catalog-shape.json`.
- **Lead's decision (2026-11-26):** `base_instructions: ""` and no `apply_patch_tool_type` are accepted, on one condition. Codex reads `base_instructions` as a replacement for its own base prompt, so `""` from `/models` would blank Codex's prompt. So zygo's Codex launch must keep writing the catalog file (K§6.3.1), which makes Codex send no `/models` request (PC-3 list d: `models_request=0`). K§6.4.1 now states this as final text, with one change-log line.
- P4-9 (Codex launch) must keep the catalog file; the `/models` route is not a substitute.
- (The question as the planner put it.) **Lead to decide** (Codex client policy, K§6.4.2 / P4-9): (a) `base_instructions` is a required string; Codex sends a catalog entry's `base_instructions` in place of its own base prompt (PC-3 G2, `nodes/PC-3.md:113-119`, measured for the catalog file, not for `/models`). The plan serves `""`, because the proxy holds no client's base prompt and routes by dialect, not by client (K-D47). (b) No `apply_patch_tool_type`: without it Codex offers no `apply_patch` tool (PC-3 F1). Both matter only when Codex reads `/models`; with the K§6.3.1 catalog file Codex sends no `/models` request (`models/result.txt` `d: models_request=0`). If the lead wants other values, step 4's constants and step 2's expected entry change together.
- PC1 records two paths (`fixtures/codex/transport/paths.txt`: `GET /v1/models`, `POST /v1/responses`); both have their function in K§6.4.1. So "each other path" is served, and every other `/openai/v1/` path gets `404 zygo_not_found` in the OpenAI shape (K§6.4.1: errors on these routes use the OpenAI shape). Today the fallbacks answer in the Anthropic shape (`routes.rs:60-62`, `:70-72` → `IntoResponse for ZygoError`, `errors.rs:53-56`).

## Plan

Base: `origin/phase-4` at f37bba43 (RS-3 merged). Branch `phase-4--RS-8`. Every commit ends with the lines `Zygo-Session: <session>` and `Zygo-Work: phase4:RS-8`; no attribution. Builds use `CARGO_INCREMENTAL=0`. Scratch files use the prefix `rs8p-`.

### Premise check (2026-11-26, against f37bba43)

- RS-3 landed: `Harness::start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness` (`tests/support/responses.rs:136`), `get(&self, path: &str)` (`:232`, sends `authorization: Bearer` and `x-zygo-session`), `post` (`:221`), `post_as` (`:228`), `upstream_paths` (`:242`), `codex_request` (`:268`), private `send(&self, method, path, headers, body: Option<&Value>)` (`:199`). The harness session is a Claude Code session with no profile (`:172-181`); the config is the built-in layer plus `[role.code]` → `gpt-mock` and `extra_toml` (`:144-153`). The default mock reply is `400` (`:138-143`).
- Router (`src/routes.rs:33-54`): `/openai/v1/responses` → `responses` (`:48`, RS-3); `/openai/v1/chat/completions` and `/openai/v1/models` → `not_in_v1` (`:49-50`, `:65-68`, OpenAI shape, no identity check); `.fallback(not_found)` (`:51`, `:60-62`) and `.method_not_allowed_fallback(method_not_allowed)` (`:52`, `:70-72`) return `ZygoError`, whose `IntoResponse` is the Anthropic shape (`errors.rs:53-56`). `ZygoError::response(&self, d: Dialect) -> Response` (`errors.rs:48`) gives `{"error":{"type":code,"message":m}}` for `Dialect::Openai` (`:37-43`).
- Anthropic models route (`routes.rs:273-277`): `model_identity(&h, &app.sessions)?` (`identity.rs:38`), `app.sessions.accept(&s)?` (`session.rs:593`), `models::list(&s, &app.served().cfg, Dialect::Anthropic)`.
- `models::list` (`models.rs:21-41`): roles of `s.config.roles` (`BTreeMap<String, Role>`, `session.rs:159`; disabled roles already removed at `session.rs:485-489`) with the profile annotation, then `s.config.custom_roles`, then catalog ids.
- Built-in roles: `zygo_config::builtin::defaults()` (`zygo-config/src/builtin.rs:23`), field `role: BTreeMap<String, Role>` (`schema.rs:18`); `check` refuses only a role whose model is unknown (`check.rs:99-101`), so an extra `[role.rs8p]` with `model = "gpt-mock"` loads.
- **Drift 1 (blocks the node).** `tests/translate.rs:1427-1445` (`wire_openai_routes_not_in_v1`) asserts `GET /openai/v1/models` → `404 zygo_route_not_in_v1` (tuple at `:1432`). The Acceptance runs every `zygo-proxy` target. `tests/translate.rs` is added to Files; the one tuple is removed (step 6). Its lines `:1438-1441` read only `["error"]["type"]` and stay true after step 5.
- **Drift 2.** The harness has no GET with chosen headers; the identity test needs one. `get_as` is added to `tests/support/responses.rs` (step 1); RS-4 and RS-5 also list this file.
- **Drift 3 (contract).** `list_openai` drops `cfg` (Planning notes). PC1 records no path besides the two served ones (Planning notes).
- No wall-clock: the harness runs on `ManualClock`; no test here waits on time or on usage rows.

### Interfaces

Consumed (as in the code now):

```rust
// tests/support/responses.rs (RS-3)
pub fn start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness;
pub fn get(&self, path: &str) -> (StatusCode, HeaderMap, Bytes);
pub fn post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes);
pub fn post_as(&self, path: &str, headers: &[(&str, &str)], body: &Value) -> (StatusCode, HeaderMap, Bytes);
pub fn upstream_paths(&self) -> Vec<String>;
pub fn token(&self) -> &str;
pub fn session_id(&self) -> &str;
pub fn codex_request(name: &str) -> Value;
// src
pub(crate) fn model_identity(h: &HeaderMap, sessions: &Sessions) -> Result<Arc<SessionState>, ZygoError>; // identity.rs:38
pub(crate) fn accept(&self, s: &Arc<SessionState>) -> Result<(), ZygoError>;                               // session.rs:593
pub fn response(&self, d: Dialect) -> Response;                                                            // errors.rs:48
pub fn defaults() -> Config;                                                                               // zygo_config::builtin
```

Produced:

```rust
// src/models.rs
pub(crate) fn list_openai(s: &SessionState) -> Value;
// tests/support/responses.rs
pub fn get_as(&self, path: &str, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Bytes);
```

### Steps

- [x] 1. `tests/support/responses.rs`: add after `get` (`:232-235`):
  `/// GET with exactly `headers`; counts nothing.` `pub fn get_as(&self, path: &str, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Bytes)` = `self.send("GET", path, headers, None)`.

- [x] 2. Write `tests/responses_models.rs` (new). Header doc comment: `//! RS-8: GET /openai/v1/models in the PC7 shape, the other /openai/v1/ paths, chat/completions stays 404 (K§6.4.1, K§3, K§11 item 22). Mock upstream on 127.0.0.1; no agent binary, no network.` Then `#[path = "support/responses.rs"] mod support;`. Helpers: `fn fixture(rel: &str) -> Vec<u8>` reads `CARGO_MANIFEST_DIR/../../fixtures/codex/<rel>` (panics with the path); `fn json_of(b: &[u8]) -> Value`. Constants: `const M: &str = "openai/v1/models";`, `const OAI_JSON: &str = "application/json";`. Tests (all `Harness::start(Dialect::Anthropic, Agent::ClaudeCode, …)`):

  `models_list_is_the_role_list_in_the_pc7_shape` — `extra_toml` = `"[role.rs8p]\nmodel = \"gpt-mock\"\ndescription = \"rs8p role\"\n"`. The path is the one Codex sent: `request.json` `path` with its leading `/v1/` replaced by `openai/v1/` (assert it equals `"openai/v1/models?client_version=0.172.0"`).
  ```rust
  let (s, hd, b) = h.get(&path);
  assert_eq!(s, 200);
  assert!(hd["content-type"].to_str().unwrap().starts_with(OAI_JSON));
  let v = json_of(&b);
  assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["models"]);
  let mut want: Vec<String> = zygo_config::builtin::defaults().role.into_keys().collect();
  want.push("rs8p".into());
  want.sort();
  let models = v["models"].as_array().unwrap();
  assert_eq!(models.iter().map(|m| m["slug"].as_str().unwrap()).collect::<Vec<_>>(), want);
  assert!(models.iter().all(|m| m["slug"] != MODEL)); // no catalog id
  let shape = json_of(&fixture("models/catalog-shape.json"));
  let keys = |m: &Value| m.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>();
  for m in models { assert_eq!(keys(m), keys(&shape["models"][0]), "{m}"); }
  assert_eq!(models.iter().find(|m| m["slug"] == "rs8p").unwrap(), &json!({
      "slug": "rs8p", "display_name": "rs8p", "description": "rs8p role",
      "supported_reasoning_levels": [], "default_reasoning_level": null,
      "shell_type": "shell_command", "visibility": "list", "supported_in_api": true, "priority": 1,
      "support_verbosity": false, "truncation_policy": {"mode": "bytes", "limit": 10000},
      "experimental_supported_tools": [], "base_instructions": ""
  }));
  assert!(h.upstream_paths().is_empty());
  ```

  `models_identity_errors_openai_shape` — with `bearer = format!("Bearer {}", h.token())`, each `(headers, status, type)`: `[]` → `(401, "zygo_token_invalid")`; `[("authorization", &bearer)]` → `(401, "zygo_token_invalid")`; `[("authorization", "Bearer wrong-token"), ("x-zygo-session", h.session_id())]` → `(401, "zygo_token_invalid")`; `[("authorization", &bearer), ("x-zygo-session", "no-such-session")]` → `(401, "zygo_session_unknown")`. For each, `h.get_as(M, headers)`: the status, and the body has exactly one top-level key `error` whose `type` is the expected code (the OpenAI shape, `errors.rs:37-43`).

  `chat_completions_stays_not_in_v1` — `want = json!({"error":{"type":"zygo_route_not_in_v1","message":"this route is not in zygo v1"}})`. `h.post("openai/v1/chat/completions", &json!({}))` and `h.post_as("openai/v1/chat/completions", &[], &json!({}))`: each `(404, want)`. `h.upstream_paths()` is empty.

  `other_openai_paths_served_or_404_openai_shape` —
  ```rust
  // PC1: the paths Codex calls, without the query.
  let txt = String::from_utf8(fixture("transport/paths.txt")).unwrap();
  let pc1: BTreeSet<(String, String)> = txt.lines().filter(|l| !l.trim().is_empty())
      .map(|l| { let f: Vec<&str> = l.split_whitespace().collect(); (f[1].to_string(), f[2].to_string()) }).collect();
  assert_eq!(pc1, BTreeSet::from([("GET".into(), "/v1/models".into()), ("POST".into(), "/v1/responses".into())]));
  ```
  Both are served: `h.get(M)` → 200; `h.post("openai/v1/responses", &codex_request("first-turn"))` → 400 (the mock's default reply) and `h.upstream_paths().len() == 1`. Then each of `("POST", "openai/v1/embeddings")`, `("GET", "openai/v1/files")`, `("POST", "openai/v1/rs8p/x")` (POST via `post_as(p, &[], &json!({}))`, GET via `get_as(p, &[])`) → `(404, json!({"error":{"type":"zygo_not_found","message":"no such route"}}))`; `get_as("openai/v1/responses", &[])` → `(405, json!({"error":{"type":"zygo_method_not_allowed","message":"method not allowed"}}))`; and the Anthropic side keeps its shape: `get_as("anthropic/v1/rs8p", &[])` → `(404, json!({"type":"error","error":{"type":"zygo_not_found","message":"no such route"}}))`. `h.upstream_paths().len()` is still 1.

- [x] 3. Run `CARGO_INCREMENTAL=0 cargo test -p zygo-proxy --test responses_models 2>&1`. Pass means it fails as expected: the first test (404 `zygo_route_not_in_v1` for `GET /openai/v1/models`), the second (404 instead of 401), the fourth (models 404; unknown paths in the Anthropic shape). `chat_completions_stays_not_in_v1` already passes (it guards K§3).

- [x] 4. `src/models.rs`:
  - Extract from `list` the role part as `fn roles(s: &SessionState) -> Vec<(String, String)>` (name, description): `s.config.roles` with the profile annotation as now (`:23-28`), then `s.config.custom_roles`. `list` builds its role entries from it, then the catalog ids as now; its output does not change.
  - Add `pub(crate) fn list_openai(s: &SessionState) -> Value` with doc `/// The session's roles and custom roles in the model-catalog shape Codex lists (K§6.4.1; PC7, fixtures/codex/models/catalog-shape.json); no catalog ids.` It returns `{"models": [entry…]}`, one entry per `roles(s)` item, in that order, with the keys and values of step 2's expected entry (`slug` and `display_name` = the name, `description` = the description; the rest fixed). Put the fixed values in one `fn catalog_entry(slug: &str, description: &str) -> Value`; `base_instructions` is the constant `BASE_INSTRUCTIONS: &str = ""` with a doc line citing the lead item in Planning notes.

- [x] 5. `src/routes.rs`:
  - `:50` → `.route("/openai/v1/models", get(openai_models))`. Update `not_in_v1`'s doc (`:64`) to `/// The v2 route `POST /openai/v1/chat/completions` (§7.6, K§3), with no identity check.`
  - New handler after `models` (`:273-277`): `async fn openai_models(State(app): State<Arc<App>>, h: HeaderMap) -> Response` — `model_identity(&h, &app.sessions)`, then `app.sessions.accept(&s)`; on `Ok` → `Json(models::list_openai(&s)).into_response()`; on `Err(e)` → `e.response(Dialect::Openai)`.
  - Fallback shape by path: `fn path_dialect(uri: &Uri) -> Dialect` = `Dialect::Openai` when `uri.path().starts_with("/openai/v1/")`, else `Dialect::Anthropic`. `not_found(uri: Uri) -> Response` and `method_not_allowed(uri: Uri) -> Response` keep their codes and messages and answer `e.response(path_dialect(&uri))`. Import `http::Uri`.

- [x] 6. `tests/translate.rs:1432`: remove the tuple `("GET", "openai/v1/models", None)`, so the loop covers only `POST /openai/v1/chat/completions` (a listed test-logic edit: the route it claimed 404 is now served, K§6.4.1). (Implementation: with one tuple left, clippy `single_element_loop` fails `-D warnings`, so the inner loop is unrolled into one `send` of `POST openai/v1/chat/completions` with the same two assertions.)

- [x] 7. Run `CARGO_INCREMENTAL=0 cargo test -p zygo-proxy --test responses_models 2>&1` → 4 passed, 0 failed. Run `CARGO_INCREMENTAL=0 cargo test -p zygo-proxy --test translate --test request_path --test session_api --test passthrough --test serve 2>&1` → 0 failed (the Anthropic models list and the non-OpenAI fallbacks are unchanged).

- [x] 8. Acceptance, alone: `test "$(cargo test -q -p zygo-proxy --test responses_models -- --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-proxy --locked` → exit 0. Then `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings` → clean; the floor `cargo test --workspace --locked --no-fail-fast` → 0 failed except the 2 known `guard_hook` environment failures under `/private/tmp`. (Correction: no `cargo fmt --check` gate; the repo is not rustfmt-clean and CI runs none.)

- [x] 9. Commit `feat(proxy): RS-8 /openai/v1/models in the PC7 shape, OpenAI-shaped fallbacks under /openai/v1/` (steps 1-6 and the ticked boxes), trailers as above. Push `phase-4--RS-8`.

### Author's checks (2026-11-26)

1. Spec coverage: K§6.4.1 item 2 → steps 2 (test 1), 4, 5; item 3 (PC1 paths served, else 404 OpenAI shape) → step 2 (test 4), step 5; identity and error shape on the new route (K§6.4.1 stage 1) → test 2; K§3 / K§11 item 22 chat route → test 3, step 6 keeps `translate.rs` claim for it.
2. Step scan: each step names one file and its exact change; the only bodies given are the test assertions and `path_dialect`'s one condition.
3. Interface match: RS-3's harness signatures copied from `tests/support/responses.rs`; `list_openai` has no consumer in the graph; `get_as` is additive (RS-4, RS-5 also edit the harness).
4. Review focus: the graph's review focus assigns no line to RS-8.
5. Proportion: the code is about 60 lines; the plan gives signatures, values and assertions, no handler bodies beyond one-line forms.
6. Live code: every path and line above was opened at f37bba43; no existing function gives the catalog shape (`grep -rn '"slug"' crates/` finds none); `models::list` is reused through `roles`.
