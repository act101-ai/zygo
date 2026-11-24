# OS-4 — Trim hook writes `result_id` and `shaper`, opens the store bounded

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§7.9 (row fields; `shaper = "size"` for a §10.1 view), O§7.11, O§10 row "Usage store busy past `HOOK_DB_BOUND`, or broken (hook)", O§16 item 15 (hook part)
**Files:** `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/hooks_basic.rs`
**Consumes:** `Store::put` (OS-1), `Stored.session` (OS-1), `TrimRow.result_id` (OS-2), `TrimRow.shaper` (OS-2), `Store::insert_trim` (OS-2), `Store::open_bounded` (OS-3), `HOOK_DB_BOUND` (OS-3)
**Produces:** `TrimOpts { db_bound: Duration }` (`Default`: `HOOK_DB_BOUND`), `trim::trim_in_with(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput>`; `trim_in` keeps its signature and calls `trim_in_with` with the default
**Acceptance:** `cargo test -p zygo-hooks --test hooks_basic && cargo test -p zygo --test hook_trim` → all pass, including new tests: a trimmed Bash call writes a row whose `result_id` is the id in the marker, whose `shaper` is `size` and whose `command` is null; with a second connection holding the write lock and `db_bound = 0`, the hook returns the same view as with no lock and writes no row; on a database built from the `97d8767` schema the hook writes a seven-column row and adds no column

## Plan
