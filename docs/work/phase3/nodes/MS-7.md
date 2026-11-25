# MS-7 — Secret pass on one message: clear, finding, could not run

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§8.6 ("Pattern set", "Run", "Three results"), S-G4; S§19 #4 and #5 (the pass result only)
**Files:** `crates/zygo-mail-sync/src/secret.rs`, `crates/zygo-mail-sync/tests/secret.rs`
**Consumes:** —
**Produces:** `zygo_mail_sync::secret::Outcome` (enum: `Clear`, `Finding(Vec<String>)`, `CouldNotRun(&'static str)`), `zygo_mail_sync::secret::check(msg: &serde_json::Value, redaction: &zygo_config::schema::RedactionConfig, main_root: &Path, policy: &zygo_redact::secret::DeadlinePolicy) -> Outcome`
**Acceptance:** `cargo test -p zygo-mail-sync --test secret` → all pass: a body with a built-in AWS key pattern gives `Finding` with that pattern id; a clean message gives `Clear` and the input value is not changed; `expire_after_scans = Some(0)` (`crates/zygo-redact/src/secret.rs:18-19`) gives `CouldNotRun`; an act-lite store file that exists and does not parse gives `CouldNotRun`; no store file gives `Clear` for a clean message (regex patterns only); `Outcome` carries pattern ids and check names only. zygo-redact source does not change.
**Uses:** zygo_redact::patterns::compile_with_store (existing, crates/zygo-redact/src/patterns.rs:36), zygo_redact::act_lite_store::read (existing, crates/zygo-redact/src/act_lite_store.rs:62-64), zygo_redact::secret::redact, DeadlinePolicy, SessionCache (existing, crates/zygo-redact/src/secret.rs:13-26, :74-76; crates/zygo-redact/src/cache.rs:122-129)

## Plan

### Premises (checked against `origin/phase-3` at `2518f10`)

- `crates/zygo-mail-sync/src/secret.rs` exists and is empty (0 bytes; made by MS-2 `fa165e9`); `pub mod secret;` is at `crates/zygo-mail-sync/src/lib.rs:13`. No `tests/secret.rs` exists. The crate already depends on `zygo-config`, `zygo-redact`, `serde_json` and `tempfile` (`crates/zygo-mail-sync/Cargo.toml:8-22`), so this node changes no `Cargo.toml`.
- `zygo_redact::patterns::compile_with_store(cfg: &RedactionConfig, stored: &[StoredValue]) -> anyhow::Result<Compiled>` (`crates/zygo-redact/src/patterns.rs:36`). The store pattern id is `ACT_LITE_ID = "act_lite_secret"` (`:8`). A pattern that does not compile is an `Err` (`:57-63`, `:80`).
- `zygo_redact::act_lite_store::read(repo: &Path) -> anyhow::Result<Vec<StoredValue>>` (`crates/zygo-redact/src/act_lite_store.rs:62-64`) calls `read_with(repo, &Lookup::system())` (`:68`). `pub struct Lookup { pub env, pub home, pub known_config, pub windows }` (`:36-44`, all fields public; `Lookup::system()` at `:47`). `pub fn store_path(repo: &Path, l: &Lookup) -> PathBuf` (`:86`). `ACT_CONFIG_DIR` sets the store folder (`:92-93`).
- `zygo_redact::secret::DeadlinePolicy { base, per_mib, expire_after_scans: Option<u64> }` (`crates/zygo-redact/src/secret.rs:13-20`), `Default` 200 ms + 50 ms per MiB, no expiry (`:22-26`). `Findings { counts: BTreeMap<String, u64>, changed: bool }` (`:38-42`). `RedactError::{Blocked { ids: Vec<String> }, Timeout}` (`:46-52`). `pub fn redact(body: &mut Value, c: &Compiled, cache: &mut SessionCache, policy: &DeadlinePolicy) -> Result<Findings, RedactError>` (`:74-76`).
- `zygo_redact::cache::SessionCache::new(pool: Arc<CachePool>) -> SessionCache` (`crates/zygo-redact/src/cache.rs:122-129`); `CachePool::new(total_cap_bytes: usize, session_cap_bytes: usize) -> Arc<CachePool>` (`:97`); `SESSION_CAP_BYTES = 64 << 20` (`:8`).
- `zygo_config::schema::RedactionConfig { pattern: Vec<RedactionPattern>, content: ContentConfig }` (`crates/zygo-config/src/schema.rs:802-805`); `zygo_config::builtin_patterns()` (`crates/zygo-config/src/builtin_patterns.rs:8`, re-exported at `crates/zygo-config/src/lib.rs:14`). The built-in AWS id is `aws_access_key_id`, pattern `(?:AKIA|ASIA)[A-Z0-9]{16}\b` (`crates/zygo-config/data/redaction_patterns.toml:26-27`).
- Live-code search: no function in the workspace tells "no act-lite store" apart from "store unreadable". The launcher treats every `read_with` error as "regex only" (`crates/zygo-launch/src/launcher.rs:367-370`). So this node adds the split in `zygo-mail-sync`, as risk #45 says.

Premise drift (reported; no spec edited, zygo-redact unchanged):
1. `act_lite_store::read` gives the same untyped `anyhow` error for a missing store as for an unreadable one (`crates/zygo-redact/src/act_lite_store.rs:72-75`). S§8.6 needs "no store file is not a failure". Resolution: on an `Err`, look at `store_path(main_root, lookup)` with `std::fs::symlink_metadata`; `NotFound` gives regex-only patterns, any other result gives `CouldNotRun`.
2. `act_lite_store::read` reads the process environment (`Lookup::system()`, `:47-53`), so a test cannot point it at a temporary store without changing process-wide variables while tests run in parallel. Resolution: this node also produces `check_with(.., lookup: &Lookup)`; `check` calls it with `Lookup::system()`. `read(repo)` is exactly `read_with(repo, &Lookup::system())`, so `check` still uses the S§8.6 store read.
3. `expire_after_scans = Some(0)` does not stop the first scan of a call (`crates/zygo-redact/src/secret.rs:18`, `:122`: `scans > 0 && ..`). A message with one string gives `Clear`. The test message has six string fields (M§5 `Message`, `crates/zygo-workspace/src/mail/mod.rs:20-27`), so the second uncached string times out.
4. The default policy is a 200 ms wall-clock budget (`secret.rs:22-26`). Tests that expect `Clear` or `Finding` pass `DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() }`, so machine load cannot give `Timeout` (CI rule: no wall-clock tests). The `Some(0)` test is decided by the scan count, not the clock. Callers (MS-9, MS-13) pass `DeadlinePolicy::default()`.

Decisions the spec leaves open (lead):
- Check names (S§8.6 says only "check name"): `"secret_timeout"` for `Timeout`, `"act_lite_store"` for a store that exists and cannot be read or parsed, `"redaction_patterns"` for patterns that do not compile.
- `Finding` ids: `Blocked { ids }` as given (sorted, deduped, `secret.rs:47`); for `Ok` with `changed = true`, the keys of `counts` (a `BTreeMap`, so sorted).
- `check` redacts a clone of `msg`; the caller's value is never changed and no redacted copy leaves the function.

### Interfaces

Consumes (exact, in the code now):
- `zygo_redact::patterns::compile_with_store(cfg: &zygo_config::RedactionConfig, stored: &[zygo_redact::act_lite_store::StoredValue]) -> anyhow::Result<zygo_redact::patterns::Compiled>`
- `zygo_redact::act_lite_store::read_with(repo: &Path, l: &Lookup) -> anyhow::Result<Vec<StoredValue>>`, `zygo_redact::act_lite_store::Lookup::system() -> Lookup`, `zygo_redact::act_lite_store::store_path(repo: &Path, l: &Lookup) -> PathBuf`
- `zygo_redact::secret::redact(body: &mut serde_json::Value, c: &Compiled, cache: &mut SessionCache, policy: &DeadlinePolicy) -> Result<Findings, RedactError>`
- `zygo_redact::secret::{DeadlinePolicy, Findings, RedactError}`
- `zygo_redact::cache::{SessionCache::new(pool: Arc<CachePool>) -> SessionCache, CachePool::new(total_cap_bytes: usize, session_cap_bytes: usize) -> Arc<CachePool>, SESSION_CAP_BYTES}`
- `zygo_config::schema::RedactionConfig`; tests also use `zygo_config::{builtin_patterns, RedactionPattern, Action}`

Produces (this node):
- `zygo_mail_sync::secret::Outcome` — `#[derive(Debug, Clone, PartialEq, Eq)] pub enum Outcome { Clear, Finding(Vec<String>), CouldNotRun(&'static str) }`
- `zygo_mail_sync::secret::check(msg: &serde_json::Value, redaction: &zygo_config::schema::RedactionConfig, main_root: &Path, policy: &zygo_redact::secret::DeadlinePolicy) -> Outcome`
- `zygo_mail_sync::secret::check_with(msg: &serde_json::Value, redaction: &zygo_config::schema::RedactionConfig, main_root: &Path, policy: &zygo_redact::secret::DeadlinePolicy, lookup: &zygo_redact::act_lite_store::Lookup) -> Outcome` (test seam, drift 2)

### Steps

- [x] 1. Write the failing tests in the new file `crates/zygo-mail-sync/tests/secret.rs`. Secret strings are built with `concat` so no literal key is in the repository (as `crates/zygo-redact/tests/secret.rs:168`). Shared helpers:

  ```rust
  //! MS-7: the secret pass on one message (S§8.6, S-G4, S§19 #4 and #5). No test depends on wall-clock time.
  use serde_json::{json, Value};
  use std::ffi::OsString;
  use std::path::{Path, PathBuf};
  use std::time::Duration;
  use tempfile::TempDir;
  use zygo_config::{builtin_patterns, Action, RedactionConfig, RedactionPattern};
  use zygo_mail_sync::secret::{check, check_with, Outcome};
  use zygo_redact::act_lite_store::{store_path, Lookup};
  use zygo_redact::DeadlinePolicy;

  /// An hour of budget: a test not about time never times out under machine load.
  fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }

  fn aws() -> String { ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat() }

  fn builtin() -> RedactionConfig { RedactionConfig { pattern: builtin_patterns(), ..Default::default() } }

  /// An M§5 message object: six string fields.
  fn msg(body: &str) -> Value {
      json!({"version": 1, "id": "01JD0000000000000000000000", "from": "alice@a", "to": "bob@b",
             "sent_at": "2026-11-24T10:00:00Z", "subject": "status", "body": body})
  }

  /// A main worktree and a store folder in one temporary directory; `ACT_CONFIG_DIR` points at `<tmp>/act`.
  fn fixture() -> (TempDir, PathBuf, Lookup) {
      let t = TempDir::new().unwrap();
      let root = t.path().join("repo");
      std::fs::create_dir_all(root.join(".git")).unwrap();
      let act: OsString = t.path().join("act").into_os_string();
      let lookup = Lookup { env: Box::new(move |k| (k == "ACT_CONFIG_DIR").then(|| act.clone())), ..Lookup::system() };
      (t, root, lookup)
  }

  fn write_store(root: &Path, lookup: &Lookup, text: &str) -> PathBuf {
      let p = store_path(root, lookup);
      std::fs::create_dir_all(p.parent().unwrap()).unwrap();
      std::fs::write(&p, text).unwrap();
      p
  }
  ```

  Tests and their assertions:

  ```rust
  #[test]
  fn aws_key_is_a_finding_with_its_pattern_id() {
      let (_t, root, lookup) = fixture();
      let m = msg(&format!("the key is {}", aws()));
      let o = check_with(&m, &builtin(), &root, &far(), &lookup);
      assert_eq!(o, Outcome::Finding(vec!["aws_access_key_id".to_string()]));
      assert!(!format!("{o:?}").contains(&aws()));
  }

  #[test]
  fn clean_message_is_clear_and_unchanged() {
      let (_t, root, lookup) = fixture();
      let m = msg("the build is green");
      let before = m.clone();
      assert_eq!(check_with(&m, &builtin(), &root, &far(), &lookup), Outcome::Clear);
      assert_eq!(m, before);
  }

  #[test]
  fn finding_leaves_the_input_unchanged() {
      let (_t, root, lookup) = fixture();
      let m = msg(&format!("the key is {}", aws()));
      let before = m.clone();
      let _ = check_with(&m, &builtin(), &root, &far(), &lookup);
      assert_eq!(m, before);
  }

  #[test]
  fn expire_after_zero_scans_could_not_run() {
      let (_t, root, lookup) = fixture();
      let p = DeadlinePolicy { base: Duration::from_secs(3600), expire_after_scans: Some(0), ..Default::default() };
      assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &p, &lookup), Outcome::CouldNotRun("secret_timeout"));
  }

  #[test]
  fn store_that_does_not_parse_could_not_run() {
      let (_t, root, lookup) = fixture();
      write_store(&root, &lookup, "not an assignment\n");
      assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::CouldNotRun("act_lite_store"));
  }

  #[test]
  fn store_that_cannot_be_read_could_not_run() {
      let (_t, root, lookup) = fixture();
      std::fs::create_dir_all(store_path(&root, &lookup)).unwrap(); // a folder where the file goes
      assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::CouldNotRun("act_lite_store"));
  }

  #[test]
  fn no_store_file_and_clean_message_is_clear() {
      let (_t, root, lookup) = fixture();
      assert!(!store_path(&root, &lookup).exists());
      assert_eq!(check_with(&msg("the build is green"), &builtin(), &root, &far(), &lookup), Outcome::Clear);
  }

  #[test]
  fn no_store_file_still_finds_regex_patterns() {
      let (_t, root, lookup) = fixture();
      let m = msg(&format!("the key is {}", aws()));
      assert_eq!(check_with(&m, &builtin(), &root, &far(), &lookup), Outcome::Finding(vec!["aws_access_key_id".to_string()]));
  }

  #[test]
  fn store_value_is_a_finding_without_its_text() {
      let (_t, root, lookup) = fixture();
      let value = ["s3cr3t", "-value-", "4417"].concat();
      write_store(&root, &lookup, &format!("TOKEN={value}\n"));
      let o = check_with(&msg(&format!("use {value} now")), &builtin(), &root, &far(), &lookup);
      assert_eq!(o, Outcome::Finding(vec!["act_lite_secret".to_string()]));
      assert!(!format!("{o:?}").contains(&value));
  }

  #[test]
  fn block_pattern_is_a_finding() {
      let (_t, root, lookup) = fixture();
      let cfg = RedactionConfig {
          pattern: vec![RedactionPattern { id: "ship_word".into(), pattern: "zz-forbidden-zz".into(), action: Action::Block, description: "d".into() }],
          ..Default::default()
      };
      assert_eq!(check_with(&msg("say zz-forbidden-zz"), &cfg, &root, &far(), &lookup), Outcome::Finding(vec!["ship_word".to_string()]));
  }

  #[test]
  fn pattern_that_does_not_compile_could_not_run() {
      let (_t, root, lookup) = fixture();
      let cfg = RedactionConfig {
          pattern: vec![RedactionPattern { id: "bad".into(), pattern: "(".into(), action: Action::Redact, description: "d".into() }],
          ..Default::default()
      };
      assert_eq!(check_with(&msg("the build is green"), &cfg, &root, &far(), &lookup), Outcome::CouldNotRun("redaction_patterns"));
  }

  #[test]
  fn check_uses_the_system_lookup() {
      let t = TempDir::new().unwrap();
      std::fs::create_dir_all(t.path().join(".git")).unwrap();
      let m = msg(&format!("the key is {}", aws()));
      // No act-lite store exists for a fresh temporary root (its project id is new), so only regex patterns apply.
      assert_eq!(check(&m, &builtin(), t.path(), &far()), Outcome::Finding(vec!["aws_access_key_id".to_string()]));
  }
  ```

  The last test only proves that `check` is wired to the store read and the redact run; it reads the real `ACT_CONFIG_DIR` location and never writes there.
- [x] 2. Run `cargo test -p zygo-mail-sync --test secret` and see it fail to compile: `unresolved imports zygo_mail_sync::secret::{check, check_with, Outcome}`.
- [x] 3. Implement `crates/zygo-mail-sync/src/secret.rs`: module doc `//! The secret pass on one message before it leaves the machine (S§8.6, S-G4).`; the `Outcome` enum and the two functions with the exact signatures in Interfaces; `pub fn check(..) -> Outcome { check_with(msg, redaction, main_root, policy, &Lookup::system()) }`. Body of `check_with`, in this order (S§8.6 "Pattern set", "Run", "Three results"):

  ```rust
  let stored = match read_with(main_root, lookup) {
      Ok(v) => v,
      Err(_) => match std::fs::symlink_metadata(store_path(main_root, lookup)) {
          Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(), // no store: regex patterns only
          _ => return Outcome::CouldNotRun("act_lite_store"),
      },
  };
  let Ok(compiled) = compile_with_store(redaction, &stored) else { return Outcome::CouldNotRun("redaction_patterns") };
  let mut copy = msg.clone();
  let mut cache = SessionCache::new(CachePool::new(SESSION_CAP_BYTES, SESSION_CAP_BYTES)); // fresh per call
  match redact(&mut copy, &compiled, &mut cache, policy) {
      Ok(f) if !f.changed => Outcome::Clear,
      Ok(f) => Outcome::Finding(f.counts.into_keys().collect()),
      Err(RedactError::Blocked { ids }) => Outcome::Finding(ids),
      Err(RedactError::Timeout) => Outcome::CouldNotRun("secret_timeout"),
  }
  ```

  No error text, store value or message text goes into `Outcome` or any output (S-C2, C3).
- [x] 4. Run `cargo test -p zygo-mail-sync --test secret` and see all 12 tests pass.
- [x] 5. Run `cargo build -p zygo-mail-sync` and `cargo test -p zygo-mail-sync` and see both pass (the MS-2 `git_builder` tests still pass).
- [x] 6. Commit `crates/zygo-mail-sync/src/secret.rs` and `crates/zygo-mail-sync/tests/secret.rs` with subject `MS-7: secret pass on one message` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:MS-7`.
- [x] 7. Verify: run `cargo test -p zygo-mail-sync --test secret` (the Acceptance command) and see all pass; run `git diff --exit-code origin/phase-3 -- crates/zygo-redact` and see exit 0 (zygo-redact source unchanged); run `git diff --name-only origin/phase-3...HEAD` and see only `crates/zygo-mail-sync/src/secret.rs`, `crates/zygo-mail-sync/tests/secret.rs` and this node file.

### Review round 1

The plan steps above stay as written; these changes follow review:

1. `check_with` scans a flat root array that holds every object key and every string leaf of the message, at any depth. zygo-redact never reads keys (`crates/zygo-redact/src/scope.rs:76`) and skips API-body paths under `messages/…` and `input/…` (`scope.rs:15-63`); a root array has neither. The caller's value is never changed. Tests: secret in `subject`, in an unknown root field, as an object key, under `input[].reasoning`, under `messages[].content[].thinking`, nested in an array of objects. The `Some(0)` test still gives `CouldNotRun("secret_timeout")` (the flat list has 13 strings).
2. On `Blocked { ids }` (block ids only, `crates/zygo-redact/src/secret.rs:151-152`), a second pass runs with every block pattern as `redact`; `Finding` holds the sorted union of ids. If the second pass fails or times out, `Finding` holds the block ids. Test: a block pattern and an AWS key in one body.
3. A compile error with store values present gives `CouldNotRun("act_lite_store")` when the patterns compile without the store, else `redaction_patterns`. Test: 65535 config patterns plus one store value go over zygo-redact's `u16` pattern limit only with the store.
4. A dangling symlink at the store path gives `CouldNotRun("act_lite_store")` (fails closed); a unix-only test pins it.
5. `check_uses_the_system_lookup` is removed: it read the real store location and proved nothing about the lookup.
6. On an `Ok` redact result, `Finding` comes from the match counts (ids with a count above zero), not from `changed`: a match whose replacement equals the original text is still a finding. Test: ids `a` then `a]` and the body `[REDACTED:a]]`.
