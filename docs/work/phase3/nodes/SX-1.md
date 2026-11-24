# SX-1 — Claude Code subagent branch of `zygo hook session-start` (mechanics text on `SubagentStart`)

**Spec:** docs/specs/2026-11-24-session-text-design.md §6.3 item 2, §9.1, §14 item 1, §17 checks 2-6 and 12; T-C1, T-C5
**Files:** `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/subagent_text.rs` (new)
**Consumes:** —
**Produces:** `session_start::mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String`
**Acceptance:** `cargo test -p zygo-hooks --test subagent_text -- subagent_start main_compact` → all pass on ubuntu, macOS and Windows, with at least one test each for T§17 checks 2, 3, 4, 5, 6 and 12, driven through `run_member` (the path `zygo hook session-start` runs) and `run_in`

## Plan
