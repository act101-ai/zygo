# SX-2 — OpenCode `session-start --event subagent` branch (mechanics text as plain text, no member part)

**Spec:** docs/specs/2026-11-24-session-text-design.md §7.2 item 2, §17 check 11; T-C5
**Files:** `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/subagent_text.rs`
**Consumes:** `mechanics_with_overrides` (SX-1)
**Produces:** `zygo hook session-start --agent opencode --event subagent`
**Acceptance:** `cargo test -p zygo-hooks --test subagent_text -- opencode_subagent` → all pass on ubuntu, macOS and Windows, including T§17 check 11: in a member session view that holds resume-note inputs the output is M as plain text and `view.json` is byte-equal before and after

## Plan
