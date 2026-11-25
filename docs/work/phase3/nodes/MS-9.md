# MS-9 — Outbox: send-time rules and secret pass, refusal and notice, counts, local retention

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§5 (Outbox), S§6.2, S§7.2 (the four refusals of a remote send), S§8.6 ("At send"; "At commit" finding: move to `refused/` and the notice), S§9 last paragraph (local retention), S§15 (`mail/outbox/` owner); S§19 #4 (send part), #21; S§16 (base-spec change note)
**Files:** `crates/zygo-mail-sync/src/outbox.rs`, `crates/zygo-mail-sync/tests/outbox.rs`, `docs/specs/2026-11-22-workspace-mail-design.md` (S§16 change note: M§5 from and to may hold a full address)
**Consumes:** `SyncSettings` (MS-3), `SyncOff` (MS-3); `Address` (MS-4), `To` (MS-4); `SyncPaths` (MS-5), `write_atomic` (MS-5), `State` (MS-5), `zygo_mail_sync::state::load` (MS-5); `MemberCache` (MS-6), `read_cache` (MS-6); `Outcome` (MS-7), `zygo_mail_sync::secret::check` (MS-7); `Verdict` (MS-2); the M§5 message file v1 and its writer: `Message` (ML-2), `deliver` (ML-2); the local delivery of the notice as the reserved sender zygo (not send, whose from is never zygo), with Message::new("zygo", ..): `zygo_workspace::mail::Message::new(from: &str, to: &str, subject: &str, body: &str, now: SystemTime) -> Result<Message, MailError>` (ML-2), `zygo_workspace::mail::store::deliver(mail_dir: &Path, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError>` (ML-2), `zygo_workspace::mail::names::resolve(ws: &Workspace, to: &str, sender_key: Option<&str>) -> Result<Vec<Target>, MailError>` (ML-4); [mail] keep: `MailConfig` (ML-1)
**Produces:** `zygo_mail_sync::outbox::SendError` (enum: `NoSenderName`, `SyncOff`, `Public`, `UnknownAddress(Vec<String>)`, `Secret(Vec<String>)`, `CheckFailed(&'static str)`, `Io(String)`), `zygo_mail_sync::outbox::SendError::line(&self) -> String`, `zygo_mail_sync::outbox::send_remote(sp: &SyncPaths, settings: Result<&SyncSettings, &SyncOff>, from: Option<&str>, to: &Address, subject: &str, body: &str, redaction: &RedactionConfig, main_root: &Path, now: SystemTime) -> Result<ulid::Ulid, SendError>`, `zygo_mail_sync::outbox::pending(sp: &SyncPaths) -> std::io::Result<Vec<(ulid::Ulid, PathBuf)>>`, `zygo_mail_sync::outbox::refuse(sp: &SyncPaths, id: ulid::Ulid, ids: &[String], notify: &mut dyn FnMut(&str, &str, &str) -> std::io::Result<()>) -> std::io::Result<()>`, `zygo_mail_sync::outbox::count(sp: &SyncPaths) -> usize`, `zygo_mail_sync::outbox::refused_count(sp: &SyncPaths) -> usize`, `zygo_mail_sync::outbox::sweep(sp: &SyncPaths, keep: Duration, now: SystemTime)`
**Acceptance:** `cargo test -p zygo-mail-sync --test outbox` → all pass: a send with a built-in AWS key in the body returns `Secret` with the pattern id and writes nothing to `outbox/`; each S§7.2 refusal gives its exact line (`zygo: remote mail needs a sender name; give this member a name in the loadout or with --name`, `zygo: mail sync is off for this workspace`, the public refusal, and an unknown address with the known addresses); a clear send writes one `0600` file through `outbox/tmp/` whose JSON has `to` = the full address and `from` = `<name>@<host>`; `refuse` moves the file to `outbox/refused/` and calls `notify` once with sender, subject `not sent: <ULID>` and the S§8.6 body, and calls nothing for sender `cli`; `sweep` deletes `refused/` files older than `keep` and `outbox/tmp/` files older than 1 h by the given time, and never touches `mail/sync/incoming/`. (The call from the P3-1 command-start sweep is in MS-17, which adds the `zygo` dependency on `zygo-mail-sync`.)

## Plan

### Premises (checked against `origin/phase-3` at `878c329e`)

- `crates/zygo-mail-sync/src/outbox.rs` exists and is empty (0 bytes, made by MS-2); `pub mod outbox;` is at `crates/zygo-mail-sync/src/lib.rs:9`. No `tests/outbox.rs` exists, so `cargo test -p zygo-mail-sync --test outbox` fails now (no such test target). The crate already depends on `zygo-core`, `zygo-config`, `zygo-workspace`, `zygo-redact`, `serde_json`, `ulid` and `tempfile` (`crates/zygo-mail-sync/Cargo.toml:7-23`): no `Cargo.toml` change.
- The secret scanner the spec names is MS-7's `zygo_mail_sync::secret::check(msg: &serde_json::Value, redaction: &RedactionConfig, main_root: &Path, policy: &DeadlinePolicy) -> Outcome` (`crates/zygo-mail-sync/src/secret.rs:23`), over `zygo_redact::secret::redact(body: &mut Value, c: &Compiled, cache: &mut SessionCache, policy: &DeadlinePolicy) -> Result<Findings, RedactError>` (`crates/zygo-redact/src/secret.rs:74`). `Outcome { Clear, Finding(Vec<String>), CouldNotRun(&'static str) }` (`crates/zygo-mail-sync/src/secret.rs:13`); the ids of a `Finding` are sorted. The built-in AWS key pattern id is `aws_access_key_id`, pattern `(?:AKIA|ASIA)[A-Z0-9]{16}\b` (`crates/zygo-config/data/redaction_patterns.toml:26-27`), from `zygo_config::builtin_patterns()` (`crates/zygo-config/src/builtin_patterns.rs:8`). Check names: `redaction_patterns`, `act_lite_store`, `secret_timeout`. No new scanner.
- `DeadlinePolicy::default()` is 200 ms plus 50 ms per MiB of wall-clock time (`crates/zygo-redact/src/secret.rs:22-26`); `expire_after_scans: Some(0)` times out at the second uncached string of a call (`:122`).
- `zygo_workspace::mail::Message::new(from, to, subject, body, now)` (`crates/zygo-workspace/src/mail/mod.rs:36`) checks only the subject (200 bytes) and body (1 MiB) limits and makes `id` with `Ulid::from_datetime(now)`. `MailError`'s `Display` is the whole user line (`mod.rs:76-96`): `MailError::Io(e)` is `zygo: mail: {e}`; `UnknownTarget` is `zygo: unknown target; known names: <a, b>` or `none` (`:85-90`).
- `zygo_workspace::mail::store::write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()>` (`crates/zygo-workspace/src/mail/store.rs:138`): create-new, `0600`, `fsync` of the file and its folder, makes no folder. `zygo_workspace::mail::ensure_mail_root(dir: &Path) -> std::io::Result<()>` (`mod.rs:110`) makes `dir` and its missing parents `0700` and refuses a link or a non-folder (`store.rs:41-60`). The M§5 sweep (`store.rs:281-305`) ages files by modification time with `now.duration_since(m) > age`, deletes a link itself, skips a folder that is not a real folder, and ignores every error; its 1 h `TMP_MAX_AGE` (`store.rs:11`) is private.
- `zygo_workspace::mail::names::sender` (`crates/zygo-workspace/src/mail/names.rs:60`) gives the member name, else the pane key, else `cli`. `zygo_core::ids::valid_member_name` (`crates/zygo-core/src/ids.rs:28-37`) is false for `cli`, `all`, `zygo` and every pane key `^k[a-z2-7]{8}$`. So "the sender has a name" (S§7.2) is `from.is_some_and(valid_member_name)`.
- `SyncPaths` (`crates/zygo-mail-sync/src/state.rs:17-46`): `outbox = mail/outbox`, `outbox_tmp = mail/outbox/tmp`, `refused = mail/outbox/refused`, `incoming = mail/sync/incoming`. `state::load` (`state.rs:311`) returns the default `State` for no file and `Err` for a link, a read error or a value outside the rules. `State.public: Option<PublicCache { url_sha256, verdict: String, at }>` (`state.rs:215-239`); MS-8's `public::check` writes `verdict = "public"` for `Verdict::Public` (`crates/zygo-mail-sync/src/public.rs:166-171`). `crate::state::read_regular(path) -> io::Result<Option<Vec<u8>>>` (`state.rs:161`, `pub(crate)`) reads a regular file with no link follow.
- `registry::read_cache(sp) -> MemberCache` (`crates/zygo-mail-sync/src/registry.rs:215`) keeps only valid hosts and valid member names, so `cli@b` and `zygo@b` are never in it. `MemberCache::knows(&Address) -> bool` (`:196`), `MemberCache::addresses() -> Vec<String>` in host order (`:188`). `address::resolve_to("cli@b", ..)` is `Err(AddrError::Reserved)` (`crates/zygo-mail-sync/src/address.rs:43-66`). `address::ulid_file(name) -> Option<Ulid>` (`address.rs:78`) takes only `<26 Crockford>.json`.
- `SyncSettings { branch, remote, host, sync_gap }` and `SyncOff { NotOn, NoHost, BadHost, BadBranch, BadGap }` (`crates/zygo-config/src/schema.rs:756-778`); `parse_duration` (`:716`); `MailConfig::default().keep == "30d"` (`:700-710`).

Premise drift (reported):
1. **Wall clock in the send-time pass.** The Produces signature of `send_remote` has no `DeadlinePolicy`, and S§8.6 fixes the default policy (200 ms). A test of a `Clear` or `Finding` result through `send_remote` would depend on wall-clock time, against S-C9. Resolution (author's call, as MS-7 drift 2): this node also produces the seam `send_remote_with(.., policy: &DeadlinePolicy)`; `send_remote` calls it with `&DeadlinePolicy::default()`. Tests use `send_remote` only for refusals decided before the pass.
2. **`write_atomic` (MS-5) is in Consumes but does not fit.** Its temp file is in the target folder (`zygo_core::session_dir::write_atomic`, called at `state.rs:93`), so a crash would leave a temp name in `outbox/` itself. The Acceptance asks for the write through `outbox/tmp/`. The node uses `store::write_synced` into `outbox/tmp/`, then a hard link into `outbox/` (the M§5 send, M-C3). `write_atomic` is not used.
3. **`Verdict` (MS-2) is in Consumes but not needed.** The public rule reads the cached verdict string `"public"` from `state.json`; it runs no probe. `Verdict` is not used.
4. **The notice parts** (`Message::new("zygo", ..)`, `store::deliver`, `names::resolve`) are used by MS-13, the caller of `refuse` (MS-13 Consumes). This node uses `Message::new` and `store::deliver` only in one test, to show a `notify` closure that delivers the notice; `resolve` is not used.

Decisions the spec leaves open (author's calls):
- **Unknown-address line** (S§7.2: "with the known addresses (as M§10)"): `zygo: unknown address; known addresses: <a@h, b@h>`, or `none` when the cache is empty, in the form of M§10's `UnknownTarget` line.
- **Pattern ids** in the S§8.6 lines and notice body are joined with `", "`.
- **Order of the refusals** is the S§7.2 list order: sender name, sync off, public, unknown address; then the M§5 limits, then the secret pass, then the write.
- **`SendError::Io(String)`** holds a whole user line: a `Message::new` limit error as its `MailError` line (`zygo: subject is longer than 200 bytes`, `zygo: body is larger than 1 MiB`), and an I/O error as `zygo: mail: <error>`. A `state.json` that `load` refuses is `Io`: the public rule cannot be read, so the send is refused (fail closed); the sync refuses the same file.
- **Public rule:** the last cached verdict is `"public"`, at any age (S§7.2 "the last public check"). No age is added.
- **Outbox layout:** `mail/outbox/<ULID>.json` (flat); `mail/outbox/refused/<ULID>.json`; the bytes are `serde_json::to_vec(&Message)`, as `store::deliver` writes them, and are never rewritten (S-G1).
- **Notice target:** `refuse` reads `from` from the file; the sender is the part before `@` (the whole value when there is no `@`). `notify` is called only when that part passes `valid_member_name`, so `cli` (S§8.6) and `zygo` (no inbox, S§7.2) get none. A file that does not parse is moved and gets no notice.
- **No cap** on the member list, the outbox or the refused folder: none is in the spec. The 1000-per-pass limit of S§8.3 step 11 is the pass's (MS-13), not `pending`'s.
- **Refused age** (round 2, author's call): the `refused/` age counts from the refusal: `refuse` sets the refused file's modification time to its `now`, so the S§9 sweep never deletes a message refused after it waited longer than `keep`.

State left when a step fails partway:
- `send_remote`: a refusal (sender name, sync off, public, unknown address, limits, `Secret`, `CheckFailed`) writes nothing: `mail/outbox/` is not even made. A failed folder check (`ensure_mail_root`) may leave the folders `mail/`, `mail/outbox/`, `mail/outbox/tmp/` (`0700`) and no file. A failed `write_synced` leaves at most `outbox/tmp/<ULID>.json`, never listed by `pending`; the sweep deletes it after 1 h. A failed link removes the `tmp/` name (best effort; else the sweep) and leaves nothing in `outbox/`. After the link the message is in the outbox and the send returns `Ok`: a failed folder `fsync` or `tmp/` name removal is ignored (as `deliver_staged`, `store.rs:174-180`), because an `Err` would make the sender send again; a left `tmp/` name is a second name of the same file, swept after 1 h.
- `refuse`: no source file is `Err(NotFound)`, nothing changes and `notify` is not called. A failed folder check or link is `Err`; the file stays in `outbox/` (the next pass checks it again) and no notice is sent. Link done, removal of the outbox name failed: `Err`, the file is in both folders, no notice; the next `refuse` of that ULID finds `refused/<ULID>.json` (`AlreadyExists`), removes the outbox name and sends the notice. Removal done, then a crash or a failed `notify`: the file is in `refused/` with no notice; `refuse` returns `notify`'s `Err`; `zygo mail ls` shows the refused count (S§8.6).
- `sweep`: every error is ignored; a file it cannot delete stays for the next sweep.

### Interfaces

Consumed (exact, in the code now):
- `zygo_config::schema::SyncSettings { pub branch: String, pub remote: String, pub host: String, pub sync_gap: Duration }`, `zygo_config::schema::SyncOff { NotOn, NoHost, BadHost, BadBranch, BadGap }` (MS-3); `zygo_config::schema::MailConfig { pub keep: String, .. }`, `zygo_config::parse_duration(s: &str) -> Option<Duration>` (ML-1; tests only); `zygo_config::schema::RedactionConfig`
- `zygo_mail_sync::address::Address { pub name: String, pub host: String }`, `To` (MS-4); `resolve_to(s: &str, own_hosts: &[String]) -> Result<To, AddrError>`, `ulid_file(name: &str) -> Option<ulid::Ulid>` (MS-4)
- `zygo_mail_sync::state::SyncPaths` (MS-5), `zygo_mail_sync::state::load(sp: &SyncPaths) -> std::io::Result<State>`, `State { pub public: Option<PublicCache>, .. }`, `PublicCache { pub url_sha256: Vec<String>, pub verdict: String, pub at: u64 }`, `crate::state::read_regular(path: &Path) -> io::Result<Option<Vec<u8>>>`; `save(sp: &SyncPaths, s: &State) -> std::io::Result<()>` (tests only). `write_atomic` is not used (drift 2).
- `zygo_mail_sync::registry::MemberCache { pub hosts: BTreeMap<String, Vec<String>> }`, `read_cache(sp: &SyncPaths) -> MemberCache`, `MemberCache::knows(&self, a: &Address) -> bool`, `MemberCache::addresses(&self) -> Vec<String>` (MS-6); `write_cache(sp: &SyncPaths, c: &MemberCache) -> std::io::Result<()>` (tests only)
- `zygo_mail_sync::secret::Outcome`, `zygo_mail_sync::secret::check(msg: &serde_json::Value, redaction: &RedactionConfig, main_root: &Path, policy: &DeadlinePolicy) -> Outcome` (MS-7)
- `zygo_redact::secret::DeadlinePolicy` (`Default`: 200 ms + 50 ms/MiB)
- `zygo_workspace::mail::Message::new(from: &str, to: &str, subject: &str, body: &str, now: SystemTime) -> Result<Message, MailError>` (ML-2), `zygo_workspace::mail::MailError` (its `Display`), `zygo_workspace::mail::ensure_mail_root(mail_dir: &Path) -> std::io::Result<()>`, `zygo_workspace::mail::store::write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()>`; tests only: `zygo_workspace::mail::store::deliver(mail_dir: &Path, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError>`, `zygo_workspace::mail::store::list(mail_dir: &Path, key: &str) -> Result<Vec<Summary>, MailError>`
- `zygo_core::ids::valid_member_name(s: &str) -> bool`
- Not used here (drift 3, 4): `Verdict` (MS-2), `names::resolve` (ML-4).

Produced (this node; consumers: MS-13 `pending`, `refuse`; MS-17 `sweep`; MS-18 `send_remote`, `SendError`, `count`, `refused_count`; MS-19 `send_remote`, `SendError`; MS-21 `count`):
- `zygo_mail_sync::outbox::SendError` — `#[derive(Debug, Clone, PartialEq, Eq)] pub enum SendError { NoSenderName, SyncOff, Public, UnknownAddress(Vec<String>), Secret(Vec<String>), CheckFailed(&'static str), Io(String) }`
- `zygo_mail_sync::outbox::SendError::line(&self) -> String`
- `zygo_mail_sync::outbox::send_remote(sp: &SyncPaths, settings: Result<&SyncSettings, &SyncOff>, from: Option<&str>, to: &Address, subject: &str, body: &str, redaction: &RedactionConfig, main_root: &Path, now: SystemTime) -> Result<ulid::Ulid, SendError>`
- `zygo_mail_sync::outbox::send_remote_with(sp: &SyncPaths, settings: Result<&SyncSettings, &SyncOff>, from: Option<&str>, to: &Address, subject: &str, body: &str, redaction: &RedactionConfig, main_root: &Path, now: SystemTime, policy: &DeadlinePolicy) -> Result<ulid::Ulid, SendError>` (test seam, drift 1)
- `zygo_mail_sync::outbox::pending(sp: &SyncPaths) -> std::io::Result<Vec<(ulid::Ulid, PathBuf)>>`
- `zygo_mail_sync::outbox::refuse(sp: &SyncPaths, id: ulid::Ulid, ids: &[String], notify: &mut dyn FnMut(&str, &str, &str) -> std::io::Result<()>, now: SystemTime) -> std::io::Result<()>` — `notify(sender, subject, body)`; the refused file's time is set to `now` (round 2)
- `zygo_mail_sync::outbox::count(sp: &SyncPaths) -> std::io::Result<usize>` — no folder is `Ok(0)`; a folder or entry that cannot be read is `Err` (round 2)
- `zygo_mail_sync::outbox::refused_count(sp: &SyncPaths) -> std::io::Result<usize>` — as `count` (round 2)
- `zygo_mail_sync::outbox::sweep(sp: &SyncPaths, keep: Duration, now: SystemTime)`

### Steps

- [x] 1. Write the failing tests in the new file `crates/zygo-mail-sync/tests/outbox.rs`. The AWS key is built with `concat`, so no literal key is in the repository (as `crates/zygo-mail-sync/tests/secret.rs:17-19`). The secret pass gets an hour of budget, so no result depends on the clock (drift 1).

  ```rust
  //! MS-9: the outbox (mail sync design S§5, S§7.2, S§8.6, S§9 last paragraph, S§15; S§19 #4 send part, #21).
  //! No test depends on wall-clock time: `now` is fixed, file times are set by the test, and the secret pass gets
  //! an hour of budget.
  use std::fs;
  use std::path::{Path, PathBuf};
  use std::time::{Duration, SystemTime, UNIX_EPOCH};
  use tempfile::TempDir;
  use ulid::Ulid;
  use zygo_config::{builtin_patterns, parse_duration, Action, MailConfig, RedactionConfig, RedactionPattern, SyncOff, SyncSettings};
  use zygo_core::paths::Paths;
  use zygo_mail_sync::address::{resolve_to, AddrError, Address};
  use zygo_mail_sync::outbox::{count, pending, refuse, refused_count, send_remote, send_remote_with, sweep, SendError};
  use zygo_mail_sync::registry::{write_cache, MemberCache};
  use zygo_mail_sync::state::{save, PublicCache, State, SyncPaths};
  use zygo_redact::DeadlinePolicy;
  use zygo_workspace::mail::store::{deliver, list};
  use zygo_workspace::mail::Message;

  const ID: &str = "0123456789abcdef";
  const KEY: &str = "kaaaaaaaa";
  const NO_NAME: &str = "zygo: remote mail needs a sender name; give this member a name in the loadout or with --name";

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

  fn now() -> SystemTime { UNIX_EPOCH + Duration::from_secs(1_800_000_000) } // 2027-01-15T08:00:00Z
  fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }
  fn aws() -> String { ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat() }
  fn builtin() -> RedactionConfig { RedactionConfig { pattern: builtin_patterns(), ..Default::default() } }
  fn on() -> SyncSettings {
      SyncSettings { branch: "zygo/mail".into(), remote: "origin".into(), host: "a".into(), sync_gap: Duration::from_secs(60) }
  }
  fn addr(name: &str, host: &str) -> Address { Address { name: name.into(), host: host.into() } }

  #[cfg(unix)]
  fn mode(p: &Path) -> u32 {
      use std::os::unix::fs::PermissionsExt;
      fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
  }

  /// A workspace on host `a` whose member cache holds `bob@b` and `carol@c`, and a main worktree with no
  /// act-lite store (regex patterns only).
  struct Fx { _t: TempDir, sp: SyncPaths, root: PathBuf }

  fn fx() -> Fx {
      let t = TempDir::new().unwrap();
      let sp = SyncPaths::new(&paths(t.path()), ID).unwrap();
      let mut c = MemberCache::default();
      c.hosts.insert("b".into(), vec!["bob".into()]);
      c.hosts.insert("c".into(), vec!["carol".into()]);
      write_cache(&sp, &c).unwrap();
      let root = t.path().join("repo");
      fs::create_dir_all(root.join(".git")).unwrap();
      Fx { _t: t, sp, root }
  }

  fn send(f: &Fx, from: Option<&str>, to: &Address, body: &str) -> Result<Ulid, SendError> {
      send_remote_with(&f.sp, Ok(&on()), from, to, "status", body, &builtin(), &f.root, now(), &far())
  }

  /// One message file in `outbox/`, as `send_remote` writes it.
  fn outbox_file(f: &Fx, from: &str) -> Ulid {
      let m = Message::new(from, "bob@b", "status", "hello", now()).unwrap();
      fs::create_dir_all(&f.sp.outbox).unwrap();
      fs::write(f.sp.outbox.join(format!("{}.json", m.id)), serde_json::to_vec(&m).unwrap()).unwrap();
      m.id.parse().unwrap()
  }

  fn put(p: &Path, age_secs: u64) {
      fs::create_dir_all(p.parent().unwrap()).unwrap();
      fs::write(p, b"x").unwrap();
      fs::File::options().write(true).open(p).unwrap().set_modified(now() - Duration::from_secs(age_secs)).unwrap();
  }

  #[test] // S§8.6 "At send", S§19 #4 send part
  fn aws_key_is_refused_with_its_pattern_id_and_nothing_is_written() {
      let f = fx();
      let e = send(&f, Some("alice"), &addr("bob", "b"), &format!("the key is {}", aws())).unwrap_err();
      assert_eq!(e, SendError::Secret(vec!["aws_access_key_id".to_string()]));
      assert_eq!(e.line(), "zygo: message not sent: it holds a secret (aws_access_key_id)");
      assert!(!e.line().contains(&aws()));
      assert!(!f.sp.outbox.exists());
  }

  #[test] // S§8.6: ids joined with ", "
  fn two_findings_name_both_ids() {
      let f = fx();
      let mut cfg = builtin();
      cfg.pattern.push(RedactionPattern { id: "ship_word".into(), pattern: "zz-forbidden-zz".into(), action: Action::Block, description: "d".into() });
      let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status",
          &format!("say zz-forbidden-zz with {}", aws()), &cfg, &f.root, now(), &far()).unwrap_err();
      assert_eq!(e.line(), "zygo: message not sent: it holds a secret (aws_access_key_id, ship_word)");
      assert!(!f.sp.outbox.exists());
  }

  #[test] // S§8.6 "Could not run"
  fn check_that_cannot_run_is_refused_with_its_check_name() {
      let f = fx();
      let bad = RedactionConfig {
          pattern: vec![RedactionPattern { id: "bad".into(), pattern: "(".into(), action: Action::Redact, description: "d".into() }],
          ..Default::default()
      };
      let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status", "hi", &bad, &f.root, now(), &far()).unwrap_err();
      assert_eq!(e, SendError::CheckFailed("redaction_patterns"));
      assert_eq!(e.line(), "zygo: message not sent: the secret check could not run (redaction_patterns); send it again");
      let expire = DeadlinePolicy { expire_after_scans: Some(0), ..far() };
      let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now(), &expire).unwrap_err();
      assert_eq!(e.line(), "zygo: message not sent: the secret check could not run (secret_timeout); send it again");
      assert!(!f.sp.outbox.exists());
  }

  #[test] // S§7.2 refusal 1, S§19 #21
  fn sender_with_no_name_is_refused() {
      let f = fx();
      for from in [None, Some("cli"), Some(KEY)] {
          let e = send_remote(&f.sp, Ok(&on()), from, &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now()).unwrap_err();
          assert_eq!(e, SendError::NoSenderName);
          assert_eq!(e.line(), NO_NAME);
      }
      assert!(!f.sp.outbox.exists());
  }

  #[test] // S§7.2 refusal 2
  fn sync_off_is_refused() {
      let f = fx();
      for off in [SyncOff::NotOn, SyncOff::NoHost, SyncOff::BadHost, SyncOff::BadBranch, SyncOff::BadGap] {
          let e = send_remote(&f.sp, Err(&off), Some("alice"), &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now()).unwrap_err();
          assert_eq!(e, SendError::SyncOff);
          assert_eq!(e.line(), "zygo: mail sync is off for this workspace");
      }
      assert!(!f.sp.outbox.exists());
  }

  fn verdict(f: &Fx, v: &str) {
      save(&f.sp, &State { public: Some(PublicCache { url_sha256: vec!["0".repeat(64)], verdict: v.into(), at: 0 }), ..Default::default() }).unwrap();
  }

  #[test] // S§7.2 refusal 3, S§8.2: the last check, at any age
  fn last_public_verdict_is_refused_and_the_others_send() {
      let f = fx();
      verdict(&f, "public");
      let e = send_remote(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now()).unwrap_err();
      assert_eq!(e, SendError::Public);
      assert_eq!(e.line(), "zygo: the remote can be read without a login; mail sync refused");
      assert!(!f.sp.outbox.exists());
      for v in ["private", "local", "unknown"] {
          verdict(&f, v);
          send(&f, Some("alice"), &addr("bob", "b"), "hi").unwrap();
      }
      assert_eq!(count(&f.sp), 3);
  }

  #[test] // S§7.2 refusal 4 (as M§10), S§19 #21
  fn unknown_address_is_refused_with_the_known_addresses() {
      let f = fx();
      let e = send(&f, Some("alice"), &addr("dave", "b"), "hi").unwrap_err();
      assert_eq!(e, SendError::UnknownAddress(vec!["bob@b".to_string(), "carol@c".to_string()]));
      assert_eq!(e.line(), "zygo: unknown address; known addresses: bob@b, carol@c");
      assert_eq!(resolve_to("cli@b", &["a".to_string()]), Err(AddrError::Reserved));
      assert!(matches!(send(&f, Some("alice"), &addr("cli", "b"), "hi"), Err(SendError::UnknownAddress(_))));
      assert!(matches!(send(&f, Some("alice"), &addr("zygo", "b"), "hi"), Err(SendError::UnknownAddress(_))));
      fs::remove_file(&f.sp.members).unwrap();
      assert_eq!(send(&f, Some("alice"), &addr("bob", "b"), "hi").unwrap_err().line(), "zygo: unknown address; known addresses: none");
      assert!(!f.sp.outbox.exists());
  }

  #[test] // M§5 limits stay; nothing is written
  fn subject_over_the_limit_is_refused() {
      let f = fx();
      let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), &"s".repeat(201), "hi", &builtin(), &f.root, now(), &far()).unwrap_err();
      assert_eq!(e, SendError::Io("zygo: subject is longer than 200 bytes".into()));
      assert_eq!(e.line(), "zygo: subject is longer than 200 bytes");
      assert!(!f.sp.outbox.exists());
  }

  #[test] // S§5 Outbox, S§6.2, M§5 send through tmp/
  fn clear_send_writes_one_0600_file_with_full_addresses() {
      let f = fx();
      let id = send(&f, Some("alice"), &addr("bob", "b"), "the build is green").unwrap();
      let file = f.sp.outbox.join(format!("{id}.json"));
      assert_eq!(pending(&f.sp).unwrap(), vec![(id, file.clone())]);
      let bytes = fs::read(&file).unwrap();
      let m: Message = serde_json::from_slice(&bytes).unwrap();
      assert_eq!(bytes, serde_json::to_vec(&m).unwrap());
      assert_eq!((m.version, m.id.clone(), m.from.as_str(), m.to.as_str()), (1, id.to_string(), "alice@a", "bob@b"));
      assert_eq!((m.sent_at.as_str(), m.subject.as_str(), m.body.as_str()), ("2027-01-15T08:00:00Z", "status", "the build is green"));
      assert_eq!(fs::read_dir(&f.sp.outbox_tmp).unwrap().count(), 0);
      assert_eq!((count(&f.sp), refused_count(&f.sp)), (1, 0));
      #[cfg(unix)]
      {
          use std::os::unix::fs::MetadataExt;
          assert_eq!((mode(&file), mode(&f.sp.outbox), mode(&f.sp.outbox_tmp)), (0o600, 0o700, 0o700));
          assert_eq!(fs::metadata(&file).unwrap().nlink(), 1);
      }
  }

  #[test] // a write that fails partway leaves nothing in outbox/
  fn failed_write_leaves_no_outbox_file() {
      let f = fx();
      fs::create_dir_all(&f.sp.outbox).unwrap();
      fs::write(&f.sp.outbox_tmp, b"").unwrap(); // tmp/ is a file: the folder check refuses it
      let e = send(&f, Some("alice"), &addr("bob", "b"), "hi").unwrap_err();
      assert!(matches!(&e, SendError::Io(l) if l.starts_with("zygo: mail: ")), "{e:?}");
      assert_eq!(pending(&f.sp).unwrap(), vec![]);
      assert_eq!(count(&f.sp), 0);
  }

  #[test]
  fn pending_is_ulid_ordered_regular_files_only() {
      let f = fx();
      let (a, b, c) = (Ulid::from_parts(1, 1), Ulid::from_parts(2, 1), Ulid::from_parts(3, 1));
      for u in [b, a] { put(&f.sp.outbox.join(format!("{u}.json")), 0); }
      put(&f.sp.outbox.join("notes.txt"), 0);
      put(&f.sp.outbox.join(format!("{}.json", a.to_string().to_lowercase())), 0);
      put(&f.sp.outbox_tmp.join(format!("{c}.json")), 0);
      put(&f.sp.refused.join(format!("{c}.json")), 0);
      #[cfg(unix)]
      std::os::unix::fs::symlink(f.sp.refused.join(format!("{c}.json")), f.sp.outbox.join(format!("{c}.json"))).unwrap();
      assert_eq!(pending(&f.sp).unwrap(), vec![(a, f.sp.outbox.join(format!("{a}.json"))), (b, f.sp.outbox.join(format!("{b}.json")))]);
      assert_eq!((count(&f.sp), refused_count(&f.sp)), (2, 1));
  }

  #[test]
  fn no_outbox_is_empty() {
      let f = fx();
      assert_eq!(pending(&f.sp).unwrap(), vec![]);
      assert_eq!((count(&f.sp), refused_count(&f.sp)), (0, 0));
  }

  fn aws_ids() -> Vec<String> { vec!["aws_access_key_id".to_string()] }

  #[test] // S§8.6 "At commit" finding
  fn refuse_moves_the_file_and_notifies_the_sender_once() {
      let f = fx();
      let id = outbox_file(&f, "alice@a");
      let before = fs::read(f.sp.outbox.join(format!("{id}.json"))).unwrap();
      let mut calls: Vec<(String, String, String)> = Vec::new();
      refuse(&f.sp, id, &aws_ids(), &mut |to: &str, subject: &str, body: &str| {
          calls.push((to.into(), subject.into(), body.into()));
          Ok(())
      }).unwrap();
      assert_eq!(calls, vec![(
          "alice".to_string(),
          format!("not sent: {id}"),
          format!("zygo did not send message {id} to bob@b: it holds a secret (aws_access_key_id). The message is kept on this machine in mail/outbox/refused/."),
      )]);
      assert_eq!(fs::read(f.sp.refused.join(format!("{id}.json"))).unwrap(), before);
      assert_eq!(pending(&f.sp).unwrap(), vec![]);
      assert_eq!((count(&f.sp), refused_count(&f.sp)), (0, 1));
      #[cfg(unix)]
      assert_eq!(mode(&f.sp.refused), 0o700);
  }

  #[test] // S§8.6: the CLI as `cli` has no inbox
  fn refuse_for_cli_sends_no_notice() {
      let f = fx();
      let id = outbox_file(&f, "cli@a");
      let mut n = 0;
      refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }).unwrap();
      assert_eq!(n, 0);
      assert_eq!((count(&f.sp), refused_count(&f.sp)), (0, 1));
  }

  #[test] // the notice is a local M§5 message from `zygo`
  fn notify_can_deliver_the_notice_from_zygo() {
      let f = fx();
      let id = outbox_file(&f, "alice@a");
      refuse(&f.sp, id, &aws_ids(), &mut |to: &str, subject: &str, body: &str| {
          let m = Message::new("zygo", to, subject, body, now()).map_err(std::io::Error::other)?;
          deliver(&f.sp.mail, KEY, to, &m, 500).map_err(std::io::Error::other)
      }).unwrap();
      let l = list(&f.sp.mail, KEY).unwrap();
      assert_eq!(l.len(), 1);
      assert_eq!((l[0].from.as_str(), l[0].subject.clone()), ("zygo", format!("not sent: {id}")));
  }

  #[test] // crash after the link, before the outbox name was removed
  fn refuse_again_after_a_partial_move_notifies_once() {
      let f = fx();
      let id = outbox_file(&f, "alice@a");
      fs::create_dir_all(&f.sp.refused).unwrap();
      fs::hard_link(f.sp.outbox.join(format!("{id}.json")), f.sp.refused.join(format!("{id}.json"))).unwrap();
      let mut n = 0;
      refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }).unwrap();
      assert_eq!(n, 1);
      assert_eq!((count(&f.sp), refused_count(&f.sp)), (0, 1));
  }

  #[test]
  fn refuse_of_a_missing_file_changes_nothing() {
      let f = fx();
      let mut n = 0;
      let e = refuse(&f.sp, Ulid::from_parts(1, 1), &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }).unwrap_err();
      assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
      assert_eq!((n, refused_count(&f.sp)), (0, 0));
  }

  #[test] // S§9 last paragraph, S§15 `mail/outbox/` row
  fn sweep_deletes_old_refused_and_tmp_files_only() {
      let f = fx();
      let keep = parse_duration(&MailConfig::default().keep).unwrap(); // 30d
      let k = keep.as_secs();
      let old_refused = f.sp.refused.join("a.json");
      let kept_refused = f.sp.refused.join("b.json");
      let old_tmp = f.sp.outbox_tmp.join("a.json");
      let kept_tmp = f.sp.outbox_tmp.join("b.json");
      let queued = f.sp.outbox.join(format!("{}.json", Ulid::from_parts(1, 1)));
      let incoming = f.sp.incoming.join("a.json");
      put(&old_refused, k + 1);
      put(&kept_refused, k);
      put(&old_tmp, 3601);
      put(&kept_tmp, 3600);
      put(&queued, 10 * 365 * 86400);
      put(&incoming, 10 * 365 * 86400);
      sweep(&f.sp, keep, now());
      assert!(!old_refused.exists() && !old_tmp.exists());
      assert!(kept_refused.exists() && kept_tmp.exists() && queued.exists() && incoming.exists());
  }

  #[cfg(unix)]
  #[test] // a refused/ that is a link is skipped; its target is never touched
  fn sweep_skips_a_linked_folder() {
      let f = fx();
      let outside = f._t.path().join("outside/a.json");
      put(&outside, 10 * 365 * 86400);
      fs::create_dir_all(&f.sp.outbox).unwrap();
      std::os::unix::fs::symlink(outside.parent().unwrap(), &f.sp.refused).unwrap();
      sweep(&f.sp, Duration::from_secs(1), now());
      assert!(outside.exists());
  }

  #[test]
  fn sweep_with_no_outbox_does_nothing() {
      let f = fx();
      sweep(&f.sp, Duration::from_secs(1), now());
      assert!(!f.sp.outbox.exists());
  }
  ```

- [x] 2. Run `cargo test -p zygo-mail-sync --test outbox` and see it fail to compile: `unresolved imports zygo_mail_sync::outbox::{count, pending, refuse, refused_count, send_remote, send_remote_with, sweep, SendError}`.
- [x] 3. In `crates/zygo-mail-sync/src/outbox.rs`, write the module doc `//! The outbox of remote mail (S§5 Outbox, S§7.2, S§8.6, S§9 last paragraph, S§15): the send-time rules and secret pass, the move of a refused message and its notice, the counts and the local sweep.`, the `SendError` enum of Produced (doc: why a remote send is refused; `Io` holds a whole user line) and `pub fn line(&self) -> String` with these exact values:
  - `NoSenderName` → `zygo: remote mail needs a sender name; give this member a name in the loadout or with --name`
  - `SyncOff` → `zygo: mail sync is off for this workspace`
  - `Public` → `zygo: the remote can be read without a login; mail sync refused`
  - `UnknownAddress(k)` → `zygo: unknown address; known addresses: {k.join(", ")}`, or `zygo: unknown address; known addresses: none` when `k` is empty
  - `Secret(ids)` → `zygo: message not sent: it holds a secret ({ids.join(", ")})`
  - `CheckFailed(c)` → `zygo: message not sent: the secret check could not run ({c}); send it again`
  - `Io(l)` → `l` unchanged
- [x] 4. In `outbox.rs`, add `send_remote` and `send_remote_with` with the exact Produced signatures, each with `#[allow(clippy::too_many_arguments)]` (as `crates/zygo-workspace/src/mail/mod.rs:140`). `send_remote` is `send_remote_with(.., &DeadlinePolicy::default())` (S§8.6 "default deadline policy"). `send_remote_with`, in this order, returning at the first refusal:
  1. `let name = from.filter(|n| valid_member_name(n)).ok_or(SendError::NoSenderName)?;`
  2. `let s = settings.map_err(|_| SendError::SyncOff)?;`
  3. `state::load(sp)`: `Ok(st)` with `st.public.as_ref().is_some_and(|p| p.verdict == "public")` → `Err(Public)`; `Err(e)` → `Err(Io(MailError::Io(e).to_string()))`.
  4. `let cache = read_cache(sp); if !cache.knows(to) { return Err(UnknownAddress(cache.addresses())) }`.
  5. `Message::new(&format!("{name}@{}", s.host), &format!("{}@{}", to.name, to.host), subject, body, now)`, an `Err(e)` → `Io(e.to_string())`.
  6. `secret::check(&v, redaction, main_root, policy)` with `v = serde_json::to_value(&msg)` (a serde error → `Io`): `Clear` continues; `Finding(ids)` → `Secret(ids)`; `CouldNotRun(c)` → `CheckFailed(c)`.
  7. Write: `ensure_mail_root` on `sp.mail`, `sp.outbox`, `sp.outbox_tmp`; `store::write_synced(&sp.outbox_tmp.join(<id>.json), &bytes)` with `bytes = serde_json::to_vec(&msg)`; `fs::hard_link` of that name to `sp.outbox.join(<id>.json)` (a link never replaces a file, M-C3); on a link error remove the `tmp/` name (error ignored) and return `Io`. After the link: `fsync` `sp.outbox` (unix, error ignored), remove the `tmp/` name (error ignored), return `Ok(id)`. Every `io::Error` maps to `Io(MailError::Io(e).to_string())`. The id is `msg.id.parse::<ulid::Ulid>()`.
- [x] 5. In `outbox.rs`, add a private `fn ulid_files(dir: &Path) -> std::io::Result<Vec<(ulid::Ulid, PathBuf)>>`: no folder (`NotFound`) is `Ok(vec![])`; each entry whose `file_type()` is a regular file (not a link) and whose name passes `address::ulid_file`, sorted by ULID. Then `pending(sp) = ulid_files(&sp.outbox)`, `count(sp) = pending(sp).map_or(0, |v| v.len())`, `refused_count(sp) = ulid_files(&sp.refused).map_or(0, |v| v.len())`.
- [x] 6. In `outbox.rs`, add `refuse` with the Produced signature: `src = sp.outbox/<id>.json`, `dst = sp.refused/<id>.json`. `crate::state::read_regular(&src)?`, `None` → `Err(io::ErrorKind::NotFound)`. Parse the bytes as `Message` (a parse error gives no sender). `ensure_mail_root` on `sp.mail` and `sp.refused`. `fs::hard_link(&src, &dst)`: `AlreadyExists` is a finished link of an earlier call; any other error is returned. `fs::remove_file(&src)?`; `fsync` `sp.refused` (unix, error ignored). Then, when the sender (the `from` before `@`, or all of it) passes `valid_member_name`, call `notify(sender, &format!("not sent: {id}"), &format!("zygo did not send message {id} to {to}: it holds a secret ({}). The message is kept on this machine in mail/outbox/refused/.", ids.join(", ")))` once and return its result; else `Ok(())`. `to` is the file's `to`.
- [x] 7. In `outbox.rs`, add `sweep` with the Produced signature and a private `const TMP_MAX_AGE: Duration = Duration::from_secs(3600);` (S§9: "older than 1 h (as M§10)"). For `(sp.refused, keep)` and `(sp.outbox_tmp, TMP_MAX_AGE)`: skip a folder whose `symlink_metadata` is not a real folder; for each entry whose `symlink_metadata` is a file or a link and `now.duration_since(modified).is_ok_and(|d| d > age)`, `fs::remove_file` it. Every error is ignored. It reads no other folder (`sp.outbox` files and `sp.incoming` are never touched), as `crates/zygo-workspace/src/mail/store.rs:281-305`.
- [x] 8. Run `cargo test -p zygo-mail-sync --test outbox` and see all 20 tests pass (19 on a non-unix host, without `sweep_skips_a_linked_folder`).
- [x] 9. Run `cargo test -p zygo-mail-sync` and `cargo clippy -p zygo-mail-sync --all-targets -- -D warnings` and see both pass (the MS-2 to MS-8 tests still pass).
- [x] 10. Edit `docs/specs/2026-11-22-workspace-mail-design.md` M§5 (S§16 change note). In the bullet that starts `- A message file (version 1):` (line 60), after the sentence that ends `Fields that a reader does not know are kept (AU78).`, insert this sentence:

  `With mail sync between machines (mail sync design S§6.2), \`from\` and \`to\` can hold a full address \`<name>@<host>\`: \`to\` is the address of the target and \`from\` the address of the sender; the file format and its version do not change.`

  Add this row at the end of the `## Change log` table (the file's existing form; the rows that come from the mail sync S§16 list name `mail sync design S§16` as the decider, `:222-223`):

  `| 2026-11-25 | M§5 | mail sync design S§16 | \`from\` and \`to\` can hold a full address \`<name>@<host>\` (S§6.2); the file format and its version do not change. |`
- [x] 11. Check the spec edit: `grep -c 'full address' docs/specs/2026-11-22-workspace-mail-design.md` → 2 (0 before the edit); `git diff --stat docs/specs/2026-11-22-workspace-mail-design.md` → 1 file changed, 2 insertions, 1 deletion (the edited bullet line and the new change-log row).
- [x] 12. Commit `crates/zygo-mail-sync/src/outbox.rs`, `crates/zygo-mail-sync/tests/outbox.rs` and `docs/specs/2026-11-22-workspace-mail-design.md` with subject `MS-9: outbox send-time rules, refusal and notice, counts, sweep` and the trailer lines `Zygo-Session: <session>` and `Zygo-Work: <unit>`, each on its own line at the end (`git commit -m`, never `--no-edit`).
- [x] 13. Verify: run the Acceptance check `cargo test -p zygo-mail-sync --test outbox` and see all pass; `git diff --name-only origin/phase-3..HEAD` lists only the three files of step 12 and this node file.
- [x] 14. Round 2, fix 1 (S§19 #20): `count` and `refused_count` return `std::io::Result<usize>`; no folder is `Ok(0)`, an unreadable folder or entry is `Err` (test `unreadable_outbox_count_is_an_error`, unix, skipped as root).
- [x] 15. Round 2, fix 2: `refuse` takes `now: SystemTime` last and sets the refused file's modification time to `now` after the link (test `refused_file_ages_from_the_refusal`).
- [x] 16. Round 2, fix 3: `refuse` runs in the M§5 order: link into `refused/`, set the time, `fsync` `refused/` (an error is returned and the outbox name stays), remove the `outbox/` name, `fsync` `outbox/` (error ignored), notify. A crash order has no black-box test; the existing partial-move and notify-error tests cover the states.
- [x] 17. Round 2, fix 4: on `AlreadyExists` the bytes of both files are compared (each read to at most `FILE_MAX + 1`); other bytes, or a file over `FILE_MAX`, are `Err(AlreadyExists)` with the outbox file kept and no notice (test `refuse_onto_a_different_refused_file_keeps_the_outbox_file`).
- [x] 18. Round 2, fix 5: `pending`, the counts and `sweep` check `mail/`, `mail/outbox/` and the read folder as `ensure_mail_root` does, without making anything; a link or non-folder is `Err(InvalidInput)` for `pending` and the counts, and `sweep` skips that folder (test `linked_outbox_is_refused_by_pending_and_not_swept`, unix). `refuse` also checks `mail/outbox/` with `ensure_mail_root`.
- [x] 19. Round 2, fix 6: `zygo_workspace::mail::store::open_regular` is private (`crates/zygo-workspace/src/mail/store.rs:78`), and so is `crate::state::open_regular` (`state.rs:127`); `refuse` reads with a local no-follow open and `take(FILE_MAX + 1)`, with the public `store::FILE_MAX`. A file over `FILE_MAX` is moved with no notice, as one that does not parse (test `refuse_of_a_file_over_the_read_limit_moves_it_without_notice`).
- [x] 20. Round 2, fix 7: tests `body_over_the_limit_is_refused`, `secret_in_the_subject_is_refused`, `refuse_of_an_unparsable_file_moves_it_without_notice`, `notify_error_after_the_move_keeps_the_file_refused`, and the unit test `outbox::tests::failed_link_removes_the_tmp_name` (the link step is the private `publish`; a link failure cannot be made through the public API, because the ULID has a random part and `ensure_mail_root` resets the folder mode).
- [x] 21. Round 2 checks: `cargo test -p zygo-mail-sync --test outbox` (29 pass), `cargo test -p zygo-mail-sync`, `cargo clippy -p zygo-mail-sync --lib --test outbox --no-deps -- -D warnings -A clippy::collapsible_if` (the `secret.rs:58` lint is the base's).
