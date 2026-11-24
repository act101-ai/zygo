# OS-5 — MCP result tools record one read-back row per successful reply

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.4, O§6.5 item 2, O§7.11 last line, O§10 row "Usage store busy or broken (MCP read-back)", O§16 item 14
**Files:** `crates/zygo-mcp/src/tools/results.rs`, `crates/zygo-mcp/tests/readbacks.rs`
**Consumes:** `Stored.session` (OS-1), `ReadbackRow` (OS-2), `Store::insert_readback` (OS-2), `Store::open_bounded` (OS-3), `HOOK_DB_BOUND` (OS-3)
**Produces:** `Readbacks::new(usage_db: PathBuf, read_in: String, bound: Duration) -> Readbacks` (opens the store at the first read-back and keeps it; a failed open is tried again at the next read-back), `results::tools_with(dir: PathBuf, rb: Option<Readbacks>) -> Vec<Box<dyn Tool>>`; `results::tools(ctx: &McpCtx)` builds `Readbacks` from the session view's `session`, `Paths::from_env().usage_db` and `HOOK_DB_BOUND`
**Acceptance:** `cargo test -p zygo-mcp --test readbacks && cargo test -p zygo-mcp && cargo test -p zygo --test mcp_stdio` → all pass, including new tests in `readbacks.rs`: `result_view` on a stored id writes one row with `bytes` = the reply's byte count, `result_id` = the id, `session` = the meta's session, `read_in` = the reading session; a result whose meta has no `session` takes the reading session; `result_search` and `result_tree` each write one row; a bad id, a bad range and a bad pattern write no row; with the write lock held by a second connection and bound 0, the reply is byte-equal to the reply with no lock and no row is written

## Plan
