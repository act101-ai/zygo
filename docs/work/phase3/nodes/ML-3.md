# ML-3 — Member name in the plan, the member record and `panes[]`

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§6 (`up` copies the name); `docs/specs/2026-11-20-workspaces-design.md` W§7.2, AU78, AU84
**Files:** `crates/zygo-workspace/src/loadout.rs`, `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/src/snapshot.rs`, `crates/zygo-workspace/tests/loadout.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-workspace/tests/snapshot.rs`
**Consumes:** `LoadoutPane.name` (ML-1), `valid_member_name` (ML-1)
**Produces:** `zygo_workspace::loadout::PlanPane.name: Option<String>`, `zygo_workspace::state::MemberRecord.name: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `zygo_workspace::state::Pane.name: Option<String>` (same attributes), `zygo_workspace::state::MemberRecord::valid_name(&self) -> Option<&str>` (the name only when it passes `valid_member_name`: AU84 at each read)
**Acceptance:** `cargo test -p zygo-workspace --test loadout --test state --test snapshot` → all pass: `loadout::resolve` copies `name` to `PlanPane`; `fresh_member` (`up.rs`) writes it to the record; a snapshot keeps `Pane.name` of the stored pane with the same key after every `%N` changed; a `workspace.json` without `name` loads, and a write keeps unknown fields; a stored name that fails the grammar reads as no name

## Plan
