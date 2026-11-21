# HVB — statusline verb: CLI, slash command, overrides, --save, help, skill text

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§8, AU1, H§9 rows §17 command table and §17.1, H§11 rows `verbs.rs` (both), `cmd/statusline.rs`, `cmd/mod.rs`, `SKILL.md`. Edge to UC only for `cmd/mod.rs`
**Files:** `crates/zygo-core/src/verbs.rs`, `crates/zygo/src/cmd/verbs.rs`, `crates/zygo/src/cmd/statusline.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/help.rs`, `crates/zygo-plugins/embedded/skills/zygo-cli/SKILL.md`, `crates/zygo/tests/statusline_cmd.rs`, `crates/zygo/tests/slash_parity.rs`
**Consumes:** `Overrides.statusline` (HV), `Config.statusline` (HC)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test statusline_cmd --test slash_parity && cargo test -p zygo-plugins` → all pass, including H§14 #33 (the embedded `commands/statusline.md` generated from the verb list)

## Plan
