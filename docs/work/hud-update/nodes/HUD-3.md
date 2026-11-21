# HUD-3 — Config tables [statusline] and [update] on today's schema; user config only

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§8, OP7, AU1, H§9 row §6.1, H§17 note 6; `docs/specs/2026-11-21-auto-update-design.md` "Opt-out". Both tables are dropped from the team layer with one warning; the repository-file filter already drops them (`repo_file.rs:219-229`)
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/tests/config_layers.rs`
**Consumes:** —
**Produces:** `schema::StatuslineConfig { style: String }`, `Config.statusline: StatuslineConfig`, `schema::UpdateConfig { check: bool }`, `Config.update: UpdateConfig`
**Acceptance:** `cargo test -p zygo-config && cargo test --workspace --locked --no-run` → all pass and the workspace builds, including H§14 #34, both tables dropped from the team layer with one warning, `check = true` and `style = "compact"` by default, and `config check` rejecting a `style` other than `compact` or `verbose`

## Plan
