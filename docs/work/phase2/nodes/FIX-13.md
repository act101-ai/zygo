# FIX-13 — Stub agent reads the one-token `=` session arguments

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` AU3 as amended 2026-11-22 (L2 round 2: Claude Code `--session-id=<id>` and `--resume=<id>`, OpenCode `-s <id>`); found by L2 round 2. Defect: the stub agent's `session_arg` (`crates/zygo-launch/tests/stub_agent/stub.rs:63`) finds only the split forms `--session-id <id>`, `--resume <id>` and `-s <id>`, so a stub launch of a Claude Code member with `LaunchOpts.agent_session` sees no session argument: `session-arg.json` is not written and the `SessionStart` input has `source` `startup` for a resume. Fix: `session_arg` also reads `--session-id=<id>` and `--resume=<id>` as one token, with the flag name `--session-id` or `--resume` and the value after the first `=`; the split forms and `-s <id>` keep working; the first session argument in `argv` that has a value wins, as today. M2's stub tests need it
**Files:** `crates/zygo-launch/tests/stub_agent/stub.rs`, `crates/zygo-launch/tests/stub_launch.rs`
**Consumes:** `AgentAdapter::preassign_args` (L2), `AgentAdapter::resume_args` (L2), `LaunchOpts.agent_session` (L2)
**Produces:** stub agent reads = session forms
**Acceptance:** `cargo test -p zygo-launch --locked --test stub_launch` → all pass, including a stub launch with `--session-id=U1` that writes `session-arg.json` naming `--session-id` with the id `U1`, and one with `--resume=U1` whose `SessionStart` input has `source` `resume`, and the existing split-form cases

## Plan
