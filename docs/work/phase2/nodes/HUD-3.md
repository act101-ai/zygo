# HUD-3 — Config tables [statusline] and [update] on today's schema; user config only

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§8, OP7, AU1, H§9 row §6.1, H§17 note 6; `docs/specs/2026-11-21-auto-update-design.md` "Opt-out". Both tables are dropped from the team layer with one warning; the repository-file filter already drops them (`repo_file.rs:219-229`)
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/tests/config_layers.rs`
**Consumes:** —
**Produces:** `schema::StatuslineConfig { style: String }`, `Config.statusline: StatuslineConfig`, `schema::UpdateConfig { check: bool }`, `Config.update: UpdateConfig`
**Acceptance:** `cargo test -p zygo-config && cargo test --workspace --locked --no-run` → all pass and the workspace builds, including the repository-file filter test for `[statusline]`, both tables dropped from the team layer with one warning, `check = true` and `style = "compact"` by default, and `config check` rejecting a `style` other than `compact` or `verbose`

## Plan

- [x] Read the contract and the authority sections (H§8, OP7, AU1, H§9 row §6.1, H§17 note 6; auto-update "Opt-out").
- [x] Write the failing tests in `tests/config_layers.rs`: defaults, user config, team-layer drop with one warning, a dropped team table of a wrong type does not refuse the load, repository-file filter for `[statusline]` and `[update]`, `config check` on `style`. Result: 19 compile errors (no `StatuslineConfig`, `UpdateConfig`, `Config.statusline`, `Config.update`).
- [x] `schema.rs`: `StatuslineConfig { style }` (default `compact`), `UpdateConfig { check }` (default `true`), `STATUSLINE_STYLES`, `Config.statusline`, `Config.update`.
- [x] `layers.rs`: `USER_ONLY` and `drop_user_only`; `load_with_builtin` removes both tables from the team layer before it parses the layer, and adds one warning that names the dropped tables.
- [x] `check.rs`: `statusline.style` must be `compact` or `verbose`.
- [x] `repo_file.rs`: no change; the `KEPT` filter drops both tables (test `repo_file_drops_statusline_and_update`).
- [x] Check 1 `cargo test -p zygo-config`: 176 passed, 0 failed.
- [x] Check 2 `cargo test --workspace --locked --no-run`: build finished, all test executables linked.
