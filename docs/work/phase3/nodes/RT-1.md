# RT-1 — Member record `first_started_at`, set by each commit of a pending start

**Spec:** docs/specs/2026-11-24-session-runtime-design.md §2.1, §3.2, §4, §5 (commit rows), §6 (rows `state.rs`, `zygo-hooks/src/workspace.rs`, `member.rs`), §7 items 4 and 5; base-spec amendment W§7.2 field table and W AU92 (docs/specs/2026-11-20-workspaces-design.md)
**Files:** `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/workspace.rs`, `crates/zygo/src/cmd/member.rs`, `docs/specs/2026-11-20-workspaces-design.md`
**Consumes:** —
**Produces:** `zygo_workspace::state::MemberRecord.first_started_at: Option<u64>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`; `MemberRecord::new` sets `None`); JSON key `members.<pane>.first_started_at` (Unix seconds) in `workspace.json` `version` 1
**Acceptance:** `cargo test --locked -p zygo-workspace -p zygo-hooks -p zygo` → all pass on ubuntu, macOS and Windows CI, and the tests whose names contain `first_started_at` (at least 6: hook commit sets it, exit commit sets it, second commit keeps it, `pending.started_at = 0` leaves it absent, `NotStarted` exit leaves it absent, old record without the field parses and a write keeps every other field and a set field) run and pass

## Plan
