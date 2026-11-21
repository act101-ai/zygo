# HUD-2 — zygo-work: bounded HUD git function (rev-parse, rev-list only)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.5, AU6, OP14, H§13 (one or two functions). Every test that uses a fake `git` shell script is `cfg(unix)`: the wrapper test, the 400 ms budget test (a stalled fake git) and the 4 KiB cut test (a fake git that writes more than 4 KiB), because a real git neither stalls nor writes 4 KiB (H§7.5). The no-upstream test uses a real `git init` repository and no fake git, so it also runs on Windows. `hud_git` calls `hud_git_with` with the program `git`; the wrapper, budget and cut tests pass their fake `git` script path to `hud_git_with`, so no test changes the process environment
**Files:** `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/hud_git.rs`
**Consumes:** —
**Produces:** `repo::HudGit { top: PathBuf, git_dir: PathBuf, common_dir: PathBuf, branch: Option<String>, ahead: Option<u32>, behind: Option<u32> }`, `repo::hud_git(cwd: &Path, budget: Duration) -> Option<HudGit>`, `repo::hud_git_with(git: &Path, cwd: &Path, budget: Duration) -> Option<HudGit>`
**Acceptance:** `cargo test -p zygo-work --test hud_git && cargo test --workspace --locked --no-run` → all pass: a `git` wrapper passed to `hud_git_with` sees only the two H§7.5 commands with `GIT_NO_LAZY_FETCH=1` and the `git_run` settings; the 400 ms budget kills a stalled fake git; fake-git output above 4 KiB is cut (these three are `cfg(unix)`); a real repository with no upstream gives no ahead/behind (all platforms)

## Plan
