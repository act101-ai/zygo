# WD-4 — Checked use: one read, one digest, one store lookup per use

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§7.3, R§7.4 (the pinned check and its outcomes), R§6.1 step 1 (the approval key is the workspace id), R-C2, R§18 items 6 (counting reader) and 8
**Files:** `crates/zygo-config/src/repo_def/mod.rs`, `crates/zygo-config/src/repo_def/check.rs` (new), `crates/zygo-config/tests/repo_def_check.rs` (new)
**Consumes:** `read_default` (WD-1), `read_at` (WD-1), `Read` (WD-1), `Files` (WD-1), `GitRunner` (WD-1), `parse` (WD-2), `digest` (WD-2), `Definition` (WD-2), `is_workspace_approved_in` (WD-3)
**Produces:** `repo_def::Pin { commit: String, digest: String }` (derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`), `repo_def::Use { Absent { full_ref: String }, Unread { why: String }, Invalid { full_ref: Option<String>, commit: String, why: String }, NotApproved { full_ref: String, files: Files, digest: String, def: Definition }, Approved { full_ref: String, pin: Pin, files: Files, def: Definition } }`, `repo_def::check_default(top: &Path, ws_id: &str, approvals: &Path, git: &dyn GitRunner) -> Use`, `repo_def::PinnedUse { Approved { def: Definition }, Gone, NotApproved, Unread { why: String } }`, `repo_def::check_pinned(top: &Path, ws_id: &str, pin: &Pin, approvals: &Path, git: &dyn GitRunner) -> PinnedUse`
**Acceptance:** `cargo test -p zygo-config --test repo_def_check` → all pass. The tests cover: a counting `GitRunner` shows one read per `check_default` call, and the `def` of `Approved` is the parse of the bytes whose digest was looked up (a runner whose content changes between two calls gives `NotApproved` for the second call, never a mix); the same `.zygo/` committed in a second repository (another workspace id) is `NotApproved` there (R§18 item 8); `check_pinned` gives `Approved` when the pinned read's digest equals `pin.digest` and the pair is stored, `NotApproved` after the entry is removed by hand, `Gone` for a pruned commit, `Unread` for a failing runner, and never `Approved` for a read whose digest differs from `pin.digest`

## Plan

**Premise check (2026-11-25, at `268b0a07`).** WD-1, WD-2 and WD-3 are DONE (graph rows WD-2, WD-3; WD-1 in the log). The consumed items are exported at `crates/zygo-config/src/repo_def/mod.rs:9-12`. Nothing named `check_default`, `check_pinned`, `Pin` or `PinnedUse` exists in `crates/`. `serde` has `derive` (root `Cargo.toml:61`; `crates/zygo-config/Cargo.toml:11`). No drift.

### Interfaces

Consumes, as the code is now:

- `read.rs:16` `pub trait GitRunner { fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String>; }`
- `read.rs:47` `pub struct Files { pub full_ref: Option<String>, pub commit: String, pub workspace: Option<Vec<u8>>, pub profiles: BTreeMap<String, Vec<u8>>, pub ignored: Vec<String> }` (derives `Debug, Clone, PartialEq, Eq`)
- `read.rs:61` `pub enum Read { Absent { full_ref: Option<String> }, Unread { why: String }, Invalid { full_ref: Option<String>, commit: String, why: String }, Gone { commit: String }, Found { files: Files } }`
- `read.rs:77` `pub fn read_default(top: &Path, git: &dyn GitRunner) -> Read`. Its `Absent` always holds `Some(full_ref)` (`read.rs:98`, `:110`, `steps` passes `Some`). It never returns `Gone` (mapped to `Unread` at `read.rs:121-124`).
- `read.rs:128` `pub fn read_at(top: &Path, commit: &str, git: &dyn GitRunner) -> Read` (`full_ref: None` in all results)
- `shape.rs:54` `pub fn parse(files: &Files) -> Result<Definition, String>`; `shape.rs:11` `pub struct Definition { pub workspace: Option<WorkspaceFile>, pub profiles: BTreeMap<String, RepoProfile> }` (derives `Debug, Clone, PartialEq, Eq`)
- `digest.rs:8` `pub fn digest(files: &Files) -> String`
- `store.rs:50` `pub fn is_workspace_approved_in(file: &Path, ws_id: &str, digest: &str) -> bool`

Produces (new file `crates/zygo-config/src/repo_def/check.rs`, re-exported from `repo_def`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin { pub commit: String, pub digest: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Use {
    Absent { full_ref: String },
    Unread { why: String },
    Invalid { full_ref: Option<String>, commit: String, why: String },
    NotApproved { full_ref: String, files: Files, digest: String, def: Definition },
    Approved { full_ref: String, pin: Pin, files: Files, def: Definition },
}
pub fn check_default(top: &Path, ws_id: &str, approvals: &Path, git: &dyn GitRunner) -> Use;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedUse { Approved { def: Definition }, Gone, NotApproved, Unread { why: String } }
pub fn check_pinned(top: &Path, ws_id: &str, pin: &Pin, approvals: &Path, git: &dyn GitRunner) -> PinnedUse;
```

The `Debug, Clone, PartialEq, Eq` derives on `Use` and `PinnedUse` are added so the tests can use `assert_eq!`. The caller computes `ws_id` (R§6.1 step 1, `zygo-workspace` identity). `zygo-config` does not depend on `zygo-workspace`, so the id is a parameter.

### Steps

- [ ] **1. Write the failing tests** in `crates/zygo-config/tests/repo_def_check.rs` (new). Header: `//! WD-4: checked use (R§7.3, R§7.4; R§18 items 6, 8).` Constants: `MAIN = "refs/remotes/origin/main"`, `ID = "3f9a1c0d5e7b2a46"`, `ID2 = "0123456789abcdef"`, `A = "version = 1\nlayout = \"main-vertical\"\n"`, `B = "version = 1\nlayout = \"even-vertical\"\n"` (A and B have the same length, so a swapped blob passes the size check of R§6.1 step 5). Helpers, copied from `tests/repo_def_read.rs:15-38` and changed as noted:
  - `git_cmd`, `git`, `blob`: as `repo_def_read.rs:15-38`.
  - `commit_files(dir, ents: &[(&str, &[u8])])`: `update-index --add --cacheinfo 100644,<blob>,<path>` for each, then `commit -q -m c` (as `commit_index`, `repo_def_read.rs:45-56`, regular files only).
  - `struct Fx { _t: TempDir, w: PathBuf, c: PathBuf }` and `fx(ents)`: as `repo_def_read.rs:63-76` (bare `o.git`, `w` with a `README` seed commit, then `ents`, push, `clone -q --no-checkout o.git c`). `ents` must not be empty: an empty commit fails.
  - `push_change(f, ents)`: `commit_files(&f.w, ents)`, `git(&f.w, ["push","-q","origin","main"])`, `git(&f.c, ["fetch","-q","origin"])`.
  - `head(c) = git(c, ["rev-parse", MAIN])`.
  - `ws(text) -> Files` with `full_ref: None`, `commit: "a".repeat(40)`, `workspace: Some(text bytes)`, empty `profiles` and `ignored`; `d(text) = repo_def::digest(&ws(text))`; `parsed(text) = repo_def::parse(&ws(text)).unwrap()`.
  - `Cnt`, the counting `GitRunner`: `calls: RefCell<Vec<Vec<String>>>`, `fail: bool`, `swap: Option<(usize, &'static [u8])>`. Each call is recorded. With `fail` it returns `Err("tool not found")`. Else it runs `SystemGit` with `Instant::now() + Duration::from_secs(600)`. When `swap = Some((n, bytes))` and the count of recorded `symbolic-ref` calls is at least `n`, the stdout of a `cat-file blob` call is replaced by `bytes`. `Cnt::reads()` = the count of calls whose `args[0] == "symbolic-ref"` (one per `read_default`, `read.rs:80`). `Cnt::blobs()` = the count of `cat-file blob` calls.
  - `store(f) -> PathBuf = f._t.path().join("approvals.toml")`; approve with `repo_def::approve_workspace_in(&p, id, &digest).unwrap()`.

  Imports: `use zygo_config::repo_def::{self, Files, GitRunner, Pin, PinnedUse, SystemGit, Use};` and the `std` items the helpers use.

  Tests:

  ```rust
  #[test] // R§6.1 results pass through; a parse failure is Invalid at the read commit
  fn absent_unread_invalid() {
      let f = fx(&[("src/x", b"x")]);
      let s = store(&f);
      assert_eq!(repo_def::check_default(&f.c, ID, &s, &Cnt::default()), Use::Absent { full_ref: MAIN.into() });
      let fail = Cnt { fail: true, ..Cnt::default() };
      assert_eq!(repo_def::check_default(&f.c, ID, &s, &fail), Use::Unread { why: "git symbolic-ref: tool not found".into() });
      let f = fx(&[(".zygo", b"x")]);
      assert_eq!(repo_def::check_default(&f.c, ID, &s, &Cnt::default()),
                 Use::Invalid { full_ref: Some(MAIN.into()), commit: head(&f.c), why: ".zygo is not a directory".into() });
      let f = fx(&[(".zygo/workspace.toml", b"version = 2\n")]);
      let why = repo_def::parse(&ws("version = 2\n")).unwrap_err();
      assert_eq!(repo_def::check_default(&f.c, ID, &store(&f), &Cnt::default()),
                 Use::Invalid { full_ref: Some(MAIN.into()), commit: head(&f.c), why });
  }

  #[test] // R§18 item 6; R§7.3: one read, one digest, one lookup; the def applied is the read checked
  fn one_read_per_use_and_never_a_mix() {
      let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
      let s = store(&f);
      repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
      let r = Cnt { swap: Some((2, B.as_bytes())), ..Cnt::default() };
      let Use::Approved { full_ref, pin, files, def } = repo_def::check_default(&f.c, ID, &s, &r) else { panic!() };
      assert_eq!((r.reads(), r.blobs()), (1, 1));
      assert_eq!((full_ref.as_str(), pin.clone()), (MAIN, Pin { commit: head(&f.c), digest: d(A) }));
      assert_eq!(files.workspace.as_deref(), Some(A.as_bytes()));
      assert_eq!(def, parsed(A));
      let Use::NotApproved { digest, def, files, .. } = repo_def::check_default(&f.c, ID, &s, &r) else { panic!() };
      assert_eq!((r.reads(), r.blobs()), (2, 2));
      assert_eq!((digest, def, files.workspace.as_deref()), (d(B), parsed(B), Some(B.as_bytes())));
      // The inverse: only B is approved. The first use (A) is NotApproved, the second (B) is Approved with B's def.
      let s2 = f._t.path().join("approvals-b.toml");
      repo_def::approve_workspace_in(&s2, ID, &d(B)).unwrap();
      let r = Cnt { swap: Some((2, B.as_bytes())), ..Cnt::default() };
      assert!(matches!(repo_def::check_default(&f.c, ID, &s2, &r), Use::NotApproved { .. }));
      let Use::Approved { pin, def, .. } = repo_def::check_default(&f.c, ID, &s2, &r) else { panic!() };
      assert_eq!((pin.digest, def), (d(B), parsed(B)));
      assert_eq!(r.reads(), 2);
  }

  #[test] // R§18 item 8; R§6.1 step 1: the key is the workspace id
  fn same_files_in_another_repository_are_not_approved() {
      let (f1, f2) = (fx(&[(".zygo/workspace.toml", A.as_bytes())]), fx(&[(".zygo/workspace.toml", A.as_bytes())]));
      let s = store(&f1);
      repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
      assert!(matches!(repo_def::check_default(&f1.c, ID, &s, &Cnt::default()), Use::Approved { .. }));
      let Use::NotApproved { digest, .. } = repo_def::check_default(&f2.c, ID2, &s, &Cnt::default()) else { panic!() };
      assert_eq!(digest, d(A));
  }

  #[test] // R§7.4: the pinned read, not the default branch
  fn pinned_approved_reads_the_pin() {
      let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
      let s = store(&f);
      repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
      let Use::Approved { pin, .. } = repo_def::check_default(&f.c, ID, &s, &Cnt::default()) else { panic!() };
      push_change(&f, &[(".zygo/workspace.toml", B.as_bytes())]);
      repo_def::approve_workspace_in(&s, ID, &d(B)).unwrap();
      let r = Cnt::default();
      assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &r), PinnedUse::Approved { def: parsed(A) });
      assert_eq!(r.reads(), 0);
      assert!(r.calls.borrow().iter().all(|c| c.iter().all(|a| !a.starts_with("refs/"))));
      // R§7.2 ruling 4: the user removes the entry by hand.
      std::fs::write(&s, format!("[[workspace]]\nid = \"{ID}\"\ndigest = \"{}\"\n", d(B))).unwrap();
      assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &Cnt::default()), PinnedUse::NotApproved);
  }

  #[test] // R§7.4: gone, unread, and never Approved for a digest other than pin.digest
  fn pinned_gone_unread_and_other_digest() {
      let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
      let s = store(&f);
      repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
      repo_def::approve_workspace_in(&s, ID, &d(B)).unwrap();
      let c = head(&f.c);
      // A commit that the clone does not have, as after a force-push and gc.
      commit_files(&f.w, &[(".zygo/workspace.toml", B.as_bytes())]);
      let gone = git(&f.w, &["rev-parse", "HEAD"]);
      assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: gone, digest: d(B) }, &s, &Cnt::default()), PinnedUse::Gone);
      let fail = Cnt { fail: true, ..Cnt::default() };
      assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: c.clone(), digest: d(A) }, &s, &fail),
                 PinnedUse::Unread { why: "git ls-tree: tool not found".into() });
      // Both digests are stored; the read at c gives d(A), so a pin of d(B) at c is not approved.
      assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: c.clone(), digest: d(B) }, &s, &Cnt::default()), PinnedUse::NotApproved);
      // The seed commit has no .zygo/ (Read::Absent): never Approved.
      let seed = git(&f.c, &["rev-parse", &format!("{MAIN}~1")]);
      assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: seed, digest: d(A) }, &s, &Cnt::default()), PinnedUse::NotApproved);
      // The other id is not approved.
      assert_eq!(repo_def::check_pinned(&f.c, ID2, &Pin { commit: c, digest: d(A) }, &s, &Cnt::default()), PinnedUse::NotApproved);
  }
  ```

  Check: the file exists and holds the 5 tests above.

- [ ] **2. Run the tests and see them fail.** `cargo test -p zygo-config --test repo_def_check`. Check: compile error, `check_default`, `check_pinned`, `Pin`, `PinnedUse` and `Use` not found in `repo_def`.

- [ ] **3. Add `crates/zygo-config/src/repo_def/check.rs`** with the Produces items above. Module doc: `//! The check at each use (R§7.3) and the pinned check (R§7.4): one read, one digest, one store lookup.` In `mod.rs` add `mod check;` and `pub use check::{check_default, check_pinned, Pin, PinnedUse, Use};`, and add "the check at each use (R§7.3, R§7.4)" to the module doc. Result mapping. All of it acts on the one `Read` value that the function got.
  - `check_default`: one `read_default(top, git)`.
    - `Absent { full_ref }` → `Use::Absent { full_ref: full_ref.unwrap_or_default() }`.
    - `Unread { why }` → `Use::Unread { why }`.
    - `Invalid { full_ref, commit, why }` → `Use::Invalid` with the same fields.
    - `Gone { commit }` (not returned now) → `Use::Unread { why: format!("commit {commit} or one of its objects is missing") }` (the text of `read.rs:122`).
    - `Found { files }`: first `parse(&files)`. `Err(why)` → `Use::Invalid { full_ref: files.full_ref, commit: files.commit, why }`. `Ok(def)` → `let digest = digest(&files)`. Then one `is_workspace_approved_in(approvals, ws_id, &digest)`. `true` → `Approved { full_ref: files.full_ref.clone().unwrap_or_default(), pin: Pin { commit: files.commit.clone(), digest }, files, def }`. `false` → `NotApproved { full_ref, files, digest, def }`.
    - No `unwrap`/`expect`: a hook may call this (C6).
  - `check_pinned`: one `read_at(top, &pin.commit, git)`.
    - `Gone { .. }` → `Gone`.
    - `Unread { why }` → `Unread { why }`.
    - `Absent` or `Invalid` → `NotApproved`.
    - `Found { files }` → `NotApproved` unless all three hold: `digest(&files) == pin.digest`, `is_workspace_approved_in(approvals, ws_id, &pin.digest)`, and `parse(&files)` is `Ok(def)`. Then → `Approved { def }`. Do the digest comparison first. Do the store lookup only on equal digests.

  Check: `cargo build -p zygo-config` succeeds with no warnings.

- [ ] **4. Run the tests and see them pass.** `cargo test -p zygo-config --test repo_def_check`. Check: 5 passed, 0 failed.

- [ ] **5. Run the floor for the crate.** `cargo test -p zygo-config --locked`, then the graph floor `cargo test --workspace --locked --no-fail-fast`. Check: all pass. `repo_def_read`, `repo_def_shape` and `repo_def_store` are unchanged and green.

- [ ] **6. Commit** only the three files: `git add crates/zygo-config/src/repo_def/mod.rs crates/zygo-config/src/repo_def/check.rs crates/zygo-config/tests/repo_def_check.rs`, then `git commit -m "WD-4: checked use of the workspace definition (check_default, check_pinned, Pin)" -m <the two trailer lines that the implement brief names>`. Check: `git show --stat HEAD` lists exactly those 3 files.

### Coverage

| Acceptance item | Test |
|---|---|
| A counting `GitRunner` shows one read per `check_default` call | `one_read_per_use_and_never_a_mix` (`reads()`, `blobs()`) |
| The `def` of `Approved` is the parse of the bytes whose digest was looked up; content that changes between two calls gives `NotApproved`, never a mix | `one_read_per_use_and_never_a_mix` (both directions) |
| The same `.zygo/` in a second repository (another id) is `NotApproved` (R§18 item 8) | `same_files_in_another_repository_are_not_approved` |
| `check_pinned`: `Approved` when the digest equals and the pair is stored | `pinned_approved_reads_the_pin` |
| `NotApproved` after the entry is removed by hand | `pinned_approved_reads_the_pin` |
| `Gone` for a pruned commit | `pinned_gone_unread_and_other_digest` (a commit the clone lacks; `read_at` gives `Gone` per `read.rs:209`) |
| `Unread` for a failing runner | `pinned_gone_unread_and_other_digest` |
| Never `Approved` for a digest other than `pin.digest` | `pinned_gone_unread_and_other_digest` (both digests stored) |
| Graph review focus 3 (approval bypass) | the "never a mix" and the "other digest" cases |
