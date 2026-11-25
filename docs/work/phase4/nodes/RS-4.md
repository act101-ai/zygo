# RS-4 — Responses → Responses passthrough and the tee's `response.completed` usage parse

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stream-order table row Responses; "The tee and the stream order"; cache row: no markers toward a Responses upstream), K§6.4.3 (Responses client → Responses upstream), K§11 items 17 and 19 (the Responses → Responses half); K-D21, K-D48
**Files:** `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/tests/responses_passthrough.rs` (new)
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3)
**Produces:** `tee::responses_usage(line: &[u8]) -> Option<Value>` (the `response.usage` of a `response.completed` line)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_passthrough -- --list 2>/dev/null | grep -c ': test$')" -ge 5 && cargo test -p zygo-proxy --locked` → at least 5 tests in `responses_passthrough` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: the upstream body equals the client body except `model`, `store`, `include` and stage edits; reasoning items are byte-equal; the usage row has the non-zero token counts of the upstream `response.completed`; a non-streaming request records the counts of the JSON body's `usage` (K§11 item 17); the upstream body holds no `cache_control` anywhere (K§11 item 19, second half)

## Planning notes

- The tee parses only `message_start` and `message_delta` today (`tee.rs:71-83`, `line_usage`). This node adds the Responses parser; the Messages parse is unchanged.
- Same-dialect passthrough keeps the body bytes where no field changes (the `changed` rule of `RequestCtx`, `pipeline.rs:47-48`).

## Plan

Written when the node starts.
