# HUD-2 — zygo-work: bounded HUD git function (rev-parse, rev-list only)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.5, AU6, OP14, H§13 (one or two functions). Every test that uses a fake `git` shell script is `cfg(unix)`: the wrapper test, the 400 ms budget test (a stalled fake git) and the 4 KiB cut test (a fake git that writes more than 4 KiB), because a real git neither stalls nor writes 4 KiB (H§7.5). The no-upstream test uses a real `git init` repository and no fake git, so it also runs on Windows. `hud_git` calls `hud_git_with` with the program `git`; the wrapper, budget and cut tests pass their fake `git` script path to `hud_git_with`, so no test changes the process environment
**Files:** `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/hud_git.rs`, `crates/zygo-config/src/repo_file.rs`, `crates/zygo-config/Cargo.toml`, `crates/zygo-plugins/src/sync.rs`, `Cargo.lock`, `crates/zygo-work/Cargo.toml`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** `repo::HudGit { top: PathBuf, git_dir: PathBuf, common_dir: PathBuf, branch: Option<String>, ahead: Option<u32>, behind: Option<u32> }`, `repo::hud_git(cwd: &Path, budget: Duration) -> Option<HudGit>`, `repo::hud_git_with(git: &Path, cwd: &Path, budget: Duration) -> Option<HudGit>`, `zygo_config::repo_file::git_command_for(program: &OsStr) -> Command`, `zygo_config::repo_file::{kill_group, kill_tree, exited_unreaped}`
**Acceptance:** `cargo test -p zygo-work --test hud_git && cargo test --workspace --locked --no-run` → all pass: a `git` wrapper passed to `hud_git_with` sees only the two H§7.5 commands with `GIT_NO_LAZY_FETCH=1` and the `git_run` settings; the 400 ms budget kills a stalled fake git; fake-git output above 4 KiB is cut (these three are `cfg(unix)`); a real repository with no upstream gives no ahead/behind (all platforms)

## Plan

### Context

**The runner today.** `crates/zygo-work/src/repo.rs` holds the pinned git runner.
- `run_bounded_err` (`:52-94`) kills the child at the deadline and reads all of stdout.
- `git_run` (`:99-110`) builds `git -c core.fsmonitor=false -C <cwd> <args>` on `zygo_config::repo_file::git_command()`. That function (`crates/zygo-config/src/repo_file.rs:81-93`) uses the program `git`, removes `GIT_ENV_CLEARED` and every `GIT_CONFIG*` variable, and sets `LC_ALL=C`. `git_run` then adds `GIT_OPTIONAL_LOCKS=0` and `GIT_TERMINAL_PROMPT=0` and returns a `String`.

**Callers that must not change.** They reach the runner or `git_command` directly or through `checkout`, `find_checkout`, `branch` or `repository_id(_by)`:
- `zygo-work`: `sources.rs:52` (`run_bounded`), `sources.rs:250` (`repository_id_by`), `guards.rs:677` (`checkout`), `current.rs:98` (`checkout`), `current.rs:102` (`branch`), `current.rs:135` (`checkout`), `hooks.rs:83` (`find_checkout`)
- `zygo-launch/src/launcher.rs:136` (`checkout`), `zygo-launch/src/launcher.rs:195` (`repository_id`)
- `zygo/src/cmd/status.rs:19` (`checkout`)
- `zygo-config/src/repo_file.rs:128` (`git_command` in its own `git_out`)

All of them keep today's behavior. They run with `cut: None` (no process group, no cut, no kill after exit), and `git_command()` keeps its behavior.

**What this node adds:**
- `git_command_for(program)` at the source.
- Process helpers in `repo_file.rs`: `kill_tree`, moved from `crates/zygo-plugins/src/sync.rs:100-112`, plus `kill_group` and `exited_unreaped`.
- A capped run mode beside today's mode.
- The HUD function, built on these.

**Why the helpers go in `repo_file.rs`.** The crate graph allows only `zygo-core` or `zygo-config` as a home that both `zygo-plugins` and `zygo-work` depend on. `zygo-core` would need a new `pub mod` line in `crates/zygo-core/src/lib.rs`, and HUD-5 lists that file and runs in parallel with this node. So the helpers go in `zygo-config`, beside `git_command`, the pinned git command they serve. `zygo-config` gains `libc` on unix (`zygo-plugins` already uses `libc` from the workspace).

**Rules from H§7.5 and AU6:**
- Only the two commands below run, each with `GIT_NO_LAZY_FETCH=1`.
- `rev-parse` has 150 ms of the 400 ms.
- At most 4 KiB of stdout is kept.
- `rev-parse` output must be exactly four lines; `HEAD` means detached.
- No upstream, or any `rev-list` error, gives no ahead/behind.

**Author's calls (H§7.5 is silent on these):**
1. **Unborn HEAD gives no git result.** Evidence (git 2.55.0, Linux only): in a repository with no commit, and in an orphan linked worktree, `rev-parse` exits 128 (`ambiguous argument 'HEAD'`). Its stdout still holds the three paths and `HEAD`. `rev-list` exits 128 too. `hud_git_with` requires a zero exit of `rev-parse`, so an unborn HEAD gives `None` on every platform. The rule depends only on the exit status (git fails on an unresolvable revision), not on whether git writes stdout before it fails. The HUD shows no worktree or branch until the first commit.
2. **Cut output is not trusted.** If `rev-parse` stdout reached the 4 KiB cut, the whole result is `None`. If `rev-list` stdout reached it, there are no marks. Real output is a few lines (H§7.5), so a cut means the output is not git's normal answer.
3. **Own process group (unix), tied to PH12.** The cut mode starts git in its own process group, so a group kill aimed at the hook does not reach git. PH12 (how Claude Code ends slow status runs) is still an open live check in PRB-1.
   - While the hook lives, the deadline kill holds each git call within the budget.
   - If the hook is killed before its deadline, a git call that is still running goes on until it ends by itself. The two calls are read-only, run no repository-configured program, and cannot prompt (`GIT_TERMINAL_PROMPT=0`).
   - If PRB-1 finds that Claude Code kills status runs by process group, this call is re-opened.
4. **Kill order (unix).** The group is killed before the leader is reaped.
   - `exited_unreaped` uses `waitid(P_PID, pid, WEXITED | WNOHANG | WNOWAIT)`. While the leader is a zombie, its PID and group id cannot be reused, so `kill(-pgid)` reaches only this group. After the kill, `Child::wait` reaps the leader.
   - Exception: if the process ignores SIGCHLD (`SIG_IGN`), the kernel reaps children by itself, and `waitid` returns -1 (`ECHILD`). The leader's PID may then already be free for reuse. On the first -1, the run falls back at once to today's plain path for its remaining life: `try_wait` polling; at the deadline, or on a `try_wait` error, `child.kill()` and `child.wait()`, which ends with `TimedOut`, as today. The fallback runs no group kill at any point. The zygo hook does not set `SIG_IGN`, so this happens only under a parent that passed it down.
   - An `Err` from `child.wait()` after `kill_group` gives `RunError::Failed`, because the exit status is unknown.
5. **Windows.** At the deadline the cut mode kills the process tree (`taskkill /T /F`, the Windows half of `kill_tree`). This also ends the real `git.exe` that the `cmd\git.exe` wrapper starts. After a normal exit there is no kill. A descendant that holds stdout open can then hold the read until the deadline, at which point the call ends with `TimedOut`.
   - **Overrun.** The worker can pass its budget by the run time of `taskkill`, because `kill_tree` waits for `taskkill` before `child.kill()`.
   - **Why it waits.** `taskkill /T` finds the tree from the live wrapper process. If the run started `taskkill` without waiting and then killed the wrapper, the tree walk could miss the real `git.exe`. Also, `zygo-plugins` keeps `kill_tree` as it is.
   - **Expected bound:** budget + one `taskkill` run, which I expect to be under 2 s. The Windows test in step 5 asserts `budget + 2 s`.
   - **Effect on the HUD.** Under H§7.7, the hook prints at its own deadline and drops a result that is not in. So an overrun delays only the worker thread, not the printed line.
6. **Overflow (C6).** The budget is capped at `HUD_BUDGET_CAP` (24 h) before any `Instant` arithmetic, and the `rev-parse` share is `budget / 8 * 3`.

**Tests.** All tests go in the new file `crates/zygo-work/tests/hud_git.rs`.
- Every test takes `forks()` first and holds it to its end. A fake script written while another test forks gives ETXTBSY; this is the pattern of `crates/zygo/tests/usage_doctor.rs:53-66`.
- The test `git()` helper isolates from the user's git config, as `crates/zygo-work/tests/policy.rs:9-20` does.
- Some tests need an environment variable on git that `git_command` passes through: `GIT_CEILING_DIRECTORIES`, or a caller's `GIT_CONFIG_*`, which `git_command` must remove. These tests run an ignored test of this binary in a child process, with the variables set on that child only. The test process environment does not change. The parent proves the child ran its assertion: the outside test's child writes a marker file after its assertion, and the wrapper test's parent reads the wrapper's log.
- **Ceiling limit (Windows).** The outside test passes the temp path as it is, not canonical, because a `\\?\` path from `canonicalize` may not match git's form of the ceiling. If the runner's temp path still does not match git's form (for example an 8.3 short name such as `RUNNER~1`), git walks up past the ceiling. The test still gives `None`, because the windows-latest temp directory is not inside a repository, but the ceiling is not proven on Windows.
- Budgets: the real-repository tests and the fake-git tests that do not test timing pass `REAL` (5 s). Only the two budget tests pass the spec's 400 ms.

### Steps

- [x] **1. Write the test file head.** Create `crates/zygo-work/tests/hud_git.rs` with:
  ```rust
  use std::ffi::OsStr;
  use std::path::{Path, PathBuf};
  use std::process::Command;
  use std::sync::{Mutex, MutexGuard};
  use std::time::{Duration, Instant};
  use tempfile::TempDir;
  use zygo_work::repo::{hud_git, hud_git_with, HudGit};

  static FORKS: Mutex<()> = Mutex::new(());
  fn forks() -> MutexGuard<'static, ()> { FORKS.lock().unwrap_or_else(|e| e.into_inner()) }
  const REAL: Duration = Duration::from_secs(5);
  const HUD: Duration = Duration::from_millis(400);

  fn git(dir: &Path, args: &[&str]) {
      let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
      let out = Command::new("git").current_dir(dir)
          .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(args)
          .env("GIT_CONFIG_GLOBAL", null).env("GIT_CONFIG_NOSYSTEM", "1")
          .output().unwrap();
      assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
  }
  /// `<tmp>/w`: a repository on `main`, with one commit when `commit`.
  fn repo(commit: bool) -> (TempDir, PathBuf) {
      let t = TempDir::new().unwrap();
      git(t.path(), &["init", "-q", "-b", "main", "w"]);
      let w = t.path().join("w");
      if commit { git(&w, &["commit", "-q", "--allow-empty", "-m", "a"]); }
      (t, w)
  }
  fn canon(p: &Path) -> PathBuf { p.canonicalize().unwrap() }
  /// Runs the ignored test `name` of this binary in a child process with `env` set on that child only.
  fn run_child(name: &str, env: &[(&str, &OsStr)]) {
      let out = Command::new(std::env::current_exe().unwrap())
          .args([name, "--exact", "--ignored", "--test-threads=1", "--nocapture"])
          .envs(env.iter().copied())
          .output().unwrap();
      let (so, se) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
      assert!(out.status.success() && so.contains("1 passed"), "{name}\nstdout:\n{so}\nstderr:\n{se}");
  }
  /// An executable `#!/bin/sh` program `<dir>/git`; the caller holds `forks()`.
  #[cfg(unix)]
  fn fake_git(dir: &Path, body: &str) -> PathBuf {
      use std::os::unix::fs::PermissionsExt;
      let p = dir.join("git");
      std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
      std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
      p
  }
  /// `<t>/w` and `<t>/w/.git` as plain directories; returns `w` and the text `<w>\n<w>/.git\n<w>/.git\n`
  /// (paths as created, not canonical).
  #[cfg(unix)]
  fn fake_tree(t: &Path) -> (PathBuf, String) {
      let w = t.join("w");
      std::fs::create_dir_all(w.join(".git")).unwrap();
      let g = w.join(".git");
      (w.clone(), format!("{}\n{}\n{}\n", w.display(), g.display(), g.display()))
  }
  /// A fake git that prints `rev_parse` (from `<t>/out`) for `rev-parse` and runs `other` for every other call.
  #[cfg(unix)]
  fn fake_rev_parse(t: &Path, rev_parse: &str, other: &str) -> PathBuf {
      let out = t.join("out");
      std::fs::write(&out, rev_parse).unwrap();
      fake_git(t, &format!("case \"$*\" in\n  *rev-parse*) cat '{}' ;;\n  *) {other} ;;\nesac", out.display()))
  }
  /// Asserts that the PID written to `pid_file` is gone or a zombie within 2 s.
  #[cfg(unix)]
  fn assert_gone(pid_file: &Path) {
      let pid = std::fs::read_to_string(pid_file).unwrap().trim().to_string();
      let gone = || { // no process, or a zombie not yet reaped by init
          let o = Command::new("ps").args(["-o", "stat=", "-p", &pid]).output().unwrap();
          let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
          s.is_empty() || s.starts_with('Z')
      };
      let until = Instant::now() + Duration::from_secs(2);
      while !gone() && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)); }
      assert!(gone(), "process {pid} still runs");
  }
  /// [`fake_rev_parse`] whose `rev-list` prints `rev_list` (from `<t>/list`) and exits 0.
  #[cfg(unix)]
  fn fake_both(t: &Path, rev_parse: &str, rev_list: &str) -> PathBuf {
      let list = t.join("list");
      std::fs::write(&list, rev_list).unwrap();
      fake_rev_parse(t, rev_parse, &format!("cat '{}'", list.display()))
  }
  ```
  Check: none yet (the file compiles at step 9).

- [x] **2. Write the all-platform real-repository tests** in `hud_git.rs`:
  ```rust
  #[test] // H§7.5 call 2: no upstream → no ahead/behind; main worktree: git dir = common dir
  fn hud_git_no_upstream_gives_no_marks() {
      let _f = forks();
      let (_t, w) = repo(true);
      assert_eq!(hud_git(&w, REAL), Some(HudGit { top: canon(&w), git_dir: canon(&w.join(".git")),
          common_dir: canon(&w.join(".git")), branch: Some("main".into()), ahead: None, behind: None }));
  }

  #[test] // H§7.5 call 1: `HEAD` means detached
  fn hud_git_detached_head_has_no_branch() {
      let _f = forks();
      let (_t, w) = repo(true);
      git(&w, &["checkout", "-q", "--detach"]);
      let g = hud_git(&w, REAL).unwrap();
      assert_eq!((g.branch, g.ahead, g.behind), (None, None, None));
  }

  #[test] // author's call 1: rev-parse exits non-zero on an unborn HEAD → none
  fn hud_git_unborn_head_is_none() {
      let _f = forks();
      let (_t, w) = repo(false);
      assert_eq!(hud_git(&w, REAL), None);
  }

  #[test] // H§7.5: linked worktree ⇔ git dir ≠ common dir; `<ahead>\t<behind>` against the upstream
  fn hud_git_linked_worktree_ahead_and_behind() {
      let _f = forks();
      let (t, w) = repo(true);
      git(&w, &["worktree", "add", "-q", "../linked", "-b", "feat"]);
      let l = t.path().join("linked");
      git(&l, &["commit", "-q", "--allow-empty", "-m", "b"]);
      git(&w, &["commit", "-q", "--allow-empty", "-m", "c"]);
      git(&l, &["branch", "-q", "--set-upstream-to=main"]);
      assert_eq!(hud_git(&l, REAL), Some(HudGit { top: canon(&l), git_dir: canon(&w.join(".git/worktrees/linked")),
          common_dir: canon(&w.join(".git")), branch: Some("feat".into()), ahead: Some(1), behind: Some(1) }));
  }

  #[test] // not a repository → none; GIT_CEILING_DIRECTORIES on the child only (as tests/sources.rs:19-23)
  fn hud_git_outside_a_repository_is_none() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let dir = t.path().join("d");
      std::fs::create_dir(&dir).unwrap();
      let mark = t.path().join("mark");
      // unix: canonical, as sources.rs (macOS /var → /private/var); Windows: not canonical, because a `\\?\` path
      // may not match git's form of the ceiling
      let base = if cfg!(windows) { t.path().to_path_buf() } else { canon(t.path()) };
      run_child("hud_git_outside_child", &[("HUD_GIT_DIR", dir.as_os_str()), ("HUD_GIT_MARK", mark.as_os_str()),
          ("GIT_CEILING_DIRECTORIES", base.as_os_str())]);
      assert_eq!(std::fs::read_to_string(&mark).unwrap(), "none"); // the child ran the assertion
  }

  /// Run only by `hud_git_outside_a_repository_is_none`, in a child process; passes when run alone.
  #[test]
  #[ignore = "run by hud_git_outside_a_repository_is_none"]
  fn hud_git_outside_child() {
      let (Some(d), Some(mark)) = (std::env::var_os("HUD_GIT_DIR"), std::env::var_os("HUD_GIT_MARK")) else { return };
      assert_eq!(hud_git(Path::new(&d), REAL), None);
      std::fs::write(mark, "none").unwrap();
  }

  #[test] // author's call 6 (C6): the largest budget does not panic
  fn hud_git_max_budget_does_not_overflow() {
      let _f = forks();
      let (_t, w) = repo(true);
      assert_eq!(hud_git(&w, Duration::MAX).unwrap().branch.as_deref(), Some("main"));
  }
  ```
  Check: none yet.

- [x] **3. Write the `cfg(unix)` wrapper test and its child** in `hud_git.rs`.
  - The parent runs the child with `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_0` and `GIT_CONFIG_VALUE_0` set on that child only.
  - The wrapper logs what reaches it, then runs the real git with isolated config. It must set `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_NOSYSTEM` itself, because `git_command` removes every `GIT_CONFIG*` variable.
  ```rust
  #[cfg(unix)]
  #[test] // H§7.5, AU6: only the two commands; GIT_NO_LAZY_FETCH=1 and the git_run settings; no caller GIT_CONFIG_*
  fn hud_git_wrapper_sees_only_two_commands() {
      let _f = forks();
      let (_t, w) = repo(true);
      git(&w, &["checkout", "-q", "-b", "feat"]);
      git(&w, &["commit", "-q", "--allow-empty", "-m", "b"]);
      git(&w, &["branch", "-q", "--set-upstream-to=main"]);
      let bin = TempDir::new().unwrap();
      let log = bin.path().join("log");
      let fake = fake_git(bin.path(), &format!(
          "printf '%s|%s|%s|%s|%s|%s|%s|%s\\n' \"$*\" \"${{GIT_NO_LAZY_FETCH-unset}}\" \"${{GIT_OPTIONAL_LOCKS-unset}}\" \
           \"${{GIT_TERMINAL_PROMPT-unset}}\" \"${{LC_ALL-unset}}\" \"${{GIT_CONFIG_COUNT-unset}}\" \
           \"${{GIT_CONFIG_KEY_0-unset}}\" \"${{GIT_CONFIG_VALUE_0-unset}}\" >> '{}'\n\
           GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 exec git \"$@\"",
          log.display()));
      run_child("hud_git_wrapper_child", &[("HUD_GIT_FAKE", fake.as_os_str()), ("HUD_GIT_REPO", w.as_os_str()),
          ("GIT_CONFIG_COUNT", OsStr::new("1")), ("GIT_CONFIG_KEY_0", OsStr::new("core.abbrev")),
          ("GIT_CONFIG_VALUE_0", OsStr::new("12"))]);
      let pre = format!("-c core.fsmonitor=false -C {}", w.display());
      let env = "|1|0|0|C|unset|unset|unset";
      assert_eq!(std::fs::read_to_string(&log).unwrap(), format!(
          "{pre} rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir --abbrev-ref HEAD{env}\n\
           {pre} rev-list --left-right --count HEAD...@{{upstream}}{env}\n"));
  }

  /// Run only by `hud_git_wrapper_sees_only_two_commands`, in a child process; passes when run alone.
  #[cfg(unix)]
  #[test]
  #[ignore = "run by hud_git_wrapper_sees_only_two_commands"]
  fn hud_git_wrapper_child() {
      let (Some(fake), Some(w)) = (std::env::var_os("HUD_GIT_FAKE"), std::env::var_os("HUD_GIT_REPO")) else { return };
      let g = hud_git_with(Path::new(&fake), Path::new(&w), REAL).unwrap();
      assert_eq!((g.branch.as_deref(), g.ahead, g.behind), (Some("feat"), Some(1), Some(0)));
  }
  ```
  Check: none yet.

- [x] **4. Write the budget tests** (spec 400 ms; two `cfg(unix)`, one `cfg(windows)`) in `hud_git.rs`. Each unix fake writes its own PID (`$$`, kept by `exec`), and `assert_gone` proves the kill:
  ```rust
  #[cfg(unix)]
  #[test] // AU6: rev-parse has 150 ms of the 400 ms
  fn hud_git_rev_parse_has_150_ms() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let pid = t.path().join("pid");
      let fake = fake_git(t.path(), &format!("echo $$ > '{}'; exec sleep 5", pid.display()));
      let start = Instant::now();
      assert_eq!(hud_git_with(&fake, t.path(), HUD), None);
      let took = start.elapsed();
      assert!(took >= Duration::from_millis(150) && took < HUD, "{took:?}");
      assert_gone(&pid);
  }

  #[cfg(unix)]
  #[test] // AU6: the 400 ms budget kills a stalled rev-list; call 1 still gives worktree and branch
  fn hud_git_budget_kills_stalled_rev_list() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, paths) = fake_tree(t.path());
      let pid = t.path().join("pid");
      let fake = fake_rev_parse(t.path(), &format!("{paths}main\n"), &format!("echo $$ > '{}'; exec sleep 5", pid.display()));
      let start = Instant::now();
      let g = hud_git_with(&fake, &w, HUD).unwrap();
      let took = start.elapsed();
      assert!(took >= HUD && took < Duration::from_secs(1), "{took:?}");
      assert_eq!((g.top, g.branch.as_deref(), g.ahead, g.behind), (canon(&w), Some("main"), None, None));
      assert_gone(&pid);
  }

  #[cfg(windows)]
  #[test] // author's call 5: a stalled fake `git.cmd` is killed at the 150 ms rev-parse deadline; bound budget + 2 s
  fn hud_git_windows_deadline_kills_stalled_fake() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let fake = t.path().join("git.cmd");
      std::fs::write(&fake, "@ping -n 6 127.0.0.1 >NUL\r\n").unwrap();
      let start = Instant::now();
      assert_eq!(hud_git_with(&fake, t.path(), HUD), None);
      let took = start.elapsed();
      assert!(took >= Duration::from_millis(150) && took < HUD + Duration::from_secs(2), "{took:?}");
  }
  ```
  On Windows, `std::process::Command` runs a `.cmd` path through `cmd.exe`. The `rev-parse` arguments hold no character that Rust's batch-file argument check refuses (`%`, a quote, or a line break). If the check refuses them on the runner, `hud_git_with` gets `NotRun` and returns `None` at once, so the test would pass without a kill. To catch that, the elapsed time must also be at least 150 ms: add `assert!(took >= Duration::from_millis(150))`. If that assert fails for this reason, the executor records the reason in this node's log and removes the test. The Windows deadline path then stays covered by review only.
  Check: none yet.

- [x] **5. Write the `cfg(unix)` output tests** (budget `REAL`) in `hud_git.rs`. Without the cut rule, the two cut fakes would give `Some(branch = 1 MiB of b)` and marks `(1, 2)` (`trim_end` drops the spaces). So the asserted results prove that the reader stopped at 4 KiB and that the cut was detected:
  ```rust
  #[cfg(unix)]
  #[test] // H§7.5 4 KiB cut; author's call 2: cut rev-parse output → none, within the budget
  fn hud_git_cut_rev_parse_is_none() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, paths) = fake_tree(t.path());
      let fake = fake_rev_parse(t.path(), &format!("{paths}{}\n", "b".repeat(1 << 20)), "exit 1");
      let start = Instant::now();
      assert_eq!(hud_git_with(&fake, &w, REAL), None);
      assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
  }

  #[cfg(unix)]
  #[test] // H§7.5 4 KiB cut; author's call 2: cut rev-list output → no marks
  fn hud_git_cut_rev_list_gives_no_marks() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, paths) = fake_tree(t.path());
      let fake = fake_both(t.path(), &format!("{paths}main\n"), &format!("1\t2{}\n", " ".repeat(1 << 20)));
      let g = hud_git_with(&fake, &w, REAL).unwrap();
      assert_eq!((g.branch.as_deref(), g.ahead, g.behind), (Some("main"), None, None));
  }

  #[cfg(unix)]
  #[test] // H§7.5: exactly four lines; four valid lines and one more → none
  fn hud_git_needs_exactly_four_lines() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, paths) = fake_tree(t.path());
      let fake = fake_rev_parse(t.path(), &format!("{paths}main\nx\n"), "exit 1");
      assert_eq!(hud_git_with(&fake, &w, REAL), None);
  }

  #[cfg(unix)]
  #[test] // lines 1-3 must be existing paths
  fn hud_git_non_path_line_is_none() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, _) = fake_tree(t.path());
      let text = format!("{}\n{}\n{}\nmain\n", w.display(), t.path().join("missing").display(), w.join(".git").display());
      let fake = fake_rev_parse(t.path(), &text, "exit 1");
      assert_eq!(hud_git_with(&fake, &w, REAL), None);
  }

  #[cfg(unix)]
  #[test] // an empty fourth line → no branch; the worktree stays
  fn hud_git_empty_head_line_has_no_branch() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, paths) = fake_tree(t.path());
      let fake = fake_rev_parse(t.path(), &format!("{paths}\n"), "exit 1");
      let g = hud_git_with(&fake, &w, REAL).unwrap();
      assert_eq!((g.top, g.branch, g.ahead, g.behind), (canon(&w), None, None, None));
  }

  #[cfg(unix)]
  #[test] // rev-list exit 0 with malformed stdout (no tab; not a number) → no marks
  fn hud_git_malformed_rev_list_gives_no_marks() {
      let _f = forks();
      for list in ["12\n", "x\t1\n"] {
          let t = TempDir::new().unwrap();
          let (w, paths) = fake_tree(t.path());
          let fake = fake_both(t.path(), &format!("{paths}main\n"), list);
          let g = hud_git_with(&fake, &w, REAL).unwrap();
          assert_eq!((g.branch.as_deref(), g.ahead, g.behind), (Some("main"), None, None), "{list:?}");
      }
  }

  #[cfg(unix)]
  #[test] // author's call 4: a descendant that keeps stdout open neither delays the result nor outlives the call
  fn hud_git_kills_a_descendant_holding_stdout() {
      let _f = forks();
      let t = TempDir::new().unwrap();
      let (w, paths) = fake_tree(t.path());
      let pid = t.path().join("pid");
      let out = t.path().join("out");
      std::fs::write(&out, format!("{paths}main\n")).unwrap();
      let fake = fake_git(t.path(), &format!(
          "case \"$*\" in\n  *rev-parse*) cat '{}'; sleep 7 & echo $! > '{}' ;;\n  *) exit 1 ;;\nesac",
          out.display(), pid.display()));
      let start = Instant::now();
      let g = hud_git_with(&fake, &w, REAL).unwrap();
      assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
      assert_eq!(g.branch.as_deref(), Some("main"));
      assert_gone(&pid);
  }
  ```
  Check: none yet.

- [x] **6. Run the tests and see them fail.** `cargo test -p zygo-work --test hud_git`. Pass condition for this step: compile errors `unresolved imports zygo_work::repo::hud_git`, `HudGit` (and `hud_git_with` on unix).

- [x] **7. Add the source helpers in `zygo-config` and reuse `kill_tree` in `zygo-plugins`.**
  - In `crates/zygo-config/Cargo.toml` add:
    ```toml
    [target.'cfg(unix)'.dependencies]
    libc.workspace = true
    ```
  - In `crates/zygo-config/src/repo_file.rs` add `use std::ffi::OsStr;`. Move the body of `git_command` (`:81-93`) into `git_command_for`, with `Command::new(program)` and the body otherwise unchanged. Keep the doc comment of `git_command` (`:79-80`) on `git_command`. Add the process helpers:
    ```rust
    pub fn git_command() -> Command { git_command_for(OsStr::new("git")) }

    /// [`git_command`] with `program` in place of `git` (a test's fake `git`).
    pub fn git_command_for(program: &OsStr) -> Command

    /// Kills `child`'s process group (unix: `libc::kill(-(child.id() as i32), libc::SIGKILL)`; the child must have been
    /// started with `process_group(0)` and not yet reaped) or its process tree (Windows: `taskkill /T /F /PID <id>`,
    /// stdout and stderr null; a descendant that left the tree survives). Does not reap.
    pub fn kill_group(child: &std::process::Child)

    /// [`kill_group`], then `child.kill()` and `child.wait()`, both results ignored.
    pub fn kill_tree(child: &mut std::process::Child)

    /// Whether `child` has exited, without reaping it: with `info` zeroed, `waitid(P_PID, child.id(), &mut info,
    /// WEXITED | WNOHANG | WNOWAIT)`. A return of 0 gives `Ok(info.si_pid() != 0)`. A return of -1 gives
    /// `Err(std::io::Error::last_os_error())`, e.g. `ECHILD` when SIGCHLD is ignored and the kernel reaps by itself.
    #[cfg(unix)]
    pub fn exited_unreaped(child: &std::process::Child) -> std::io::Result<bool>
    ```
    `kill_group` plus `kill_tree` are the body of `kill_tree` at `crates/zygo-plugins/src/sync.rs:100-112`, split in two.
  - In `crates/zygo-plugins/src/sync.rs`, delete `fn kill_tree` (`:99-112`, with its doc comment) and add `use zygo_config::repo_file::kill_tree;`. The call at `:83` does not change.

  Check:
  - `cargo build -p zygo-config` updates `Cargo.lock`. `git diff Cargo.lock` shows only `"libc",` added to the `zygo-config` package entry.
  - `cargo test -p zygo-config` and `cargo test -p zygo-plugins` pass.

- [x] **8. Add the capped run mode** in `repo.rs`. Move the body of `run_bounded_err` (`:52-94`) into a new function, and make `run_bounded_err` call it:
  ```rust
  /// The end of a bounded run that exited: exit success, stdout (at most the cut), whether stdout passed the cut,
  /// and up to 64 KiB of stderr (read only on a non-zero exit, else empty).
  struct Ran { ok: bool, out: Vec<u8>, cut: bool, err: Vec<u8> }

  pub(crate) fn run_bounded_err(cmd: Command, deadline: Instant) -> Result<Vec<u8>, (RunError, Vec<u8>)> {
      match run_bounded_out(cmd, deadline, None) {
          Ok(Ran { ok: true, out, .. }) => Ok(out),
          Ok(Ran { err, .. }) => Err((RunError::Failed, err)),
          Err(e) => Err((e, Vec::new())),
      }
  }

  /// Runs `cmd` with stdin null until it exits or `deadline`. `cut: None` is the plain mode of [`run_bounded_err`].
  /// `Some(n)`: stdout keeps `n` bytes (the rest is drained) and `Ran.cut` tells whether there were more. The child is
  /// also killed with its descendants: on unix it runs in its own process group, and the group is killed at the deadline
  /// and once the child has exited, before it is reaped. On Windows the process tree is killed at the deadline.
  fn run_bounded_out(cmd: Command, deadline: Instant, cut: Option<u64>) -> Result<Ran, RunError>
  ```
  Keep the doc comment of `run_bounded_err` (`:49-51`). The body is today's, with these differences only:
  - The stdout thread reads `(&mut out).take(limit).read_to_end(&mut buf)`, with exactly `let limit = cut.map_or(u64::MAX, |n| n.saturating_add(1));` (no `+ 1` on `u64::MAX`, which panics in debug). With `Some(n)`, `buf.len() as u64 > n` sets `cut = true` and truncates `buf` to `n`. Then it runs `std::io::copy(&mut out, &mut std::io::sink())` (the stderr thread's pattern, `:72-73`). It sends `(buf, cut)`.
  - With `Some` on unix, `CommandExt::process_group(&mut cmd, 0)` runs before `spawn`. The wait loop then polls `zygo_config::repo_file::exited_unreaped(&child)` in place of `try_wait`:
    - `Ok(true)`: call `kill_group(&child)`, then `child.wait()` for the status. An `Err` from that `wait` returns `Err(RunError::Failed)`.
    - `Ok(false)`: before the deadline, sleep 10 ms; at the deadline, `kill_tree(&mut child)` and return `Err(RunError::TimedOut)`.
    - `Err(_)`: at once, and for the rest of this run, use the plain loop of today (`try_wait`; at the deadline or on a `try_wait` error, `child.kill()`, `child.wait()` and `TimedOut`). There is no group kill after this point (author's call 4).
  - With `Some` on Windows, the loop is today's `try_wait` loop, except that the deadline branch calls `kill_tree(&mut child)` in place of `child.kill(); child.wait()` (author's call 5).
  - The result is `Ok(Ran { ok: status.success(), out, cut, err })`. `err` is `erx.recv_timeout(left()).unwrap_or_default()` only when the exit is non-zero, else empty. The `TimedOut` and `Failed` (no pipe) paths stay.

  With `None`, the code paths are today's: `try_wait`, `child.kill()` and `wait()` at the deadline, no process group and no group kill.

  Then split `git_run` (`:99-110`) into a command builder and the run. Keep the doc comment of `git_run` on `git_run`:
  ```rust
  /// `<program> -c core.fsmonitor=false -C <cwd> <args>` on `git_command_for`, with `GIT_OPTIONAL_LOCKS=0` and
  /// `GIT_TERMINAL_PROMPT=0`.
  fn git_cmd(program: &OsStr, cwd: &Path, args: &[&str]) -> Command

  pub(crate) fn git_run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<String, RunError>
  ```
  `git_run` builds `git_cmd(OsStr::new("git"), cwd, args)` and keeps its `match` (`:103-109`) on `run_bounded_err`.

  Check: `cargo test -p zygo-work --lib --test collection --test guards --test policy --test sources` passes. `hud_git` is not built yet; it compiles at step 9. These tests check the plain mode (`cut: None`), which a `take(u64::MAX + 1)` would make panic:
  - `repo::tests::find_checkout_tells_timeout_from_not_a_repository` (`repo.rs:205-212`): `git_run` → `run_bounded_err`.
  - The `sources` tests: `run_bounded` through `run_json`, `sources.rs:52`.

- [x] **9. Add the HUD type and functions** in `repo.rs`:
  ```rust
  /// Total budget cap of [`hud_git_with`]: no `Instant` arithmetic can overflow (C6).
  const HUD_BUDGET_CAP: Duration = Duration::from_secs(24 * 60 * 60);
  /// Stdout kept of each HUD git call (H§7.5).
  const HUD_CUT: u64 = 4 * 1024;

  /// The HUD's git facts (H§7.5): `top`, `git_dir` and `common_dir` are canonical; a linked worktree has
  /// `git_dir != common_dir`.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct HudGit {
      pub top: PathBuf,
      pub git_dir: PathBuf,
      pub common_dir: PathBuf,
      /// `None` when detached (`rev-parse` prints `HEAD`) or the line is empty.
      pub branch: Option<String>,
      /// Both `None` with no upstream, any `rev-list` error, or output past the cut.
      pub ahead: Option<u32>,
      pub behind: Option<u32>,
  }

  /// [`hud_git_with`] with the program `git`.
  pub fn hud_git(cwd: &Path, budget: Duration) -> Option<HudGit> { hud_git_with(Path::new("git"), cwd, budget) }

  /// The two HUD git calls (H§7.5) in `cwd` within `budget` (`rev-parse` gets 3/8 of it), each with
  /// `GIT_NO_LAZY_FETCH=1` and at most 4 KiB of stdout. On unix each call runs in its own process group, which is
  /// killed at the deadline and when the call exits. On Windows the call's process tree is killed at the deadline.
  /// None when `rev-parse` fails, times out, passes the cut, or does not print four lines that start with three
  /// existing paths.
  pub fn hud_git_with(git: &Path, cwd: &Path, budget: Duration) -> Option<HudGit>
  ```
  `hud_git_with` algorithm:
  1. Compute `budget = budget.min(HUD_BUDGET_CAP)`, `start = Instant::now()`, `deadline = start + budget`, and the rev-parse deadline `start + budget / 8 * 3` (150 ms of 400 ms, AU6).
  2. Build `c = git_cmd(git.as_os_str(), cwd, &["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-dir", "--git-common-dir", "--abbrev-ref", "HEAD"])` and set `c.env("GIT_NO_LAZY_FETCH", "1")`. Run `run_bounded_out(c, rev_parse_deadline, Some(HUD_CUT)).ok()?`. Continue only on `ok && !cut`; else return `None`.
  3. Strip one trailing `b'\n'` if present and split on `b'\n'`. Anything other than exactly four parts gives `None`.
  4. Lines 1-3 → path. On unix use `OsStr::from_bytes`, which keeps invalid UTF-8 for the lossy name in HUD-9. Elsewhere use `String::from_utf8_lossy`. Then `canonicalize().ok()?`.
  5. Line 4 → `branch`: `None` when it is empty or equals `b"HEAD"`, else `String::from_utf8_lossy(line).into_owned()`.
  6. Only now run `rev-list --left-right --count HEAD...@{upstream}` the same way (with `GIT_NO_LAZY_FETCH=1`, `Some(HUD_CUT)` and `deadline`). Only `Ok(Ran { ok: true, cut: false, .. })` counts. Read its stdout as UTF-8, `trim_end`, `split_once('\t')`, and parse both sides with `parse::<u32>()`. Success gives `(Some(ahead), Some(behind))`; any failure gives `(None, None)`.

  Check: `cargo build -p zygo-work --all-targets` passes with no warning.

- [x] **10. Run the tests and see them pass.**
  - `cargo test -p zygo-work --test hud_git`: on unix 16 passed, 0 failed, 2 ignored (`hud_git_outside_child`, `hud_git_wrapper_child`); on Windows 7 passed, 1 ignored.
  - `cargo test -p zygo-work --test hud_git -- --ignored` → 2 passed on unix, 1 on Windows (the children return early with no environment).
  - `cargo test -p zygo-work`, `cargo test -p zygo-config` and `cargo test -p zygo-plugins` → all pass.

- [x] **11. Lint.** `cargo clippy -p zygo-work -p zygo-config -p zygo-plugins --all-targets --locked -- -D warnings` → no warning.

- [x] **12. Commit** `crates/zygo-config/Cargo.toml`, `crates/zygo-config/src/repo_file.rs`, `crates/zygo-plugins/src/sync.rs`, `Cargo.lock`, `crates/zygo-work/src/repo.rs` and `crates/zygo-work/tests/hud_git.rs` with the message:
  ```
  zygo-work: bounded HUD git function (rev-parse, rev-list only)

  hud_git/hud_git_with run the two H§7.5 commands on the pinned git runner
  with GIT_NO_LAZY_FETCH=1, 150 ms of the 400 ms for rev-parse, stdout cut
  at 4 KiB, and the call's process group (unix) or tree (Windows) killed.
  kill_tree moves from zygo-plugins to zygo_config::repo_file for reuse.

  Zygo-Session: <the executor's own zygo session id>
  Zygo-Work: HUD-2
  ```
  Check: `git status --short` shows none of the six files.

- [x] **13. Acceptance.** `cargo test -p zygo-work --test hud_git && cargo test --workspace --locked --no-run` → all pass: a `git` wrapper passed to `hud_git_with` sees only the two H§7.5 commands with `GIT_NO_LAZY_FETCH=1` and the `git_run` settings; the 400 ms budget kills a stalled fake git; fake-git output above 4 KiB is cut (these three are `cfg(unix)`); a real repository with no upstream gives no ahead/behind (all platforms).

### Interfaces

**Consumes** (existing code):
- `zygo_config::repo_file::git_command() -> std::process::Command` (`crates/zygo-config/src/repo_file.rs:81`). Its behavior does not change; after step 7 it calls `git_command_for`.
- `kill_tree(child: &mut Child)` from `crates/zygo-plugins/src/sync.rs:100-112`. It moves to `repo_file.rs` with the same behavior.
- `crate::repo::RunError` (`repo.rs:25`)
- `libc::{kill, SIGKILL, waitid, siginfo_t, P_PID, WEXITED, WNOHANG, WNOWAIT}` (unix; workspace `libc = "0.2.191"`)

**Produces:**
- In `zygo_config::repo_file`:
  - `pub fn git_command_for(program: &OsStr) -> Command`
  - `pub fn kill_group(child: &Child)`
  - `pub fn kill_tree(child: &mut Child)`
  - `#[cfg(unix)] pub fn exited_unreaped(child: &Child) -> std::io::Result<bool>`
- `pub struct repo::HudGit { pub top: PathBuf, pub git_dir: PathBuf, pub common_dir: PathBuf, pub branch: Option<String>, pub ahead: Option<u32>, pub behind: Option<u32> }`. It derives `Debug, Clone, PartialEq, Eq`, and its paths are canonical. Consumer: HUD-9 `collect`.
- `pub fn repo::hud_git(cwd: &Path, budget: Duration) -> Option<HudGit>`. Consumer: HUD-9, with a budget of 400 ms.
- `pub fn repo::hud_git_with(git: &Path, cwd: &Path, budget: Duration) -> Option<HudGit>`
- Crate-internal:
  - `struct Ran { ok: bool, out: Vec<u8>, cut: bool, err: Vec<u8> }`
  - `fn run_bounded_out(cmd: Command, deadline: Instant, cut: Option<u64>) -> Result<Ran, RunError>`
  - `fn git_cmd(program: &OsStr, cwd: &Path, args: &[&str]) -> Command`
  - `run_bounded`, `run_bounded_err` and `git_run` keep their signatures and behavior.

### Round 2

Diff-review findings, confirmed by the coordinator; each fixed test first.

- [x] **R1 (M2).** SIGCHLD set to `SIG_IGN` by the parent: in the cut mode, ECHILD from `try_wait` after the `waitid` fallback means the child has exited (the kernel reaped it); stdout is read as usual, with no group kill. Plain mode unchanged. Test: `hud_git_sigchld_ignored_still_gives_the_branch` (cfg(unix), child process sets `SIG_IGN` through libc).
- [x] **R2 (L1).** `rev-list ... HEAD...@{upstream} --`; the wrapper test's expected line gains `--`. Test: `hud_git_file_named_like_the_range_keeps_the_marks`.
- [x] **R3 (L2).** Branch read with `--symbolic-full-name HEAD` (git 2.55.0: `refs/heads/main` on a branch and with a tag `main`, `HEAD` when detached, exit 128 when unborn); `refs/heads/` stripped, anything else → no branch. Test: `hud_git_tag_named_like_the_branch_keeps_the_branch`.
- [x] **R4 (L3).** `hud_git_rev_parse_has_150_ms`: a missing or empty PID file does not panic; no process with the fake's unique marker remains (`ps -A -ww -o args=`). `hud_git_budget_kills_stalled_rev_list`: bound budget + 1 s, with the reason in a comment.
- [x] **R5 (L4).** Windows test: the `ping.exe` that the fake starts is gone after the call (`tasklist` count before and after; reason in a comment). Unverified until windows-latest CI runs.
- [x] **R6 (L5).** `hud_git_with` doc comment states the fallback (no group kill after a `waitid` error; ECHILD from `try_wait` counts as an exit).
- [x] **R7 (L6).** zygo-config unit tests for `kill_group` and `exited_unreaped`; the `child.wait()` error → `Failed` path, or the reason it cannot be reached without fakes. Tests: `kill_group_kills_the_childs_group`, `exited_unreaped_tells_an_exit_without_reaping`. The `child.wait()` error path has no test: it needs a third party to reap the leader between `waitid(WNOWAIT)` seeing the exit and `child.wait()`. std has no hook in that gap. A `waitpid(-1)` thread or a `SIG_IGN` set at that moment is a race, not a fixed order. With `SIG_IGN` set before the call, `waitid` gives ECHILD first, which is the R1 path.
- [x] **R8.** H§7.5 (and the AU11 detached-head note) rewritten as final text for R2 and R3; one `## Change log` line; spec added to the Files line.
