# HUD-12 — zygo status and zygo usage in tokens and spend; doctor icon line

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` AU26, OP1, AU2 (doctor line), H§9 rows §10.7, §17 `zygo doctor`; H§17 note 5: HUD-5 already added its `compact_cmd.rs` assertions (`model`, `context_window`, `threshold` in `slash_off_disables_and_on_clears`, `crates/zygo/tests/compact_cmd.rs`); nothing of note 5 is left for HUD-12. Owns H§14 #18 across layers; its render, data and usage parts are tested in the listed test files. Every new `SessionView` or `SessionStatus` literal in its tests ends in `..Default::default` (H§17 note 1). Its `statusline_data.rs` part of H§14 #18 calls `collect` with a test-built `Paths` (own usage db) and an `env` closure
**Files:** `crates/zygo/src/cmd/status.rs`, `crates/zygo/src/cmd/usage.rs`, `crates/zygo/src/cmd/doctor.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo-hooks/tests/statusline_data.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** `Store::bytes_trimmed` (HUD-1), `SessionStatus.spend_usd` (HUD-6), `tokens::BYTES_PER_TOKEN` (HUD-5), `statusline::render::spend_text` (HUD-8), `statusline::short` (HUD-8), `statusline::pct` (HUD-8), `statusline::trimmed` (HUD-8), `statusline::render::icon_set` (HUD-8), `statusline::render::glyphs` (HUD-8), `statusline::data::collect` (HUD-9)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test usage_doctor --test compact_cmd && cargo test -p zygo --lib cmd::status && cargo test --workspace --locked --no-run && cargo test -p zygo-hooks --test statusline_render --test statusline_data && cargo test -p zygo-usage --test store` → all pass, including H§14 #18 and #32

## Plan

Steps (each test was written first and seen to fail before the code, except where a note says the product was already merged):

- [x] `crates/zygo/src/cmd/status.rs` tests (fail first: 4 of 6): `inside_a_session` expects `savings: cache -, trimmed - tok` and `spend: $0.00`, also with the window hidden (C7); `h14_18_trimmed_in_tokens_and_the_spend_line` (canned reply with the OP13 numbers, 92,160 trimmed bytes → `savings: cache 92%, trimmed 23k tok` then `spend: $3.41`; 5 bytes → `2 tok`); `spend_line_forms` (`≈$1.84`, `$3.41+`, `$?`, `$0.00`, `$123`, and no line with no `spend_usd`); `server_text_cannot_forge_a_status_line` with the new text; no `spend:` line outside a session or with an unreachable session.
- [x] `status.rs`: the trimmed bytes of `statusline::trimmed` become `short(ceil(bytes ÷ BYTES_PER_TOKEN))` with the unit `tok`; a `spend:` line from `statusline::render::spend_text` follows the `savings:` line.
- [x] `crates/zygo/tests/usage_doctor.rs` usage tests (fail first: 7): every expected report gains `tokens trimmed (estimate): M` after `bytes trimmed: N`; `h14_18_usage_keeps_bytes_and_adds_estimated_tokens` (`bytes trimmed: 92160`, `tokens trimmed (estimate): 23040`, 5 bytes → 2, all sessions → 23042).
- [x] `crates/zygo/src/cmd/usage.rs`: the estimate line, `s.bytes_trimmed.div_ceil(BYTES_PER_TOKEN)`, as a whole number.
- [x] `usage_doctor.rs` doctor tests (fail first: 3): `h14_32_doctor_icon_line_holds_the_nerd_glyphs_and_names_the_set` (7 environments through the `Env.env` closure; the 19 glyphs are written in the test by code point), `h14_32_doctor_cli_prints_the_icon_line` (the binary with `ZYGO_NERD_FONT=1` and `=0`), `doctor_report_section_order` with `icons: ` between the plugin lines and the repo policy line.
- [x] `crates/zygo/src/cmd/doctor.rs`: `icon_line(icon_set(env.env))` after the plugin lines.
- [x] Two tests that fix the set of lines got the new lines: `usage_report_escapes_role_and_model` (11 → 12 lines) and `doctor_plugins_echoed_fields_are_escaped` (`icons:` is a known line start).
- [x] `crates/zygo/tests/compact_cmd.rs`: `h14_18_status_prints_trimmed_tokens_and_spend` runs the binary against a live proxy, CLI and `/zygo:status` (written after the `status.rs` change, so it passed at its first run).
- [x] `crates/zygo-hooks/tests/statusline_render.rs`: `h14_18_trimmed_bytes_show_as_estimated_tokens` (compact `✂ 23k`, plain `trim 23k`, verbose `trimmed 23k tok` with each icon set, the ceiling and `short` boundaries). The render is the merged HUD-8 product: the test passed at its first run.
- [x] `crates/zygo-hooks/tests/statusline_data.rs`: `h14_18_trimmed_row_between_two_calls_shows_at_the_second_call` calls `collect` with the test-built `Paths` (own usage db) and an `env` closure, at `T` and `T + 1`; one status request in all; no trimmed key in the cache file. The data layer is the merged HUD-9 product: the test passed at its first run.
- [x] `crates/zygo-usage/tests/store.rs`: `h14_18_bytes_trimmed_equals_summary_and_follows_a_new_row` (92,160 bytes, 23,040 tokens, one read-only handle sees a later row). `Store::bytes_trimmed` is the merged HUD-1 product: the test passed at its first run. The `EXPLAIN QUERY PLAN` part of H§14 #18 is the HUD-1 test `bytes_trimmed_query_uses_the_session_index` in the same file.
- [x] The Acceptance commands, then `cargo test -p zygo --locked` and `cargo test -p zygo-hooks --locked`.

Design calls:

1. **Doctor icon line text** (H§13 leaves it to the builder): `icons: nerd set (ZYGO_NERD_FONT=0 selects the plain set); test: <glyphs>` and `icons: plain set (ZYGO_NERD_FONT=1 selects the nerd set); test: <glyphs>`. The glyphs are `glyphs(IconSet::Nerd)` in the order of the H§6.5 table, one space between them, with each active set: the line is a font test, so a user on the plain set also sees whether the Nerd glyphs draw. The line has no color.
2. **Doctor line position**: after the plugin lines and before the repo policy line. The repo policy line stays last (`doctor_report_repo_policy_line`, `doctor_cli_valid_config_exits_zero` and `doctor_report_section_order` fix that).
3. **The active set** is `statusline::render::icon_set` with the `Env.env` closure of the report, so `zygo doctor` names the set that the status line uses in the same environment.
4. **`spend:` line position**: the last session line, after `savings:`. It shows also when C7 hides the window number: the spend is not a window number.
5. **No `spend_usd` in the reply** (an old server): no `spend:` line, as AU15 gives no spend segment.
6. **Unknown trimmed count** (no usage store, or a store that cannot be read): `trimmed - tok`, as the line printed `trimmed - bytes` before.
7. **`zygo usage` estimate** is the whole number (`23040`), not the `short` form: H§14 #18 gives `tokens trimmed (estimate): 23040`. It follows the report's filter (`--session`, `--since`) because it comes from the same `summary` value as the bytes line.
8. **`statusline::trimmed` is not changed.** It still reads `Store::summary`; its file `crates/zygo-hooks/src/statusline/mod.rs` is not in this node's Files. `status.rs` consumes it as it is. `Store::bytes_trimmed` is consumed through `data::collect` and by the store test.
