# HSU — zygo status and zygo usage in tokens and spend; doctor icon line

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` AU26, OP1, AU2 (doctor line), H§9 rows §10.7, §17 `zygo doctor`; H§17 note 5 (`compact_cmd.rs:173-176` gains `model`/`context_window` assertions)
**Files:** `crates/zygo/src/cmd/status.rs`, `crates/zygo/src/cmd/usage.rs`, `crates/zygo/src/cmd/doctor.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo/tests/compact_cmd.rs`
**Consumes:** `Store::bytes_trimmed` (HU), `SessionStatus.spend_usd` (HSP), `tokens::BYTES_PER_TOKEN` (HS)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test usage_doctor --test compact_cmd && cargo test -p zygo --lib cmd::status` → all pass, including H§14 #18 and #32

## Plan
