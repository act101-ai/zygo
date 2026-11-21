# UU — Auto-update: one check for zygo up

**Spec:** `docs/specs/2026-11-21-auto-update-design.md` "Where", "When the check is skipped" (`zygo up` asks one time; `ZYGO_SESSION` skips, so panes and `ws run` never ask), "Citation corrections" (W7 dispatch); H§17 note 3
**Files:** `crates/zygo/src/cmd/up.rs`, `crates/zygo/tests/update.rs`
**Consumes:** `update::start_check` (UC), `up::up` (G4)
**Produces:** —
**Acceptance:** `cargo test -p zygo --test update` → all pass, including a `zygo up` that prompts once before any pane starts and a `ws run` with `ZYGO_SESSION` that makes no request

## Plan
