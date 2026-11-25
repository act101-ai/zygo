# MS-5 — Sync state files: paths, identity, state.json, imported log, marks

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§5 (Identity, Outbox, Imported log), S§7.5 (identity file; recorded blob ids), S§8.3 (`state.json`, `again`, `lock`), S§8.4 (`last_start`), S§8.5 (imported log: append, `fsync`, 27 bytes per entry, never trimmed), S§15; S§16 (W§7.1 paths, M-C4)
**Files:** `crates/zygo-mail-sync/src/state.rs`, `crates/zygo-mail-sync/tests/state.rs`, `docs/specs/2026-11-20-workspaces-design.md` (S§16 change note: W§7.1 gains the cache path and mail/outbox/, mail/sync/)
**Consumes:** Paths (existing, crates/zygo-core/src/paths.rs: workspaces_dir, workspace_cache_dir); the mail/ folder and its modes: `mail_dir` (ML-2)
**Produces:** `zygo_mail_sync::state::SyncPaths { pub mail: PathBuf, pub outbox: PathBuf, pub outbox_tmp: PathBuf, pub refused: PathBuf, pub sync: PathBuf, pub incoming: PathBuf, pub imported: PathBuf, pub identity: PathBuf, pub state: PathBuf, pub members: PathBuf, pub lock: PathBuf, pub again: PathBuf, pub last_start: PathBuf, pub worktree: PathBuf }`, `zygo_mail_sync::state::SyncPaths::new(paths: &Paths, ws_id: &str) -> SyncPaths`, `zygo_mail_sync::state::write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()>`, `zygo_mail_sync::state::identity(sp: &SyncPaths) -> std::io::Result<Option<String>>`, `zygo_mail_sync::state::make_identity(sp: &SyncPaths) -> std::io::Result<String>`, `zygo_mail_sync::state::PublicCache { pub url_sha256: Vec<String>, pub verdict: String, pub at: u64 }`, `zygo_mail_sync::state::State { pub last_ok: Option<u64>, pub last_error: Option<String>, pub public: Option<PublicCache>, pub pushed: BTreeMap<String, String>, pub pending: BTreeMap<String, String>, pub ci_note_shown: bool }`, `zygo_mail_sync::state::load(sp: &SyncPaths) -> State`, `zygo_mail_sync::state::save(sp: &SyncPaths, s: &State) -> std::io::Result<()>`, `zygo_mail_sync::state::imported_load(sp: &SyncPaths) -> std::io::Result<std::collections::HashSet<ulid::Ulid>>`, `zygo_mail_sync::state::imported_append(sp: &SyncPaths, id: ulid::Ulid) -> std::io::Result<()>`, `zygo_mail_sync::state::mark_again(sp: &SyncPaths) -> std::io::Result<()>`, `zygo_mail_sync::state::take_again(sp: &SyncPaths) -> std::io::Result<bool>`, `zygo_mail_sync::state::read_last_start(sp: &SyncPaths) -> Option<u64>`, `zygo_mail_sync::state::write_last_start(sp: &SyncPaths, unix: u64) -> std::io::Result<()>`
**Acceptance:** `cargo test -p zygo-mail-sync --test state` → all pass: `SyncPaths` puts state under `<workspaces_dir>/<id>/mail/outbox/` and `mail/sync/` and the worktree at `<workspace_cache_dir>/<id>/mail-sync/`; `make_identity` writes 32 lower-case hex characters once, mode `0600` (unix); a second call of `identity` returns the same value; `imported_append` adds 27 bytes per ULID and `imported_load` reads them back; a torn last line is ignored, not fatal; `save` then `load` round-trips and the file holds no URL (only SHA-256 hex); `take_again` is true once after `mark_again`; every file is written atomically, `0600`, in folders `0700` (unix).

## Plan

Premise check (live code at origin/phase-3, d9f3e17f; this branch has it):
- MS-2 landed the crate `crates/zygo-mail-sync` with `pub mod state;` in `src/lib.rs:15`; `src/state.rs` exists and is empty (0 lines). `Cargo.toml` already has every dependency this node needs: `zygo-core`, `zygo-workspace`, `anyhow`, `serde` (workspace `features = ["derive"]`), `serde_json`, `ulid` (3.0.0), `rand` (0.10.4), `tempfile`, and `libc` under `cfg(unix)`. `sha2` and `hex` are normal dependencies, so the integration test can use them. No `Cargo.toml` change.
- `Paths` methods (`crates/zygo-core/src/paths.rs`): `pub fn workspaces_dir(&self) -> PathBuf` (`:73`) = `state_dir()/workspaces`; `pub fn workspace_cache_dir(&self) -> PathBuf` (`:78`) = `cache_dir/workspaces`. Names match the node file.
- ML-2 `mail_dir` (`crates/zygo-workspace/src/mail/mod.rs:91`): `pub fn mail_dir(paths: &Paths, ws_id: &str) -> PathBuf` = `Store::open(paths, ws_id).dir().join("mail")` = `<workspaces_dir>/<id>/mail`; it creates nothing. Its folder modes come from `pub fn ensure_mail_root(mail_dir: &Path) -> std::io::Result<()>` (`mail/mod.rs:96`), which calls `store::make_dir` (`mail/store.rs:44-64`): recursive create with mode `0700`, refuses a leaf that is a symlink or not a folder (`InvalidInput`), sets the leaf to `0700` when it is not. The function takes any path; it is not limited to `mail/`.
- Drift 1: the module `crates/zygo-mail-sync/src/identity.rs` exists (empty, MS-2). The node puts `identity`/`make_identity` in `state.rs`, as Produces says; `identity.rs` stays for the S§7.5 checks of a later node. A function `state::identity` and a module `crate::identity` do not clash.
- Drift 2: the atomic writer exists: `zygo_core::session_dir::write_atomic(target: &Path, bytes: &[u8]) -> anyhow::Result<()>` (`crates/zygo-core/src/session_dir.rs:141-155`): temp file in the target's folder, `0600` on unix, `sync_all`, `persist` (rename). It returns `anyhow::Result` and makes no folder. Produces needs `std::io::Result<()>` and `0700` folders, so `state::write_atomic` is a thin wrapper: `ensure_mail_root(parent)`, then the core writer, with the error mapped back to `io::Error`. No new temp-and-rename code.
- Identity generator: no helper exists. The house pattern is `format!("{:032x}", rand::random::<u128>())` (`crates/zygo-workspace/src/snapshot.rs:432`), which gives exactly 32 lower-case hex characters. `make_identity` uses it.
- A pane key is `^k[a-z2-7]{8}$` (`crates/zygo-workspace/src/identity.rs:58-61`), so `mail/outbox/` and `mail/sync/` never collide with a member maildir `mail/<pane key>/`.
- `ulid::Ulid::from_string(&str) -> Result<Ulid, DecodeError>` and `Display` (26 characters) exist in ulid 3.0.0; 26 + `\n` = 27 bytes (S§8.5).
- Review focus item 17 names MS-5 for "a torn last line of the imported log" (step 8). Its other half, `finish_incoming`, is MS-12's.
- Downstream Consumes (MS-6, MS-8 to MS-13, MS-15 to MS-21) name only items in Produces below; none needs a change.

Author's calls (the spec text does not decide them):
- A complete imported-log line that is not a ULID is skipped, not fatal, like the torn last line. `imported_append` writes a `\n` before its entry when the file does not end with `\n`, so an entry never joins a torn fragment. Crash safety holds: a torn entry was never completed, so S§8.5 step 5 appends that ULID again.
- `identity` returns `Err(InvalidData)` for a file that is not 32 lower-case hex characters (one trailing `\n` allowed), and `make_identity` then fails instead of writing a new identity: a silent new identity would make this workspace a stranger to its own registry and acks (S§7.5). `--new-identity` (MS-15) deletes `sp.identity` before it calls `make_identity`.
- `SyncPaths.lock` is a path only: S§8.3 step 3 (the fs4 lock and the 30 s CLI wait) belongs to MS-13, which consumes `SyncPaths`. `SyncPaths.members` is likewise a path for MS-6.
- `load` returns `State::default()` for a missing or unparsable `state.json` (the signature has no error).
- The test file is `#![cfg(unix)]` as a whole (Global constraints: tests that use workspace state or file modes are `cfg(unix)`; as `tests/git_builder.rs:3`). That covers every mode assertion.

Consumes (live):
- `zygo_core::paths::Paths` with `pub fn workspaces_dir(&self) -> PathBuf` (`crates/zygo-core/src/paths.rs:73`) and `pub fn workspace_cache_dir(&self) -> PathBuf` (`paths.rs:78`)
- `zygo_workspace::mail::mail_dir(paths: &Paths, ws_id: &str) -> PathBuf` (`crates/zygo-workspace/src/mail/mod.rs:91`, ML-2)
- `zygo_workspace::mail::ensure_mail_root(mail_dir: &std::path::Path) -> std::io::Result<()>` (`mail/mod.rs:96`, ML-2): makes a folder and its parents `0700`
- `zygo_core::session_dir::write_atomic(target: &Path, bytes: &[u8]) -> anyhow::Result<()>` (`crates/zygo-core/src/session_dir.rs:141`, existing)
- `zygo_workspace::identity::valid_pane_key(s: &str) -> bool` (`crates/zygo-workspace/src/identity.rs:58`, existing; test only)

Produces (exact; all in `crates/zygo-mail-sync/src/state.rs`, module `zygo_mail_sync::state`):
- `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SyncPaths { pub mail: PathBuf, pub outbox: PathBuf, pub outbox_tmp: PathBuf, pub refused: PathBuf, pub sync: PathBuf, pub incoming: PathBuf, pub imported: PathBuf, pub identity: PathBuf, pub state: PathBuf, pub members: PathBuf, pub lock: PathBuf, pub again: PathBuf, pub last_start: PathBuf, pub worktree: PathBuf }`
- `pub fn SyncPaths::new(paths: &Paths, ws_id: &str) -> SyncPaths`; with `m = mail_dir(paths, ws_id)`: `mail = m`, `outbox = m/outbox`, `outbox_tmp = m/outbox/tmp`, `refused = m/outbox/refused`, `sync = m/sync`, `incoming = m/sync/incoming`, `imported = m/sync/imported`, `identity = m/sync/identity`, `state = m/sync/state.json`, `members = m/sync/members.json`, `lock = m/sync/lock`, `again = m/sync/again`, `last_start = m/sync/last_start`, `worktree = paths.workspace_cache_dir()/<ws_id>/mail-sync`. Pure: it creates and checks nothing.
- `pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()>`
- `pub fn identity(sp: &SyncPaths) -> std::io::Result<Option<String>>`
- `pub fn make_identity(sp: &SyncPaths) -> std::io::Result<String>`
- `#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)] pub struct PublicCache { pub url_sha256: Vec<String>, pub verdict: String, pub at: u64 }`
- `#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(default)] pub struct State { pub last_ok: Option<u64>, pub last_error: Option<String>, pub public: Option<PublicCache>, pub pushed: BTreeMap<String, String>, pub pending: BTreeMap<String, String>, pub ci_note_shown: bool }`; `pushed`/`pending` map an own file's branch path (`registry/<host>.json`, `ack/<host>.json`) to the blob id last pushed / about to be pushed (S§7.5); `last_error` holds a check name only (S§8.3, S-C2); `ci_note_shown` is the S§6.6 once-only note
- `pub fn load(sp: &SyncPaths) -> State`
- `pub fn save(sp: &SyncPaths, s: &State) -> std::io::Result<()>`
- `pub fn imported_load(sp: &SyncPaths) -> std::io::Result<std::collections::HashSet<ulid::Ulid>>`
- `pub fn imported_append(sp: &SyncPaths, id: ulid::Ulid) -> std::io::Result<()>`
- `pub fn mark_again(sp: &SyncPaths) -> std::io::Result<()>`
- `pub fn take_again(sp: &SyncPaths) -> std::io::Result<bool>`
- `pub fn read_last_start(sp: &SyncPaths) -> Option<u64>`
- `pub fn write_last_start(sp: &SyncPaths, unix: u64) -> std::io::Result<()>`

Steps:

- [ ] 1. Create `crates/zygo-mail-sync/tests/state.rs` with the header and helpers:
  ```rust
  //! MS-5: the sync state files (mail sync design S§5, S§7.5, S§8.3, S§8.4, S§8.5, S§15; S§16 W§7.1, M-C4).
  //! No test depends on wall-clock time.
  #![cfg(unix)]

  use std::collections::{BTreeMap, HashSet};
  use std::fs;
  use std::os::unix::fs::PermissionsExt;
  use std::path::Path;
  use sha2::{Digest, Sha256};
  use tempfile::TempDir;
  use ulid::Ulid;
  use zygo_core::paths::Paths;
  use zygo_mail_sync::state::*;

  const ID: &str = "0123456789abcdef";

  fn paths(p: &Path) -> Paths {
      Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
          sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
          plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
  }
  fn setup() -> (TempDir, SyncPaths) { let t = TempDir::new().unwrap(); let sp = SyncPaths::new(&paths(t.path()), ID); (t, sp) }
  fn mode(p: &Path) -> u32 { fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777 }
  /// The sorted file names in `dir`.
  fn names(dir: &Path) -> Vec<String> {
      let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
      v.sort();
      v
  }
  fn u(n: u64) -> Ulid { Ulid::from_parts(n, n as u128) }
  ```

- [ ] 2. Add test `sync_paths_follow_w71` (S§5, S§6.3, S§15, W§7.1 as amended):
  ```rust
  let t = TempDir::new().unwrap();
  let p = paths(t.path());
  let sp = SyncPaths::new(&p, ID);
  let m = p.workspaces_dir().join(ID).join("mail");
  assert_eq!(sp.mail, m);
  assert_eq!(sp.mail, zygo_workspace::mail::mail_dir(&p, ID));
  assert_eq!((sp.outbox.clone(), sp.outbox_tmp.clone(), sp.refused.clone()),
      (m.join("outbox"), m.join("outbox/tmp"), m.join("outbox/refused")));
  let s = m.join("sync");
  assert_eq!(sp.sync, s);
  assert_eq!([&sp.incoming, &sp.imported, &sp.identity, &sp.state, &sp.members, &sp.lock, &sp.again, &sp.last_start],
      [&s.join("incoming"), &s.join("imported"), &s.join("identity"), &s.join("state.json"), &s.join("members.json"),
       &s.join("lock"), &s.join("again"), &s.join("last_start")]);
  assert_eq!(sp.worktree, p.workspace_cache_dir().join(ID).join("mail-sync"));
  assert_eq!(sp.worktree, t.path().join("cache/workspaces").join(ID).join("mail-sync"));
  // pure: nothing is created
  assert!(!t.path().join("data").exists() && !t.path().join("cache").exists());
  // the two folders never collide with a member maildir mail/<pane key>/
  assert!(!zygo_workspace::identity::valid_pane_key("outbox") && !zygo_workspace::identity::valid_pane_key("sync"));
  ```

- [ ] 3. Add test `write_atomic_replaces_0600_in_0700_folders` (W-C4, M-C4, S§16 M-C4):
  ```rust
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
  ```

- [ ] 4. Add test `write_atomic_refuses_a_linked_folder` (M-C4 through `ensure_mail_root`):
  ```rust
  let (t, sp) = setup();
  let real = t.path().join("elsewhere");
  fs::create_dir(&real).unwrap();
  fs::create_dir_all(&sp.mail).unwrap();
  std::os::unix::fs::symlink(&real, &sp.sync).unwrap();
  let e = write_atomic(&sp.state, b"{}").unwrap_err();
  assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
  assert_eq!(names(&real), Vec::<String>::new());
  ```

- [ ] 5. Add test `make_identity_writes_32_hex_once` (S§5 Identity, S§7.5):
  ```rust
  let (_t, sp) = setup();
  assert_eq!(identity(&sp).unwrap(), None);
  let a = make_identity(&sp).unwrap();
  assert_eq!(a.len(), 32);
  assert!(a.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')), "{a}");
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
  assert_eq!(fs::read_to_string(&sp.identity).unwrap(), format!("{fixed}\n"));
  ```

- [ ] 6. Add test `identity_refuses_a_bad_file` (author's call above):
  ```rust
  let (_t, sp) = setup();
  fs::create_dir_all(&sp.sync).unwrap();
  for bad in ["", "xyz", "00112233445566778899AABBCCDDEEFF", "00112233445566778899aabbccddeef", "00112233445566778899aabbccddeeff0"] {
      fs::write(&sp.identity, bad).unwrap();
      assert_eq!(identity(&sp).unwrap_err().kind(), std::io::ErrorKind::InvalidData, "{bad:?}");
      assert_eq!(make_identity(&sp).unwrap_err().kind(), std::io::ErrorKind::InvalidData, "{bad:?}");
      assert_eq!(fs::read_to_string(&sp.identity).unwrap(), bad);
  }
  ```

- [ ] 7. Add test `imported_append_adds_27_bytes_and_load_reads_back` (S§8.5, S§13 limit 6):
  ```rust
  let (_t, sp) = setup();
  assert!(imported_load(&sp).unwrap().is_empty()); // no file yet
  for n in 1..=3 {
      imported_append(&sp, u(n)).unwrap();
      assert_eq!(fs::metadata(&sp.imported).unwrap().len(), 27 * n);
  }
  assert_eq!(fs::read_to_string(&sp.imported).unwrap(), format!("{}\n{}\n{}\n", u(1), u(2), u(3)));
  assert_eq!(imported_load(&sp).unwrap(), [u(1), u(2), u(3)].into_iter().collect::<HashSet<_>>());
  assert_eq!(mode(&sp.imported), 0o600);
  assert_eq!(mode(&sp.sync), 0o700);
  imported_append(&sp, u(1)).unwrap(); // never trimmed; a repeat is one more line
  assert_eq!(fs::metadata(&sp.imported).unwrap().len(), 27 * 4);
  assert_eq!(imported_load(&sp).unwrap().len(), 3);
  ```

- [ ] 8. Add test `imported_load_ignores_a_torn_last_line` (S§8.5; review focus item 17):
  ```rust
  let (_t, sp) = setup();
  fs::create_dir_all(&sp.sync).unwrap();
  let whole = format!("{}\n{}\n", u(1), u(2));
  let s3 = u(3).to_string();
  // a torn tail: part of an entry, then a whole entry without its newline
  for tail in [&s3[..10], &s3[..]] {
      fs::write(&sp.imported, format!("{whole}{tail}")).unwrap();
      assert_eq!(imported_load(&sp).unwrap(), [u(1), u(2)].into_iter().collect::<HashSet<_>>(), "{tail:?}");
  }
  // a complete line that is not a ULID is skipped
  fs::write(&sp.imported, format!("{whole}not-a-ulid\n")).unwrap();
  assert_eq!(imported_load(&sp).unwrap(), [u(1), u(2)].into_iter().collect::<HashSet<_>>());
  // an append after a torn tail starts a new line: the new entry is read, the fragment is not
  let frag = &s3[..10];
  fs::write(&sp.imported, format!("{whole}{frag}")).unwrap();
  imported_append(&sp, u(4)).unwrap();
  assert_eq!(fs::read_to_string(&sp.imported).unwrap(), format!("{whole}{frag}\n{}\n", u(4)));
  assert_eq!(imported_load(&sp).unwrap(), [u(1), u(2), u(4)].into_iter().collect::<HashSet<_>>());
  ```

- [ ] 9. Add test `state_round_trip_holds_no_url` (S§8.2 cache, S§8.3 last paragraph, S§7.5 blob ids, W-C7, S-C2):
  ```rust
  let (_t, sp) = setup();
  let url = "https://user:pw@example.com/team/repo.git";
  let h = hex::encode(Sha256::digest("https://example.com/team/repo.git".as_bytes()));
  let s = State {
      last_ok: Some(1_700_000_000), last_error: Some("push".into()),
      public: Some(PublicCache { url_sha256: vec![h.clone()], verdict: "private".into(), at: 1_700_000_000 }),
      pushed: BTreeMap::from([("ack/a.json".into(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".into())]),
      pending: BTreeMap::from([("registry/a.json".into(), "d00491fd7e5bb6fa28c517a0bb32b8b506539d4d".into())]),
      ci_note_shown: true,
  };
  save(&sp, &s).unwrap();
  assert_eq!(load(&sp), s);
  let text = fs::read_to_string(&sp.state).unwrap();
  assert!(text.contains(&h), "{text}");
  for no in [url, "https://", "example.com", "user", "pw@"] { assert!(!text.contains(no), "{no:?} in {text}"); }
  assert_eq!(mode(&sp.state), 0o600);
  assert_eq!(mode(&sp.sync), 0o700);
  assert_eq!(names(&sp.sync), ["state.json"]);
  ```

- [ ] 10. Add test `load_gives_default_for_missing_or_bad_file` (S§8.3; author's call above):
  ```rust
  let (_t, sp) = setup();
  assert_eq!(load(&sp), State::default());
  fs::create_dir_all(&sp.sync).unwrap();
  for bad in ["", "{", "[]", "{\"pushed\": 3}"] {
      fs::write(&sp.state, bad).unwrap();
      assert_eq!(load(&sp), State::default(), "{bad:?}");
  }
  // missing fields take their defaults; unknown fields are ignored
  fs::write(&sp.state, "{\"last_ok\": 5, \"later\": true}").unwrap();
  assert_eq!(load(&sp), State { last_ok: Some(5), ..State::default() });
  ```

- [ ] 11. Add test `take_again_is_true_once_after_mark_again` (S§8.3 steps 2, 4, 17):
  ```rust
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
  ```

- [ ] 12. Add test `last_start_round_trip` (S§8.4; the value is a parameter, no clock read):
  ```rust
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
  ```

- [ ] 13. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-mail-sync --test state`; see it fail to compile on the missing `SyncPaths`, `write_atomic`, `identity`, `make_identity`, `PublicCache`, `State`, `load`, `save`, `imported_load`, `imported_append`, `mark_again`, `take_again`, `read_last_start`, `write_last_start`.

- [ ] 14. In `crates/zygo-mail-sync/src/state.rs`, write the module doc comment: the sync state files of one workspace (S§5, S§15): `mail/outbox/` (with `tmp/`, `refused/`), `mail/sync/` (`identity`, `state.json`, `members.json`, `imported`, `incoming/`, `lock`, `again`, `last_start`) and the worktree `<workspace_cache_dir>/<id>/mail-sync/`; every write is atomic, files `0600`, folders `0700` (W-C4, M-C4); no file holds a URL or mail content (W-C7, S-C2). Then add `SyncPaths` and `SyncPaths::new` exactly as in Produces (paths built with `PathBuf::join`, `mail` from `zygo_workspace::mail::mail_dir`).

- [ ] 15. In `state.rs`, add `write_atomic` and its private error map. Bodies:
  ```rust
  /// W-C4, M-C4: makes `path`'s folder (and its parents) `0700`, then writes `bytes` to a `0600` temp file in it,
  /// syncs it and renames it over `path` (`zygo_core::session_dir::write_atomic`).
  pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
      let dir = path.parent().filter(|d| !d.as_os_str().is_empty())
          .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "write_atomic: the path has no folder"))?;
      zygo_workspace::mail::ensure_mail_root(dir)?;
      zygo_core::session_dir::write_atomic(path, bytes).map_err(to_io)
  }

  /// The `io::Error` inside the core writer's `anyhow::Error` (a failed rename is a `tempfile::PersistError`).
  fn to_io(e: anyhow::Error) -> io::Error {
      match e.downcast::<io::Error>() {
          Ok(e) => e,
          Err(e) => match e.downcast::<tempfile::PersistError>() { Ok(p) => p.error, Err(e) => io::Error::other(e) },
      }
  }
  ```

- [ ] 16. In `state.rs`, add the identity functions: a private `fn valid_identity(s: &str) -> bool` (exactly 32 bytes, each `0-9` or `a-f`); `identity` reads `sp.identity` as text: `NotFound` → `Ok(None)`; the text without one trailing `\n` passes `valid_identity` → `Ok(Some(text))`; else `Err(io::Error::new(io::ErrorKind::InvalidData, "mail/sync/identity is not 32 lower-case hex characters"))`; any other read error is returned. `make_identity`: `if let Some(v) = identity(sp)? { return Ok(v) }`, then `let v = format!("{:032x}", rand::random::<u128>());`, `write_atomic(&sp.identity, v.as_bytes())?`, `Ok(v)`. Doc comments cite S§5 Identity and S§7.5 (random, `0600`, per workspace, not a secret; made at the first sync; `--new-identity` deletes the file first).

- [ ] 17. In `state.rs`, add `PublicCache` and `State` with the derives and `#[serde(default)]` of Produces, each field documented with its spec source (S§8.2 cache: `url_sha256` = SHA-256 hex of each URL without user info, `verdict`, `at` = Unix seconds of the injected clock; S§8.3 `last_ok`, `last_error` = check name only; S§7.5 `pushed`, `pending`; S§6.6 `ci_note_shown`). `load`: `fs::read(&sp.state).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()`. `save`: `write_atomic(&sp.state, &serde_json::to_vec_pretty(s).map_err(io::Error::other)?)`.

- [ ] 18. In `state.rs`, add `imported_load` and `imported_append`. Bodies (the torn-line rule, S§8.5):
  ```rust
  /// S§8.5: each ULID in the imported log. Only `\n`-ended lines count: the bytes after the last `\n` are a torn
  /// entry from a crash and are ignored; a line that is not a ULID is skipped. No file: an empty set.
  pub fn imported_load(sp: &SyncPaths) -> std::io::Result<HashSet<Ulid>> {
      let bytes = match fs::read(&sp.imported) {
          Ok(b) => b,
          Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
          Err(e) => return Err(e),
      };
      let mut lines = bytes.split(|b| *b == b'\n');
      lines.next_back(); // after the last `\n`: empty, or a torn entry
      Ok(lines.filter_map(|l| std::str::from_utf8(l).ok().and_then(|s| Ulid::from_string(s).ok())).collect())
  }

  /// S§8.5 step 2: appends `<ULID>\n` (27 bytes) and syncs the file. When the file does not end with `\n` (a torn
  /// entry), a `\n` goes first, so the new entry is a line of its own. The log is never trimmed (S§13 limit 6).
  pub fn imported_append(sp: &SyncPaths, id: Ulid) -> std::io::Result<()> {
      zygo_workspace::mail::ensure_mail_root(&sp.sync)?;
      let mut o = OpenOptions::new();
      o.read(true).append(true).create(true);
      #[cfg(unix)]
      {
          use std::os::unix::fs::OpenOptionsExt;
          o.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
      }
      let mut f = o.open(&sp.imported)?;
      let mut line = String::with_capacity(28);
      if f.metadata()?.len() > 0 {
          let mut last = [0u8; 1];
          f.seek(SeekFrom::End(-1))?;
          f.read_exact(&mut last)?;
          if last[0] != b'\n' { line.push('\n'); }
      }
      line.push_str(&id.to_string());
      line.push('\n');
      f.write_all(line.as_bytes())?;
      f.sync_all()
  }
  ```

- [ ] 19. In `state.rs`, add the marks: `mark_again` = `write_atomic(&sp.again, b"")` (S§8.3 step 2); `take_again` = `fs::remove_file(&sp.again)`: `Ok` → `Ok(true)`, `NotFound` → `Ok(false)`, other error returned (S§8.3 steps 4 and 17; the holder clears and looks with one call); `read_last_start` = `fs::read_to_string(&sp.last_start).ok()?.trim().parse::<u64>().ok()`; `write_last_start` = `write_atomic(&sp.last_start, unix.to_string().as_bytes())` (S§8.4: its own file, atomic, the hook path is its only writer). The `use` lines: `std::collections::{BTreeMap, HashSet}`, `std::fs::{self, OpenOptions}`, `std::io::{self, Read, Seek, SeekFrom, Write}`, `std::path::{Path, PathBuf}`, `serde::{Deserialize, Serialize}`, `ulid::Ulid`, `zygo_core::paths::Paths`.

- [ ] 20. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-mail-sync --test state`; pass = all 11 tests pass (steps 2 to 12), with no warning from `zygo-mail-sync`.

- [ ] 21. In `docs/specs/2026-11-20-workspaces-design.md` W§7.1, rewrite the table to its final text by inserting two rows right after the row `` | `…/<id>/errors.log` | One line per failed hook or snapshot write. At most 64 KiB. | ``:
  ```
  | `…/<id>/mail/outbox/` | Messages to remote addresses that wait for a push; `refused/` holds the ones the secret pass refused, `tmp/` partial writes (mail sync design S§5, S§8.3 step 11). Folders `0700`, files `0600`. |
  | `…/<id>/mail/sync/` | Mail sync state (mail sync design S§15): `identity` (S§7.5), `state.json` and `members.json` (S§8.3, S§7.3), `imported` and `incoming/` (S§8.5), `lock` and `again` (S§8.3), `last_start` (S§8.4). Folders `0700`, files `0600`. |
  ```
  and one row right after the row that starts `` | `$HOME/.cache/zygo/workspaces/<id>/results/<pane key>/` ``:
  ```
  | `$HOME/.cache/zygo/workspaces/<id>/mail-sync/` | The detached worktree of the mail side branch (mail sync design S§6.3). Removed only by `zygo ws forget`, by path. |
  ```
  No other line of W§7.1 changes. Append to the `## Change log` table, after its last row (the WD-5 row of 2026-11-24), the row:
  ```
  | 2026-11-24 | W§7.1 | author's call (node MS-5) | Mail sync design S§16 row W§7.1: the sync keeps its state in `mail/outbox/` and `mail/sync/` and its side-branch worktree in the cache root `$HOME/.cache/zygo/workspaces/<id>/mail-sync/`. |
  ```

- [ ] 22. Check the spec edit: `grep -c 'mail/outbox/\|mail/sync/\|/mail-sync/' docs/specs/2026-11-20-workspaces-design.md` → 4 (three table rows and the change-log row; 0 before the edit); `git diff --stat docs/specs/2026-11-20-workspaces-design.md` → 4 insertions, 0 deletions.

- [ ] 23. Run the acceptance and the rest: `env -u ZYGO_SESSION_DIR cargo test -p zygo-mail-sync --test state && env -u ZYGO_SESSION_DIR cargo test -p zygo-mail-sync`; pass = all tests pass, `git_builder` among them. Then `env -u ZYGO_SESSION_DIR cargo build --workspace --locked`; pass = it builds with no warning from `zygo-mail-sync`.

- [ ] 24. Commit `crates/zygo-mail-sync/src/state.rs`, `crates/zygo-mail-sync/tests/state.rs` and `docs/specs/2026-11-20-workspaces-design.md` with the message `MS-5: sync state files: paths, identity, state.json, imported log, marks`, then a blank line, then the trailers `Zygo-Session: <session>` and `Zygo-Work: <unit>` from the implementing brief, each on its own line. No attribution lines.
