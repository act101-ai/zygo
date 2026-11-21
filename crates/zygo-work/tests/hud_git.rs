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
/// `<t>/m/sleep`: a link to the system `sleep`. Exec'd by its full path, the process's arguments hold the unique
/// temporary path `t`; the base name stays `sleep` for a multi-call coreutils binary.
#[cfg(unix)]
fn marked_sleep(t: &Path) -> PathBuf {
    let o = Command::new("sh").args(["-c", "command -v sleep"]).output().unwrap();
    let real = String::from_utf8(o.stdout).unwrap().trim().to_string();
    std::fs::create_dir(t.join("m")).unwrap();
    let p = t.join("m").join("sleep");
    std::os::unix::fs::symlink(real, &p).unwrap();
    p
}
/// Asserts that within 2 s no process has `marker` in its arguments (`ps -A -ww -o args=`; a zombie shows only its
/// name).
#[cfg(unix)]
fn assert_no_process_with(marker: &str) {
    let found = || {
        let o = Command::new("ps").args(["-A", "-ww", "-o", "args="]).output().unwrap();
        assert!(o.status.success(), "ps failed");
        String::from_utf8_lossy(&o.stdout).lines().any(|l| l.contains(marker))
    };
    let until = Instant::now() + Duration::from_secs(2);
    while found() && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)); }
    assert!(!found(), "a process with {marker} in its arguments still runs");
}
/// [`fake_rev_parse`] whose `rev-list` prints `rev_list` (from `<t>/list`) and exits 0.
#[cfg(unix)]
fn fake_both(t: &Path, rev_parse: &str, rev_list: &str) -> PathBuf {
    let list = t.join("list");
    std::fs::write(&list, rev_list).unwrap();
    fake_rev_parse(t, rev_parse, &format!("cat '{}'", list.display()))
}

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

#[test] // review L1: a file named like the range does not make `rev-list` ambiguous (`--` ends the revisions)
fn hud_git_file_named_like_the_range_keeps_the_marks() {
    let _f = forks();
    let (_t, w) = repo(true);
    git(&w, &["checkout", "-q", "-b", "feat"]);
    git(&w, &["commit", "-q", "--allow-empty", "-m", "b"]);
    git(&w, &["branch", "-q", "--set-upstream-to=main"]);
    std::fs::write(w.join("HEAD...@{upstream}"), "").unwrap();
    let g = hud_git(&w, REAL).unwrap();
    assert_eq!((g.branch.as_deref(), g.ahead, g.behind), (Some("feat"), Some(1), Some(0)));
}

#[test] // review L2: a tag named like the branch keeps the plain branch name (`--abbrev-ref` gives `heads/main`)
fn hud_git_tag_named_like_the_branch_keeps_the_branch() {
    let _f = forks();
    let (_t, w) = repo(true);
    git(&w, &["tag", "main"]);
    assert_eq!(hud_git(&w, REAL).unwrap().branch.as_deref(), Some("main"));
}

#[cfg(unix)]
#[test] // review M2: under SIGCHLD = SIG_IGN the kernel reaps git; the result still comes in
fn hud_git_sigchld_ignored_still_gives_the_branch() {
    let _f = forks();
    let (t, w) = repo(true);
    let mark = t.path().join("mark");
    run_child("hud_git_sigchld_child", &[("HUD_GIT_SIGCHLD_REPO", w.as_os_str()), ("HUD_GIT_MARK", mark.as_os_str())]);
    assert_eq!(std::fs::read_to_string(&mark).unwrap(), "main"); // the child ran the assertion
}

/// Run only by `hud_git_sigchld_ignored_still_gives_the_branch`, in a child process; passes when run alone.
#[cfg(unix)]
#[test]
#[ignore = "run by hud_git_sigchld_ignored_still_gives_the_branch"]
fn hud_git_sigchld_child() {
    let (Some(w), Some(mark)) = (std::env::var_os("HUD_GIT_SIGCHLD_REPO"), std::env::var_os("HUD_GIT_MARK")) else {
        return;
    };
    // SAFETY: sets this process's SIGCHLD disposition; the child process runs this one test alone.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
    let w = PathBuf::from(w);
    let g = hud_git(&w, REAL).unwrap();
    assert_eq!((g.top, g.branch.as_deref()), (canon(&w), Some("main")));
    std::fs::write(mark, "main").unwrap();
}

#[test] // author's call 6 (C6): the largest budget does not panic
fn hud_git_max_budget_does_not_overflow() {
    let _f = forks();
    let (_t, w) = repo(true);
    assert_eq!(hud_git(&w, Duration::MAX).unwrap().branch.as_deref(), Some("main"));
}

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
        "{pre} rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir --symbolic-full-name HEAD{env}\n\
         {pre} rev-list --left-right --count HEAD...@{{upstream}} --{env}\n"));
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

#[cfg(unix)]
#[test] // AU6: rev-parse has 150 ms of the 400 ms
fn hud_git_rev_parse_has_150_ms() {
    let _f = forks();
    let t = TempDir::new().unwrap();
    let pid = t.path().join("pid");
    // Every process of this fake has the unique path `t` in its arguments: `/bin/sh <t>/git …`, then `<t>/m/sleep 5`.
    let stall = marked_sleep(t.path());
    let fake = fake_git(t.path(), &format!("echo $$ > '{}'; exec '{}' 5", pid.display(), stall.display()));
    let start = Instant::now();
    assert_eq!(hud_git_with(&fake, t.path(), HUD), None);
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(150) && took < HUD, "{took:?}");
    // On a slow runner the kill can come before sh writes its PID; the marker check below covers that case.
    if std::fs::read_to_string(&pid).is_ok_and(|s| !s.trim().is_empty()) {
        assert_gone(&pid);
    }
    assert_no_process_with(&t.path().display().to_string());
}

#[cfg(unix)]
#[test] // AU6: the 400 ms budget kills a stalled rev-list; call 1 still gives worktree and branch
fn hud_git_budget_kills_stalled_rev_list() {
    let _f = forks();
    let t = TempDir::new().unwrap();
    let (w, paths) = fake_tree(t.path());
    let pid = t.path().join("pid");
    let fake = fake_rev_parse(t.path(), &format!("{paths}refs/heads/main\n"), &format!("echo $$ > '{}'; exec sleep 5", pid.display()));
    let start = Instant::now();
    let g = hud_git_with(&fake, &w, HUD).unwrap();
    let took = start.elapsed();
    // The lower bound is the spec budget. The upper bound gives a slow runner 1 s past it for two process starts, the
    // 10 ms polls and the kill, so the test checks that the kill ends the call, not how fast the runner is.
    assert!(took >= HUD && took < HUD + Duration::from_secs(1), "{took:?}");
    assert_eq!((g.top, g.branch.as_deref(), g.ahead, g.behind), (canon(&w), Some("main"), None, None));
    assert_gone(&pid);
}

#[cfg(windows)]
#[test] // author's call 5: a stalled fake `git.cmd` is killed at the 150 ms rev-parse deadline; bound budget + 2 s
fn hud_git_windows_deadline_kills_stalled_fake() {
    let _f = forks();
    // The fake's descendant is `ping.exe` (cmd.exe → ping.exe). ping takes no free marker argument, so the test counts
    // PING.EXE processes before and after: `forks()` serializes this binary's tests and cargo runs test binaries one
    // at a time, so no other ping.exe of this suite starts between the counts. A count after that is not above the
    // count before shows that the tree kill also ended the fake's ping.exe.
    let pings = || {
        let o = Command::new("tasklist").args(["/FI", "IMAGENAME eq PING.EXE", "/NH", "/FO", "CSV"]).output().unwrap();
        assert!(o.status.success(), "tasklist failed");
        String::from_utf8_lossy(&o.stdout).lines().filter(|l| l.to_ascii_uppercase().starts_with("\"PING.EXE\"")).count()
    };
    let before = pings();
    let t = TempDir::new().unwrap();
    let fake = t.path().join("git.cmd");
    std::fs::write(&fake, "@ping -n 6 127.0.0.1 >NUL\r\n").unwrap();
    let start = Instant::now();
    assert_eq!(hud_git_with(&fake, t.path(), HUD), None);
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(150) && took < HUD + Duration::from_secs(2), "{took:?}");
    let until = Instant::now() + Duration::from_secs(2);
    while pings() > before && Instant::now() < until { std::thread::sleep(Duration::from_millis(50)); }
    assert!(pings() <= before, "the fake's ping.exe still runs");
}

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
    let fake = fake_both(t.path(), &format!("{paths}refs/heads/main\n"), &format!("1\t2{}\n", " ".repeat(1 << 20)));
    let g = hud_git_with(&fake, &w, REAL).unwrap();
    assert_eq!((g.branch.as_deref(), g.ahead, g.behind), (Some("main"), None, None));
}

#[cfg(unix)]
#[test] // H§7.5: exactly four lines; four valid lines and one more → none
fn hud_git_needs_exactly_four_lines() {
    let _f = forks();
    let t = TempDir::new().unwrap();
    let (w, paths) = fake_tree(t.path());
    let fake = fake_rev_parse(t.path(), &format!("{paths}refs/heads/main\nx\n"), "exit 1");
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
        let fake = fake_both(t.path(), &format!("{paths}refs/heads/main\n"), list);
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
    std::fs::write(&out, format!("{paths}refs/heads/main\n")).unwrap();
    let fake = fake_git(t.path(), &format!(
        "case \"$*\" in\n  *rev-parse*) cat '{}'; sleep 7 & echo $! > '{}' ;;\n  *) exit 1 ;;\nesac",
        out.display(), pid.display()));
    let start = Instant::now();
    let g = hud_git_with(&fake, &w, REAL).unwrap();
    assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
    assert_eq!(g.branch.as_deref(), Some("main"));
    assert_gone(&pid);
}
