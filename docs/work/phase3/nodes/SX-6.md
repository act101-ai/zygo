# SX-6 — OpenCode plugin transform gives the mechanics text on non-main `zygo/` models

**Spec:** docs/specs/2026-11-24-session-text-design.md §7.2 item 1, §14 item 3, §17 check 10
**Files:** `crates/zygo-launch/assets/opencode-plugin.js`, `crates/zygo-launch/tests/opencode.rs`
**Consumes:** `zygo hook session-start --agent opencode --event subagent` (SX-2)
**Produces:** plugin hook `experimental.chat.system.transform` (routes by `input.model`)
**Acceptance:** `cargo test -p zygo-launch --test opencode` → all pass on ubuntu and macOS (the `node_run` tests are `cfg(unix)`, `crates/zygo-launch/tests/opencode.rs:669`), with T§17 check 10 asserted from the stub's `calls.txt`: with `ZYGO_MAIN_ROLE=lead`, `read/zygo` and `code/zygo` call `hook session-start --agent opencode --event subagent` and push the stub output into `output.system`; `lead/zygo` calls `hook session-start --agent opencode` with no `--event`; `lead/other` calls nothing; with `ZYGO_MAIN_ROLE` unset a `zygo` model calls `--event subagent`

## Plan
