# SX-5 — Claude Code launch registers `SubagentStart` → `hook session-start` on every launch

**Spec:** docs/specs/2026-11-24-session-text-design.md §6.3 items 1 and 3, §8.4, §17 checks 1 and 9; T-C2
**Files:** `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`
**Consumes:** —
**Produces:** settings key `hooks.SubagentStart`
**Acceptance:** `cargo test -p zygo-launch --test claude_code -- subagent_start zygo_role_prompt` → all pass on ubuntu, macOS and Windows: T§17 check 1 (plain launch: one entry, command `<zygo> hook session-start`, no matcher; member launch: two entries in the order `session-start`, `workspace --event subagent-start`) and check 9 (each `zygo-<role>` prompt of a default launch equals the role prompt, one space, M); then `cargo test -p zygo-launch --test claude_code` → all pass

## Plan
