# SX-3 — `zygo hook pre-compact` sets the per-subagent re-inject flag

**Spec:** docs/specs/2026-11-24-session-text-design.md §9.2 item 1, §14 item 2, §16 rows `pre_compact.rs` and `session_dir.rs`, §17 check 7; T-C1
**Files:** `crates/zygo-core/src/session_dir.rs`, `crates/zygo-hooks/src/pre_compact.rs`, `crates/zygo-hooks/tests/reinject.rs` (new)
**Consumes:** —
**Produces:** `SessionDir::reinject(&self) -> PathBuf`, `pre_compact::flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf>`
**Acceptance:** `cargo test -p zygo-hooks --test reinject -- pre_compact` → all pass on ubuntu, macOS and Windows: T§17 check 7 (`PreCompact-sub.json` prints the §10.6 text and creates `reinject/a1a80dd100f197242`, with file `0600` and directory `0700` asserted under `cfg(unix)`; no file for `PreCompact-main.json`, for `agent_type` `zygo-read`, or for `agent_id` `../x`; `flag_path` is `None` for an id that fails the 1-64 `[A-Za-z0-9_-]` rule) and T§14 item 2 (flag cannot be created: the §10.6 text is still printed, one stderr line, exit 0)

## Plan
