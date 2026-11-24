# ML-5 — Multiplexer operations for the wake: literal keys and Enter

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C5, M§7.3 step 2; `docs/specs/2026-11-20-workspaces-design.md` W-C3, W-C8, AU62
**Files:** `crates/zygo-workspace/src/mux/mod.rs`, `crates/zygo-workspace/src/mux/tmux.rs`, `crates/zygo-workspace/tests/mux.rs`, `crates/zygo-workspace/tests/snapshot.rs`, `crates/zygo-workspace/tests/admin.rs`
**Consumes:** —
**Produces:** `zygo_workspace::mux::Multiplexer::send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError>` (`send-keys -t <pane> -l -- <text>`; a pane id `%<digits>` only; a text with a control character refused before any spawn), `zygo_workspace::mux::Multiplexer::send_enter(&self, pane: &str) -> Result<(), MuxError>` (`send-keys -t <pane> Enter`)
**Acceptance:** `cargo test -p zygo-workspace --test mux --test snapshot --test admin -- --include-ignored` → all pass (tmux cases run on tmux 3.2 or later, else print the skip reason; `cfg(unix)`): on a private server the literal text appears in the pane once and `send_enter` submits it; both calls pass `-S <socket>`; a target that is not `%<digits>` and a text with `\n` or `\x1b` are refused with no tmux spawn; the test fakes (`tests/snapshot.rs`, `tests/admin.rs`) implement the two operations

## Plan
