# MS-6 — Registry and acknowledgment files, validation, member cache

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§6.1 (one writer per path), S§7.3, S§7.4, S-C5, S-C8; S§19 #12 (registry part)
**Files:** `crates/zygo-mail-sync/src/registry.rs`, `crates/zygo-mail-sync/tests/registry.rs`
**Consumes:** `valid_mail_host` (MS-3); `Address` (MS-4); `SyncPaths` (MS-5), `write_atomic` (MS-5); the member name form: `valid_member_name` (ML-1)
**Produces:** `zygo_mail_sync::registry::Check(pub &'static str)`, `zygo_mail_sync::registry::Registry { pub host: String, pub identity: String, pub members: Vec<String> }`, `zygo_mail_sync::registry::Ack { pub host: String, pub identity: String, pub imported: BTreeSet<ulid::Ulid> }`, `zygo_mail_sync::registry::parse_registry(file_host: &str, mode: &str, size: u64, bytes: &[u8]) -> Result<Registry, Check>`, `zygo_mail_sync::registry::parse_ack(file_host: &str, mode: &str, size: u64, bytes: &[u8]) -> Result<Ack, Check>`, `zygo_mail_sync::registry::Registry::to_bytes(&self) -> Vec<u8>`, `zygo_mail_sync::registry::Ack::to_bytes(&self) -> Vec<u8>`, `zygo_mail_sync::registry::REGISTRY_MAX: u64 = 65536`, `zygo_mail_sync::registry::ACK_MAX: u64 = 8388608`, `zygo_mail_sync::registry::ACK_ENTRIES_MAX: usize = 200000`, `zygo_mail_sync::registry::MemberCache { pub hosts: BTreeMap<String, Vec<String>> }`, `zygo_mail_sync::registry::write_cache(sp: &SyncPaths, c: &MemberCache) -> std::io::Result<()>`, `zygo_mail_sync::registry::read_cache(sp: &SyncPaths) -> MemberCache`, `zygo_mail_sync::registry::MemberCache::addresses(&self) -> Vec<String>`, `zygo_mail_sync::registry::MemberCache::knows(&self, a: &Address) -> bool`
**Acceptance:** `cargo test -p zygo-mail-sync --test registry` → all pass: a registry is refused (with a named check) for mode other than `100644`, size over 64 KiB, bad JSON, `version` not 1, `host` not equal to the file name or not in the host form, identity not `^[0-9a-f]{32}$`, more than 256 members (257 refused, 256 accepted), or a member name outside M§6; an ack is refused at over 8 MiB, over 200 000 entries or an entry that is not a ULID, and the caller treats a refused ack as empty; `to_bytes` then parse round-trips; the member cache file is `0600`, written atomically, holds names only, and has no age filter.

## Plan

Premise check (live code at origin/phase-3, 268b0a07; this branch has it):
- MS-2 declared `pub mod registry;` (`crates/zygo-mail-sync/src/lib.rs:12`); `src/registry.rs` exists and is empty (0 bytes). `tests/registry.rs` does not exist.
- No crate cycle: `valid_member_name` is in `zygo-core` (`crates/zygo-core/src/ids.rs:29`), `valid_mail_host` in `zygo-config` (`crates/zygo-config/src/schema.rs:733`); `crates/zygo-mail-sync/Cargo.toml` already depends on both, and `src/address.rs:3-4` imports both. No `Cargo.toml` change: `serde` (derive), `serde_json`, `ulid` 3.0.0 (no `serde` feature, so ULIDs go through strings) and `tempfile` are normal dependencies.
- `SyncPaths.members` = `mail/sync/members.json` (`src/state.rs:37`, `:68`). `write_atomic` (`state.rs:82`) checks only the file's own folder; the private `write_file` (`state.rs:109`) adds the `mail/` check with the public `zygo_workspace::mail::ensure_mail_root` (`crates/zygo-workspace/src/mail/mod.rs:110`).
- Existing code this node reuses, not rebuilds: `state.rs:161` `read_regular` (no link follow, no FIFO wait), `state.rs:178` `valid_identity` (`^[0-9a-f]{32}$` on bytes), `address.rs:78` `ulid_file` (canonical 26-character Crockford ULID). The first two are private: step 4 makes them `pub(crate)`. That widens Files by `crates/zygo-mail-sync/src/state.rs` (visibility only, two words).
- Downstream Consumes (MS-9, MS-11, MS-13, MS-19, MS-20) name only items in Produces below.
- S§6.1 (one writer per path) and S§19 #12: this node gives each file its owner identity and refuses a registry with 257 names or a `host` not equal to its file name (step 2); the identity comparison against the tip (collision, twin) is MS-11, and the cache build from the valid registries is MS-13.
- Review focus (graph.md): no item names MS-6. Item 47 (own ack upkeep) is MS-13/MS-15. The node's focus is its Acceptance list; each part has a test below.

Author's calls (the spec text does not decide them):
- Check names (`Check.0`), each a valid `state.json` `last_error` (`state.rs:253`, `^[a-z0-9-]{1,64}$`): `mode`, `size`, `json`, `version`, `host`, `identity`, `members`, `member` (registry); `entries`, `entry` (ack). Order of checks: mode, size, json, version, host, identity, count, each entry. So a file with several faults names the first.
- `size` is refused when the `size` argument or `bytes.len()` is over the cap: the caller (MS-11) gets `size` from `ls-tree -l` before it reads the blob, and the bytes are checked again (S-C5).
- `version` is read from the JSON value before the typed parse, so `{"version":2,…}` of another shape names `version`, not `json`. A missing or wrong-type field after that names `json`. Unknown keys are ignored (forward reading); `to_bytes` writes none.
- `to_bytes` is compact JSON, key order `version, host, identity, members|imported`, no trailing newline; `imported` in ULID order. Same value, same bytes, so S§7.5 blob ids are stable.
- A duplicate ack entry collapses in the `BTreeSet`; the 200 000 limit counts the array as given.
- `members.json` is `{"hosts":{"<host>":["<name>",…]}}` (serde of `MemberCache`): no identity, no time. `read_cache` keeps only hosts that pass `valid_mail_host` and names that pass `valid_member_name` (AU84: checked at each read); a missing, linked, unreadable or unparsable file is the empty cache.
- `addresses()` is host order (BTreeMap), then names as stored.

Consumes (live):
- `zygo_config::valid_mail_host(s: &str) -> bool` (`crates/zygo-config/src/schema.rs:733`, MS-3)
- `zygo_core::ids::valid_member_name(s: &str) -> bool` (`crates/zygo-core/src/ids.rs:29`, ML-1; refuses `all`, `cli`, `zygo` and the pane-key form)
- `zygo_mail_sync::address::Address { pub name: String, pub host: String }` (`src/address.rs:11`, MS-4); `zygo_mail_sync::address::ulid_file(name: &str) -> Option<ulid::Ulid>` (`address.rs:78`, MS-4)
- `zygo_mail_sync::state::SyncPaths` (`src/state.rs:17`, MS-5), `SyncPaths::new(paths: &Paths, ws_id: &str) -> std::io::Result<SyncPaths>` (`state.rs:51`); `zygo_mail_sync::state::write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()>` (`state.rs:82`, MS-5)
- `zygo_workspace::mail::ensure_mail_root(mail_dir: &std::path::Path) -> std::io::Result<()>` (`crates/zygo-workspace/src/mail/mod.rs:110`, ML-2)

Produces (exact; `crates/zygo-mail-sync/src/registry.rs`, module `zygo_mail_sync::registry`):
- `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Check(pub &'static str);`
- `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Registry { pub host: String, pub identity: String, pub members: Vec<String> }`
- `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Ack { pub host: String, pub identity: String, pub imported: BTreeSet<ulid::Ulid> }`
- `pub const REGISTRY_MAX: u64 = 65536;` `pub const ACK_MAX: u64 = 8388608;` `pub const ACK_ENTRIES_MAX: usize = 200000;` and the private `const MEMBERS_MAX: usize = 256;`
- `pub fn parse_registry(file_host: &str, mode: &str, size: u64, bytes: &[u8]) -> Result<Registry, Check>`
- `pub fn parse_ack(file_host: &str, mode: &str, size: u64, bytes: &[u8]) -> Result<Ack, Check>`
- `impl Registry { pub fn to_bytes(&self) -> Vec<u8> }`, `impl Ack { pub fn to_bytes(&self) -> Vec<u8> }`
- `#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)] pub struct MemberCache { pub hosts: BTreeMap<String, Vec<String>> }`
- `pub fn write_cache(sp: &SyncPaths, c: &MemberCache) -> std::io::Result<()>`, `pub fn read_cache(sp: &SyncPaths) -> MemberCache`
- `impl MemberCache { pub fn addresses(&self) -> Vec<String>; pub fn knows(&self, a: &Address) -> bool }`

Steps:

- [x] 1. Create `crates/zygo-mail-sync/tests/registry.rs`. Header: `//! MS-6: registry and ack files, member cache (mail sync design S§6.1, S§7.3, S§7.4, S-C5, S-C8; S§19 #12). No test uses wall-clock time; mode and link tests are cfg(unix).` Imports `serde_json::json`, `std::collections::{BTreeMap, BTreeSet}`, `std::fs`, `tempfile::TempDir`, `ulid::Ulid`, `zygo_core::paths::Paths`, `zygo_mail_sync::{address::Address, registry::*, state::SyncPaths}`. Helpers, copied from `tests/state.rs:14-33`, `:65-67`: `paths`, `setup` (ws id `0123456789abcdef`), `u(n) = Ulid::from_parts(n, n as u128)`; plus:
  ```rust
  const IDN: &str = "0123456789abcdef0123456789abcdef";
  fn j(v: serde_json::Value) -> Vec<u8> { serde_json::to_vec(&v).unwrap() }
  fn reg(host: &str, members: &[String]) -> Vec<u8> { j(json!({"version":1,"host":host,"identity":IDN,"members":members})) }
  fn names(n: usize) -> Vec<String> { (0..n).map(|i| format!("m{i}")).collect() }
  fn pr(fh: &str, b: &[u8]) -> Result<Registry, Check> { parse_registry(fh, "100644", b.len() as u64, b) }
  fn pa(fh: &str, b: &[u8]) -> Result<Ack, Check> { parse_ack(fh, "100644", b.len() as u64, b) }
  fn ack(host: &str, ids: &[String]) -> Vec<u8> { j(json!({"version":1,"host":host,"identity":IDN,"imported":ids})) }
  fn ids(n: u64) -> Vec<String> { (1..=n).map(|i| u(i).to_string()).collect() }
  ```
- [x] 2. Add the registry tests (S§7.3 Validation, S-C5):
  ```rust
  #[test] fn registry_valid_file_parses() {
      let m = vec!["alice".to_string(), "bob".into()];
      assert_eq!(pr("b", &reg("b", &m)), Ok(Registry { host: "b".into(), identity: IDN.into(), members: m }));
  }
  #[test] fn registry_refused_for_a_mode_other_than_100644() {
      let b = reg("b", &names(1));
      for m in ["100755", "120000", "160000", "040000", ""] { assert_eq!(parse_registry("b", m, b.len() as u64, &b), Err(Check("mode"))); }
  }
  #[test] fn registry_refused_over_64_kib() {
      let mut b = reg("b", &names(1));
      assert_eq!(parse_registry("b", "100644", 65537, &b), Err(Check("size")));
      b.resize(65536, b' ');
      assert!(pr("b", &b).is_ok());
      b.push(b' ');
      assert_eq!(parse_registry("b", "100644", 10, &b), Err(Check("size"))); // the bytes, not only the argument
  }
  #[test] fn registry_refused_for_bad_json() {
      for b in [b"{".to_vec(), b"\xff".to_vec(),
                j(json!({"version":1,"host":"b","identity":IDN,"members":"alice"})),
                j(json!({"version":1,"host":"b","identity":IDN}))] {
          assert_eq!(pr("b", &b), Err(Check("json")));
      }
  }
  #[test] fn registry_refused_when_version_is_not_1() {
      for v in [json!(2), json!("1"), json!(0)] {
          assert_eq!(pr("b", &j(json!({"version":v,"host":"b","identity":IDN,"members":[]}))), Err(Check("version")));
      }
      assert_eq!(pr("b", b"[]"), Err(Check("version")));
  }
  #[test] fn registry_refused_when_host_is_not_the_file_name_or_form() {
      assert_eq!(pr("b", &reg("c", &names(1))), Err(Check("host")));
      for h in ["-b", "B", &"a".repeat(33)] { assert_eq!(pr(h, &reg(h, &names(1))), Err(Check("host"))); }
  }
  #[test] fn registry_refused_for_a_bad_identity() {
      for id in [IDN.to_uppercase(), IDN[..31].to_string(), format!("{IDN}0"), "g".repeat(32)] {
          assert_eq!(pr("b", &j(json!({"version":1,"host":"b","identity":id,"members":[]}))), Err(Check("identity")));
      }
  }
  #[test] fn registry_holds_256_members_and_refuses_257() {
      assert_eq!(pr("b", &reg("b", &names(256))).unwrap().members.len(), 256);
      assert_eq!(pr("b", &reg("b", &names(257))), Err(Check("members")));
  }
  #[test] fn registry_refused_for_a_member_name_outside_m6() {
      for n in ["all", "cli", "zygo", "kabcdefgh", "Alice", "1a", "", &"a".repeat(33)] {
          assert_eq!(pr("b", &reg("b", &[n.to_string()])), Err(Check("member")), "{n}");
      }
  }
  #[test] fn registry_to_bytes_round_trips() {
      let r = Registry { host: "b".into(), identity: IDN.into(), members: vec!["alice".into(), "bob".into()] };
      assert_eq!(r.to_bytes(), format!(r#"{{"version":1,"host":"b","identity":"{IDN}","members":["alice","bob"]}}"#).into_bytes());
      assert_eq!(pr("b", &r.to_bytes()), Ok(r));
  }
  ```
- [x] 3. Run `cargo test -p zygo-mail-sync --test registry`: it fails to compile (`unresolved import zygo_mail_sync::registry::…`).
- [x] 4. In `crates/zygo-mail-sync/src/state.rs`, change `fn read_regular` (`:161`) and `fn valid_identity` (`:178`) to `pub(crate) fn`. No other change.
- [x] 5. Write `src/registry.rs`: module doc (S§6.1, S§7.3, S§7.4, S-C5, S-C8; "a refused ack counts as empty: the caller uses `parse_ack(..).map(|a| a.imported).unwrap_or_default()` and the prune removes nothing for that host"), `Check`, `Registry`, `Ack`, the four constants, `parse_registry`, `Registry::to_bytes`. Private serde types `#[derive(Serialize, Deserialize)] struct RegistryFile { version: u32, host: String, identity: String, members: Vec<String> }` and `AckFile { version: u32, host: String, identity: String, imported: Vec<String> }` (field order = key order). A private `fn head(fh: &str, mode: &str, size: u64, bytes: &[u8], max: u64) -> Result<serde_json::Value, Check>` shared by both parsers:
  ```rust
  if mode != "100644" { return Err(Check("mode")) }
  if size > max || bytes.len() as u64 > max { return Err(Check("size")) }
  let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Check("json"))?;
  if v.get("version") != Some(&serde_json::json!(1)) { return Err(Check("version")) }
  Ok(v)
  ```
  Then `serde_json::from_value::<RegistryFile>` (`Err` → `json`); a private `fn owner(fh: &str, host: &str, identity: &str) -> Result<(), Check>`: `host != fh || !valid_mail_host(host)` → `host`; `!crate::state::valid_identity(identity.as_bytes())` → `identity`. Then `members.len() > MEMBERS_MAX` → `members`; any `!valid_member_name(n)` → `member`.
- [x] 6. Run `cargo test -p zygo-mail-sync --test registry`: the 10 registry tests pass.
- [x] 7. Add the ack tests (S§7.4 Validation):
  ```rust
  #[test] fn ack_valid_file_parses() {
      let a = pa("b", &ack("b", &[u(2).to_string(), u(1).to_string(), u(1).to_string()])).unwrap();
      assert_eq!((a.host.as_str(), a.identity.as_str()), ("b", IDN));
      assert_eq!(a.imported, BTreeSet::from([u(1), u(2)]));
  }
  #[test] fn ack_refused_as_the_registry() {
      let b = ack("b", &ids(1));
      assert_eq!(parse_ack("b", "120000", b.len() as u64, &b), Err(Check("mode")));
      assert_eq!(pa("b", b"{"), Err(Check("json")));
      assert_eq!(pa("b", &j(json!({"version":2,"host":"b","identity":IDN,"imported":[]}))), Err(Check("version")));
      assert_eq!(pa("c", &b), Err(Check("host")));
      assert_eq!(pa("b", &j(json!({"version":1,"host":"b","identity":"x","imported":[]}))), Err(Check("identity")));
  }
  #[test] fn ack_refused_over_8_mib() {
      let b = ack("b", &ids(1));
      assert_eq!(parse_ack("b", "100644", 8388609, &b), Err(Check("size")));
      assert!(parse_ack("b", "100644", 8388608, &b).is_ok());
  }
  #[test] fn ack_holds_200000_entries_and_refuses_200001() {
      assert_eq!(pa("b", &ack("b", &ids(200000))).unwrap().imported.len(), 200000);
      assert_eq!(pa("b", &ack("b", &ids(200001))), Err(Check("entries")));
  }
  #[test] fn ack_refused_for_an_entry_that_is_not_a_ulid() {
      let lower = "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_lowercase();
      for e in ["not-a-ulid", lower.as_str(), "01ARZ3NDEKTSV4RRFFQ69G5FAVX", "01ARZ3NDEKTSV4RRFFQ69G5FAU"] {
          assert_eq!(pa("b", &ack("b", &[e.to_string()])), Err(Check("entry")), "{e}");
      }
  }
  #[test] fn refused_ack_is_empty_for_the_caller() {
      let r = pa("b", &ack("b", &["x".into()]));
      assert!(r.map(|a| a.imported).unwrap_or_default().is_empty());
  }
  #[test] fn ack_to_bytes_round_trips() {
      let a = Ack { host: "b".into(), identity: IDN.into(), imported: BTreeSet::from([u(2), u(1)]) };
      assert_eq!(a.to_bytes(), format!(r#"{{"version":1,"host":"b","identity":"{IDN}","imported":["{}","{}"]}}"#, u(1), u(2)).into_bytes());
      assert_eq!(pa("b", &a.to_bytes()), Ok(a));
  }
  ```
  (`…FAVX` is 27 characters; `…FAU` has the excluded letter `U`.)
- [x] 8. Run `cargo test -p zygo-mail-sync --test registry`: it fails to compile (`parse_ack`, `Ack::to_bytes` missing).
- [x] 9. Add `parse_ack` and `Ack::to_bytes` to `src/registry.rs`: `head(.., ACK_MAX)`, `AckFile`, `owner`, then `imported.len() > ACK_ENTRIES_MAX` → `entries`; each entry `crate::address::ulid_file(&format!("{e}.json"))`, `None` → `entry`.
- [x] 10. Run `cargo test -p zygo-mail-sync --test registry`: the 17 tests pass.
- [x] 11. Add the member cache tests (S§7.3 Member cache, S-C8):
  ```rust
  fn cache() -> MemberCache {
      MemberCache { hosts: BTreeMap::from([("b".into(), vec!["bob".into(), "carol".into()]), ("c".into(), vec!["dave".into()])]) }
  }
  #[cfg(unix)] #[test] fn write_cache_is_0600_atomic_and_names_only() {
      use std::os::unix::fs::PermissionsExt;
      let (_t, sp) = setup();
      write_cache(&sp, &MemberCache::default()).unwrap();
      write_cache(&sp, &cache()).unwrap();
      assert_eq!(fs::read(&sp.members).unwrap(), br#"{"hosts":{"b":["bob","carol"],"c":["dave"]}}"#);
      assert_eq!(fs::metadata(&sp.members).unwrap().permissions().mode() & 0o777, 0o600);
      let left: Vec<_> = fs::read_dir(&sp.sync).unwrap().map(|e| e.unwrap().file_name()).collect();
      assert_eq!(left, ["members.json"]); // no temp file left
  }
  #[cfg(unix)] #[test] fn write_cache_refuses_a_linked_mail_folder() {
      let (t, sp) = setup();
      let real = t.path().join("elsewhere");
      fs::create_dir(&real).unwrap();
      fs::create_dir_all(sp.mail.parent().unwrap()).unwrap();
      std::os::unix::fs::symlink(&real, &sp.mail).unwrap();
      assert_eq!(write_cache(&sp, &cache()).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
      assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
  }
  #[test] fn read_cache_has_no_age_filter() {
      let (_t, sp) = setup();
      write_cache(&sp, &cache()).unwrap();
      fs::File::options().write(true).open(&sp.members).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();
      assert_eq!(read_cache(&sp), cache());
  }
  #[test] fn read_cache_is_empty_for_a_missing_or_bad_file_and_drops_bad_names() {
      let (_t, sp) = setup();
      assert_eq!(read_cache(&sp), MemberCache::default());
      fs::create_dir_all(&sp.sync).unwrap();
      fs::write(&sp.members, b"{").unwrap();
      assert_eq!(read_cache(&sp), MemberCache::default());
      fs::write(&sp.members, br#"{"hosts":{"b":["bob","zygo","Eve"],"-x":["amy"]}}"#).unwrap();
      assert_eq!(read_cache(&sp).hosts, BTreeMap::from([("b".to_string(), vec!["bob".to_string()])]));
  }
  #[cfg(unix)] #[test] fn read_cache_refuses_a_link() {
      let (t, sp) = setup();
      let real = t.path().join("real.json");
      fs::write(&real, br#"{"hosts":{"b":["bob"]}}"#).unwrap();
      fs::create_dir_all(&sp.sync).unwrap();
      std::os::unix::fs::symlink(&real, &sp.members).unwrap();
      assert_eq!(read_cache(&sp), MemberCache::default());
  }
  #[test] fn cache_addresses_and_knows() {
      let c = cache();
      assert_eq!(c.addresses(), ["bob@b", "carol@b", "dave@c"]);
      let a = |n: &str, h: &str| Address { name: n.into(), host: h.into() };
      assert!(c.knows(&a("bob", "b")));
      assert!(!c.knows(&a("bob", "c")));
      assert!(!c.knows(&a("eve", "b")));
  }
  ```
- [x] 12. Run `cargo test -p zygo-mail-sync --test registry`: it fails to compile (`MemberCache`, `write_cache`, `read_cache` missing).
- [x] 13. Add the cache to `src/registry.rs`: `write_cache` = `zygo_workspace::mail::ensure_mail_root(&sp.mail)?` then `write_atomic(&sp.members, &serde_json::to_vec(c).map_err(std::io::Error::other)?)` (doc: the caller holds `mail/sync/lock`, S§15). `read_cache` = `crate::state::read_regular(&sp.members)`, then `serde_json::from_slice::<MemberCache>`; any `Err` or `None` → `MemberCache::default()`; then `retain` hosts that pass `valid_mail_host` and names that pass `valid_member_name`, and drop a host left with no names. `addresses` and `knows` as the Produces line and the tests fix.
- [x] 14. Run `cargo test -p zygo-mail-sync --test registry`: the 23 tests pass (20 on a non-unix target).
- [x] 15. Run the floor for this crate: `cargo fmt --all -- --check`, `cargo clippy -p zygo-mail-sync --all-targets --locked -- -D warnings`, `cargo test -p zygo-mail-sync --locked`: all exit 0 (the `state` tests still pass after step 4).
- [x] 16. Commit `crates/zygo-mail-sync/src/registry.rs`, `crates/zygo-mail-sync/src/state.rs`, `crates/zygo-mail-sync/tests/registry.rs` with `git commit -m "MS-6: registry and ack files, member cache" -m "<trailers>"`, where `<trailers>` is the `Zygo-Session` and `Zygo-Work` lines that the implement brief gives; `git log -1 --stat` shows those three files only.
