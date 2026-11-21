# HU — Usage store: Store::bytes_trimmed

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.4, AU4 (trimmed read on every call)
**Files:** `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** —
**Produces:** `Store::bytes_trimmed(&self, session: &str) -> anyhow::Result<u64>`
**Acceptance:** `cargo test -p zygo-usage` → all pass, including `bytes_trimmed` equal to the `summary` trimmed value for the same session and a query plan that uses the session index

## Plan
