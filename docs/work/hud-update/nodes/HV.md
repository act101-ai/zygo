# HV — SessionView statusline_style, overrides statusline, launcher writes the style

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§8, AU1, H§11 rows `overrides.rs`, `session_view.rs`, `launcher.rs`, H§17 note 1 (follows WS C1's literal-site pattern: no literal outside this node changes)
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo-core/src/overrides.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/tests/launcher.rs`
**Consumes:** `SessionView::default` (G2), `Config.statusline` (HC)
**Produces:** `SessionView.statusline_style: String`, `Overrides.statusline: Option<String>`
**Acceptance:** `cargo test -p zygo-core && cargo test -p zygo-launch --test launcher && cargo test --workspace --locked --no-run` → all pass and the workspace builds: an old `view.json` without the field reads as `compact`; the launcher writes the user-config style into `view.json`; `overrides.json` round-trips `statusline`

## Plan
