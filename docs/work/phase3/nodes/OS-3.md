# OS-3 — Bounded usage-store open for the hook and the MCP server

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§7.11 "Bound", O-C10, O§6.5 item 3 (no `ALTER` in a bounded open), O§8.4 (busy store with the bound passed as 0)
**Files:** `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** `Store::insert_trim` (OS-2), `Store::open_with_column_check` (OS-2)
**Produces:** `HOOK_DB_BOUND` (const), `HOOK_DB_BOUND: Duration = Duration::from_millis(100)`, `Store::open_bounded(path: &Path, bound: Duration) -> anyhow::Result<Store>` (busy timeout = `bound` for each statement, no retry loop, no `PRAGMA table_info`, no `ALTER`, creates no directory or file)
**Acceptance:** `cargo test -p zygo-usage --test store` → all pass, including new tests: `open_bounded(.., Duration::ZERO)` gives `busy_timeout_ms() == 0` and `open_bounded(.., HOOK_DB_BOUND)` gives 100; with a second connection holding `BEGIN IMMEDIATE`, `insert_trim` through `open_bounded(.., Duration::ZERO)` returns a busy error (no clock read in any assertion); `open_bounded` on a seven-column database leaves `PRAGMA table_info(trims)` at seven columns and its `insert_trim` writes a row; `open_bounded` on a missing path is an error and creates no file

## Plan
