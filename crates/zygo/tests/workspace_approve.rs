//! WD-15: `zygo workspace approve [--yes] [--digest <hex>]` (workspace definition design R§7.1, R§7.2, R§9.5
//! "Shown to the operator", R§10, R§11; R§18 items 4, 5, 7, 15 and 24). Not `cfg(unix)`: the subverb starts nothing,
//! needs no tmux and the OP5 check is not applied to it. No network: the user config turns the built-in marketplace off
//! (`common::NO_MARKETPLACE`), and each run asserts that no marketplace sync ran (`common::assert_no_marketplace_sync`,
//! FX-23). No agent starts, so no stub agent. Every test that starts a process takes `serial()`, so no test writes a
//! script while another test forks (ETXTBSY; FX-25 review F3).
mod common;

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use zygo::cmd::verbs::VerbCtx;
use zygo::cmd::workspace::listing;
use zygo_config::repo_def::{check_default, SystemGit, Use};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_work::repo::checkout;
use zygo_workspace::identity::workspace_id;

const WS: &str = ".zygo/workspace.toml";
const MIN: &str = "version = 1\n";
const NEXT: &str = "to start it: zygo up (a workspace that exists already: zygo down, then zygo up --reset)";
const REFUSED: &str =
    "zygo: approval is not available inside an agent session; run it in your own shell: zygo workspace approve";
const SLASH: &str = "run in your shell: zygo workspace approve";
const USAGE: &str = "zygo workspace: usage: workspace approve [--yes] [--digest <hex>]";

static LIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

/// The file that turns user and system git config off.
fn no_config() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

/// `git -C <dir> <args>` with no user or system config, no template, no signing, a fixed identity,
/// `core.autocrlf=false` and `core.protectNTFS=false`; no `GIT_DIR`-like variable of the caller.
fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", no_config())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TEMPLATE_DIR", "")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.protectNTFS=false",
            "-C",
        ])
        .arg(dir)
        .args(args);
    c
}

/// Runs it, asserts success, returns stdout trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd(dir, args).stdin(Stdio::null()).output().expect("git on PATH");
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// `hash-object -w --stdin` of `bytes` in `dir`; returns the blob id.
fn blob(dir: &Path, bytes: &[u8]) -> String {
    let mut child = git_cmd(dir, &["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git on PATH");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "hash-object: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// Adds each regular file to the index of `dir` (nothing on disk), then `commit -q -m c`.
fn commit_files(dir: &Path, ents: &[(&str, &[u8])]) {
    for (path, bytes) in ents {
        let oid = blob(dir, bytes);
        git(dir, &["update-index", "--add", "--cacheinfo", &format!("100644,{oid},{path}")]);
    }
    git(dir, &["commit", "-q", "-m", "c"]);
}

struct Fx {
    t: tempfile::TempDir,
    c: PathBuf,
}

/// `o.git` (bare), `w` with a `README` seed commit, then `ents` in a second commit when not empty, pushed; `c` cloned
/// `--no-checkout`.
fn fx(ents: &[(&str, &[u8])]) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let (o, w) = (t.path().join("o.git"), t.path().join("w"));
    git(t.path(), &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
    git(t.path(), &["init", "-q", "-b", "main", w.to_str().unwrap()]);
    commit_files(&w, &[("README", b"seed")]);
    if !ents.is_empty() {
        commit_files(&w, ents);
    }
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(t.path(), &["clone", "-q", "--no-checkout", "o.git", "c"]);
    let c = t.path().join("c");
    Fx { t, c }
}

/// `f.t/home` with `config.toml` = `user_toml`, a newline, then [`common::NO_MARKETPLACE`] (its `[[marketplace]]`
/// header closes any open table, so it goes after the other keys).
fn home(f: &Fx, user_toml: &str) -> PathBuf {
    let h = f.t.path().join("home");
    std::fs::create_dir_all(&h).unwrap();
    std::fs::write(h.join("config.toml"), format!("{user_toml}\n{}", common::NO_MARKETPLACE)).unwrap();
    h
}

/// `Paths.approvals_file` of `Paths::from_env` under `common::scratch_home`.
fn approvals(home: &Path) -> PathBuf {
    if cfg!(windows) { home.join("zygo").join("approvals.toml") } else { home.join(".local/share/zygo/approvals.toml") }
}

fn id(c: &Path) -> String {
    workspace_id(&checkout(c, Instant::now() + Duration::from_secs(60)).unwrap().common_dir)
}

fn head(c: &Path) -> String {
    git(c, &["rev-parse", "refs/remotes/origin/main"])
}

/// The digest of the files on the default branch, not stored.
fn digest(f: &Fx) -> String {
    match check_default(&f.c, &id(&f.c), &f.t.path().join("none.toml"), &SystemGit) {
        Use::NotApproved { digest, .. } => digest,
        u => panic!("not NotApproved: {u:?}"),
    }
}

fn cmd(home: &Path, dir: &Path, args: &[&str], extra: &[(&str, &OsStr)]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_zygo"));
    c.args(["workspace", "approve"]).args(args).current_dir(dir);
    common::scratch_home(&mut c, home);
    c.env_remove("ZYGO_SESSION_DIR")
        .env_remove("ZYGO_SESSION")
        .env_remove("ZYGO_WORKSPACE")
        .env_remove("TMUX")
        .env("GIT_CONFIG_GLOBAL", no_config())
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for (k, v) in extra {
        c.env(k, v);
    }
    c
}

fn run_in(home: &Path, dir: &Path, args: &[&str], stdin: Option<&[u8]>, extra: &[(&str, &OsStr)]) -> Output {
    let mut c = cmd(home, dir, args, extra);
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = c.spawn().expect("zygo binary");
    if let Some(b) = stdin {
        common::feed_stdin(child.stdin.take().unwrap(), b);
    }
    let o = child.wait_with_output().unwrap();
    common::assert_no_marketplace_sync(home);
    o
}

fn approve(f: &Fx, home: &Path, args: &[&str], stdin: Option<&[u8]>, extra: &[(&str, &OsStr)]) -> Output {
    run_in(home, &f.c, args, stdin, extra)
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

/// The `workspace` array of `file` as `(id, digest)` pairs; empty when the file is missing.
fn entries(file: &Path) -> Vec<(String, String)> {
    let Ok(s) = std::fs::read_to_string(file) else { return vec![] };
    let t: toml::Table = toml::from_str(&s).unwrap();
    t.get("workspace")
        .and_then(|w| w.as_array())
        .map(|a| {
            a.iter()
                .map(|e| {
                    (e["id"].as_str().unwrap().to_string(), e["digest"].as_str().unwrap().to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strings(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn min() -> Fx {
    fx(&[(WS, MIN.as_bytes())])
}

#[test] // R§18 item 5: the answer yes stores one entry with the id and the digest
fn answer_yes_stores_one_entry() {
    let _g = serial();
    for answer in ["y\n", "Y\n", "yes\n", "YES\n"] {
        let f = min();
        let h = home(&f, "");
        let d = digest(&f);
        let o = approve(&f, &h, &[], Some(answer.as_bytes()), &[]);
        let (out, err) = (text(&o.stdout), text(&o.stderr));
        assert_eq!(o.status.code(), Some(0), "{answer:?}: {err}");
        assert_eq!(out, format!("approved {d}\n{NEXT}\n"));
        assert!(err.contains(&format!("\ndigest: {d}\n")) && err.ends_with("approve these workspace files? [y/N] "), "{err}");
        assert_eq!(entries(&approvals(&h)), vec![(id(&f.c), d.clone())]);
    }
}

#[test] // R§7.1: an answer other than yes, and the end of input, store nothing
fn answer_no_or_end_of_input_stores_nothing() {
    let _g = serial();
    let inputs: [Option<&[u8]>; 3] = [Some(b"n\n"), Some(b""), None];
    for stdin in inputs {
        let f = min();
        let h = home(&f, "");
        let o = approve(&f, &h, &[], stdin, &[]);
        let out = text(&o.stdout);
        assert_eq!(
            (o.status.code(), out.as_str()),
            (Some(0), "not approved; run zygo workspace approve\n"),
            "{stdin:?}: {}",
            text(&o.stderr)
        );
        assert!(!approvals(&h).exists());
    }
}

#[test] // R§7.1 step 6: with --yes the listing goes to stdout with the full ref and the full commit id
fn yes_prints_the_listing_on_stdout() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let d = digest(&f);
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(out.starts_with("folder: "), "{out}");
    let tail = format!(
        "ref: refs/remotes/origin/main\ncommit: {}\ndigest: {d}\n.zygo/workspace.toml (12 bytes):\n    version = 1\nignored: none\nsummary:\n  agents: 5\n  member orchestrator: kind claude-code, job orchestrator, profile -, pool true, cwd -\n  member worker-1: kind claude-code, job worker, profile -, pool true, cwd -\n  member worker-2: kind claude-code, job worker, profile -, pool true, cwd -\n  member worker-3: kind claude-code, job worker, profile -, pool true, cwd -\n  member worker-4: kind claude-code, job worker, profile -, pool true, cwd -\napproved {d}\n{NEXT}\n",
        head(&f.c)
    );
    assert!(out.ends_with(&tail), "{out}");
    assert_eq!(out.lines().count(), tail.lines().count() + 1);
    assert!(!err.contains("[y/N]"), "{err}");
    assert_eq!(entries(&approvals(&h)), vec![(id(&f.c), d.clone())]);
}

#[test] // R§7.1 step 4: --digest of another value approves nothing; a malformed value is a usage error
fn digest_of_another_value_stores_nothing() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let d = digest(&f);
    let other = "a".repeat(64);
    let o = approve(&f, &h, &["--yes", "--digest", &other], None, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        text(&o.stderr),
        format!("zygo: the workspace files changed: their digest is {d}, not {other}; nothing approved\n")
    );
    assert!(!approvals(&h).exists());

    let o = approve(&f, &h, &["--yes", "--digest", "abc"], None, &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str()),
        (Some(2), "zygo workspace: --digest takes 64 hex characters\n")
    );
    assert!(!approvals(&h).exists());

    let o = approve(&f, &h, &["--yes", "--digest", &d.to_ascii_uppercase()], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(entries(&approvals(&h)), vec![(id(&f.c), d.clone())]);
}

#[test] // plan call 3: an unknown token, a repeat, a missing value and --save
fn usage_errors_exit_2() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let cases: [&[&str]; 4] = [&["--bogus"], &["--yes", "--yes"], &["--digest"], &["--save"]];
    for args in cases {
        let o = approve(&f, &h, args, None, &[]);
        let err = text(&o.stderr);
        assert_eq!((o.status.code(), err.as_str()), (Some(2), &*format!("{USAGE}\n")), "{args:?}");
        assert!(!approvals(&h).exists());
    }
}

#[test] // R§7.1 step 2
fn inside_an_agent_session_stores_nothing() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let s = f.t.path().join("s");
    let o = approve(&f, &h, &["--yes"], None, &[("ZYGO_SESSION_DIR", s.as_os_str())]);
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    assert_eq!((o.status.code(), err.as_str(), out.as_str()), (Some(1), &*format!("{REFUSED}\n"), ""));
    assert!(!approvals(&h).exists());
}

#[test] // R§7.1 steps 1 and 2; R§18 item 24: no OP5, so this runs on Windows too
fn slash_form_returns_the_command_and_approves_nothing() {
    let _g = serial();
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let paths = Paths {
        config_file: r.join("config.toml"),
        cache_dir: r.join("cache"),
        data_dir: r.join("data"),
        sessions_dir: r.join("cache/sessions"),
        marketplaces_dir: r.join("cache/marketplaces"),
        plugin_store: r.join("data/plugins"),
        approvals_file: r.join("data/approvals.toml"),
        usage_db: r.join("data/usage.db"),
    };
    let ctx = VerbCtx { slash: true, save: false, session_dir: Some(SessionDir::new(r.join("s"))), paths };
    let verb = zygo::cmd::verbs::VERBS.iter().find(|v| v.spec.name == "workspace").unwrap();
    for args in [vec!["approve"], vec!["approve", "--yes"], vec!["approve", "--bogus"]] {
        assert_eq!((verb.handler)(&ctx, &strings(&args)).unwrap(), SLASH, "{args:?}");
    }
    assert_eq!(zygo::cmd::workspace::approve(&ctx, &[], &mut std::io::empty(), &mut std::io::sink()).unwrap(), SLASH);
    assert!(!ctx.paths.approvals_file.exists());
}

#[test] // R§7.1 step 5: a second approve stores nothing more
fn second_approve_keeps_the_file() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let d = digest(&f);
    let o = approve(&f, &h, &["--yes"], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let before = std::fs::read(approvals(&h)).unwrap();
    let o2 = approve(&f, &h, &[], None, &[]);
    let out2 = text(&o2.stdout);
    assert_eq!((o2.status.code(), out2.as_str()), (Some(0), &*format!("workspace files already approved ({d})\n")));
    assert_eq!(std::fs::read(approvals(&h)).unwrap(), before);
}

#[test] // R§10 "Two sessions at once"
fn two_approvals_store_one_entry() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let d = digest(&f);
    let spawn = || {
        cmd(&h, &f.c, &["--yes"], &[])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("zygo binary")
    };
    let (c1, c2) = (spawn(), spawn());
    let (o1, o2) = (c1.wait_with_output().unwrap(), c2.wait_with_output().unwrap());
    common::assert_no_marketplace_sync(&h);
    let (out1, out2) = (text(&o1.stdout), text(&o2.stdout));
    for out in [&out1, &out2] {
        assert!(
            out.ends_with(&format!("approved {d}\n{NEXT}\n"))
                || out.ends_with(&format!("workspace files already approved ({d})\n")),
            "{out}"
        );
    }
    assert_eq!((o1.status.code(), o2.status.code()), (Some(0), Some(0)));
    assert_eq!(entries(&approvals(&h)), vec![(id(&f.c), d.clone())]);
}

#[test] // R§7.1 step 3: absent
fn absent_exits_1() {
    let _g = serial();
    let f = fx(&[]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str()),
        (Some(1), "zygo: this repository has no workspace files at refs/remotes/origin/main\n")
    );
    assert!(!approvals(&h).exists());
}

#[cfg(unix)]
#[test] // FX-25 f6, R§7.1 step 3: a failed git worktree list keeps its reason, exit 1
fn worktree_list_failure_exits_1_with_the_reason() {
    let _g = serial();
    use std::os::unix::fs::PermissionsExt;
    let f = fx(&[(WS, MIN.as_bytes())]);
    let h = home(&f, "");
    let old = std::env::var_os("PATH").unwrap_or_default();
    let real = std::env::split_paths(&old).map(|d| d.join("git")).find(|g| g.is_file()).expect("git on PATH");
    let bin = f.t.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("git");
    let body = format!(
        "#!/bin/sh\ncase \" $* \" in *\" worktree list \"*) echo 'fatal: stub' >&2; exit 1;; esac\nexec '{}' \"$@\"\n",
        real.display()
    );
    std::fs::write(&stub, body).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    // `PATH` goes to the child only; the test process keeps its own.
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&old))).unwrap();
    let o = approve(&f, &h, &["--yes"], None, &[("PATH", path.as_os_str())]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str(), text(&o.stdout).as_str()),
        (Some(1), "zygo: workspace approve: git worktree list failed: fatal: stub\n", "")
    );
    assert!(!approvals(&h).exists());
}

#[test] // R§7.1 step 3: not a repository
fn outside_a_repository_exits_2() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let plain = f.t.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let o = run_in(&h, &plain, &["--yes"], None, &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str()),
        (Some(2), "zygo: workspace approve needs a git repository\n")
    );
    assert!(!approvals(&h).exists());
}

#[test] // R§7.1 step 3 (WD-15 review F5): a lookup error other than "not a repository" keeps its reason, exit 1
fn bare_repository_without_checkout_exits_1_with_the_reason() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let o = run_in(&h, &f.t.path().join("o.git"), &["--yes"], None, &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str(), text(&o.stdout).as_str()),
        (Some(1), "zygo: workspace approve: zygo up needs a worktree; this repository is bare\n", "")
    );
    assert!(!approvals(&h).exists());
}

#[test] // R§7.1 step 3, R§18 item 4 (`approve` exits 1)
fn invalid_exits_1_with_the_violation() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n[members.a]\nargs = [\"x\"]\n")]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str()),
        (Some(1), "zygo: this repository's workspace files (refs/remotes/origin/main) are invalid (.zygo/workspace.toml: members.a.args: not allowed in a repository file); nothing approved\n")
    );
    assert!(!approvals(&h).exists());
}

#[test] // R§7.1 step 3: unread
fn unread_exits_1_with_the_reason() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    git(&f.c, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/heads/x"]);
    let o = approve(&f, &h, &["--yes"], None, &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr).as_str()),
        (Some(1), "zygo: this repository's workspace files are unread (origin/HEAD points outside refs/remotes/origin/); nothing approved\n")
    );
    assert!(!approvals(&h).exists());
}

#[test] // R§18 item 7 as rewritten 2026-11-26; R-C7; review focus 36
fn listing_escapes_terminal_and_bidi_characters() {
    let _g = serial();
    let f = fx(&[
        (WS, "version = 1\r\n# caf\u{e9} \u{202e}evil\r\n".as_bytes()),
        (".zygo/a\u{1b}[2Jb\rc", b"x"),
    ]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    for raw in ['\u{1b}', '\r', '\u{202e}', '\u{e9}'] {
        assert!(!out.contains(raw), "{raw:?} in {out:?}");
    }
    assert!(out.contains("\n    version = 1\\u{d}\n    # caf\\u{e9} \\u{202e}evil\\u{d}\n"), "{out}");
    assert!(out.contains("\nignored: \".zygo/a\\u{1b}[2Jb\\u{d}c\"\n"), "{out}");

    // A comment with ESC is invalid, and the violation shows no ESC.
    let f = fx(&[(WS, "version = 1\n# a\u{1b}[2Jb\n".as_bytes())]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(!err.contains('\u{1b}'), "{err:?}");
    assert!(err.starts_with("zygo: this repository's workspace files (refs/remotes/origin/main) are invalid (.zygo/workspace.toml: TOML parse error at line 2"), "{err}");
    assert!(err.contains("invalid comment character") && err.ends_with("; nothing approved\n"), "{err}");
    assert!(!approvals(&h).exists());

    // A member name with U+202E is shown escaped in the violation.
    let f = fx(&[(WS, "version = 1\n[members.\"a\u{202e}b\"]\n".as_bytes())]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(!err.contains('\u{202e}'), "{err:?}");
    assert!(err.contains(".zygo/workspace.toml: members.a\\u{202e}b: not a valid member name"), "{err}");
    assert!(!approvals(&h).exists());
}

#[test] // R§7.1 step 6: the character rule of the listing
fn listing_keeps_printable_ascii_and_escapes_the_rest() {
    assert_eq!(listing(b"a\tb\n"), "a\\tb");
    assert_eq!(listing(b"x\r\ny\n"), "x\\u{d}\ny");
    assert_eq!(
        listing("caf\u{e9}\u{202e}\u{1b}[2J\u{7f}\u{2028}".as_bytes()),
        "caf\\u{e9}\\u{202e}\\u{1b}[2J\\u{7f}\\u{2028}"
    );
    assert_eq!(listing(b"a\n\nb"), "a\n\nb");
    assert_eq!(listing(b""), "");
    assert_eq!(listing(b"\xff"), "\\u{fffd}");
}

const SUMMARY_WS: &str = "version = 1\n[members.orchestrator]\njob = \"orchestrator\"\n[members.worker-1]\nprofile = \"rust\"\npool = true\n[members.worker-2]\nkind = \"opencode\"\n";
const RUST: &str = "overrides = { review = \"gpt-6-sol\" }\ndisabled = [\"design\"]\n";

/// `f.t/pool` with the folders `a` and `b`.
fn pool(f: &Fx) -> PathBuf {
    let p = f.t.path().join("pool");
    for d in ["a", "b"] {
        std::fs::create_dir_all(p.join(d)).unwrap();
    }
    p
}

/// A TOML literal string, so a Windows path needs no escape.
fn pool_toml(p: &Path) -> String {
    format!("[workspace]\npool = '{}'\n", p.display())
}

#[test] // R§18 item 15 (summary part), R§9.5 "Shown to the operator"
fn summary_names_passthrough_members_and_the_profile_that_does_not_apply() {
    let _g = serial();
    let ents: [(&str, &[u8]); 2] = [(WS, SUMMARY_WS.as_bytes()), (".zygo/profiles/rust.toml", RUST.as_bytes())];
    let f = fx(&ents);
    let p = pool(&f);
    let h = home(&f, &pool_toml(&p));
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    for line in [
        "  agents: 3",
        "  member orchestrator: kind claude-code, job orchestrator, profile -, pool false, cwd -",
        "  member worker-1: kind claude-code, job worker, profile rust, pool true, cwd -",
        "  member worker-2: kind opencode, job worker, profile -, pool false, cwd -",
        r#"  profile rust: overrides review = "gpt-6-sol"; disabled design"#,
    ] {
        assert!(out.lines().any(|l| l == line), "{line:?} not in {out}");
    }
    let shown = zygo_core::esc(&p.display().to_string());
    let pt = format!(
        "  worker-1 run in passthrough on pool {shown}; profile rust of worker-1 does not apply in passthrough"
    );
    assert!(out.lines().any(|l| l == pt), "{out}");

    // The same files without a pool: no passthrough line.
    let f2 = fx(&ents);
    let h2 = home(&f2, "");
    let o = approve(&f2, &h2, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!out.contains("run in passthrough"), "{out}");

    // The default member set with a pool: every member runs in passthrough.
    let f3 = min();
    let p3 = pool(&f3);
    let h3 = home(&f3, &pool_toml(&p3));
    let o = approve(&f3, &h3, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let pt = format!(
        "  orchestrator, worker-1, worker-2, worker-3, worker-4 run in passthrough on pool {}",
        zygo_core::esc(&p3.display().to_string())
    );
    assert!(out.lines().any(|l| l == pt), "{out}");
}

#[test] // R§9.5 "Local mode only" (WD-15 review F6): with `[server] url` the summary claims no passthrough, as `zygo up` refuses the pool
fn summary_with_a_server_url_names_the_refusal_of_zygo_up() {
    let _g = serial();
    let f = min();
    let p = pool(&f);
    let h = home(&f, &format!("{}[server]\nurl = \"http://127.0.0.1:9/\"\n", pool_toml(&p)));
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!out.contains("run in passthrough"), "{out}");
    let refused = format!(
        "  pool {}: passthrough is local mode only, and [server] url is set; zygo up starts nothing",
        zygo_core::esc(&p.display().to_string())
    );
    assert!(out.lines().any(|l| l == refused), "{refused:?} not in {out}");
}

const POOL_WS: &str = "version = 1\n[members.w]\npool = true\nprofile = \"p\"\n";
const POOL_P: &str = "disabled = [\"design\"]\n";

#[test] // R§9.5 (WD-15 review round 2, finding 1): a pool `zygo up` cannot expand: the summary names its refusal, no passthrough
fn summary_with_an_unexpandable_pool_names_the_refusal_of_zygo_up() {
    let _g = serial();
    let f = fx(&[(WS, POOL_WS.as_bytes()), (".zygo/profiles/p.toml", POOL_P.as_bytes())]);
    let h = home(&f, "[workspace]\npool = '~nouser/x'\n");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!out.contains("run in passthrough"), "{out}");
    let refused = "  pool ~nouser/x: must be absolute after ~ expansion; zygo up starts nothing";
    assert!(out.lines().any(|l| l == refused), "{refused:?} not in {out}");
}

#[test] // R§10 (WD-15 review round 2, finding 2): a user `[loadout.repository]` replaces the repository loadout in the summary
fn summary_with_a_user_repository_loadout_counts_what_zygo_up_starts() {
    let _g = serial();
    let f = fx(&[(WS, POOL_WS.as_bytes()), (".zygo/profiles/p.toml", POOL_P.as_bytes())]);
    let p = pool(&f);
    let user = format!(
        "{}[loadout.repository]\n[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"shell\"\n",
        pool_toml(&p)
    );
    let h = home(&f, &user);
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    for line in [
        "  repository loadout not used: the user config has [loadout.repository]",
        "  agents: 0",
        "  profile p: overrides -; disabled design",
    ] {
        assert!(out.lines().any(|l| l == line), "{line:?} not in {out}");
    }
    assert!(!out.lines().any(|l| l == "  agents: 1"), "{out}");
    assert!(!out.contains("  member w:"), "{out}");
    assert!(!out.contains("run in passthrough"), "{out}");

    // A user `[loadout.repository]` with a pool member: the passthrough line names the user pane.
    let f = fx(&[(WS, POOL_WS.as_bytes()), (".zygo/profiles/p.toml", POOL_P.as_bytes())]);
    let p = pool(&f);
    let user = format!(
        "{}[loadout.repository]\n[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"shell\"\n[[loadout.repository.window.pane]]\nkind = \"claude-code\"\nname = \"u\"\npool = true\n",
        pool_toml(&p)
    );
    let h = home(&f, &user);
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let pt = format!("  u run in passthrough on pool {}", zygo_core::esc(&p.display().to_string()));
    for line in ["  agents: 1", pt.as_str()] {
        assert!(out.lines().any(|l| l == line), "{line:?} not in {out}");
    }
    assert!(!out.contains("  member w:") && !out.contains("w run in passthrough"), "{out}");
}

#[test] // R§7.1 step 6 (WD-15 review round 2, finding 3): each ignored entry is quoted, so a `, ` in a name cannot split it
fn ignored_entries_are_quoted() {
    let _g = serial();
    let f = fx(&[(WS, MIN.as_bytes()), (".zygo/a, .zygo/b", b"x"), (".zygo/profiles/c (1 bytes):", b"x")]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let want = r#"ignored: ".zygo/a, .zygo", ".zygo/profiles/c (1 bytes):""#;
    assert!(out.lines().any(|l| l == want), "{want:?} not in {out}");
}

#[test] // R§11 "Approvals file invalid"
fn invalid_approvals_file_is_left_as_it_is() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let file = approvals(&h);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"workspace = 3\n").unwrap();
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(
        err.starts_with("zygo: cannot store the approval: ")
            && err.contains("invalid approvals file")
            && err.ends_with("; nothing approved\n"),
        "{err}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"workspace = 3\n");
    assert!(!out.contains("approved "), "{out}");
}

#[test] // R§11 "Approval write fails"
fn approval_write_failure_exits_1() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let parent = approvals(&h).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
    std::fs::write(&parent, b"not a folder").unwrap();
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(err.starts_with("zygo: cannot store the approval: ") && err.ends_with("; nothing approved\n"), "{err}");
    assert!(approvals(&h).parent().unwrap().is_file());
}

#[test] // R§11 "Approval write fails" (WD-15 review round 2, finding 4): the line names the cause, not only the path
fn approval_write_failure_names_the_cause() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let file = approvals(&h);
    std::fs::create_dir_all(&file).unwrap();
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    assert_eq!(o.status.code(), Some(1), "{err}");
    let head = format!("zygo: cannot store the approval: {}: ", zygo_core::esc(&file.display().to_string()));
    let cause = err.strip_prefix(&head).and_then(|r| r.strip_suffix("; nothing approved\n"));
    assert!(cause.is_some_and(|c| !c.is_empty()), "{err}");
    if cfg!(unix) {
        assert!(err.contains("Is a directory"), "{err}");
    }
    assert!(!out.contains("approved "), "{out}");
    assert!(file.is_dir() && std::fs::read_dir(&file).unwrap().next().is_none());
}

#[test] // R§7.1 step 4 (WD-15 review F1): --digest is compared before the already-approved check of step 5
fn digest_of_another_value_on_approved_files_exits_1() {
    let _g = serial();
    let f = min();
    let h = home(&f, "");
    let d = digest(&f);
    let o = approve(&f, &h, &["--yes"], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let before = std::fs::read(approvals(&h)).unwrap();
    let other = "a".repeat(64);
    let o = approve(&f, &h, &["--yes", "--digest", other.as_str()], None, &[]);
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    assert_eq!(
        (o.status.code(), out.as_str(), err.as_str()),
        (Some(1), "", &*format!("zygo: the workspace files changed: their digest is {d}, not {other}; nothing approved\n"))
    );
    assert_eq!(std::fs::read(approvals(&h)).unwrap(), before);

    // The matching digest on approved files: the already-approved line, exit 0.
    let o = approve(&f, &h, &["--yes", "--digest", d.as_str()], None, &[]);
    let out = text(&o.stdout);
    assert_eq!((o.status.code(), out.as_str()), (Some(0), &*format!("workspace files already approved ({d})\n")));
    assert_eq!(std::fs::read(approvals(&h)).unwrap(), before);
}

/// A double-quoted summary value at the start of `s`: its text (`\"` and `\\` read back) and the rest after the
/// closing quote. `None` when `s` does not start with `"` or the string has no end.
fn quoted(s: &str) -> Option<(String, &str)> {
    let mut chars = s.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?.1),
            '"' => return Some((out, &s[i + 2..])),
            c => out.push(c),
        }
    }
    None
}

/// The overrides and the disabled roles of a summary `profile <name>:` line, read by the grammar of plan call 9:
/// `overrides <role> = "<model>", ...; disabled <role>, ...` (`-` when empty). Panics on any other text.
fn profile_clauses(line: &str) -> (Vec<(String, String)>, Vec<String>) {
    let (_, rest) = line.split_once(": overrides ").expect("overrides clause");
    let mut overrides = Vec::new();
    let mut rest = rest;
    if let Some(r) = rest.strip_prefix("-") {
        rest = r;
    } else {
        loop {
            let (role, r) = rest.split_once(" = ").expect("role = model");
            assert!(role.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-')), "role {role:?}");
            let (model, r) = quoted(r).expect("quoted model");
            overrides.push((role.to_string(), model));
            match r.strip_prefix(", ") {
                Some(r) => rest = r,
                None => {
                    rest = r;
                    break;
                }
            }
        }
    }
    let list = rest.strip_prefix("; disabled ").expect("one disabled clause");
    let disabled = if list == "-" { vec![] } else { list.split(", ").map(str::to_string).collect() };
    for r in &disabled {
        assert!(r.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-')), "disabled {r:?}");
    }
    (overrides, disabled)
}

#[test] // R§7.1 step 6, plan call 9 (WD-15 review F2): a free-text value cannot fake the structure of a summary line
fn summary_quotes_free_text_values() {
    let _g = serial();
    let ws = "version = 1\n[members.w]\nprofile = \"rust\"\ncwd = 'a \"b\", pool true'\n";
    let p = "overrides = { review = \"opus; disabled -\", x = \"y, implement = haiku\" }\ndisabled = [\"design\"]\n";
    let q = "overrides = { a = 'b\\\" c\\' }\n";
    let f = fx(&[(WS, ws.as_bytes()), (".zygo/profiles/rust.toml", p.as_bytes()), (".zygo/profiles/tricky.toml", q.as_bytes())]);
    let h = home(&f, "");
    let o = approve(&f, &h, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let line = |prefix: &str| out.lines().find(|l| l.starts_with(prefix)).unwrap_or_else(|| panic!("{prefix:?} in {out}"));

    let rust = line("  profile rust:");
    assert_eq!(
        rust,
        r#"  profile rust: overrides review = "opus; disabled -", x = "y, implement = haiku"; disabled design"#
    );
    assert_eq!(
        profile_clauses(rust),
        (
            vec![("review".into(), "opus; disabled -".into()), ("x".into(), "y, implement = haiku".into())],
            vec!["design".to_string()]
        )
    );

    let tricky = line("  profile tricky:");
    assert_eq!(tricky, r#"  profile tricky: overrides a = "b\\\" c\\"; disabled -"#);
    assert_eq!(profile_clauses(tricky), (vec![("a".into(), "b\\\" c\\".into())], vec![]));

    assert_eq!(
        line("  member w:"),
        r#"  member w: kind claude-code, job worker, profile rust, pool false, cwd "a \"b\", pool true""#
    );
}

/// `f.t/b.git`: a bare clone of `o.git` with `refs/remotes/origin/*` fetched and `origin/HEAD` set to `main`.
fn bare_clone(f: &Fx) -> PathBuf {
    let t = f.t.path();
    git(t, &["clone", "-q", "--bare", "o.git", "b.git"]);
    let b = t.join("b.git");
    git(&b, &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
    git(&b, &["fetch", "-q", "origin"]);
    git(&b, &["remote", "set-head", "origin", "main"]);
    b
}

/// Approve with `--yes` in `dir`, then the launcher's read (`check_default` at the checkout's top, R§8) is `Approved`
/// with the digest that approve printed.
fn approve_where_the_launcher_reads(f: &Fx, dir: &Path) {
    let h = home(f, "");
    let d = match check_default(dir, &id(dir), &approvals(&h), &SystemGit) {
        Use::NotApproved { digest, .. } => digest,
        u => panic!("the launcher's read before approve: {u:?}"),
    };
    let o = run_in(&h, dir, &["--yes"], None, &[]);
    let out = text(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(out.ends_with(&format!("approved {d}\n{NEXT}\n")), "{out}");
    let top = checkout(dir, Instant::now() + Duration::from_secs(60)).unwrap().top;
    assert!(out.starts_with(&format!("folder: {}\n", zygo_core::esc(&top.to_string_lossy()))), "{out}");
    match check_default(dir, &id(dir), &approvals(&h), &SystemGit) {
        Use::Approved { pin, .. } => assert_eq!(pin.digest, d),
        u => panic!("the launcher's read after approve: {u:?}"),
    }
}

#[test] // R§7.1 step 3 (WD-15 review F3): a linked checkout of a bare repository
fn bare_repository_linked_checkout_is_approved_where_the_launcher_reads() {
    let _g = serial();
    let f = min();
    let b = bare_clone(&f);
    let wt = f.t.path().join("wt");
    git(&b, &["worktree", "add", "-q", wt.to_str().unwrap(), "main"]);
    approve_where_the_launcher_reads(&f, &wt);
}

#[test] // R§7.1 step 3 (WD-15 review F3): the first checkout's folder is deleted; approve reads at the current checkout
fn first_checkout_deleted_is_approved_where_the_launcher_reads() {
    let _g = serial();
    let f = min();
    let b = bare_clone(&f);
    let (w1, w2) = (f.t.path().join("w1"), f.t.path().join("w2"));
    git(&b, &["worktree", "add", "-q", w1.to_str().unwrap(), "main"]);
    git(&b, &["worktree", "add", "-q", "-b", "side", w2.to_str().unwrap(), "main"]);
    std::fs::remove_dir_all(&w1).unwrap();
    approve_where_the_launcher_reads(&f, &w2);
}
