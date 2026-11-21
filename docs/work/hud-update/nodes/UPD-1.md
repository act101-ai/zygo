# UPD-1 — Auto-update: split check/install; start check at today's zygo claude / zygo opencode dispatch

**Spec:** `docs/specs/2026-11-21-auto-update-design.md` "Where" (`cmd/mod.rs:395-396`), "When the check is skipped", "Check", "Prompt", "Other sessions", "Opt-out", "Proxy", "Left to the builder". TTY detection with `std::io::IsTerminal` (no new dependency, so no `Cargo.toml`/`Cargo.lock` change); tests drive the check through an injectable `PromptIo` (TTY flag, input, output) instead of a terminal
**Files:** `crates/zygo/src/cmd/update.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/tests/update.rs`
**Consumes:** `Config.update` (HUD-3)
**Produces:** `update::PromptIo`, `update::StartOutcome { Continue, Exit { code: i32 } }`, `update::start_check(paths: &Paths, cfg: &Config, args: &[String], io: &mut dyn PromptIo) -> StartOutcome`
**Acceptance:** `cargo test -p zygo --test update && cargo test --workspace --locked --no-run` → all pass and the workspace builds, one test per auto-update acceptance item #1, #2, #3, #4, #5, #6, #7 (the Windows deferral of #7 runs on the windows-latest job of `.github/workflows/ci.yml`)

## Plan
