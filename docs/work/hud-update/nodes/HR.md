# HR — HUD render: segments, colors, icons, width fit, golden files

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§6.2-H§6.8, OP2-OP14, AU2, AU3, AU7-AU10, AU13, AU15, AU17-AU21, AU23, AU25 (order only), AU27, AU28, AU30; H§11 rows `statusline.rs`, `Cargo.toml` (`unicode-width`). The hook becomes a `statusline/` module; this node writes the pure render from a data struct
**Files:** `Cargo.toml`, `Cargo.lock`, `crates/zygo-hooks/Cargo.toml`, `crates/zygo-hooks/src/statusline.rs`, `crates/zygo-hooks/src/statusline/mod.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo-hooks/tests/golden/`
**Consumes:** `SessionStatus.model` (HS), `SessionStatus.threshold` (HS), `SessionStatus.spend_usd` (HSP), `SessionStatus.spend_notional` (HSP), `tokens::BYTES_PER_TOKEN` (HS), `Prices::cost` (HPR)
**Produces:** `statusline::render::HudData`, `statusline::render::RenderEnv { style: Style, icons: IconSet, color: ColorMode, width: u16 }`, `statusline::render::render(d: &HudData, env: &RenderEnv) -> String`
**Acceptance:** `cargo test -p zygo-hooks --test statusline_render` → all pass, one test per H§14 item #1, #2, #3, #4, #8, #9, #10, #12, #20, #21, #29, #31

## Plan
