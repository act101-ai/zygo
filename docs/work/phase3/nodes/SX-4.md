# SX-4 — `zygo hook trim` gives the mechanics text once after a subagent's compaction

**Spec:** docs/specs/2026-11-24-session-text-design.md §9.2 items 2-4, §14 item 2, §17 check 8; T-C1, T-C5
**Files:** `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/reinject.rs`
**Consumes:** `mechanics_with_overrides` (SX-1), `flag_path` (SX-3), `SessionDir::reinject` (SX-3)
**Produces:** `zygo hook trim` output field `hookSpecificOutput.additionalContext`
**Acceptance:** `cargo test -p zygo-hooks --test reinject -- trim` → all pass on ubuntu, macOS and Windows: T§17 check 8 (after the flag, under-threshold `PostToolUse-Bash-sub.json` prints exactly `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":M}}`, the flag is gone, a second run prints nothing; over-threshold gives one object with both `updatedToolOutput` and `additionalContext`; `PostToolUse-Bash-main.json` reads no flag), a flag that cannot be removed gives no text and one stderr line with the trim output unchanged, and an `agent_id` that fails the name rule removes no file; then `cargo test -p zygo-hooks` → all pass with no existing trim test changed

## Plan
