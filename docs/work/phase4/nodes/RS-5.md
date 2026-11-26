# RS-5 — Responses → Responses passthrough, the stream form seen by the tee, and the `response.completed` usage parse

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stream-order table row Responses; "The tee and the stream order"; cache row: no markers toward a Responses upstream), K§6.4.3 (Responses client → Responses upstream), K§11 items 17 and 19 (the Responses → Responses half); K-D21, K-D48
**Files:** `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/pipeline.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/src/resolve.rs`, `crates/zygo-proxy/tests/support/responses.rs`, `crates/zygo-proxy/tests/responses_passthrough.rs` (new), `crates/zygo-proxy/tests/responses_route.rs`
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3)
**Produces:** `ResponseCtx.lines: Dialect` (the dialect of the SSE lines the tee and the response stages see: `Openai` only for a Responses upstream answering a Responses client with no mapper; `Anthropic` for an Anthropic upstream and for every mapped stream), `tee::responses_usage(line: &[u8]) -> Option<Value>` (the `response.usage` object of a `response.completed` or `response.incomplete` line)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_passthrough -- --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo-proxy --locked` → at least 6 tests in `responses_passthrough` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: the upstream body equals the client body except `model`, `store`, `include` and stage edits; the upstream body has `store` == `false` and an `include` array that contains `reasoning.encrypted_content`, also when the client sent `store: true` and no `include`; reasoning items are byte-equal; the usage row has the non-zero token counts of the upstream `response.completed`; a non-streaming request records the counts of the JSON body's `usage` (K§11 item 17); the upstream body holds no `cache_control` anywhere (K§11 item 19, second half)

## Planning notes

- How the tee tells the forms apart (author): `ResponseCtx.dialect` (`pipeline.rs:80-81`) is the upstream dialect, which does not say whether a mapper ran (a chat or Responses upstream answering a Messages client is mapped to Messages events, `forward.rs:1098-1110`). The forward step knows whether it built a mapper for the attempt, so it sets `lines` and gives it to the tee; the tee parses `message_start`/`message_delta` usage (`tee.rs:71-83`, `line_usage`) for `Anthropic` lines and `response.completed` usage for `Openai` lines. The Messages parse is unchanged.
- Same-dialect passthrough keeps the body bytes where no field changes (the `changed` rule of `RequestCtx`, `pipeline.rs:47-48`).

## Plan

Base: `origin/phase-4` at da0c0387 (RS-4 merged, #165). Every path, line and signature below was opened at that commit.

**Check:** `test "$(cargo test -q -p zygo-proxy --test responses_passthrough -- --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo-proxy --locked` (the Acceptance command).

### Premise check (2026-11-25)

- Same-dialect request path exists but is unused: `attempt_body` (`forward.rs:872-925`) sends a Responses body toward an `openai` candidate through the Messages-client tail (`reasoning::without_zygo_thinking`, which reads only `messages`, `translate/reasoning.rs:94-95`; `schema::rewrite`, which reads only `tools[].input_schema` and `output_config`, `translate/schema.rs:145-173`), then `model_body` (`:964-999`). Neither edits a Responses body, and nothing sets `store` or `include`. RS-5 adds a branch before them.
- RS-3 disabled the candidate in `resolve.rs:85` (`Dialect::Openai => &[Dialect::Anthropic, Dialect::OpenaiChat]`), not in `forward.rs`. `resolve.rs` was missing from Files; added. `tests/responses_route.rs:274-280` `openai_only_provider_is_no_candidate` asserts the RS-3 interim refusal and fails once the candidate returns; `tests/responses_route.rs` added to Files (the test is deleted, its replacement is test 8 below).
- Response side of the same-dialect pair: `translated = c.dialect != ctx.dialect` (`forward.rs:509`) is false, so the upstream body goes `Box::pin(resp.bytes_stream())` (`:530-531`) with no mapper, no restore (`armed` is false: `:518-520`) and no encoder (`:574`). The tee is unarmed (`ctx.line_mode` is set only on the Messages route, `window.rs:130`), but it still scans each whole line for usage (`tee.rs:183-185`, `:261-266`) with the Messages parser `line_usage` (`tee.rs:71-83`). A non-streamed body is read at `finish` as `usage` with Messages field names (`tee.rs:194-201`). So today the pair would record zero cache reads and the cached count inside input, or nothing on a stream.
- `ResponseCtx.dialect` is now `pipeline.rs:93` (the planning note's `:80-81` drifted). `Usage::observe` (`tee.rs:128-132`) has one caller, `forward.rs:1455`. `Tee::new(streamed: bool)` has callers in `tests/session_api.rs` and `tests/request_path.rs` (not in Files), so its signature stays and the form is set by a builder.
- The translated-path count rule already exists: `translate::usage_of(u: Option<&Value>, from: Dialect) -> Value` (`translate/mod.rs:151-159`, `pub(crate)`) turns a Responses `usage` into Messages usage (`input_tokens` less `cached_tokens`, `cache_read_input_tokens` = cached, `cache_creation_input_tokens` = 0). The tee reuses it.
- Cache markers: `CacheStage` returns early unless `route == Messages` (`cache.rs:35`), so no marker reaches a Responses body today; test 4 pins it. OpenCode's default is `insert_markers.opencode = true` (`zygo-config/src/schema.rs:468`).
- RS-9 (namespace tools): flattening applies only on the Messages pivot. Request obligation 6 is listed under "Request obligations (Responses → Messages)" (K§6.4.3), and the Responses → Responses bullet sends the body "as received" except `model`, `store`, `include` and the stage edits; the response stream is not encoded, so a namespaced call returns natively. Spec text made explicit (below); test 1 pins an unflattened `namespace` tool.
- Spec rewritten as final text (change-log line 2026-11-25): K§6.4.2 C3 (the tee also reads `response.incomplete` usage, which carries the full counts of a `max_output_tokens` or `content_filter` ending and is read by the existing mapper too, `translate/sse.rs:270-276`; the count rule; the forward step names the form; stale citations), K§6.4.3 Responses → Responses (`include` rule, including another JSON type → 400; `namespace` tools unflattened) and candidates of a Responses client (`openai` first).
- File overlap with open nodes: RS-6 (`forward.rs`) and RS-7 (`tee.rs`) already depend on RS-5. RS-10, RS-11, RS-12, PC-4, PC-7, PC-11 share no file with this Files list. No edge added.

### Interfaces

Consumes (as in the code now):

- `ModelRoute::Responses` (`pipeline.rs:33`); `ModelRoute::dialect(self) -> Dialect` → `Dialect::Openai` (`pipeline.rs:38-43`).
- `Harness::start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness` (`tests/support/responses.rs:198`); `Harness::start_with_profile(upstream: Dialect, agent: Agent, extra_toml: &str, profile: Option<Profile>) -> Harness` (`:203`); `post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes)` (`:308`); `reply_with(&self, status: u16, content_type: &str, body: Vec<u8>)` (`:343`); `upstream_bodies(&self) -> Vec<Value>` (`:330`); `upstream_paths(&self) -> Vec<String>` (`:334`); `usage_rows(&self) -> Vec<RequestRow>` (`:370`); `codex_request(name: &str) -> Value` (`:379`); `MODEL = "gpt-mock"`, `UPSTREAM_MODEL = "up-rs"`, `PROVIDER = "p-rs"`.
- `fn attempt_body(body: &mut Value, raw: &Bytes, changed: bool, c: &Candidate, client: Dialect, rw: &Rewrite) -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError>` (`forward.rs:872`); `fn model_body(body: &mut Value, raw: &Bytes, changed: bool, c: &Candidate) -> Result<(Bytes, Vec<Adjustment>), ZygoError>` (`forward.rs:964`).
- `pub(crate) fn usage_of(u: Option<&Value>, from: Dialect) -> Value` (`translate/mod.rs:151`).
- `pub(crate) fn upstream_dialect(p: &Provider, client: Dialect) -> Option<Dialect>` (`resolve.rs:82`).
- `fixtures/codex/requests/turn-with-reasoning.json` (read only): `store: false`, `include: ["reasoning.encrypted_content"]`, `stream: true`, `client_metadata`, `prompt_cache_key`, two `reasoning` items (`rs_pc3_1`, `rs_pc3_2`, `content: null`).

Produces:

- `ResponseCtx.lines: Dialect` (`pipeline.rs`, after `dialect`; also in its `Debug`): `Dialect::Openai` only when the upstream is `openai` and the client is `openai` (not translated); `Dialect::Anthropic` for every other pair. RS-7 reads it in the window stage.
- `pub fn responses_usage(line: &[u8]) -> Option<Value>` (`tee.rs`): the `response.usage` object of one SSE `data:` line whose `type` is `response.completed` or `response.incomplete`; `None` for every other line, and when `usage` is absent, `null` or not an object.
- `pub fn with_lines(mut self, lines: Dialect) -> Tee` (`tee.rs`): the form of the body the tee reads (SSE lines, or the whole non-streamed JSON); `Tee::new` keeps `Dialect::Anthropic`.
- `Usage::observe(&mut self, lines: Dialect, line: &[u8])` (`tee.rs`, stays `pub(crate)`).
- Test support: `Harness::start_dialects(dialects: &[Dialect], agent: Agent, extra_toml: &str) -> Harness` and `Harness::upstream_raw(&self) -> Vec<Vec<u8>>`.

### Steps

- [x] 1. **Harness.** In `crates/zygo-proxy/tests/support/responses.rs`:
  - Move the body of `start_with_profile` into `fn start_inner(dialects: &[Dialect], agent: Agent, extra_toml: &str, profile: Option<Profile>) -> Harness`. The TOML line is `dialects = [<each name quoted, joined with ", ">]`; the mock's first error body is the Anthropic form when `dialects[0] == Dialect::Anthropic`, else the OpenAI form (as now). `start_with_profile(upstream, …)` calls `start_inner(&[upstream], …)`.
  - `pub fn start_dialects(dialects: &[Dialect], agent: Agent, extra_toml: &str) -> Harness` calls `start_inner(dialects, agent, extra_toml, None)`.
  - `pub fn upstream_raw(&self) -> Vec<Vec<u8>>`: each recorded request body as received (`got[i].2`).
  - Check: `cargo test -p zygo-proxy --test responses_route --test responses_forward --locked` passes (no behaviour change).

- [x] 2. **Failing tests.** Write `crates/zygo-proxy/tests/responses_passthrough.rs`: `#[path = "support/responses.rs"] mod support;`, `const R: &str = "openai/v1/responses";`, `const M: &str = "anthropic/v1/messages";`, `SSE = "text/event-stream"`, `JSON = "application/json"`. Each harness is `Harness::start(Dialect::Openai, Agent::ClaudeCode, "")` unless named otherwise.

  Helpers (no logic under test): `counts(r: &RequestRow) -> (u64, u64, u64, u64)` = `(input_tokens, cache_read_tokens, cache_write_tokens, output_tokens)`; `oai_err(b) -> Value` copied from `tests/responses_route.rs:24-30`; `sse(evs: &[Value]) -> Vec<u8>` = `event: <type>\ndata: <compact json>\n\n` per event; `contains(hay: &[u8], needle: &[u8]) -> bool` by `windows`.

  Fixtures in the file:
  - `usage_up() -> Value` = `{"input_tokens":120,"input_tokens_details":{"cached_tokens":20},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":150}`.
  - `resp_events(terminal: &str, status: &str, usage: Value) -> Vec<Value>`, `response` object `R0 = {"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null}`: `response.created {sequence_number:0, response:R0}`; `response.output_item.added {1, output_index:0, item:{type:"message",id:"msg_up",role:"assistant",status:"in_progress",content:[]}}`; `response.content_part.added {2, output_index:0, content_index:0, item_id:"msg_up", part:{type:"output_text",text:""}}`; `response.output_text.delta {3, output_index:0, content_index:0, item_id:"msg_up", delta:"hi"}`; `response.content_part.done {4, …, part:{type:"output_text",text:"hi"}}`; `response.output_item.done {5, output_index:0, item:{type:"message",id:"msg_up",role:"assistant",status:"completed",content:[{type:"output_text",text:"hi"}]}}`; `<terminal> {6, response: R0 with status:<status>, output:[that message item], usage:<usage>}` (for `response.incomplete` also `incomplete_details:{reason:"max_output_tokens"}`).
  - `resp_json() -> Value` = `{"id":"resp_up","object":"response","model":"up-rs","status":"completed","output":[the message item],"usage":{"input_tokens":50,"input_tokens_details":{"cached_tokens":10},"output_tokens":5,"total_tokens":55}}`.
  - `ns_tool() -> Value` = `{"type":"namespace","name":"multi_agent_v1","description":"agents","tools":[{"type":"function","name":"spawn_agent","description":"spawn","parameters":{"type":"object","properties":{}}}]}`.

  Tests:
  1. `body_passes_except_model_store_include` (K§11 item 17, K§6.4.3, RS-9). `b = codex_request("turn-with-reasoning")`; `b["store"] = true`; remove `include`; push `ns_tool()` onto `b["tools"]`. Reply `200 SSE sse(&resp_events("response.completed","completed",usage_up()))`. Post `b`.
     ```rust
     assert_eq!(h.upstream_paths(), ["/v1/responses"]);
     let up = &h.upstream_bodies()[0];
     let mut want = b.clone();
     want["model"] = json!(UPSTREAM_MODEL);
     want["store"] = json!(false);
     want["include"] = json!(["reasoning.encrypted_content"]);
     assert_eq!(up, &want);
     let raw = &h.upstream_raw()[0];
     for item in b["input"].as_array().unwrap().iter().filter(|i| i["type"] == "reasoning") {
         assert!(contains(raw, serde_json::to_string(item).unwrap().as_bytes()), "{item}");
     }
     assert!(up["tools"].as_array().unwrap().contains(&ns_tool()));
     assert!(!contains(raw, b"cache_control"));
     ```
  2. `include_keeps_client_entries` (K§6.4.3). Reply as in test 1. Post `turn-with-reasoning` with `include = ["message.output_text.logprobs"]`, then with `include = ["reasoning.encrypted_content","message.output_text.logprobs"]`, then with `include = null`.
     ```rust
     let ups = h.upstream_bodies();
     assert_eq!(ups[0]["include"], json!(["message.output_text.logprobs","reasoning.encrypted_content"]));
     assert_eq!(ups[1]["include"], json!(["reasoning.encrypted_content","message.output_text.logprobs"]));
     assert_eq!(ups[2]["include"], json!(["reasoning.encrypted_content"]));
     assert!(ups.iter().all(|u| u["store"] == json!(false)));
     ```
  3. `include_not_array_400` (K§6.4.3). Post `turn-with-reasoning` with `include = "reasoning.encrypted_content"`.
     ```rust
     assert_eq!(s.as_u16(), 400);
     assert_eq!(oai_err(&b), json!({"type":"zygo_bad_request","message":"include must be an array"}));
     assert!(h.upstream_bodies().is_empty());
     ```
  4. `no_cache_markers_toward_responses_upstream` (K§11 item 19 second half, K-D48). `Harness::start(Dialect::Openai, Agent::OpenCode, "")` (`insert_markers.opencode` defaults to `true`). Reply as in test 1; post `codex_request("first-turn")`.
     ```rust
     assert_eq!(s.as_u16(), 200);
     assert!(!contains(&h.upstream_raw()[0], b"cache_control"));
     ```
  5. `stream_passes_bytes_and_records_completed_usage` (K§11 item 17, K§6.4.2 C3). `up = sse(&resp_events("response.completed","completed",usage_up()))`; reply `200 SSE up`; post `turn-with-reasoning`.
     ```rust
     assert_eq!(s.as_u16(), 200);
     assert!(hv(&hd, "content-type").starts_with(SSE));
     assert_eq!(&b[..], &up[..]);
     let rows = h.usage_rows();
     assert_eq!(rows.len(), 1);
     assert_eq!((rows[0].status, rows[0].provider.as_str(), rows[0].route.as_str()), (200, PROVIDER, "/openai/v1/responses"));
     assert_eq!(counts(&rows[0]), (100, 20, 0, 30));
     ```
  6. `incomplete_usage_recorded` (K§6.4.2 C3). Reply `sse(&resp_events("response.incomplete","incomplete",json!({"input_tokens":200,"input_tokens_details":{"cached_tokens":0},"output_tokens":64,"total_tokens":264})))`.
     ```rust
     assert_eq!(&b[..], &up[..]);
     assert_eq!(counts(&h.usage_rows()[0]), (200, 0, 0, 64));
     ```
  7. `non_streaming_records_json_usage` (K§11 item 17). `b = turn-with-reasoning` with `stream = false`; reply `200 JSON serde_json::to_vec(&resp_json())`.
     ```rust
     assert!(hv(&hd, "content-type").starts_with(JSON));
     assert_eq!(serde_json::from_slice::<Value>(&b).unwrap(), resp_json());
     assert_eq!(counts(&h.usage_rows()[0]), (40, 10, 0, 5));
     ```
  8. `openai_first_candidate_dialect` (K§6.4.3 candidates). `Harness::start_dialects(&[Dialect::Anthropic, Dialect::OpenaiChat, Dialect::Openai], Agent::ClaudeCode, "")`; reply as in test 5; post `first-turn`.
     ```rust
     assert_eq!(h.upstream_paths(), ["/v1/responses"]);
     assert_eq!(s.as_u16(), 200);
     ```
  9. `mapped_streams_parse_as_messages` (review focus 11, K§6.4.2 C3). For `(d, reply)` in `[(Dialect::Openai, sse(&resp_events("response.completed","completed",usage_up()))), (Dialect::OpenaiChat, chat_sse())]`, a fresh `Harness::start(d, Agent::ClaudeCode, "")` posts to `M` the Messages body `{"model":MODEL,"max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hi"}]}`. `chat_sse() -> Vec<u8>`: `data: <compact json>\n\n` for `{"id":"c1","object":"chat.completion.chunk","model":"up-rs","choices":[{"index":0,"delta":{"role":"assistant","content":"hi"},"finish_reason":null}]}`, the same with `delta:{}` and `finish_reason:"stop"`, and `{"id":"c1","object":"chat.completion.chunk","model":"up-rs","choices":[],"usage":{"prompt_tokens":120,"completion_tokens":30,"total_tokens":150,"prompt_tokens_details":{"cached_tokens":20}}}`, then `data: [DONE]\n\n`.
     ```rust
     assert_eq!(s.as_u16(), 200, "{d:?}");
     let rows = h.usage_rows();
     assert_eq!((rows[0].route.as_str(), counts(&rows[0])), ("/anthropic/v1/messages", (100, 20, 0, 30)), "{d:?}");
     ```
  10. `responses_usage_reads_terminal_lines_only` (Produces).
      ```rust
      use zygo_proxy::tee::responses_usage;
      let line = |v: Value| format!("data: {v}").into_bytes();
      assert_eq!(responses_usage(&line(json!({"type":"response.completed","response":{"usage":usage_up()}}))), Some(usage_up()));
      assert_eq!(responses_usage(&line(json!({"type":"response.incomplete","response":{"usage":usage_up()}}))), Some(usage_up()));
      assert_eq!(responses_usage(&line(json!({"type":"response.created","response":{"usage":null}}))), None);
      assert_eq!(responses_usage(&line(json!({"type":"response.completed","response":{"usage":null}}))), None);
      assert_eq!(responses_usage(&line(json!({"type":"message_delta","usage":{"output_tokens":3}}))), None);
      assert_eq!(responses_usage(b"event: response.completed"), None);
      assert_eq!(responses_usage(b"data: {not json usage"), None);
      ```
  11. `non_2xx_passes_status_and_body` (K§6.4.4). Reply `400 JSON br#"{"error":{"type":"invalid_request_error","message":"bad reasoning item","param":"input","code":null}}"#`; post `turn-with-reasoning`.
      ```rust
      assert_eq!(s.as_u16(), 400);
      assert_eq!(&b[..], &reply[..]);
      assert_eq!(h.usage_rows()[0].status, 400);
      ```
  `hv(h, n) -> String` is the header value as a string (as in `tests/responses_forward.rs:84-86`).

- [x] 3. **See them fail.** `cargo test -p zygo-proxy --test responses_passthrough --locked` fails to compile (`tee::responses_usage` does not exist). With step 4 alone (the function in place), tests 1-9 and 11 fail: `openai` is no candidate (`400 zygo_no_provider`).

- [x] 4. **Tee.** In `crates/zygo-proxy/src/tee.rs` (`use zygo_config::Dialect;`, `use crate::translate::usage_of;`):
  - `pub fn responses_usage(line: &[u8]) -> Option<Value>` as in Produces: strip `data:`, skip a line with no `usage` bytes (as `line_usage` does), parse the trimmed rest, `type` is `response.completed` or `response.incomplete`, take `response.usage` when it is an object.
  - `fn usage_line(lines: Dialect, line: &[u8]) -> Option<(bool, Value)>`: `Dialect::Openai` → `responses_usage(line).map(|u| (true, usage_of(Some(&u), Dialect::Openai)))` (a terminal event holds the final counts: `set`); any other dialect → `line_usage(line)`.
  - Field `lines: Dialect` on `Tee`, `Dialect::Anthropic` in `new`; `pub fn with_lines(mut self, lines: Dialect) -> Tee`.
  - `Usage::observe(&mut self, lines: Dialect, line: &[u8])` and `Tee::read` call `usage_line(lines, line)` in place of `line_usage(line)`; the rest of each body is unchanged.
  - `finish`, non-streamed: when `self.lines == Dialect::Openai`, the object `v["usage"]` (only when it is an object) goes through `usage_of(Some(u), Dialect::Openai)` before `self.usage.set` and `CacheSplit::read`; the Messages form is unchanged.

- [x] 5. **Response context.** In `crates/zygo-proxy/src/pipeline.rs`, add `pub lines: Dialect` to `ResponseCtx` after `dialect`, doc `/// The form of the body the tee and the response stages read: Openai only for a Responses upstream answering a Responses client (K§6.4.2 C3), else Anthropic.`, and `.field("lines", &self.lines)` in its `Debug`.

- [x] 6. **Forward.** In `crates/zygo-proxy/src/forward.rs`:
  - `fn responses_same(body: &mut Value, raw: &Bytes, changed: bool, c: &Candidate) -> Result<(Bytes, Vec<Adjustment>, NullMap), ZygoError>`, doc `/// A Responses body toward an openai candidate (K§6.4.3 Responses → Responses): store := false, include gains reasoning.encrypted_content, then model_body. body is left as it was.` Rules: `store` is set to `false` (inserted when absent); `include` absent or `null` → `["reasoning.encrypted_content"]`; an array without that string → it is appended; an array with it → unchanged; another JSON type → `Err(ZygoError::bad_request("include must be an array"))`. When neither field changes, `model_body(body, raw, changed, c)`; else the edits go on a clone and `model_body(&mut clone, raw, true, c)`. The `NullMap` is `NullMap::default()`.
  - In `attempt_body`, before `if client == Dialect::Openai && c.dialect != client` (`:882`): `if client == Dialect::Openai && c.dialect == Dialect::Openai { return responses_same(body, raw, changed, c); }`.
  - After `let translated = …` (`:509`): `let form = if translated { Dialect::Anthropic } else { c.dialect };` (a translated body reaches the tee as Messages events or a Messages body: the mapper output, an Anthropic upstream, or `translate_response`).
  - `let mut tee = Tee::new(streamed).with_lines(form);` (`:591`); `lines: form,` in the `ResponseCtx` literal (`:613-624`).
  - `rctx.usage.observe(rctx.lines, &l.0);` (`:1455`).

- [x] 7. **Candidate order.** In `crates/zygo-proxy/src/resolve.rs:85`: `Dialect::Openai => &[Dialect::Openai, Dialect::Anthropic, Dialect::OpenaiChat],`. Doc of `upstream_dialect` (`:78-81`): `` `openai` client → `openai`, else `anthropic`, else `openai-chat` (same dialect first; then Messages is the pivot: one pass before two, K-C2) ``; drop the RS-5 sentence.

- [x] 8. **RS-3 interim test.** Delete `openai_only_provider_is_no_candidate` and its doc line from `crates/zygo-proxy/tests/responses_route.rs:274-280`; drop `Openai as O` from the import at `:12` (its only user).

- [x] 9. **See them pass.** The Check command exits 0: `responses_passthrough` lists 11 tests (≥ 6), and every `zygo-proxy` target passes (`session_api` and `request_path` keep `Tee::new(bool)`).

- [x] 10. **Floor.** `cargo test --workspace --locked --no-fail-fast` passes (CI `test` job, `.github/workflows/ci.yml:53`). No test uses a sleep, a deadline or a wall-clock assertion; `usage_rows` waits with no total limit (`tests/support/responses.rs:370-375`).

- [x] 11. **Commit.** One commit `feat(proxy): RS-5 Responses passthrough and Responses usage in the tee`, trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:RS-5`.

### Implementation notes (2026-11-25)

- Step 3: with step 4 alone, 9 of 11 tests fail (`400 zygo_no_provider`), not tests 1-9 and 11. Test 9 (`mapped_streams_parse_as_messages`) passes before step 7 because it posts a Messages body, and an `openai` upstream was already a candidate for a Messages client; it guards the Messages parse of mapped streams against the new form split. Test 10 passes once step 4 is in place.
- Step 6: `responses_same` also returns `zygo_bad_request` when the body is not a JSON object. The route accepts only an object, so this branch is not reached; it replaces a panic.
- Step 4: `Tee::new` is written over several lines; the added `lines` field made the one-line literal too long.

### Author's checks (2026-11-25)

1. Spec coverage: K§6.4.3 Responses → Responses (`model`, `store`, `include`, stage edits, native tools and reasoning) → tests 1-3, step 6; candidates → test 8, step 7; K§6.4.2 C3 row Responses and the non-streaming sentence → tests 5-7, steps 4-6; cache row (no markers toward a Responses upstream) → tests 1, 4; K§6.4.4 for the pair → test 11; K§11 item 17 → tests 1, 5, 7; item 19 second half → test 4; K-D21 (unchanged reasoning bytes, no mark) → test 1; K-D48 → tests 4, 5, 9.
2. Step scan: each code step gives the signature and the fixed values; the only bodies are the `include` rule and `form`, which the tests do not fully decide.
3. Interface match: `ResponseCtx.lines: Dialect` and `tee::responses_usage(line: &[u8]) -> Option<Value>` keep the Produces names; RS-6 and RS-7 read `forward.rs` and `tee.rs` after this node; `Tee::new(bool)` and `ModelRoute`, `Harness::start` are unchanged.
4. Review focus: line 11 (reasoning byte-equal, `include` kept, `store` false, `response.completed` counts, a mapped chat stream not read as Responses lines) → tests 1, 2, 5, 9; line 12 (RS-5 half: no `cache_control` toward a Responses upstream) → tests 1, 4.
5. Proportion: about 60 lines of code across four source files; the plan gives signatures and assertions, not bodies.
6. Live code: every cited path and line was opened at da0c0387; `usage_of` already holds the count rule and is reused.
