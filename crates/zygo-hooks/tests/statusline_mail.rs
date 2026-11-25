//! ML-15: the unread mail count (M§9a, M§13 #10) and the member name in `collect` (M§6, M§9a).
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_hooks::statusline::data::collect;
use zygo_hooks::statusline::mail::unread;

const ID: &str = "0123456789abcdef";
const KEY: &str = "kabcd2345";

fn paths(t: &TempDir) -> Paths {
    let (cache_dir, data_dir) = (t.path().join("cache"), t.path().join("data"));
    Paths {
        config_file: t.path().join("config.toml"),
        sessions_dir: t.path().join("sessions"),
        marketplaces_dir: cache_dir.join("marketplaces"),
        plugin_store: data_dir.join("plugins"),
        approvals_file: data_dir.join("approvals.toml"),
        usage_db: data_dir.join("usage.db"),
        cache_dir,
        data_dir,
    }
}

fn view(ws_id: &str, pane: &str, name: Option<&str>) -> SessionView {
    let member = Member { ws_id: ws_id.into(), pane: pane.into(), name: name.map(str::to_string), ..Default::default() };
    SessionView { session: "S1".into(), workspace: Some(ViewWorkspace { member, resume_note: None, ..Default::default() }), ..Default::default() }
}

/// `<workspaces_dir>/<ws_id>/mail/<pane>/new` with `n` message files; no check of either part.
fn inbox(p: &Paths, ws_id: &str, pane: &str, n: usize) -> PathBuf {
    let dir = p.workspaces_dir().join(ws_id).join("mail").join(pane).join("new");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..n {
        std::fs::write(dir.join(format!("01MCC5RM0000000000000000{i:02}.json")), br#"{"version":1,"subject":"s3cr3t"}"#).unwrap();
    }
    dir
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

#[test] // M§9a: the count of regular <ULID>.json files in mail/<pane key>/new/
fn unread_counts_the_json_names_in_new() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    let new = inbox(&p, ID, KEY, 3);
    std::fs::write(new.join("x.tmp"), b"").unwrap();
    std::fs::create_dir_all(new.parent().unwrap().join("cur")).unwrap();
    std::fs::write(new.parent().unwrap().join("cur/01MCC5RM000000000000000099.json"), b"{}").unwrap();
    assert_eq!(unread(&p, &view(ID, KEY, None), later()), Some(3));
    inbox(&p, ID, "kzzzz2222", 0);
    assert_eq!(unread(&p, &view(ID, "kzzzz2222", None), later()), Some(0));
}

#[test] // M§9a: hidden outside a workspace; a missing new/ counts 0 (the store's count)
fn unread_outside_a_workspace_is_none_and_a_missing_new_is_zero() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    inbox(&p, ID, KEY, 3);
    assert_eq!(unread(&p, &SessionView { session: "S1".into(), ..Default::default() }, later()), None);
    assert_eq!(unread(&p, &view(ID, "kqqqq2222", None), later()), Some(0));
}

#[test] // M§9a: only regular <ULID>.json files count; no link is followed
fn unread_skips_links_folders_and_non_ulid_names() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    let new = inbox(&p, ID, KEY, 2);
    for name in [
        "junk.json",
        "01mcc5rm000000000000000003.json",
        "01MCC5RM00000000000000000U.json",
        "01MCC5RM00000000000000004.json",
        "01MCC5RM0000000000000000005.json",
        "01MCC5RM000000000000000006",
        "01MCC5RM000000000000000007.json.tmp",
        ".json",
    ] {
        std::fs::write(new.join(name), b"{}").unwrap();
    }
    std::fs::create_dir(new.join("01MCC5RM000000000000000008.json")).unwrap();
    #[cfg(unix)] // a Windows symlink needs a privilege that CI may not have
    {
        let target = t.path().join("outside.json");
        std::fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, new.join("01MCC5RM000000000000000009.json")).unwrap();
        std::os::unix::fs::symlink(t.path().join("gone"), new.join("01MCC5RM000000000000000010.json")).unwrap();
    }
    assert_eq!(unread(&p, &view(ID, KEY, None), later()), Some(2));
}

#[test] // M§9a: the HUD count is the store's count (M§5), so the HUD and `zygo workspace` agree
fn unread_matches_the_store_count() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    let new = inbox(&p, ID, KEY, 2);
    std::fs::write(new.join("junk.json"), b"{}").unwrap();
    std::fs::create_dir(new.join("01MCC5RM000000000000000020.json")).unwrap();
    #[cfg(unix)] // a Windows symlink needs a privilege that CI may not have
    {
        let target = t.path().join("outside.json");
        std::fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, new.join("01MCC5RM000000000000000021.json")).unwrap();
    }
    let mail = p.workspaces_dir().join(ID).join("mail");
    let store = zygo_workspace::mail::store::unread(&mail, KEY).unwrap();
    assert_eq!(store, 2);
    assert_eq!(unread(&p, &view(ID, KEY, None), later()), Some(store));
    let missing = zygo_workspace::mail::store::unread(&mail, "kqqqq2222").unwrap();
    assert_eq!(unread(&p, &view(ID, "kqqqq2222", None), later()), Some(missing));
}

#[test] // M§9a, WS AU84: both ids are checked before a path is built
fn unread_hostile_pane_key_or_ws_id_builds_no_path() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    inbox(&p, ID, KEY, 3);
    // Each traversal below names the real inbox: a count would mean that a path was built.
    for pane in ["", "KABCD2345", "kabcd234", "kabcd23456", "kabcd2381", "../mail/kabcd2345", "kabcd2345/"] {
        assert_eq!(unread(&p, &view(ID, pane, None), later()), None, "{pane:?}");
    }
    for ws in ["", "0123456789ABCDEF", "0123456789abcde", "../workspaces/0123456789abcdef"] {
        assert_eq!(unread(&p, &view(ws, KEY, None), later()), None, "{ws:?}");
    }
    assert_eq!(unread(&p, &view(ID, KEY, None), later()), Some(3));
}

#[test] // H§7.7: the read runs within the deadline; a passed deadline hides the segment
fn unread_after_the_deadline_is_none() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    inbox(&p, ID, KEY, 3);
    assert_eq!(unread(&p, &view(ID, KEY, None), Instant::now()), None);
    assert_eq!(unread(&p, &view(ID, KEY, None), later()), Some(3));
}

#[cfg(unix)]
#[test] // M§13 #10, M-C2: no message file is opened, so a file with mode 0000 is still counted
fn unread_counts_a_file_it_cannot_open() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    if is_root() {
        eprintln!("skipped: the test runs as root, and root ignores mode 0000");
        return;
    }
    let new = inbox(&p, ID, KEY, 3);
    for e in std::fs::read_dir(&new).unwrap() {
        std::fs::set_permissions(e.unwrap().path(), std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    assert_eq!(unread(&p, &view(ID, KEY, None), later()), Some(3));
}

#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: `geteuid` takes no argument and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(unix)]
#[test] // M§9a: a read error hides the segment; here `new/` itself cannot be read
fn unread_of_an_unreadable_new_dir_is_none() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    if is_root() {
        eprintln!("skipped: the test runs as root, and root ignores mode 0000");
        return;
    }
    let new = inbox(&p, ID, KEY, 3);
    let mode = std::fs::metadata(&new).unwrap().permissions().mode();
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o000)).unwrap();
    let got = unread(&p, &view(ID, KEY, None), later());
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(got, None);
}

#[test] // M§9a: `collect` gives the name (checked, M§6) and the count; no message text reaches the HUD data (M-C2)
fn collect_gives_the_member_name_and_the_unread_count() {
    let t = tempfile::tempdir().unwrap();
    let p = paths(&t);
    let dir = t.path().join("sessions").join("S1");
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    inbox(&p, ID, KEY, 3);
    let run = |v: SessionView| {
        v.save(&sd).unwrap();
        collect(Box::new(Cursor::new(Vec::new())), &dir, &p, &|_: &str| None, later())
    };
    let d = run(view(ID, KEY, Some("ana")));
    assert_eq!((d.member_name.as_deref(), d.unread), (Some("ana"), Some(3)));
    assert!(!format!("{d:?}").contains("s3cr3t"));
    for bad in ["Bad Name", "kabcd2345", "all", ""] {
        assert_eq!(run(view(ID, KEY, Some(bad))).member_name, None, "{bad:?}");
    }
    let d = run(SessionView { session: "S1".into(), ..Default::default() });
    assert_eq!((d.member_name, d.unread), (None, None));
}
