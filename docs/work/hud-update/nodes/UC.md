# UC — Auto-update: split check/install; start check for zygo claude and zygo opencode

**Spec:** `docs/specs/2026-11-21-auto-update-design.md` "Where", "When the check is skipped", "Check", "Prompt", "Other sessions", "Opt-out", "Proxy", "Left to the builder". No functional workspaces dependency; G4 orders the shared `cmd/mod.rs` only
**Files:** `crates/zygo/src/cmd/update.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/tests/update.rs`
**Consumes:** `Config.update` (HC)
**Produces:** `update::StartOutcome { Continue, Exit(i32) }`, `update::start_check(paths: &Paths, cfg: &Config, args: &[String]) -> StartOutcome`
**Acceptance:** `cargo test -p zygo --test update && cargo test --workspace --locked --no-run` → all pass and the workspace builds, one test per auto-update acceptance item #1, #2, #3, #4, #5, #6, #7 (the Windows deferral of #7 runs on the windows-latest job of `.github/workflows/ci.yml`)

## Plan
