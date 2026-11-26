//! MS-17: zygo mail sync, its options and the hidden background form (mail sync design S§6.6, S§7.6, S§8.1-S§8.3,
//! S§9; workspaces design AU44, AU71). Two machines are two scratch homes on one `file://` bare repository; no agent runs.
#![cfg(unix)]
mod common;
#[path = "common/e2e.rs"]
mod e2e;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime};

use zygo_config::{builtin_patterns, RedactionConfig, SyncSettings};
use zygo_core::Agent;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::outbox::send_remote_with;
use zygo_mail_sync::state::{load, SyncPaths};
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::{mail_dir, store};
use zygo_workspace::state::MemberRecord;

use common::feed_stdin;
use common::workspace::{Harness, WS_ID};
use e2e::set_mtime;

const ALICE: &str = "kaaaaaaaa";
const BOB: &str = "kbbbbbbbb";
const CI_NOTE: &str = "zygo: each mail sync push can start CI and webhooks on the forge. The commits hold [skip ci]; webhooks, other workflow events and CI systems that do not read [skip ci] still run. Exclude zygo/** in such CI configs.";
const COLLISION: &str = "zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.";

/// `git -c user.name=t -c user.email=t@t.invalid -c commit.gpgSign=false <args>` in `dir`; panics on a non-zero
/// exit; the stdout without its trailing newline.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgSign=false"])
        .args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", err(&o));
    out(&o).trim_end().to_string()
}

/// A bare repository `bare.git` with one commit on `main` (pushed from a seed clone), and its `file://` URL.
struct Remote { _t: tempfile::TempDir, bare: PathBuf, url: String }
fn remote() -> Remote {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    git(&p, &["init", "-q", "--bare", "bare.git"]);
    let bare = p.join("bare.git");
    let url = format!("file://{}", bare.display());
    git(&p, &["clone", "-q", &url, "seed"]);
    std::fs::write(p.join("seed/README"), "x\n").unwrap();
    git(&p.join("seed"), &["add", "README"]);
    git(&p.join("seed"), &["commit", "-q", "-m", "seed"]);
    git(&p.join("seed"), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    Remote { _t: t, bare, url }
}

/// User config: `[mail]` with `host = "<host>"` when given, then `[mail.workspace.app]` `sync = "zygo/mail"`, then
/// `extra` as it is (so `extra` keys land in `[mail.workspace.app]`).
fn config(h: &Harness, host: Option<&str>, extra: &str) {
    let host = host.map(|x| format!("host = \"{x}\"\n")).unwrap_or_default();
    std::fs::write(h.root().join("config.toml"), format!("[mail]\n{host}[mail.workspace.app]\nsync = \"zygo/mail\"\n{extra}")).unwrap();
}

/// One machine (D13): a scratch home whose `repo` is a clone of `r`, `git` linked into `bin`, the config of `host`,
/// and the workspace `app` (`WS_ID`) with the members `(pane key, name)`.
fn machine(r: &Remote, host: &str, members: &[(&str, &str)]) -> Harness {
    let h = Harness::no_tmux();
    git(h.root(), &["clone", "-q", &r.url, "repo"]);
    std::os::unix::fs::symlink(which::which("git").unwrap(), h.bin.join("git")).unwrap();
    config(&h, Some(host), "");
    h.seed(WS_ID, &mut |w| {
        for (k, n) in members {
            w.members.insert(k.to_string(), MemberRecord { name: Some(n.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
        }
    });
    h
}

/// `h.zygo(args, …)` with `ZYGO_WORKSPACE = WS_ID` and then `extra`; `input` fed to stdin (as `tests/mail_cli.rs:62`),
/// none: the null device.
fn zygo(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)], input: Option<&[u8]>) -> Output {
    let mut env = vec![("ZYGO_WORKSPACE", OsStr::new(WS_ID))];
    env.extend_from_slice(extra);
    let mut c = h.zygo(args, &env);
    let Some(bytes) = input else { return c.output().unwrap() };
    c.stdin(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    feed_stdin(child.stdin.take().unwrap(), bytes);
    child.wait_with_output().unwrap()
}
fn out(o: &Output) -> String { String::from_utf8_lossy(&o.stdout).into_owned() }
fn err(o: &Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }

/// `zygo mail sync`, which must exit 0.
fn sync(h: &Harness) -> Output {
    let o = zygo(h, &["mail", "sync"], &[], None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    o
}

fn sp(h: &Harness) -> SyncPaths { SyncPaths::new(&h.paths, WS_ID).unwrap() }

/// One message from `from` on host `own` to `to` in the outbox of `h`; the secret pass has a 1 h base, so no clock
/// decides the test. Returns the ULID.
fn send(h: &Harness, own: &str, from: &str, to: (&str, &str)) -> String {
    let s = SyncSettings { branch: "zygo/mail".into(), remote: "origin".into(), host: own.into(), sync_gap: Duration::from_secs(60) };
    let policy = DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() };
    let red = RedactionConfig { pattern: builtin_patterns(), ..Default::default() };
    let to = Address { name: to.0.into(), host: to.1.into() };
    send_remote_with(&sp(h), Ok(&s), Some(from), &to, "s", "b", &red, &h.repo(), SystemTime::now(), &policy).unwrap().to_string()
}

/// The paths at the tip of `refs/heads/zygo/mail` of the bare repository; empty when the branch does not exist.
fn tip_paths(r: &Remote) -> Vec<String> {
    let o = std::process::Command::new("git").args(["ls-tree", "-r", "--name-only", "refs/heads/zygo/mail"])
        .current_dir(&r.bare).output().unwrap();
    if !o.status.success() { return Vec::new(); }
    out(&o).lines().map(String::from).collect()
}
fn commits(r: &Remote) -> String { git(&r.bare, &["rev-list", "--count", "refs/heads/zygo/mail"]) }
fn errors(h: &Harness) -> String { std::fs::read_to_string(h.store(WS_ID).dir().join("errors.log")).unwrap_or_default() }

/// A dead session folder `<sessions_dir>/<name>` with `token`, `pid` and `results/f0`, each entry 25 h old (no lock on
/// `pid`, so not live; as `tests/results_sweep.rs:27-36`).
fn stale(h: &Harness, name: &str) -> PathBuf {
    let d = h.paths.sessions_dir.join(name);
    std::fs::create_dir_all(d.join("results")).unwrap();
    for (f, b) in [("results/f0", "x"), ("token", "t"), ("pid", "1")] { std::fs::write(d.join(f), b).unwrap(); }
    let t = SystemTime::now() - Duration::from_secs(25 * 3600);
    for p in [d.join("results/f0"), d.join("token"), d.join("pid"), d.join("results"), d.clone()] { set_mtime(&p, t); }
    d
}

#[test] // S§8.3 steps 13-16, S§19 #1 (CLI part), S§9 When: pushed; no commit with no change; --prune
fn sync_pushes_the_outbox_message_and_a_second_run_commits_nothing() { // T1
    let r = remote();
    let (a, b) = (machine(&r, "a", &[(ALICE, "alice")]), machine(&r, "b", &[(BOB, "bob")]));
    sync(&a);
    sync(&b);
    sync(&a);
    let id = send(&a, "a", "alice", ("bob", "b"));
    let o = sync(&a);
    assert!(out(&o).ends_with("zygo: mail sync: 1 sent, 0 delivered\n"), "{}", out(&o));
    assert!(tip_paths(&r).contains(&format!("bob@b/{id}.json")));
    assert_eq!(zygo_mail_sync::outbox::count(&sp(&a)).unwrap(), 0);
    let n = commits(&r);
    let o = sync(&a);
    assert!(out(&o).ends_with("zygo: mail sync: 0 sent, 0 delivered\n"), "{}", out(&o));
    assert_eq!(commits(&r), n);
    let o = sync(&b);
    assert!(out(&o).ends_with("zygo: mail sync: 0 sent, 1 delivered\n"), "{}", out(&o));
    assert_eq!(store::unread(&mail_dir(&b.paths, WS_ID), BOB).unwrap(), 1);
    let o = zygo(&a, &["mail", "sync", "--prune"], &[], None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(commits(&r), "1"); // S§9 Commit: the prune is one root commit
    assert!(!tip_paths(&r).contains(&format!("bob@b/{id}.json"))); // acked by b
}

#[test] // AU44, D6: --name picks the workspace by name; no name and no checkout gives the mail verbs' line
fn name_option_picks_the_workspace_by_name() { // T2
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    let o = a.zygo(&["mail", "sync"], &[]).output().unwrap();
    assert_eq!((o.status.code(), err(&o)), (Some(1), "no workspace for this session or folder\n".to_string()));
    let o = a.zygo(&["mail", "sync", "--name", "nope"], &[]).output().unwrap();
    assert_eq!((o.status.code(), err(&o)), (Some(1), "zygo: no workspace named nope\n".to_string()));
    let o = a.zygo(&["mail", "sync", "--name", "app"], &[]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(tip_paths(&r).contains(&"registry/a.json".to_string()));
}

#[test] // S§8.3 step 1, D3: sync off exits 1 with its reason, prints nothing on stdout and writes nothing
fn sync_off_exits_1_with_its_reason() { // T3
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    for (cfg, line) in [
        ("", "zygo: mail sync is off for workspace app; set sync in [mail.workspace.app] of the user config"),
        ("[mail.workspace.app]\nsync = \"zygo/mail\"\n", "zygo: mail sync is off: no mail host; set host in [mail] of the user config"),
        ("[mail]\nhost = \"-a\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n", "zygo: mail sync is off: the mail host is not 1 to 32 of a-z, 0-9 and -, starting with a letter or a digit"),
        ("[mail]\nhost = \"a\"\n[mail.workspace.app]\nsync = \"main\"\n", "zygo: mail sync is off: sync in [mail.workspace.app] is not zygo/ and 1 to 32 of a-z, 0-9 and -"),
        ("[mail]\nhost = \"a\"\nsync_gap = \"x\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n", "zygo: mail sync is off: sync_gap in [mail] is not <n>s, <n>m or <n>h"),
    ] {
        std::fs::write(a.root().join("config.toml"), cfg).unwrap();
        let o = zygo(&a, &["mail", "sync"], &[], None);
        assert_eq!((o.status.code(), err(&o), out(&o)), (Some(1), format!("{line}\n"), String::new()), "{cfg}");
        assert!(!sp(&a).sync.exists(), "{cfg}");
    }
    assert!(tip_paths(&r).is_empty());
}

#[test] // S§8.2 (as rewritten), D1, D12: the unknown line is printed; no URL user info on the terminal or in a file
fn printed_urls_carry_no_userinfo() { // T4
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    git(&a.repo(), &["remote", "set-url", "--push", "origin", "https://u1:s3cret@127.0.0.1:1/r.git"]);
    let o = sync(&a);
    assert!(err(&o).lines().any(|l| l.starts_with("zygo: cannot check if the remote is public: ")), "{}", err(&o));
    let state = std::fs::read_to_string(&sp(&a).state).unwrap();
    for text in [out(&o), err(&o), errors(&a), state] {
        assert!(!text.contains("s3cret") && !text.contains("u1:"), "{text}");
    }
}

#[test] // S§8.3 steps 1-3, S§8.1 child row: the child is silent and exits 0 when the lock is held, when off, for a bad id
fn background_is_silent_and_exits_0_when_the_lock_is_held() { // T6
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    let s = sp(&a);
    for d in [&s.mail, &s.sync] { zygo_workspace::mail::ensure_mail_root(d).unwrap(); }
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&s.lock).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    let o = zygo(&a, &["mail", "sync", "--ws-id", WS_ID, "--background"], &[], None);
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(0), String::new(), String::new()));
    assert!(s.again.exists()); // S§8.3 step 2: the holder sees the mark
    drop(f);
    assert!(tip_paths(&r).is_empty());
    std::fs::write(a.root().join("config.toml"), "").unwrap();
    for id in [WS_ID, "../x"] {
        let o = zygo(&a, &["mail", "sync", "--ws-id", id, "--background"], &[], None);
        assert_eq!((o.status.code(), out(&o), err(&o)), (Some(0), String::new(), String::new()), "{id}");
    }
    assert_eq!(errors(&a), "");
}

#[test] // S§8.3, S-C2: a child's stop goes to errors.log as its check name; the CLI prints the line
fn background_errors_go_to_errors_log_as_check_names() { // T7
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    config(&a, Some("a"), "remote = \"nope\"\n");
    let o = zygo(&a, &["mail", "sync", "--ws-id", WS_ID, "--background"], &[], None);
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(1), String::new(), String::new()));
    let log = errors(&a);
    assert!(log.lines().last().unwrap().ends_with(" mail sync: stopped: remote"), "{log}");
    assert!(!log.contains("nope"), "{log}");
    let o = zygo(&a, &["mail", "sync"], &[], None);
    assert_eq!((o.status.code(), err(&o)), (Some(1), "zygo: no such remote\n".to_string()));
}

#[test] // S§17 row cmd/mod.rs, AU71: no results sweep for mail sync --background; zygo mail sync sweeps
fn results_sweep_skips_the_background_form_only() { // T11
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    std::fs::write(a.root().join("config.toml"), "").unwrap();
    let old = stale(&a, "old-s");
    let o = zygo(&a, &["mail", "sync", "--ws-id", WS_ID, "--background"], &[], None);
    assert_eq!(o.status.code(), Some(0));
    assert!(old.join("results/f0").exists());
    let o = zygo(&a, &["mail", "sync"], &[], None);
    assert_eq!(o.status.code(), Some(1)); // sync off, after the sweep
    assert!(!old.exists());
}

#[test] // review focus 53, AU71, AU44, C6: a panic in the child prints nothing, skips the sweep, logs one check name
fn background_panic_prints_nothing_and_skips_the_results_sweep() { // T12
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    let old = stale(&a, "old-s");
    let o = zygo(&a, &["mail", "sync", "--ws-id", WS_ID, "--background"], &[("ZYGO_TEST_PANIC_SYNC", OsStr::new("1"))], None);
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(101), String::new(), String::new()));
    assert!(old.join("results/f0").exists());
    let log = errors(&a);
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.ends_with(" mail sync: stopped: panic\n"), "{log}");
}

#[test] // S§7.6, S§19 #20 (drop part), D5: the count and the question; n deletes nothing; --yes deletes
fn drop_host_counts_asks_and_yes_deletes() { // T8
    let r = remote();
    let (a, b) = (machine(&r, "a", &[(ALICE, "alice")]), machine(&r, "b", &[(BOB, "bob")]));
    sync(&a);
    sync(&b);
    sync(&a);
    for _ in 0..3 { send(&a, "a", "alice", ("bob", "b")); }
    sync(&a);
    let prompt = "zygo: 3 messages wait for host b; they will be deleted. Delete them? [y/N]";
    let before = tip_paths(&r);
    let o = zygo(&a, &["mail", "sync", "--drop-host", "b"], &[], Some(b"n\n"));
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(0), "zygo: host b not dropped\n".to_string(), format!("{prompt} ")));
    assert_eq!(tip_paths(&r), before);
    let o = zygo(&a, &["mail", "sync", "--drop-host", "b", "--yes"], &[], None);
    assert_eq!((o.status.code(), out(&o), err(&o)), (Some(0), "zygo: host b dropped; 3 messages deleted\n".to_string(), format!("{prompt} y (--yes)\n")));
    let after = tip_paths(&r);
    assert!(after.iter().all(|p| !p.starts_with("bob@b/") && p != "registry/b.json" && p != "ack/b.json"), "{after:?}");
    assert!(after.contains(&"registry/a.json".to_string()));
}

#[test] // S§7.6, S§19 #9 (CLI part): the collision line; --take-host syncs; --new-identity with a new host syncs
fn take_host_and_new_identity_run_the_sync() { // T9
    let r = remote();
    let x1 = machine(&r, "x", &[(ALICE, "alice")]);
    sync(&x1);
    let x2 = machine(&r, "x", &[(ALICE, "alice")]);
    let o = zygo(&x2, &["mail", "sync"], &[], None);
    assert_eq!((o.status.code(), err(&o)), (Some(1), format!("{COLLISION}\n")));
    let o = zygo(&x2, &["mail", "sync", "--take-host"], &[], None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).ends_with("zygo: mail sync: 0 sent, 0 delivered\n"), "{}", out(&o));
    let o = zygo(&x1, &["mail", "sync"], &[], None);
    assert_eq!((o.status.code(), err(&o)), (Some(1), format!("{COLLISION}\n")));
    config(&x1, Some("x1"), "");
    let o = zygo(&x1, &["mail", "sync", "--new-identity"], &[], None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(tip_paths(&r).contains(&"registry/x1.json".to_string()));
}

#[test] // S§8.1 row 4, AU44: the user options are in the help, the hidden form is not; zygo mail lists sync
fn help_shows_the_options_and_hides_the_child_form() { // T5
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    let help = out(&a.zygo(&["mail", "sync", "--help"], &[]).output().unwrap());
    for f in ["--name", "--prune", "--take-host", "--new-identity", "--drop-host", "--yes"] {
        assert!(help.contains(f), "{f}: {help}");
    }
    assert!(!help.contains("--ws-id") && !help.contains("--background"), "{help}");
    let mail = out(&a.zygo(&["mail", "--help"], &[]).output().unwrap());
    assert!(mail.lines().any(|l| l.trim_start().starts_with("sync ")), "{mail}");
}
