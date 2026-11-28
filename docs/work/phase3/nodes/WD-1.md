# WD-1 — Git read of `.zygo/` from the default branch and at a pinned commit

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§6.1 (steps 2-5, results, "Difference from the `.zygo.toml` read", hostile input), R§7.4 (pinned read: steps 3-5 at the saved commit; commit or object gone), R-C4, R-C7, R§17 (git commands left to the builder), R§18 items 1, 2, 3 and 18 (ref cases)
**Files:** `crates/zygo-config/src/repo_def/mod.rs` (new), `crates/zygo-config/src/repo_def/read.rs` (new), `crates/zygo-config/src/lib.rs`, `crates/zygo-config/src/repo_file.rs`, `crates/zygo-config/tests/repo_def_read.rs` (new)
**Consumes:** —
**Produces:** `repo_def::GitRunner` (trait: `fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<std::process::Output, String>`), `repo_def::SystemGit` (the `GitRunner` that runs `repo_file::git_command()`), `repo_def::Files { full_ref: Option<String>, commit: String, workspace: Option<Vec<u8>>, profiles: BTreeMap<String, Vec<u8>>, ignored: Vec<String> }`, `repo_def::Read { Absent { full_ref: Option<String> }, Unread { why: String }, Invalid { full_ref: Option<String>, commit: String, why: String }, Gone { commit: String }, Found { files: Files } }`, `repo_def::read_default(top: &Path, git: &dyn GitRunner) -> Read`, `repo_def::read_at(top: &Path, commit: &str, git: &dyn GitRunner) -> Read`, `repo_def::WORKSPACE_MAX: u64`, `repo_def::PROFILE_MAX: u64`, `repo_def::PROFILES_MAX: usize`
**Acceptance:** `cargo test -p zygo-config --test repo_def_read` → all pass on ubuntu, macOS and Windows. The tests use temp git repositories (a bare origin and a clone) and cover: R§18 item 1 (a working-tree edit, a commit on another branch and an edit in a linked worktree leave the read files equal; a default-branch commit changes them); item 2 (`.zygo` as link, submodule and file; `workspace.toml` as link; `profiles/x.toml` as link and as tree: `Invalid` with the R§6.1 reason, and a recording `GitRunner` shows no content read); item 3 (65537-byte `workspace.toml`, 16385-byte profile, 33 profiles: `Invalid` with path and limit before any content read, recording `GitRunner`); item 18 ref cases (`origin/HEAD` symbolic to `refs/heads/x`: `Unread` with `origin/HEAD points outside refs/remotes/origin/`; no `origin/HEAD`, a branch `main` and a tag `main` with different content: the branch content is read); `read_at` of an unreachable commit after `git reflog expire --expire=now --all` and `git gc --prune=now` gives `Gone`; a failing `GitRunner` gives `Unread`; every call after step 2 names the commit id or a full `refs/…` name (recording `GitRunner`)

## Plan

**Premises checked against the live code (branch `phase3/WD-1` at `4e4e840`, equal to `origin/phase-3`):**

- DR-1 landed (PR #17): it made `crates/zygo-work/src/decisions/record.rs` and `docs/decisions/README.md`. WD-1 uses nothing from it; the edge is the graph's "DDR first" rule (graph Workflows item 3).
- `crates/zygo-config/src/` has no `repo_def` module and no `GitRunner`, `SystemGit`, `Files` or `Read` symbol. `lib.rs:9` is `pub mod repo_file;`.
- `repo_file::git_command()` (`crates/zygo-config/src/repo_file.rs:82-84`) is `pub`: `git` with the `GIT_ENV_CLEARED` variables and every `GIT_CONFIG*` variable removed and `LC_ALL=C`. Reused by `SystemGit`.
- The bounded run `repo_file::git_out` (`repo_file.rs:170-222`) is private, returns the private `GitOut` (exit code, no `ExitStatus`). `GIT_BOUND` (`repo_file.rs:40`, 2 s) is private too. MS-1's `zygo_work::repo::run_whole` cannot be used: `zygo-work` depends on `zygo-config` (`crates/zygo-work/Cargo.toml:9`), so the reverse edge is a cycle. `SystemGit` therefore reuses `git_out`, made `pub(crate)` (Author's calls, Step 3).
- `zygo_core::esc` (`crates/zygo-core/src/text.rs:4`, re-exported at `crates/zygo-core/src/lib.rs:18`) escapes control and bidi characters as `\u{1b}`; `zygo-core` is already a dependency of `zygo-config`. `tempfile` is a dev-dependency.
- The `.zygo.toml` git tests are `#[ignore]` because they run under the 2 s production bound (`crates/zygo-config/tests/config_layers.rs:491-493`). WD-1's tests run every git call through a recording `GitRunner` that hands `SystemGit` a deadline 600 s away, so none is ignored and none depends on wall-clock time.
- A link entry in git without a filesystem link: `crates/zygo-plugins/tests/check.rs:40-49` (`hash-object -w`, then `update-index --add --cacheinfo 120000,<sha>,<rel>`). WD-1 builds every fixture entry this way (blobs from `hash-object -w --stdin`, so no checkout and no line-ending conversion is involved), and a submodule with `--cacheinfo 160000,<commit>,.zygo`.
- git behavior, checked with git 2.50.1: `symbolic-ref --quiet refs/remotes/origin/HEAD` prints the full target (`refs/remotes/origin/main`) with exit 0, and exits 1 with no output both when the ref is missing and when it is a direct ref; `rev-parse --verify --quiet <ref>` then exits 1 with no output for a missing ref and 0 for a direct one; `rev-parse --verify --quiet <ref>^{commit}` exits 1 with no output for a missing ref; `ls-tree -z --long --full-tree <commit> -- .zygo` prints `<mode> <type> <oid> <size padded>\t<path>\0` with size `-` for trees and gitlinks, prints nothing when `.zygo` is absent, matches the path case-sensitively even with `core.ignorecase=true`, and exits 128 for a missing commit; `cat-file -e <oid>` exits 1 with no output for a missing object.
- No helper in `zygo-config` or `zygo-core` checks an object id or parses an `ls-tree --long` listing (`grep -rn "is_oid\|is_hex\|ls-tree" crates/zygo-config/src crates/zygo-core/src`: only `confirm_absent`'s `--name-only` call, `repo_file.rs:232`); both are new private helpers in `read.rs`.
- CI runs `cargo test --workspace --locked --no-fail-fast` on ubuntu, macOS and Windows (`.github/workflows/ci.yml:35,45`).
- Review focus assigned to WD-1 (graph "Review focus"): item 29 (short names, option injection, `origin/HEAD` outside `refs/remotes/origin/`) → Step 1 tests `calls_name_the_commit_or_a_full_ref`, `origin_head_outside_is_unread`, `read_at_refuses_a_non_id`; item 30 (content before size and mode checks; link and submodule modes) → Step 1 tests `tree_entries_are_refused_before_any_content_read`, `sizes_are_refused_before_any_content_read`.

### Interfaces

Consumes: —. (Uses the existing `zygo_config::repo_file::git_command() -> std::process::Command` and `zygo_core::esc(s: &str) -> String`.)

Produces (`crates/zygo-config/src/repo_def/read.rs`, re-exported from `repo_def/mod.rs`):

```rust
pub trait GitRunner {
    /// `git -C <top> <args>`, finished by `deadline`. `Err` is a one-line reason (`timed out`, `tool not found`, `failed`).
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<std::process::Output, String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemGit;                       // impl GitRunner: runs repo_file::git_command()

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    pub full_ref: Option<String>,           // None for a pinned read
    pub commit: String,                     // the read commit, 40 or 64 lower-case hex
    pub workspace: Option<Vec<u8>>,         // .zygo/workspace.toml
    pub profiles: BTreeMap<String, Vec<u8>>, // key: the stem, `rust` for .zygo/profiles/rust.toml
    pub ignored: Vec<String>,               // full paths, e.g. `.zygo/README`, `.zygo/profiles/notes.md`
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    Absent { full_ref: Option<String> },
    Unread { why: String },
    Invalid { full_ref: Option<String>, commit: String, why: String },
    Gone { commit: String },
    Found { files: Files },
}

pub const WORKSPACE_MAX: u64 = 64 * 1024;   // R§6.1 step 4: 64 KiB
pub const PROFILE_MAX: u64 = 16 * 1024;     // R§6.1 step 4: 16 KiB
pub const PROFILES_MAX: usize = 32;         // R§6.1 step 4

pub fn read_default(top: &Path, git: &dyn GitRunner) -> Read;
pub fn read_at(top: &Path, commit: &str, git: &dyn GitRunner) -> Read;
```

Contract change: `Read::Absent { full_ref: String }` became `Read::Absent { full_ref: Option<String> }` (Produces line updated). Reason: R§7.4 makes a pinned read run R§6.1 steps 3-5, and step 4 can end in "absent"; `read_at` has no ref, so it cannot fill a `String` truthfully (`Invalid` and `Files` already carry `Option<String>` for the same reason). `read_default` always gives `Some`. Consumer: WD-4 (`check_default` maps `Absent { full_ref: Some(r) }` to its own `Use::Absent { full_ref: r }`, which stays `String`; `check_pinned` gets `Absent { full_ref: None }`). No other node names `Read::Absent`.

### Author's calls

- `read_default` and `read_at` each set one deadline, `Instant::now() + Duration::from_secs(2)` (a private `const BUDGET: Duration` in `read.rs`, R§6.1 "one 2 s budget"), and pass the same `Instant` to every call of that read.
- `Unread.why` is `git <subcommand>: <reason>`, with `<reason>` the runner's `Err` text, `failed` for an unexpected exit or output, as `repo_file.rs:250,256`. The step 2 refusal is exactly `origin/HEAD points outside refs/remotes/origin/`.
- `refs/remotes/origin/HEAD` that exists as a direct ref (not symbolic) is Unread with the same refusal: its target is an object id, which does not start with `refs/remotes/origin/`.
- A ref that does not resolve (`rev-parse` exit 1, no output: no `refs/heads/main`, or a dangling `origin/HEAD` target) is `Absent { full_ref: Some(<ref>) }`, as the `.zygo.toml` read treats a missing branch (`repo_file.rs:226-242`).
- Any other git failure, a non-repository included, is Unread (the callers pass the repository top).
- Step 3 is up to three listings by pathspec with `--`: `.zygo`, then `.zygo/` if it is a tree, then `.zygo/profiles/` if that is a tree. No `-r`, so deeper trees are never listed. An output entry whose path is not the expected one (`.zygo`; `.zygo/<name>`; `.zygo/profiles/<name>`, no further `/`) is Unread `git ls-tree: failed`.
- Step 4 order (first violation wins): `.zygo` mode; `workspace.toml` mode; `profiles` mode; each `profiles` entry ending in `.toml`, in byte order of name (mode, then name pattern, then `none`); then the sizes in the same file order; then the profile count; then "absent". Modes `100644` and `100755` with type `blob` are regular files; `040000` with type `tree` is a tree.
- Step 4 reason texts (R§17: text not fixed by the spec): `.zygo is not a directory` (spec text); `<path> is not a regular file`; `.zygo/profiles is not a directory`; `<path>: a profile file name must match ^[a-z0-9-]{1,32}\.toml$`; `<path>: the profile name none is reserved`; `<path> is larger than <limit> bytes`; `.zygo/profiles holds more than 32 profile files`. Step 5: `<path>: read <n> bytes, listed <m>`; `<path> is not UTF-8`. Every entry name in a reason passes `zygo_core::esc` (R-C7), after `String::from_utf8_lossy`.
- `Files.ignored` holds the full paths of the ignored entries, lossy UTF-8, not escaped (the printer escapes), in listing order: `.zygo/` entries, then `.zygo/profiles/` entries, each in byte order.
- Step 5 reads each file with `cat-file blob <oid>`, one call per file: it names the listed object id (R§6.1 step 5), never applies a filter or attribute (unlike `git show`), and needs no stdin. Known cost: up to 33 reads plus 4-5 other calls share the 2 s budget; a slow host can make a read with many profiles Unread.
- `read_at` checks `commit` before any git call: 40 or 64 characters of `0-9a-f`, else `Unread { why: format!("pinned commit {} is not an object id", esc(commit)) }`. So no pin text is ever passed as an option or a name.
- Gone: when a step 3-5 call fails, `cat-file -e <commit>`, and then `cat-file -e <oid>` of the tree or blob that call named (if known from a listing), under the same deadline; the first one that exits 1 with empty stdout and stderr gives `Gone { commit }`; otherwise Unread with the first call's reason. `read_default` never returns `Gone`: it turns it into `Unread { why: format!("commit {commit} or one of its objects is missing") }` (a fresh read of a ref whose objects are gone is a broken repository, not a gone pin).
- Lead's call: `SystemGit` reuses `repo_file::git_out` (now `pub(crate)`); no copy.
- `Read::Found` carries the files only; the digest (R§6.5) and the parsed definition are WD-2's `digest` and `parse`, joined by WD-4.
- The workspace id (R§6.1 step 1) is not computed here; WD-4 keys the approval (its `ws_id` parameter).
- Tests: one integration file; every read goes through the test's `Rec` runner (records args and deadline, then calls `SystemGit` with a deadline 600 s away), so no test is `#[ignore]`d and none asserts a time.

### Spec gaps

- The spec sets no limit on the number of entries in `.zygo/` or `.zygo/profiles/` whose names do not end in `.toml`, nor on the size of a listing. The listing stdout and `Files.ignored` grow with the tree, bounded only by the 2 s budget. No cap is added.

### Steps

- [x] **Step 1 — failing tests.** Create `crates/zygo-config/tests/repo_def_read.rs` with the helpers and tests below. Add `pub mod repo_def;` after `pub mod repo_file;` in `crates/zygo-config/src/lib.rs`, and create `crates/zygo-config/src/repo_def/mod.rs` with `mod read;` and `pub use read::{read_at, read_default, Files, GitRunner, Read, SystemGit, PROFILES_MAX, PROFILE_MAX, WORKSPACE_MAX};`, and `crates/zygo-config/src/repo_def/read.rs` with the Interfaces items, each function body `todo!()` and `impl GitRunner for SystemGit` body `todo!()`.

  Helpers:

  ```rust
  //! WD-1: the git read of `.zygo/` (R§6.1 steps 2-5, R§7.4 pinned read; R§18 items 1, 2, 3, 18).
  use std::cell::RefCell;
  use std::collections::BTreeMap;
  use std::io::Write;
  use std::path::{Path, PathBuf};
  use std::process::{Command, Output, Stdio};
  use std::time::{Duration, Instant};
  use zygo_config::repo_def::{self, Files, GitRunner, Read, SystemGit};

  const MAIN: &str = "refs/remotes/origin/main";

  /// `git -C <dir> <args>` with no user or system config, no signing, a fixed identity, `core.autocrlf=false` and
  /// `core.protectNTFS=false` (Git for Windows refuses an index path with a control character otherwise; the
  /// fixtures never write such a path to disk: `w` holds index-only commits and `c` is cloned `--no-checkout`).
  fn git_cmd(dir: &Path, args: &[&str]) -> Command {
      let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
      let mut c = Command::new("git");
      c.env("GIT_CONFIG_GLOBAL", empty).env("GIT_CONFIG_NOSYSTEM", "1")
          .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
                 "-c", "core.autocrlf=false", "-c", "core.protectNTFS=false", "-C"]).arg(dir).args(args);
      c
  }
  /// Runs it, asserts success, returns stdout trimmed.
  fn git(dir: &Path, args: &[&str]) -> String;
  /// `hash-object -w --stdin` of `bytes` in `dir`; returns the blob id.
  fn blob(dir: &Path, bytes: &[u8]) -> String;

  enum E<'a> { File(&'a str, &'a [u8]), Exec(&'a str, &'a [u8]), Link(&'a str, &'a str), Gitlink(&'a str) }

  /// Adds `ents` to the index of `dir` with `update-index --add --cacheinfo <mode>,<oid>,<path>` (100644, 100755,
  /// 120000 with the target as blob, 160000 with the commit `HEAD`), nothing on disk; then `commit -q -m c`.
  fn commit_index(dir: &Path, ents: &[E]);

  struct Fx { _t: tempfile::TempDir, w: PathBuf, c: PathBuf }
  /// `o.git`: `init -q --bare -b main`. `w`: `init -q -b main`, a first commit holding `README` (`seed`, via
  /// `commit_index`), then `commit_index(ents)`, `remote add origin ../o.git`, `push -q origin main`.
  /// `c`: `clone -q --no-checkout o.git c` (so `refs/remotes/origin/HEAD` -> `refs/remotes/origin/main`).
  fn fx(ents: &[E]) -> Fx;

  /// Records each call's args and deadline; fails the calls whose subcommand is `fail` (`"*"`: all) with
  /// `Err("tool not found")`; else runs `SystemGit` with `Instant::now() + Duration::from_secs(600)`; with
  /// `short_blob`, drops the last stdout byte of `cat-file blob`.
  #[derive(Default)]
  struct Rec { calls: RefCell<Vec<(Vec<String>, Instant)>>, fail: Option<&'static str>, short_blob: bool }
  impl GitRunner for Rec { /* as above */ }
  impl Rec {
      fn args(&self) -> Vec<Vec<String>>;
      /// Calls that start with `cat-file blob`.
      fn content_reads(&self) -> usize;
  }
  fn head(c: &Path) -> String { git(c, &["rev-parse", MAIN]) }
  fn found(r: Read) -> Files { match r { Read::Found { files } => files, other => panic!("{other:?}") } }
  ```

  Tests:

  ```rust
  fn base() -> Fx {
      fx(&[E::File(".zygo/workspace.toml", b"version = 1\n"), E::File(".zygo/profiles/rust.toml", b"[overrides]\n"),
           E::Exec(".zygo/profiles/go.toml", b"x"), E::File(".zygo/profiles/notes.md", b"n"), E::File(".zygo/README", b"r")])
  }

  #[test]
  fn found_reads_the_default_branch_files() {
      let f = base();
      let r = repo_def::read_default(&f.c, &Rec::default());
      assert_eq!(r, Read::Found { files: Files {
          full_ref: Some(MAIN.into()), commit: head(&f.c), workspace: Some(b"version = 1\n".to_vec()),
          profiles: BTreeMap::from([("go".into(), b"x".to_vec()), ("rust".into(), b"[overrides]\n".to_vec())]),
          ignored: vec![".zygo/README".into(), ".zygo/profiles/notes.md".into()] } });
  }

  #[test] // review focus 29; R§6.1 step 2 "each later call names that commit id"; one budget
  fn calls_name_the_commit_or_a_full_ref() {
      let f = base();
      let c = head(&f.c);
      let oid = |p: &str| git(&f.c, &["rev-parse", &format!("{c}:{p}")]);
      let rec = Rec::default();
      found(repo_def::read_default(&f.c, &rec));
      let a = rec.args();
      assert_eq!(a.len(), 8);
      assert_eq!(a[0], ["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]);
      assert_eq!(a[1], ["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main^{commit}"]);
      assert_eq!(a[2], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo"]);
      assert_eq!(a[3], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo/"]);
      assert_eq!(a[4], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo/profiles/"]);
      assert_eq!(a[5], ["cat-file", "blob", oid(".zygo/workspace.toml").as_str()]);
      assert_eq!(a[6], ["cat-file", "blob", oid(".zygo/profiles/go.toml").as_str()]);
      assert_eq!(a[7], ["cat-file", "blob", oid(".zygo/profiles/rust.toml").as_str()]);
      for arg in a.iter().flatten() {
          assert!(!["main", "HEAD", "origin/HEAD", "origin/main"].contains(&arg.as_str()), "short name {arg}");
      }
      let d: Vec<Instant> = rec.calls.borrow().iter().map(|x| x.1).collect();
      assert!(d.windows(2).all(|w| w[0] == w[1]), "one deadline per read");
  }

  #[test] // R§18 item 1
  fn default_branch_only() {
      let f = base();
      git(&f.c, &["reset", "-q", "--hard"]);
      let first = found(repo_def::read_default(&f.c, &Rec::default()));
      let wt = f.c.parent().unwrap().join("wt");
      git(&f.c, &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap(), MAIN]);
      std::fs::write(wt.join(".zygo/workspace.toml"), "version = 2\n").unwrap();
      assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "edit in a linked worktree");
      assert_eq!(found(repo_def::read_default(&wt, &Rec::default())), first, "read from the linked worktree");
      git(&wt, &["commit", "-q", "-am", "side"]);
      assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "commit on another branch");
      std::fs::write(f.c.join(".zygo/workspace.toml"), "version = 3\n").unwrap();
      assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "working-tree edit");
      git(&f.c, &["commit", "-q", "-am", "local"]);
      assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "unpushed local commit");
      git(&f.c, &["push", "-q", "origin", "main"]);
      let after = found(repo_def::read_default(&f.c, &Rec::default()));
      assert_eq!(after.workspace.as_deref(), Some(&b"version = 3\n"[..]));
      assert_ne!(after.commit, first.commit);
  }

  fn invalid(f: &Fx, why: &str) -> Read { Read::Invalid { full_ref: Some(MAIN.into()), commit: head(&f.c), why: why.into() } }

  #[test] // R§18 item 2; review focus 30
  fn tree_entries_are_refused_before_any_content_read() {
      let pattern = "a profile file name must match ^[a-z0-9-]{1,32}\\.toml$";
      let long = format!(".zygo/profiles/{}.toml", "a".repeat(33));
      let cases: Vec<(Vec<E>, String)> = vec![
          (vec![E::Link(".zygo", "elsewhere")], ".zygo is not a directory".into()),
          (vec![E::Gitlink(".zygo")], ".zygo is not a directory".into()),
          (vec![E::File(".zygo", b"x")], ".zygo is not a directory".into()),
          (vec![E::Link(".zygo/workspace.toml", "../w.toml")], ".zygo/workspace.toml is not a regular file".into()),
          (vec![E::File(".zygo/workspace.toml/a", b"x")], ".zygo/workspace.toml is not a regular file".into()),
          (vec![E::File(".zygo/profiles", b"x")], ".zygo/profiles is not a directory".into()),
          (vec![E::Link(".zygo/profiles/x.toml", "../x")], ".zygo/profiles/x.toml is not a regular file".into()),
          (vec![E::File(".zygo/profiles/x.toml/a", b"x")], ".zygo/profiles/x.toml is not a regular file".into()),
          (vec![E::File(".zygo/profiles/Rust.toml", b"x")], format!(".zygo/profiles/Rust.toml: {pattern}")),
          (vec![E::File(".zygo/profiles/None.toml", b"x")], format!(".zygo/profiles/None.toml: {pattern}")),
          (vec![E::File(".zygo/profiles/.toml", b"x")], format!(".zygo/profiles/.toml: {pattern}")),
          (vec![E::File(&long, b"x")], format!("{long}: {pattern}")),
          (vec![E::File(".zygo/profiles/a\u{1b}b.toml", b"x")], format!(".zygo/profiles/a\\u{{1b}}b.toml: {pattern}")),
          (vec![E::File(".zygo/profiles/none.toml", b"x")], ".zygo/profiles/none.toml: the profile name none is reserved".into()),
      ];
      for (ents, why) in cases {
          let f = fx(&ents);
          let rec = Rec::default();
          assert_eq!(repo_def::read_default(&f.c, &rec), invalid(&f, &why), "{why}");
          assert_eq!(rec.content_reads(), 0, "content read before the check: {why}");
      }
  }
  ```

  ```rust
  #[test] // R§18 item 3; review focus 30
  fn sizes_are_refused_before_any_content_read() {
      let big_ws = vec![b'a'; 65537];
      let big_profile = vec![b'a'; 16385];
      let names: Vec<String> = (0..33).map(|i| format!(".zygo/profiles/p{i:02}.toml")).collect();
      let many: Vec<E> = names.iter().map(|n| E::File(n, b"x")).collect();
      let cases: Vec<(Vec<E>, &str)> = vec![
          (vec![E::File(".zygo/workspace.toml", &big_ws)], ".zygo/workspace.toml is larger than 65536 bytes"),
          (vec![E::File(".zygo/profiles/big.toml", &big_profile)], ".zygo/profiles/big.toml is larger than 16384 bytes"),
          (many, ".zygo/profiles holds more than 32 profile files"),
      ];
      for (ents, why) in cases {
          let f = fx(&ents);
          let rec = Rec::default();
          assert_eq!(repo_def::read_default(&f.c, &rec), invalid(&f, why), "{why}");
          assert_eq!(rec.content_reads(), 0, "content read before the size check: {why}");
      }
  }

  #[test] // the limits themselves are allowed
  fn limits_are_the_spec_values() {
      assert_eq!((repo_def::WORKSPACE_MAX, repo_def::PROFILE_MAX, repo_def::PROFILES_MAX), (65536, 16384, 32));
      let ws = vec![b'a'; 65536];
      let p = vec![b'b'; 16384];
      let names: Vec<String> = (0..32).map(|i| format!(".zygo/profiles/p{i:02}.toml")).collect();
      let mut ents: Vec<E> = names.iter().map(|n| E::File(n, &p)).collect();
      ents.push(E::File(".zygo/workspace.toml", &ws));
      let files = found(repo_def::read_default(&fx(&ents).c, &Rec::default()));
      assert_eq!(files.workspace.map(|w| w.len()), Some(65536));
      assert_eq!(files.profiles.len(), 32);
      assert!(files.profiles.values().all(|v| v.len() == 16384));
  }

  #[test] // R§6.1 step 4: nothing to read
  fn absent_definitions() {
      let none = fx(&[E::File("src/lib.rs", b"")]);
      assert_eq!(repo_def::read_default(&none.c, &Rec::default()), Read::Absent { full_ref: Some(MAIN.into()) });
      let empty = fx(&[E::File(".zygo/README", b"r"), E::File(".zygo/profiles/notes.md", b"n")]);
      assert_eq!(repo_def::read_default(&empty.c, &Rec::default()), Read::Absent { full_ref: Some(MAIN.into()) });
  }

  #[test] // R§18 item 18; review focus 29
  fn origin_head_outside_is_unread() {
      let f = base();
      let unread = Read::Unread { why: "origin/HEAD points outside refs/remotes/origin/".into() };
      git(&f.c, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/heads/x"]);
      assert_eq!(repo_def::read_default(&f.c, &Rec::default()), unread);
      git(&f.c, &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
      git(&f.c, &["update-ref", "--no-deref", "refs/remotes/origin/HEAD", &head(&f.c)]);
      assert_eq!(repo_def::read_default(&f.c, &Rec::default()), unread, "a direct origin/HEAD");
  }

  #[test] // R§18 item 18: no origin/HEAD; a tag `main` never wins over the branch
  fn no_origin_head_reads_the_branch_not_the_tag() {
      let f = base();                                    // clone `c` has local branch main = origin/main
      git(&f.w, &["checkout", "-q", "-b", "other"]);
      commit_index(&f.w, &[E::File(".zygo/workspace.toml", b"version = 9\n")]);
      git(&f.w, &["push", "-q", "origin", "other"]);
      git(&f.c, &["fetch", "-q", "origin"]);
      git(&f.c, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/other"]);
      let moved = found(repo_def::read_default(&f.c, &Rec::default()));   // origin/HEAD moved inside refs/remotes/origin/
      assert_eq!(moved.full_ref.as_deref(), Some("refs/remotes/origin/other"));
      assert_eq!(moved.workspace.as_deref(), Some(&b"version = 9\n"[..]));
      git(&f.c, &["tag", "main", "refs/remotes/origin/other"]);
      git(&f.c, &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
      let files = found(repo_def::read_default(&f.c, &Rec::default()));
      assert_eq!(files.full_ref.as_deref(), Some("refs/heads/main"));
      assert_eq!(files.commit, git(&f.c, &["rev-parse", "refs/heads/main"]));
      assert_eq!(files.workspace.as_deref(), Some(&b"version = 1\n"[..]));
      git(&f.c, &["update-ref", "-d", "refs/heads/main"]);
      assert_eq!(repo_def::read_default(&f.c, &Rec::default()), Read::Absent { full_ref: Some("refs/heads/main".into()) });
  }

  #[test] // R§7.4: steps 3-5 at the saved commit, no default-branch lookup
  fn read_at_reads_the_pinned_commit() {
      let f = base();
      let pinned = found(repo_def::read_default(&f.c, &Rec::default()));
      commit_index(&f.w, &[E::File(".zygo/workspace.toml", b"version = 5\n")]);
      git(&f.w, &["push", "-q", "origin", "main"]);
      git(&f.c, &["fetch", "-q", "origin"]);
      let rec = Rec::default();
      let at = found(repo_def::read_at(&f.c, &pinned.commit, &rec));
      assert_eq!(at, Files { full_ref: None, ..pinned.clone() });
      for call in rec.args() {
          assert!(call[0] == "ls-tree" || call[0] == "cat-file", "{call:?}");
          assert!(call.iter().all(|a| !a.starts_with("refs/")), "{call:?}");
      }
  }

  #[test] // R§7.4: commit gone after a force-push and gc
  fn read_at_of_a_pruned_commit_is_gone() {
      let f = base();
      git(&f.c, &["reset", "-q", "--hard"]);
      git(&f.c, &["checkout", "-q", "-b", "side"]);
      std::fs::write(f.c.join(".zygo/workspace.toml"), "version = 7\n").unwrap();
      git(&f.c, &["commit", "-q", "-am", "side"]);
      let gone = git(&f.c, &["rev-parse", "HEAD"]);
      git(&f.c, &["checkout", "-q", "main"]);
      git(&f.c, &["branch", "-q", "-D", "side"]);
      git(&f.c, &["reflog", "expire", "--expire=now", "--all"]);
      git(&f.c, &["gc", "-q", "--prune=now"]);
      assert!(!git_cmd(&f.c, &["cat-file", "-e", &gone]).status().unwrap().success(), "fixture: commit still there");
      assert_eq!(repo_def::read_at(&f.c, &gone, &Rec::default()), Read::Gone { commit: gone.clone() });
  }

  #[test] // review focus 29: a pin is never passed to git unless it is an object id
  fn read_at_refuses_a_non_id() {
      let f = base();
      let (upper, short) = ("A".repeat(40), "a".repeat(39));
      for bad in ["main", "-x", "", "refs/heads/main", upper.as_str(), short.as_str()] {
          let rec = Rec::default();
          assert_eq!(repo_def::read_at(&f.c, bad, &rec),
                     Read::Unread { why: format!("pinned commit {bad} is not an object id") });
          assert!(rec.args().is_empty(), "{bad}: git called");
      }
  }

  #[test] // R§6.1 results: a failed git call is unread, naming the call
  fn failing_runner_is_unread() {
      let f = base();
      let all = Rec { fail: Some("*"), ..Rec::default() };
      assert_eq!(repo_def::read_default(&f.c, &all), Read::Unread { why: "git symbolic-ref: tool not found".into() });
      let c = head(&f.c);
      let lst = Rec { fail: Some("ls-tree"), ..Rec::default() };
      assert_eq!(repo_def::read_at(&f.c, &c, &lst), Read::Unread { why: "git ls-tree: tool not found".into() });
      assert!(lst.args().contains(&vec!["cat-file".to_string(), "-e".into(), c.clone()]), "Gone confirmed first");
  }

  #[test] // R§6.1 step 5
  fn content_must_match_size_and_be_utf8() {
      let f = fx(&[E::File(".zygo/workspace.toml", &[0xff, 0xfe])]);
      assert_eq!(repo_def::read_default(&f.c, &Rec::default()), invalid(&f, ".zygo/workspace.toml is not UTF-8"));
      let f = fx(&[E::File(".zygo/workspace.toml", b"version = 1\n")]);
      let short = Rec { short_blob: true, ..Rec::default() };
      assert_eq!(repo_def::read_default(&f.c, &short), invalid(&f, ".zygo/workspace.toml: read 11 bytes, listed 12"));
  }
  ```

  Check: the file compiles (`cargo test -p zygo-config --test repo_def_read --no-run` exits 0).

- [x] **Step 2 — see them fail.** Run `cargo test -p zygo-config --test repo_def_read`. Pass: every test fails at a `todo!()` (and none fails on a fixture `git` assertion).

- [x] **Step 3 — `SystemGit`.** In `crates/zygo-config/src/repo_file.rs`: make `fn git_out` (line 170) `pub(crate)`; make `struct GitOut` (line 150) and its fields `pub(crate)`; widen `GitOut` in place by replacing `code: Option<i32>` with `status: std::process::ExitStatus` (line 221 stores `status`; the uses at lines 160, 229 and 232 read `o.status.code()`). An exit code alone cannot become a portable `ExitStatus` (and a signal death, code `None`, cannot be rebuilt), so the field is widened, not converted. In `read.rs`, `impl GitRunner for SystemGit`: `crate::repo_file::git_out(top, args, deadline).map(|o| Output { status: o.status, stdout: o.out, stderr: o.err }).map_err(str::to_owned)`. The `GitRunner` signature is unchanged. No timing test (no wall-clock tests in CI); every other test runs through it.

- [x] **Step 4 — `read_default` and `read_at`.** In `read.rs`, with private helpers. Algorithm (R§6.1, R§17):

  ```text
  read_default(top, git):
    d = now + BUDGET
    o = git symbolic-ref --quiet refs/remotes/origin/HEAD
      exit 0  -> t = stdout without one trailing "\n" (not UTF-8: Unread "git symbolic-ref: failed")
                 t starts with "refs/remotes/origin/" ? full_ref = t : Unread(REFUSAL)
      exit 1, stdout and stderr empty ->
           o2 = git rev-parse --verify --quiet refs/remotes/origin/HEAD
             exit 1, empty -> full_ref = "refs/heads/main"
             exit 0        -> Unread(REFUSAL)            # a direct ref
             else          -> Unread "git rev-parse: failed"
      else -> Unread "git symbolic-ref: <reason>"
    o = git rev-parse --verify --quiet <full_ref>^{commit}
      exit 0 and stdout trimmed is an object id -> commit
      exit 1, empty -> Absent { Some(full_ref) }
      else -> Unread "git rev-parse: <reason>"
    steps_3_to_5(Some(full_ref), commit, d), with Gone -> Unread "commit <commit> or one of its objects is missing"

  read_at(top, commit, git):
    commit not an object id (40 or 64 of 0-9a-f) -> Unread "pinned commit <esc(commit)> is not an object id"
    steps_3_to_5(None, commit, now + BUDGET)

  steps_3_to_5(full_ref, commit, d):
    L1 = ls-tree -z --long --full-tree <commit> -- .zygo         ; no entry -> Absent
         entry not (040000 tree) -> Invalid ".zygo is not a directory"
    L2 = ls-tree ... -- .zygo/        ; workspace.toml, profiles; others -> ignored
    workspace present and not regular -> Invalid; profiles present and not a tree -> Invalid
    L3 = ls-tree ... -- .zygo/profiles/   (only when profiles is a tree); non-.toml -> ignored
    step 4 checks in the Author's-calls order; then "no workspace and no profile" -> Absent
    for each file (workspace, then profiles by name): cat-file blob <oid>;
         len != listed size -> Invalid "<path>: read <n> bytes, listed <m>"; not UTF-8 -> Invalid
    Found
    any failed call (Err, non-zero exit, unparsable listing) -> gone check (Author's calls) -> Gone or Unread
  ```

  Entry parse of a listing: split stdout at `\0` (drop the empty tail); each record splits at its first `\t` into meta and path; meta is four ASCII fields `mode type oid size`, size `-` or a `u64`; oid an object id. Any other shape: Unread `git ls-tree: failed`.

- [x] **Step 5 — see them pass.** Run `cargo test -p zygo-config --test repo_def_read`. Pass: 14 tests, 0 failed, 0 ignored. Then `cargo test -p zygo-config`: all pass; `cargo build -p zygo-config --tests` prints no warning from `repo_def/` or `tests/repo_def_read.rs`.

- [x] **Step 6 — commit.** `git add crates/zygo-config/src/lib.rs crates/zygo-config/src/repo_file.rs crates/zygo-config/src/repo_def crates/zygo-config/tests/repo_def_read.rs`, then commit with subject `WD-1: git read of .zygo/ from the default branch and at a pinned commit` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:WD-1`, each on its own line. Check: `git show --stat HEAD` lists exactly those five files and the message carries no Co-Authored-By or "Generated with" line.

### Review round 1

- **F1** — a full ref is resolved by exact name only: `show-ref --verify --quiet <ref>` (quiet exit 1: missing → `Absent`, or `refs/heads/main` for `origin/HEAD`), then `show-ref --verify --hash <ref>`, then `rev-parse --verify --quiet <oid>^{commit}`; `rev-parse` never gets a ref name, so `refs/tags/<ref>` is never read. Tests: `no_origin_head_reads_the_branch_not_the_tag` (F8: now adds the tag `refs/tags/refs/heads/main`), `dangling_origin_head_is_absent_not_the_tag`, `origin_head_tag_changes_nothing`, `calls_name_the_commit_or_a_full_ref` (strengthened: a call with a `refs/` argument is `symbolic-ref` or `show-ref --verify`; `rev-parse` gets only `<oid>^{commit}`).
- **F2** — `SystemGit::command()` sets `GIT_NO_LAZY_FETCH=1` and `GIT_TERMINAL_PROMPT=0` (as DR-11). Test: `partial_clone_is_read_without_a_fetch` (a `--filter=blob:none --no-local` clone; the blob is still not here after the read), `system_git_command_environment`.
- **F4** — `GIT_NO_REPLACE_OBJECTS=1` on every `SystemGit` call. Test: `replace_refs_are_ignored`.
- **F5** — `SystemGit::command()` removes `GIT_ICASE_PATHSPECS`, `GIT_GLOB_PATHSPECS`, `GIT_NOGLOB_PATHSPECS` and `GIT_LITERAL_PATHSPECS` (not `repo_file`'s list). Test: `system_git_command_environment` (the built command's environment; the test process environment is not changed).
- **F6** — the gone check also reads the root tree id from `cat-file commit <commit>` and runs `cat-file -e` on it (`<commit>^{tree}` itself fails when the tree is missing). Test: `read_at_with_the_root_tree_gone_is_gone`.
- **F7** — tests: `read_at_with_a_subtree_or_blob_gone_is_gone`, `read_default_maps_gone_to_unread`, `exit_1_with_stderr_is_not_gone`, `odd_ls_tree_output_is_unread`, `long_content_and_short_profile_are_invalid`, `dangling_origin_head_is_absent_not_the_tag`, `non_zero_exit_is_unread`, `calls_name_the_commit_or_a_full_ref`. Found by them: `ls-tree --long` lists a blob it does not have with the size `BAD` and exits 0, so a missing blob failed at the listing with the tree as the named object and gave `Unread`; `list` now names that blob for the gone check.
- **F8** — fixed in `no_origin_head_reads_the_branch_not_the_tag` (see F1).
- **F11** — `Files.ignored` is sorted in byte order before it is returned. Test: `ignored_is_in_byte_order`.
- **F3** — not fixed: the 2 s budget on Windows is not measured.
- **F9** — not fixed: refuted; on Windows a kill cannot be told from an exit.
- **F10** — not fixed: an output cap is a new rule for the operator.

Author's calls (round 1):

- A blob that a partial clone does not have (lazy fetch off) is `Gone` for `read_at` (so `Unread` "commit … or one of its objects is missing" for `read_default`). R§7.4: "the pinned commit or one of its objects is gone"; the repository does not have the object, and the read cannot tell a promised blob from a pruned one without a promisor query.
- `Files.ignored` is in byte order of the full path (it was `.zygo/` entries, then `.zygo/profiles/` entries).
- A ref that exists but does not peel to a commit (`rev-parse <oid>^{commit}` quiet exit 1) stays `Absent`, as before.
- `SystemGit::command()` is public so that the test can check the environment of the built command.
