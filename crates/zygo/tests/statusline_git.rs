//! HUD-9: the git worker of the status line HUD on the `zygo` binary (H§7.5, H§14 #25, #26; review focus 9).
//!
//! Each test runs `zygo hook statusline` as a child process with its own `HOME`, `GIT_CONFIG_GLOBAL` and `PATH`.
//! The tests run one at a time: a test writes programs and then runs them, and a fork of another test thread
//! could hold such a file open for writing.

mod common;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
#[cfg(unix)]
use std::time::{Duration, Instant};

use tempfile::TempDir;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;

const LOGO: &str = "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}";
const BRANCH: &str = "\u{e0a0}";
const TREE: &str = "\u{f0645}";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// The git budget of a run with the real git: a loaded machine can need more than 400 ms. The hook's 3 s bound
/// still holds.
const BUDGET: (&str, &str) = ("ZYGO_TEST_GIT_BUDGET_MS", "2400");

/// A home directory with a verbose session `S1` that has no proxy (the HUD shows `offline`).
struct Fx {
    home: TempDir,
    dir: PathBuf,
}

fn fx() -> Fx {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "").unwrap();
    std::fs::write(home.path().join(".gitconfig"), "").unwrap();
    let dir = home.path().join("sessions").join("S1");
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    SessionView { session: "S1".into(), statusline_style: "verbose".into(), ..Default::default() }.save(&sd).unwrap();
    Fx { home, dir }
}

impl Fx {
    /// `zygo hook statusline` in `cwd` with `stdin`: no color, Nerd icons, width 200, no controlling terminal.
    fn hud(&self, cwd: &Path, stdin: &str, vars: &[(&str, &OsStr)]) -> (i32, String, String) {
        let home = self.home.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, home);
        cmd.args(["hook", "statusline"])
            .current_dir(cwd)
            .env("ZYGO_SESSION_DIR", &self.dir)
            .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
            .env("NO_COLOR", "1")
            .env("ZYGO_NERD_FONT", "1")
            .env("COLUMNS", "200");
        for name in ["ZYGO_WORK", "ZYGO_URL", "ZYGO_AGENT", "ZYGO_TEST_NOW"] {
            cmd.env_remove(name);
        }
        cmd.envs(vars.iter().copied());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: `setsid` is async-signal-safe and touches no memory of this process; the child has no
            // controlling terminal after it.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        common::feed_stdin(child.stdin.take().unwrap(), stdin.as_bytes());
        let out = child.wait_with_output().unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    }

    fn budget(&self) -> [(&'static str, &'static OsStr); 1] {
        [(BUDGET.0, OsStr::new(BUDGET.1))]
    }
}

/// `git <args>` in `dir` with no user or system configuration; returns whether it exited 0.
fn git_try(dir: &Path, args: &[&OsStr]) -> bool {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    out.status.success()
}

fn git(dir: &Path, args: &[&str]) {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    assert!(git_try(dir, &args), "git {args:?} in {}", dir.display());
}

/// `<parent>/origin` on `main` with one commit, its clone `<parent>/c`, and the linked worktree `wt` of the clone
/// at `at`, on the branch `wt` that follows `origin/main`, one commit ahead.
fn linked_ahead(parent: &Path, at: &Path) {
    git(parent, &["init", "-q", "-b", "main", "origin"]);
    std::fs::write(parent.join("origin").join("f.txt"), "f\n").unwrap();
    git(&parent.join("origin"), &["add", "f.txt"]);
    git(&parent.join("origin"), &["commit", "-q", "-m", "a"]);
    git(parent, &["clone", "-q", "origin", "c"]);
    let args: Vec<&OsStr> = ["worktree", "add", "-q", "-b", "wt"].iter().map(OsStr::new).chain([at.as_os_str(), OsStr::new("origin/main")]).collect();
    assert!(git_try(&parent.join("c"), &args), "worktree add {}", at.display());
    git(at, &["commit", "-q", "--allow-empty", "-m", "b"]);
}

/// An executable `#!/bin/sh` program.
#[cfg(unix)]
fn program(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// H§14 #25 and review focus 9: a repository that configures programs (`core.fsmonitor`, `core.hooksPath` with
/// `post-index-change`, `reference-transaction` and `post-checkout`, `filter.m.clean` named in `.gitattributes`,
/// `.git/info/attributes` and a global `core.attributesFile`, and `remote.origin.uploadpack` of a partial clone),
/// each writing a marker file, with every file stat-dirty. After a render no marker exists, and the worktree, the
/// branch and the ahead mark show. A control run of plain git commands proves that the programs do write markers.
#[cfg(unix)]
#[test]
#[ignore = "timing: real git must end inside the 3 s hook bound"]
fn h14_25_configured_programs_leave_no_marker_after_a_render() {
    let _serial = serial();
    let fx = fx();
    let root = fx.home.path().join("r");
    let (bin, hooks, markers) = (root.join("bin"), root.join("hooks"), root.join("markers"));
    for d in [&bin, &hooks, &markers] {
        std::fs::create_dir_all(d).unwrap();
    }
    let marker = |name: &str| markers.join(name);

    // A partial clone with a linked worktree, one commit ahead of its upstream.
    git(&root, &["init", "-q", "-b", "main", "origin"]);
    let origin = root.join("origin");
    std::fs::write(origin.join("f.txt"), "f\n").unwrap();
    std::fs::write(origin.join(".gitattributes"), "* filter=m\n").unwrap();
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-q", "-m", "a"]);
    git(&origin, &["config", "uploadpack.allowFilter", "true"]);
    let url = format!("file://{}", origin.display());
    git(&root, &["clone", "-q", "--filter=blob:none", &url, "c"]);
    let clone = root.join("c");
    git(&clone, &["worktree", "add", "-q", "-b", "wt", "../wt", "origin/main"]);
    let wt = root.join("wt");
    git(&wt, &["commit", "-q", "--allow-empty", "-m", "b"]);

    // The programs, configured after the setup so that the setup writes no marker.
    program(&bin.join("fsmonitor"), &format!(": > '{}'", marker("fsmonitor").display()));
    program(&bin.join("clean"), &format!(": > '{}'\ncat", marker("clean").display()));
    program(&bin.join("uploadpack"), &format!(": > '{}'\nexit 1", marker("uploadpack").display()));
    for hook in ["post-index-change", "reference-transaction", "post-checkout"] {
        program(&hooks.join(hook), &format!(": > '{}'", marker(hook).display()));
    }
    std::fs::write(clone.join(".git").join("info").join("attributes"), "* filter=m\n").unwrap();
    std::fs::write(fx.home.path().join("gattrs"), "* filter=m\n").unwrap();
    std::fs::write(
        fx.home.path().join(".gitconfig"),
        format!("[core]\n\tattributesFile = {}\n", fx.home.path().join("gattrs").display()),
    )
    .unwrap();
    for (key, value) in [
        ("core.fsmonitor", bin.join("fsmonitor")),
        ("core.hooksPath", hooks.clone()),
        ("filter.m.clean", bin.join("clean")),
        ("remote.origin.uploadpack", bin.join("uploadpack")),
    ] {
        git(&clone, &["config", key, value.to_str().unwrap()]);
    }
    let dirty = || {
        for name in ["f.txt", ".gitattributes"] {
            let path = wt.join(name);
            let text = std::fs::read(&path).unwrap();
            std::fs::write(&path, text).unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(std::time::SystemTime::now() - Duration::from_secs(1000)).unwrap();
        }
    };
    let found = || {
        let mut names: Vec<String> =
            std::fs::read_dir(&markers).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };

    // Control: plain git commands in this repository run the programs.
    dirty();
    let user_git = |args: &[&str]| {
        let _ = Command::new("git")
            .current_dir(&wt)
            .args(args)
            .env("HOME", fx.home.path())
            .env("GIT_CONFIG_GLOBAL", fx.home.path().join(".gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
    };
    user_git(&["status"]);
    user_git(&["checkout", "-q", "-b", "control"]);
    user_git(&["checkout", "-q", "wt"]);
    user_git(&["fetch", "-q"]);
    let control = found();
    for name in ["clean", "fsmonitor", "post-checkout", "reference-transaction", "uploadpack"] {
        assert!(control.iter().any(|n| n == name), "the control run wrote no `{name}` marker: {control:?}");
    }
    for name in &control {
        std::fs::remove_file(marker(name)).unwrap();
    }
    dirty();
    assert!(found().is_empty());

    // The render.
    let stdin = serde_json::json!({ "cwd": wt }).to_string();
    let (code, out, err) = fx.hud(&wt, &stdin, &fx.budget());
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(out, format!("{LOGO}  {BRANCH} wt {TREE} wt ↑1\nz\u{28e}\u{183}o  offline"));
    assert_eq!(found(), Vec::<String>::new(), "a HUD git call ran a configured program");
    // A second render in another cwd of the repository (the main worktree) runs git again: still no marker.
    let stdin = serde_json::json!({ "cwd": clone }).to_string();
    let (_, out, _) = fx.hud(&clone, &stdin, &fx.budget());
    assert_eq!(out, format!("{LOGO}  {BRANCH} main\nz\u{28e}\u{183}o  offline"));
    assert_eq!(found(), Vec::<String>::new(), "a HUD git call ran a configured program");
}

/// H§14 #25: a `git rev-list` that does not end. The worktree and the branch still show (from `rev-parse`), with no
/// marks, within the bound; the next call within 30 s does not run git.
#[cfg(unix)]
#[test]
#[ignore = "timing: asserts elapsed time against the 3 s hook bound"]
fn h14_25_slow_rev_list_keeps_the_worktree_and_the_branch() {
    let _serial = serial();
    let fx = fx();
    let root = fx.home.path().join("r");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let wt = root.join("wt");
    linked_ahead(&root, &wt);
    let real = String::from_utf8(Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    let log = root.join("calls");
    program(
        &root.join("bin").join("git"),
        &format!("echo \"$*\" >> '{}'\ncase \"$*\" in\n  *rev-list*) sleep 30 ;;\n  *) exec '{}' \"$@\" ;;\nesac", log.display(), real.trim()),
    );
    let path = std::env::join_paths(
        std::iter::once(root.join("bin")).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let vars = [("PATH", path.as_os_str()), (BUDGET.0, OsStr::new("2000"))];
    let stdin = serde_json::json!({ "cwd": wt }).to_string();
    let calls = || std::fs::read_to_string(&log).unwrap().lines().count();

    let start = Instant::now();
    let (code, out, err) = fx.hud(&wt, &stdin, &vars);
    let took = start.elapsed();
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("{LOGO}  {BRANCH} wt {TREE} wt\nz\u{28e}\u{183}o  offline"));
    assert!(took < Duration::from_millis(3700), "took {took:?}");
    assert_eq!(calls(), 2, "{}", std::fs::read_to_string(&log).unwrap());

    let start = Instant::now();
    let (_, again, _) = fx.hud(&wt, &stdin, &vars);
    assert_eq!(again, out);
    assert_eq!(calls(), 2, "the git part was not due, and git ran");
    assert!(start.elapsed() < Duration::from_millis(1900), "the second call waited: {:?}", start.elapsed());
}

/// H§14 #24 and #25 (C6): a git worker that panics gives a HUD with no git segment, one stderr line and exit 0.
#[test]
#[ignore = "timing: the 3 s statusline hook bound in the zygo binary"]
fn h14_25_git_worker_panic_writes_one_stderr_line() {
    let _serial = serial();
    let fx = fx();
    let root = fx.home.path().join("r");
    std::fs::create_dir_all(&root).unwrap();
    let wt = root.join("wt");
    linked_ahead(&root, &wt);
    let stdin = serde_json::json!({ "cwd": wt }).to_string();
    // Both runs at one `ZYGO_TEST_NOW`: the 30 s rule does not depend on the wall clock.
    let now = ("ZYGO_TEST_NOW", OsStr::new("1800000000"));
    let vars = [("ZYGO_TEST_GIT_PANIC", OsStr::new("1")), (BUDGET.0, OsStr::new(BUDGET.1)), now];
    let (code, out, err) = fx.hud(&wt, &stdin, &vars);
    assert_eq!((code, out, err), (0, format!("{LOGO}\nz\u{28e}\u{183}o  offline"), "zygo hook: statusline git failed\n".to_string()));
    // The failed try is not due for 30 s: the next run has no git segment and no error line.
    let (code, out, err) = fx.hud(&wt, &stdin, &[(BUDGET.0, OsStr::new(BUDGET.1)), now]);
    assert_eq!((code, out, err), (0, format!("{LOGO}\nz\u{28e}\u{183}o  offline"), String::new()));
}

/// The git worker in the hook's own cwd when stdin names none (H§7.1), and its result without the seam.
#[test]
#[ignore = "timing: real git must end inside the 3 s hook bound"]
fn git_segments_from_the_hooks_own_cwd() {
    let _serial = serial();
    let fx = fx();
    let root = fx.home.path().join("r");
    std::fs::create_dir_all(&root).unwrap();
    let wt = root.join("wt");
    linked_ahead(&root, &wt);
    let (code, out, err) = fx.hud(&wt, "{}", &fx.budget());
    assert_eq!((code, out, err), (0, format!("{LOGO}  {BRANCH} wt {TREE} wt ↑1\nz\u{28e}\u{183}o  offline"), String::new()));
}

/// H§14 #26: a worktree directory whose name holds invalid UTF-8 shows with U+FFFD. Only Linux file systems hold
/// such a name.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "timing: real git must end inside the 3 s hook bound"]
fn h14_26_worktree_name_with_invalid_utf8_shows_the_replacement_character() {
    use std::os::unix::ffi::OsStrExt;
    let _serial = serial();
    let fx = fx();
    let root = fx.home.path().join("r");
    std::fs::create_dir_all(&root).unwrap();
    let wt = root.join(OsStr::from_bytes(b"wt\xff"));
    linked_ahead(&root, &wt);
    let (code, out, err) = fx.hud(&wt, "{}", &fx.budget());
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("{LOGO}  {BRANCH} wt {TREE} wt\u{fffd} ↑1\nz\u{28e}\u{183}o  offline"));
}

/// H§14 #26: a directory name that holds a newline gives no worktree segment (and no branch); a renamed file
/// changes nothing, because the HUD has no dirty count (OP14). Windows refuses the newline name.
#[cfg(unix)]
#[test]
#[ignore = "timing: real git must end inside the 3 s hook bound"]
fn h14_26_newline_in_the_directory_name_gives_no_worktree_segment() {
    let _serial = serial();
    let fx = fx();
    let root = fx.home.path().join("r");
    std::fs::create_dir_all(&root).unwrap();
    let wt = root.join("a\nb");
    linked_ahead(&root, &wt);
    let (code, out, err) = fx.hud(&wt, "{}", &fx.budget());
    assert_eq!((code, out, err), (0, format!("{LOGO}\nz\u{28e}\u{183}o  offline"), String::new()));

    let fx = self::fx();
    let root = fx.home.path().join("r");
    std::fs::create_dir_all(&root).unwrap();
    let wt = root.join("wt");
    linked_ahead(&root, &wt);
    git(&wt, &["mv", "f.txt", "g.txt"]);
    let (_, out, _) = fx.hud(&wt, "{}", &fx.budget());
    assert_eq!(out, format!("{LOGO}  {BRANCH} wt {TREE} wt ↑1\nz\u{28e}\u{183}o  offline"));
    assert!(!out.contains('●'));
}
