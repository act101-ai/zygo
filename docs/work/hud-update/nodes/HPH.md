# HPH — Workspace phase segment

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.6, AU25, H§17 note 3; reads `view.workspace.member.ws_id` (WS C1) and parses only `version` and `phase` of `workspace.json` (WS§7.2)
**Files:** `crates/zygo-hooks/src/statusline/mod.rs`, `crates/zygo-hooks/src/statusline/phase.rs`, `crates/zygo-hooks/tests/statusline_phase.rs`
**Consumes:** `Paths::workspaces_dir` (G4), `SessionView.workspace` (G4), `identity::valid_ws_id` (G4), `statusline::data::collect` (HD2)
**Produces:** —
**Acceptance:** `cargo test -p zygo-hooks --test statusline_phase` → all pass, including H§14 #36

## Plan
