# FIX-23 — zygo's settings merge line is true when the user sets disableAllHooks

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §8.5 (Flags: the merge line; a user `disableAllHooks: true` wins and stops every hook, `zygo`'s included, while `attribution` and `includeCoAuthoredBy` always stay `zygo`'s). Defect: with a user `--settings` value that holds `disableAllHooks: true`, `ClaudeCode::build` (`crates/zygo-launch/src/claude_code.rs:474-478`) still prints `MERGE_WARN` (`claude_code.rs:22`), `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`, but the merged file then stops every hook, so the line is false. Found in the FIX-21 merge review (2026-11-22).
**Files:** `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`, `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** `ClaudeCode::settings` (FIX-21)
**Produces:** a §8.5 Flags merge line that `build` prints and that is true for the merged settings: it does not say the hooks stay in force when the merged user settings set `disableAllHooks: true`
**Acceptance:** `cargo test -p zygo-launch --locked --test claude_code` → all pass, including a case where a user `--settings` value with `disableAllHooks: true` gives a stderr line that does not say the hooks stay in force

## Plan

Not planned yet. Added by the FIX-21 merge (2026-11-22, controller item c).
