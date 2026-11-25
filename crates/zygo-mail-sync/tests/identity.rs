//! MS-11: tip metadata, own hosts, collision and twin checks (mail sync design S§7.3-S§7.5; S§19 #9-#11
//! detection). No test depends on wall-clock time.
#![cfg(unix)]

mod common;

use common::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use zygo_core::paths::Paths;
use zygo_mail_sync::identity::*;
use zygo_mail_sync::registry::{Ack, Check, REGISTRY_MAX, Registry};
use zygo_mail_sync::state::{State, SyncPaths};

const ME: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const THIRD: &str = "cccccccccccccccccccccccccccccccc";
const B1: &str = "1111111111111111111111111111111111111111";
const B2: &str = "2222222222222222222222222222222222222222";
const B3: &str = "3333333333333333333333333333333333333333";
const MOVED_WS: &str = "fedcba9876543210";
const COLLISION: &str = "zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.";
const TWIN: &str = "zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host";

fn reg(host: &str, id: &str) -> Registry {
    Registry { host: host.into(), identity: id.into(), members: vec!["alice".into()] }
}

fn ack(host: &str, id: &str) -> Ack {
    Ack { host: host.into(), identity: id.into(), imported: BTreeSet::new() }
}

/// Each `(file host, identity, blob)` is a valid file under key `file host`.
fn meta(regs: &[(&str, &str, &str)], acks: &[(&str, &str, &str)]) -> TipMeta {
    TipMeta {
        registries: regs.iter()
            .map(|(h, i, b)| (h.to_string(), Read { blob: b.to_string(), value: Ok(reg(h, i)) }))
            .collect(),
        acks: acks.iter()
            .map(|(h, i, b)| (h.to_string(), Read { blob: b.to_string(), value: Ok(ack(h, i)) }))
            .collect(),
    }
}

/// The zygo paths under `p`, with the field values of `common::paths`.
fn paths_in(p: &Path) -> Paths {
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

/// A temp root with one workspace state per `(ws_id, identity)`.
fn states(ids: &[(&str, &str)]) -> (tempfile::TempDir, Paths) {
    let d = tempfile::TempDir::new().unwrap();
    let paths = paths_in(d.path());
    for (ws, id) in ids {
        let sp = SyncPaths::new(&paths, ws).unwrap();
        std::fs::create_dir_all(&sp.sync).unwrap();
        std::fs::write(&sp.identity, id).unwrap();
    }
    (d, paths)
}

/// A root commit with `ZYGO-MAIL` and each `(folder, name, mode, bytes)`; names are given in git order.
fn tip(m: &Machine, files: &[(&str, &str, &str, &[u8])]) -> String {
    let marker = blob(&m.root, b"1\n");
    let mut top: Vec<(&str, &[u8], String)> = vec![("100644", b"ZYGO-MAIL", marker)];
    for folder in ["ack", "registry"] {
        let ids: Vec<(&str, &str, String)> = files.iter().filter(|f| f.0 == folder)
            .map(|(_, name, mode, bytes)| (*mode, *name, blob(&m.root, bytes))).collect();
        if ids.is_empty() {
            continue;
        }
        let entries: Vec<(&str, &[u8], &str)> = ids.iter().map(|(mo, n, id)| (*mo, n.as_bytes(), id.as_str())).collect();
        top.push(("40000", folder.as_bytes(), tree_literally(&m.root, &entries)));
    }
    let entries: Vec<(&str, &[u8], &str)> = top.iter().map(|(mo, n, id)| (*mo, *n, id.as_str())).collect();
    let top = tree_literally(&m.root, &entries);
    commit_tree(&m.root, &top, None)
}

#[test]
fn read_meta_reads_valid_registry_and_ack() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let a = Ack { imported: BTreeSet::from([ulid::Ulid::from_string("01J00000000000000000000000").unwrap()]), ..ack("x", ME) };
    let (rb, ab) = (reg("x", ME).to_bytes(), a.to_bytes());
    let t = tip(&m, &[("ack", "x.json", "100644", &ab), ("registry", "x.json", "100644", &rb)]);
    let mut log = Vec::new();
    let got = read_meta(&m.root, &t, far(), &mut |l| log.push(l.to_string())).unwrap();
    assert_eq!(got, TipMeta {
        registries: BTreeMap::from([("x".to_string(), Read { blob: blob(&m.root, &rb), value: Ok(reg("x", ME)) })]),
        acks: BTreeMap::from([("x".to_string(), Read { blob: blob(&m.root, &ab), value: Ok(a) })]),
    });
    assert!(log.is_empty());
}

#[test]
fn read_meta_logs_one_line_per_invalid_file_with_check_and_blob_only() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let big = reg("big", ME).to_bytes();
    let huge = vec![b' '; REGISTRY_MAX as usize + 1];
    let forged = ack("z", ME).to_bytes(); // in ack/y.json: host is not the file name
    let t = tip(&m, &[("ack", "y.json", "100644", &forged),
        ("registry", "README", "100644", b"x"), ("registry", "big.json", "100755", &big),
        ("registry", "huge.json", "100644", &huge), ("registry", "x.json", "100644", b"{")]);
    let mut log = Vec::new();
    let got = read_meta(&m.root, &t, far(), &mut |l| log.push(l.to_string())).unwrap();
    assert_eq!(log, vec![
        format!("mail sync: registry file refused: check mode, object {}", blob(&m.root, &big)),
        format!("mail sync: registry file refused: check size, object {}", blob(&m.root, &huge)),
        format!("mail sync: registry file refused: check json, object {}", blob(&m.root, b"{")),
        format!("mail sync: ack file refused: check host, object {}", blob(&m.root, &forged)),
    ]);
    assert_eq!(got.registries.keys().collect::<Vec<_>>(), ["big", "huge", "x"]); // README: not a .json name, not read
    assert_eq!(got.registries["big"].value, Err(Check("mode")));
    assert_eq!(got.registries["x"].value, Err(Check("json")));
    assert_eq!(got.acks["y"].value, Err(Check("host")));
    for l in &log {
        for h in ["big", "huge", "README", "x.json", "y.json", "alice"] {
            assert!(!l.contains(h), "{l}");
        }
    }
}

#[test]
fn read_meta_of_a_tip_without_registry_or_ack_is_empty() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let t = tip(&m, &[]);
    let mut log = Vec::new();
    assert_eq!(read_meta(&m.root, &t, far(), &mut |l| log.push(l.to_string())), Ok(TipMeta::default()));
    assert!(log.is_empty());
}

#[test]
fn own_hosts_is_the_host_plus_each_host_whose_valid_ack_holds_this_identity() {
    let mut m = meta(&[("reg", ME, B1)], &[("old", ME, B1), ("x", ME, B3), ("y", OTHER, B2)]);
    m.acks.insert("bad".into(), Read { blob: B3.into(), value: Err(Check("json")) });
    assert_eq!(own_hosts(&m, ME, "x"), ["x", "old"]);
    assert_eq!(own_hosts(&TipMeta::default(), ME, "x"), ["x"]);
}

#[test]
fn another_identity_in_the_current_hosts_registry_is_a_collision() {
    let (_d, p) = states(&[]);
    let e = check(&meta(&[("x", OTHER, B1)], &[]), "x", ME, &State::default(), &p, WS_ID).unwrap_err();
    assert_eq!(e, Stop::Collision { moved_from: None });
    assert_eq!(e.line(), COLLISION);
    assert_eq!(e.check_name(), "collision");
}

#[test]
fn another_identity_in_the_current_hosts_ack_is_a_collision() {
    let (_d, p) = states(&[]);
    let st = State { pushed: BTreeMap::from([("registry/x.json".into(), B1.into())]), ..Default::default() };
    let m = meta(&[("x", ME, B1)], &[("x", OTHER, B2)]);
    assert_eq!(check(&m, "x", ME, &st, &p, WS_ID), Err(Stop::Collision { moved_from: None }));
}

#[test]
fn collision_names_the_other_workspace_state_that_holds_that_identity() {
    // S§19 #10; Review focus 51
    let (d, p) = states(&[(WS_ID, ME), ("1111111111111111", THIRD), (MOVED_WS, OTHER)]);
    let bad = p.workspaces_dir().join("not-a-ws/mail/sync");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("identity"), OTHER).unwrap(); // not a workspace id: never read
    let e = check(&meta(&[("x", OTHER, B1)], &[]), "x", ME, &State::default(), &p, WS_ID).unwrap_err();
    assert_eq!(e, Stop::Collision { moved_from: Some(MOVED_WS.into()) });
    assert_eq!(e.line(), format!("{COLLISION} The workspace state {MOVED_WS} on this machine also holds that identity."));
    assert_eq!(e.check_name(), "collision"); // the errors.log value holds no workspace id
    drop(d);
    // this workspace's own state is never named
    let (_d2, p2) = states(&[(WS_ID, OTHER)]);
    assert_eq!(check(&meta(&[("x", OTHER, B1)], &[]), "x", ME, &State::default(), &p2, WS_ID),
        Err(Stop::Collision { moved_from: None }));
}

#[test]
fn an_own_blob_that_is_neither_pushed_nor_pending_is_a_twin() {
    // S§19 #11 detection
    let (_d, p) = states(&[]);
    let st = State { pushed: BTreeMap::from([("ack/x.json".into(), B2.into())]),
        pending: BTreeMap::from([("ack/x.json".into(), B3.into())]), ..Default::default() };
    let e = check(&meta(&[], &[("x", ME, B1)]), "x", ME, &st, &p, WS_ID).unwrap_err();
    assert_eq!(e, Stop::Twin);
    assert_eq!(e.line(), TWIN);
    assert_eq!(e.check_name(), "twin");
    // an own file of an older host counts too; no record at all is a twin
    assert_eq!(check(&meta(&[("old", ME, B1)], &[]), "x", ME, &State::default(), &p, WS_ID), Err(Stop::Twin));
}

#[test]
fn the_pushed_or_the_pending_blob_is_no_twin() {
    // Review focus 48: crash after "about to push", before the push
    let (_d, p) = states(&[]);
    let m = meta(&[("x", ME, B1)], &[("old", ME, B3), ("x", ME, B2)]);
    let st = State {
        pushed: BTreeMap::from([("registry/x.json".into(), B1.into()), ("ack/x.json".into(), B3.into()), ("ack/old.json".into(), B3.into())]),
        pending: BTreeMap::from([("ack/x.json".into(), B2.into())]),
        ..Default::default()
    };
    assert_eq!(check(&m, "x", ME, &st, &p, WS_ID), Ok(()));
}

#[test]
fn a_tip_with_no_own_files_passes() {
    let (_d, p) = states(&[]);
    assert_eq!(check(&TipMeta::default(), "x", ME, &State::default(), &p, WS_ID), Ok(()));
    let mut m = meta(&[("y", OTHER, B1)], &[("y", OTHER, B2)]); // another host's files
    m.registries.insert("x".into(), Read { blob: B3.into(), value: Err(Check("identity")) }); // a refused file is skipped
    assert_eq!(check(&m, "x", ME, &State::default(), &p, WS_ID), Ok(()));
}

#[test]
fn a_real_tip_with_another_identity_at_the_host_is_a_collision() {
    // S§19 #9 detection, through git
    let r = Remote::new();
    let m = Machine::new(&r);
    let t = tip(&m, &[("registry", "x.json", "100644", &reg("x", OTHER).to_bytes())]);
    let meta = read_meta(&m.root, &t, far(), &mut |_| {}).unwrap();
    let (_d, p) = states(&[]);
    assert_eq!(check(&meta, "x", ME, &State::default(), &p, WS_ID), Err(Stop::Collision { moved_from: None }));
}

/// A root commit whose `registry` and `ack` trees are given as raw `(mode, name, hex id)` entries, in git order.
fn raw_tip(m: &Machine, regs: &[(&str, &str, &str)], acks: &[(&str, &str, &str)]) -> String {
    let marker = blob(&m.root, b"1\n");
    let mut top: Vec<(&str, &[u8], String)> = vec![("100644", b"ZYGO-MAIL", marker)];
    for (folder, es) in [("ack", acks), ("registry", regs)] {
        if es.is_empty() {
            continue;
        }
        let entries: Vec<(&str, &[u8], &str)> = es.iter().map(|(mo, n, id)| (*mo, n.as_bytes(), *id)).collect();
        top.push(("40000", folder.as_bytes(), tree_literally(&m.root, &entries)));
    }
    let entries: Vec<(&str, &[u8], &str)> = top.iter().map(|(mo, n, id)| (*mo, *n, id.as_str())).collect();
    let top = tree_literally(&m.root, &entries);
    commit_tree(&m.root, &top, None)
}

#[test]
fn read_meta_refuses_links_submodules_and_trees_by_mode_without_a_read() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let link = blob(&m.root, &reg("l", ME).to_bytes());
    let missing = "0123456789012345678901234567890123456789"; // no such object: a `cat` of it would fail
    let t = raw_tip(&m, &[("120000", "l.json", &link), ("160000", "s.json", missing)], &[("40000", "t.json", missing)]);
    let mut log = Vec::new();
    let got = read_meta(&m.root, &t, far(), &mut |l| log.push(l.to_string())).unwrap();
    assert_eq!(got.registries["l"], Read { blob: link.clone(), value: Err(Check("mode")) });
    assert_eq!(got.registries["s"], Read { blob: missing.into(), value: Err(Check("mode")) });
    assert_eq!(got.acks["t"], Read { blob: missing.into(), value: Err(Check("mode")) });
    assert_eq!(log, vec![
        format!("mail sync: registry file refused: check mode, object {link}"),
        format!("mail sync: registry file refused: check mode, object {missing}"),
        format!("mail sync: ack file refused: check mode, object {missing}"),
    ]);
}

#[test]
fn read_meta_after_the_deadline_is_an_error() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let t = tip(&m, &[("registry", "x.json", "100644", &reg("x", ME).to_bytes())]);
    let past = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(1)).unwrap();
    assert!(read_meta(&m.root, &t, past, &mut |_| {}).is_err());
}

#[test]
fn read_meta_refuses_a_name_that_is_not_a_host_without_a_key() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let (b, c) = (reg("x", ME).to_bytes(), ack("x", ME).to_bytes());
    let t = tip(&m, &[("ack", "Bad.json", "100644", &c), ("registry", ".json", "100644", &b),
        ("registry", "a\tb.json", "100644", &b), ("registry", "x.json", "100644", &b)]);
    let mut log = Vec::new();
    let got = read_meta(&m.root, &t, far(), &mut |l| log.push(l.to_string())).unwrap();
    assert_eq!(got.registries.keys().collect::<Vec<_>>(), ["x"]);
    assert!(got.acks.is_empty());
    let (bb, cb) = (blob(&m.root, &b), blob(&m.root, &c));
    assert_eq!(log, vec![
        format!("mail sync: registry file refused: check name, object {bb}"),
        format!("mail sync: registry file refused: check name, object {bb}"),
        format!("mail sync: ack file refused: check name, object {cb}"),
    ]);
}

#[test]
fn a_duplicate_entry_at_the_host_never_hides_a_collision() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let (other, mine) = (blob(&m.root, &reg("x", OTHER).to_bytes()), blob(&m.root, &reg("x", ME).to_bytes()));
    let (_d, p) = states(&[]);
    let st = State { pushed: BTreeMap::from([("registry/x.json".into(), mine.clone())]), ..Default::default() };
    for (a, b) in [(&other, &mine), (&mine, &other)] {
        let t = raw_tip(&m, &[("100644", "x.json", a), ("100644", "x.json", b)], &[]);
        let meta = read_meta(&m.root, &t, far(), &mut |_| {}).unwrap();
        assert_eq!(check(&meta, "x", ME, &st, &p, WS_ID), Err(Stop::Collision { moved_from: None }));
    }
}

#[test]
fn the_moved_scan_skips_unreadable_invalid_and_linked_states() {
    use std::os::unix::fs::symlink;
    let m = meta(&[("x", OTHER, B1)], &[]);
    let setups: [fn(&Path, &Path); 6] = [
        |ws, _| { std::fs::create_dir_all(ws.join("mail/sync/identity")).unwrap(); },           // unreadable
        |ws, _| { std::fs::create_dir_all(ws.join("mail/sync")).unwrap(); std::fs::write(ws.join("mail/sync/identity"), "zz").unwrap(); },
        |ws, real| { std::fs::create_dir_all(ws.join("mail/sync")).unwrap(); symlink(real.join("mail/sync/identity"), ws.join("mail/sync/identity")).unwrap(); },
        |ws, real| { symlink(real, ws).unwrap(); },                                               // workspaces/<id> link
        |ws, real| { std::fs::create_dir_all(ws).unwrap(); symlink(real.join("mail"), ws.join("mail")).unwrap(); },
        |ws, real| { std::fs::create_dir_all(ws.join("mail")).unwrap(); symlink(real.join("mail/sync"), ws.join("mail/sync")).unwrap(); },
    ];
    for (i, setup) in setups.iter().enumerate() {
        let (d, p) = states(&[]);
        let real = d.path().join("elsewhere");
        std::fs::create_dir_all(real.join("mail/sync")).unwrap();
        std::fs::write(real.join("mail/sync/identity"), OTHER).unwrap();
        std::fs::create_dir_all(p.workspaces_dir()).unwrap();
        setup(&p.workspaces_dir().join(MOVED_WS), &real);
        assert_eq!(check(&m, "x", ME, &State::default(), &p, WS_ID), Err(Stop::Collision { moved_from: None }), "setup {i}");
    }
}

#[test]
fn an_own_ack_under_another_host_with_no_record_is_an_own_host_and_a_twin() {
    // S§19 #11, S§7.5 "Own hosts" and "Twin"
    let r = Remote::new();
    let m = Machine::new(&r);
    let t = tip(&m, &[("ack", "c.json", "100644", &ack("c", ME).to_bytes())]);
    let meta = read_meta(&m.root, &t, far(), &mut |_| {}).unwrap();
    assert_eq!(own_hosts(&meta, ME, "x"), ["x", "c"]);
    let (_d, p) = states(&[]);
    assert_eq!(check(&meta, "x", ME, &State::default(), &p, WS_ID), Err(Stop::Twin));
}
