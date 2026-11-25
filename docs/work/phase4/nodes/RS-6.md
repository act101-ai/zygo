# RS-6 — Cache markers at forward time on the Anthropic upstream body of a Responses client

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.2 (stage table row cache markers), K§11 item 19 (the Responses → Anthropic half); base spec `docs/specs/2026-11-18-zygo-design.md` §10.4 (marker placement); K-D48
**Files:** `crates/zygo-proxy/src/cache.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/responses_cache.rs` (new)
**Consumes:** `ModelRoute::Responses` (RS-3), `Harness::start` (RS-3)
**Produces:** `cache::mark_upstream(body: &mut Value) -> bool` (the base-spec §10.4 marker placement on a Messages body built for an Anthropic upstream; `false` when it already holds a marker)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_cache -- --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-proxy --locked` → at least 3 tests in `responses_cache` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: Responses → Anthropic with `[cache] insert_markers.<session client> = true` puts the markers in the upstream body; with it off, none; Responses → chat, none; the Anthropic client's request-time stage (`cache.rs:33-40`) is unchanged (existing cache tests pass unchanged)

## Planning notes

- Client policy stays the per-client config key `insert_markers.<agent>` read as today (`cache.rs:34`); the Codex default is P4-9 (K§6.4.2).
- The marker is placed in `attempt_body` (`forward.rs:779-831`) after translation, only when the candidate dialect is Anthropic and the client dialect is Openai.
- This node and RS-7 share no file (RS-7 changes `window.rs` and `tee.rs`), so they run in parallel after RS-5.

## Plan

Written when the node starts.
