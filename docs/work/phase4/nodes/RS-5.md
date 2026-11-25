# RS-5 — Responses → Responses passthrough, the stream form seen by the tee, and the `response.completed` usage parse

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stream-order table row Responses; "The tee and the stream order"; cache row: no markers toward a Responses upstream), K§6.4.3 (Responses client → Responses upstream), K§11 items 17 and 19 (the Responses → Responses half); K-D21, K-D48
**Files:** `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/pipeline.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/tests/support/responses.rs`, `crates/zygo-proxy/tests/responses_passthrough.rs` (new)
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3)
**Produces:** `ResponseCtx.lines: Dialect` (the dialect of the SSE lines the tee and the response stages see: `Openai` only for a Responses upstream answering a Responses client with no mapper; `Anthropic` for an Anthropic upstream and for every mapped stream), `tee::responses_usage(line: &[u8]) -> Option<Value>` (the `response.usage` of a `response.completed` line)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_passthrough -- --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo-proxy --locked` → at least 6 tests in `responses_passthrough` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: the upstream body equals the client body except `model`, `store`, `include` and stage edits; the upstream body has `store` == `false` and an `include` array that contains `reasoning.encrypted_content`, also when the client sent `store: true` and no `include`; reasoning items are byte-equal; the usage row has the non-zero token counts of the upstream `response.completed`; a non-streaming request records the counts of the JSON body's `usage` (K§11 item 17); the upstream body holds no `cache_control` anywhere (K§11 item 19, second half)

## Planning notes

- How the tee tells the forms apart (author): `ResponseCtx.dialect` (`pipeline.rs:80-81`) is the upstream dialect, which does not say whether a mapper ran (a chat or Responses upstream answering a Messages client is mapped to Messages events, `forward.rs:1098-1110`). The forward step knows whether it built a mapper for the attempt, so it sets `lines` and gives it to the tee; the tee parses `message_start`/`message_delta` usage (`tee.rs:71-83`, `line_usage`) for `Anthropic` lines and `response.completed` usage for `Openai` lines. The Messages parse is unchanged.
- Same-dialect passthrough keeps the body bytes where no field changes (the `changed` rule of `RequestCtx`, `pipeline.rs:47-48`).

## Plan

Written when the node starts.
