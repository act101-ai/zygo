# ML-8 — `zygo ws name <key> <name>`

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§6 (last paragraph); `docs/specs/2026-11-20-workspaces-design.md` W§12, AU84
**Files:** `crates/zygo/src/cmd/ws.rs`, `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/tests/admin.rs`
**Consumes:** `MemberRecord.name` (ML-3), `Pane.name` (ML-3), `valid_member_name` (ML-1)
**Produces:** `zygo_workspace::admin::rename_member(paths: &Paths, workspace: Option<&str>, cwd: &Path, key: &str, new_name: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32`, `cmd::ws::WsCmd::Name { key: String, name: String, workspace: Option<String> }`
**Acceptance:** `cargo test -p zygo-workspace --test admin && cargo test -p zygo --lib` → all pass: the record's `name` and the `panes[]` entry change in one write under `lock`; `mail/<key>/` is not moved; an invalid key, an invalid or reserved name, or a name another member holds exits 1 with one line and changes nothing; an unknown key exits 1

## Plan
