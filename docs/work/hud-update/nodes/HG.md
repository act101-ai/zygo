# HG — zygo-work: bounded HUD git function (rev-parse, rev-list only)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.5, AU6, OP14, H§13 (one or two functions)
**Files:** `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/hud_git.rs`
**Consumes:** —
**Produces:** `repo::HudGit { top: PathBuf, git_dir: PathBuf, common_dir: PathBuf, branch: Option<String>, ahead: Option<u32>, behind: Option<u32> }`, `repo::hud_git(cwd: &Path, budget: Duration) -> Option<HudGit>`
**Acceptance:** `cargo test -p zygo-work --test hud_git` → all pass: a `git` wrapper on `PATH` sees only the two H§7.5 commands with `GIT_NO_LAZY_FETCH=1` and the `git_run` settings; the 400 ms budget kills a stalled git; output above 4 KiB is cut; no upstream gives no ahead/behind

## Plan
