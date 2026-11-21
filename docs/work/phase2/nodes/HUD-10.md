# HUD-10 — Team-server session spend at built-in list prices (AU14)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` AU14, H§7.3 step 5, H§9 row §7.2 (no configured price leaves the server), H§17 note 2. Owns the `crates/zygo-hooks/Cargo.toml` dev-dependency additions its team-server test needs. Every new `SessionView` or `SessionStatus` literal in its tests ends in `..Default::default` (H§17 note 1)
**Files:** `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/tests/serve.rs`, `crates/zygo-hooks/tests/statusline_team.rs`, `crates/zygo-hooks/Cargo.toml`, `Cargo.lock`
**Consumes:** `Prices::cost` (HUD-4), `SessionStatus.team_session` (HUD-6), `statusline::render::render` (HUD-8)
**Produces:** —
**Acceptance:** `cargo test -p zygo-proxy --test serve && cargo test -p zygo-hooks --test statusline_team && cargo test --workspace --locked --no-run` → all pass, including H§14 #15

## Plan
