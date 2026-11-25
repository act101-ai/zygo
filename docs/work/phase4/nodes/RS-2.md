# RS-2 — Messages → Responses encoder: SSE stream and JSON response

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.3 (stream obligations 1-9), K§6.7 (upstream streams), K§11 items 13, 14 (the after-the-first-event half, at encoder level), 15 and 16; K-D21, K-D50
**Files:** `crates/zygo-proxy/src/translate/responses_client.rs`, `crates/zygo-proxy/tests/responses_stream.rs` (new)
**Consumes:** `mark_reasoning` (RS-1), `unmark_reasoning` (RS-1), `request` (RS-1)
**Produces:** `translate::responses_client::Encoder::new(model: &str) -> Encoder`, `Encoder::push(&mut self, chunk: &[u8]) -> Vec<SseEvent>` (Messages SSE bytes in, Responses events out), `Encoder::fail(&mut self, code: &str, message: &str) -> Vec<SseEvent>` (one `response.failed` when started and not ended), `Encoder::finish(&mut self) -> Vec<SseEvent>`, `Encoder::ended(&self) -> bool`, `Encoder::adjustments(&self) -> Vec<Adjustment>`, `translate::responses_client::response(msg: &Value, model: &str) -> Value` (non-streaming Response object)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_stream -- --list 2>/dev/null | grep -c ': test$')" -ge 10 && cargo test -p zygo-proxy --locked --test responses_stream` → at least 10 tests (a count of 0 fails), then all pass on ubuntu, macOS and Windows CI: a Messages SSE fixture with text, tool use, thinking and redacted thinking gives the event sequence of stream obligations 1-5 with `sequence_number` rising by one from 0 (K§11 item 13); `server_tool_use` dropped with `server_tool_dropped`; `max_tokens` → `response.incomplete`; an error event after the first event → exactly one `response.failed` and no other terminal event; `ping` and unknown events dropped; an unparsable event after start → `response.failed`; a tool call round trip (encoder output → next request through `request`) keeps `call_id` (K§11 item 15); a thinking round trip to the same provider keeps the signature, to another provider is dropped with its code (K§11 item 16); the JSON form carries the same content

## Planning notes

- Interfaces now: `SseEvent { … }` and `SseEvent::to_bytes` (`translate/sse.rs:12-19`), the line handling of `StreamMapper::push` (`sse.rs:111`) as the pattern for whole-line input, `HOLD_MAX` (`tee.rs:22`).
- The response id is `resp_<ULID>` from `zygo_core::ids::ulid` (as the proxy tests import it).
- This node works on bytes and values only; the HTTP status before the first byte is RS-3.

## Plan

Written when the node starts.
