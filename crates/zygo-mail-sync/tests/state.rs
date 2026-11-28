//! MS-5: the sync state files (mail sync design S§5, S§7.5, S§8.3, S§8.4, S§8.5, S§15; S§16 W§7.1, M-C4).
//! No test depends on wall-clock time. The tests of modes and links are `cfg(unix)`; the rest also run on
//! Windows, where `assert_mode` checks nothing.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use ulid::Ulid;
use zygo_core::paths::Paths;
use zygo_mail_sync::state::*;

const ID: &str = "0123456789abcdef";

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

fn setup() -> (TempDir, SyncPaths) {
    let t = TempDir::new().unwrap();
    let sp = SyncPaths::new(&paths(t.path()), ID).unwrap();
    fs::create_dir_all(sp.mail.parent().unwrap()).unwrap(); // the workspace exists (MS-21 review R2-1)
    (t, sp)
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn set_mode(p: &Path, m: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
}

/// The mode of `p` is `m` (unix); nothing on other systems.
fn assert_mode(p: &Path, m: u32) {
    #[cfg(unix)]
    assert_eq!(mode(p), m, "{}", p.display());
    #[cfg(not(unix))]
    let _ = (p, m);
}

/// The sorted file names in `dir`.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

fn u(n: u64) -> Ulid {
    Ulid::from_parts(n, n as u128)
}

fn set(v: &[Ulid]) -> HashSet<Ulid> {
    v.iter().copied().collect()
}

#[test]
fn sync_paths_follow_w71() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let sp = SyncPaths::new(&p, ID).unwrap();
    let m = p.workspaces_dir().join(ID).join("mail");
    assert_eq!(sp.mail, m);
    assert_eq!(sp.mail, zygo_workspace::mail::mail_dir(&p, ID));
    assert_eq!(
        (sp.outbox.clone(), sp.outbox_tmp.clone(), sp.refused.clone()),
        (
            m.join("outbox"),
            m.join("outbox").join("tmp"),
            m.join("outbox").join("refused")
        )
    );
    let s = m.join("sync");
    assert_eq!(sp.sync, s);
    assert_eq!(
        [
            &sp.incoming,
            &sp.imported,
            &sp.identity,
            &sp.state,
            &sp.members,
            &sp.lock,
            &sp.again,
            &sp.last_start
        ],
        [
            &s.join("incoming"),
            &s.join("imported"),
            &s.join("identity"),
            &s.join("state.json"),
            &s.join("members.json"),
            &s.join("lock"),
            &s.join("again"),
            &s.join("last_start")
        ]
    );
    assert_eq!(
        sp.worktree,
        p.workspace_cache_dir().join(ID).join("mail-sync")
    );
    assert_eq!(
        sp.worktree,
        t.path()
            .join("cache")
            .join("workspaces")
            .join(ID)
            .join("mail-sync")
    );
    // pure: nothing is created
    assert!(!t.path().join("data").exists() && !t.path().join("cache").exists());
    // the two folders never collide with a member maildir mail/<pane key>/
    assert!(
        !zygo_workspace::identity::valid_pane_key("outbox")
            && !zygo_workspace::identity::valid_pane_key("sync")
    );
}

#[test]
fn sync_paths_refuse_a_bad_workspace_id() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    for bad in [
        "../../escape",
        "",
        "0123456789ABCDEF",
        "0123456789abcde",
        "0123456789abcdef0",
    ] {
        let e = SyncPaths::new(&p, bad).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{bad:?}");
    }
    assert_eq!(names(t.path()), Vec::<String>::new()); // nothing is created
}

#[cfg(unix)]
#[test]
fn write_atomic_replaces_0600_in_0700_folders() {
    let (_t, sp) = setup();
    let f = sp.sync.join("x");
    // MS-21 review R2-1: it makes the file's folder only, never a parent of it.
    assert_eq!(write_atomic(&f, b"one").unwrap_err().kind(), std::io::ErrorKind::NotFound);
    assert!(fs::symlink_metadata(&sp.mail).is_err());
    for d in [&sp.mail, &sp.outbox] {
        zygo_workspace::mail::ensure_mail_dir(d).unwrap();
    }
    write_atomic(&f, b"one").unwrap();
    set_mode(&f, 0o644);
    write_atomic(&f, b"two").unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"two");
    assert_eq!(mode(&f), 0o600);
    assert_eq!((mode(&sp.mail), mode(&sp.sync)), (0o700, 0o700));
    assert_eq!(names(&sp.sync), ["x"]); // no temp file left
    write_atomic(&sp.outbox_tmp.join("y"), b"").unwrap();
    assert_eq!((mode(&sp.outbox), mode(&sp.outbox_tmp)), (0o700, 0o700));
}

#[cfg(unix)]
#[test]
fn write_atomic_refuses_a_linked_folder() {
    let (t, sp) = setup();
    let real = t.path().join("elsewhere");
    fs::create_dir(&real).unwrap();
    fs::create_dir_all(&sp.mail).unwrap();
    std::os::unix::fs::symlink(&real, &sp.sync).unwrap();
    let e = write_atomic(&sp.state, b"{}").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(names(&real), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn a_write_sets_mail_to_0700() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    set_mode(&sp.mail, 0o755);
    mark_again(&sp).unwrap();
    assert_eq!((mode(&sp.mail), mode(&sp.sync)), (0o700, 0o700));
    set_mode(&sp.mail, 0o755);
    imported_append(&sp, u(1)).unwrap();
    assert_eq!(mode(&sp.mail), 0o700);
}

#[cfg(unix)]
#[test]
fn a_write_refuses_a_linked_mail_folder() {
    let (t, sp) = setup();
    let real = t.path().join("elsewhere");
    fs::create_dir(&real).unwrap();
    fs::create_dir_all(sp.mail.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &sp.mail).unwrap();
    assert_eq!(
        save(&sp, &State::default()).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        imported_append(&sp, u(1)).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(names(&real), Vec::<String>::new());
}

#[test]
fn make_identity_writes_32_hex_once() {
    let (_t, sp) = setup();
    assert_eq!(identity(&sp).unwrap(), None);
    let a = make_identity(&sp).unwrap();
    assert_eq!(a.len(), 32);
    assert!(
        a.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')),
        "{a}"
    );
    assert_eq!(fs::read_to_string(&sp.identity).unwrap(), a);
    assert_mode(&sp.identity, 0o600);
    assert_mode(&sp.sync, 0o700);
    assert_eq!(identity(&sp).unwrap(), Some(a.clone()));
    assert_eq!(make_identity(&sp).unwrap(), a);
    // an existing identity is kept, not replaced; a `\n` or `\r\n` ending is allowed
    let fixed = "00112233445566778899aabbccddeeff";
    for end in ["\n", "\r\n"] {
        fs::write(&sp.identity, format!("{fixed}{end}")).unwrap();
        assert_eq!(identity(&sp).unwrap().as_deref(), Some(fixed), "{end:?}");
        assert_eq!(make_identity(&sp).unwrap(), fixed);
        assert_eq!(
            fs::read_to_string(&sp.identity).unwrap(),
            format!("{fixed}{end}")
        );
    }
}

#[test]
fn identity_refuses_a_bad_file() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    for bad in [
        "",
        "xyz",
        "00112233445566778899AABBCCDDEEFF",
        "00112233445566778899aabbccddeef",
        "00112233445566778899aabbccddeeff0",
        "00112233445566778899aabbccddeeff\n\n",
        "00112233445566778899aabbccddeeff\r",
    ] {
        fs::write(&sp.identity, bad).unwrap();
        assert_eq!(
            identity(&sp).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData,
            "{bad:?}"
        );
        assert_eq!(
            make_identity(&sp).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData,
            "{bad:?}"
        );
        assert_eq!(fs::read_to_string(&sp.identity).unwrap(), bad);
    }
}

#[cfg(unix)]
#[test]
fn reads_refuse_a_link_and_a_fifo() {
    let (t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    let other = t.path().join("other");
    fs::write(
        &other,
        format!("00112233445566778899aabbccddeeff\n{}\n", u(1)),
    )
    .unwrap();
    for p in [&sp.identity, &sp.imported, &sp.state, &sp.last_start] {
        std::os::unix::fs::symlink(&other, p).unwrap();
    }
    assert!(identity(&sp).is_err());
    assert!(make_identity(&sp).is_err());
    assert!(imported_load(&sp).is_err());
    assert!(imported_append(&sp, u(2)).is_err());
    assert!(load(&sp).is_err());
    fs::write(&other, "7").unwrap();
    assert_eq!(read_last_start(&sp), None);
    assert_eq!(
        fs::read_to_string(&other).unwrap(),
        "7",
        "the link target is not changed"
    );
    // a FIFO is refused without a wait for a writer
    for p in [&sp.identity, &sp.imported, &sp.state, &sp.last_start] {
        fs::remove_file(p).unwrap();
        let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }
    assert!(identity(&sp).is_err());
    assert!(imported_load(&sp).is_err());
    assert!(load(&sp).is_err());
    assert_eq!(read_last_start(&sp), None);
}

#[test]
fn imported_append_adds_27_bytes_and_load_reads_back() {
    let (_t, sp) = setup();
    assert!(imported_load(&sp).unwrap().is_empty()); // no file yet
    for n in 1..=3 {
        imported_append(&sp, u(n)).unwrap();
        assert_eq!(fs::metadata(&sp.imported).unwrap().len(), 27 * n);
    }
    assert_eq!(
        fs::read_to_string(&sp.imported).unwrap(),
        format!("{}\n{}\n{}\n", u(1), u(2), u(3))
    );
    assert_eq!(imported_load(&sp).unwrap(), set(&[u(1), u(2), u(3)]));
    assert_mode(&sp.imported, 0o600);
    assert_mode(&sp.sync, 0o700);
    imported_append(&sp, u(1)).unwrap(); // never trimmed; a repeat is one more line
    assert_eq!(fs::metadata(&sp.imported).unwrap().len(), 27 * 4);
    assert_eq!(imported_load(&sp).unwrap().len(), 3);
}

#[cfg(unix)]
#[test]
fn imported_append_sets_an_old_file_to_0600() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    fs::write(&sp.imported, "").unwrap();
    set_mode(&sp.imported, 0o644);
    imported_append(&sp, u(1)).unwrap();
    assert_eq!(mode(&sp.imported), 0o600);
}

#[test]
fn imported_load_ignores_a_torn_last_line() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    let whole = format!("{}\n{}\n", u(1), u(2));
    let s3 = u(3).to_string();
    // a torn tail: part of an entry, then a whole entry without its newline
    for tail in [&s3[..10], &s3[..]] {
        fs::write(&sp.imported, format!("{whole}{tail}")).unwrap();
        assert_eq!(imported_load(&sp).unwrap(), set(&[u(1), u(2)]), "{tail:?}");
    }
    // a complete line that is not a ULID is skipped
    fs::write(&sp.imported, format!("{whole}not-a-ulid\n")).unwrap();
    assert_eq!(imported_load(&sp).unwrap(), set(&[u(1), u(2)]));
    // a `\r\n` line ending is read
    fs::write(&sp.imported, format!("{}\r\n{}\n", u(1), u(2))).unwrap();
    assert_eq!(imported_load(&sp).unwrap(), set(&[u(1), u(2)]));
    // an append after a torn tail starts a new line: the new entry is read, the fragment is not
    let frag = &s3[..10];
    fs::write(&sp.imported, format!("{whole}{frag}")).unwrap();
    imported_append(&sp, u(4)).unwrap();
    assert_eq!(
        fs::read_to_string(&sp.imported).unwrap(),
        format!("{whole}{frag}\n{}\n", u(4))
    );
    assert_eq!(imported_load(&sp).unwrap(), set(&[u(1), u(2), u(4)]));
}

fn good_state() -> State {
    let h = hex::encode(Sha256::digest(
        "https://example.com/team/repo.git".as_bytes(),
    ));
    State {
        last_ok: Some(1_700_000_000),
        last_error: Some("push".into()),
        public: Some(PublicCache {
            url_sha256: vec![h],
            verdict: "private".into(),
            at: 1_700_000_000,
        }),
        pushed: BTreeMap::from([(
            "ack/a.json".into(),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".into(),
        )]),
        pending: BTreeMap::from([(
            "registry/a.json".into(),
            "d00491fd7e5bb6fa28c517a0bb32b8b506539d4d".into(),
        )]),
        ci_note_shown: true,
        // FX-30: a delivery that the import began and no pass end counted yet (ULID → pane key).
        pending_wake: BTreeMap::from([("01HF0000000000000000000000".into(), "kbobbb222".into())]),
    }
}

fn bad_states() -> Vec<(&'static str, State)> {
    let mut bad: Vec<(&'static str, State)> = Vec::new();
    let mut s = good_state();
    s.last_error = Some("https://u:tok@h/x".into());
    bad.push(("last_error url", s));
    // FX-30: a `pending_wake` ULID that is not canonical, or a key that is not a pane key (AU84).
    for (id, key) in [
        ("01hf0000000000000000000000", "kbobbb222"),
        ("01HF000000000000000000000", "kbobbb222"),
        ("bob@b", "kbobbb222"),
        ("01HF0000000000000000000000", "../x"),
        ("01HF0000000000000000000000", "bob"),
        ("01HF0000000000000000000000", ""),
    ] {
        let mut s = good_state();
        s.pending_wake.insert(id.into(), key.into());
        bad.push(("pending_wake", s));
    }
    for e in ["", "Push", "push failed", "push\n", &"a".repeat(65)] {
        let mut s = good_state();
        s.last_error = Some(e.into());
        bad.push(("last_error", s));
    }
    for v in ["", "Public", "https://u:tok@h/x", "maybe"] {
        let mut s = good_state();
        s.public.as_mut().unwrap().verdict = v.into();
        bad.push(("verdict", s));
    }
    for h in [
        "https://example.com/team/repo.git",
        &"A".repeat(64),
        &"a".repeat(63),
        &"a".repeat(65),
    ] {
        let mut s = good_state();
        s.public.as_mut().unwrap().url_sha256.push(h.into());
        bad.push(("url_sha256", s));
    }
    for (k, v) in [
        (
            "https://u:tok@h/x",
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
        ),
        ("ack/A.json", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
        ("ack/../x.json", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
        ("other/a.json", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
        ("ack/a.json", "https://u:tok@h/x"),
        ("ack/a.json", "E69DE29BB2D1D6434B8B29AE775AD8C2E48C5391"),
        ("ack/a.json", "e69de29"),
    ] {
        let mut s = good_state();
        s.pushed.insert(k.into(), v.into());
        bad.push(("pushed", s.clone()));
        let mut s = good_state();
        s.pending.insert(k.into(), v.into());
        bad.push(("pending", s));
    }
    bad
}

#[test]
fn state_round_trip_holds_no_url() {
    let (_t, sp) = setup();
    let s = good_state();
    let h = s.public.as_ref().unwrap().url_sha256[0].clone();
    save(&sp, &s).unwrap();
    assert_eq!(load(&sp).unwrap(), s);
    let text = fs::read_to_string(&sp.state).unwrap();
    assert!(text.contains(&h), "{text}");
    for no in ["https://", "example.com", "user", "pw@"] {
        assert!(!text.contains(no), "{no:?} in {text}");
    }
    assert_mode(&sp.state, 0o600);
    assert_mode(&sp.sync, 0o700);
    assert_eq!(names(&sp.sync), ["state.json"]);
    // every verdict of S§8.2 is kept
    for v in ["public", "private", "local", "unknown"] {
        let mut s = good_state();
        s.public.as_mut().unwrap().verdict = v.into();
        save(&sp, &s).unwrap();
        assert_eq!(load(&sp).unwrap(), s, "{v}");
    }
}

#[test]
fn save_refuses_a_value_outside_the_spec() {
    let (_t, sp) = setup();
    save(&sp, &good_state()).unwrap();
    let before = fs::read(&sp.state).unwrap();
    let bad = bad_states();
    for (what, s) in &bad {
        let e = save(&sp, s).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{what}: {s:?}");
        assert_eq!(
            fs::read(&sp.state).unwrap(),
            before,
            "{what}: the file is unchanged"
        );
    }
    assert_eq!(names(&sp.sync), ["state.json"]);
}

#[test]
fn load_gives_default_for_a_missing_file() {
    let (_t, sp) = setup();
    assert_eq!(load(&sp).unwrap(), State::default());
    // missing fields take their defaults; unknown fields are ignored
    fs::create_dir_all(&sp.sync).unwrap();
    fs::write(&sp.state, "{\"last_ok\": 5, \"later\": true}").unwrap();
    assert_eq!(
        load(&sp).unwrap(),
        State {
            last_ok: Some(5),
            ..State::default()
        }
    );
    fs::write(&sp.state, "{\"public\": {\"verdict\": \"local\"}}").unwrap();
    assert_eq!(
        load(&sp).unwrap(),
        State {
            public: Some(PublicCache {
                verdict: "local".into(),
                ..PublicCache::default()
            }),
            ..State::default()
        }
    );
}

#[test]
fn load_refuses_a_bad_file_and_leaves_it() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    for bad in ["", "{", "[]", "{\"pushed\": 3}"] {
        fs::write(&sp.state, bad).unwrap();
        assert_eq!(
            load(&sp).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData,
            "{bad:?}"
        );
        assert_eq!(fs::read_to_string(&sp.state).unwrap(), bad);
    }
}

/// MS-26: an empty `public` object has no `verdict`, which is `""`; `check_state` refuses it, so `load` refuses the
/// file (the `PublicCache` doc comment).
#[test]
fn load_refuses_a_public_cache_without_a_verdict() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    fs::write(&sp.state, br#"{"public": {}}"#).unwrap();
    let e = load(&sp).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData, "{e}");
    assert_eq!(fs::read(&sp.state).unwrap(), br#"{"public": {}}"#);
}

#[test]
fn load_refuses_a_value_outside_the_spec() {
    let (_t, sp) = setup();
    fs::create_dir_all(&sp.sync).unwrap();
    for (what, s) in bad_states() {
        let bytes = serde_json::to_vec_pretty(&s).unwrap();
        fs::write(&sp.state, &bytes).unwrap();
        let e = load(&sp).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData, "{what}: {s:?}");
        assert_eq!(
            fs::read(&sp.state).unwrap(),
            bytes,
            "{what}: the file is unchanged"
        );
    }
    fs::write(&sp.state, serde_json::to_vec_pretty(&good_state()).unwrap()).unwrap();
    let s = load(&sp).unwrap();
    assert_eq!(s, good_state());
    save(&sp, &s).unwrap();
}

#[test]
fn take_again_is_true_once_after_mark_again() {
    let (_t, sp) = setup();
    assert!(!take_again(&sp).unwrap());
    mark_again(&sp).unwrap();
    assert_eq!(fs::read(&sp.again).unwrap(), b"");
    assert_mode(&sp.again, 0o600);
    assert_mode(&sp.sync, 0o700);
    assert!(take_again(&sp).unwrap());
    assert!(!sp.again.exists());
    assert!(!take_again(&sp).unwrap());
    mark_again(&sp).unwrap();
    mark_again(&sp).unwrap(); // two marks merge into one look
    assert!(take_again(&sp).unwrap());
    assert!(!take_again(&sp).unwrap());
}

#[test]
fn last_start_round_trip() {
    let (_t, sp) = setup();
    assert_eq!(read_last_start(&sp), None);
    write_last_start(&sp, 1_700_000_060).unwrap();
    assert_eq!(read_last_start(&sp), Some(1_700_000_060));
    assert_eq!(fs::read_to_string(&sp.last_start).unwrap(), "1700000060");
    assert_mode(&sp.last_start, 0o600);
    assert_mode(&sp.sync, 0o700);
    write_last_start(&sp, 7).unwrap();
    assert_eq!(read_last_start(&sp), Some(7));
    for bad in ["", "x", "-1", "1.5"] {
        fs::write(&sp.last_start, bad).unwrap();
        assert_eq!(read_last_start(&sp), None, "{bad:?}");
    }
}
