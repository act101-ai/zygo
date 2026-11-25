# RS-4 — Response side for Anthropic and chat upstreams: stream order, encoder after the tee, non-streaming path, errors after the first event

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stage table row usage; the stream-order table rows Anthropic and chat), K§6.4.3 (the response side of Responses → Anthropic and Responses → chat; stream obligations 6 and 8), K§6.7 (upstream streams), K§11 items 14 (the after-the-first-event half, at the route) and 18; K-D48, K-D50
**Files:** `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/support/responses.rs`, `crates/zygo-proxy/tests/responses_forward.rs` (new)
**Consumes:** `Encoder::new` (RS-2), `response` (RS-2), `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3), `fixtures/codex/requests/` (PC-3)
**Produces:** —
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_forward -- --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo-proxy --locked` → at least 6 tests in `responses_forward` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: a captured Codex request streamed through a Claude Code session to an Anthropic upstream returns Responses SSE and the usage row has the upstream Messages counts; Responses → chat reaches the chat mock, its stream returns as Responses SSE and the usage row is recorded (K§11 item 18); an upstream error event after the first event → one `response.failed` and no other terminal event (K§11 item 14, second half); a non-streaming request to each upstream returns one Response object with the same content; a non-2xx non-streaming answer keeps its status with an OpenAI error body; the existing Messages-client stream tests pass unchanged

## Planning notes

- Stream order per K§6.4.2: upstream Messages SSE → tee → `Encoder`; chat SSE → existing `StreamMapper` (`translate/sse.rs:48`) → tee → `Encoder`. Today the mapper output goes into the tee at `forward.rs:1098-1110`; the encoder goes after the tee, so a later window rewrite on `message_delta` reaches the client inside `response.completed` (RS-7).
- The translated non-streaming paths are measured as today, on the Messages form (K§6.4.2).
- `tests/support/responses.rs` is in Files for any helper this node adds to the harness of RS-3.

## Plan

Written when the node starts.
