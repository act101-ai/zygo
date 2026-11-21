# HS — Proxy status: model, context_window, threshold (AU28, AU29); BYTES_PER_TOKEN

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2 (rows `effective_window`, `model`, `context_window`, `threshold`), AU28, AU29, OP1, H§17 notes 1 and 5, H§9 rows §7.2, §10.2.2, §10.2.4. Adds its fields after WS X1's in the same `SessionStatus` literal sites, which this node now owns
**Files:** `crates/zygo-core/src/tokens.rs`, `crates/zygo-core/src/lib.rs`, `crates/zygo-proxy/src/client.rs`, `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/window.rs`, `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/tests/session_api.rs`, `crates/zygo-proxy/tests/window.rs`, `crates/zygo-hooks/src/statusline.rs`
**Consumes:** `SessionStatus.requests_in_flight` (G1)
**Produces:** `tokens::BYTES_PER_TOKEN: u64`, `SessionStatus.model: Option<String>`, `SessionStatus.context_window: Option<u64>`, `SessionStatus.threshold: Option<u64>`, `window::soft_threshold(...) -> Option<u64> (made public)`
**Acceptance:** `cargo test -p zygo-proxy --test session_api --test window && cargo test --workspace --locked --no-run` → all pass and the workspace builds, including H§14 #19, the existing AU29 assertions unchanged plus `model`/`context_window` assertions, and new tests for `[1m]`, another role's model, a non-streamed request and an OpenCode request

## Plan
