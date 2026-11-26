# RS-4 — Response side for Anthropic and chat upstreams: stream order, encoder after the tee, non-streaming path, errors after the first event

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stage table row usage; the stream-order table rows Anthropic and chat), K§6.4.3 (the response side of Responses → Anthropic and Responses → chat; stream obligations 6 and 8), K§6.7 (upstream streams), K§11 items 14 (the after-the-first-event half, at the route) and 18; K-D48, K-D50
**Files:** `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/support/responses.rs`, `crates/zygo-proxy/tests/responses_forward.rs` (new), `docs/specs/2026-11-25-client-model-design.md` (round 2: K§6.4.3 stream obligation 6, K§6.4.4, one change-log line)
**Consumes:** `Encoder::new` (RS-2), `response` (RS-2), `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3), `fixtures/codex/requests/` (PC-3)
**Produces:** —
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_forward -- --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo-proxy --locked` → at least 6 tests in `responses_forward` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: a captured Codex request streamed through a Claude Code session to an Anthropic upstream returns Responses SSE and the usage row has the upstream Messages counts; Responses → chat reaches the chat mock, its stream returns as Responses SSE and the usage row is recorded (K§11 item 18); an upstream error event after the first event → one `response.failed` and no other terminal event (K§11 item 14, second half); a non-streaming request to each upstream returns one Response object with the same content; a non-2xx non-streaming answer keeps its status with an OpenAI error body; the existing Messages-client stream tests pass unchanged

## Planning notes

- Stream order per K§6.4.2: upstream Messages SSE → tee → `Encoder`; chat SSE → existing `StreamMapper` (`translate/sse.rs:48`) → tee → `Encoder`. Today the mapper output goes into the tee at `forward.rs:1098-1110`; the encoder goes after the tee, so a later window rewrite on `message_delta` reaches the client inside `response.completed` (RS-7).
- The translated non-streaming paths are measured as today, on the Messages form (K§6.4.2).
- `tests/support/responses.rs` is in Files for any helper this node adds to the harness of RS-3.
- Base §7.6 null removal now covers the translated Responses route (author's call, RS-4 step 5c).
- Lead's decision: the `message_events` defect (`redacted_thinking` and `server_tool_use` become empty text blocks when an upstream answers a streaming request with JSON) is a separate node, RS-10, which the lead adds. RS-4 does not fix it.
- Lead's decision: RS-4 merges only after PC-12 is DONE (graph edge). Implementation proceeds now.

## Plan

Base: `origin/phase-4` at ac96e1fa (RS-2 merged 1ad733f0, RS-3 merged f37bba43). Every path, line and signature below was opened at that commit.

### Premise check (2026-11-26)

- RS-3 left a placeholder: `forward.rs:512-515` answers every translated 2xx on the Responses route with `502 zygo_upstream_bad_response`. RS-4 removes it.
- The translated stream branch (`forward.rs:527-530`) builds `StreamMapper::new(c.dialect, …)`; with `c.dialect = Anthropic` the mapper emits nothing (`translate/sse.rs:111-114`, `:149-152`). So toward an Anthropic upstream a Responses client must get no mapper: upstream Messages SSE goes straight into the tee.
- The non-streamed branch (`forward.rs:534-540`) checks `is_response(v, c.dialect)` and calls `translate_response(&v, c.dialect, …)` (`translate/mod.rs:52-63`). Both read an Anthropic body as a Responses body (`output`). Toward an Anthropic upstream the body is already the Messages form: RS-4 checks it with its own `is_message` and uses it as is.
- The tee reads Messages `message_start` / `message_delta` usage on streamed bodies (`tee.rs:71-83`, `:223-266`) and the `usage` of a whole JSON copy at `finish` (`tee.rs:193-206`). It returns each chunk unchanged when it is not in line mode (`tee.rs:171-191`). Line mode is armed only on the Messages route (`window.rs:130`). So the encoder placed after the tee gets the Messages bytes as the tee saw them, and the usage row keeps the Messages counts (K§6.4.2 C3) with no tee change.
- `next_chunk` (`forward.rs:1184-1236`) runs upstream → mapper → restore filter → tee. The K§6.4.2 citation `forward.rs:1098-1110` has drifted to `:1184-1236`. Only the line numbers changed.
- Injected nulls: `armed = route == ModelRoute::Messages && …` (`forward.rs:517`, from da682dd8, before the Responses route existed). `responses_attempt` runs `schema::rewrite` on the Messages pivot (`forward.rs:906`). The built-in rule `optional_property` (family `openai`, `crates/zygo-config/data/schema_rules.toml:63-66`) makes each optional property required and nullable. Codex's `exec_command` has 9 optional properties (`fixtures/codex/requests/first-turn.json`, `required: ["cmd"]`). Toward a chat upstream, a model's `"workdir": null` would therefore reach Codex. Base spec §7.6 (`docs/specs/2026-11-18-zygo-design.md:314`) says that an injected null is removed on the way back, so that the client sees its own shape. This is a defect that the RS-4 path would expose, and it is in scope (§5.4). Step 5c fixes it.
- Encoder API (RS-2, `translate/responses_client.rs`): `Calls::of(req: &Value) -> Calls` (`:582`), `Encoder::new(model: &str, t: &Target, calls: Calls) -> Encoder` (`:854`), `push(&mut self, chunk: &[u8]) -> Vec<SseEvent>` (`:877`), `fail(&mut self, code: &str, message: &str) -> Vec<SseEvent>` (`:902`, emits nothing before the start), `finish(&mut self) -> Vec<SseEvent>` (`:912`, gives `response.failed` `upstream stream ended early` when no terminal event came, and starts the stream when it was not started), `ended(&self) -> bool` (`:931`), `adjustments(&self) -> Vec<Adjustment>` (`:942`), `response(msg: &Value, model: &str, t: &Target, calls: &Calls) -> (Value, Vec<Adjustment>)` (`:736`). RS-9 (PR #161, open) changes only the inside of `Calls` (a `namespaced` table) and keeps these signatures.
- PC-12: `origin/phase-4--PC-12` `nodes/PC-12.md` "Observed": PC17 and PC18 are `verified` on Codex 0.172.0 for this exact event sequence. The PC-12 row is still `TODO` in `graph.md:87` on `origin/phase-4`. RS-4 reads no file of PC-12.
- The harness (`tests/support/responses.rs`) has one fixed reply per mock (`reply_with`, `:251`). It cannot fail a body partway through. Step 1 adds `reply_cut`.

### Interfaces

Consumes (as in the code now):

- `responses_client::Calls::of(req: &Value) -> Calls`; `Encoder::new(model: &str, t: &Target, calls: Calls) -> Encoder`; `Encoder::{push, fail, finish, ended, adjustments}`; `responses_client::response(msg: &Value, model: &str, t: &Target, calls: &Calls) -> (Value, Vec<Adjustment>)`. All of these are listed above.
- `ModelRoute::Responses` (`pipeline.rs:33`); `ModelRoute::dialect` → `Dialect::Openai` (`pipeline.rs:38-43`).
- `Harness::start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness`, `post`, `reply_with(&self, status: u16, content_type: &str, body: Vec<u8>)`, `usage_rows`, `upstream_paths`, `codex_request(name: &str) -> Value`, `MODEL`, `PROVIDER` (`tests/support/responses.rs`).
- `fixtures/codex/requests/{first-turn,after-custom-tool-call-output}.json` (read only). `first-turn` has `stream: true`, `model: "gpt-mock"` and `exec_command` with `required: ["cmd"]`. `after-custom-tool-call-output` has the `custom` tool `apply_patch`.
- `RestoreFilter::push_raw` / `finish_raw` (`translate/schema.rs:632`, `:674`), `NullMap::restore_message` (`schema.rs:53`), `message_events` (`translate/sse.rs:436`).

Produces: nothing for later nodes. RS-5 builds on the `Encode` field of `Flow` (step 5e) and on `reply_cut`.

### Steps

- [x] 1. **Harness.** In `tests/support/responses.rs`, add to `Upstream` the field `cut: Arc<AtomicBool>`. When the field is set, the mock sends the reply body as `Body::from_stream(futures_util::stream::iter([Ok::<Bytes, std::io::Error>(body), Err(std::io::Error::other("cut"))]))`, so that the proxy sees a read error after the first bytes. (Fixed during implementation: with `stream::iter`, hyper ends the connection before it flushes the head, and the proxy sees a send error and answers 503. The error item now comes from `stream::once(async { tokio::task::yield_now().await; Err(..) })` chained after the first chunk, so hyper flushes the head and the bytes before the error. There is no timer. The file passed 60 runs out of 60.) Add `pub fn reply_cut(&self, status: u16, content_type: &str, body: Vec<u8>)`, which sets the reply and `cut = true`. `reply_with` sets `cut = false`. Pass: `cargo test -p zygo-proxy --test responses_route --locked` passes unchanged.

- [x] 2. **Failing tests.** Write `crates/zygo-proxy/tests/responses_forward.rs`. Its header is `#[path = "support/responses.rs"] mod support;`, `const R: &str = "openai/v1/responses";`, and each harness is `Harness::start(<A|C>, Agent::ClaudeCode, "")`. Helpers, which hold no logic under test:
  - `msg_sse(evs: &[Value]) -> Vec<u8>`: `event: <type>\ndata: <compact json>\n\n` for each event.
  - `chat_sse(chunks: &[Value]) -> Vec<u8>`: `data: <json>\n\n` for each chunk, then `data: [DONE]\n\n`.
  - `events(b: &[u8]) -> Vec<(String, Value)>`: splits on blank lines and takes `event:` and `data:`.
  - `terminals(evs)`: the events named `response.completed|incomplete|failed`.
  - `done_items(evs)`: the `item` of each `response.output_item.done`.
  - `norm(v: Value) -> Value`: each match of `\b(resp|msg|rs|fc|ctc)_[0-9A-HJKMNP-TV-Z]{26}` replaced with `$1_X`.
  - `oai_err(b) -> Value`, copied from `tests/responses_route.rs:24-30`.

  Fixtures in the file:
  - `MSG_EVENTS`, a fn that returns `Vec<Value>`: `message_start` (message `{id:"msg_up", type:"message", role:"assistant", model:"up-rs", content:[], usage:{input_tokens:100, cache_read_input_tokens:30, cache_creation_input_tokens:5, output_tokens:1}}`); `ping`; text block 0 with deltas `"hel"`, `"lo"`; `tool_use` block 1 `{id:"toolu_1", name:"exec_command", input:{}}` with `input_json_delta` `{"cmd":"ls"}`; `tool_use` block 2 `{id:"toolu_2", name:"apply_patch"}` with `input_json_delta` `{"input":"*** Begin Patch\n*** End Patch\n"}`; `server_tool_use` block 3; `message_delta` `{stop_reason:"tool_use"}` usage `{output_tokens:42}`; `message_stop`.
  - `MSG_JSON`, the same content as one Messages body: `type:"message"`, content `[text "hello", tool_use toolu_1 exec_command {"cmd":"ls"}, tool_use toolu_2 apply_patch {"input":"*** Begin Patch\n*** End Patch\n"}, server_tool_use]`, `stop_reason:"tool_use"`, usage `{input_tokens:100, cache_read_input_tokens:30, cache_creation_input_tokens:5, output_tokens:42}`.
  - `CHAT_CHUNKS`: content `"hel"`, `"lo"`; `tool_calls[{index:0, id:"call_1", type:"function", function:{name:"exec_command", arguments:"{\"cmd\":\"ls\"}"}}]`; `finish_reason:"tool_calls"`; `{choices:[], usage:{prompt_tokens:120, completion_tokens:7, prompt_tokens_details:{cached_tokens:20}}}`.
  - `CHAT_JSON`: one completion with `message:{role:"assistant", content:"hello", tool_calls:[the same call]}`, `finish_reason:"tool_calls"`, and the same usage.

  Tests. Each body is `codex_request("after-custom-tool-call-output")` unless named otherwise:
  1. `anthropic_stream_returns_responses_sse_and_usage`. The reply is `msg_sse(MSG_EVENTS)`, `text/event-stream`. Assert:
     - status 200 and `content-type` starts with `text/event-stream`;
     - `upstream_paths() == ["/v1/messages"]`;
     - the event names start `["response.created","response.in_progress"]`;
     - the `sequence_number`s are `0..n` in order;
     - `terminals.len() == 1`, the last event is named `response.completed`, its `response.output == []` and its `response.model == body["model"]`;
     - `response.usage == {"input_tokens":135,"input_tokens_details":{"cached_tokens":30},"output_tokens":42,"total_tokens":177}`;
     - `done_items` types are `["message","function_call","custom_tool_call"]`: the `custom_tool_call` has `name:"apply_patch"`, `call_id:"toolu_2"` and `input:"*** Begin Patch\n*** End Patch\n"`, and the `function_call` has `call_id:"toolu_1"` and `arguments` that parse to `{"cmd":"ls"}`. The `custom_tool_call` item shows that `Calls` comes from the client's body: the upstream Messages body has `apply_patch` as a plain tool;
     - no event is named `ping` and no event `data` holds `server_tool_use`;
     - the single usage row has `(status, provider, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens) == (200, PROVIDER, 100, 30, 5, 42)`, `route == "/openai/v1/responses"`, and `adjustments` contains `"server_tool_dropped"`.
  2. `chat_stream_returns_responses_sse_and_usage` (K§11 item 18), with body `first-turn` and reply `chat_sse(CHAT_CHUNKS)`. Assert:
     - `upstream_paths() == ["/v1/chat/completions"]`, status 200 and SSE;
     - one terminal, `response.completed`, with usage `{"input_tokens":120,"input_tokens_details":{"cached_tokens":20},"output_tokens":7,"total_tokens":127}`;
     - `done_items` types are `["message","function_call"]` and the message text is `"hello"`;
     - the row has `(input_tokens, cache_read_tokens, cache_write_tokens, output_tokens) == (100, 20, 0, 7)`.
  3. `anthropic_error_after_first_event_one_failed` (K§11 item 14, second half). The reply is `msg_sse` of `message_start`, then text block 0 start, then delta `"par"`, then `{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`, then `content_block_stop`, `message_delta` and `message_stop`. Assert:
     - status 200;
     - one `response.output_text.delta` with `delta:"par"`;
     - `terminals.len() == 1` and the terminal is the last event;
     - its name is `response.failed` and `response.error == {"code":"overloaded_error","message":"Overloaded"}`;
     - no event is named `response.completed`.
  4. `chat_error_after_first_event_one_failed`. The body is `first-turn` and the reply is `chat_sse` of content `"par"`, then `{"error":{"message":"boom","type":"server_error"}}`, then a `finish_reason:"stop"` chunk. Assert one terminal, last, `response.failed` with `response.error == {"code":"api_error","message":"boom"}`.
  5. `non_streaming_anthropic_same_content`. First post the body with the `msg_sse(MSG_EVENTS)` reply. Then post the body with `"stream": false` and the reply `MSG_JSON` (`application/json`). Assert:
     - status 200, `content-type` starts with `application/json`;
     - the body is one object with `object:"response"` and `status:"completed"`;
     - `norm(output) == norm(done_items of the stream run)`;
     - `usage` equals the stream's terminal usage;
     - `x-zygo-adjustments` contains `server_tool_dropped`.
     Then post the stream body (`stream: true`) with a JSON reply of `MSG_JSON` without its `server_tool_use` block, and assert that the client gets SSE whose `norm(done_items)` equal the same list. The block is left out because `message_events` (`translate/sse.rs:436-463`) turns any block type it does not know into a text block; see the lead notes. Rows 2 and 3 have `(input_tokens, cache_read_tokens, cache_write_tokens, output_tokens) == (100, 30, 5, 42)`.
  6. `non_streaming_chat_same_content`. The body is `first-turn` with `stream:false` and the reply is `CHAT_JSON`. Assert:
     - one Response object whose `norm(output)` equals the `norm(done_items)` of a stream run with `CHAT_CHUNKS`;
     - `usage == {"input_tokens":120,"input_tokens_details":{"cached_tokens":20},"output_tokens":7,"total_tokens":127}`;
     - the row has `(100, 20, 0, 7)`.
  7. `non_2xx_non_streaming_keeps_status_openai_error`. With `stream:false`:
     - Anthropic replies 500 `{"type":"error","error":{"type":"api_error","message":"boom"}}`. Assert status 500, JSON, `oai_err == {"type":"api_error","message":"boom"}` and no `data:` in the body.
     - Chat (body `first-turn`) replies 429 `{"error":{"message":"slow down","type":"rate_limit"}}`. Assert status 429 and `oai_err == {"type":"rate_limit_error","message":"slow down"}`.
  8. `read_error_mid_stream_one_failed`. Call `reply_cut(200, "text/event-stream", msg_sse([message_start, text start, delta "par"]))`. Assert status 200, one terminal event, the terminal is last, and it is named `response.failed` with `response.error.code == "api_error"`.
  9. `injected_null_removed_for_responses_client` (base §7.6). The body is `first-turn` and the upstream is chat with the arguments `{"cmd":"ls","workdir":null}`.
     - Stream: `x-zygo-adjustments` contains an item that starts with `schema:optional_property:`, and the `function_call` `arguments` parse to exactly `{"cmd":"ls"}`.
     - Non-streaming (`stream:false`, `CHAT_JSON` with those arguments): the `function_call` `arguments` parse to `{"cmd":"ls"}`.
  10. `bad_2xx_body_is_502_openai_shape`. Anthropic replies 200 `application/json` `{"output":[]}` and the body has `stream:false`. Assert status 502 and `oai_err(b) == {"type":"zygo_upstream_bad_response","message":"upstream response could not be translated"}` (`errors.rs:36-42`, `forward.rs:199-201`).

- [x] 3. Run `cargo test -p zygo-proxy --test responses_forward --locked`. Expect failure: tests 1-6 and 8-10 fail on the RS-3 placeholder 502 or on the Anthropic body check. Test 7 may already pass, because RS-3 built that path; it stays as the contract's case. (Run 2026-11-26: 8 failed, 2 passed. Tests 7 and 10 pass already: the RS-3 placeholder gives the same 502 `zygo_upstream_bad_response` body that test 10 asks for. Test 10 stays to keep that answer after the placeholder goes.)

- [x] 4. Commit the tests and the harness helper: `test(proxy): RS-4 Responses response side tests`. Trailers: `Zygo-Session: <session>`, `Zygo-Work: phase4:RS-4`.

- [x] 5. **Implement in `crates/zygo-proxy/src/forward.rs`:**
  - a. In `run`, inside the `route == ModelRoute::Responses` block (`:339-341`), build `calls` from the client's body as received, before any stage: `let calls = (route == ModelRoute::Responses).then(|| responses_client::Calls::of(&body));`. After `name` (`:342`), add `let client_model = name.clone();`, the name the client sent (stream obligation 1).
  - b. Remove the placeholder guard at `:512-515`.
  - c. `armed` (`:517`) becomes `status.is_success() && !nulls.is_empty() && (route == ModelRoute::Messages || (route == ModelRoute::Responses && translated))`. The same-dialect Responses path (RS-5) stays unarmed.
  - d. In the translated stream branch (`:527-530`), use `mapper = (c.dialect != Dialect::Anthropic).then(|| StreamMapper::new(c.dialect, &Target::of(c)).errors(estimate, key));`. Toward Anthropic, the upstream Messages SSE goes to `restore.push_raw` (when armed) and to the tee, and `next_chunk` already does both (`:1196-1199`).
  - e. In the whole-body branch (`:531-548`), a 2xx toward Anthropic is accepted when `is_message(&v)`, and `m = v` (no `translate_response`). Other dialects keep `is_response` and `translate_response`. Add `fn is_message(v: &Value) -> bool`: `type == "message"` and `content` is an array. Keep the Messages `v` in scope (call it `msg: Option<Value>`, set only on a 2xx) for the next sub-step.
  - f. Add the post-tee stage:
    ```rust
    /// A translated 2xx to a Responses client, after the tee (K§6.4.2 C3): the stream encoder, or a whole answer's
    /// Response object, sent when the upstream body ends.
    enum Encode { Stream(Box<responses_client::Encoder>), Whole(Bytes) }
    ```
    In `run`, set it when `ctx.dialect == Dialect::Openai && translated && status.is_success()`, using `t = Target::of(c)` and `calls` from 5a:
    - `streamed` (an SSE upstream, or the whole body sent as `message_events`) gives `Encode::Stream(Box::new(Encoder::new(&client_model, &t, calls)))`;
    - otherwise `let (r, codes) = responses_client::response(&msg, &client_model, &t, &calls); adjustments.extend(codes);` gives `Encode::Whole(serde_json::to_vec(&r)…)`. This comes before `zygo_headers`, so the codes reach the header and the row.
    For any other case the field is `None`. Add the field `encode: Option<Encode>` to `Flow`.
  - g. In `next_chunk`, after `out` is computed (the tee's Messages bytes), pass it through `fn encoded(encode: &mut Option<Encode>, out: Bytes, end: bool) -> Bytes`:
    - `None` gives `out`;
    - `Stream(e)` gives the bytes of `e.push(&out)`, plus `e.finish()` when `end`;
    - `Whole(b)` gives nothing until `end`, then `b`.
    Here `end` is `f.done` after this chunk. When `Stream(e)` reports `e.ended()` and `f.done` is still false, call `tee.finish(&mut on_line)` and set `f.done = true`: the upstream is no longer read, the same as `m.ended()` at `:1206-1209`.
  - h. `Some(Err(_))` (`:1212-1216`) with `Some(Encode::Stream(e))`: let `b = e.fail("api_error", "upstream response failed")`, or `e.finish()` when `!e.ended()` after that. Set `done = failed = true` and return `Ok(b)`, so that a started stream ends with one terminal event (stream obligation 6). With no encoder, the code is unchanged.
  - i. In `Flow::drop`, before `self.pending.row.adjustments = …` (`:1180`), extend `self.rctx.adjustments` with `e.adjustments()` when `encode` is `Stream(e)`. `server_tool_dropped` and `reasoning_unmarked:<n>` then reach the row.

- [x] 6. Run `cargo test -p zygo-proxy --test responses_forward --locked`. Pass: all 10 tests pass. Then run `cargo test -p zygo-proxy --test responses_route --test responses_stream --test translate --test serve --locked`. Pass: all pass unchanged.

- [x] 7. In this file's Planning notes, add one line: "Base §7.6 null removal now covers the translated Responses route (author's call, RS-4 step 5c)." Commit `forward.rs` and this file: `feat(proxy): RS-4 Responses response side for Anthropic and chat upstreams`, with the same trailers. Push `phase-4--RS-4`.

- [x] 8. Run the Acceptance check alone as one command (it is the Check line). Pass: exit 0, and `--list` counts at least 6.

- [x] 9. Run the verification floor: `CARGO_INCREMENTAL=0 cargo test --workspace --locked --no-fail-fast`. Pass: exit 0. Then run `git diff origin/phase-4 --stat -- crates/zygo-proxy/tests/`. Pass: it lists only `responses_forward.rs` and `support/responses.rs`, which shows that the existing Messages-client stream tests are unchanged. (Run 2026-11-26: Acceptance exit 0, with 10 tests in `responses_forward`. Workspace: 2419 passed, 2 failed, 778 ignored. The 2 failures are the known `zygo --test guard_hook` environment failures under /private/tmp (`binary_guard_not_a_repository_writes_no_line`, `binary_guard_slow_rev_parse_says_unread_once`). The test diff lists only the two files.)

### Author's checks (2026-11-26)

1. **Spec coverage.**
   - K§6.4.2 stream-order rows Anthropic and chat → 5d, 5f, 5g; tests 1 and 2.
   - Usage row, measured on the Messages form → tests 1, 2, 5 and 6 (row counts).
   - Stream obligation 6 (after the start), K§11 item 14 second half → 5g, 5h; tests 3, 4 and 8.
   - Stream obligation 8 → 5e, 5f; tests 5, 6 and 7. Obligation 8's non-2xx half → test 7.
   - K§11 item 18 → test 2.
   - K§6.7 upstream streams (an unparsable event and the bounds stay in the encoder, RS-2) → test 8 for an early end.
   - K-D48 (the tee parses Messages for these pairs) → no tee change, row asserts.
   - K-D50 → test 7 and RS-3's tests.
   - The lead's note 1 (`Calls` from the client's body) → 5a; the test 1 `custom_tool_call` assert.
   - Note 3 (usage still recorded) → the row asserts.
   - "Existing Messages-client stream tests pass unchanged" → steps 6 and 9.
2. **Step scan.** Each code step names one site with its line and its exact condition. The only body given is the `Encode` enum, because the tests do not decide its shape.
3. **Interface match.** All Consumes signatures were checked at `responses_client.rs:582`, `:736`, `:854`, `:877`, `:902`, `:912`, `:931` and `:942`. RS-9's diff (`origin/phase-4--RS-9`) keeps `Calls::of(&Value)` and `Encoder::new`. RS-5 consumes the `Flow` stage and the harness.
4. **Review focus.** Item 10, second half (an error after the first event at the route) → tests 3 and 4. Item 9 stays RS-2's (encoder unit tests).
5. **Proportion.** The plan gives sites, conditions and assertions. The code is about 60 lines in `forward.rs` and about 30 in the harness.
6. **Live code.** Every cited line was opened at ac96e1fa. There is no existing Messages-body check for an Anthropic 2xx: `is_response` covers only `output` and `choices` (`translate/mod.rs:60-63`). Hence `is_message`.

### Lead notes

- **Contract drift.** None in Consumes or Acceptance. One addition, the author's call: base §7.6 injected-null removal on the translated Responses route (step 5c, test 9). It is evidenced above and stays inside `forward.rs`. The K§6.4.2 line citation `forward.rs:1098-1110` is stale (now `:1184-1236`). That is spec text outside Files, so it is left for BS-1 or the next spec edit.
- **Found, outside Files: a new-node candidate.** `message_events` (`translate/sse.rs:436-463`) maps every block type other than `thinking` and `tool_use` to a text block. So `redacted_thinking` and `server_tool_use` in a JSON 2xx sent to a client that asked to stream come back as empty text blocks. This affects the Messages client today, and the Responses client through RS-4 step 5f. It is reached only when an upstream answers a streaming request with JSON.
- **PC-12** is `TODO` in `graph.md:87`, although `nodes/PC-12.md` "Observed" on `origin/phase-4--PC-12` records PC17 and PC18 as verified. RS-4 depends on it in the graph.
- **Files outside the Files line:** none.
- **Overlap.** RS-5 shares `forward.rs` and `tests/support/responses.rs`, and runs after RS-4. RS-9 shares no file: it edits `responses_client.rs`, `responses_request.rs`, `responses_stream.rs` and the spec, and keeps the `Calls::of` and `Encoder::new` signatures. RS-8 shares no file: it edits `routes.rs`, `models.rs` and `responses_models.rs`.

## Review round 1 findings

Review of PR #165 at bcd6758b (1 important, 4 nits). Round 2 merges `origin/phase-4` at 8e4445d7 (RS-10 added). RS-9 (PR #161) is still open, so there is no namespaced map-back test yet.

- F1 (important, fixed): `read_error_mid_stream_one_failed` passed with the read-error branch removed. Fix: the test asserts `error == {code: api_error, message: "upstream response failed"}` and that the `par` text delta comes before the `response.failed` event. Mutation check: with the branch (`next_chunk`, `Some(Err(_))`, `Encode::Stream`) removed, this test and the F3 test fail (the client body ends with an EOF error). With the branch restored, both pass.
- F2 (nit, fixed): `reply_cut` no longer depends on hyper's poll order. The mock is now a raw HTTP/1.1 server on tokio (`exchange` in `tests/support/responses.rs`). It reads one request (the head and a `content-length` body), records it, and answers with `connection: close`. A cut reply declares a `content-length` one byte larger than the body it sends, then closes. There is no timer, and the signature of `reply_cut` is unchanged. `responses_forward`: 30 runs, 0 failures.
- F3 (nit, fixed): a read error before the first whole event now gives `response.created`, `response.in_progress`, then `response.failed {api_error, "upstream response failed"}`. Before the fix, `finish` dispatched the partial line and gave `upstream event is not JSON`. `Streamed::read_failed` uses a spare encoder (same model, target and calls), started by an empty `message_start`. It does this only when the real encoder has emitted nothing, so the client sees one sequence from 0. Test: `read_error_before_first_event_failed`.
- F4 (nit, fixed): after the encoder's terminal event, `next_chunk` keeps reading the upstream through the mapper and the tee to its end, and sends nothing more to the client. A client disconnect still drops `Flow`. Test: `encoder_failure_row_has_final_usage` (a 5 MiB `text_delta`, then `message_delta` `output_tokens: 4242`). The row has `(100, 30, 5, 4242)`; before the fix it had 1 output token.
- F5 (nit, fixed): a translated Responses stream arms the tee's line mode, and `Streamed::line` rewrites each Messages `error` line before the encoder:
  - The provider key is replaced with `[REDACTED]` before the encoder's 64-character cut, so no prefix of the key survives the cut either.
  - An `invalid_request_error` whose message is a PC14 over-long form gets the code `context_length_exceeded`. Its `response.failed` message is then set back to the full message by the PC14 rule (`pc14_form`, now shared with `responses_error`). This also covers a chat upstream's context error, which the §7.6 mapping turns into `prompt is too long`.
  - The line stages are not called on this route. The window stage arms only on the Messages route (`window.rs:130`), so nothing changes for the stages; RS-7 decides them for Responses.
  - Spec: K§6.4.3 stream obligation 6 and K§6.4.4 are rewritten, with one change-log line (author's call).
  - Tests: `in_stream_context_error_pc14_code` (both Anthropic forms, a message that only contains the phrase, and the chat mapping) and `in_stream_error_message_without_key` (a whole key, and a key across the cut).
- Results (round 2):
  - Acceptance: exit 0, with 14 tests in `responses_forward`.
  - `cargo test -p zygo-proxy --locked`: 490 passed, 0 failed.
  - `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings`: clean.
  - `responses_forward`: 30 runs, 0 failures.
  - Workspace: 2423 passed, 2 failed, 778 ignored. The 2 failures are the known `zygo --test guard_hook` environment failures under /private/tmp.

### Round 3

Review round 2 of PR #165 at a516e20c (2 important, 2 nits). Each fix is test-first: the test failed before the fix and passes after it.

- G1 (important, fixed in 3fb742f9): after the encoder's terminal event, `next_chunk` ends the client body. The rest of the upstream is read in a detached task (`drain`, `tokio::spawn`) that owns the `Flow`. The task reads through the tee, and the usage row is written once, when the `Flow` drops. The task ends at the upstream end, at a read error or at the upstream idle read timeout. Tests: `client_released_at_completed` and `client_released_at_encoder_failure`. In each test the mock (`reply_held`) declares `content-length` + 1 and keeps the connection open until the test sends on a oneshot. The client body ends with `response.completed` or `response.failed`. After the release, the row has the final `message_delta` counts. Before the fix both reads did not end. `encoder_failure_row_has_final_usage` passes with no change, because `usage_rows` waits until the row is written.
- G2 (important, fixed in 3d957228): the `prompt is too long` branch of `pc14_form` now goes through `pc14_message`, which applies the 512-character cap and replaces Cc, Cf, Zl and Zp characters. `responses_error` uses the same function. Test: `in_stream_prompt_too_long_capped_and_cleaned`. Before the fix the message had 5046 characters.
- G3 (nit, fixed in 617529f9): if a data line does not parse as JSON and contains the provider key, `Streamed::line` replaces each occurrence of the key with `[REDACTED]` (`scrub`). The parsed path is unchanged. Test: `split_error_event_without_key`. Before the fix the key reached the client in `response.failed`.
- G4 (nit, fixed in ce397965): K§6.4.3 stream obligation 9 now states both bounded buffers: the tee's partial line and the encoder's held bytes. It gives the peak as about 2 × `HOLD_MAX` per stream. One change-log line was added (author's call). The code is unchanged.
- Results (round 3):
  - Check `cargo test -p zygo-proxy --locked`: 501 passed, 0 failed.
  - Acceptance: exit 0, with 18 tests in `responses_forward`.
  - `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings`: clean.
  - Workspace: 2434 passed, 2 failed, 778 ignored. The 2 failures are the known `zygo --test guard_hook` environment failures under /private/tmp.

### Round 3 review dispositions

Review round 3 of PR #165 at 18492aeb (H1-H7), the last review round. Code fixes are test-first: each new test failed before its fix, except where noted.

- H1 (fixed in fb537ff0): `Streamed::line` holds the `data` lines of an error event split over lines (its `event` is `error`, or its first `data` line names `"error"`) until the blank line. It joins them as SSE does and reads the joined event on the parsed path: the key is removed from the decoded message, then `pc14_form`. Each held line goes on to the encoder as an SSE comment (`:`), which the encoder ignores; the blank line carries the rewritten `data` line. Lines of other events are not held, so text deltas pass unchanged and with no delay. Held data over `HOLD_MAX` is dropped and the stream fails with `upstream event too long`: that is the message the encoder already gives for this input (held event data, not one line), so the client sees no change. Test `split_error_event_escaped_key` (key with its first byte as `t`; before the fix the key reached the client). Test `split_error_event_over_hold_max` (no echo of the held text) passed before the fix too: it guards the limit. The round-2 G3 raw-substring branch stays for one case the joined path does not hold: a keyed `data` line that is no JSON, in an event with no `event: error` line and whose first `data` line does not name `"error"`. `split_error_event_without_key` passes and now goes through the joined path.
- H2 (fixed in fb537ff0, with H1): test `split_prompt_too_long_pc14`. A split `invalid_request_error` event `prompt is too long: …` gets the code `context_length_exceeded` and a message of at most 512 characters with no Cc, Cf, Zl or Zp character. Before the fix the code was `invalid_request_error`.
- H3 (rejected): the drain is proven by `encoder_failure_row_has_final_usage`. With `tokio::spawn(drain(f))` replaced by `drop(f)` that test fails (row `(100, 30, 5, 1)`, want `(100, 30, 5, 4242)`; run again in this round). The two G1 tests prove the client release, not the drain.
- H4 (recorded): DDR-01M8B3A7JCGK6YAE2VZ7P6QZ3C (`docs/decisions/DDR-01M8B3A7JCGK6YAE2VZ7P6QZ3C.md`, kind `accepted-limit`, class `none`), in 98dd5bcb.
- H5 (fixed in 4c063f4b): K§6.4.3 stream obligation 6 states that the client body ends at the terminal event; the Anthropic-route drain in a detached task through the tee, to the end, a read error or the per-read idle timeout; the chat route's stop at the mapper's terminal or error event; that the client gets nothing more; and the one usage row with the counts read.
- H6 (fixed in 4c063f4b): the K§6.4.4 sentence applies the 512-character cap and the Cc/Cf/Zl/Zp replacement to the `prompt is too long` form and the `max_tokens` limit form, before and inside the stream. The change log is in date order (the one 2026-11-26 line moved to the end; no line removed) with the new 2026-11-25 line.
- H7 (fixed in 13d49e93): tests `namespaced_call_mapped_back` (`multi_agent_v1__spawn_agent` returns as `function_call` with name `spawn_agent` and namespace `multi_agent_v1`, streamed and not streamed) and `streamed_request_json_answer_completed_and_row` (ends with `response.completed`, row `(100, 30, 5, 42)`). Both are coverage tests and passed at once. `usage_rows` waits with no total limit: `usage_written` lives in `serve.rs` and takes a `Duration`, so its signature is unchanged; the harness calls it in a loop with a 1 s wait per call until it returns true.
- Results (round 3 dispositions):
  - Check `cargo test -p zygo-proxy --locked`: 521 passed, 0 failed, 15 ignored.
  - Acceptance: exit 0, with 23 tests in `responses_forward`.
  - `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings`: clean.
  - Workspace `--no-fail-fast`: 2454 passed, 2 failed, 778 ignored. The 2 failures are the known `zygo --test guard_hook` ones (`binary_guard_not_a_repository_writes_no_line`, `binary_guard_slow_rev_parse_says_unread_once`).
  - `git diff origin/phase-4 --stat -- crates/zygo-proxy/tests/`: only `responses_forward.rs` and `support/responses.rs`.
  - `responses_forward`: 20 runs, 0 failures.
