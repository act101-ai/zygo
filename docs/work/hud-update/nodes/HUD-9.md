# HUD-9 — HUD data: bounded stdin, cache v2, lease, workers, deadline, failure display

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7, H§7.1, H§7.7, AU4, AU6, AU11, AU12, AU20, AU24, AU26 (trimmed source), C6; H§11 row `cmd/hook.rs` (`reads_stdin: false`). Replaces the old line and `{at, line}` cache, so it owns their tests at `crates/zygo/tests/compact_cmd.rs:290-335`. The environment is read inside `collect`; no separate environment type. A failed premise of PRB-1 that changes an item below re-plans this node before it starts: the executor adds the H§12 fallback to this contract or a fallback node first (H§12, H§17 note 4)
**Files:** `crates/zygo-hooks/src/statusline/mod.rs`, `crates/zygo-hooks/src/statusline/data.rs`, `crates/zygo/src/cmd/hook.rs`, `crates/zygo-hooks/tests/statusline_data.rs`, `crates/zygo/tests/compact_cmd.rs`
**Consumes:** `statusline::render::render` (HUD-8), `statusline::render::HudData` (HUD-8), `statusline::width::terminal_width` (HUD-8), `repo::hud_git` (HUD-2), `Store::bytes_trimmed` (HUD-1), `SessionView.statusline_style` (HUD-7), `Overrides.statusline` (HUD-7)
**Produces:** `statusline::data::collect(stdin: &mut dyn Read, session_dir: &Path, deadline: Instant) -> HudData`
**Acceptance:** `cargo test -p zygo-hooks --test statusline_data && cargo test -p zygo --test compact_cmd` → all pass, one test per H§14 item #5, #6, #7, #13, #14, #16, #17, #22, #23, #24, #25, #26, #27, #28, #30, #35, and the `compact_cmd.rs` status line tests rewritten for the HUD

## Plan
