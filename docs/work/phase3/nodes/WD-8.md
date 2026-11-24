# WD-8 — Member record fields `launch.job` and `launch.definition`

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§7.4 (`member.launch.definition = { commit, digest }`), R§9.3 effect 2 (`member.launch.job`; absent for no job), R§12
**Files:** `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`
**Consumes:** `Pin` (WD-4)
**Produces:** `state::Launch.job: Option<String>`, `state::Launch.definition: Option<zygo_config::repo_def::Pin>` (each `#[serde(default, skip_serializing_if = "Option::is_none")]`)
**Acceptance:** `test "$(cargo test -q -p zygo-workspace --test state -- launch_definition --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-workspace --test state` → every count holds (at least 2 named `launch_definition…` in zygo-workspace `state`; a count of 0 fails before any run), then all pass. New tests: a record with both fields round-trips; a record written without them loads with both `None` and serializes with neither key.

## Plan
