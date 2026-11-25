# RS-7 — Window rewrite for a Responses client, Anthropic and Responses upstreams

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stage table row window; the encoder runs after the tee), K§6.4.5 (the format of the rewrite; arming is client policy), K§11 item 21; base spec §10.2.2, §10.2.6
**Files:** `crates/zygo-proxy/src/window.rs`, `crates/zygo-proxy/src/tee.rs`, `crates/zygo-proxy/tests/responses_window.rs` (new)
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3), `ResponseCtx.lines` (RS-5), `tee::responses_usage` (RS-5)
**Produces:** `window::rewrite_responses_usage(line: &[u8], u: Usage, window: u64) -> Option<Vec<u8>>` (§10.2.2 with the Responses field names, on a `response.completed` line)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_window -- --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-proxy --locked` → at least 4 tests in `responses_window` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: armed, `response.completed` usage follows §10.2.2 for an Anthropic upstream (rewrite on the Messages `message_delta` before the encoder) and for a Responses upstream (rewrite on `response.completed`); unarmed, the bytes pass unchanged (K§11 item 21); the existing `window` tests pass unchanged

## Planning notes

- The stage tells the two forms apart by `ResponseCtx.lines` (RS-5): `Anthropic` lines → the existing `message_delta` branch (`window.rs:139-181`); `Openai` lines → the `response.completed` branch. No change to `pipeline.rs` or `forward.rs` is needed here.
- Arming stays the per-client rule of today (`window.rs:129-136`: Claude Code session, no `x-claude-code-compaction` header, a served model with a context window), extended from `ModelRoute::Messages` to `ModelRoute::Responses` (author). So the test session (Claude Code) is armed, and an OpenCode session is the unarmed case. The Codex arming rule (K§6.4.5, PC6) and the descriptor field are P4-9 and P4-6.
- Trigger 1 reads the `x-claude-code-request-class` header (`window.rs:162`); on the Responses route it fires only when that header is present, as today.
- The rewrite works in whole SSE lines inside the `HOLD_MAX` bound (`tee.rs:22`).

## Plan

Written when the node starts.
