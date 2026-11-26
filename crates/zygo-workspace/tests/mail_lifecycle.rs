//! FX-13: member record and maildir lifecycle (M§5 Retention, P1).
//!
//! Every test and helper is `cfg(unix)`: the tmux stand-ins are shell scripts and the tests check file modes. Every
//! test takes `serial()`, so no test writes a script while another test forks (ETXTBSY).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use zygo_config::{Config, Loadout, LoadoutPane, LoadoutWindow, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::admin;
use zygo_workspace::identity::{valid_pane_key, workspace_id};
use zygo_workspace::mail::{
    mail_dir, prune, remove_maildir, send, store as mail_store, Delivery, MailError, Message, MAIL_LOCK_WAIT,
};
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::repair;
use zygo_workspace::state::{
    BuildPlan, MemberRecord, MemberState, Pane, PaneKind, Pending, Phase, Store, Window, Workspace,
};
use zygo_workspace::up::{self, UpOpts};

const S5: Duration = Duration::from_secs(5);
const OTHER: &str = "fedcba9876543210";
const THIRD: &str = "0123456789abcdef";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;

// ---- helpers (copied from tests/up.rs and tests/admin.rs) ----

fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TEMPLATE_DIR", "")
        .output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn canon(p: &Path) -> PathBuf { p.canonicalize().unwrap() }

/// `<parent>/<name>`: a repository on `main` with one commit; returns the canonical path.
fn repo(parent: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(parent).unwrap();
    git(parent, &["init", "-q", "-b", "main", name]);
    git(&parent.join(name), &["commit", "-q", "--allow-empty", "-m", "a"]);
    canon(&parent.join(name))
}

/// An executable `#!/bin/sh` program `<dir>/<name>`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let f = dir.join(name);
    fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    f
}

const ANSWER_BODY: &str = r#"d='@D'
printf '%s\036' "$*" >> "$d/tmux.log"
sub=; tgt=
while [ $# -gt 0 ]; do
  case "$1" in
    -V) echo 'tmux 3.4'; exit 0 ;;
    -S|-t|-F|-s|-c|-x|-y|-n) [ "$1" = -t ] && tgt=$2; shift ;;
    -*) ;;
    *) [ -z "$sub" ] && sub=$1 ;;
  esac
  shift
done
s=${tgt#=}; s=${s%%:*}
case "$sub" in
  has-session) [ -f "$d/exists-$s" ] && exit 0; [ -f "$d/error-$s" ] && { echo 'permission denied' >&2; exit 1; } ;;
  show-options) [ -f "$d/ws-$s" ] && { cat "$d/ws-$s"; exit 0; }; [ -f "$d/exists-$s" ] && exit 0 ;;
  display-message) [ -f "$d/clients-$s" ] && { cat "$d/clients-$s"; exit 0; } ;;
esac
echo 'no server running on /tmp/zygo-none/s' >&2
exit 1"#;

/// The stand-in tmux of `tests/admin.rs`: it logs each call and answers "no server running" to the calls here.
fn answer_tmux(dir: &Path) -> PathBuf { script(dir, "atmux", &ANSWER_BODY.replace("@D", &dir.display().to_string())) }

fn member(kind: Agent, state: MemberState, session: Option<&str>) -> MemberRecord {
    let mut m = MemberRecord::new(kind);
    m.state = state;
    m.agent_session = session.map(String::from);
    m
}

fn member_in(kind: Agent, state: MemberState, session: Option<&str>, zygo_session: &str) -> MemberRecord {
    let mut m = member(kind, state, session);
    m.zygo_session = Some(zygo_session.into());
    m
}

fn pane(key: &str, window: u32, index: u32, kind: PaneKind, cwd: &Path) -> Pane {
    Pane { key: key.into(), window, index, cwd: cwd.to_path_buf(), kind, ..Pane::default() }
}

static LIVE: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

fn text(b: Vec<u8>) -> String { String::from_utf8(b).unwrap() }

fn pane_run(p: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path, key: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = admin::forget_pane(p, tmux, name, cwd, key, &mut out, &mut err);
    (code, text(out), text(err))
}

/// Delivers `n` messages to `mail/<key>/new` of workspace `id` with the store alone (no record check).
fn put(p: &Paths, id: &str, key: &str, n: usize) {
    for i in 0..n {
        let m = Message::new("cli", "x", &format!("s{i}"), "b", SystemTime::now()).unwrap();
        mail_store::deliver(&mail_dir(p, id), key, "x", &m, 500).unwrap();
    }
}

fn unread(p: &Paths, id: &str, key: &str) -> usize { mail_store::unread(&mail_dir(p, id), key).unwrap() }

fn gone(path: &Path) -> bool { fs::symlink_metadata(path).is_err() }

fn mode(path: &Path) -> u32 { fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777 }

struct PaneSetup {
    _td: TempDir,
    p: Paths,
    tmux: PathBuf,
    id: String,
    store: Store,
    plain: PathBuf,
}

/// As `tests/admin.rs` `pane_setup`: workspace `app` in phase `down` with records `kaaaaaaaa` and `kbbbbbbbb`.
fn pane_setup() -> PaneSetup {
    let td = TempDir::new().unwrap();
    let t = td.path().to_path_buf();
    let p = paths(&t);
    let tmux = answer_tmux(&t);
    let r = repo(&t, "app");
    let id = workspace_id(&canon(&r.join(".git")));
    let store = Store::open(&p, &id);
    store
        .update(S5, &mut |ws| {
            ws.name = "app".into();
            ws.tmux_session = "zygo-app".into();
            ws.common_dir = canon(&r.join(".git"));
            ws.main_root = r.clone();
            ws.loadout = "solo".into();
            ws.phase = Phase::Down;
            ws.panes = vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &r)];
            ws.members.insert("kaaaaaaaa".into(), member_in(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"), "01P"));
            ws.members.insert("kbbbbbbbb".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2")));
            Ok(())
        })
        .unwrap();
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    PaneSetup { _td: td, p, tmux, id, store, plain }
}

/// A stored state of workspace `id` (identity fields as `tests/mail_names.rs` `ws`) with the named records.
fn stored(p: &Paths, id: &str, members: &[(&str, Option<&str>)]) -> Store {
    let store = Store::open(p, id);
    store
        .update(S5, &mut |ws| {
            ws.name = "app".into();
            ws.common_dir = "/srv/app/.git".into();
            ws.main_root = "/srv/app".into();
            ws.tmux_session = "zygo-app".into();
            ws.loadout = "solo".into();
            ws.phase = Phase::Up;
            ws.members.clear();
            for (k, n) in members {
                let mut m = MemberRecord::new(Agent::ClaudeCode);
                m.name = n.map(String::from);
                ws.members.insert(k.to_string(), m);
            }
            Ok(())
        })
        .unwrap();
    store
}

fn read(store: &Store) -> Workspace { store.read().unwrap().unwrap() }

// ---- step 3: paths a1, a2; the API ----

#[test] // a1: a send from a state read before `forget-pane` makes no mail/<key>/ again
fn lifecycle_stale_send_after_forget_pane_creates_no_maildir() {
    let _s = serial();
    let PaneSetup { _td, p, tmux, id, store, plain } = pane_setup();
    put(&p, &id, "kbbbbbbbb", 1);
    let stale = read(&store);
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    let r = send(&p, &stale, None, "kbbbbbbbb", "s", "b", &MailConfig::default(), at(T0));
    assert!(matches!(r, Err(MailError::UnknownTarget { .. })), "{r:?}");
    assert!(gone(&mail_dir(&p, &id).join("kbbbbbbbb")));
}

#[test] // a2: a send to a key whose record is gone (any removal) makes no maildir
fn lifecycle_send_to_a_key_with_no_record_creates_no_maildir() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", Some("lead")), ("kbbbbbbbb", Some("bob"))]);
    let stale = read(&store);
    store
        .update(S5, &mut |ws| {
            ws.members.remove("kbbbbbbbb");
            Ok(())
        })
        .unwrap();
    let r = send(&p, &stale, None, "kbbbbbbbb", "s", "b", &MailConfig::default(), at(T0));
    assert!(matches!(r, Err(MailError::UnknownTarget { .. })), "{r:?}");
    let r = send(&p, &stale, None, "all", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    assert_eq!(r.sent.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["kaaaaaaaa"]);
    assert!(r.refused.is_empty(), "{r:?}");
    assert!(gone(&mail_dir(&p, id).join("kbbbbbbbb")));

    let msg = Message::new("cli", "bob", "s", "b", at(T0 + 1)).unwrap();
    let d = Delivery::begin(&store, MAIL_LOCK_WAIT).unwrap();
    let e = d.deliver("kbbbbbbbb", "bob", &msg, 500).unwrap_err();
    assert!(matches!(&e, MailError::NoRecord { key } if key == "kbbbbbbbb"), "{e:?}");
    assert_eq!(e.to_string(), "zygo: pane kbbbbbbbb has no member record; nothing delivered");
    drop(d);
    assert!(gone(&mail_dir(&p, id).join("kbbbbbbbb")));

    let e = Delivery::begin(&Store::open(&p, OTHER), MAIL_LOCK_WAIT).err().unwrap();
    assert!(matches!(e, MailError::NoWorkspace), "{e:?}");
    assert_eq!(e.to_string(), "zygo: the workspace has no state; nothing sent");
    assert!(gone(&p.workspaces_dir().join(OTHER)));
}

#[test] // F4: the report carries the read under the lock: a record added after the caller's read gets its label, and
// the wake reads that record
fn lifecycle_send_report_carries_the_read_under_the_lock() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", Some("lead")), ("kbbbbbbbb", Some("bob"))]);
    let stale = read(&store);
    store
        .update(S5, &mut |ws| {
            let mut m = MemberRecord::new(Agent::ClaudeCode);
            m.name = Some("carol".into());
            ws.members.insert("kcccccccc".into(), m);
            Ok(())
        })
        .unwrap();
    let r = send(&p, &stale, Some("kaaaaaaaa"), "all", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    let got: Vec<(&str, &str)> = r.sent.iter().map(|s| (s.key.as_str(), s.label.as_str())).collect();
    assert_eq!(got, [("kbbbbbbbb", "bob"), ("kcccccccc", "carol")]);
    assert!(r.refused.is_empty(), "{r:?}");
    assert_eq!(r.workspace.members.get("kcccccccc").and_then(|m| m.valid_name()), Some("carol"));
    assert!(!stale.members.contains_key("kcccccccc"));
}

#[test] // P1 removal: prune and remove_maildir take mail/lock exclusive and remove only keys with no record
fn lifecycle_prune_removes_only_keys_without_a_record() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", Some("lead"))]);
    let mail = mail_dir(&p, id);
    put(&p, id, "kaaaaaaaa", 1);
    put(&p, id, "kbbbbbbbb", 1);
    fs::write(mail.join("kcccccccc"), "not a maildir").unwrap();
    let outside = t.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep"), "k").unwrap();
    std::os::unix::fs::symlink(&outside, mail.join("kdddddddd")).unwrap();
    fs::create_dir_all(mail.join("sync")).unwrap();
    fs::create_dir_all(mail.join("outbox")).unwrap();

    let held = Delivery::begin(&store, MAIL_LOCK_WAIT).unwrap();
    let e = prune(&store, Duration::ZERO).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "{e}");
    assert_eq!(e.to_string(), format!("{}/lock is held by another process", mail.display()));
    for k in ["kaaaaaaaa", "kbbbbbbbb", "kcccccccc", "kdddddddd", "sync", "outbox"] {
        assert!(!gone(&mail.join(k)), "{k}");
    }
    drop(held);

    assert_eq!(prune(&store, MAIL_LOCK_WAIT).unwrap(), ["kbbbbbbbb", "kcccccccc", "kdddddddd"]);
    for k in ["kbbbbbbbb", "kcccccccc", "kdddddddd"] {
        assert!(gone(&mail.join(k)), "{k}");
    }
    assert_eq!(unread(&p, id, "kaaaaaaaa"), 1);
    assert!(outside.join("keep").is_file());
    assert!(mail.join("sync").is_dir() && mail.join("outbox").is_dir());
    assert_eq!(mode(&mail.join("lock")), 0o600);
    assert_eq!(mode(&mail), 0o700);
    assert!(!remove_maildir(&store, "kaaaaaaaa", MAIL_LOCK_WAIT).unwrap());
    assert_eq!(unread(&p, id, "kaaaaaaaa"), 1);

    // A workspace with no mail/: nothing to do, and mail/ is not made.
    let second = stored(&p, OTHER, &[("kaaaaaaaa", None)]);
    assert_eq!(prune(&second, MAIL_LOCK_WAIT).unwrap(), Vec::<String>::new());
    assert!(gone(&mail_dir(&p, OTHER)));
    assert!(!remove_maildir(&second, "kbbbbbbbb", MAIL_LOCK_WAIT).unwrap());
    assert!(gone(&mail_dir(&p, OTHER)));

    // A workspace whose mail/ is a link: it is not followed.
    let q = paths(&t.join("q"));
    let third = stored(&q, id, &[("kaaaaaaaa", None)]);
    let target = t.join("linked");
    fs::create_dir_all(target.join("kbbbbbbbb/new")).unwrap();
    std::os::unix::fs::symlink(&target, mail_dir(&q, id)).unwrap();
    let e = prune(&third, MAIL_LOCK_WAIT).unwrap_err();
    assert_eq!(e.to_string(), format!("{} is a link; it is not followed", mail_dir(&q, id).display()));
    assert!(target.join("kbbbbbbbb/new").is_dir());
    assert!(remove_maildir(&third, "kbbbbbbbb", MAIL_LOCK_WAIT).is_err());
    assert!(target.join("kbbbbbbbb/new").is_dir());
}

#[test] // F5: prune and remove_maildir remove nothing when the state under the lock is invalid, newer or unreadable
fn lifecycle_removal_with_a_bad_state_removes_nothing() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", None)]);
    put(&p, id, "kaaaaaaaa", 1);
    put(&p, id, "kbbbbbbbb", 1);
    let file = store.dir().join("workspace.json");
    let good = fs::read(&file).unwrap();
    let mut newer: serde_json::Value = serde_json::from_slice(&good).unwrap();
    newer["version"] = serde_json::json!(2);
    let cases: [(&str, Vec<u8>, u32); 3] = [
        ("invalid", b"{ not json".to_vec(), 0o600),
        ("newer", serde_json::to_vec(&newer).unwrap(), 0o600),
        ("unreadable", good.clone(), 0o000),
    ];
    for (what, bytes, mode) in cases {
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&file, &bytes).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        if mode == 0 && fs::read(&file).is_ok() {
            continue; // root reads a 0000 file: no unreadable state here
        }
        assert!(prune(&store, MAIL_LOCK_WAIT).is_err(), "{what}");
        assert!(remove_maildir(&store, "kbbbbbbbb", MAIL_LOCK_WAIT).is_err(), "{what}");
        assert_eq!(unread(&p, id, "kaaaaaaaa"), 1, "{what}");
        assert_eq!(unread(&p, id, "kbbbbbbbb"), 1, "{what}");
    }
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&file, &good).unwrap();
    assert_eq!(prune(&store, MAIL_LOCK_WAIT).unwrap(), ["kbbbbbbbb"]);
}

#[test] // F5: a send while a removal holds mail/lock exclusive is refused with the Busy line and delivers nothing
#[ignore = "timing: send waits MAIL_LOCK_WAIT (2 s) for mail/lock"]
fn lifecycle_send_while_a_removal_holds_the_lock_is_busy() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", Some("lead")), ("kbbbbbbbb", Some("bob"))]);
    let stale = read(&store);
    let mail = mail_dir(&p, id);
    fs::create_dir_all(&mail).unwrap();
    let held = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(mail.join("lock")).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    let e = send(&p, &stale, None, "bob", "s", "b", &MailConfig::default(), at(T0)).unwrap_err();
    assert!(matches!(e, MailError::Busy), "{e:?}");
    assert_eq!(e.to_string(), "zygo: mail is busy; try again");
    assert!(gone(&mail.join("kbbbbbbbb")));
    drop(held);
    let r = send(&p, &stale, None, "bob", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    assert_eq!(r.sent.len(), 1);
}

// ---- step 5: path c (`read`) ----

#[test] // c: a maildir removed while a read runs is not made again by that read
fn lifecycle_read_does_not_make_a_removed_maildir_again() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", None)]);
    put(&p, id, "kaaaaaaaa", 1);
    let mail = mail_dir(&p, id);
    let hm = mail.clone();
    up::testing::hook(
        store.dir(),
        "mail_read_before_dirs",
        Some(Box::new(move || mail_store::remove_pane(&hm, "kaaaaaaaa").map_err(|e| e.to_string()))),
    );
    let got = mail_store::read(&mail, "kaaaaaaaa", 10);
    up::testing::hook(store.dir(), "mail_read_before_dirs", None);
    assert_eq!(got.unwrap(), Vec::<Message>::new());
    assert!(gone(&mail.join("kaaaaaaaa")));
}

/// `<dir>/new/<id>.json`, one message file outside `mail/`; returns its path.
fn outside_message(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir.join("new")).unwrap();
    let m = Message::new("cli", "x", "s", "b", at(T0)).unwrap();
    let f = dir.join("new").join(format!("{}.json", m.id));
    fs::write(&f, serde_json::to_vec(&m).unwrap()).unwrap();
    f
}

#[test] // F1: a read does not follow a link at mail/<key>; the files it points to stay
fn lifecycle_read_refuses_a_link_at_the_maildir() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    stored(&p, id, &[("kaaaaaaaa", None)]);
    let mail = mail_dir(&p, id);
    fs::create_dir_all(&mail).unwrap();
    let outside = t.join("outside");
    let f = outside_message(&outside);
    std::os::unix::fs::symlink(&outside, mail.join("kaaaaaaaa")).unwrap();
    let r = mail_store::read(&mail, "kaaaaaaaa", 10);
    assert!(matches!(r, Err(MailError::Io(_))), "{r:?}");
    assert!(f.is_file());
    assert!(gone(&outside.join("cur")));
}

#[test] // F1: a read does not follow a link at mail/<key>/new; the files it points to stay
fn lifecycle_read_refuses_a_link_at_new() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    stored(&p, id, &[("kaaaaaaaa", None)]);
    let mail = mail_dir(&p, id);
    fs::create_dir_all(mail.join("kaaaaaaaa")).unwrap();
    let outside = t.join("outside");
    let f = outside_message(&outside);
    std::os::unix::fs::symlink(outside.join("new"), mail.join("kaaaaaaaa/new")).unwrap();
    let r = mail_store::read(&mail, "kaaaaaaaa", 10);
    assert!(matches!(r, Err(MailError::Io(_))), "{r:?}");
    assert!(f.is_file());
    assert!(gone(&mail.join("kaaaaaaaa/cur")));
}

#[test] // R1-2: a read does not follow a link at mail/; the files it points to stay and no lock is made there
fn lifecycle_read_refuses_a_link_at_mail() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    stored(&p, id, &[("kaaaaaaaa", None)]);
    let mail = mail_dir(&p, id);
    let _ = fs::remove_dir_all(&mail);
    let target = t.join("outside");
    let f = outside_message(&target.join("kaaaaaaaa"));
    std::os::unix::fs::symlink(&target, &mail).unwrap();
    let r = mail_store::read(&mail, "kaaaaaaaa", 10);
    assert!(matches!(r, Err(MailError::Io(_))), "{r:?}");
    assert!(f.is_file());
    assert!(gone(&target.join("kaaaaaaaa/cur")));
    assert!(gone(&target.join("lock")));
}

#[test] // R1-2: a read does not follow a link at mail/<key>/cur; the files it points to stay
fn lifecycle_read_refuses_a_link_at_cur() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    stored(&p, id, &[("kaaaaaaaa", None)]);
    put(&p, id, "kaaaaaaaa", 1);
    let mail = mail_dir(&p, id);
    let outside = t.join("outside");
    let f = outside_message(&outside);
    fs::create_dir_all(outside.join("cur")).unwrap();
    fs::remove_dir(mail.join("kaaaaaaaa/cur")).unwrap();
    std::os::unix::fs::symlink(outside.join("cur"), mail.join("kaaaaaaaa/cur")).unwrap();
    let r = mail_store::read(&mail, "kaaaaaaaa", 10);
    assert!(matches!(r, Err(MailError::Io(_))), "{r:?}");
    assert!(f.is_file());
    assert_eq!(fs::read_dir(outside.join("cur")).unwrap().count(), 0);
    assert_eq!(unread(&p, id, "kaaaaaaaa"), 1);
}

#[test] // R1-5: a read that moves a message sets an existing cur/ to 0700 (M-C4)
fn lifecycle_read_sets_an_existing_cur_to_0700() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    stored(&p, id, &[("kaaaaaaaa", None)]);
    put(&p, id, "kaaaaaaaa", 1);
    let mail = mail_dir(&p, id);
    let cur = mail.join("kaaaaaaaa/cur");
    fs::set_permissions(&cur, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(mode(&cur), 0o755);
    assert_eq!(mail_store::read(&mail, "kaaaaaaaa", 10).unwrap().len(), 1);
    assert_eq!(mode(&cur), 0o700);
}

#[test] // F2: a read takes mail/lock shared: a removal that holds it exclusive makes the read Busy, and a removal
// cannot run while a read lists and moves
fn lifecycle_read_and_removal_exclude_each_other() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", None)]);
    put(&p, id, "kaaaaaaaa", 1);
    let mail = mail_dir(&p, id);

    let held = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(mail.join("lock")).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    let r = mail_store::read_within(&mail, "kaaaaaaaa", 10, Duration::ZERO);
    assert!(matches!(r, Err(MailError::Busy)), "{r:?}");
    assert_eq!(MailError::Busy.to_string(), "zygo: mail is busy; try again");
    assert_eq!(unread(&p, id, "kaaaaaaaa"), 1);
    assert_eq!(fs::read_dir(mail.join("kaaaaaaaa/cur")).unwrap().count(), 0);
    drop(held);

    // Under the read's lock, a removal cannot take mail/lock.
    let kept = Arc::new(Mutex::new(None));
    let (hk, hs) = (kept.clone(), Store::open(&p, id));
    up::testing::hook(
        store.dir(),
        "mail_read_before_dirs",
        Some(Box::new(move || {
            *hk.lock().unwrap() = Some(prune(&hs, Duration::ZERO).map_err(|e| e.kind()));
            Ok(())
        })),
    );
    let got = mail_store::read_within(&mail, "kaaaaaaaa", 10, Duration::ZERO);
    up::testing::hook(store.dir(), "mail_read_before_dirs", None);
    assert_eq!(got.unwrap().len(), 1);
    assert_eq!(*kept.lock().unwrap(), Some(Err(std::io::ErrorKind::WouldBlock)));

    // A read of a workspace with no mail/ takes no lock and makes nothing.
    let mail2 = mail_dir(&p, OTHER);
    assert_eq!(mail_store::read(&mail2, "kaaaaaaaa", 10).unwrap(), Vec::<Message>::new());
    assert!(gone(&mail2));
}

#[test] // F1: a read of a missing maildir or a missing new/ returns nothing and makes no folder
fn lifecycle_read_of_a_missing_maildir_makes_nothing() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let id = THIRD;
    stored(&p, id, &[("kaaaaaaaa", None)]);
    let mail = mail_dir(&p, id);
    fs::create_dir_all(&mail).unwrap();
    assert_eq!(mail_store::read(&mail, "kaaaaaaaa", 10).unwrap(), Vec::<Message>::new());
    assert!(gone(&mail.join("kaaaaaaaa")));
    fs::create_dir_all(mail.join("kaaaaaaaa")).unwrap();
    assert_eq!(mail_store::read(&mail, "kaaaaaaaa", 10).unwrap(), Vec::<Message>::new());
    assert!(gone(&mail.join("kaaaaaaaa/new")) && gone(&mail.join("kaaaaaaaa/cur")));
}

// ---- step 6: path b4 through `repair` ----

/// A tmux stand-in that answers `-V` with 3.4 and every other call with "no server" (as `tests/up.rs` `gone_tmux`).
fn gone_tmux(dir: &Path) -> PathBuf {
    let body = "for a in \"$@\"; do if [ \"$a\" = -V ]; then echo 'tmux 3.4'; exit 0; fi; done\n\
                echo 'no server running on /tmp/zygo-none/s' >&2\nexit 1";
    script(dir, "gtmux", body)
}

#[test] // b4: repair takes back a dead loadout build and removes the maildir of each fresh record it drops
fn lifecycle_repair_take_back_removes_the_maildir() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", None), ("kbbbbbbbb", None)]);
    store
        .update(S5, &mut |ws| {
            ws.phase = Phase::Building;
            ws.build_plan = Some(BuildPlan::Loadout);
            ws.fresh_members = vec!["kaaaaaaaa".into(), "kbbbbbbbb".into()];
            ws.members.get_mut("kbbbbbbbb").unwrap().agent_session = Some("c-1".into());
            Ok(())
        })
        .unwrap();
    put(&p, id, "kaaaaaaaa", 1);
    put(&p, id, "kbbbbbbbb", 1);
    let mux = Tmux::new(gone_tmux(t), None);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    repair::repair(&store, &mux, &t.join("zygo"), &mut out, &mut err).unwrap();
    assert_eq!((text(out), text(err)), (String::new(), String::new()));
    let ws = read(&store);
    assert_eq!(ws.phase, Phase::Down);
    assert_eq!(ws.members.keys().collect::<Vec<_>>(), ["kbbbbbbbb"]);
    assert!(gone(&mail_dir(&p, id).join("kaaaaaaaa")));
    assert_eq!(unread(&p, id, "kbbbbbbbb"), 1);
}

#[test] // F3: repair in phase up or down removes a maildir with no record that a failed removal left
fn lifecycle_repair_in_up_or_down_prunes_an_orphan_maildir() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", None)]);
    let mux = Tmux::new(gone_tmux(t), None);
    let mail = mail_dir(&p, id);
    for phase in [Phase::Up, Phase::Down] {
        set(&store, |ws| ws.phase = phase);
        put(&p, id, "kaaaaaaaa", 1);
        put(&p, id, "kzzzzzzzz", 1);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        repair::repair(&store, &mux, &t.join("zygo"), &mut out, &mut err).unwrap();
        assert_eq!((text(out), text(err)), (String::new(), String::new()), "{phase:?}");
        assert!(gone(&mail.join("kzzzzzzzz")), "{phase:?}");
        assert_eq!(read(&store).phase, phase);
        assert!(!gone(&mail.join("kaaaaaaaa")), "{phase:?}");
    }
}

#[test] // F3: the prune of repair in phase up is best effort: a held mail/lock leaves the orphan, with no line and no
// error, for the next removal
#[ignore = "timing: repair waits MAIL_LOCK_WAIT (2 s) for mail/lock"]
fn lifecycle_repair_in_up_leaves_an_orphan_while_mail_lock_is_held() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let id = THIRD;
    let store = stored(&p, id, &[("kaaaaaaaa", None)]);
    let mux = Tmux::new(gone_tmux(t), None);
    put(&p, id, "kzzzzzzzz", 1);
    let held = Delivery::begin(&store, MAIL_LOCK_WAIT).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    repair::repair(&store, &mux, &t.join("zygo"), &mut out, &mut err).unwrap();
    assert_eq!((text(out), text(err)), (String::new(), String::new()));
    assert_eq!(unread(&p, id, "kzzzzzzzz"), 1);
    drop(held);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    repair::repair(&store, &mux, &t.join("zygo"), &mut out, &mut err).unwrap();
    assert!(gone(&mail_dir(&p, id).join("kzzzzzzzz")));
}

// ---- steps 7 and 8: `up` (copied from tests/up.rs) ----

/// The zygo stand-in of `tests/up.rs` (D25): `ws run` copies the state to `<dir>/runs/<key>.json`, waits `delay`,
/// changes to the pane's cwd read from that copy and runs `sleep 600`; every other call exits 0.
fn stand_in(dir: &Path, state: &Path, delay: &str) -> PathBuf {
    let runs = dir.join("runs");
    let body = format!(
        "if [ \"$1\" = ws ] && [ \"$2\" = run ]; then\n\
         mkdir -p '{runs}'\ncp '{state}' '{runs}'/\"$6\".json\nsleep {delay}\n\
         d=$(awk -v k=\"\\\"$6\\\"\" '/\"key\": / {{ f = index($0, k) > 0 }} f && /\"cwd\": / {{ sub(/.*\"cwd\": \"/, \"\"); sub(/\",?$/, \"\"); print; exit }}' '{runs}'/\"$6\".json)\n\
         cd \"$d\"\nexec sleep 600\nfi\nexit 0",
        runs = runs.display(),
        state = state.display()
    );
    script(dir, "zygo", &body)
}

fn opts(t: &Path, cwd: &Path, tmux: &Path, zygo: &Path) -> UpOpts {
    let mut vars = BTreeMap::new();
    vars.insert(OsString::from("PATH"), std::env::var_os("PATH").unwrap_or_default());
    vars.insert(OsString::from("HOME"), t.join("home").into_os_string());
    UpOpts {
        name: None,
        loadout: None,
        reset: false,
        no_attach: true,
        cwd: cwd.to_path_buf(),
        zygo: zygo.to_path_buf(),
        tmux: tmux.to_path_buf(),
        tmux_env: None,
        zygo_session: None,
        vars,
        paths: paths(t),
        config: Config::default(),
        default_loadout_set: false,
        git: None,
    }
}

fn run(o: &UpOpts) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::run_with(o, &mut out, &mut err);
    (code, text(out), text(err))
}

fn win(index: u32, name: Option<&str>, layout: &str, width: u32, height: u32) -> Window {
    Window { index, name: name.map(String::from), layout: layout.into(), width, height, ..Window::default() }
}

/// A saved state of the workspace of `repo`: name `app`, phase `down`, loadout `solo`, and the three lists.
fn saved(store: &Store, repo: &Path, windows: Vec<Window>, panes: Vec<Pane>, members: Vec<(&str, MemberRecord)>) {
    let common = canon(&repo.join(".git"));
    store
        .update(S5, &mut |ws| {
            ws.name = "app".into();
            ws.tmux_session = "zygo-app".into();
            ws.common_dir = common.clone();
            ws.main_root = repo.to_path_buf();
            ws.loadout = "solo".into();
            ws.phase = Phase::Down;
            ws.windows = windows.clone();
            ws.panes = panes.clone();
            ws.members = members.iter().map(|(k, m)| (k.to_string(), m.clone())).collect();
            Ok(())
        })
        .unwrap();
}

fn set(store: &Store, f: impl Fn(&mut Workspace)) {
    store
        .update(S5, &mut |ws| {
            f(ws);
            Ok(())
        })
        .unwrap();
}

fn lp(kind: &str) -> LoadoutPane { LoadoutPane { kind: kind.into(), ..LoadoutPane::default() } }

fn lw(name: Option<&str>, layout: Option<&str>, pane: Vec<LoadoutPane>) -> LoadoutWindow {
    LoadoutWindow { name: name.map(String::from), layout: layout.map(String::from), pane }
}

fn with_loadout(name: &str, window: Vec<LoadoutWindow>) -> Config {
    let mut c = Config::default();
    c.loadout.insert(name.into(), Loadout { window });
    c
}

/// `pair`: window `agents` (main-vertical; claude-code and a shell on `branch:feature-x`) and window `logs` (a shell).
fn pair() -> Config {
    let shell = LoadoutPane { cwd: Some("branch:feature-x".into()), ..lp("shell") };
    with_loadout("pair", vec![lw(Some("agents"), Some("main-vertical"), vec![lp("claude-code"), shell]), lw(Some("logs"), None, vec![lp("shell")])])
}

struct Live {
    t: TempDir,
    srv: PrivateServer,
    repo: PathBuf,
    id: String,
    o: UpOpts,
}

impl Live {
    fn store(&self) -> Store { Store::open(&self.o.paths, &self.id) }
}

/// Repository `app` with worktree `../wt-x` on the new branch `feature-x`, a private server and the stand-in zygo.
fn live(delay: &str) -> Live {
    let t = TempDir::new().unwrap();
    let srv = PrivateServer::start();
    let repo = repo(t.path(), "app");
    git(&repo, &["worktree", "add", "-q", "-b", "feature-x", "../wt-x"]);
    let id = workspace_id(&canon(&repo.join(".git")));
    let p = paths(t.path());
    let state = p.workspaces_dir().join(&id).join("workspace.json");
    let zygo = stand_in(t.path(), &state, delay);
    let o = opts(t.path(), &repo, srv.wrapper(), &zygo);
    Live { t, srv, repo, id, o }
}

/// A tmux wrapper as `tests/up.rs` `socket_only_tmux` whose `new-session` exits 1 with `injected`: `fail` makes no
/// session.
fn failing_new_session(dir: &Path, real: &Path, mode: &str) -> PathBuf {
    let r = real.display();
    let body = format!(
        "sock=; prev=; for a in \"$@\"; do [ \"$prev\" = -S ] && sock=$a; prev=$a; done\n\
         case \"$*\" in *new-session*)\n\
         case {mode} in make) '{r}' -f /dev/null \"$@\" ;; foreign) '{r}' -f /dev/null -S \"$sock\" new-session -d -s zygo-app sleep 600 ;; esac\n\
         echo injected >&2; exit 1 ;;\n\
         esac\n\
         for a in \"$@\"; do case \"$a\" in -S|-V) exec '{r}' -f /dev/null \"$@\";; esac; done\n\
         echo 'no server running on /tmp/zygo-none/s' >&2\nexit 1"
    );
    script(dir, &format!("ftmux-{mode}"), &body)
}

/// `$TMUX` of a client of the private server.
fn inside(l: &Live) -> Option<OsString> { Some(OsString::from(format!("{},1,0", l.srv.socket().display()))) }

/// The names in `mail/` of workspace `id` that are pane keys.
fn maildirs(p: &Paths, id: &str) -> Vec<String> {
    let Ok(rd) = fs::read_dir(mail_dir(p, id)) else { return Vec::new() };
    let mut v: Vec<String> =
        rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|k| valid_pane_key(k)).collect();
    v.sort();
    v
}

/// A send from a hook: reads the state, sends to the `fresh_members` key whose record is claude-code, and keeps that
/// key and the number of messages sent.
fn send_to_fresh(p: &Paths, store: &Store, kept: &Mutex<Option<(String, usize)>>) {
    let ws = read(store);
    let key = ws
        .fresh_members
        .iter()
        .find(|k| ws.members.get(*k).is_some_and(|m| m.launch.kind == Agent::ClaudeCode))
        .cloned()
        .expect("a fresh claude-code record");
    let r = send(p, &ws, None, &key, "s", "b", &MailConfig::default(), SystemTime::now()).unwrap();
    *kept.lock().unwrap() = Some((key, r.sent.len()));
}

#[test] // b1: `up --reset` drops every record, and their maildirs go after `phase = up`
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 2 s cwd wait"]
fn lifecycle_up_reset_removes_the_dropped_maildirs() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    saved(
        &store,
        &l.repo,
        vec![win(0, None, "tiled", 200, 50)],
        vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo)],
        vec![
            ("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"))),
            ("kbbbbbbbb", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2"))),
            ("kcccccccc", member(Agent::ClaudeCode, MemberState::Ended, Some("c-3"))),
        ],
    );
    for k in ["kaaaaaaaa", "kbbbbbbbb", "kcccccccc"] {
        put(&l.o.paths, &l.id, k, 1);
    }
    l.o.reset = true;
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    let ws = read(&store);
    assert_eq!(ws.members.len(), 1);
    for k in ["kaaaaaaaa", "kbbbbbbbb", "kcccccccc"] {
        assert!(gone(&mail_dir(&l.o.paths, &l.id).join(k)), "{k}");
    }
    assert!(maildirs(&l.o.paths, &l.id).iter().all(|k| ws.members.contains_key(k)), "{:?}", maildirs(&l.o.paths, &l.id));
}

#[test] // F3, R1-4: `up` on a running session removes a maildir with no record that a failed removal left, through the
// best-effort prune of `repair` in phase `up` (step 5 runs it for every existing state; step 6 adds no prune)
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 2 s cwd wait"]
fn lifecycle_up_on_a_running_session_prunes_an_orphan_maildir() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    saved(
        &store,
        &l.repo,
        vec![win(0, None, "tiled", 200, 50)],
        vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo)],
        vec![("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1")))],
    );
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    assert_eq!(read(&store).phase, Phase::Up);
    put(&l.o.paths, &l.id, "kzzzzzzzz", 1);
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(gone(&mail_dir(&l.o.paths, &l.id).join("kzzzzzzzz")));
    assert!(read(&store).members.contains_key("kaaaaaaaa"));
}

#[test] // b2: a restore drops a bare record and its maildir; a failed restore's undo puts the record back and keeps it
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 2 s cwd wait"]
fn lifecycle_restore_forget_removes_the_maildir_and_undo_keeps_it() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    let mut pend = member(Agent::ClaudeCode, MemberState::Running, None);
    pend.pending =
        Some(Pending { zygo_session: "01DEAD".into(), agent_session: Some("p-1".into()), started_at: 1, ..Pending::default() });
    let mut bare = member(Agent::ClaudeCode, MemberState::Running, None);
    bare.launch.profile = Some("fast".into());
    saved(
        &store,
        &l.repo,
        vec![win(0, None, "tiled", 200, 50)],
        vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo), pane("kbbbbbbbb", 0, 1, PaneKind::ClaudeCode, &l.repo)],
        vec![("kaaaaaaaa", bare.clone()), ("kbbbbbbbb", pend.clone())],
    );
    let sock = l.srv.socket().to_path_buf();
    set(&store, move |w| w.tmux_socket = Some(sock.clone()));
    put(&l.o.paths, &l.id, "kaaaaaaaa", 1);
    put(&l.o.paths, &l.id, "kbbbbbbbb", 1);
    let mut o = l.o.clone();
    o.tmux = failing_new_session(l.t.path(), &real, "fail");
    let (code, _, err) = run(&o);
    assert_eq!(code, 1, "{err}");
    assert_eq!(read(&store).members["kaaaaaaaa"], bare);
    assert_eq!(unread(&l.o.paths, &l.id, "kaaaaaaaa"), 1);
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!read(&store).members.contains_key("kaaaaaaaa"), "{err}");
    assert!(gone(&mail_dir(&l.o.paths, &l.id).join("kaaaaaaaa")));
    assert_eq!(unread(&l.o.paths, &l.id, "kbbbbbbbb"), 1);
}

#[test] // b3: a fresh member whose split failed has no record and no maildir, also after a send during the build
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 2 s cwd wait"]
fn lifecycle_fresh_member_not_built_has_no_maildir() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.config = with_loadout("duo", vec![lw(None, None, vec![lp("shell"), lp("claude-code")])]);
    l.o.loadout = Some("duo".into());
    fs::create_dir_all(store.dir()).unwrap();
    let kept: Arc<Mutex<Option<(String, usize)>>> = Arc::new(Mutex::new(None));
    let calls = AtomicUsize::new(0);
    let (hp, hs, hk) = (l.o.paths.clone(), store.clone(), Arc::clone(&kept));
    up::testing::hook(
        store.dir(),
        "split",
        Some(Box::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                send_to_fresh(&hp, &hs, &hk);
                Err("no space for new pane".into())
            } else {
                Ok(())
            }
        })),
    );
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "split", None);
    assert_eq!(code, 0, "{err}");
    let (key, sent) = kept.lock().unwrap().take().expect("the split hook ran");
    assert_eq!(sent, 1);
    let ws = read(&store);
    assert!(ws.members.is_empty(), "{ws:?}");
    assert!(gone(&mail_dir(&l.o.paths, &l.id).join(&key)), "{key}");
}

#[test] // b4: the `up` undo takes back the plan; the maildir of a fresh record it drops goes too
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 2 s cwd wait"]
fn lifecycle_up_undo_take_back_removes_the_maildir() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    l.o.tmux_env = inside(&l);
    l.o.tmux = failing_new_session(l.t.path(), &real, "fail");
    fs::create_dir_all(store.dir()).unwrap();
    let kept: Arc<Mutex<Option<(String, usize)>>> = Arc::new(Mutex::new(None));
    let (hp, hs, hk) = (l.o.paths.clone(), store.clone(), Arc::clone(&kept));
    up::testing::hook(
        store.dir(),
        "before_undo",
        Some(Box::new(move || {
            send_to_fresh(&hp, &hs, &hk);
            Ok(())
        })),
    );
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "before_undo", None);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("injected"), "{err}");
    let (key, sent) = kept.lock().unwrap().take().expect("the before_undo hook ran");
    assert_eq!(sent, 1);
    let ws = read(&store);
    assert!(ws.members.is_empty(), "{ws:?}");
    assert!(gone(&mail_dir(&l.o.paths, &l.id).join(&key)), "{key}");
}

// ---- step 4: path a3 (`forget-pane`) ----

#[test] // a3: forget-pane waits for mail/lock while a delivery holds it, leaves the maildir, and the next run removes it
#[ignore = "timing: forget-pane waits MAIL_LOCK_WAIT (2 s) for mail/lock"]
fn lifecycle_removal_waits_for_a_delivery_in_flight() {
    let _s = serial();
    let PaneSetup { _td, p, tmux, id, store, plain } = pane_setup();
    put(&p, &id, "kbbbbbbbb", 1);
    let kept: Arc<Mutex<Option<(i32, String, String)>>> = Arc::new(Mutex::new(None));
    let once = AtomicBool::new(false);
    let (hp, ht, hc, hk) = (p.clone(), tmux.clone(), plain.clone(), Arc::clone(&kept));
    up::testing::hook(
        store.dir(),
        "mail_delivery_after_read",
        Some(Box::new(move || {
            if !once.swap(true, Ordering::SeqCst) {
                *hk.lock().unwrap() = Some(pane_run(&hp, &ht, Some("app"), &hc, "kbbbbbbbb"));
            }
            Ok(())
        })),
    );
    let r = send(&p, &read(&store), None, "kbbbbbbbb", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    up::testing::hook(store.dir(), "mail_delivery_after_read", None);
    assert_eq!(r.sent.len(), 1, "{r:?}");
    let mail = mail_dir(&p, &id);
    assert_eq!(
        kept.lock().unwrap().take().unwrap(),
        (
            1,
            String::new(),
            format!("zygo: cannot forget pane kbbbbbbbb of app: {}/lock is held by another process\n", mail.display())
        )
    );
    assert!(!read(&store).members.contains_key("kbbbbbbbb"));
    assert_eq!(unread(&p, &id, "kbbbbbbbb"), 2);
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(gone(&mail.join("kbbbbbbbb")));
}
