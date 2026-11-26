//! FX-13: member record and maildir lifecycle (M§5 Retention, P1).
//!
//! Every test and helper is `cfg(unix)`: the tmux stand-ins are shell scripts and the tests check file modes. Every
//! test takes `serial()`, so no test writes a script while another test forks (ETXTBSY).
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use zygo_config::MailConfig;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::admin;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::mail::{
    mail_dir, prune, remove_maildir, send, store as mail_store, Delivery, MailError, Message, MAIL_LOCK_WAIT,
};
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind, Phase, Store, Workspace};

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
