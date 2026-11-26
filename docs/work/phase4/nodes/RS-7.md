# RS-7 — Window rewrite for a Responses client, Anthropic and Responses upstreams

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stage table row window; the encoder runs after the tee), K§6.4.5 (the format of the rewrite; arming is client policy), K§11 item 21; base spec §10.2.2, §10.2.6
**Files:** `crates/zygo-proxy/src/window.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/responses_window.rs` (new)
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3), `ResponseCtx.lines` (RS-5), `tee::responses_usage` (RS-5)
**Produces:** `window::rewrite_responses_usage(line: &[u8], u: Usage, window: u64) -> Option<Vec<u8>>` (K§6.4.5: §10.2.2 with the Responses field names, on a `response.completed` or `response.incomplete` line)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_window -- --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-proxy --locked` → at least 4 tests in `responses_window` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: armed, `response.completed` usage follows §10.2.2 for an Anthropic upstream (rewrite on the Messages `message_delta` before the encoder) and for a Responses upstream (rewrite on `response.completed`); unarmed, the bytes pass unchanged (K§11 item 21); the existing `window` tests pass unchanged

## Planning notes

- Premise drift (2026-11-25 premise check). The earlier note said that forward.rs needs no change. It does: on a translated Responses stream, the line callback gives each line only to `Streamed::line` (the K§6.4.4 error rewrite), never to the line stages (`forward.rs:1511-1515`). So the window stage never sees the Anthropic or chat `message_delta`. The tee needs no change: `Tee::scan` reads each line's usage before it calls the line callback, and `forward.rs:1510` already puts the counts in `ResponseCtx.usage`. Files: tee.rs out, forward.rs in. K§6.4.5 is rewritten as final text (change log 2026-11-25): the Responses-form edits, `response.incomplete`, and the trigger 1 and trigger 2 rules on Responses lines.
- The stage tells the two forms apart by `ResponseCtx.lines` (RS-5): `Openai` → the new Responses branch; any other → the existing Messages branch (`window.rs:139-181`).
- Until P4-6, arming is today's per-client rule (`window.rs:129-134`: a Claude Code session, no `x-claude-code-compaction` header, a served model with a context window), extended from `ModelRoute::Messages` to `ModelRoute::Responses`. So the test session (Claude Code) is armed, and an OpenCode session is the unarmed case. The Codex arming rule (K§6.4.5, PC6) and the descriptor field come in P4-9 and P4-6.
- Every Responses stream with an encoder already runs in line mode (`forward.rs:609`, for the error rewrite). So the forward step needs its own "the window stage armed" flag, taken from `RequestCtx.line_mode`.
- Trigger 1 reads the `x-claude-code-request-class` header (`window.rs:162`). On the Responses route it fires only when that header is present, as today.
- The rewrite works in whole SSE lines within the `HOLD_MAX` bound (`tee.rs:23`). A terminal line over the bound passes unchanged.
- Worked numbers for the harness model (`gpt-mock`, `context_window` 200000, not a `claude-` id, so `agent_window` is 1000000): safety 180000, window 1000000. An upstream prompt of 190000 with 150000 cached gives the tee counts input 40000, cache read 150000, cache write 0. The cache read is raised to 960000, so the client sees `input_tokens` 1000000, `cached_tokens` 960000, and `total_tokens` 1000000 + output.

## Interfaces

Consumed (as in the code at `8c7e3290`):

```rust
// crates/zygo-proxy/src/pipeline.rs:29-35, :84-102
pub enum ModelRoute { Messages, CountTokens, Responses }
pub struct ResponseCtx {
    pub session: Arc<SessionState>, pub cfg: Arc<Merged>, pub headers: HeaderMap, pub status: StatusCode,
    pub resolved: Resolved, pub provider: String, pub dialect: Dialect,
    /// Openai only for a Responses upstream answering a Responses client (K§6.4.2 C3), else Anthropic.
    pub lines: Dialect,
    pub adjustments: Vec<Adjustment>, pub usage: Usage, pub compact_call: bool,
}
// crates/zygo-proxy/src/tee.rs:30, :33-38
pub struct SseLine(pub Vec<u8>);
pub struct Usage { pub input: u64, pub output: u64, pub cache_read: u64, pub cache_write: u64 }
// crates/zygo-proxy/src/translate/responses_client.rs:514
pub fn flat_name(ns: &str, name: &str) -> String;
// crates/zygo-proxy/src/window.rs:94, :102, :241, :266
pub fn compact_tool(name: &str) -> bool;
pub fn soft_threshold(compact: Option<WindowArg>, row: &ModelRow, safety: Option<u64>) -> Option<u64>;
fn edit_obj(o: &Obj, start: usize, off: &dyn Fn(&RawValue) -> usize, vals: &[(&str, u64); 3], edits: &mut Vec<Edit>) -> Option<()>;
pub fn rewrite_usage(line: &[u8], u: Usage, window: u64) -> Option<Vec<u8>>;
// crates/zygo-proxy/tests/support/responses.rs:211, :331, :338, :403, :412
pub fn start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness;
pub fn post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes);
pub fn post_as(&self, path: &str, headers: &[(&str, &str)], body: &Value) -> (StatusCode, HeaderMap, Bytes);
pub fn usage_rows(&self) -> Vec<RequestRow>;
pub fn codex_request(name: &str) -> Value;
```

Produced:

```rust
// crates/zygo-proxy/src/window.rs
/// The `response.completed` or `response.incomplete` data line with `response.usage` set by K§6.4.5 so that the
/// client's measure equals `window`: `input_tokens_details.cached_tokens` := max(window − input − cache_write,
/// cache_read), `input_tokens` := input + cache_write + cached_tokens, `total_tokens` := input_tokens + u.output;
/// a missing field inserted; every other byte kept. `None` (line unchanged): not UTF-8, no `data:` prefix, another
/// `type`, no `response.usage` object, `input_tokens_details` not an object, or an edited object repeats a key.
pub fn rewrite_responses_usage(line: &[u8], u: Usage, window: u64) -> Option<Vec<u8>>;
```

## Plan

All tests are in the new file `crates/zygo-proxy/tests/responses_window.rs`. It starts with `#[path = "support/responses.rs"] mod support;` and uses `Harness` (a mock upstream on 127.0.0.1 and a `ManualClock`; no agent binary, no network, no sleep). It copies these helpers unchanged:

- `sse`, `counts` and `resp_events` from `tests/responses_passthrough.rs:27-29, :45-47, :57-85` (with `r0` and `msg_done`);
- `msg_sse`, `chat_sse`, `events`, `terminals` and `chunk` from `tests/responses_forward.rs:28-65, :167-170`.

New helpers (no logic under test):

```rust
const R: &str = "openai/v1/responses";
const SSE: &str = "text/event-stream";
/// The client's usage after a rewrite to window 1000000 with upstream input 40000, cached 150000, output 30.
fn rewritten() -> Value {
    json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":960000},"output_tokens":30,"total_tokens":1000030})
}
/// The `context_*` adjustments of a row.
fn codes(r: &zygo_usage::RequestRow) -> Vec<&str> {
    r.adjustments.iter().map(String::as_str).filter(|a| a.starts_with("context_")).collect()
}
/// Anthropic SSE: message_start with `start_usage`; one text block "hi", or one tool_use block named `tool` when
/// `Some`; message_delta {"stop_reason": stop} with usage {"output_tokens":30}; message_stop.
fn anthropic_stream(start_usage: Value, tool: Option<&str>, stop: &str) -> Vec<u8>;
/// `sse(&resp_events(terminal, status, usage))`, with each item of `calls` sent as one `response.output_item.added`
/// before the terminal event and appended to the terminal `response.output`.
fn responses_stream(terminal: &str, status: &str, calls: Vec<Value>, usage: Value) -> Vec<u8>;
/// `authorization: Bearer <token>` and `x-zygo-session: <session>`, then `extra`.
fn auth<'a>(h: &'a Harness, bearer: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)>;
```

Two more helpers hold the upstream usage and its rewrite:

```rust
fn usage_up() -> Value {
    json!({"input_tokens":190000,"input_tokens_details":{"cached_tokens":150000},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":190030})
}
fn usage_down() -> Value {
    json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":960000},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":1000030})
}
```

`Value::to_string` and `sse` serialize the same way, so the compact text of `usage_up()` occurs once in a stream, inside the terminal event. Each streaming post sends `codex_request("turn-with-reasoning")` (`"stream": true`).

- [x] **1. Write the unit test of the rewrite** (`rewrite_responses_usage_sets_three_numbers_only`):

```rust
use zygo_proxy::tee::Usage;
use zygo_proxy::window::rewrite_responses_usage;
let u = Usage { input: 40000, output: 30, cache_read: 150000, cache_write: 0 };
let line = |ty: &str, usage: &str| format!(r#"data: {{"type":"{ty}","sequence_number":6,"response":{{"id":"resp_up","status":"completed","output":[],"usage":{usage}}}}}"#);
let (up, down) = (usage_up().to_string(), usage_down().to_string());
for ty in ["response.completed", "response.incomplete"] {
    assert_eq!(rewrite_responses_usage(line(ty, &up).as_bytes(), u, 1_000_000), Some(line(ty, &down).into_bytes()), "{ty}");
}
for ty in ["response.failed", "response.created", "message_delta"] {
    assert_eq!(rewrite_responses_usage(line(ty, &up).as_bytes(), u, 1_000_000), None, "{ty}");
}
for usage in ["null", r#"{"input_tokens":1,"input_tokens":2}"#, r#"{"input_tokens":1,"input_tokens_details":7}"#,
              r#"{"input_tokens":1,"input_tokens_details":{"cached_tokens":1,"cached_tokens":2}}"#] {
    assert_eq!(rewrite_responses_usage(line("response.completed", usage).as_bytes(), u, 1_000_000), None, "{usage}");
}
let bare = line("response.completed", &up);
assert_eq!(rewrite_responses_usage(bare.trim_start_matches("data: ").as_bytes(), u, 1_000_000), None);
// Missing fields are inserted; the order of the members is not fixed.
let usage_of = |b: Vec<u8>| serde_json::from_slice::<Value>(&b[5..]).unwrap()["response"]["usage"].clone();
let u2 = Usage { input: 190000, output: 30, cache_read: 0, cache_write: 0 };
let no_details = r#"{"input_tokens":190000,"output_tokens":30,"total_tokens":190030}"#;
assert_eq!(usage_of(rewrite_responses_usage(line("response.completed", no_details).as_bytes(), u2, 1_000_000).unwrap()),
    json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":810000},"output_tokens":30,"total_tokens":1000030}));
assert_eq!(usage_of(rewrite_responses_usage(line("response.completed", "{}").as_bytes(), u2, 1_000_000).unwrap()),
    json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":810000},"total_tokens":1000030}));
```

- [x] **2. Write the armed Messages-form test** (`armed_anthropic_and_chat_upstreams_rewrite_before_the_encoder`). For `d` in `[Dialect::Anthropic, Dialect::OpenaiChat]`, use `Harness::start(d, Agent::ClaudeCode, "")`.
  - Anthropic reply: `anthropic_stream(json!({"input_tokens":40000,"cache_read_input_tokens":150000,"cache_creation_input_tokens":0,"output_tokens":1}), None, "end_turn")`.
  - Chat reply: `chat_sse(&[chunk(json!({"role":"assistant","content":"hi"}), Value::Null), chunk(json!({}), json!("stop")), json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs","choices":[],"usage":{"prompt_tokens":190000,"completion_tokens":30,"total_tokens":190030,"prompt_tokens_details":{"cached_tokens":150000}}})])`.

```rust
let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
assert_eq!(s.as_u16(), 200, "{d:?}");
let evs = events(&b);
let t = terminals(&evs);
assert_eq!(t.len(), 1, "{d:?}");
assert_eq!(t[0].0, "response.completed", "{d:?}");
assert_eq!(t[0].1["response"]["usage"], rewritten(), "{d:?}");
let rows = h.usage_rows();
assert_eq!(counts(&rows[0]), (40000, 150000, 0, 30), "{d:?}"); // upstream counts, before the rewrite
assert_eq!(codes(&rows[0]), ["context_safety"], "{d:?}");
```

- [x] **3. Write the armed Responses-form test** (`armed_responses_upstream_rewrites_completed_usage`). Use `Harness::start(Dialect::Openai, Agent::ClaudeCode, "")` and reply `up = responses_stream("response.completed", "completed", vec![], usage_up())`. The client bytes are `up` with only the usage text changed:

```rust
let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
assert_eq!(s.as_u16(), 200);
let want = String::from_utf8(up).unwrap().replace(&usage_up().to_string(), &usage_down().to_string());
assert_eq!(String::from_utf8(b.to_vec()).unwrap(), want);
let rows = h.usage_rows();
assert_eq!(counts(&rows[0]), (40000, 150000, 0, 30));
assert_eq!(codes(&rows[0]), ["context_safety"]);
```

- [x] **4. Write the unarmed test** (`unarmed_bytes_pass_unchanged`). It uses the upstream usage of steps 2 and 3, which fires trigger 0 when armed.
  - (a) `Agent::OpenCode` with an `Openai` upstream: `assert_eq!(&b[..], &up[..])` and `codes(&rows[0])` is empty.
  - (b) `Agent::OpenCode` with an `Anthropic` upstream: the terminal `response.usage` is `json!({"input_tokens":190000,"input_tokens_details":{"cached_tokens":150000},"output_tokens":30,"total_tokens":190030})`, and `codes(&rows[0])` is empty.
  - (c) `Agent::ClaudeCode` with an `Openai` upstream, sent by `h.post_as(R, &auth(&h, &bearer, &[("x-claude-code-compaction", "1")]), &body)`: `assert_eq!(&b[..], &up[..])`. Read no rows here, because `post_as` counts no post.

- [x] **5. Write the trigger 2 test** (`compact_call_rewrites_on_both_forms`). The upstream prompt is 1000, with nothing cached and output 30, so trigger 0 does not fire. Let `compact_usage` be `json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":999000},"output_tokens":30,"total_tokens":1000030})`.
  - (a) Anthropic upstream, reply `anthropic_stream(json!({"input_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}), Some("mcp__zygo__compact"), "tool_use")`: the terminal `response.usage` equals `compact_usage`, and `codes(&rows[0]) == ["context_compact"]`.
  - (b) Openai upstream, `calls = vec![json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"compact","namespace":"mcp__zygo","arguments":"{}","status":"completed"})]` and usage `{"input_tokens":1000,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"total_tokens":1030}`: the terminal `response.usage` equals `compact_usage`, and `codes(&rows[0]) == ["context_compact"]`.
  - (c) The same stream as (b) with `"namespace":"mcp__zygox"`: the bytes pass unchanged, and `codes(&rows[0])` is empty.

- [x] **6. Write the trigger 1 test on the Responses form** (`soft_window_fires_at_end_of_turn_on_responses_form`).
  - Use `Harness::start(Dialect::Openai, Agent::ClaudeCode, "")`. Set `let bearer = format!("Bearer {}", h.token());`.
  - Set the override: `h.post_as(&format!("v1/zygo/sessions/{}/compact", h.session_id()), &auth(&h, &bearer, &[]), &json!({"window":100000}))`. It returns status 200.
  - Use the usage `{"input_tokens":120000,"input_tokens_details":{"cached_tokens":100000},"output_tokens":30,"total_tokens":120030}`. The prompt is 120000: over the threshold 100000 and under the safety 180000.
  - Each post is `h.post_as(R, &auth(&h, &bearer, &extra), &body)`.
    - (a) `extra = [("x-claude-code-request-class", "main")]`, no call item: the terminal `response.usage` is `json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":980000},"output_tokens":30,"total_tokens":1000030})`.
    - (b) The same header, with `calls = vec![json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"exec_command","arguments":"{}","status":"completed"})]`: the bytes pass unchanged.
    - (c) No header and no call item: the bytes pass unchanged.

- [x] **7. Run** `cargo test -p zygo-proxy --test responses_window`. Expected: a compile error, because `rewrite_responses_usage` does not exist. Comment out step 1 and run again. Expected: steps 2, 3, 5(a), 5(b) and 6(a) fail on their usage assertions; step 4 passes. Then restore step 1.

- [x] **8. Implement in `crates/zygo-proxy/src/window.rs`:**
  - `WindowStage::on_request`: change `ctx.route == ModelRoute::Messages` to `matches!(ctx.route, ModelRoute::Messages | ModelRoute::Responses)`. In the doc comment, change "Messages" to "Messages or Responses (K§6.4.5)".
  - `edit_obj`: change `vals: &[(&str, u64); 3]` to `vals: &[(&str, u64)]`. Make no other change to `rewrite_usage`.
  - Add `pub fn rewrite_responses_usage` as in Produced. Build it on `Obj`, `Key` and `edit_obj`, the same way `rewrite_usage` is built. The edits never overlap: `usage` gets `input_tokens` and `total_tokens` (and `input_tokens_details` when it is missing), and `input_tokens_details` gets `cached_tokens`.
  - Split `on_response_line`. After the status check, dispatch on `ctx.lines`: `Dialect::Openai` → `fn responses_line(ctx: &mut ResponseCtx, line: &mut SseLine)`; any other → `fn messages_line(ctx: &mut ResponseCtx, line: &mut SseLine)`, which is today's body with the same behavior. Both call two new private helpers:
    - `fn triggers(ctx: &ResponseCtx, end_turn: bool) -> Option<(u64, [bool; 3])>`: the window target and (t0, t1, t2), computed as `window.rs:157-167` does now, with `end_turn` in place of the `stop_reason` test. It returns `None` when there is no served row or no context window.
    - `fn record(ctx: &mut ResponseCtx, fired: [bool; 3])`: the code loop of `window.rs:173-177`.
  - `responses_line` acts only on lines whose `data:` contains `response.output_item.added`, `response.completed` or `response.incomplete`:
    - `response.output_item.added` with `item.type == "function_call"`: set `ctx.compact_call` when `compact_tool(&name)`, where `name` is `flat_name(ns, item.name)` when `item.namespace` is a string, else `item.name`.
    - `response.completed` or `response.incomplete`: `end_turn` is true when the type is `response.completed` and no `response.output[*].type` is `function_call` or `custom_tool_call`. Call `triggers`. When a trigger fired and `rewrite_responses_usage(&line.0, ctx.usage, window)` returns `Some`, replace the line and `record` the codes.

- [x] **9. Implement in `crates/zygo-proxy/src/forward.rs`:**
  - `struct Flow` (`forward.rs:1418-1433`): add `/// The window stage armed line mode at request time (§10.2.6): a Responses stream's lines also reach the line stages (K§6.4.5).` `window: bool,`. Set it to `window: ctx.line_mode` in the `Flow { … }` literal (`forward.rs:637`).
  - `Flow::step` (`forward.rs:1507-1516`): destructure `window` too. Change the `Some(Encode::Stream(s))` arm to `{ s.line(l); if *window { pipeline.on_response_line(rctx, l) } }`. Replace the comment at `forward.rs:1512-1513` with: "A Responses stream: its error lines (K§6.4.4), then the line stages when the window stage armed them (K§6.4.5)."

- [x] **10. Run** `cargo test -p zygo-proxy --test responses_window`. Expected: 6 passed, 0 failed.

- [x] **11. Run the Acceptance check:**

```sh
test "$(cargo test -q -p zygo-proxy --test responses_window -- --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-proxy --locked
```

  Expected: exit 0, with 6 tests listed in `responses_window` and every `zygo-proxy` target passing (`window`, `responses_passthrough`, `responses_forward` and `responses_route` unchanged). Then run `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings`. Expected: no warnings.

- [x] **12. Commit** `feat(proxy): RS-7 window rewrite for a Responses client`. End the message with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:RS-7`.

## Implementation notes

- RS-6 moved the forward.rs lines: `struct Flow` is at `forward.rs:1427`, the `Flow { … }` literal at `:639`, `Flow::step` at `:1513`. The change is as in step 9.
- Step 7, red run with step 1 commented out: steps 2, 3, 5 and 6 failed on their usage assertions, and step 4 passed. Step 5(b) did not run, because 5(a) fails first in the same test.
- `rewrite_responses_usage`, when `input_tokens_details` is missing: the insertion is pushed after the `edit_obj` edits of `usage`, so at the same offset it lands first. No spec change.

## Round 1 review

- **F1** (end of turn read only the terminal `response.output`). Fixed in `d07d7ab9`: `ResponseCtx.call_item` (`pipeline.rs`) records a call item from a streamed `response.output_item.added` or `.done`. End of turn is a `response.completed` with no call item in the stream and none in the terminal output. The call items are `function_call`, `custom_tool_call`, `local_shell_call`, `computer_call`, `mcp_call` and `mcp_approval_request` (`window.rs` `CALL_ITEMS`). K§6.4.5 end-of-turn sentence rewritten as final text. Test: `end_of_turn_uses_streamed_call_items`. It failed first: the `function_call` case, with an empty terminal output, was rewritten.
- **F2** (`total_tokens` used the tee's output count, 0 for a non-integer `output_tokens`). Fixed in `d07d7ab9`: `rewrite_responses_usage` returns `None` when a present `input_tokens`, `cached_tokens` or `output_tokens` is not a u64 integer. K§6.4.5 gets the clause. Test: `non_integer_usage_leaves_the_line_unchanged` (`3e1`, `1.9e5`, `150000.0`). It failed first: the `3e1` line was rewritten.
- **F3** (test gaps). Fixed in `d07d7ab9`. `custom_tool_call` end of turn: covered by F1's test. `incomplete_usage_rewritten_on_both_forms`: a `max_tokens` stop through the encoder and a Responses `response.incomplete`, each with the rewritten usage. `soft_window_fires_at_end_of_turn_on_responses_form` asserts `context_signal` in the usage row. `soft_window_on_anthropic_upstream_fires_at_end_turn_only`: trigger 1 with `x-claude-code-request-class: main`. These tests passed on first run because the behavior was already present; they add coverage only.
- Spec change-log line: 2026-11-25, K§6.4.5, author's call.
- Outside the listed files: `crates/zygo-proxy/src/pipeline.rs` holds `ResponseCtx`, so the new field is declared there.
