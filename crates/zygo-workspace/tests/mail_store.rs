#![cfg(unix)]
//! ML-2: the maildir store (M§5, M-C3, M-C4, M§10 "Write fails partway"; S§6.2, S§8.5 steps 1 and 3).
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_workspace::mail::store::{self, Summary};
use zygo_workspace::mail::{ensure_mail_root, mail_dir, MailError, Message, BODY_MAX, SUBJECT_MAX};
use zygo_workspace::state::Store;

const ID: &str = "0123456789abcdef";
const KEY: &str = "kabcdefgh";
const OTHER: &str = "kzyxwv234";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const KEEP: Duration = Duration::from_secs(30 * 86_400);
const HOUR: Duration = Duration::from_secs(3600);
const S1: Duration = Duration::from_secs(1);
const ULID: &str = "01JDQ2Y7Z8K4M5N6P7Q8R9S0TV";

fn at(secs: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(secs) }
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}
/// A temporary folder and the mail folder of workspace `ID` in it (not created).
fn setup() -> (TempDir, PathBuf) { let t = TempDir::new().unwrap(); let m = mail_dir(&paths(t.path()), ID); (t, m) }
fn msg(subject: &str, secs: u64) -> Message { Message::new("lead", "elena", subject, "hello", at(secs)).unwrap() }
fn sub(m: &Path, key: &str, s: &str) -> PathBuf { m.join(key).join(s) }
fn file(m: &Path, key: &str, s: &str, id: &str) -> PathBuf { sub(m, key, s).join(format!("{id}.json")) }
/// The sorted file names in `dir`; empty when `dir` is missing.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).map(|r| r.map(|e| e.unwrap().file_name().into_string().unwrap()).collect())
        .unwrap_or_default();
    v.sort();
    v
}
fn mode(p: &Path) -> u32 { fs::metadata(p).unwrap().permissions().mode() & 0o777 }
fn mtime(p: &Path) -> SystemTime { fs::metadata(p).unwrap().modified().unwrap() }
/// Puts `n` valid message files straight into `mail/<key>/new/`, named `Ulid::from_parts(i, 0)`.
fn fill_new(m: &Path, key: &str, n: u64) {
    fs::create_dir_all(sub(m, key, "new")).unwrap();
    for i in 0..n {
        let mut x = msg("fill", T0);
        x.id = ulid::Ulid::from_parts(i, 0).to_string();
        fs::write(file(m, key, "new", &x.id), serde_json::to_vec(&x).unwrap()).unwrap();
    }
}
/// Writes `bytes` with `write_synced` at `mail/sync/incoming/<KEY>.<id>.json` (the S§8.5 step 1 place).
fn stage(m: &Path, id: &str, bytes: &[u8]) -> PathBuf {
    let dir = m.join("sync").join("incoming");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{KEY}.{id}.json"));
    store::write_synced(&p, bytes).unwrap();
    p
}

#[test]
fn message_new_fills_the_version_1_fields() {
    let m = Message::new("lead", "elena", "Plan", "Body\n", at(T0)).unwrap();
    assert_eq!(m.version, 1);
    assert_eq!((m.from.as_str(), m.to.as_str(), m.subject.as_str(), m.body.as_str()), ("lead", "elena", "Plan", "Body\n"));
    assert_eq!(m.sent_at, "2023-11-14T22:13:20Z");
    assert_eq!(m.id.len(), 26);
    assert!(m.id.bytes().all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !b"ILOU".contains(&c))));
    assert_eq!(ulid::Ulid::from_string(&m.id).unwrap().timestamp_ms(), T0 * 1000);
    assert!(m.extra.is_empty());
    let v = serde_json::to_value(&m).unwrap();
    let keys: BTreeSet<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["body", "from", "id", "sent_at", "subject", "to", "version"]));
    // Whole seconds, UTC.
    assert_eq!(Message::new("a", "b", "s", "", at(T0) + Duration::from_millis(1500)).unwrap().sent_at, "2023-11-14T22:13:21Z");
    // ULID order follows `now`.
    assert!(msg("a", T0).id < msg("b", T0 + 1).id);
}

#[test]
fn message_new_refuses_a_subject_over_200_bytes() {
    assert_eq!(SUBJECT_MAX, 200);
    assert!(Message::new("a", "b", &"s".repeat(200), "", at(T0)).is_ok());
    assert!(Message::new("a", "b", &"é".repeat(100), "", at(T0)).is_ok());
    let e = Message::new("a", "b", &"s".repeat(201), "", at(T0)).unwrap_err();
    assert!(matches!(e, MailError::SubjectTooLong));
    assert_eq!(e.to_string(), "zygo: subject is longer than 200 bytes");
    // 200 characters, 201 bytes: bytes count.
    assert!(matches!(Message::new("a", "b", &format!("{}é", "s".repeat(199)), "", at(T0)), Err(MailError::SubjectTooLong)));
}

#[test]
fn message_new_refuses_a_body_over_1_mib() {
    assert_eq!(BODY_MAX, 1_048_576);
    assert!(Message::new("a", "b", "s", &"x".repeat(BODY_MAX), at(T0)).is_ok());
    let e = Message::new("a", "b", "s", &"x".repeat(BODY_MAX + 1), at(T0)).unwrap_err();
    assert!(matches!(e, MailError::BodyTooLarge));
    assert_eq!(e.to_string(), "zygo: body is larger than 1 MiB");
}

#[test]
fn mail_dir_is_under_the_workspace_state() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    assert_eq!(mail_dir(&p, ID), Store::open(&p, ID).dir().join("mail"));
    assert!(!mail_dir(&p, ID).exists()); // it creates nothing
}

#[test]
fn a_1_mib_body_arrives_byte_equal() {
    let (_t, m) = setup();
    let unit = "a\"\\\né\u{1}"; // 7 bytes: quote, backslash, newline, two-byte char, control char
    let mut body = unit.repeat(BODY_MAX / unit.len());
    while body.len() < BODY_MAX { body.push('z'); }
    assert_eq!(body.len(), BODY_MAX);
    let x = Message::new("lead", "elena", "big", &body, at(T0)).unwrap();
    store::deliver(&m, KEY, "elena", &x, 500).unwrap();
    let got = store::read(&m, KEY, 10).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].body.as_bytes(), body.as_bytes());
    assert_eq!(got[0], x);
}

#[test]
fn deliver_writes_in_tmp_then_renames_into_new() {
    let (_t, m) = setup();
    let x = msg("s", T0);
    store::deliver(&m, KEY, "elena", &x, 500).unwrap();
    assert!(names(&sub(&m, KEY, "tmp")).is_empty());
    assert_eq!(names(&sub(&m, KEY, "new")), [format!("{}.json", x.id)]);
    let on_disk: Message = serde_json::from_slice(&fs::read(file(&m, KEY, "new", &x.id)).unwrap()).unwrap();
    assert_eq!(on_disk, x);
}

#[test]
fn a_partial_file_in_tmp_is_never_counted_listed_or_read() {
    let (_t, m) = setup();
    let x = msg("s", T0);
    store::deliver(&m, KEY, "elena", &x, 500).unwrap();
    let partial = file(&m, KEY, "tmp", ULID);
    fs::write(&partial, b"{\"version\":1,\"id\":\"").unwrap();
    fs::write(sub(&m, KEY, "new").join("notes.txt"), b"x").unwrap();
    assert_eq!(store::unread(&m, KEY).unwrap(), 1);
    assert_eq!(store::list(&m, KEY).unwrap().iter().map(|s| s.id.clone()).collect::<Vec<_>>(), std::slice::from_ref(&x.id));
    assert_eq!(store::read(&m, KEY, 10).unwrap(), [x]);
    assert!(partial.exists()); // read never touches tmp/
    assert!(sub(&m, KEY, "new").join("notes.txt").exists());
}

#[test]
fn read_moves_oldest_first_and_returns_at_most_max() {
    let (_t, m) = setup();
    let (three, one, two) = (msg("three", T0 + 3), msg("one", T0 + 1), msg("two", T0 + 2));
    for x in [&three, &one, &two] { store::deliver(&m, KEY, "elena", x, 500).unwrap(); }
    let got = store::read(&m, KEY, 2).unwrap();
    assert_eq!(got.iter().map(|x| x.subject.as_str()).collect::<Vec<_>>(), ["one", "two"]);
    assert_eq!(names(&sub(&m, KEY, "new")), [format!("{}.json", three.id)]);
    assert_eq!(names(&sub(&m, KEY, "cur")), [format!("{}.json", one.id), format!("{}.json", two.id)]);
    assert_eq!(store::read(&m, KEY, 10).unwrap(), [three]);
    assert!(store::read(&m, KEY, 10).unwrap().is_empty());
}

#[test]
fn two_readers_and_a_sender_get_each_message_once() {
    let (_t, m) = setup();
    const N: u64 = 60;
    let done = Arc::new(AtomicBool::new(false));
    let sender = { let (m, done) = (m.clone(), done.clone()); thread::spawn(move || {
        let mut ids = BTreeSet::new();
        for i in 0..N {
            let x = msg(&format!("s{i}"), T0 + i);
            store::deliver(&m, KEY, "elena", &x, 500).unwrap();
            ids.insert(x.id);
        }
        done.store(true, Ordering::SeqCst);
        ids
    }) };
    let reader = || { let (m, done) = (m.clone(), done.clone()); thread::spawn(move || {
        let mut got = Vec::new();
        loop {
            let finished = done.load(Ordering::SeqCst); // read before the read: a last empty read after it means new/ is empty
            let r = store::read(&m, KEY, 7).unwrap();
            let empty = r.is_empty();
            got.extend(r.into_iter().map(|x| x.id));
            if finished && empty { break got; }
        }
    }) };
    let (a, b) = (reader(), reader());
    let sent = sender.join().unwrap();
    let mut all = a.join().unwrap();
    all.extend(b.join().unwrap());
    assert_eq!(all.len() as u64, N); // none twice, none lost, none partial (a partial file would fail to parse)
    assert_eq!(all.into_iter().collect::<BTreeSet<_>>(), sent);
    assert!(names(&sub(&m, KEY, "new")).is_empty());
    assert!(names(&sub(&m, KEY, "tmp")).is_empty());
    assert_eq!(names(&sub(&m, KEY, "cur")).len() as u64, N);
}

#[test]
fn the_501st_unread_message_is_refused() {
    let (_t, m) = setup();
    fill_new(&m, KEY, 500);
    let e = store::deliver(&m, KEY, "elena", &msg("x", T0), 500).unwrap_err();
    assert!(matches!(e, MailError::Full { .. }));
    assert_eq!(e.to_string(), "zygo: elena has 500 unread messages");
    assert_eq!(names(&sub(&m, KEY, "new")).len(), 500);
    assert!(names(&sub(&m, KEY, "tmp")).is_empty()); // refused before any write
    assert_eq!(store::read(&m, KEY, 1).unwrap().len(), 1);
    store::deliver(&m, KEY, "elena", &msg("x", T0), 500).unwrap();
    assert_eq!(store::unread(&m, KEY).unwrap(), 500);
}

#[test]
fn deliver_refuses_an_id_already_delivered() {
    let (_t, m) = setup();
    let x = msg("s", T0);
    store::deliver(&m, KEY, "elena", &x, 500).unwrap();
    let e = store::deliver(&m, KEY, "elena", &x, 500).unwrap_err();
    assert!(matches!(e, MailError::Duplicate { .. }));
    assert_eq!(e.to_string(), format!("zygo: message {} was already delivered", x.id));
    assert!(names(&sub(&m, KEY, "tmp")).is_empty());
    store::read(&m, KEY, 10).unwrap();
    assert!(matches!(store::deliver(&m, KEY, "elena", &x, 500), Err(MailError::Duplicate { .. }))); // also when in cur/
    assert!(names(&sub(&m, KEY, "new")).is_empty());
}

#[test]
fn unknown_fields_survive_a_read() {
    let (_t, m) = setup();
    let mut x = msg("s", T0);
    x.extra.insert("thread".into(), json!("t-1"));
    x.extra.insert("zz".into(), json!({"k": [1, 2]}));
    store::deliver(&m, KEY, "elena", &x, 500).unwrap();
    let got = store::read(&m, KEY, 10).unwrap();
    assert_eq!(got[0].extra, x.extra);
    assert_eq!(got[0], x);
    let on_disk: Value = serde_json::from_slice(&fs::read(file(&m, KEY, "cur", &x.id)).unwrap()).unwrap();
    assert_eq!(on_disk["thread"], "t-1");
}

#[test]
fn list_and_unread_move_nothing() {
    let (_t, m) = setup();
    assert!(store::list(&m, KEY).unwrap().is_empty());
    assert_eq!(store::unread(&m, KEY).unwrap(), 0);
    assert!(store::read(&m, KEY, 10).unwrap().is_empty());
    assert!(!m.exists()); // a read of a missing maildir makes nothing
    let (second, first) = (msg("second", T0 + 2), msg("first", T0 + 1));
    store::deliver(&m, KEY, "elena", &second, 500).unwrap();
    store::deliver(&m, KEY, "elena", &first, 500).unwrap();
    assert_eq!(store::list(&m, KEY).unwrap(), [
        Summary { id: first.id.clone(), from: "lead".into(), subject: "first".into() },
        Summary { id: second.id.clone(), from: "lead".into(), subject: "second".into() },
    ]);
    assert_eq!(store::unread(&m, KEY).unwrap(), 2);
    assert_eq!(names(&sub(&m, KEY, "new")).len(), 2);
    assert!(names(&sub(&m, KEY, "cur")).is_empty());
}

#[test]
fn sweep_deletes_old_cur_and_tmp_and_nothing_in_new() {
    let (t, m) = setup();
    store::sweep(&t.path().join("none"), KEEP, at(T0)); // a missing folder: no panic
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    store::read(&m, KEY, 10).unwrap();
    let b = msg("b", T0 + 1);
    store::deliver(&m, KEY, "elena", &b, 500).unwrap();
    let (cur, new, tmp) = (file(&m, KEY, "cur", &a.id), file(&m, KEY, "new", &b.id), file(&m, KEY, "tmp", ULID));
    fs::write(&tmp, b"{\"version\":1,").unwrap();
    let incoming = stage(&m, ULID, b"x"); // S§8.5: incoming/ is not swept
    let (tm, cm) = (mtime(&tmp), mtime(&cur));
    let all = |c: bool, t: bool| { assert_eq!(cur.exists(), c); assert_eq!(tmp.exists(), t); assert!(new.exists()); assert!(incoming.exists()); };
    store::sweep(&m, KEEP, UNIX_EPOCH); all(true, true); // a clock before every file
    store::sweep(&m, KEEP, tm + HOUR - S1); all(true, true);
    store::sweep(&m, KEEP, tm + HOUR + S1); all(true, false);
    store::sweep(&m, KEEP, cm + KEEP - S1); all(true, false);
    store::sweep(&m, KEEP, cm + KEEP + S1); all(false, false);
    store::sweep(&m, KEEP, cm + KEEP * 100); all(false, false);
}

#[test]
fn folders_are_0700_and_files_0600() {
    let (_t, m) = setup();
    let x = msg("x", T0);
    store::deliver(&m, KEY, "elena", &x, 500).unwrap();
    store::read(&m, KEY, 10).unwrap();
    let y = msg("y", T0 + 1);
    store::deliver(&m, KEY, "elena", &y, 500).unwrap();
    for d in [m.clone(), m.join(KEY), sub(&m, KEY, "tmp"), sub(&m, KEY, "new"), sub(&m, KEY, "cur")] {
        assert_eq!(mode(&d), 0o700, "{}", d.display());
    }
    assert_eq!(mode(&file(&m, KEY, "cur", &x.id)), 0o600);
    assert_eq!(mode(&file(&m, KEY, "new", &y.id)), 0o600);
}

#[test]
fn a_bad_pane_key_is_refused_before_any_path_is_built() {
    let (_t, m) = setup();
    let kept = stage(&m, ULID, b"x"); // mail/sync/ exists; "sync" is not a pane key
    for bad in ["", "k", "kabcdefg", "kabcdefghi", "Kabcdefgh", "kabcdefg1", "kabcdefg8", "../kabcdefgh", "kabcd/fgh", "sync"] {
        assert!(matches!(store::deliver(&m, bad, "x", &msg("s", T0), 500), Err(MailError::BadKey)), "{bad:?}");
        assert!(matches!(store::read(&m, bad, 10), Err(MailError::BadKey)), "{bad:?}");
        assert!(matches!(store::list(&m, bad), Err(MailError::BadKey)), "{bad:?}");
        assert!(matches!(store::unread(&m, bad), Err(MailError::BadKey)), "{bad:?}");
        assert_eq!(store::remove_pane(&m, bad).unwrap_err().kind(), ErrorKind::InvalidInput, "{bad:?}");
    }
    assert_eq!(names(&m), ["sync"]); // nothing made, nothing removed
    assert!(kept.exists());
    assert_eq!(MailError::BadKey.to_string(), "zygo: invalid pane key");
}

#[test]
fn remove_pane_deletes_only_that_maildir() {
    let (_t, m) = setup();
    store::deliver(&m, KEY, "elena", &msg("a", T0), 500).unwrap();
    store::deliver(&m, OTHER, "joe", &msg("b", T0), 500).unwrap();
    store::remove_pane(&m, KEY).unwrap();
    assert!(!m.join(KEY).exists());
    assert_eq!(store::unread(&m, OTHER).unwrap(), 1);
    store::remove_pane(&m, KEY).unwrap(); // already gone: Ok
}

#[test]
fn write_synced_creates_new_0600_and_refuses_an_existing_file() {
    let t = TempDir::new().unwrap();
    let p = t.path().join("f.json");
    store::write_synced(&p, b"{ \"a\" : 1 }\n").unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"{ \"a\" : 1 }\n");
    assert_eq!(mode(&p), 0o600);
    assert_eq!(store::write_synced(&p, b"x").unwrap_err().kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&p).unwrap(), b"{ \"a\" : 1 }\n");
}

#[test]
fn deliver_staged_puts_the_staged_bytes_into_new_unchanged() {
    let (_t, m) = setup();
    let bytes = format!("{{ \"to\" :\"elena@mac\",\n  \"zz\": {{\"k\": [1, 2]}},\"version\":1, \"body\":\"b\",\"subject\":\"s\",\
        \"from\":\"lead@mac\",\"sent_at\":\"2023-11-14T22:13:20Z\",  \"id\":\"{ULID}\"}}\n");
    let staged = stage(&m, ULID, bytes.as_bytes());
    assert_eq!(mode(&staged), 0o600);
    assert!(store::deliver_staged(&m, KEY, ULID, &staged, 500).unwrap());
    assert!(!staged.exists());
    assert_eq!(fs::read(file(&m, KEY, "new", ULID)).unwrap(), bytes.as_bytes());
    assert_eq!(mode(&file(&m, KEY, "new", ULID)), 0o600);
    let got = store::read(&m, KEY, 10).unwrap();
    assert_eq!(got[0].extra["zz"], json!({"k": [1, 2]}));
    assert_eq!(fs::read(file(&m, KEY, "cur", ULID)).unwrap(), bytes.as_bytes()); // read never rewrites
}

#[test]
fn deliver_staged_refuses_a_bad_key_or_id_with_no_rename() {
    let (_t, m) = setup();
    let staged = stage(&m, ULID, b"{}");
    assert!(matches!(store::deliver_staged(&m, "../x", ULID, &staged, 500), Err(MailError::BadKey)));
    for bad in ["", "01JDQ2Y7Z8K4M5N6P7Q8R9S0T", "01JDQ2Y7Z8K4M5N6P7Q8R9S0TVX", "01jdq2y7z8k4m5n6p7q8r9s0tv",
                "01JDQ2Y7Z8K4M5N6P7Q8R9S0TI", "01JDQ2Y7Z8K4M5N6P7Q8R9S0TU", "../../../../../../../x.json"] {
        let e = store::deliver_staged(&m, KEY, bad, &staged, 500).unwrap_err();
        assert!(matches!(e, MailError::BadId), "{bad:?}");
    }
    assert_eq!(MailError::BadId.to_string(), "zygo: invalid message id");
    let mut x = msg("s", T0);
    x.id = "../x".into();
    assert!(matches!(store::deliver(&m, KEY, "elena", &x, 500), Err(MailError::BadId)));
    assert!(staged.exists());
    assert!(!m.join(KEY).exists());
}

#[test]
fn deliver_staged_skips_an_id_already_in_new_or_cur() {
    for s in ["new", "cur"] {
        let (_t, m) = setup();
        fs::create_dir_all(sub(&m, KEY, s)).unwrap();
        fs::write(file(&m, KEY, s, ULID), b"old").unwrap();
        let staged = stage(&m, ULID, b"{\"new\":true}");
        assert!(!store::deliver_staged(&m, KEY, ULID, &staged, 500).unwrap(), "{s}");
        assert!(!staged.exists(), "{s}");
        assert_eq!(fs::read(file(&m, KEY, s, ULID)).unwrap(), b"old");
        if s == "cur" { assert!(!file(&m, KEY, "new", ULID).exists()); }
    }
}

#[test]
fn deliver_staged_leaves_the_staged_file_when_new_is_full() {
    let (_t, m) = setup();
    fill_new(&m, KEY, 500);
    let staged = stage(&m, ULID, b"{}");
    let e = store::deliver_staged(&m, KEY, ULID, &staged, 500).unwrap_err();
    assert!(matches!(e, MailError::Full { .. }));
    assert_eq!(e.to_string(), format!("zygo: {KEY} has 500 unread messages"));
    assert!(staged.exists());
    assert_eq!(names(&sub(&m, KEY, "new")).len(), 500);
}

#[test]
fn deliver_staged_never_overwrites_an_existing_file_in_new() {
    // Lead's change: the move into new/ is a no-replace link, not a check then a rename.
    let (_t, m) = setup();
    fs::create_dir_all(sub(&m, KEY, "new")).unwrap();
    let old = b"{\"old\":\"bytes that differ\"}\n";
    fs::write(file(&m, KEY, "new", ULID), old).unwrap();
    let staged = stage(&m, ULID, b"{\"new\":true}");
    assert!(!store::deliver_staged(&m, KEY, ULID, &staged, 500).unwrap());
    assert!(!staged.exists());
    assert_eq!(fs::read(file(&m, KEY, "new", ULID)).unwrap(), old);
    assert_eq!(names(&sub(&m, KEY, "new")), [format!("{ULID}.json")]);
}

// Review round 1.

/// Sets the modification time of `p` to `t`.
fn set_mtime(p: &Path, t: SystemTime) { fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap(); }
fn mkfifo(p: &Path) {
    let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
}
/// A valid id that sorts after every id `msg` makes.
const LATE: &str = "7ZZZZZZZZZZZZZZZZZZZZZZZZZ";
const EARLY: &str = "00000000000000000000000000";

#[test]
fn read_keeps_moved_messages_when_a_later_entry_cannot_be_read() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    fs::create_dir(file(&m, KEY, "new", LATE)).unwrap(); // a folder named <ULID>.json
    let unreadable = file(&m, KEY, "new", "7ZZZZZZZZZZZZZZZZZZZZZZZZY");
    fs::write(&unreadable, b"{}").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(store::read(&m, KEY, 10).unwrap(), std::slice::from_ref(&a));
    assert!(file(&m, KEY, "cur", &a.id).exists());
    assert!(file(&m, KEY, "cur", "7ZZZZZZZZZZZZZZZZZZZZZZZZY").exists()); // moved, then skipped: left for the sweep
    assert!(file(&m, KEY, "new", LATE).is_dir()); // not a file: not moved
    assert!(store::read(&m, KEY, 10).unwrap().is_empty());
}

#[test]
fn read_never_replaces_a_file_in_cur() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    fs::create_dir_all(sub(&m, KEY, "cur")).unwrap();
    fs::write(file(&m, KEY, "cur", &a.id), b"OLD").unwrap();
    assert!(store::read(&m, KEY, 10).unwrap().is_empty()); // a duplicate: removed from new/, not returned
    assert_eq!(fs::read(file(&m, KEY, "cur", &a.id)).unwrap(), b"OLD");
    assert!(names(&sub(&m, KEY, "new")).is_empty());
}

#[test]
fn a_fifo_in_new_is_skipped_without_blocking() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    let fifo = file(&m, KEY, "new", LATE);
    mkfifo(&fifo);
    assert_eq!(store::unread(&m, KEY).unwrap(), 1);
    assert_eq!(store::list(&m, KEY).unwrap().len(), 1);
    assert_eq!(store::read(&m, KEY, 10).unwrap(), [a]);
    assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo()); // left where it is
    assert!(!file(&m, KEY, "cur", LATE).exists());
}

#[test]
fn a_symlink_in_new_is_not_followed_returned_or_moved() {
    let (t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    let mut planted = msg("planted", T0 + 1);
    planted.id = LATE.into();
    let outside = t.path().join("outside.json");
    fs::write(&outside, serde_json::to_vec(&planted).unwrap()).unwrap();
    let link = file(&m, KEY, "new", LATE);
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert_eq!(store::unread(&m, KEY).unwrap(), 1);
    assert_eq!(store::list(&m, KEY).unwrap().iter().map(|s| s.id.clone()).collect::<Vec<_>>(), std::slice::from_ref(&a.id));
    assert_eq!(store::read(&m, KEY, 10).unwrap(), [a]);
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert!(fs::symlink_metadata(file(&m, KEY, "cur", LATE)).is_err());
    assert!(outside.exists());
}

#[test]
fn a_folder_named_like_a_message_does_not_break_list_or_unread() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    fs::create_dir(file(&m, KEY, "new", LATE)).unwrap();
    assert_eq!(store::list(&m, KEY).unwrap().iter().map(|s| s.id.clone()).collect::<Vec<_>>(), [a.id]);
    assert_eq!(store::unread(&m, KEY).unwrap(), 1);
}

#[test]
fn a_file_over_8_mib_is_not_listed_or_returned() {
    assert_eq!(store::FILE_MAX, 8 * 1024 * 1024);
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    let mut big = msg("big", T0 + 1);
    big.id = LATE.into();
    big.body = "x".repeat(store::FILE_MAX as usize);
    fs::write(file(&m, KEY, "new", LATE), serde_json::to_vec(&big).unwrap()).unwrap();
    assert_eq!(store::list(&m, KEY).unwrap().len(), 1);
    assert_eq!(store::read(&m, KEY, 10).unwrap(), [a]);
}

#[test]
fn existing_loose_folders_are_set_to_0700() {
    let (_t, m) = setup();
    let all = [m.clone(), m.join(KEY), sub(&m, KEY, "tmp"), sub(&m, KEY, "new"), sub(&m, KEY, "cur")];
    for d in &all { fs::create_dir_all(d).unwrap(); fs::set_permissions(d, fs::Permissions::from_mode(0o755)).unwrap(); }
    store::deliver(&m, KEY, "elena", &msg("a", T0), 500).unwrap();
    for d in &all { assert_eq!(mode(d), 0o700, "{}", d.display()); }
}

#[test]
fn ensure_mail_root_makes_and_fixes_mail_at_0700() {
    let (_t, m) = setup();
    ensure_mail_root(&m).unwrap();
    assert_eq!(mode(&m), 0o700);
    fs::set_permissions(&m, fs::Permissions::from_mode(0o755)).unwrap();
    ensure_mail_root(&m).unwrap();
    assert_eq!(mode(&m), 0o700);
}

#[test]
fn a_symlinked_maildir_folder_is_refused() {
    let (t, m) = setup();
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(m.join(KEY)).unwrap();
    std::os::unix::fs::symlink(&outside, sub(&m, KEY, "new")).unwrap();
    assert!(matches!(store::deliver(&m, KEY, "elena", &msg("a", T0), 500), Err(MailError::Io(_))));
    assert_eq!(mode(&outside), 0o755);
    assert!(names(&outside).is_empty());
}

#[test]
fn a_read_message_is_aged_from_its_read() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    let planted = at(T0);
    set_mtime(&file(&m, KEY, "new", &a.id), planted); // it waited in new/ since T0
    store::read(&m, KEY, 10).unwrap();
    assert!(mtime(&file(&m, KEY, "cur", &a.id)) > planted);
    store::sweep(&m, KEEP, planted + KEEP + S1);
    assert!(file(&m, KEY, "cur", &a.id).exists());
}

#[test]
fn deliver_over_a_leftover_tmp_file_is_a_duplicate() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    fs::create_dir_all(sub(&m, KEY, "tmp")).unwrap();
    fs::write(file(&m, KEY, "tmp", &a.id), b"{\"version\":1,").unwrap();
    let e = store::deliver(&m, KEY, "elena", &a, 500).unwrap_err();
    assert!(matches!(e, MailError::Duplicate { .. }), "{e}");
    assert_eq!(fs::read(file(&m, KEY, "tmp", &a.id)).unwrap(), b"{\"version\":1,");
}

#[test]
fn sweep_never_follows_a_symlinked_cur_or_tmp() {
    let (t, m) = setup();
    store::deliver(&m, KEY, "elena", &msg("a", T0), 500).unwrap();
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let old = outside.join(format!("{ULID}.json"));
    fs::write(&old, b"x").unwrap();
    set_mtime(&old, at(T0));
    for s in ["cur", "tmp"] {
        fs::remove_dir(sub(&m, KEY, s)).unwrap();
        std::os::unix::fs::symlink(&outside, sub(&m, KEY, s)).unwrap();
    }
    store::sweep(&m, KEEP, at(T0) + KEEP * 100);
    assert!(old.exists());
}

#[test]
fn sweep_removes_an_old_symlink_in_cur_but_not_its_target() {
    let (t, m) = setup();
    store::deliver(&m, KEY, "elena", &msg("a", T0), 500).unwrap();
    store::read(&m, KEY, 10).unwrap();
    let target = t.path().join("target.json");
    fs::write(&target, b"x").unwrap();
    let link = file(&m, KEY, "cur", ULID);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let lm = fs::symlink_metadata(&link).unwrap().modified().unwrap();
    store::sweep(&m, KEEP, lm + KEEP * 100);
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(target.exists());
}

#[test]
fn read_counts_only_returned_messages_toward_max() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    store::deliver(&m, KEY, "elena", &a, 500).unwrap();
    fs::write(file(&m, KEY, "new", EARLY), b"not json").unwrap();
    assert_eq!(store::read(&m, KEY, 1).unwrap(), [a]);
    assert!(file(&m, KEY, "cur", EARLY).exists());
}

#[test]
fn a_failed_delivery_leaves_the_message_in_tmp_and_new_unchanged() {
    let (_t, m) = setup();
    let a = msg("a", T0);
    fs::create_dir_all(sub(&m, KEY, "tmp")).unwrap();
    fs::write(sub(&m, KEY, "new"), b"a file, not a folder").unwrap();
    assert!(matches!(store::deliver(&m, KEY, "elena", &a, 500), Err(MailError::Io(_))));
    let staged = file(&m, KEY, "tmp", &a.id);
    store::write_synced(&staged, &serde_json::to_vec(&a).unwrap()).unwrap();
    assert!(matches!(store::deliver_staged(&m, KEY, &a.id, &staged, 500), Err(MailError::Io(_))));
    assert!(staged.exists());
    assert_eq!(fs::read(sub(&m, KEY, "new")).unwrap(), b"a file, not a folder");
}
