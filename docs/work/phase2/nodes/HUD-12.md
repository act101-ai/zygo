# HUD-12 — zygo status and zygo usage in tokens and spend; doctor icon line

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` AU26, OP1, AU2 (doctor line), H§9 rows §10.7, §17 `zygo doctor`; H§17 note 5: HUD-5 already added its `compact_cmd.rs` assertions (`model`, `context_window`, `threshold` in `slash_off_disables_and_on_clears`, `crates/zygo/tests/compact_cmd.rs`); nothing of note 5 is left for HUD-12. Owns H§14 #18 across layers; its render, data and usage parts are tested in the listed test files. Every new `SessionView` or `SessionStatus` literal in its tests ends in `..Default::default` (H§17 note 1). Its `statusline_data.rs` part of H§14 #18 calls `collect` with a test-built `Paths` (own usage db) and an `env` closure
**Files:** `crates/zygo/src/cmd/status.rs`, `crates/zygo/src/cmd/usage.rs`, `crates/zygo/src/cmd/doctor.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo-hooks/tests/statusline_data.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** `Store::bytes_trimmed` (HUD-1), `SessionStatus.spend_usd` (HUD-6), `tokens::BYTES_PER_TOKEN` (HUD-5), `statusline::render::spend_text` (HUD-8), `statusline::short` (HUD-8), `statusline::pct` (HUD-8), `statusline::trimmed` (HUD-8), `statusline::render::icon_set` (HUD-8), `statusline::render::glyphs` (HUD-8), `statusline::data::collect` (HUD-9)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test usage_doctor --test compact_cmd && cargo test -p zygo --lib cmd::status && cargo test --workspace --locked --no-run && cargo test -p zygo-hooks --test statusline_render --test statusline_data && cargo test -p zygo-usage --test store` → all pass, including H§14 #18 and #32

## Plan
