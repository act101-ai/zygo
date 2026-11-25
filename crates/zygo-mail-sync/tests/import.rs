//! MS-12: import (mail sync design S§8.5, S§6.2, S-C5, S-C8; S§19 #12, #13, #14, #15). No test depends on wall-clock time.
#![cfg(unix)]

mod common;

use common::*;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use ulid::Ulid;
use zygo_core::paths::Paths;
use zygo_mail_sync::branch::ensure_worktree;
use zygo_mail_sync::env::{Crash, NoSeam, Seam};
use zygo_mail_sync::import::*;
use zygo_mail_sync::state::{imported_append, imported_load, SyncPaths};

const KEY: &str = "kbbbbbbbb"; // bob's pane key
fn u(n: u64) -> Ulid { Ulid::from_parts(1_764_000_000_000 + n, n as u128) }
fn file(id: Ulid) -> String { format!("{id}.json") }
/// A valid message from alice@a.
fn msg(id: Ulid, to: &str) -> Vec<u8> {
    format!(r#"{{"version":1,"id":"{id}","from":"alice@a","to":"{to}","sent_at":"2026-11-25T10:00:00Z","subject":"s","body":"b"}}"#).into_bytes()
}
/// Odd key order, whitespace, an unknown key and a final newline.
fn odd(id: Ulid) -> Vec<u8> {
    format!("{{ \"body\" : \"b\",\n  \"to\":\"bob@b\", \"x-extra\": [1, {{\"k\": null}}],\t\"version\": 1, \"subject\":\"s\",\"id\":\"{id}\",\"from\":\"alice@a\",\"sent_at\":\"2026-11-25T10:00:00Z\" }}\n").into_bytes()
}
/// check_message for a 100644 blob under bob@b named by `id`, with its real size.
fn check(id: Ulid, bytes: &[u8]) -> Result<(), &'static str> {
    check_message("bob@b", &file(id), "100644", "blob", bytes.len() as u64, bytes)
}
/// msg(u(1), "bob@b") with `field` set to `value` (serde_json in the test only).
fn with(field: &str, value: serde_json::Value) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_slice(&msg(u(1), "bob@b")).unwrap();
    v[field] = value;
    serde_json::to_vec(&v).unwrap()
}

#[test]
fn constants_have_the_spec_values() {
    assert_eq!(CANDIDATE_CAP, 500);
    assert_eq!(MESSAGE_MAX, 8388608);
    assert_eq!(MESSAGE_MAX, zygo_workspace::mail::store::FILE_MAX);
}

#[test]
fn check_message_accepts_any_key_order_and_unknown_keys() {
    assert_eq!(check(u(1), &msg(u(1), "bob@b")), Ok(()));
    assert_eq!(check(u(1), &odd(u(1))), Ok(()));
}

#[test]
fn check_message_names_the_first_failed_check() {
    let m = msg(u(1), "bob@b");
    let f = file(u(1));
    let n = m.len() as u64;
    assert_eq!(check_message("bob@b", &f, "120000", "blob", n, &m), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "160000", "commit", u64::MAX, b""), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "040000", "tree", u64::MAX, b""), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "100755", "blob", n, &m), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "100644", "tree", n, &m), Err("mode"));
    assert_eq!(check_message("bob@b", &f, "120000", "blob", n, b"{bad"), Err("mode")); // first failure wins
    for bad in ["..", "x.json", "01arz3ndektsv4rrffq69g5fav.json", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "../01ARZ3NDEKTSV4RRFFQ69G5FAV.json"] {
        assert_eq!(check_message("bob@b", bad, "100644", "blob", n, &m), Err("name"), "{bad:?}");
    }
    assert_eq!(check_message("bob@b", &f, "100644", "blob", 8388609, &m), Err("size"));
    assert_eq!(check_message("bob@b", &f, "100644", "blob", 8388608, &m), Ok(()));
    for bad in [&b"{bad"[..], b"", b"[1]", b"null",
        br#"[1,"01ARZ3NDEKTSV4RRFFQ69G5FAV","alice@a","bob@b","2026-11-25T10:00:00Z","s","b"]"#] {
        assert_eq!(check(u(1), bad), Err("json"), "{:?}", String::from_utf8_lossy(bad));
    }
    let mut missing: serde_json::Value = serde_json::from_slice(&m).unwrap();
    missing.as_object_mut().unwrap().remove("body");
    assert_eq!(check(u(1), &serde_json::to_vec(&missing).unwrap()), Err("json"));
    let dup = format!(r#"{{"version":1,"id":"{}","id":"{}","from":"alice@a","to":"bob@b","sent_at":"2026-11-25T10:00:00Z","subject":"s","body":"b"}}"#, u(1), u(1));
    assert_eq!(check(u(1), dup.as_bytes()), Err("json"));
    assert_eq!(check(u(1), &with("version", 2.into())), Err("version"));
    assert_eq!(check(u(1), &with("version", "1".into())), Err("json"));
    assert_eq!(check(u(1), &with("id", u(2).to_string().into())), Err("id"));
    assert_eq!(check(u(1), &with("to", "eve@b".into())), Err("to"));
    assert_eq!(check(u(1), &with("to", "bob".into())), Err("to"));
    for bad in ["alice", "cli@a", "zygo@a", "alice@A", "@a", "alice@"] {
        assert_eq!(check(u(1), &with("from", bad.into())), Err("from"), "{bad}");
    }
    assert_eq!(check(u(1), &with("sent_at", "yesterday".into())), Err("sent-at"));
}

#[test]
fn check_message_limits_are_bytes_not_characters() {
    assert_eq!(check(u(1), &with("subject", "a".repeat(200).into())), Ok(()));
    assert_eq!(check(u(1), &with("subject", "é".repeat(100).into())), Ok(())); // 200 bytes
    assert_eq!(check(u(1), &with("subject", "é".repeat(101).into())), Err("subject")); // 101 chars, 202 bytes
    assert_eq!(check(u(1), &with("body", "é".repeat(524288).into())), Ok(())); // 1048576 bytes
    assert_eq!(check(u(1), &with("body", format!("{}a", "é".repeat(524288)).into())), Err("body"));
}

#[test]
fn check_message_refuses_a_hostile_sent_at() {
    for good in ["2026-11-25T10:00:00Z", "2026-11-25t10:00:00z", "2026-11-25T10:00:00.123456789+05:30",
        "2024-02-29T23:59:60-00:00", "2000-02-29T00:00:00Z"] {
        assert_eq!(check(u(1), &with("sent_at", good.into())), Ok(()), "{good}");
    }
    for bad in ["", "2026-11-25T10:00:00", "2026-11-25 10:00:00Z", "2026-02-29T10:00:00Z", "1900-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z", "2026-00-10T00:00:00Z", "2026-11-31T00:00:00Z", "2026-11-00T00:00:00Z",
        "2026-11-25T24:00:00Z", "2026-11-25T10:60:00Z", "2026-11-25T10:00:61Z", "2026-11-25T10:00:00+24:00",
        "2026-11-25T10:00:00+05:60", "2026-11-25T10:00:00.Z", "+2026-11-25T10:00:00Z", "２026-11-25T10:00:00Z",
        "2026-11-25T10:00:00Z\n", "2026-11-25T10:00:00ZZ", "26-11-25T10:00:00Z"] {
        assert_eq!(check(u(1), &with("sent_at", bad.into())), Err("sent-at"), "{bad:?}");
    }
}

fn paths(p: &Path) -> Paths {
    Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    }
}
/// `sp` on a fresh TempDir (no git), with `mail/`, `mail/sync/` and `incoming/` made.
fn local() -> (tempfile::TempDir, SyncPaths) {
    let t = tempfile::TempDir::new().unwrap();
    let sp = SyncPaths::new(&paths(t.path()), WS_ID).unwrap();
    for d in [&sp.mail, &sp.sync, &sp.incoming] {
        zygo_workspace::mail::ensure_mail_root(d).unwrap();
    }
    (t, sp)
}
fn staged(sp: &SyncPaths, name: &str, bytes: &[u8]) {
    zygo_workspace::mail::store::write_synced(&sp.incoming.join(name), bytes).unwrap();
}
/// The blob ids of `all`, from one `git hash-object -w --stdin-paths` over files in a TempDir.
fn blobs(m: &Machine, all: &[Vec<u8>]) -> Vec<String> {
    let t = tempfile::TempDir::new().unwrap();
    let mut list = String::new();
    for (i, b) in all.iter().enumerate() {
        let p = t.path().join(i.to_string());
        fs::write(&p, b).unwrap();
        list.push_str(&format!("{}\n", p.display()));
    }
    git_in(&m.root, &["hash-object", "-w", "--stdin-paths"], list.as_bytes()).lines().map(String::from).collect()
}
/// The sync paths of machine `m` (its `Machine::new` folder), checked against `m.wt` and `mail_dir`.
fn sync_paths(m: &Machine) -> SyncPaths {
    let p = paths(m.root.parent().unwrap());
    let sp = SyncPaths::new(&p, WS_ID).unwrap();
    assert_eq!(sp.worktree, m.wt);
    assert_eq!(sp.mail, zygo_workspace::mail::mail_dir(&p, WS_ID));
    sp
}
fn new_dir(sp: &SyncPaths) -> PathBuf { sp.mail.join(KEY).join("new") }
fn bob() -> Vec<Target> { vec![Target { name: "bob".into(), pane_key: KEY.into() }] }
/// The tip whose tree is `ZYGO-MAIL` plus each `(folder, tree id)` (given in name order); the worktree points at it.
fn tip(m: &Machine, folders: &[(&str, String)]) -> String {
    let mark = blob(&m.root, b"1\n");
    let mut e: Vec<(&str, &[u8], &str)> = vec![("100644", b"ZYGO-MAIL", &mark)];
    for (name, t) in folders {
        e.push(("40000", name.as_bytes(), t));
    }
    let c = commit_tree(&m.root, &tree_literally(&m.root, &e), None);
    ensure_worktree(&m.root, &m.wt, &c, far()).unwrap();
    c
}
/// One folder tree of valid messages `msg(id, to)`.
fn folder(m: &Machine, to: &str, ids: &[Ulid]) -> String {
    let bs: Vec<(String, String)> = ids.iter().map(|id| (file(*id), blob(&m.root, &msg(*id, to)))).collect();
    tree_literally(&m.root, &bs.iter().map(|(n, b)| ("100644", n.as_bytes(), b.as_str())).collect::<Vec<_>>())
}
struct CrashAt(&'static str);
impl Seam for CrashAt {
    fn at(&self, p: &'static str) -> Result<(), Crash> { if p == self.0 { Err(Crash(p)) } else { Ok(()) } }
}
/// `run` with host list `hosts`, targets `bob()`, deadline `far()`; returns the result and the log lines.
fn go(m: &Machine, sp: &SyncPaths, tip: &str, hosts: &[&str], ack: &BTreeSet<Ulid>, max: u32, seam: &dyn Seam)
    -> (Result<Imported, Crash>, Vec<String>) {
    let hosts: Vec<String> = hosts.iter().map(|h| h.to_string()).collect();
    let mut lines = Vec::new();
    let r = run(sp, &sp.mail, &m.wt, tip, &hosts, &bob(), ack, max, far(), seam, &mut |l| lines.push(l.to_string()));
    (r, lines)
}
/// Every path under `dir`, links not followed.
fn all_files(dir: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if fs::symlink_metadata(&p).unwrap().is_dir() {
            out.extend(all_files(&p));
        }
        out.insert(p);
    }
    out
}
fn log_line(id: Ulid, check: &str) -> String { format!("mail sync: import {id}: {check}") }

#[test]
fn delivers_the_blob_bytes_unchanged() { // S§6.2, S§19 #1 delivery part
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let b = odd(u(1));
    let f = tree_literally(&m.root, &[("100644", file(u(1)).as_bytes(), &blob(&m.root, &b))]);
    let t = tip(&m, &[("bob@b", f)]);
    let (res, lines) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 5000, &NoSeam);
    assert_eq!(res, Ok(Imported { delivered: vec![(KEY.into(), u(1))], acked: vec![u(1)], left: 0 }));
    assert_eq!(fs::read(new_dir(&sp).join(file(u(1)))).unwrap(), b);
    assert_eq!(names(&new_dir(&sp)), [file(u(1))]);
    assert_eq!(names(&sp.incoming), Vec::<String>::new());
    assert_eq!(imported_load(&sp).unwrap(), HashSet::from([u(1)]));
    assert!(lines.is_empty());
}

#[test]
fn hostile_entries_are_acked_not_delivered() { // S§19 #12 delivery part, S-C5
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let ln = blob(&m.root, b"/etc/passwd");
    let big = blob(&m.root, &vec![b' '; 9 * 1024 * 1024]);
    let wrong_id = blob(&m.root, &msg(u(99), "bob@b"));
    let wrong_to = blob(&m.root, &msg(u(5), "eve@b"));
    let bad = blob(&m.root, b"{not json");
    let names_: Vec<String> = (1..=6).map(|n| file(u(n))).collect();
    let f = tree_literally(&m.root, &[("120000", names_[0].as_bytes(), &ln),
        ("160000", names_[1].as_bytes(), "0123456789abcdef0123456789abcdef01234567"),
        ("100644", names_[2].as_bytes(), &big), ("100644", names_[3].as_bytes(), &wrong_id),
        ("100644", names_[4].as_bytes(), &wrong_to), ("100644", names_[5].as_bytes(), &bad)]);
    let t = tip(&m, &[("bob@b", f)]);
    let before = all_files(m.root.parent().unwrap());
    let (res, lines) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 5000, &NoSeam);
    assert_eq!(res, Ok(Imported { delivered: vec![], acked: (1..=6).map(u).collect(), left: 0 }));
    assert_eq!(lines, [log_line(u(1), "mode"), log_line(u(2), "mode"), log_line(u(3), "size"),
        log_line(u(4), "id"), log_line(u(5), "to"), log_line(u(6), "json")]);
    assert!(!sp.mail.join(KEY).exists());
    for p in all_files(m.root.parent().unwrap()).difference(&before) { // a new parent of mail/sync/ is allowed
        assert!(p.starts_with(&sp.sync) || sp.sync.starts_with(p), "{p:?}");
    }
    assert_eq!(imported_load(&sp).unwrap(), (1..=6).map(u).collect::<HashSet<_>>());
}

#[test]
fn hostile_names_never_reach_a_path() { // review focus 8
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let ok = blob(&m.root, &msg(u(1), "bob@b"));
    let inner = tree_literally(&m.root, &[("100644", b"x", &ok)]);
    let long = [b'x'; 300];
    let f = tree_literally(&m.root, &[("100644", file(u(1)).as_bytes(), &ok), ("40000", file(u(2)).as_bytes(), &inner),
        ("100644", b"..", &ok), ("100644", b"a\tb", &ok), ("100644", b"a\nb", &ok), ("100644", &long, &ok),
        ("100644", b"\xff\xfe", &ok), ("100644", b"01arz3ndektsv4rrffq69g5fav.json", &ok)]);
    let t = tip(&m, &[("bob@b", f)]);
    let before = all_files(m.root.parent().unwrap());
    let (res, lines) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 5000, &NoSeam);
    assert_eq!(res, Ok(Imported { delivered: vec![(KEY.into(), u(1))], acked: vec![u(1), u(2)], left: 0 }));
    assert_eq!(lines, [log_line(u(2), "mode")]);
    assert_eq!(names(&new_dir(&sp)), [file(u(1))]);
    for p in all_files(m.root.parent().unwrap()).difference(&before) {
        let md = sp.mail.join(KEY);
        assert!(p.starts_with(&sp.sync) || p.starts_with(&md) || sp.sync.starts_with(p) || md.starts_with(p), "{p:?}");
    }
}

#[test]
fn the_cap_gives_500_per_run_and_all_once_after_three() { // S§19 #13
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let ids: Vec<Ulid> = (1..=1200).map(u).collect();
    let bs = blobs(&m, &ids.iter().map(|id| msg(*id, "bob@b")).collect::<Vec<_>>());
    let fs_: Vec<String> = ids.iter().map(|id| file(*id)).collect();
    let e: Vec<(&str, &[u8], &str)> = fs_.iter().zip(&bs).map(|(n, b)| ("100644", n.as_bytes(), b.as_str())).collect();
    let t = tip(&m, &[("bob@b", tree_literally(&m.root, &e))]);
    let mut ack = BTreeSet::new();
    let mut got = Vec::new();
    for (want, left) in [(500, 1), (500, 1), (200, 0)] {
        let (res, lines) = go(&m, &sp, &t, &["b"], &ack, 5000, &NoSeam);
        let res = res.unwrap();
        assert_eq!((res.delivered.len(), res.acked.len(), res.left), (want, want, left));
        assert!(lines.is_empty());
        got.extend(res.delivered.iter().map(|(_, id)| *id));
        ack.extend(res.acked);
    }
    assert_eq!(got, ids); // each once, in ULID order
    assert_eq!(names(&new_dir(&sp)).len(), 1200);
}

#[test]
fn a_ulid_in_the_imported_log_is_acked_not_delivered() {
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let t = tip(&m, &[("bob@b", folder(&m, "bob@b", &[u(1)]))]);
    imported_append(&sp, u(1)).unwrap();
    let (res, lines) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 5000, &NoSeam);
    assert_eq!(res, Ok(Imported { delivered: vec![], acked: vec![u(1)], left: 0 }));
    assert!(lines.is_empty());
    assert!(!new_dir(&sp).exists());
}

#[test]
fn a_full_inbox_leaves_the_message_unacked() { // S§19 #15
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let t = tip(&m, &[("bob@b", folder(&m, "bob@b", &[u(1)]))]);
    fs::create_dir_all(new_dir(&sp)).unwrap();
    fs::write(new_dir(&sp).join(file(u(90))), b"{}").unwrap();
    fs::write(new_dir(&sp).join(file(u(91))), b"{}").unwrap();
    let (res, lines) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 2, &NoSeam);
    assert_eq!(res, Ok(Imported { delivered: vec![], acked: vec![], left: 1 }));
    assert!(lines.is_empty());
    assert!(!imported_load(&sp).unwrap().contains(&u(1)));
    assert!(!sp.incoming.exists() || names(&sp.incoming).is_empty());
    fs::remove_file(new_dir(&sp).join(file(u(90)))).unwrap(); // bob reads one
    let (res, _) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 2, &NoSeam);
    assert_eq!(res.unwrap().delivered, [(KEY.to_string(), u(1))]);
}

#[test]
fn a_crash_after_each_delivery_step_gives_one_file() { // S§19 #14 delivery part
    for (point, finished) in [("after_incoming", 1), ("after_log", 1), ("after_new", 0), ("after_ack", 0)] {
        let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
        let b = msg(u(1), "bob@b");
        let t = tip(&m, &[("bob@b", folder(&m, "bob@b", &[u(1)]))]);
        assert_eq!(go(&m, &sp, &t, &["b"], &BTreeSet::new(), 5000, &CrashAt(point)).0, Err(Crash(point)));
        assert_eq!(finish_incoming(&sp, &sp.mail).unwrap(), finished, "{point}");
        let (res, _) = go(&m, &sp, &t, &["b"], &BTreeSet::new(), 5000, &NoSeam);
        assert_eq!(res, Ok(Imported { delivered: vec![], acked: vec![u(1)], left: 0 }), "{point}");
        assert_eq!(names(&new_dir(&sp)), [file(u(1))], "{point}");
        assert_eq!(fs::read(new_dir(&sp).join(file(u(1)))).unwrap(), b, "{point}");
        assert_eq!(names(&sp.incoming), Vec::<String>::new(), "{point}");
        assert_eq!(fs::read_to_string(&sp.imported).unwrap().matches(&u(1).to_string()).count(), 1, "{point}");
    }
}

#[test]
fn finish_incoming_with_the_ulid_logged_and_the_target_present() { // review focus 17
    let (_t, sp) = local();
    let b = msg(u(1), "bob@b");
    staged(&sp, &format!("{KEY}.{}.json", u(1)), &b);
    imported_append(&sp, u(1)).unwrap();
    fs::create_dir_all(new_dir(&sp)).unwrap();
    fs::write(new_dir(&sp).join(file(u(1))), &b).unwrap();
    assert_eq!(finish_incoming(&sp, &sp.mail).unwrap(), 0);
    assert_eq!(names(&sp.incoming), Vec::<String>::new());
    assert_eq!(names(&new_dir(&sp)), [file(u(1))]);
    assert_eq!(fs::read_to_string(&sp.imported).unwrap(), format!("{}\n", u(1)));
}

#[test]
fn finish_incoming_after_a_torn_log() { // review focus 17, the MS-12 half
    let (_t, sp) = local();
    fs::write(&sp.imported, &u(2).to_string()[..20]).unwrap();
    staged(&sp, &format!("{KEY}.{}.json", u(1)), &msg(u(1), "bob@b"));
    assert_eq!(finish_incoming(&sp, &sp.mail).unwrap(), 1);
    assert_eq!(imported_load(&sp).unwrap(), HashSet::from([u(1)]));
    assert_eq!(names(&new_dir(&sp)), [file(u(1))]);
}

#[test]
fn incoming_is_never_swept() {
    let (_t, sp) = local();
    for n in ["notes.txt".to_string(), format!("{KEY}.bad.json"), format!("KBBBBBBBB.{}.json", u(1))] {
        staged(&sp, &n, b"{}");
    }
    assert_eq!(finish_incoming(&sp, &sp.mail).unwrap(), 0);
    assert_eq!(names(&sp.incoming).len(), 3);
    zygo_workspace::mail::store::sweep(&sp.mail, Duration::ZERO, SystemTime::now() + Duration::from_secs(365 * 86400));
    assert_eq!(names(&sp.incoming).len(), 3);
}

#[test]
fn own_hosts_dedupe_and_unknown_names() { // S§7.5 own hosts, S§13 limit 2, one delivery per ULID
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let t = tip(&m, &[("bob@b", folder(&m, "bob@b", &[u(1)])), ("bob@b2", folder(&m, "bob@b2", &[u(1), u(2)])),
        ("carol@b", folder(&m, "carol@b", &[u(3)]))]);
    let (res, lines) = go(&m, &sp, &t, &["b", "b2"], &BTreeSet::new(), 5000, &NoSeam);
    assert_eq!(res, Ok(Imported { delivered: vec![(KEY.into(), u(1)), (KEY.into(), u(2))], acked: vec![u(1), u(2)], left: 0 }));
    assert!(lines.is_empty());
    assert_eq!(names(&new_dir(&sp)), [file(u(1)), file(u(2))]);
}

#[test]
fn own_ack_entries_are_not_candidates() {
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let t = tip(&m, &[("bob@b", folder(&m, "bob@b", &[u(1)]))]);
    let (res, _) = go(&m, &sp, &t, &["b"], &BTreeSet::from([u(1)]), 5000, &NoSeam);
    assert_eq!(res, Ok(Imported::default()));
    assert!(!sp.mail.join(KEY).exists());
}

#[test]
fn a_passed_deadline_or_a_bad_target_reads_nothing() {
    let r = Remote::new(); let m = Machine::new(&r); let sp = sync_paths(&m);
    let t = tip(&m, &[("bob@b", folder(&m, "bob@b", &[u(1)]))]);
    let mut lines = Vec::new();
    let res = run(&sp, &sp.mail, &m.wt, &t, &["b".into()], &bob(), &BTreeSet::new(), 5000, Instant::now(), &NoSeam,
        &mut |l| lines.push(l.to_string()));
    assert_eq!(res, Ok(Imported { delivered: vec![], acked: vec![], left: 1 }));
    assert_eq!(lines, ["mail sync: import: ls-tree"]);
    for (targets, host) in [(vec![Target { name: "bob".into(), pane_key: "../x".into() }], "b"),
        (vec![Target { name: "cli".into(), pane_key: KEY.into() }], "b"), (bob(), "B!")] {
        let res = run(&sp, &sp.mail, &m.wt, &t, &[host.into()], &targets, &BTreeSet::new(), 5000, far(), &NoSeam, &mut |_| {});
        assert_eq!(res, Ok(Imported::default()));
    }
    assert!(!sp.mail.join(KEY).exists() && !sp.incoming.exists());
}
