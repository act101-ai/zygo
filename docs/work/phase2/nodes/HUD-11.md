# HUD-11 — statusline verb: CLI, slash command, overrides, --save, help, skill text

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§8, AU1, H§9 rows §17 command table and §17.1, H§11 rows `verbs.rs` (both), `cmd/statusline.rs`, `cmd/mod.rs`, `SKILL.md`. Edge to UPD-1 only for `cmd/mod.rs`; the embedded `commands/statusline.md` is generated from the verb list (`zygo-plugins/src/embedded.rs:32`). Raises the verb count in `crates/zygo-plugins/tests/snapshot.rs:27`, `:34` from 13 to 14; edge to HUD-9 for `cli_surface.rs`. Adds `statusline` to the verb list of `verbs_match_spec` (`cli_surface.rs:79-104`). Every new `SessionView` or `SessionStatus` literal in its tests ends in `..Default::default` (H§17 note 1)
**Files:** `crates/zygo-core/src/verbs.rs`, `crates/zygo/src/cmd/verbs.rs`, `crates/zygo/src/cmd/statusline.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/help.rs`, `crates/zygo-plugins/embedded/skills/zygo-cli/SKILL.md`, `crates/zygo/tests/statusline_cmd.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo/tests/cli_surface.rs`, `crates/zygo-plugins/tests/snapshot.rs`
**Consumes:** `Overrides.statusline` (HUD-7), `Config.statusline` (HUD-3), `statusline::short` (HUD-8)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test statusline_cmd --test slash_parity --test cli_surface && cargo test -p zygo-plugins && cargo test -p zygo-plugins --test snapshot && cargo test --workspace --locked --no-run` → all pass, including H§14 #33

## Plan
