# ML-2 — Maildir store: message file, atomic send, read by rename, limits, sweep

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C2, M-C3, M-C4; M§5; M§10 row "Write fails partway"; `docs/specs/2026-11-24-mail-sync-design.md` S§6.2, S§8.5 steps 1 and 3 (the byte-preserving delivery that mail sync uses)
**Files:** `crates/zygo-workspace/src/lib.rs`, `crates/zygo-workspace/src/mail/mod.rs`, `crates/zygo-workspace/src/mail/store.rs`, `crates/zygo-workspace/tests/mail_store.rs`, `crates/zygo-workspace/Cargo.toml`, `Cargo.lock`
**Consumes:** —
**Produces:** `zygo_workspace::mail::Message { version: u32, id: String, from: String, to: String, sent_at: String, subject: String, body: String, extra: serde_json::Map<String, serde_json::Value> }`, `zygo_workspace::mail::Message::new(from: &str, to: &str, subject: &str, body: &str, now: SystemTime) -> Result<Message, MailError>`, `zygo_workspace::mail::MailError` (enum; `Display` gives the spec's user lines), `zygo_workspace::mail::SUBJECT_MAX: usize` (200), `zygo_workspace::mail::BODY_MAX: usize` (1048576), `zygo_workspace::mail::mail_dir(paths: &Paths, ws_id: &str) -> PathBuf`, `zygo_workspace::mail::store::deliver(mail_dir: &Path, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError>`, `zygo_workspace::mail::store::read(mail_dir: &Path, key: &str, max: usize) -> Result<Vec<Message>, MailError>`, `zygo_workspace::mail::store::Summary { id: String, from: String, subject: String }`, `zygo_workspace::mail::store::list(mail_dir: &Path, key: &str) -> Result<Vec<Summary>, MailError>`, `zygo_workspace::mail::store::unread(mail_dir: &Path, key: &str) -> Result<usize, MailError>`, `zygo_workspace::mail::store::sweep(mail_dir: &Path, keep: Duration, now: SystemTime)`, `zygo_workspace::mail::store::remove_pane(mail_dir: &Path, key: &str) -> std::io::Result<()>`, `zygo_workspace::mail::store::write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()>` (the M§5 file write: create-new, `0600`, the bytes unchanged, `fsync` of the file and its folder), `zygo_workspace::mail::store::deliver_staged(mail_dir: &Path, key: &str, id: &str, staged: &Path, max_unread: u32) -> Result<bool, MailError>` (the M§5 rename into `mail/<key>/new/<id>.json` of a file the caller wrote with `write_synced` under `mail_dir`; checks the key, the ULID `id` and the unread count first; never reads or rewrites the bytes; `Ok(false)` and the staged file removed when `new/<id>.json` or `cur/<id>.json` already exists; a full `new/` returns the M§5 error and leaves the staged file in place)
**Acceptance:** `cargo test -p zygo-workspace --test mail_store` → all pass: a 1 MiB body arrives byte-equal (M§13 #3); a body of 1 MiB + 1 byte and a subject of 201 bytes are refused; a file appears in `new/` only by its rename from `tmp/`, never as a partial file; read moves `new/` to `cur/` oldest first (ULID order) and returns at most `max`; two readers on one maildir return each message once between them; the 501st unread file is refused with `zygo: <label> has 500 unread messages`; unknown JSON fields survive a read; sweep (injected `now`) deletes `cur/` files older than `keep` and `tmp/` files older than 1 h, and nothing in `new/`; on unix `mail/` and each maildir folder are `0700` and each file `0600`; a pane key that fails `^k[a-z2-7]{8}$` is refused before any path is built; `write_synced` then `deliver_staged` puts a file whose bytes have odd key order, extra whitespace and an unknown field into `new/<ULID>.json` byte-equal; `deliver_staged` refuses a bad key and a non-ULID id with no rename, returns `Ok(false)` and removes the staged file when `new/<ULID>.json` or `cur/<ULID>.json` exists, and with 500 unread leaves the staged file where it is

## Plan

### Premise check (live code at `efd4f49`)

- No mail code exists yet: no `crates/zygo-workspace/src/mail/`, no `mail` module in `crates/zygo-workspace/src/lib.rs` (modules `activity` … `up`, lines 2-14).
- **Drift: `ulid` is not a dependency of `zygo-workspace`.** `crates/zygo-workspace/Cargo.toml` lists `zygo-core`, `zygo-config`, `zygo-work`, `zygo-proxy`, `anyhow`, `serde`, `serde_json`, `sha2`, `hex`, `rand`, `fs4`, `libc`, `time`. `ulid = "3.0.0"` is a workspace dependency (`Cargo.toml` `[workspace.dependencies]`) used by `zygo-core`, `zygo-config` and `zygo-results`. `Message::new` needs `ulid::Ulid::from_datetime(now)` (ulid 3.0.0 `src/time.rs:48`); `zygo_core::ids::ulid()` (`crates/zygo-core/src/ids.rs:2`) calls `Ulid::generate()`, which reads the wall clock, so it cannot take the injected `now`. Step 1 adds one line to `crates/zygo-workspace/Cargo.toml` and updates `Cargo.lock`. Neither file is on the contract's Files line. The crate-graph constraint is not affected: `ulid` is an external crate, not a `zygo-*` crate.
- Existing helpers checked and not reused: `zygo_core::session_dir::write_atomic` (`crates/zygo-core/src/session_dir.rs:137`) writes its temporary file in the target's own folder, replaces an existing target and does not `fsync` the folder; M§5 needs the file in `tmp/`, create-new, and a later rename into `new/`. `zygo_results::store::is_id` (`crates/zygo-results/src/store.rs:94`) checks a ULID, but `zygo-workspace` may not depend on `zygo-results` (crate graph).
- Reused: `zygo_workspace::identity::valid_pane_key(s: &str) -> bool` (`crates/zygo-workspace/src/identity.rs:58`, `^k[a-z2-7]{8}$`); `zygo_workspace::state::Store::open(paths: &Paths, ws_id: &str) -> Store` and `Store::dir(&self) -> &Path` (`crates/zygo-workspace/src/state.rs:413`, `:418`; the folder `<workspaces_dir>/<id>`); `zygo_core::paths::Paths` (`crates/zygo-core/src/paths.rs:5`); `time::format_description::well_known::Rfc3339` (as `crates/zygo-proxy/src/session.rs:252`); the `#[serde(flatten)] extra` pattern for unknown fields (`crates/zygo-workspace/src/state.rs:58`).
- `std::fs::File::sync_all` on a folder opened with `File::open` returns `Ok` on macOS (checked by hand), so the folder `fsync` runs on every unix.

### Interfaces

Consumes: — (from other nodes). From the live code: `valid_pane_key`, `Store::open`, `Store::dir`, `Paths`, as listed above.

Produces (exact):

```rust
// crates/zygo-workspace/src/mail/mod.rs
pub const SUBJECT_MAX: usize = 200;
pub const BODY_MAX: usize = 1048576;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message { pub version: u32, pub id: String, pub from: String, pub to: String, pub sent_at: String,
    pub subject: String, pub body: String, #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value> }
impl Message { pub fn new(from: &str, to: &str, subject: &str, body: &str, now: SystemTime) -> Result<Message, MailError> }
#[derive(Debug)]
pub enum MailError { SubjectTooLong, BodyTooLarge, BadKey, BadId, Full { label: String, max: u32 }, Duplicate { id: String }, Io(std::io::Error) }
pub fn mail_dir(paths: &Paths, ws_id: &str) -> PathBuf

// crates/zygo-workspace/src/mail/store.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary { pub id: String, pub from: String, pub subject: String }
pub fn deliver(mail_dir: &Path, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError>
pub fn read(mail_dir: &Path, key: &str, max: usize) -> Result<Vec<Message>, MailError>
pub fn list(mail_dir: &Path, key: &str) -> Result<Vec<Summary>, MailError>
pub fn unread(mail_dir: &Path, key: &str) -> Result<usize, MailError>
pub fn sweep(mail_dir: &Path, keep: Duration, now: SystemTime)
pub fn remove_pane(mail_dir: &Path, key: &str) -> std::io::Result<()>
pub fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()>
pub fn deliver_staged(mail_dir: &Path, key: &str, id: &str, staged: &Path, max_unread: u32) -> Result<bool, MailError>
```

`MailError` `Display` (each is the whole user line, with the `zygo: ` prefix, as the contract's acceptance and ML-11's acceptance quote such lines):

| Variant | Text |
|---|---|
| `SubjectTooLong` | `zygo: subject is longer than 200 bytes` |
| `BodyTooLarge` | `zygo: body is larger than 1 MiB` |
| `BadKey` | `zygo: invalid pane key` |
| `BadId` | `zygo: invalid message id` |
| `Full { label, max }` | `zygo: {label} has {max} unread messages` (M§5) |
| `Duplicate { id }` | `zygo: message {id} was already delivered` |
| `Io(e)` | `zygo: mail: {e}` |

Consumers and what they rely on: ML-4 (`Message::new`, `deliver`, `mail_dir`, `MailError`; it may add a variant for an unknown target in `mail/mod.rs`), ML-6, ML-10, ML-13, ML-14 (`unread`, `remove_pane`, `mail_dir`), ML-11 and ML-12 (`read`, `list`, `unread`, `sweep`), MS-2 (`Message`, `mail_dir`), MS-12 (`write_synced`, `deliver_staged`, `mail_dir`; it writes the staged file at `mail/sync/incoming/<pane key>.<ULID>.json`, S§8.5 step 1, so the sweep must not touch `mail/sync/`).

### Rules the code follows

- **Pane key first.** Every function that takes `key` checks `valid_pane_key(key)` before it joins `key` to any path: `BadKey` (`remove_pane`: `io::Error` of kind `InvalidInput`). Nothing is created for a bad key.
- **Message id.** `deliver` and `deliver_staged` check the id against `^[0-9A-HJKMNP-TV-Z]{26}$` (S§8.5's file rule without `.json`) before any path: `BadId`. A private `fn valid_id(s: &str) -> bool` in `store.rs` does it by bytes (26 bytes, each `0-9` or upper case `A-Z` except `I`, `L`, `O`, `U`).
- **Message files.** `read`, `list` and `unread` see only names `<id>.json` whose `<id>` passes `valid_id`, sorted by byte order, which is ULID order (oldest first). Another name in `new/` is never counted, listed, moved or read. A missing maildir or `new/` is no mail: `Ok(vec![])` / `Ok(0)`, and nothing is created.
- **Folders.** A write path makes the missing parts of `mail/<key>/{tmp,new,cur}` (and `mail/` with its parents) with `std::fs::DirBuilder::new().recursive(true)` and, on unix, `.mode(0o700)` (M-C4). `recursive(true)` makes a folder that another thread made at the same moment no error.
- **No clean-up on a failed write.** When `write_synced` or the rename fails, `deliver` removes nothing: the file stays in `tmp/`, and the sweep deletes it after 1 h (M§10 "Write fails partway").
- **Clock.** No code in `mail/` reads a clock: `Message::new` and `sweep` take `now`. The ULID comes from `ulid::Ulid::from_datetime(now)`.

### Steps

- [x] **1. Add `ulid` to the crate.** In `crates/zygo-workspace/Cargo.toml` `[dependencies]`, after `time.workspace = true`, add `ulid.workspace = true`. Run `cargo build -p zygo-workspace` to update `Cargo.lock` (the `zygo-workspace` entry gains `"ulid"`). Check: `cargo build -p zygo-workspace --locked` exits 0; `cargo tree -p zygo-workspace -e normal --depth 1 | grep ' zygo-'` prints only `zygo-config`, `zygo-core`, `zygo-proxy`, `zygo-work`.

- [x] **2. Write the test file header and helpers** in `crates/zygo-workspace/tests/mail_store.rs`. The whole file is `cfg(unix)` (Global constraints: tests that use workspace state or file modes). No test reads a clock: times come from `at(..)` or from a file's own modification time.

```rust
#![cfg(unix)]
//! ML-2: the maildir store (M§5, M-C3, M-C4, M§10 "Write fails partway"; S§6.2, S§8.5 steps 1 and 3).
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_workspace::mail::store::{self, Summary};
use zygo_workspace::mail::{mail_dir, MailError, Message, BODY_MAX, SUBJECT_MAX};
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
```

- [x] **3. Add the message tests** (M§5 message file and limits):

```rust
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
```

- [x] **4. Add the send, read and limit tests** (M§5, M-C3, M§13 #3, graph review focus 4):

```rust
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
    assert_eq!(store::list(&m, KEY).unwrap().iter().map(|s| s.id.clone()).collect::<Vec<_>>(), [x.id.clone()]);
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
```


  - *Deviation (clippy, `cloned_ref_to_slice_refs`):* in `a_partial_file_in_tmp_is_never_counted_listed_or_read` the expected value `[x.id.clone()]` is `std::slice::from_ref(&x.id)`; the check is the same.
- [x] **5. Add the sweep, mode, key and removal tests** (M§5 retention, M§10, M-C4, AU84):

```rust
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
```

- [x] **6. Add the byte-preserving delivery tests** (S§6.2, S§8.5 steps 1 and 3):

```rust
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
```


  - *Lead's change (added test):* `deliver_staged_never_overwrites_an_existing_file_in_new`: a `new/<ULID>.json` with other bytes is byte-identical after `deliver_staged` of the same id, which returns `Ok(false)` and removes the staged file.
- [x] **7. Run the tests and see them fail.** `cargo test -p zygo-workspace --test mail_store` → fails to compile with error E0432, unresolved import `zygo_workspace::mail` (the module does not exist).

- [x] **8. Write `crates/zygo-workspace/src/mail/mod.rs`** and add `pub mod mail;` to `crates/zygo-workspace/src/lib.rs` between `pub mod loadout;` and `pub mod mux;`. Add `mail` to the module list of the crate doc line (`//! Workspaces: identity, … and mux (AU103)` → `… mux and mail (AU103, M§5)`).
  - Module doc: `//! Workspace mail: the message file and the maildir store (M§5). Owner: node ML-2.` Then `pub mod store;`.
  - `SUBJECT_MAX`, `BODY_MAX`, `Message` (derive and `#[serde(flatten)] pub extra` as in Interfaces; field order as in Interfaces, so the file has the M§5 key order), `MailError` with the `Display` table above, `impl std::error::Error for MailError`, `impl From<std::io::Error> for MailError` (→ `Io`).
  - `Message::new`: `subject.len() > SUBJECT_MAX` → `Err(SubjectTooLong)`; then `body.len() > BODY_MAX` → `Err(BodyTooLarge)`; `version: 1`; `id: ulid::Ulid::from_datetime(now).to_string()`; `sent_at`: `time::OffsetDateTime::from(now)` with the nanoseconds set to 0, formatted with `Rfc3339` (`2023-11-14T22:13:20Z`); `extra` empty. It checks no name: names are ML-4's.
  - `mail_dir(paths, ws_id)` = `crate::state::Store::open(paths, ws_id).dir().join("mail")`. It creates and checks nothing, as `Store::open`.

- [x] **9. Write `crates/zygo-workspace/src/mail/store.rs`** with the signatures of Interfaces and the Rules above. Module doc: `//! The maildir of each member: mail/<pane key>/{tmp,new,cur}/<ULID>.json (M§5, M-C3, M-C4).` Private items: `const TMP_MAX_AGE: Duration = Duration::from_secs(3600);` (M§10), `fn valid_id(s: &str) -> bool`, `fn maildir(mail_dir: &Path, key: &str) -> Result<PathBuf, MailError>` (the key check, then the join), `fn make_dirs(md: &Path) -> std::io::Result<()>` (`tmp`, `new`, `cur`, 0700), `fn ids(dir: &Path) -> std::io::Result<Vec<String>>` (the sorted valid ids; `NotFound` → empty), `fn sync_dir(dir: &Path) -> std::io::Result<()>` (unix: `File::open(dir)?.sync_all()`; other systems: `Ok(())`).
  - `write_synced`: `OpenOptions` with `write(true).create_new(true)` and, on unix, `mode(0o600)`; `write_all(bytes)`; `sync_all()`; then `sync_dir` of the parent. It makes no folder.
  - `deliver_staged`, in this order (contract): `maildir(key)?`; `valid_id(id)` else `BadId`; `ids(new)?.len() >= max_unread as usize` → `Full { label: key.to_string(), max: max_unread }` with the staged file left in place (author's call: the signature has no label, so the line names the pane key); `new/<id>.json` or `cur/<id>.json` exists (`symlink_metadata` is `Ok`) → remove the staged file, `Ok(false)`; `make_dirs`; `fs::rename(staged, new/<id>.json)`; `sync_dir(new)`; `Ok(true)`. It never opens the staged file.
  - `deliver`: `maildir(key)?`; `valid_id(&msg.id)` else `BadId`; `msg.subject.len() > SUBJECT_MAX` → `SubjectTooLong`, `msg.body.len() > BODY_MAX` → `BodyTooLarge` (the fields are public, so `deliver` checks again); `ids(new)?.len() >= max_unread as usize` → `Full { label: label.to_string(), max: max_unread }` before any write; `make_dirs`; `serde_json::to_vec(msg)` (a serde error becomes `Io` through `std::io::Error::other`), then `write_synced(tmp/<id>.json, &bytes)?`; then `deliver_staged(mail_dir, key, &msg.id, &tmp, max_unread)?`: `true` → `Ok(())`, `false` → `Err(Duplicate { id })`.
  - `read`: `maildir(key)?`; for each id of `ids(new)?` while fewer than `max` files were moved: make `cur/` (0700, recursive); `fs::rename(new/<id>.json, cur/<id>.json)`; `NotFound` → skip it, uncounted (another reader took it, M§5); another error → `Err(Io)`; then read `cur/<id>.json` and parse it as `Message`; a file that does not parse is skipped, not returned, and stays in `cur/` for the sweep. `max` counts moved files, so one call moves at most `max` files.
  - `list`: `maildir(key)?`; for each id of `ids(new)?`, read and parse the file; `NotFound` (a reader took it) or a parse failure → skip; else `Summary { id, from, subject }`. It moves nothing.
  - `unread`: `maildir(key)?`; `ids(new)?.len()`.
  - `sweep`: every error is ignored. For each entry of `mail_dir` whose name is UTF-8, passes `valid_pane_key` and is a folder by `symlink_metadata` (no link followed; so `mail/sync/` is never entered): in `cur/`, remove each file whose modification time `m` has `now.duration_since(m)` greater than `keep`; in `tmp/`, the same with `TMP_MAX_AGE`. `new/` is never read. A `now` before `m` (`duration_since` is `Err`) removes nothing.
  - `remove_pane`: `valid_pane_key(key)` else `Err(std::io::Error::new(ErrorKind::InvalidInput, "invalid pane key"))`; `fs::remove_dir_all(mail_dir.join(key))`, `NotFound` → `Ok(())`.


  - *Lead's change (`deliver_staged`):* no check then rename into `new/`. The move is `std::fs::hard_link(staged, new/<id>.json)`, which fails with `AlreadyExists` when the target exists (then: remove the staged file, `Ok(false)`); then `sync_dir(new)`; then remove the staged file. The pre-check of `cur/<id>.json` for the duplicate rule stays; there is no pre-check of `new/`. `deliver` reaches `new/` only through `deliver_staged`, so it uses the same method.
  - *Deviation:* after the link and the `fsync` of `new/`, a failed remove of the staged file is ignored: the message is delivered, and the name left is a `tmp/` file for the sweep or a staged file that a later `deliver_staged` removes as a duplicate. An error there would tell the sender that a delivered message failed.
  - *Deviation:* when `deliver_staged` returns `Full` inside `deliver` (a race after the first count), `deliver` gives the line with its own `label`, not the pane key.
- [x] **10. Run the tests and see them pass.** `cargo test -p zygo-workspace --test mail_store` → `test result: ok. 22 passed; 0 failed; 0 ignored`.


  - *Deviation:* 23 tests, not 22 (the lead's added test).
- [x] **11. Verify the crate, the clock rule and the floor.**
  - `grep -rn 'SystemTime::now\|Instant::now\|Ulid::generate\|ids::ulid' crates/zygo-workspace/src/mail crates/zygo-workspace/tests/mail_store.rs` → no output.
  - `grep -n 'unix' crates/zygo-workspace/src/mail/store.rs` shows every `PermissionsExt`, `OpenOptionsExt`, `DirBuilderExt` use under `#[cfg(unix)]` (the crate builds on windows-latest; CI job `test` checks it).
  - `cargo test -p zygo-workspace --locked --no-fail-fast` → every `test result:` line is `ok`.
  - `cargo build --workspace --locked` → exit 0.


  - *Note:* `cargo clippy -p zygo-workspace --all-targets -- -D warnings` fails before it reaches this node's code, on findings in files this node does not touch: `crates/zygo-work/src/decisions/record.rs:277`, `crates/zygo-work/src/gate/transition.rs:10`, `:158`, and `crates/zygo-workspace/tests/down.rs:419`, `:1670`, `:1684`, `:1699`. With `--no-deps`, `src/mail/` and `tests/mail_store.rs` have no finding.
- [x] **12. Commit** `crates/zygo-workspace/Cargo.toml`, `Cargo.lock`, `crates/zygo-workspace/src/lib.rs`, `crates/zygo-workspace/src/mail/mod.rs`, `crates/zygo-workspace/src/mail/store.rs` and `crates/zygo-workspace/tests/mail_store.rs` on `phase3/ML-2`, message `ML-2: maildir store`, with the `Zygo-Session` and `Zygo-Work` trailers of the implementer's brief and no Claude attribution. Check: `git show --stat HEAD` lists those six files and nothing else.

  - *Deviation:* the branch is `phase3/ML-2--ML-2` (the implementer's checkout). The plan ticks and these notes are a second commit, on `docs/work/phase3/nodes/ML-2.md` only.


### Spec coverage

| Requirement | Step / test |
|---|---|
| M§5 message file version 1, fields, ULID name | 3 `message_new_fills_the_version_1_fields`; 4 `deliver_writes_in_tmp_then_renames_into_new` |
| M§5 subject ≤ 200 bytes, body ≤ 1 MiB | 3 `message_new_refuses_a_subject_over_200_bytes`, `message_new_refuses_a_body_over_1_mib` |
| M§13 #3 1 MiB byte-equal | 4 `a_1_mib_body_arrives_byte_equal` |
| M§5 unknown fields kept (AU78) | 4 `unknown_fields_survive_a_read`; 6 `deliver_staged_puts_the_staged_bytes_into_new_unchanged` |
| M-C3 / M§5 send: `tmp/`, `fsync`, rename into `new/` | 4 `deliver_writes_in_tmp_then_renames_into_new`, `two_readers_and_a_sender_get_each_message_once`; 6 `write_synced_creates_new_0600_and_refuses_an_existing_file` |
| M§5 read: rename `new/` → `cur/`, gone means taken; oldest first; `max` | 4 `read_moves_oldest_first_and_returns_at_most_max`, `two_readers_and_a_sender_get_each_message_once` |
| Graph review focus 4 (two readers and a sender) | 4 `two_readers_and_a_sender_get_each_message_once` |
| M§5 limit `max_unread` = 500 and its line | 4 `the_501st_unread_message_is_refused`; 6 `deliver_staged_leaves_the_staged_file_when_new_is_full` |
| M§5 retention: sweep `cur/` older than `keep` | 5 `sweep_deletes_old_cur_and_tmp_and_nothing_in_new` |
| M§10 "Write fails partway": stays in `tmp/`, swept after 1 h | 4 `a_partial_file_in_tmp_is_never_counted_listed_or_read`; 5 `sweep_deletes_old_cur_and_tmp_and_nothing_in_new` |
| M§5 `forget-pane` deletes `mail/<key>/` (used by ML-14) | 5 `remove_pane_deletes_only_that_maildir` |
| M-C4 folders `0700`, files `0600` | 5 `folders_are_0700_and_files_0600`; 6 `write_synced_…`, `deliver_staged_puts_…` |
| AU84 pane key before any path | 5 `a_bad_pane_key_is_refused_before_any_path_is_built`; 6 `deliver_staged_refuses_a_bad_key_or_id_with_no_rename` |
| M-C2 mail only under `mail/` | 8 `mail_dir`; 9: the store writes only under `mail_dir`; 4 `mail_dir_is_under_the_workspace_state` |
| S§6.2 / S§8.5 step 1 (write, `0600`, `fsync`) and step 3 (rename into `new/`), bytes unchanged | 6 all four `write_synced` / `deliver_staged` tests |
| S§8.5 `incoming/` is not swept | 5 `sweep_deletes_old_cur_and_tmp_and_nothing_in_new` |
| No test reads a clock (graph constraint) | 2, 11 grep |
