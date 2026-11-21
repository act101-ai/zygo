# HD2 — HUD data: bounded stdin, cache v2, lease, workers, deadline, failure display

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7, H§7.1, H§7.7, AU4, AU6, AU11, AU12, AU20, AU24, AU26 (trimmed source), C6; H§11 row `cmd/hook.rs` (`reads_stdin: false`)
**Files:** `crates/zygo-hooks/src/statusline/mod.rs`, `crates/zygo-hooks/src/statusline/data.rs`, `crates/zygo/src/cmd/hook.rs`, `crates/zygo-hooks/tests/statusline_data.rs`
**Consumes:** `statusline::render::render` (HR), `statusline::render::HudData` (HR), `repo::hud_git` (HG), `Store::bytes_trimmed` (HU), `SessionView.statusline_style` (HV), `Overrides.statusline` (HV)
**Produces:** `statusline::data::collect(stdin: &mut dyn Read, env: &HookEnv, deadline: Instant) -> HudData`
**Acceptance:** `cargo test -p zygo-hooks --test statusline_data` → all pass, one test per H§14 item #5, #6, #7, #13, #14, #16, #17, #22, #23, #24, #25, #26, #27, #28, #30, #35

## Plan
