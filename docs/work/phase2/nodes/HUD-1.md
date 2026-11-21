# HUD-1 — Usage store: Store::bytes_trimmed

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.4, AU4 (trimmed read on every call). Setup for the Windows check used by every Rust node: `rustup target add x86_64-pc-windows-gnu` and a MinGW-w64 C compiler (`x86_64-w64-mingw32-gcc`, for the bundled SQLite and ring build scripts); source: the windows-latest job of `.github/workflows/ci.yml`
**Files:** `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** —
**Produces:** `Store::bytes_trimmed(&self, session: &str) -> anyhow::Result<u64>`
**Acceptance:** `cargo test -p zygo-usage && cargo test --workspace --locked --no-run && cargo check --workspace --locked --target x86_64-pc-windows-gnu` → all pass, including `bytes_trimmed` equal to the `summary` trimmed value for the same session and a query plan that uses the session index

## Plan
