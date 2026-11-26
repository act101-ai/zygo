#![cfg(unix)]
//! ML-4: address resolution and the send operation (M§5 send and `all`, M§6, M§8 `to` and sender, M§10
//! "Target name unknown", M-C2).
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use zygo_config::MailConfig;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::mail::names::{members, resolve, sender, Target};
use zygo_workspace::mail::{mail_dir, send, MailError, Message, Refused, Sent, SendReport, BODY_MAX, SUBJECT_MAX};
use zygo_workspace::state::{MemberRecord, Phase, Store, Workspace};

const ID: &str = "0123456789abcdef";
const LEAD: &str = "klead2222";
const ELENA: &str = "kelena222";
const ANON: &str = "kanon2222";
const BAD: &str = "kbad22222";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;

fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}
fn t(key: &str, name: Option<&str>) -> Target { Target { key: key.into(), name: name.map(String::from) } }
/// Identity fields as tests/state.rs:20-24, so `Store::update` accepts it. `BAD` stores a name that fails AU84.
fn ws() -> Workspace {
    let mut ws = Workspace::new(ID);
    ws.name = "app".into(); ws.common_dir = "/srv/app/.git".into(); ws.main_root = "/srv/app".into();
    ws.tmux_session = "zygo-app".into(); ws.loadout = "solo".into(); ws.phase = Phase::Up;
    for (k, n) in [(LEAD, Some("lead")), (ELENA, Some("elena")), (ANON, None), (BAD, Some("Bad_Name"))] {
        let mut m = MemberRecord::new(Agent::ClaudeCode);
        m.name = n.map(String::from);
        ws.members.insert(k.into(), m);
    }
    ws
}
/// Writes `w` as the state of workspace `ID` and returns it: `send` reads the member records from the state under
/// `mail/lock` (M§5 P1, FX-13), so the fixture is stored before each send.
fn stored_as(p: &Paths, w: Workspace) -> Workspace {
    Store::open(p, ID).update(Duration::from_secs(5), &mut |s| { *s = w.clone(); Ok(()) }).unwrap();
    w
}
/// [`stored_as`] with [`ws`].
fn stored(p: &Paths) -> Workspace { stored_as(p, ws()) }
/// The names in `mail/`: a send takes `mail/lock` before it resolves `to`, so `mail/` holds `lock` and no maildir
/// when nothing was delivered.
fn mail_entries(p: &Paths) -> Vec<String> {
    let mut v: Vec<String> =
        fs::read_dir(mail_dir(p, ID)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}
/// Each message file in `mail/<key>/new/`, parsed; empty when the folder is missing.
fn new_files(paths: &Paths, key: &str) -> Vec<Message> {
    let dir = mail_dir(paths, ID).join(key).join("new");
    let Ok(rd) = fs::read_dir(&dir) else { return Vec::new() };
    rd.map(|e| serde_json::from_slice(&fs::read(e.unwrap().path()).unwrap()).unwrap()).collect()
}
/// Every regular file under `dir`, at any depth.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { out.extend(files(&p)) } else { out.push(p) }
    }
    out
}
fn holds(p: &Path, marker: &str) -> bool { String::from_utf8_lossy(&fs::read(p).unwrap_or_default()).contains(marker) }

#[test]
fn members_lists_every_member_in_key_order() {
    assert_eq!(members(&ws()), vec![t(ANON, None), t(BAD, None), t(ELENA, Some("elena")), t(LEAD, Some("lead"))]);
}

#[test]
fn resolve_by_name() {
    assert_eq!(resolve(&ws(), "elena", Some(LEAD)).unwrap(), vec![t(ELENA, Some("elena"))]);
}

#[test]
fn resolve_by_pane_key() {
    assert_eq!(resolve(&ws(), ANON, Some(LEAD)).unwrap(), vec![t(ANON, None)]);
    assert_eq!(resolve(&ws(), ELENA, None).unwrap(), vec![t(ELENA, Some("elena"))]);
}

#[test]
fn resolve_all_excludes_the_sender() {
    assert_eq!(resolve(&ws(), "all", Some(LEAD)).unwrap(), vec![t(ANON, None), t(BAD, None), t(ELENA, Some("elena"))]);
    assert_eq!(resolve(&ws(), "all", None).unwrap(), members(&ws()));
}

#[test]
fn unknown_target_names_each_known_name() {
    for to in ["bob", "Bad_Name", "kzzzzzzzz", "", "cli", "zygo", "elena@host"] {
        let e = resolve(&ws(), to, Some(LEAD)).unwrap_err();
        assert!(matches!(&e, MailError::UnknownTarget { known } if known == &["elena", "lead"]), "{to}: {e:?}");
        assert_eq!(e.to_string(), "zygo: unknown target; known names: elena, lead");
    }
    let mut w = ws();
    for m in w.members.values_mut() {
        m.name = None;
    }
    let e = resolve(&w, "elena", None).unwrap_err();
    assert!(matches!(&e, MailError::UnknownTarget { known } if known.is_empty()));
    assert_eq!(e.to_string(), "zygo: unknown target; known names: none");
}

#[test]
fn stored_name_that_fails_au84_is_not_matched() {
    let mut w = ws();
    w.members.get_mut(BAD).unwrap().name = Some("kelena222".into());
    assert_eq!(resolve(&w, "kelena222", None).unwrap(), vec![t(ELENA, Some("elena"))]);
    w.members.get_mut(BAD).unwrap().name = Some("cli".into());
    let e = resolve(&w, "cli", None).unwrap_err();
    assert!(matches!(&e, MailError::UnknownTarget { known } if known == &["elena", "lead"]), "{e:?}");
    assert_eq!(resolve(&w, BAD, None).unwrap(), vec![t(BAD, None)]);
}

#[test]
fn sender_is_name_then_key_then_cli() {
    let w = ws();
    assert_eq!(sender(&w, Some(LEAD)), "lead");
    assert_eq!(sender(&w, Some(ANON)), ANON);
    assert_eq!(sender(&w, Some(BAD)), BAD);
    assert_eq!(sender(&w, None), "cli");
    assert_eq!(sender(&w, Some("kzzzzzzzz")), "cli");
}

#[test]
fn send_by_name_writes_one_file() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let r = send(&p, &stored(&p), Some(LEAD), "elena", "Plan", "Body\n", &MailConfig::default(), at(T0)).unwrap();
    assert!(r.refused.is_empty(), "{r:?}");
    let sent = r.sent;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].key, ELENA);
    let got = new_files(&p, ELENA);
    assert_eq!(got.len(), 1);
    let m = &got[0];
    assert_eq!(sent[0], Sent { id: m.id.clone(), key: ELENA.into() });
    assert_eq!((m.from.as_str(), m.to.as_str(), m.subject.as_str(), m.body.as_str()), ("lead", "elena", "Plan", "Body\n"));
}

#[test]
fn send_to_a_member_with_no_name() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let sent = send(&p, &stored(&p), None, ANON, "s", "b", &MailConfig::default(), at(T0)).unwrap().sent;
    assert_eq!(sent.len(), 1);
    let got = new_files(&p, ANON);
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].from.as_str(), got[0].to.as_str()), ("cli", ANON));
    assert_eq!(got[0].id, sent[0].id);
}

#[test]
fn send_to_all_writes_one_file_per_other_member() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let sent = send(&p, &stored(&p), Some(LEAD), "all", "s", "b", &MailConfig::default(), at(T0)).unwrap().sent;
    assert_eq!(sent.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), [ANON, BAD, ELENA]);
    assert_eq!(sent.iter().map(|s| &s.id).collect::<BTreeSet<_>>().len(), 3);
    for (s, to) in sent.iter().zip([ANON, BAD, "elena"]) {
        let got = new_files(&p, &s.key);
        assert_eq!(got.len(), 1, "{}", s.key);
        assert_eq!(got[0].to, to);
        assert_eq!(got[0].id, s.id);
        assert_eq!(got[0].from, "lead");
    }
    assert!(!mail_dir(&p, ID).join(LEAD).exists());
}

#[test]
fn send_unknown_target_writes_nothing() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let r = send(&p, &stored(&p), Some(LEAD), "bob", "s", "b", &MailConfig::default(), at(T0));
    assert!(matches!(r, Err(MailError::UnknownTarget { .. })), "{r:?}");
    assert_eq!(mail_entries(&p), ["lock"]);
}

#[test]
fn send_full_names_the_target() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let cfg = MailConfig { max_unread: 1, ..MailConfig::default() };
    send(&p, &stored(&p), Some(LEAD), "elena", "s", "b", &cfg, at(T0)).unwrap();
    let r = send(&p, &stored(&p), Some(LEAD), "elena", "s", "b", &cfg, at(T0 + 1)).unwrap();
    assert!(r.sent.is_empty(), "{r:?}");
    assert_eq!(r.refused.len(), 1);
    assert_eq!(r.refused[0].key, ELENA);
    assert_eq!(r.refused[0].error.to_string(), "zygo: elena has 1 unread messages");
}

#[test]
fn send_writes_no_content_outside_mail() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let store = Store::open(&p, ID);
    stored(&p);
    store.log_error("x");
    let cfg = MailConfig { max_unread: 1, ..MailConfig::default() };
    let (subj, body) = ("subj-7f3a", "body-9c1e");
    send(&p, &stored(&p), Some(LEAD), "all", subj, body, &cfg, at(T0)).unwrap();
    assert!(matches!(send(&p, &stored(&p), Some(LEAD), "bob", subj, body, &cfg, at(T0 + 1)), Err(MailError::UnknownTarget { .. })));
    let r = send(&p, &stored(&p), Some(LEAD), "elena", subj, body, &cfg, at(T0 + 2)).unwrap();
    assert!(matches!(&r.refused[..], [Refused { error: MailError::Full { .. }, .. }]), "{r:?}");
    for f in ["workspace.json", "errors.log"] {
        let path = store.dir().join(f);
        assert!(path.is_file(), "{f}");
        assert!(!holds(&path, subj) && !holds(&path, body), "{f}");
    }
    let mail = mail_dir(&p, ID);
    for f in files(store.dir()) {
        if holds(&f, subj) || holds(&f, body) {
            assert!(f.starts_with(&mail), "{}", f.display());
        }
    }
}

/// Keys of a report's `sent` and `refused`, in order.
fn keys(r: &SendReport) -> (Vec<&str>, Vec<&str>) {
    (r.sent.iter().map(|s| s.key.as_str()).collect(), r.refused.iter().map(|f| f.key.as_str()).collect())
}

#[test]
fn send_to_all_delivers_past_a_full_target() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let cfg = MailConfig { max_unread: 1, ..MailConfig::default() };
    send(&p, &stored(&p), Some(LEAD), BAD, "s", "b", &cfg, at(T0)).unwrap();
    let r = send(&p, &stored(&p), Some(LEAD), "all", "s", "b", &cfg, at(T0 + 1)).unwrap();
    assert_eq!(keys(&r), (vec![ANON, ELENA], vec![BAD]), "{r:?}");
    assert!(matches!(&r.refused[0].error, MailError::Full { max: 1, .. }), "{r:?}");
    for s in &r.sent {
        let got = new_files(&p, &s.key);
        assert_eq!(got.len(), 1, "{}", s.key);
        assert_eq!(got[0].id, s.id);
    }
    assert_eq!(new_files(&p, BAD).len(), 1);
}

#[test]
fn send_to_a_shared_name_reaches_each_holder() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let mut w = ws();
    w.members.get_mut(ANON).unwrap().name = Some("elena".into());
    let w = stored_as(&p, w);
    let r = send(&p, &w, Some(LEAD), "elena", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    assert_eq!(keys(&r), (vec![ANON, ELENA], vec![]), "{r:?}");
    for s in &r.sent {
        let got = new_files(&p, &s.key);
        assert_eq!(got.len(), 1, "{}", s.key);
        assert_eq!((got[0].id.as_str(), got[0].to.as_str()), (s.id.as_str(), "elena"));
    }
}

#[test]
fn send_to_own_name_reaches_the_sender() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let r = send(&p, &stored(&p), Some(LEAD), "lead", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    assert_eq!(keys(&r), (vec![LEAD], vec![]), "{r:?}");
    let got = new_files(&p, LEAD);
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].from.as_str(), got[0].to.as_str()), ("lead", "lead"));
}

#[test]
fn send_over_the_limits_writes_nothing() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let long = "s".repeat(SUBJECT_MAX + 1);
    let big = "b".repeat(BODY_MAX + 1);
    let r = send(&p, &stored(&p), Some(LEAD), "all", &long, "b", &MailConfig::default(), at(T0));
    assert!(matches!(r, Err(MailError::SubjectTooLong)), "{r:?}");
    let r = send(&p, &stored(&p), Some(LEAD), "elena", "s", &big, &MailConfig::default(), at(T0));
    assert!(matches!(r, Err(MailError::BodyTooLarge)), "{r:?}");
    assert!(!mail_dir(&p, ID).exists());
}

#[test]
fn send_to_all_with_only_the_sender() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let mut w = ws();
    w.members.retain(|k, _| k == LEAD);
    let w = stored_as(&p, w);
    let r = send(&p, &w, Some(LEAD), "all", "s", "b", &MailConfig::default(), at(T0)).unwrap();
    assert_eq!(keys(&r), (vec![], vec![]), "{r:?}");
    let long = "s".repeat(SUBJECT_MAX + 1);
    let r = send(&p, &w, Some(LEAD), "all", &long, "b", &MailConfig::default(), at(T0));
    assert!(matches!(r, Err(MailError::SubjectTooLong)), "{r:?}");
    assert_eq!(mail_entries(&p), ["lock"]);
}
