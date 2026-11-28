# WD-3 — Workspace approvals in `approvals.toml`

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§7.2, R§11 rows "Approvals file invalid" and "Approval write fails", R§18 items 8 (the store key) and 21
**Files:** `crates/zygo-config/src/repo_def/mod.rs`, `crates/zygo-config/src/repo_def/store.rs` (new), `crates/zygo-config/tests/repo_def_store.rs` (new), `crates/zygo-plugins/tests/approve_shared.rs` (new)
**Consumes:** `digest` (WD-2)
**Produces:** `repo_def::is_workspace_approved_in(file: &Path, ws_id: &str, digest: &str) -> bool`, `repo_def::approve_workspace_in(file: &Path, ws_id: &str, digest: &str) -> anyhow::Result<Stored>`, `repo_def::Stored { Added, AlreadyPresent }`
**Acceptance:** `cargo test -p zygo-config --test repo_def_store && cargo test -p zygo-plugins --test approve_shared` → all pass. The tests cover: an entry `[[workspace]]` with `id` (16 hex) and `digest` (64 hex) appended with every earlier byte kept (lock on `<file>.lock`, temp file, `fsync`, rename, as `crates/zygo-plugins/src/approve.rs:38-77`); a second approve of the same pair is `AlreadyPresent` and the file is byte-equal; another `ws_id` with the same digest is not approved; an invalid approvals file approves nothing, `approve_workspace_in` returns `Err` and the file is byte-equal; a failed write leaves no temp file (`cfg(unix)` read-only directory case; the invalid-file case runs everywhere); `zygo_plugins::approve::is_approved_in` finds each plugin approval in a file that also holds `[[workspace]]` entries, and `zygo_plugins::approve::approve_in` keeps those entries byte for byte (R§18 item 21)

## Plan

**Premises checked against the live code (branch `phase3/WD-3` at `57cc1691`, equal to `origin/phase-3`):**

- WD-2 landed (`adb3069b`, PR #72): `crates/zygo-config/src/repo_def/mod.rs:8` re-exports `digest::{digest, short}`; `digest(files: &Files) -> String` (`repo_def/digest.rs:8`) returns 64 lower-case hex. The store does not call it; the tests use it to make real digests.
- `crates/zygo-config/src/repo_def/` has `mod.rs`, `read.rs`, `shape.rs`, `digest.rs`; no `store.rs` and no symbol `Stored`, `approve_workspace_in` or `is_workspace_approved_in` in the crate. The spec R§16 names one file `repo_def.rs`; R§17 leaves the module split to the builder, so `repo_def/store.rs` is within the spec.
- The protocol to copy is `zygo_plugins::approve::approve_in` (`crates/zygo-plugins/src/approve.rs:38-77`): lock `<file>.lock` with `fs4::FileExt::lock` (`:44-47`), read (missing file = empty, `:49-53`), parse or `Err` with the file left as it is (`:54`), append as text with a blank line before (`:56-61`), parse again (`:62`), temp file `.<name>.<ulid>.tmp` with `create_new`, `write_all`, `sync_all`, `rename`, and `remove_file(tmp)` on any error (`:64-74`). The reader (`:30-34`) returns `false` for an unreadable or invalid file.
- R§18 item 21 needs no change in `zygo-plugins`: its `File` (`approve.rs:16-17`) is a serde struct with no `deny_unknown_fields`, so a `[[workspace]]` array is ignored by `is_approved_in` and `approve_in`. WD-3 proves this with tests only.
- Crate graph: `zygo-plugins` depends on `zygo-config` (`crates/zygo-plugins/Cargo.toml:9`), not the reverse, so `zygo-config` cannot call `approve_in`; the store copies the protocol (as the spec says, "uses the protocol of `approve_in`"). `zygo-config` already has `fs4`, `serde`, `toml`, `anyhow`, `zygo-core` (`crates/zygo-config/Cargo.toml`) and `libc` for unix (`[target.'cfg(unix)'.dependencies]`); `tempfile` is a dev-dependency of both crates. No new dependency.
- Reuse check: `zygo_core::session_dir::write_atomic` (`crates/zygo-core/src/session_dir.rs:141-156`) does temp, `sync_all`, rename, but on a failed rename `tmp.persist(target)?` boxes `tempfile::PersistError`, which owns the `NamedTempFile`; the temp file stays until the caller drops the error. R§11 "the temp file is removed" is then false at return. The store uses the explicit `approve.rs:64-74` body instead.
- Workspace id form: `^[0-9a-f]{16}$` (`docs/specs/2026-11-20-workspaces-design.md` W§5, "Workspace id"); digest form 64 lower-case hex (R§6.5), the same `hex64` test as `approve.rs:21`.
- `Paths.approvals_file` is `data_dir.join("approvals.toml")` (`crates/zygo-core/src/paths.rs:50`). WD-3 takes the file as a parameter only (`*_in`), as the contract says; no `Paths` wrapper is produced.
- Consumers: WD-4 (`is_workspace_approved_in`, `nodes/WD-4.md:5`), WD-15 (`approve_workspace_in`, `Stored`, `nodes/WD-15.md:5`). Both use the contract names and signatures below unchanged.
- Review focus item 32 (graph "Review focus": approval keyed by digest alone; a plugin approvals file broken by `[[workspace]]` entries) is covered by Step 1 `approval_is_keyed_by_id_and_digest` and Step 3 `plugin_approvals_hold_beside_workspace_entries`, `plugin_approve_keeps_workspace_entries_byte_for_byte`.
- No spec premise drifted. The spec is not changed.

### Interfaces

Consumes (as in the code now):

```rust
// crates/zygo-config/src/repo_def/digest.rs:8 (WD-2), tests only
pub fn digest(files: &Files) -> String;
// crates/zygo-plugins/src/approve.rs:30, :38 (existing), approve_shared tests only
pub fn is_approved_in(file: &Path, m: &str, p: &str, digest: &str) -> bool;
pub fn approve_in(file: &Path, m: &str, p: &str, digest: &str) -> anyhow::Result<()>;
```

Produces (`crates/zygo-config/src/repo_def/store.rs`, re-exported from `repo_def/mod.rs`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored { Added, AlreadyPresent }
pub fn is_workspace_approved_in(file: &Path, ws_id: &str, digest: &str) -> bool;
pub fn approve_workspace_in(file: &Path, ws_id: &str, digest: &str) -> anyhow::Result<Stored>;
```

### Steps

- [x] **Step 1 — store tests.** New `crates/zygo-config/tests/repo_def_store.rs`, header `//! WD-3: workspace approvals in approvals.toml (R§7.2, R§11, R§18 item 8).` Helpers:

  ```rust
  use std::path::{Path, PathBuf};
  use zygo_config::repo_def::{self, approve_workspace_in as approve, is_workspace_approved_in as approved, Files, Stored};
  const ID: &str = "3f9a1c0d5e7b2a46";
  const ID2: &str = "0123456789abcdef";
  fn d(text: &str) -> String { repo_def::digest(&Files { full_ref: None, commit: "a".repeat(40), workspace: Some(text.as_bytes().to_vec()), profiles: Default::default(), ignored: vec![] }) }
  fn entry(id: &str, digest: &str) -> String { format!("[[workspace]]\nid = \"{id}\"\ndigest = \"{digest}\"\n") }
  fn names(dir: &Path) -> Vec<String> { /* sorted file names in dir */ }
  fn store(t: &tempfile::TempDir) -> PathBuf { t.path().join("approvals.toml") }
  ```

  Tests (D = `d("version = 1\n")`, D2 = `d("version = 1\n# 2\n")`):

  ```rust
  #[test] fn approve_writes_one_entry_to_a_new_store() {
      // file and its folder absent: `t.path().join("sub/approvals.toml")`
      assert_eq!(approve(&f, ID, &D).unwrap(), Stored::Added);
      assert_eq!(fs::read_to_string(&f).unwrap(), entry(ID, &D));
      assert!(approved(&f, ID, &D));
      assert_eq!(names(f.parent().unwrap()), ["approvals.toml", "approvals.toml.lock"]); // lock kept, no temp file
  }
  #[test] fn approve_keeps_every_earlier_byte() {
      let before = format!("# mine\n[[approval]]\nmarketplace = \"m\"\nplugin = \"p\"\ndigest = \"{}\"\n[[workspace]]\nid = \"{ID2}\"\ndigest = \"{D}\"", "a".repeat(64)); // no final newline
      fs::write(&f, &before);
      assert_eq!(approve(&f, ID, &D2).unwrap(), Stored::Added);
      assert_eq!(fs::read_to_string(&f).unwrap(), format!("{before}\n\n{}", entry(ID, &D2)));
      assert!(approved(&f, ID2, &D) && approved(&f, ID, &D2)); // the earlier entry stays (R§7.2 "never removed")
  }
  #[test] fn second_approve_is_already_present_and_byte_equal() {
      approve(&f, ID, &D); let b = fs::read(&f);
      assert_eq!(approve(&f, ID, &D).unwrap(), Stored::AlreadyPresent);
      assert_eq!(fs::read(&f).unwrap(), b);
  }
  #[test] fn approval_is_keyed_by_id_and_digest() {           // R§18 item 8, review focus 32
      approve(&f, ID, &D);
      assert!(!approved(&f, ID2, &D));   // same digest, another workspace
      assert!(!approved(&f, ID, &D2));   // same workspace, another digest
      assert_eq!(approve(&f, ID2, &D).unwrap(), Stored::Added); // a second entry, not AlreadyPresent
  }
  #[test] fn missing_store_approves_nothing() { assert!(!approved(&t.path().join("none.toml"), ID, &D)); }
  #[test] fn invalid_store_approves_nothing_and_is_left_as_it_is() {
      for text in [
          format!("{}[x\n", entry(ID, &D)),                      // not TOML
          "workspace = 1\n".to_string(),                          // not an array of tables
          format!("[[workspace]]\nid = 1\ndigest = \"{D}\"\n"),   // id not a string
          format!("[[workspace]]\nid = \"{ID}\"\n"),              // digest missing
      ] {
          fs::write(&f, &text);
          assert!(!approved(&f, ID, &D));
          let e = approve(&f, ID, &D).unwrap_err();
          assert!(format!("{e:#}").contains("invalid approvals file"), "{e:#}");
          assert_eq!(fs::read_to_string(&f).unwrap(), text);
          assert_eq!(names(t.path()), ["approvals.toml", "approvals.toml.lock"]);
      }
  }
  #[test] fn malformed_ids_and_digests_are_refused() {
      for (id, dg) in [("3F9A1C0D5E7B2A46", D.clone()), ("3f9a1c0d5e7b2a4", D.clone()), ("3f9a1c0d5e7b2a46x", D.clone()), (ID, D[..63].to_string()), (ID, D.to_uppercase())] {
          assert!(approve(&f, id, &dg).is_err());
          assert!(!f.exists());
      }
      fs::write(&f, entry("3F9A1C0D5E7B2A46", &D)); assert!(!approved(&f, "3F9A1C0D5E7B2A46", &D));
  }
  #[cfg(unix)] #[test] fn failed_write_leaves_no_temp_file() {
      // skipped with an eprintln when libc::geteuid() == 0 (root writes in a read-only folder), as
      // crates/zygo-hooks/tests/workspace.rs:1537-1540
      approve(&f, ID, &D);                                  // file and lock file exist
      let b = fs::read(&f); let n = names(t.path());
      set_permissions(t.path(), 0o500);                     // restored before the asserts that can fail
      let r = approve(&f, ID2, &D);
      set_permissions(t.path(), 0o700);
      assert!(r.is_err());
      assert_eq!(fs::read(&f).unwrap(), b);
      assert_eq!(names(t.path()), n);                       // no `.approvals.toml.*.tmp`
      assert!(!approved(&f, ID2, &D));
  }
  #[test] fn parallel_approves_all_land() {
      // 4 threads, ids "000000000000000{i}", same D; each returns Ok(Stored::Added); afterwards all 4 are approved
      // and the file parses with 4 `[[workspace]]` entries. Outcome only; no time assert.
  }
  ```

- [x] **Step 2 — see them fail.** `cargo test -p zygo-config --test repo_def_store` → compile error E0432 (no `approve_workspace_in`, `is_workspace_approved_in`, `Stored` in `repo_def`).

- [x] **Step 3 — shared-file tests.** New `crates/zygo-plugins/tests/approve_shared.rs`, header `//! WD-3: plugin approvals beside [[workspace]] entries in one approvals.toml (R§7.2, R§18 item 21).` Uses `zygo_plugins::approve::{approve_in, is_approved_in}` and `zygo_config::repo_def::{approve_workspace_in, is_workspace_approved_in}`; P1 = `"a".repeat(64)`, P2 = `"b".repeat(64)`, W = `"c".repeat(64)`.

  ```rust
  #[test] fn plugin_approvals_hold_beside_workspace_entries() {
      approve_in(&f, "m1", "p1", &P1); approve_workspace_in(&f, ID, &W); approve_in(&f, "m2", "p2", &P2); approve_workspace_in(&f, ID2, &W);
      assert!(is_approved_in(&f, "m1", "p1", &P1) && is_approved_in(&f, "m2", "p2", &P2));
      assert!(!is_approved_in(&f, "m1", "p1", &P2));
      assert!(is_workspace_approved_in(&f, ID, &W) && is_workspace_approved_in(&f, ID2, &W));
      // also a hand-written file whose `[[workspace]]` entries come first and carry an extra key `note = "x"`:
      // is_approved_in finds the plugin approval written after them
  }
  #[test] fn plugin_approve_keeps_workspace_entries_byte_for_byte() {
      approve_workspace_in(&f, ID, &W); approve_workspace_in(&f, ID2, &W);
      let before = fs::read_to_string(&f).unwrap();
      approve_in(&f, "m", "p", &P1).unwrap();
      let after = fs::read_to_string(&f).unwrap();
      assert!(after.starts_with(&before));
      assert_eq!(&after[before.len()..], format!("\n[[approval]]\nmarketplace = \"m\"\nplugin = \"p\"\ndigest = \"{P1}\"\n"));
      assert!(is_workspace_approved_in(&f, ID, &W) && is_workspace_approved_in(&f, ID2, &W));
  }
  ```

  `cargo test -p zygo-plugins --test approve_shared` → compile error E0432 (as Step 2).

- [x] **Step 4 — the store.** New `crates/zygo-config/src/repo_def/store.rs`, doc line `//! Workspace approvals (R§7.2): [[workspace]] id digest in the plugin approvals file, written with the protocol of zygo_plugins::approve::approve_in.` In `repo_def/mod.rs` add `mod store;` and `pub use store::{approve_workspace_in, is_workspace_approved_in, Stored};`; extend the module doc with "and the approval store (R§7.2)".
  - Private shape: `#[derive(Deserialize)] struct File { #[serde(default)] workspace: Vec<Entry> }`, `#[derive(Deserialize)] struct Entry { id: String, digest: String }` (no `deny_unknown_fields`; other tables such as `[[approval]]` are ignored, as `approve.rs:16-19` ignores `[[workspace]]`). "Invalid" is exactly: not TOML, or this shape does not deserialize.
  - Private `fn id16(s) -> bool` (`^[0-9a-f]{16}$`) and `fn hex64(s) -> bool` (as `approve.rs:21`).
  - `is_workspace_approved_in`: `id16(ws_id) && hex64(digest)`, read to string, parse `File`, any entry with `id == ws_id && digest == digest`; every failure is `false`.
  - `approve_workspace_in`: the `approve.rs:38-77` body with these changes only: input check `bail!("workspace id is not 16 lower-case hex characters")` / `bail!("digest is not 64 lower-case hex characters")` before any file system call; the duplicate test runs on the `File` parsed from the text read under the lock (one read; return `Ok(Stored::AlreadyPresent)` with nothing written); the appended text is `format!("[[workspace]]\nid = {}\ndigest = {}\n", q(ws_id), q(digest))`; the second parse error is `"{file}: workspace is not an array of tables"`; success returns `Ok(Stored::Added)`. Error context on the first parse stays `"{file}: invalid approvals file"`.

- [x] **Step 5 — acceptance.** `cargo test -p zygo-config --test repo_def_store && cargo test -p zygo-plugins --test approve_shared` → all pass (9 + 2). Then `cargo test -p zygo-config -p zygo-plugins --locked` → no other test changes; `cargo build --workspace --locked` → builds (no other crate touched).
