# HUD-11 — statusline verb: CLI, slash command, overrides, --save, help, skill text

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§8, AU1, H§9 rows §17 command table and §17.1, H§11 rows `verbs.rs` (both), `cmd/statusline.rs`, `cmd/mod.rs`, `SKILL.md`. Edge to UPD-1 only for `cmd/mod.rs`; the embedded `commands/statusline.md` is generated from the verb list (`zygo-plugins/src/embedded.rs:32`). Raises the verb count in `crates/zygo-plugins/tests/snapshot.rs:27`, `:34` from 13 to 14; edge to HUD-9 for `cli_surface.rs`. Adds `statusline` to the verb list of `verbs_match_spec` (`cli_surface.rs:79-104`). Every new `SessionView` or `SessionStatus` literal in its tests ends in `..Default::default` (H§17 note 1). Tests that call `collect` pass a test-built `Paths` and an `env` closure
**Files:** `crates/zygo-core/src/verbs.rs`, `crates/zygo/src/cmd/verbs.rs`, `crates/zygo/src/cmd/statusline.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/help.rs`, `crates/zygo-plugins/embedded/skills/zygo-cli/SKILL.md`, `crates/zygo/tests/statusline_cmd.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo/tests/cli_surface.rs`, `crates/zygo-plugins/tests/snapshot.rs`
**Consumes:** `Overrides.statusline` (HUD-7), `Config.statusline` (HUD-3), `statusline::short` (HUD-8), `statusline::data::collect` (HUD-9)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test statusline_cmd --test slash_parity --test cli_surface && cargo test -p zygo-plugins && cargo test -p zygo-plugins --test snapshot && cargo test --workspace --locked --no-run` → all pass, including H§14 #33

## Plan

Steps done (test first):

- [x] Read the contract, H§8, AU1, H§9, H§11, H§14 #33, and the pattern of the session verbs (`units`, `results`, `redaction`).
- [x] Wrote `crates/zygo/tests/statusline_cmd.rs` (12 tests on the binary: CLI, slash through `zygo hook prompt`, and the next render of `zygo hook statusline`). Saw all 12 fail (`unrecognized subcommand 'statusline'`, `commands/statusline.md missing`).
- [x] Added the `statusline` rows to `SAME` in `slash_parity.rs`; added `statusline` to `help_lists_every_command`, `every_command_exists` and `verbs_match_spec` in `cli_surface.rs`; raised the counts in `zygo-plugins/tests/snapshot.rs`. Saw `cli_surface` fail (3 tests).
- [x] `zygo-core/src/verbs.rs`: `VerbSpec` `statusline`, args `[compact|verbose]`, summary `Status line style`, scope `SessionSave` (entry 14, last).
- [x] `cmd/verbs.rs`: `VERBS` entry; `Setting::Statusline(Style)`; `apply` writes `Overrides.statusline`; `save` writes `statusline.style` with `set_user_key`.
- [x] `cmd/statusline.rs` (new): the verb, `name`, `current`, and 5 unit tests.
- [x] `cmd/mod.rs`: module, `Cmd::Statusline(VerbArgs)`, dispatch name. The UPD-1 update check is unchanged.
- [x] `cmd/help.rs`: the `now:` value of `statusline`.
- [x] `SKILL.md`: one line for `zygo statusline [compact|verbose]` and `--save`.
- [x] Ran the Acceptance commands, `cargo test -p zygo --locked` and `cargo test -p zygo-plugins --locked`.

Design calls:

1. **OpenCode (operator decision, 2026-11-21).** An OpenCode session has the same verb with the same behavior as a Claude Code session. The H§8 bullet and the last sentence of H§14 #33 (exit 2 with `OpenCode has no zygo status line; run zygo status`) are not built. The verb does not read `ZYGO_AGENT`. Test: `opencode_session_behaves_as_a_claude_code_session`. The OpenCode commands come from `VERB_SPECS` (`zygo-launch/src/opencode.rs`), so `statusline` is there with no extra code.
2. **Text of the read form** (H§8 gives no text): `statusline <style> (<source>)`. Sources: `session` (`overrides.json`), `launch` (`view.json`, the value the launcher wrote), `user config` and `default` (outside a session; and `default` in a session whose files hold no style). In a session the value and the source follow the rule of the hook (`zygo_hooks::statusline::style`), so the verb prints what the next render uses. User config is not read in a session (AU1: a running session reads only `view.json` and `overrides.json`).
3. **`--save` with no style** changes nothing and prints the read form, as `redaction --save` does.
4. **Usage error text:** `zygo statusline: usage: statusline [compact|verbose]`, exit 2. The style is parsed before the session check, so `statusline wide` outside a session gives the usage text.
5. **No team layer.** `[statusline]` is user config only, so the verb loads the config with no team table; it makes no server request outside a session. A user config that does not load stops a change before any write (exit 1), as for the other session verbs. The read form in a session needs no config.
6. **Writes.** `overrides.json` only through `set_session` (`Overrides::save`: temporary file, then rename). User config through `set_user_key` (`zygo-plugins/src/select.rs`: lock, temporary file, then rename; every other byte kept), the path the other `--save` verbs use.
7. **`help`:** `now:` shows the style word only. In a session it shows also when the config does not load.
8. **Position in `VERB_SPECS`:** last, so no index of `VERBS` moves.
9. **No row in `KEEP`** (`slash_parity.rs`, "saved value shows in the next session"): its sessions have no `view.json`, and the product rule is that a session takes its launch style from `view.json`, not from user config. `statusline_cmd.rs` covers the session and the saved value; H§14 #35 (launcher) covers the next launch.
10. **`snapshot.rs:86`** also pins the count of embedded files (15 → 16); the contract names only lines 27 and 34.
