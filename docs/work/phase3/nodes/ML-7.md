# ML-7 — Member name in the session view marker, hand start `--name`

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§6 (hand-started member, `--name <n>`), M§9a (Member name row: `name: Option<String>` on `Member`, `#[serde(default)]`); `docs/specs/2026-11-20-workspaces-design.md` AU9, AU58, AU102
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/claude.rs`, `crates/zygo/src/cmd/opencode.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/src/cmd/ws.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo/tests/workspace_member.rs`
**Consumes:** `MemberRecord.name` (ML-3), `MemberRecord::valid_name` (ML-3), `valid_member_name` (ML-1)
**Produces:** `zygo_core::session_view::Member.name: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `cmd::member::MemberCtx.name: Option<String>`, `cmd::Launch.name: Option<String>` and `cmd::LaunchOpenCode.name: Option<String>` (`--name <n>`)
**Acceptance:** `cargo test -p zygo-core --test core_basics && cargo test -p zygo --lib && cargo test -p zygo --test workspace_member -- --include-ignored` → all pass (tmux cases run or print the skip reason; `cfg(unix)`): a view without `name` loads; `zygo ws run` of a named record writes `workspace.member.name` into `view.json`; a hand start with `--name elena` writes the name to the record and the view; a `--name` that fails the grammar, `all`, `cli`, or a name another member of the workspace holds exits 1 with one line and starts nothing; `--name` outside a workspace pane exits 1 with one line

## Plan
