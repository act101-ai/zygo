# WD-9 — Pool directory listing, assignment and shared lines

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§9.5 ("Directories", "Assignment", "A pool that does not work", "Shared directories"; `~` expanded as `--config-dir` does, `crates/zygo/src/cmd/claude.rs:66-90`), R-C6, R§18 item 13 (listing and assignment; the mode-`000` case is `cfg(unix)` by R§18 item 13)
**Files:** `crates/zygo-workspace/src/pool.rs` (new), `crates/zygo-workspace/src/lib.rs`, `crates/zygo-workspace/tests/pool.rs` (new)
**Consumes:** `WorkspaceConfig.pool` (WD-5)
**Produces:** `pool::home() -> Option<PathBuf>`, `pool::expand(pool: &str, home: Option<&Path>) -> Result<PathBuf, String>`, `pool::list(dir: &Path) -> Result<Vec<PathBuf>, String>` (entries whose name does not start with `.` and that are directories, links followed, in byte order of name; the `Err` is the `<reason>` of `zygo: pool <dir>: <reason>; nothing started`), `pool::order(names: &[Option<String>]) -> Vec<usize>` (pane indices: named panes in byte order of name, then unnamed panes in pane order), `pool::assign(names: &[Option<String>], k: usize) -> Vec<usize>` (directory index per pane: the i-th pane of `order` gets `i mod k`), `pool::shared_lines(dir_of: &[(String, PathBuf)]) -> Vec<String>` (`pool: <dir> is shared by <name>, <name>`)
**Acceptance:** `cargo test -p zygo-workspace --test pool` → all pass on ubuntu, macOS and Windows. The tests cover: directories `a` and `b`, a file `c` and a directory `.x` list as `[a, b]`; the default-set names with `[a, b]` give `orchestrator → a`, `worker-1 → b`, `worker-2 → a`, `worker-3 → b`, `worker-4 → a`; a missing path, a file and a directory with no subdirectory each give `Err`; a relative path after expansion gives `Err`; `shared_lines` gives one line for `a` and one for `b`; `#[cfg(unix)]`: a pool directory that holds mode-`000` files lists the same and opens no file in it

## Plan

### Interfaces

Consumes (live): `WorkspaceConfig.pool: Option<String>` (`crates/zygo-config/src/schema.rs:729-731`, WD-5; `None` by default). WD-9 does not read it; WD-12, WD-15 and WD-17 pass its text to `expand`.

Produces (exact, in `crates/zygo-workspace/src/pool.rs`):
- `pub fn home() -> Option<PathBuf>` (fix round 1): the home directory of the `~` rule, moved from `claude.rs`: `HOME` (`USERPROFILE` on Windows) when absolute, else `std::env::home_dir`. `pub fn home_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf>` is the same rule over a variable reader; `zygo claude` uses it because it reads the variables from the `Env` it builds (its tests set `HOME` there only). Callers of `expand` pass `home().as_deref()`.
- `pub fn tilde(p: &str, home: Option<&Path>) -> Option<Result<PathBuf, String>>`: the one `~` rule, shared with `zygo claude --config-dir`. `None` when `p` is neither `~` nor starts with `~/`; `Some(Ok(home))` for `~` and `~/` (as `claude.rs:68-73`, so no trailing separator); `Some(Ok(home.join(rest)))` for `~/<rest>`; `Some(Err("no home directory"))` when `home` is `None`.
- `pub fn expand(pool: &str, home: Option<&Path>) -> Result<PathBuf, String>`: `tilde`, else `PathBuf::from(pool)`; `Err("must be absolute after ~ expansion")` (the text of `check.rs:171`) when the result is not absolute.
- `pub fn list(dir: &Path) -> Result<Vec<PathBuf>, String>`: `dir.join(name)` for each entry whose name does not start with `.` and that is a directory (links followed), in byte order of name. `Err` reasons: `not a directory` (missing path or a file), `not readable (<io::ErrorKind>)` (`read_dir` or an entry fails; the kind only, no path), `no subdirectory`.
- `pub fn order(names: &[Option<String>]) -> Vec<usize>`: pane indices; named panes in byte order of name (stable sort), then unnamed panes in pane order.
- `pub fn assign(names: &[Option<String>], k: usize) -> Vec<usize>`: `out[order[i]] = i % k`. `k >= 1`: `list` never returns an empty `Ok`.
- `pub fn shared_lines(dir_of: &[(String, PathBuf)]) -> Vec<String>`: one line per directory that two or more members share, `pool: <dir> is shared by <name>, <name>[, <name>…]`. Lines are in the order of each directory's first appearance in `dir_of`, names in `dir_of` order. `<dir>` (`to_string_lossy`) and each `<name>` pass `zygo_core::esc` (R-C7: member names are repository text).

Decisions (spec text decides): "links followed" is `Path::is_dir` (a stat, not an open: R-C6), so a dangling link is skipped. "Byte order of name" is `OsStr::as_encoded_bytes` order (the UTF-8 bytes for UTF-8 names on every OS). The reason texts are free by R§17 ("the text of lines this spec does not fix").

Shared `~` rule (outside the Files list, with reason): the rule lives inside the private `set_config_dir` of `crates/zygo/src/cmd/claude.rs:66-90`. `zygo` depends on `zygo-workspace`, not the reverse, so the shared function goes in `pool.rs` (`tilde`), and step 6 changes `claude.rs:67-78` to call it. Behaviour does not change. `zygo-config`'s `absolute_after_tilde` (`check.rs:244-246`) is a text-only check in a lower crate, which cannot call `zygo-workspace`; it stays.

### Steps

- [x] 1. Create `crates/zygo-workspace/tests/pool.rs`:
  ```rust
  use std::path::{Path, PathBuf};

  use zygo_workspace::pool::*;

  const DEFAULT_SET: [&str; 5] = ["orchestrator", "worker-1", "worker-2", "worker-3", "worker-4"];

  fn names(n: &[&str]) -> Vec<Option<String>> { n.iter().map(|s| Some(s.to_string())).collect() }

  /// A pool with directories `a` and `b`, a file `c` and a directory `.x`.
  fn pool_ab() -> tempfile::TempDir {
      let d = tempfile::tempdir().unwrap();
      for s in ["a", "b", ".x"] { std::fs::create_dir(d.path().join(s)).unwrap(); }
      std::fs::write(d.path().join("c"), "").unwrap();
      d
  }

  #[test]
  fn list_keeps_visible_directories_in_name_order() {
      let d = pool_ab();
      assert_eq!(list(d.path()).unwrap(), vec![d.path().join("a"), d.path().join("b")]);
  }

  #[test]
  fn list_refuses_a_missing_path_a_file_and_no_subdirectory() {
      let d = tempfile::tempdir().unwrap();
      assert_eq!(list(&d.path().join("missing")), Err("not a directory".into()));
      std::fs::write(d.path().join("f"), "").unwrap();
      assert_eq!(list(&d.path().join("f")), Err("not a directory".into()));
      std::fs::create_dir(d.path().join(".hidden")).unwrap();
      assert_eq!(list(d.path()), Err("no subdirectory".into())); // holds only a file and a dot directory
  }

  #[test]
  fn expand_follows_the_config_dir_tilde_rule() {
      let h = tempfile::tempdir().unwrap();
      assert_eq!(expand("~", Some(h.path())), Ok(h.path().to_path_buf()));
      assert_eq!(expand("~/", Some(h.path())), Ok(h.path().to_path_buf()));
      assert_eq!(expand("~/.claude-interns", Some(h.path())), Ok(h.path().join(".claude-interns")));
      let abs = h.path().to_string_lossy().into_owned();
      assert_eq!(expand(&abs, None), Ok(PathBuf::from(&abs)));
      assert_eq!(expand("~/x", None), Err("no home directory".into()));
      assert_eq!(tilde("rel", Some(h.path())), None);
  }

  #[test]
  fn expand_refuses_a_relative_path() {
      let h = tempfile::tempdir().unwrap();
      assert_eq!(expand("pool", Some(h.path())), Err("must be absolute after ~ expansion".into()));
      assert_eq!(expand("~x/pool", Some(h.path())), Err("must be absolute after ~ expansion".into()));
  }

  #[test]
  fn default_set_on_two_directories_alternates() {
      let d = pool_ab();
      let dirs = list(d.path()).unwrap();
      let got: Vec<(&str, &Path)> =
          DEFAULT_SET.iter().zip(assign(&names(&DEFAULT_SET), dirs.len())).map(|(n, i)| (*n, dirs[i].as_path())).collect();
      let (a, b) = (d.path().join("a"), d.path().join("b"));
      assert_eq!(got, vec![("orchestrator", &*a), ("worker-1", &*b), ("worker-2", &*a), ("worker-3", &*b), ("worker-4", &*a)]);
  }

  #[test]
  fn order_puts_named_panes_by_name_then_unnamed_in_pane_order() {
      let n = vec![Some("b".into()), None, Some("a".into()), None];
      assert_eq!(order(&n), vec![2, 0, 1, 3]);
      assert_eq!(assign(&n, 2), vec![1, 0, 0, 1]);
  }

  #[test]
  fn shared_lines_name_each_shared_directory_once() {
      let (a, b) = (PathBuf::from("/p/a"), PathBuf::from("/p/b"));
      let dir_of: Vec<(String, PathBuf)> =
          [(0, &a), (1, &b), (2, &a), (3, &b), (4, &a)].iter().map(|(i, p)| (DEFAULT_SET[*i].to_string(), (*p).clone())).collect();
      assert_eq!(
          shared_lines(&dir_of),
          vec![
              format!("pool: {} is shared by orchestrator, worker-2, worker-4", a.display()),
              format!("pool: {} is shared by worker-1, worker-3", b.display()),
          ]
      );
      assert_eq!(shared_lines(&dir_of[..2]), Vec::<String>::new()); // k = n: nothing shared
  }

  /// R§18 item 13: unix only; Windows has no portable way to make a file unreadable to its owner.
  #[cfg(unix)]
  #[test]
  fn mode_000_files_change_nothing() {
      use std::os::unix::fs::PermissionsExt;
      let d = pool_ab();
      for f in [d.path().join("a/.credentials.json"), d.path().join("a/settings.json"), d.path().join("z")] {
          std::fs::write(&f, "{}").unwrap();
          std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
      }
      assert_eq!(list(d.path()).unwrap(), vec![d.path().join("a"), d.path().join("b")]);
  }
  ```
  The mode-`000` test holds no directory of mode `000`, so the temp directory removes itself (an unlink needs write access to the parent, not to the file). As root, a mode-`000` file stays readable, and the test still checks that the listing is the same.
- [x] 2. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --locked --test pool` → compile error: `zygo_workspace::pool` is not found.
- [x] 3. Add `pub mod pool;` to `crates/zygo-workspace/src/lib.rs` between `pub mod mux;` and `pub mod repair;`, and add "pool" to the crate doc line. Create `crates/zygo-workspace/src/pool.rs` with the six functions of "Interfaces" (module doc: R§9.5, R-C6; "names and types only, no file opened"). Bodies follow the signatures. `list`: `if !dir.is_dir()` → `not a directory`; `read_dir` with `map_err(|e| format!("not readable ({:?})", e.kind()))`, and the same for each entry; keep each entry whose `file_name().as_encoded_bytes()` does not start with `b'.'` and whose `path().is_dir()`; `sort_by` the encoded bytes of the names; empty → `no subdirectory`. `shared_lines`: group by directory in order of first appearance, emit groups of length >= 2.
- [x] 4. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --locked --test pool` → 8 pass on unix (7 on Windows).
- [x] 5. Commit `crates/zygo-workspace/src/pool.rs`, `crates/zygo-workspace/src/lib.rs`, `crates/zygo-workspace/tests/pool.rs`: `WD-9: pool listing, assignment and shared lines`.
- [x] 6. In `crates/zygo/src/cmd/claude.rs` `set_config_dir`, replace the `rest`/`dir` match (`:68-78`) with:
  ```rust
  let home = env.get(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|h| h.is_absolute());
  let dir = match zygo_workspace::pool::tilde(d, home.or_else(std::env::home_dir).as_deref()) {
      Some(Ok(h)) => h,
      Some(Err(_)) => return Err(format!("zygo claude: --config-dir {d}: no home directory")),
      None => cwd.join(d),
  };
  ```
  Keep the doc comment and the lines after it as they are.
- [x] 7. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked config_dir` → the four existing tests pass (`config_dir_sets_env`, `config_dir_expands_tilde`, `config_dir_bare_tilde_is_home`, `config_dir_relative_is_absolute_against_cwd`). Commit `crates/zygo/src/cmd/claude.rs`: `WD-9: --config-dir uses the shared ~ rule`.
- [x] 8. Verification: `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --locked --test pool` → all pass.

## Fix round 1

Review findings, each with a test that failed first (except 5, which no test on a unix host can see):

1. `~//x` resolved to `/x`: `tilde` now strips every leading separator of `rest` (`/`, and `\` on Windows); a `rest` that still has a root or a prefix (`~/C:\x`) gives `must be absolute after ~ expansion`. `--config-dir` shares the fix and now prints the `tilde` reason. Tests: `tilde_strips_every_leading_separator`, `tilde_refuses_a_rest_with_a_prefix_or_root` (`cfg(windows)`).
2. The home lookup moved from `claude.rs` into `pool::home` / `pool::home_in`; `claude.rs` calls `home_in` over its `Env`. `expand`'s doc says callers pass `home().as_deref()`. Test: `home_uses_an_absolute_variable_else_the_os_home`. Deviation: a bare `home()` reads the process environment, which would break `config_dir_expands_tilde` (it sets `HOME` only in the injected `Env`), so `home_in` exists beside it.
3. A failed stat now reports its kind: the root uses `fs::metadata` (`NotFound` or not a directory → `not a directory`, any other error → `not readable (<kind>)`). A directory entry is confirmed with `DirEntry::metadata` (a stat on unix, which fails when the root cannot be searched; the find data on Windows, no handle). Tests (`cfg(unix)`): `a_root_without_search_permission_is_not_readable`, `a_pool_under_a_mode_000_parent_is_not_readable`.
4. `shared_lines` groups by `fs::canonicalize(dir)` (the path as given when it fails) and prints the first path as given. Test (`cfg(unix)`): `two_links_to_one_directory_are_shared`.
5. `list` reads `DirEntry::file_type` first: a file is skipped without a stat, only a symlink is followed with `fs::metadata`. A dangling link or a loop (`NotFound`, `ELOOP`, Windows error 1921) is skipped; any other error on a link is `not readable (<kind>)`.
6. Added tests: `links_to_directories_are_followed_and_broken_links_skipped` (`cfg(unix)`), `list_is_in_byte_order_of_name` (`10`, `9`, `Cap`, `a`, `é`), `shared_lines_escape_the_directory_and_the_names` (fails when either `esc` call is removed; checked by mutation), `tilde("~", None)` in `tilde_strips_every_leading_separator`.

Check: `env -u ZYGO_SESSION_DIR cargo test -p zygo-workspace --locked --test pool` → 16 pass on macOS. The `cfg(windows)` code was not compiled here (no Windows target installed).
