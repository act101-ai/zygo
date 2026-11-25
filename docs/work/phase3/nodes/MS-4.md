# MS-4 — Address grammar and the ULID file form

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§5 (Address, Local address), S§7.2, S§8.5 "Checks" (name form `^[0-9A-HJKMNP-TV-Z]{26}\.json$`); S§16 (M§6)
**Files:** `crates/zygo-mail-sync/src/address.rs`, `crates/zygo-mail-sync/tests/address.rs`, `docs/specs/2026-11-22-workspace-mail-design.md` (S§16 change note: M§6 gains the address grammar and the reserved name zygo)
**Consumes:** `valid_mail_host` (MS-3); the member name form and the reserved names cli and all: `valid_member_name` (ML-1), `RESERVED_MEMBER_NAMES` (ML-1); zygo_core::ids::reserved_member_name (existing, crates/zygo-core/src/ids.rs:11)
**Produces:** `zygo_mail_sync::address::Address { pub name: String, pub host: String }`, `zygo_mail_sync::address::parse(s: &str) -> Option<Address>`, `zygo_mail_sync::address::To` (enum: `Local(String)`, `Remote(Address)`, `All`), `zygo_mail_sync::address::AddrError` (enum: `Reserved`, `BadForm`), `zygo_mail_sync::address::resolve_to(s: &str, own_hosts: &[String]) -> Result<To, AddrError>`, `zygo_mail_sync::address::RESERVED: [&str; 3]`, `zygo_mail_sync::address::ulid_file(name: &str) -> Option<ulid::Ulid>`
**Acceptance:** `cargo test -p zygo-mail-sync --test address` → all pass: a bare name is `Local`; `<name>@<own host>` is `Local(<name>)`; `<name>@<other host>` is `Remote`; `all` is `All`; `cli@<host>` and `zygo@<host>` are `Reserved`; a name or host outside its form is `BadForm`; `ulid_file` accepts Crockford ULID names only (no `I`, `L`, `O`, `U`, no lower case, no other suffix).

## Plan

Premise check (live code at origin/phase-3, ac931a1d):
- `valid_mail_host` (MS-3) exists as `zygo_config::schema::valid_mail_host`, re-exported as `zygo_config::valid_mail_host` (`crates/zygo-config/src/lib.rs:19`, `pub use schema::*`). It holds as the contract says.
- `valid_member_name` and `RESERVED_MEMBER_NAMES` (ML-1) exist in `zygo_core::ids`. They hold as the contract says. `valid_member_name` also refuses the pane-key form `^k[a-z2-7]{8}$` (ML-1 review), by way of `reserved_member_name`.
- `crates/zygo-mail-sync/src/address.rs` exists and is empty (MS-2). `lib.rs` already declares `pub mod address;`. `Cargo.toml` already depends on `zygo-core`, `zygo-config` and `ulid` (3.0.0). This node changes no `Cargo.toml`.
- `ulid` 3.0.0 `Ulid::from_string` (crate `src/base32.rs:101-123`) accepts lower case. It drops the high bits of a 26-character name whose first character is above `7`, with no error. `ulid_file` therefore checks the bytes itself and requires that the decoded value prints as the same name.
- M§8 (`docs/specs/2026-11-22-workspace-mail-design.md:98`): `mail_send`'s `to` is a name, a pane key or `all`. So a bare value of pane-key form is `Local`, not `BadForm`. The contract Consumes line now names `reserved_member_name` for this.
- No review-focus item in `docs/work/phase3/graph.md` names MS-4.
- Consumers (grep of `docs/work/phase3/nodes/`): MS-6 takes `Address`; MS-12 takes `Address`, `parse`, `ulid_file`; MS-18 and MS-19 take `resolve_to`, `To`, `Address`; MS-9 takes `Address`, `To`. Each name and form below matches those lines.

Consumes (live):
- `pub fn valid_mail_host(s: &str) -> bool` (`crates/zygo-config/src/schema.rs:733`, MS-3): true exactly for `^[a-z0-9][a-z0-9-]{0,31}$`.
- `pub fn valid_member_name(s: &str) -> bool` (`crates/zygo-core/src/ids.rs:19`, ML-1): true exactly for `^[a-z][a-z0-9-]{0,31}$` and not `reserved_member_name`.
- `pub const RESERVED_MEMBER_NAMES: [&str; 2] = ["all", "cli"];` (`crates/zygo-core/src/ids.rs:7`, ML-1).
- `pub fn reserved_member_name(s: &str) -> bool` (`crates/zygo-core/src/ids.rs:11`, added by the ML-1 review; not in ML-1's Produces line, so the contract names it as existing code): in `RESERVED_MEMBER_NAMES`, or of pane-key form `^k[a-z2-7]{8}$`.
- `ulid::Ulid` (crate `ulid` 3.0.0): `pub struct Ulid(pub u128)`, `Ulid::from_string(&str) -> Result<Ulid, DecodeError>`, `Display` as 26 upper-case Crockford characters.

Produces (exact, all in `crates/zygo-mail-sync/src/address.rs`):
- `pub const RESERVED: [&str; 3] = ["all", "cli", "zygo"];`
- `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Address { pub name: String, pub host: String }`
- `#[derive(Debug, Clone, PartialEq, Eq)] pub enum To { Local(String), Remote(Address), All }`
- `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum AddrError { Reserved, BadForm }`
- `pub fn parse(s: &str) -> Option<Address>`: `Some` exactly when `s` is `<name>@<host>` (split at the first `@`), `valid_member_name(name)`, `name` not in `RESERVED`, and `valid_mail_host(host)` (which refuses a second `@`).
- `pub fn resolve_to(s: &str, own_hosts: &[String]) -> Result<To, AddrError>`, in this order:
  1. `s == "all"` → `Ok(To::All)`.
  2. `s` has no `@`: in `RESERVED` → `Err(Reserved)`; `valid_member_name(s)` or pane-key form (`reserved_member_name(s) && !RESERVED_MEMBER_NAMES.contains(&s)`) → `Ok(To::Local(s))`; else `Err(BadForm)`.
  3. Split at the first `@` into `name`, `host`: `!valid_mail_host(host)` → `Err(BadForm)`; `name` in `RESERVED` → `Err(Reserved)`; `!valid_member_name(name)` → `Err(BadForm)`; `own_hosts` holds `host` → `Ok(To::Local(name))`; else `Ok(To::Remote(Address { name, host }))`.
- `pub fn ulid_file(name: &str) -> Option<ulid::Ulid>`: `Some` exactly when `name` is `^[0-9A-HJKMNP-TV-Z]{26}\.json$` and the 26 characters decode to a `Ulid` whose `to_string()` equals them (this refuses a first character above `7`).

Steps:

- [x] 1. Create `crates/zygo-mail-sync/tests/address.rs` with this header and helpers:
  ```rust
  //! MS-4: the address grammar (mail sync design S§5, S§7.2; S§16 M§6) and the ULID file name form
  //! (S§8.5 "Checks"). Pure functions: no file, no clock.

  use ulid::Ulid;
  use zygo_core::ids::RESERVED_MEMBER_NAMES;
  use zygo_mail_sync::address::*;

  fn own() -> Vec<String> {
      vec!["studio-2".into(), "old-host".into()]
  }

  fn remote(name: &str, host: &str) -> Result<To, AddrError> {
      Ok(To::Remote(Address { name: name.into(), host: host.into() }))
  }

  fn local(name: &str) -> Result<To, AddrError> {
      Ok(To::Local(name.into()))
  }

  const GOOD: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
  ```
- [x] 2. In the same file, add the tests of `RESERVED` and `resolve_to`:
  ```rust
  #[test]
  fn reserved_names_are_all_cli_and_zygo() {
      assert_eq!(RESERVED, ["all", "cli", "zygo"]);
      assert_eq!(&RESERVED[..2], &RESERVED_MEMBER_NAMES[..]);
  }

  #[test]
  fn a_bare_name_is_local() {
      assert_eq!(resolve_to("bob", &own()), local("bob"));
      assert_eq!(resolve_to("bob", &[]), local("bob"));
      let max = format!("a{}", "b".repeat(31));
      assert_eq!(resolve_to(&max, &own()), local(&max));
      // M§8: a bare pane key is a local `to`
      assert_eq!(resolve_to("kabcdefgh", &own()), local("kabcdefgh"));
  }

  #[test]
  fn an_own_host_address_is_local() {
      assert_eq!(resolve_to("bob@studio-2", &own()), local("bob"));
      assert_eq!(resolve_to("bob@old-host", &own()), local("bob")); // an older own host (S§7.5)
  }

  #[test]
  fn another_host_address_is_remote() {
      assert_eq!(resolve_to("bob@b", &own()), remote("bob", "b"));
      assert_eq!(resolve_to("bob@studio-2", &[]), remote("bob", "studio-2"));
      let name = format!("a{}", "b".repeat(31));
      let host = "0".repeat(32);
      assert_eq!(resolve_to(&format!("{name}@{host}"), &own()), remote(&name, &host));
  }

  #[test]
  fn all_is_all() {
      assert_eq!(resolve_to("all", &own()), Ok(To::All));
      assert_eq!(resolve_to("all", &[]), Ok(To::All));
  }

  #[test]
  fn reserved_names_are_refused() {
      for s in ["cli@b", "zygo@b", "cli@studio-2", "zygo@studio-2", "all@b", "all@studio-2", "cli", "zygo"] {
          assert_eq!(resolve_to(s, &own()), Err(AddrError::Reserved), "{s:?}");
      }
  }

  #[test]
  fn a_name_or_host_outside_its_form_is_bad_form() {
      let long_name = format!("a{}", "b".repeat(32));
      let long_host = "a".repeat(33);
      let bad = [
          String::new(), "Bob".into(), "1bob".into(), "-bob".into(), "a_b".into(), "bob ".into(), long_name.clone(),
          "@b".into(), "bob@".into(), "@".into(), "Bob@b".into(), "1bob@b".into(), "kabcdefgh@b".into(),
          format!("{long_name}@b"), "bob@B".into(), "bob@-b".into(), "bob@a_b".into(), "bob@a.b".into(),
          "bob@b/c".into(), "bob@b@c".into(), "bob@ b".into(), "bob @b".into(), "bob@b\n".into(),
          format!("bob@{long_host}"), "cli@B".into(),
      ];
      for s in &bad {
          assert_eq!(resolve_to(s, &own()), Err(AddrError::BadForm), "{s:?}");
      }
  }
  ```
- [x] 3. In the same file, add the tests of `parse`:
  ```rust
  #[test]
  fn parse_takes_the_address_form() {
      assert_eq!(parse("bob@b"), Some(Address { name: "bob".into(), host: "b".into() }));
      assert_eq!(parse("x9-y@studio-2"), Some(Address { name: "x9-y".into(), host: "studio-2".into() }));
  }

  #[test]
  fn parse_refuses_others() {
      for s in ["bob", "all", "", "@", "@b", "bob@", "all@b", "cli@b", "zygo@b", "Bob@b", "bob@B", "bob@b@c",
          "kabcdefgh@b", "bob@b/c", "bob@b\n"] {
          assert_eq!(parse(s), None, "{s:?}");
      }
  }
  ```
- [x] 4. In the same file, add the tests of `ulid_file`:
  ```rust
  #[test]
  fn ulid_file_takes_crockford_ulid_names() {
      assert_eq!(ulid_file(&format!("{GOOD}.json")), Some(Ulid::from_string(GOOD).unwrap()));
      assert_eq!(ulid_file("00000000000000000000000000.json"), Some(Ulid(0)));
      assert_eq!(ulid_file("7ZZZZZZZZZZZZZZZZZZZZZZZZZ.json"), Some(Ulid(u128::MAX)));
      // every allowed character
      assert!(ulid_file("0123456789ABCDEFGHJKMNPQRS.json").is_some());
      assert!(ulid_file("00000000TVWXYZ000000000000.json").is_some());
  }

  #[test]
  fn ulid_file_refuses_other_names() {
      let lower = GOOD.to_lowercase();
      let one_lower = format!("{}v", &GOOD[..25]);
      let bad = [
          format!("{lower}.json"), format!("{one_lower}.json"),
          format!("{}I.json", &GOOD[..25]), format!("{}L.json", &GOOD[..25]),
          format!("{}O.json", &GOOD[..25]), format!("{}U.json", &GOOD[..25]),
          format!("{}.json", &GOOD[..25]), format!("{GOOD}0.json"),
          GOOD.to_string(), format!("{GOOD}.JSON"), format!("{GOOD}.json.tmp"), format!("{GOOD}.txt"),
          format!("{GOOD}json"), format!("x{GOOD}.json"), format!("{GOOD}.json\n"), format!("{GOOD}.json/"),
          format!("../{GOOD}.json"), String::new(), ".json".into(),
          "80000000000000000000000000.json".into(), "ZZZZZZZZZZZZZZZZZZZZZZZZZZ.json".into(),
          format!("{}é.json", &GOOD[..24]),
      ];
      for s in &bad {
          assert_eq!(ulid_file(s), None, "{s:?}");
      }
  }
  ```
- [x] 5. Run `cargo test -p zygo-mail-sync --test address`. Expected: compile error, `RESERVED`, `Address`, `To`, `AddrError`, `parse`, `resolve_to` and `ulid_file` not found in `zygo_mail_sync::address`.
- [x] 6. Write `crates/zygo-mail-sync/src/address.rs` as Produces states: module doc `//! Addresses (mail sync design S§5, S§7.2) and the ULID file name form (S§8.5 "Checks").`, imports `zygo_config::valid_mail_host` and `zygo_core::ids::{reserved_member_name, valid_member_name, RESERVED_MEMBER_NAMES}`, one doc line on each item that cites its spec section. `ulid_file` body: `let stem = name.strip_suffix(".json")?;` then `stem.len() == 26` and every byte matches `b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z'`, then `let u = ulid::Ulid::from_string(stem).ok()?;` and `(u.to_string() == stem).then_some(u)`. No `regex` dependency.
- [x] 7. Run `cargo test -p zygo-mail-sync --test address`. Expected: 11 passed, 0 failed.
- [x] 8. Rewrite M§6 of `docs/specs/2026-11-22-workspace-mail-design.md` (keep the heading `## 6. Names`) to this final text:
  ```markdown
  ## 6. Names

  **(author)** A loadout pane gets an optional `name` (W§11 schema addition): `^[a-z][a-z0-9-]{0,31}$`, unique in the loadout. `up` copies it into the `panes[]` entry and the member record. A hand-started member (AU9) takes `--name <n>` on `zygo claude` / `zygo opencode`, else it has no name and gets mail by pane key only. `all`, `cli`, `zygo` and every name of pane-key form `^k[a-z2-7]{8}$` (W§5) are reserved, so `mail_send`'s `to` (a name or a pane key) is never ambiguous. `zygo` is the sender of the mail sync notice for a refused message (mail sync design S§8.6); like `cli`, it has no inbox. Names are checked at each read, as AU84.

  A rename by `zygo ws name <key> <name>` changes the name; mail stays in the pane-key folder.

  **Addresses (mail sync design S§5, S§7.2).** An address is `<name>@<host>`: `<name>` is a member name as above, `<host>` is the name of one workspace on the side branch, form `^[a-z0-9][a-z0-9-]{0,31}$`. A `to` value is one of:

  - `<name>` or a pane key: a member of this machine. A bare name never means a remote member.
  - `<name>@<host>`: with a host of this workspace (mail sync design S§7.5), the same as `<name>`; with another host, a remote member (mail sync design S§8).
  - `all`: the members of this machine, as M§5. Not sent to other hosts.

  A send to an address whose name is reserved (`cli@<host>`, `zygo@<host>`) is refused: they have no inbox.
  ```
  Then add this row at the end of the table under `## Change log` in the same file:
  `| 2026-11-25 | M§6 | author's call (S§16) | Gains the address grammar (S§7.2) and the reserved name `zygo`, the sender of the S§8.6 refused-message notice, which has no inbox. |`
  Check: `grep -c "Addresses (mail sync design S§5, S§7.2)" docs/specs/2026-11-22-workspace-mail-design.md` prints 1, and the change log's last row starts `| 2026-11-25 | M§6 |`.
- [x] 9. Run `cargo test -p zygo-mail-sync` and `cargo fmt --all -- --check`. Expected: all pass, no format diff.
- [x] 10. Commit `crates/zygo-mail-sync/src/address.rs`, `crates/zygo-mail-sync/tests/address.rs` and `docs/specs/2026-11-22-workspace-mail-design.md` with message `MS-4: address grammar and the ULID file form`, then the trailers `Zygo-Session: <session>` and `Zygo-Work: <unit>` from the brief, each on its own line. No other file. Check: `git show --stat HEAD` lists exactly those three files.

Author's checks:
- Spec coverage: S§5 Address and Local address → `parse`, `resolve_to` step 3, tests `an_own_host_address_is_local`, `another_host_address_is_remote`; S§7.2 bare name, `all`, reserved `cli` and `zygo` → `a_bare_name_is_local`, `all_is_all`, `reserved_names_are_refused`; S§8.5 name form → `ulid_file_*`; S§16 M§6 → step 8. The S§7.2 refusals for no sender name, sync off, public and not in the member cache belong to MS-9, MS-18 and MS-19, not here.
- Step scan: each step is one action with a checkable result; no placeholder.
- Interface match: MS-6, MS-9, MS-12, MS-18 and MS-19 name only `Address`, `To`, `parse`, `resolve_to` and `ulid_file`, with the paths in Produces. Nothing consumes `AddrError` or `RESERVED` by name yet; they are in the contract.
- Review focus: no item names MS-4. Item 8 (hostile entry names) is tested in MS-10 and MS-12; `ulid_file_refuses_other_names` holds `../`, newline and `/` cases that they rely on.
- Proportion: one source file of pure functions, one test file of 11 tests, one spec section.
- Live code: every Consumes line cites the file and line at ac931a1d.

### Departure

- Step 9, `cargo fmt --all -- --check`: the expected "no format diff" does not hold against the live code. The repository has no `rustfmt.toml`, and the check reports differences in 295 files at the base (ac931a1d plus this node), most of them in files this node does not change. CI runs no format check (`.github/workflows/ci.yml:45` runs `cargo test --workspace --locked --no-fail-fast` only), and the phase 2 ledger already records this (`docs/work/phase2/graph.md:138`). `address.rs` and `tests/address.rs` keep the repository's existing wide-line style and the test text of steps 1-4 as written; they are not reformatted.
