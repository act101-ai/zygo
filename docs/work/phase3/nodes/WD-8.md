# WD-8 — Member record fields `launch.job` and `launch.definition`

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§7.4 (`member.launch.definition = { commit, digest }`), R§9.3 effect 2 (`member.launch.job`; absent for no job), R§12, R§14 row W§7.2
**Files:** `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`, `docs/specs/2026-11-20-workspaces-design.md` (W§7.2 amendment)
**Consumes:** `Pin` (WD-4)
**Produces:** `state::Launch.job: Option<String>`, `state::Launch.definition: Option<zygo_config::repo_def::Pin>` (each `#[serde(default, skip_serializing_if = "Option::is_none")]`)
**Acceptance:** `cargo test -p zygo-workspace --test state` → all pass. New tests: a record with both fields round-trips; a record written without them loads with both `None` and serializes with neither key. `grep -n "launch.definition" docs/specs/2026-11-20-workspaces-design.md` → the W§7.2 line

## Plan
