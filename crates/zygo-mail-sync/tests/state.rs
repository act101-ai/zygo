//! MS-5: the sync state files (mail sync design S§5, S§7.5, S§8.3, S§8.4, S§8.5, S§15; S§16 W§7.1, M-C4).
//! No test depends on wall-clock time.
#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    let sp = SyncPaths::new(&paths(t.path()), ID);
    (t, sp)
}

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
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

#[test]
fn sync_paths_follow_w71() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let sp = SyncPaths::new(&p, ID);
    let m = p.workspaces_dir().join(ID).join("mail");
    assert_eq!(sp.mail, m);
    assert_eq!(sp.mail, zygo_workspace::mail::mail_dir(&p, ID));
    assert_eq!(
        (sp.outbox.clone(), sp.outbox_tmp.clone(), sp.refused.clone()),
        (
            m.join("outbox"),
            m.join("outbox/tmp"),
            m.join("outbox/refused")
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
        t.path().join("cache/workspaces").join(ID).join("mail-sync")
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
fn write_atomic_replaces_0600_in_0700_folders() {
    let (_t, sp) = setup();
    let f = sp.sync.join("x");
    write_atomic(&f, b"one").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    write_atomic(&f, b"two").unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"two");
    assert_eq!(mode(&f), 0o600);
    assert_eq!((mode(&sp.mail), mode(&sp.sync)), (0o700, 0o700));
    assert_eq!(names(&sp.sync), ["x"]); // no temp file left
    write_atomic(&sp.outbox_tmp.join("y"), b"").unwrap();
    assert_eq!((mode(&sp.outbox), mode(&sp.outbox_tmp)), (0o700, 0o700));
}

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
    assert_eq!(mode(&sp.identity), 0o600);
    assert_eq!(mode(&sp.sync), 0o700);
    assert_eq!(identity(&sp).unwrap(), Some(a.clone()));
    assert_eq!(make_identity(&sp).unwrap(), a);
    // an existing identity is kept, not replaced
    let fixed = "00112233445566778899aabbccddeeff";
    fs::write(&sp.identity, format!("{fixed}\n")).unwrap();
    assert_eq!(identity(&sp).unwrap().as_deref(), Some(fixed));
    assert_eq!(make_identity(&sp).unwrap(), fixed);
    assert_eq!(
        fs::read_to_string(&sp.identity).unwrap(),
        format!("{fixed}\n")
    );
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
    assert_eq!(
        imported_load(&sp).unwrap(),
        [u(1), u(2), u(3)].into_iter().collect::<HashSet<_>>()
    );
    assert_eq!(mode(&sp.imported), 0o600);
    assert_eq!(mode(&sp.sync), 0o700);
    imported_append(&sp, u(1)).unwrap(); // never trimmed; a repeat is one more line
    assert_eq!(fs::metadata(&sp.imported).unwrap().len(), 27 * 4);
    assert_eq!(imported_load(&sp).unwrap().len(), 3);
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
        assert_eq!(
            imported_load(&sp).unwrap(),
            [u(1), u(2)].into_iter().collect::<HashSet<_>>(),
            "{tail:?}"
        );
    }
    // a complete line that is not a ULID is skipped
    fs::write(&sp.imported, format!("{whole}not-a-ulid\n")).unwrap();
    assert_eq!(
        imported_load(&sp).unwrap(),
        [u(1), u(2)].into_iter().collect::<HashSet<_>>()
    );
    // an append after a torn tail starts a new line: the new entry is read, the fragment is not
    let frag = &s3[..10];
    fs::write(&sp.imported, format!("{whole}{frag}")).unwrap();
    imported_append(&sp, u(4)).unwrap();
    assert_eq!(
        fs::read_to_string(&sp.imported).unwrap(),
        format!("{whole}{frag}\n{}\n", u(4))
    );
    assert_eq!(
        imported_load(&sp).unwrap(),
        [u(1), u(2), u(4)].into_iter().collect::<HashSet<_>>()
    );
}

#[test]
fn state_round_trip_holds_no_url() {
    let (_t, sp) = setup();
    let url = "https://user:pw@example.com/team/repo.git";
    let h = hex::encode(Sha256::digest(
        "https://example.com/team/repo.git".as_bytes(),
    ));
    let s = State {
        last_ok: Some(1_700_000_000),
        last_error: Some("push".into()),
        public: Some(PublicCache {
            url_sha256: vec![h.clone()],
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
    };
    save(&sp, &s).unwrap();
    assert_eq!(load(&sp), s);
    let text = fs::read_to_string(&sp.state).unwrap();
    assert!(text.contains(&h), "{text}");
    for no in [url, "https://", "example.com", "user", "pw@"] {
        assert!(!text.contains(no), "{no:?} in {text}");
    }
    assert_eq!(mode(&sp.state), 0o600);
    assert_eq!(mode(&sp.sync), 0o700);
    assert_eq!(names(&sp.sync), ["state.json"]);
}

#[test]
fn load_gives_default_for_missing_or_bad_file() {
    let (_t, sp) = setup();
    assert_eq!(load(&sp), State::default());
    fs::create_dir_all(&sp.sync).unwrap();
    for bad in ["", "{", "[]", "{\"pushed\": 3}"] {
        fs::write(&sp.state, bad).unwrap();
        assert_eq!(load(&sp), State::default(), "{bad:?}");
    }
    // missing fields take their defaults; unknown fields are ignored
    fs::write(&sp.state, "{\"last_ok\": 5, \"later\": true}").unwrap();
    assert_eq!(
        load(&sp),
        State {
            last_ok: Some(5),
            ..State::default()
        }
    );
}

#[test]
fn take_again_is_true_once_after_mark_again() {
    let (_t, sp) = setup();
    assert!(!take_again(&sp).unwrap());
    mark_again(&sp).unwrap();
    assert_eq!(fs::read(&sp.again).unwrap(), b"");
    assert_eq!(mode(&sp.again), 0o600);
    assert_eq!(mode(&sp.sync), 0o700);
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
    assert_eq!(mode(&sp.last_start), 0o600);
    assert_eq!(mode(&sp.sync), 0o700);
    write_last_start(&sp, 7).unwrap();
    assert_eq!(read_last_start(&sp), Some(7));
    for bad in ["", "x", "-1", "1.5"] {
        fs::write(&sp.last_start, bad).unwrap();
        assert_eq!(read_last_start(&sp), None, "{bad:?}");
    }
}
