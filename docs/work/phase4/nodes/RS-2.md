# RS-2 — Messages → Responses encoder: SSE stream and JSON response

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.3 (stream obligations 1-9), K§6.7 (upstream streams), K§11 items 13, 14 (the after-the-first-event half, at encoder level), 15 and 16; base spec §10.2.6 (`HOLD_MAX`); K-D21, K-D50
**Files:** `crates/zygo-proxy/src/translate/responses_client.rs`, `crates/zygo-proxy/tests/responses_stream.rs` (new), `docs/specs/2026-11-25-client-model-design.md` (K§6.4.3 stream obligations 2, 5, 9 and one change-log line)
**Consumes:** `encode_thinking` (RS-1), `decode_thinking` (RS-1), `request` (RS-1), `Target` (`translate/mod.rs:28`), `SseEvent` (`translate/sse.rs:12`), `Adjustment` (`pipeline.rs:26`), `HOLD_MAX` (`tee.rs:22`), `zygo_core::ids::ulid` (`crates/zygo-core/src/ids.rs:2`)
**Produces:** `translate::responses_client::Calls` with `Calls::of(req: &Value) -> Calls` (the call-item builder; RS-9 extends it), `translate::responses_client::Encoder::new(model: &str, t: &Target, calls: Calls) -> Encoder`, `Encoder::push(&mut self, chunk: &[u8]) -> Vec<SseEvent>` (Messages SSE bytes in, Responses events out), `Encoder::fail(&mut self, code: &str, message: &str) -> Vec<SseEvent>` (one `response.failed` when started and not ended), `Encoder::finish(&mut self) -> Vec<SseEvent>`, `Encoder::ended(&self) -> bool`, `Encoder::held(&self) -> usize`, `Encoder::adjustments(&self) -> Vec<Adjustment>`, `translate::responses_client::response(msg: &Value, model: &str, t: &Target, calls: &Calls) -> (Value, Vec<Adjustment>)` (non-streaming Response object and its codes)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_stream -- --list 2>/dev/null | grep -c ': test$')" -ge 12 && cargo test -p zygo-proxy --locked --test responses_stream` → at least 12 tests (a count of 0 fails), then all pass on ubuntu, macOS and Windows CI: a Messages SSE fixture with text, tool use, thinking and redacted thinking gives the event sequence of stream obligations 1-5 with `sequence_number` rising by one from 0 (K§11 item 13); `server_tool_use` dropped with `server_tool_dropped`; `max_tokens` → `response.incomplete`; an error event after the first event → exactly one `response.failed` and no other terminal event; `ping` and unknown events dropped; an unparsable event after start → `response.failed`; stream obligation 9: a partial line with no newline that grows past `HOLD_MAX`, and `input_json_delta` arguments of one tool call that grow past `HOLD_MAX`, each end the stream with one `response.failed`, and the encoder then holds no buffered bytes; a tool call round trip (encoder output → next request through `request`) keeps `call_id` (K§11 item 15); a thinking round trip to the same provider keeps the signature, to another provider is dropped with its code (K§11 item 16); the JSON form carries the same content

## Planning notes

- Interfaces now: `SseEvent` and `SseEvent::to_bytes` (`translate/sse.rs:12-19`); `StreamMapper::push` (`sse.rs:111-141`) is the pattern for whole-line input and its `HOLD_MAX` bound (`sse.rs:133-139`, `HOLD_MAX` at `tee.rs:22`).
- The `HOLD_MAX` cases feed bytes in chunks from a generator; no case sleeps or measures time.
- The response id is `resp_<ULID>` from `zygo_core::ids::ulid` (as the proxy tests import it).
- This node works on bytes and values only; the HTTP status before the first byte is RS-3.

### Premise check (2026-11-25, against `fe18f3dd`, RS-1 merged)

- **Contract drift, fixed in the contract (§5.4, author's call).** `encode_thinking(t: &Target, block: &Value)` (`translate/reasoning.rs:59`) needs a `Target`: the mark names provider and upstream model, and `decode_thinking` (`reasoning.rs:66-71`) refuses any other. The contract's `Encoder::new(model: &str)` and `response(msg, model)` have none, so they cannot mark a reasoning item. Both now take `t: &Target`. RS-4 builds it with `Target::of(candidate)` (`translate/mod.rs:34`); RS-4 is `TODO` and reads these signatures.
- **Spec gap, fixed in the spec (§5.4, author's call; files outside the original Files line).** Request obligation 4 (`responses_client.rs:461-470`, `:358-363`) sends a client `custom` tool (`apply_patch`, freeform) upstream as a function tool. Stream obligation 2 maps every `tool_use` to a `function_call` item. Codex matches the call item's kind to the tool's registered kind: `fixtures/codex/requests/turn-with-reasoning.json` holds Codex's `custom_tool_call_output` `"unsupported custom tool call: apply_patch"` for a kind it had not registered (PC-3 F1, `nodes/PC-3.md:31-32`, `:151-153`), and the applied patch came only from a `custom_tool_call` item (`fixtures/codex/transport/replies/turn-patch.sse`, `requests/after-custom-tool-call-output.json`). So a model call of `apply_patch` must come back as a `custom_tool_call` item. The encoder learns the client's custom tool names from the client body: `Calls::of(req)`. `Calls` is also the one place that builds a call item, so RS-9 adds its namespace map there (`nodes/RS-9.md:14`). Step 1 rewrites stream obligation 2. The spec file joins the Files line.
- **`Encoder::held`** is added to Produces: the acceptance text requires that "the encoder then holds no buffered bytes", and a test can see that only through an accessor.
- **`response` returns its adjustment codes** (`(Value, Vec<Adjustment>)`): a non-streaming body with a `server_tool_use` block drops it with `server_tool_dropped` too (stream obligation 3 applies to the JSON form by obligation 8), and a bare `Value` cannot carry the code.
- **The terminal event's `output` is `[]`.** Global constraint "no buffer grows with the response" (graph.md, K-C2, stream obligation 9). Each item reaches the client whole in its `response.output_item.done`; the mock replies Codex 0.172.0 consumed carry every item in `output_item.done` (`transport/replies/*.sse`, `scripts/premises/mock_upstream.py:85-101`). Only the current item's text, thinking or arguments is held, for its done events, inside the `HOLD_MAX` bound. Step 1 writes this into obligations 5 and 9.
- **Usage.** The tee reads `message_start` `message.usage`, then cumulative maxima from `message_delta` `usage` (`tee.rs:69-80`, base spec §14). The encoder keeps the same: per field, `message_start` sets it, a later `message_delta` value raises it (max). The §10.2.2 window rewrite only raises `cache_read_input_tokens` (base spec :461-463), so a rewritten `message_delta` (RS-7) reaches `response.completed`.
- **Codex SSE consumed** (PC-3, `transport/replies/*.sse`; `mock_upstream.py:85-101`): `response.created` with `response.id`; `response.output_text.delta` with `item_id`, `output_index`, `content_index`, `delta`; `response.output_item.done` with the whole item (`message` with `output_text` parts and `annotations: []`; `reasoning` with `summary` of `summary_text` and `encrypted_content`; `function_call` with `call_id`, `name`, `arguments`, `status`; `custom_tool_call` with `call_id`, `name`, `input`, `status`); `response.completed` with `response.id` and `usage`. The history Codex sends back (`requests/*.json`) repeats those items as received; `request` ignores their `id` and `status`.
- **No existing builder to reuse.** `message_events` (`sse.rs:436-468`) turns a Messages body into events but maps every unknown block, `redacted_thinking` and `server_tool_use` included, to text (`sse.rs:455`); `response` builds items directly with the same item functions as the encoder.
- **No wall clock.** No event carries `created_at`; no test reads a clock.
- Base spec §10.2.6 (`2026-11-18-zygo-design.md:494-496`) bounds the tee's held line; K§6.4.3 obligation 9 applies the same `HOLD_MAX` to the encoder.
- **Lead's decision (round 1).** Two choices rest on a reading of Codex's behaviour, not on a run: `response.completed` with an empty `output`, and Codex ignoring the encoder's extra events (`output_item.added`, `content_part.*`, the `*.delta` and `*.done` events that PC-3's mock replies did not send). A new real-Codex premise node PC-12 (the lead adds it to graph.md) confirms both; RS-4 depends on it. The event sequence the encoder emits is kept in one place, the doc comment on `Encoder` (`responses_client.rs`), so PC-12 can serve exactly that sequence from the mock.

## Plan

### Interfaces

Consumed, as they are now:

```rust
// crates/zygo-proxy/src/translate/reasoning.rs
pub fn encode_thinking(t: &Target, block: &Value) -> String;            // :59
pub fn decode_thinking(v: &str, t: &Target) -> Option<Value>;           // :66
pub const THINKING_PREFIX: &str = "zygo1t:";                             // :53
// crates/zygo-proxy/src/translate/responses_client.rs
pub fn request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError>; // :104
// crates/zygo-proxy/src/translate/mod.rs
pub struct Target { pub provider: String, pub upstream_model: String } // :28
pub(crate) fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str>;          // :233
// crates/zygo-proxy/src/translate/sse.rs
pub struct SseEvent { pub event: String, pub data: Value }               // :12
impl SseEvent { pub fn to_bytes(&self) -> Vec<u8> }                       // :19
// crates/zygo-proxy/src/pipeline.rs
pub struct Adjustment(pub String);                                       // :26
// crates/zygo-proxy/src/tee.rs
pub const HOLD_MAX: usize = 4 << 20;                                     // :22
// crates/zygo-core/src/ids.rs
pub fn ulid() -> String;                                                 // :2
```

Produced, in `crates/zygo-proxy/src/translate/responses_client.rs`:

```rust
/// What the client's request says about its tools that the call items need (K§6.4.3 stream obligation 2).
#[derive(Debug, Clone, Default)]
pub struct Calls { /* private: the names of the client's `custom` tools */ }
impl Calls {
    /// The `name` of each `tools[]` entry of type `custom` with a string name; anything else is ignored.
    pub fn of(req: &Value) -> Calls;
}

/// Messages SSE → Responses SSE for one response (K§6.4.3 stream obligations 1-7, 9). No `Debug` needed.
pub struct Encoder { /* private */ }
impl Encoder {
    pub fn new(model: &str, t: &Target, calls: Calls) -> Encoder;
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent>;
    pub fn fail(&mut self, code: &str, message: &str) -> Vec<SseEvent>;
    pub fn finish(&mut self) -> Vec<SseEvent>;
    pub fn ended(&self) -> bool;
    pub fn held(&self) -> usize;
    pub fn adjustments(&self) -> Vec<Adjustment>;
}

/// A Messages JSON body as one Response object (stream obligation 8).
pub fn response(msg: &Value, model: &str, t: &Target, calls: &Calls) -> (Value, Vec<Adjustment>);
```

### Fixed values (the tests assert them)

- Ids: response `resp_<ULID>` (`ulid()`, 26 characters). Item id `<p>_<ULID>_<output_index>` with the response's ULID and `p` = `msg` (message), `rs` (reasoning), `fc` (function call), `ctc` (custom tool call). `response()` makes its own ULID.
- Every event's `data` has `"type"` = the event name and `"sequence_number"`, starting at 0 on `response.created` and rising by one per event of this encoder. `SseEvent.event` = the same name.
- Response object R (in `response.created`, `response.in_progress`): `{"id","object":"response","model":<client model>,"status":"in_progress","output":[]}`.
- Items, built only by these private functions, used by both `Encoder` and `response`:
  - text: `{"type":"message","id","role":"assistant","status":"completed","content":[{"type":"output_text","text":T,"annotations":[]}]}`
  - thinking: `{"type":"reasoning","id","summary":[{"type":"summary_text","text":TH}],"encrypted_content":encode_thinking(t, {"type":"thinking","thinking":TH,"signature":SIG})}`; `summary` is `[]` when TH is empty.
  - redacted thinking: `{"type":"reasoning","id","summary":[],"encrypted_content":encode_thinking(t, {"type":"redacted_thinking","data":D})}`
  - `Calls::item` (private method), `tool_use` (id I, name N, arguments A as JSON text): `{"type":"function_call","id","call_id":I,"name":N,"arguments":A,"status":"completed"}`; when N is a client `custom` tool: `{"type":"custom_tool_call","id","call_id":I,"name":N,"input":X,"status":"completed"}`, X = the string field `input` of A parsed, else A as received.
  - The `output_item.added` form of each item has `"status":"in_progress"` (message, calls), empty `content`/`summary`, `arguments: ""` / `input: ""`, and no `encrypted_content`.
- Events per block (`oi` = `output_index`, counted over emitted items from 0; `iid` = item id):
  - text: `response.output_item.added {output_index, item}`, `response.content_part.added {item_id, output_index, content_index:0, part:{"type":"output_text","text":"","annotations":[]}}`, per non-empty `text_delta`: `response.output_text.delta {item_id, output_index, content_index:0, delta}`, at stop: `response.output_text.done {item_id, output_index, content_index:0, text}`, `response.content_part.done {…, part}` (whole part), `response.output_item.done {output_index, item}`.
  - thinking: `output_item.added`; at the first non-empty `thinking_delta`: `response.reasoning_summary_part.added {item_id, output_index, summary_index:0, part:{"type":"summary_text","text":""}}`, then per delta `response.reasoning_summary_text.delta {item_id, output_index, summary_index:0, delta}`; `signature_delta` values are joined, no event; at stop, if a part was opened: `response.reasoning_summary_text.done {…, text}`, `response.reasoning_summary_part.done {…, part}`; then `output_item.done`.
  - redacted thinking (`data` from `content_block_start`): `output_item.added`, `output_item.done`.
  - function call: `output_item.added`; per non-empty `input_json_delta`: `response.function_call_arguments.delta {item_id, output_index, delta}` (implementation: an empty `partial_json`, which Anthropic can send first, gives no event and does not replace the start's `input`); at stop: `response.function_call_arguments.done {item_id, output_index, arguments}`, `output_item.done`. Arguments = the joined `partial_json`; with no delta, the `content_block_start` `input` as compact JSON.
  - custom tool call: `output_item.added`; no event per delta (the JSON text is held); at stop: `response.custom_tool_call_input.delta {item_id, output_index, delta:X}`, `response.custom_tool_call_input.done {item_id, output_index, input:X}`, `output_item.done`.
- Usage U from Messages usage M (input, cache read, cache write, output; absent = 0): `{"input_tokens":in+cr+cw,"input_tokens_details":{"cached_tokens":cr},"output_tokens":out,"total_tokens":in+cr+cw+out}`.
- Terminal: at `message_stop`: `response.completed {response: R with "status":"completed","usage":U}`, or for `stop_reason` `max_tokens`: `response.incomplete {response: R with "status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"usage":U}`. `output` stays `[]`.
- Failure: `response.failed {response: R with "status":"failed","error":{"code","message"}}`. Codes and messages: upstream `error` event → `code` = its `error.type` when 1-32 characters of `[a-z0-9_]` else `api_error`, `message` = its `error.message` (else `""`); unparsable `data` → `api_error`, `upstream event is not JSON`; partial line or held event over the bound with no open tool call → `api_error`, `upstream stream line too long`; a tool call's held arguments over the bound → `api_error`, `upstream tool call too long`; a text or thinking block's held text over the bound → `api_error`, `upstream content block too long`; `finish` with no terminal → `api_error`, `upstream stream ended early`.
- Bound: partial line + the current event's `data` lines + the current item's held text, thinking, signature or arguments ≤ `HOLD_MAX` (implementation: within one push, the lines of the chunk already read are not counted; test `stream_one_chunk_of_small_blocks_over_hold_max_completes`, added in step 6, holds that). Over it: all of them are freed (`held() == 0`), then `response.failed`.
- Start: the first event that is not `ping` and not of unknown type starts the stream (`response.created`, `response.in_progress`); an `error` event or unparsable `data` as that first event gives start, then `response.failed`. `fail` before start returns `[]` (the caller still owns the HTTP status, K§6.4.4) and leaves `ended()` false; `finish` before start gives start, then `response.failed` `upstream stream ended early`. After a terminal event, `push`, `fail` and `finish` return `[]`.
- Dropped, no event: `ping`; an event whose `type` is not one of `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`, `error`, `ping`; a delta whose `index` is not the open block's or whose delta type is not the block's; a `content_block_start` of another type than `text`, `thinking`, `redacted_thinking`, `tool_use`. `server_tool_use` and each block type ending in `_tool_result`: dropped with their deltas, and `adjustments()` = `[Adjustment("server_tool_dropped")]` (once).

### Steps

- [x] 1. Rewrite K§6.4.3 in `docs/specs/2026-11-25-client-model-design.md` as final text (§5.4): stream obligation 2 adds "a `tool_use` of a tool the client sent as `custom` → a `custom_tool_call` item (`output_item.added`, `custom_tool_call_input.delta`, `custom_tool_call_input.done`, `output_item.done` with `input` = the tool input's `input` string, else its JSON text); Codex matches the item kind to the tool kind (PC-3)"; obligation 5 adds "the terminal event's `response.output` is empty: each item reached the client in its `output_item.done`"; obligation 9 adds "the encoder holds only the partial line, the current event and the current item's text, thinking or arguments, inside `HOLD_MAX`; over it the stream ends with `response.failed`". Append to `## Change log`: `- 2026-11-25: K§6.4.3 stream obligations 2, 5, 9 — custom tool calls return as custom_tool_call items; the terminal event carries no output copy; the encoder's held bytes (RS-2 premise check); author's call.` Check: `git diff --stat` shows only the spec file; commit `docs(spec): K§6.4.3 stream rules from RS-2 premise check`.
- [x] 2. Create `crates/zygo-proxy/tests/responses_stream.rs` with helpers: `tgt()` = `Target{provider:"p-ant",upstream_model:"up-ant"}`, `other()` = `Target{provider:"p-oai",upstream_model:"up-oai"}`; `sse(events: &[Value]) -> String` (each `event: <type>\ndata: <json>\n\n`); `fixture() -> Vec<Value>` = `message_start` (usage `{"input_tokens":10,"cache_read_input_tokens":5,"cache_creation_input_tokens":2,"output_tokens":1}`), block 0 `thinking` (`thinking_delta` "t1", `signature_delta` "sig1"), block 1 `redacted_thinking` (`data` "R1"), block 2 `text` (`text_delta` "Hel", "lo"), block 3 `tool_use` (`id` "toolu_1", `name` "exec_command", `input_json_delta` `{"cmd":` and `"ls"}`), `message_delta` (`stop_reason` "tool_use", usage `{"output_tokens":7}`), `message_stop`; `enc()` = `Encoder::new("role-x", &tgt(), Calls::default())`; `norm(v)` removes every `id` and `item_id` key recursively (for comparisons across encoders); `types(evs)` = the event names. Each test below is written in this step; the file compiles once step 4 defines the types (stubs that `todo!()` are allowed in step 3).
  - `stream_sequence_text_tool_thinking_redacted` (K§11 item 13): `types` equals exactly `response.created, response.in_progress, output_item.added, reasoning_summary_part.added, reasoning_summary_text.delta, reasoning_summary_text.done, reasoning_summary_part.done, output_item.done, output_item.added, output_item.done, output_item.added, content_part.added, output_text.delta, output_text.delta, output_text.done, content_part.done, output_item.done, output_item.added, function_call_arguments.delta, function_call_arguments.delta, function_call_arguments.done, output_item.done, response.completed` (each but the two `response.*` firsts and the last with the `response.` prefix); `data["sequence_number"] == i` for each index i; `data["type"] == event`; `response.id` matches `^resp_[0-9A-Z]{26}$`, `model == "role-x"`, `status == "in_progress"`; the four `output_item.done` items have `output_index` 0..=3, and each item's events share `output_index` and `item_id`/`item.id`; item 0 `summary == [{"type":"summary_text","text":"t1"}]` and `decode_thinking(encrypted_content, &tgt()) == Some({"type":"thinking","thinking":"t1","signature":"sig1"})`; item 1 `summary == []`, decodes to `{"type":"redacted_thinking","data":"R1"}`; item 2 content `[{"type":"output_text","text":"Hello","annotations":[]}]`; item 3 `{"type":"function_call","call_id":"toolu_1","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","status":"completed"}`; `response.completed` `status == "completed"`, `output == []`, `usage == {"input_tokens":17,"input_tokens_details":{"cached_tokens":5},"output_tokens":7,"total_tokens":24}`; `ended()`; `adjustments() == []`.
  - `stream_chunked_equals_whole`: the fixture fed in 1-byte and in 7-byte chunks (with `\r\n` line ends in the 7-byte run) gives `norm` of the whole-feed events.
  - `stream_server_tool_dropped` (K§11 item 13): text "a", `server_tool_use` (with an `input_json_delta`), `web_search_tool_result`, text "b" → two message items at `output_index` 0 and 1, no other item, `sequence_number` rising by one; `adjustments() == [Adjustment("server_tool_dropped".into())]`.
  - `stream_max_tokens_incomplete`: `stop_reason` `max_tokens` → last event `response.incomplete` with `status "incomplete"`, `incomplete_details == {"reason":"max_output_tokens"}`, `usage` present; no `response.completed`.
  - `stream_error_after_text_one_failed` (K§11 item 14, review focus 9): text delta, then `{"type":"error","error":{"type":"overloaded_error","message":"busy"}}`, then `message_stop` → exactly one terminal (`response.failed`, `error == {"code":"overloaded_error","message":"busy"}`), it is the last event, no `response.completed`; later `push`, `fail`, `finish` give `[]`; `ended()`. A second case: `error.type` `"Bad Type!"` → `code "api_error"`.
  - `stream_ping_and_unknown_dropped` (review focus 9): `ping` and `{"type":"future_event"}` between every fixture event → `norm` equal to the plain fixture's events.
  - `stream_unparsable_after_start_fails` (review focus 9): after `message_start`, `data: {not json` → one `response.failed` (`api_error`, `upstream event is not JSON`), `ended()`, `sequence_number` still rising by one.
  - `stream_error_before_start`: first event an `error` → `response.created`, `response.in_progress`, `response.failed`, sequence 0, 1, 2.
  - `stream_partial_line_over_hold_max_fails` (obligation 9, review focus 9): after `message_start`, push `data: ` then 64 KiB chunks of `x` with no newline from an iterator until `HOLD_MAX` is passed → across all outputs exactly one `response.failed` (`upstream stream line too long`), `held() == 0`, `ended()`; one more chunk gives `[]` and `held() == 0`.
  - `stream_tool_args_over_hold_max_fails` (obligation 9, review focus 9): a `tool_use` block, then whole `input_json_delta` events of 64 KiB each until their sum passes `HOLD_MAX` → exactly one `response.failed` (`upstream tool call too long`), `held() == 0`, `ended()`.
  - `stream_fail_and_finish`: `fail("upstream_error","gone")` before start → `[]`, `!ended()`; after `message_start` → one `response.failed` with that code and message; a second `fail` → `[]`. A truncated fixture (no `message_stop`): `finish()` → one `response.failed` `upstream stream ended early`. `finish()` on a new encoder → created, in_progress, failed.
  - `tool_call_round_trip_keeps_call_id` (K§11 item 15): the fixture's `function_call` item (from `output_item.done`) plus `{"type":"function_call_output","call_id":"toolu_1","output":"ok"}`, after a user message, through `request(&req, &tgt(), None)` with `max_output_tokens: 100` and the `exec_command` function tool → assistant `tool_use` `id == "toolu_1"`, `input == {"cmd":"ls"}`; user `tool_result` `tool_use_id == "toolu_1"`.
  - `custom_tool_call_round_trip`: `Calls::of` a body with tool `{"type":"custom","name":"apply_patch"}`; a `tool_use` `apply_patch` with `input_json_delta` `{"input":"*** Begin Patch\n*** End Patch\n"}` → `output_item.done` item `{"type":"custom_tool_call","call_id":…,"name":"apply_patch","input":"*** Begin Patch\n*** End Patch\n","status":"completed"}`, events `output_item.added, custom_tool_call_input.delta, custom_tool_call_input.done, output_item.done`; the item and its `custom_tool_call_output` through `request` → `tool_use` `input == {"input":"*** Begin Patch\n*** End Patch\n"}` with the same id. `Calls::of` ignores a `custom` entry with no string name and a `function` entry of the same name.
  - `thinking_round_trip_same_provider_keeps_signature` (K§11 item 16): items 0 and 1 of the fixture between a user message and an assistant `output_text` message, through `request` with `tgt()` → the assistant message starts with `{"type":"thinking","thinking":"t1","signature":"sig1"}` then `{"type":"redacted_thinking","data":"R1"}`; no `reasoning_dropped` code.
  - `thinking_round_trip_other_provider_dropped` (K§11 item 16): the same input through `request` with `other()` → no thinking block, adjustments contain `Adjustment("reasoning_dropped:2")`.
  - `json_response_same_content` (obligation 8): the Messages body with the fixture's content (`thinking` t1/sig1, `redacted_thinking` R1, `text` "Hello", `tool_use` toolu_1 `{"cmd":"ls"}`, a `server_tool_use` block), `stop_reason` `tool_use`, the fixture's usage summed → `response(...)`: `id` matches `^resp_`, `object "response"`, `status "completed"`, `norm(output)` equals `norm` of the stream's four `output_item.done` items, `usage` as in the stream test, codes `[server_tool_dropped]`; with `stop_reason` `max_tokens` → `status "incomplete"`, `incomplete_details.reason "max_output_tokens"`.
- [x] 3. Run `cargo test -p zygo-proxy --locked --test responses_stream` (CARGO_INCREMENTAL=0) → fails to compile or each test fails (`Calls`, `Encoder`, `response` missing). Commit the test file: `test(proxy): RS-2 Responses encoder tests`.
- [x] 4. In `responses_client.rs` add `Calls` and `Calls::of`, the private item functions and `Calls::item` (the only call-item builder), the usage function U, and `response(...)` per "Fixed values". Imports: `super::reasoning::encode_thinking`, `super::sse::SseEvent`, `crate::tee::HOLD_MAX`, `zygo_core::ids::ulid`.
- [x] 5. Add `Encoder` per "Fixed values": line splitting as `StreamMapper::push` (`sse.rs:111-141`, each byte scanned once, `\r` stripped, multi-line `data` joined with `\n`, event dispatched on its blank line by the data's `type`); one open block at a time keyed by the Messages `index`; usage maxima as the tee; the `HOLD_MAX` bound checked after each push and each delta, freeing all held bytes before `response.failed`. Doc comments name K§6.4.3 stream obligations.
- [x] 6. Run `cargo test -p zygo-proxy --locked --test responses_stream` → all pass. Run the Acceptance command alone → the `--list` count is at least 12 and all pass.
- [x] 7. Run `cargo test -p zygo-proxy --locked --test responses_request --test translate` → pass (RS-1 and the existing mapper unchanged). Commit `feat(proxy): RS-2 Messages to Responses encoder and JSON response`.
- [x] 8. Verification floor in the checkout: `cargo test --workspace --locked --no-fail-fast` → exit 0 apart from the known `zygo --test guard_hook` environment failures (name each failing test in the node's output). Push.

### Round 1 output (2026-11-25)

- Acceptance alone: `--list` count 17 (≥ 12); `responses_stream` 17 passed, 0 failed; exit 0.
- `cargo test -p zygo-proxy --locked --test responses_request --test translate`: 49 and 48 passed.
- `cargo test --workspace --locked --no-fail-fast`: 2374 passed, 2 failed, both the known `zygo --test guard_hook` environment failures under `/private/tmp`: `binary_guard_not_a_repository_writes_no_line`, `binary_guard_slow_rev_parse_says_unread_once`.
- `cargo clippy -p zygo-proxy --all-targets --locked -- -D warnings`: clean.

### Author's checks (2026-11-25)

1. Spec coverage: stream obligation 1 → step 2 sequence test (created, in_progress, id, model, status); 2 → sequence, custom, JSON tests; 3 → server-tool test; 4 → `sequence_number`/`output_index`/`item_id` asserts in the sequence, unparsable and error tests; 5 → sequence (completed, usage) and max-tokens tests; 6 (after start) → error-after-text, error-before-start, fail tests (before the first byte is RS-3/RS-4); 7 → ping/unknown and unparsable tests; 8 → JSON test (the error status half is RS-4); 9 → two `HOLD_MAX` tests; K§6.7 upstream streams → same; K§11 13-16 → named tests.
2. Step scan: each step names one file and one result; no body is given for code that the fixed values and tests decide.
3. Interface match: `request`, `encode_thinking`, `decode_thinking` match `responses_client.rs:104`, `reasoning.rs:59`, `:66`; RS-4 consumes `Encoder::new` and `response` with the signatures above (both `TODO`/unplanned); RS-9 extends `Calls::of` and `Calls::item`.
4. Review focus 9 (graph.md) → `stream_error_after_text_one_failed`, `stream_ping_and_unknown_dropped`, `stream_unparsable_after_start_fails`, the two `HOLD_MAX` tests; strictly rising `sequence_number` is asserted in each.
5. Proportion: the plan fixes wire values and assertions; the encoder body is left to the implementer.
6. Live code: every cited path and line opened at `fe18f3dd`; no existing Messages → Responses builder (`message_events`, `sse.rs:436`, goes the other way and folds unknown blocks into text).
