# MS-2 — zygo-mail-sync crate: hardened git calls and test seams

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S-C4, S-C9, S§8.7 (last bullet); S§17 rows "The mail module" and `crates/zygo-plugins/src/sync.rs:32-58`; S§18 (seams; where the builder and runner live)
**Files:** `Cargo.toml`, `Cargo.lock`, `crates/zygo-mail-sync/Cargo.toml`, `crates/zygo-mail-sync/src/lib.rs`, `crates/zygo-mail-sync/src/git.rs`, `crates/zygo-mail-sync/src/env.rs`, `crates/zygo-mail-sync/tests/git_builder.rs`, and the empty module files `crates/zygo-mail-sync/src/address.rs`, `crates/zygo-mail-sync/src/state.rs`, `crates/zygo-mail-sync/src/registry.rs`, `crates/zygo-mail-sync/src/secret.rs`, `crates/zygo-mail-sync/src/public.rs`, `crates/zygo-mail-sync/src/outbox.rs`, `crates/zygo-mail-sync/src/branch.rs`, `crates/zygo-mail-sync/src/identity.rs`, `crates/zygo-mail-sync/src/import.rs`, `crates/zygo-mail-sync/src/sync.rs`, `crates/zygo-mail-sync/src/prune.rs`, `crates/zygo-mail-sync/src/ops.rs`, `crates/zygo-mail-sync/src/spawn.rs`
**Consumes:** `run_whole` (MS-1), `run_stream` (MS-1), `Whole` (MS-1), `Flow` (MS-1), `Streamed` (MS-1), `RunError` (MS-1); the P3-1 mail store module zygo_workspace::mail, as a Cargo dependency on zygo-workspace: `Message` (ML-2), `mail_dir` (ML-2)
**Produces:** `zygo_mail_sync::git::sync_git(cwd: &Path) -> std::process::Command`, `zygo_mail_sync::git::run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<Whole, RunError>`, `zygo_mail_sync::git::stream(cwd: &Path, args: &[&str], deadline: Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>`, `zygo_mail_sync::git::has_text(hay: &[u8], needles: &[&str]) -> bool`, `zygo_mail_sync::env::Clock` (trait: `fn now(&self) -> SystemTime`), `zygo_mail_sync::env::SystemClock`, `zygo_mail_sync::env::Verdict` (enum: `Public`, `Private`, `Local`, `Unknown(String)`), `zygo_mail_sync::env::Probe` (trait: `fn probe(&self, https_url: &str) -> Verdict`), `zygo_mail_sync::env::Spawner` (trait: `fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()>`), `zygo_mail_sync::env::Crash`, `zygo_mail_sync::env::Seam` (trait: `fn at(&self, point: &'static str) -> Result<(), Crash>`), `zygo_mail_sync::env::NoSeam`, `zygo_mail_sync::env::Env { pub paths: Paths, pub clock: Box<dyn Clock>, pub probe: Box<dyn Probe>, pub seam: Box<dyn Seam>, pub spawner: Box<dyn Spawner> }`
**Acceptance:** `cargo test -p zygo-mail-sync --test git_builder && cargo test --workspace --locked --no-run` → all pass and the workspace builds on ubuntu, macOS and Windows: the `sync_git` command carries every override and environment entry of `git_command` (`crates/zygo-plugins/src/sync.rs:24-52`) unchanged, plus `-c gc.auto=0 -c maintenance.auto=false`; `has_text` matches without case; `git_command` and its existing tests do not change.

## Plan

### Premises (checked against `origin/phase-3` at `b36b0eb`)

- MS-1 runner: `crates/zygo-work/src/repo.rs` (`pub mod repo`, `crates/zygo-work/src/lib.rs:10`). Exact: `pub enum RunError { NotRun, TimedOut, Failed, Absent }` (`:26`, derives `Debug, Clone, Copy, PartialEq, Eq`); `pub struct Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }` (`:74`); `pub fn run_whole(cmd: Command, deadline: Instant) -> Result<Whole, RunError>` (`:85`; `TimedOut` with no spawn when the deadline is past); `pub enum Flow { Go, Stop }` (`:91`); `pub enum Streamed { Done { ok: bool, err: Vec<u8> }, Stopped }` (`:101`); `pub fn run_stream(cmd: Command, deadline: Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>` (`:171`; each NUL-ended entry without its NUL).
- Builder: `pub fn git_command(cwd: &Path) -> Command` at `crates/zygo-plugins/src/sync.rs:32` (body `:34-52`); `pub mod sync` at `crates/zygo-plugins/src/lib.rs:6`. MS-2 calls it and does not move or change it (S§17 "Reuse `git_command`"; S§18 lets the builder choose).
- ML-2: `pub struct Message` (`crates/zygo-workspace/src/mail/mod.rs:20`), `pub fn mail_dir(paths: &Paths, ws_id: &str) -> PathBuf` (`:91`); `pub mod mail` at `crates/zygo-workspace/src/lib.rs:10`. MS-2 adds only the Cargo dependency; no MS-2 code uses them.
- `Paths`: `zygo_core::paths::Paths` (`crates/zygo-core/src/paths.rs:5`, derives `Debug, Clone, PartialEq, Eq`; `Paths::from_env()` at `:26`).
- DR-5: merged; its only overlap is `Cargo.lock`. No crate `zygo-mail-sync` exists yet (`Cargo.toml:3-17`).
- Later nodes: no other MS node lists `crates/zygo-mail-sync/Cargo.toml` or the root `Cargo.toml` in its Files (MS-4 to MS-16 list only their own `src/` and `tests/` files; MS-16, MS-17, MS-19 list their own crate's `Cargo.toml`). So this node declares every dependency that the S§17 module list needs, and the root workspace dependency `zygo-mail-sync` that MS-16, MS-17 and MS-19 use as `zygo-mail-sync.workspace = true`.
- CI runs `cargo test --workspace --locked --no-fail-fast` on ubuntu, macOS and Windows (`.github/workflows/ci.yml:45`); no clippy or fmt step.

### Interfaces

Consumes (exact, in the code now):
- `zygo_work::repo::{run_whole, run_stream, Whole, Flow, Streamed, RunError}` as listed above (MS-1).
- `zygo_plugins::sync::git_command(cwd: &Path) -> std::process::Command` (existing).
- `zygo_workspace::mail::{Message, mail_dir}` (ML-2), as a Cargo dependency only.
- `zygo_core::paths::Paths` (existing).

Produces (this node):
- `zygo_mail_sync::git::sync_git(cwd: &Path) -> std::process::Command`
- `zygo_mail_sync::git::run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<Whole, RunError>`
- `zygo_mail_sync::git::stream(cwd: &Path, args: &[&str], deadline: Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>`
- `zygo_mail_sync::git::has_text(hay: &[u8], needles: &[&str]) -> bool`
- `zygo_mail_sync::env::{Clock, SystemClock, Verdict, Probe, Spawner, Crash, Seam, NoSeam, Env}` as in the contract.

### Steps

- [x] **1. Crate skeleton.** Root `Cargo.toml`: add `"crates/zygo-mail-sync",` to `members` after `"crates/zygo-workspace",`, and `zygo-mail-sync = { path = "crates/zygo-mail-sync" }` to `[workspace.dependencies]` after the `zygo-workspace` line. New `crates/zygo-mail-sync/Cargo.toml`:

  ```toml
  [package]
  name = "zygo-mail-sync"
  version.workspace = true
  edition.workspace = true
  license.workspace = true

  [dependencies]
  zygo-core.workspace = true
  zygo-config.workspace = true
  zygo-work.workspace = true
  zygo-plugins.workspace = true
  zygo-workspace.workspace = true
  zygo-redact.workspace = true
  anyhow.workspace = true
  serde.workspace = true
  serde_json.workspace = true
  ulid.workspace = true
  rand.workspace = true
  sha2.workspace = true
  hex.workspace = true
  fs4.workspace = true

  [dev-dependencies]
  tempfile.workspace = true

  [target.'cfg(unix)'.dependencies]
  libc.workspace = true
  ```

  (Sources of each dependency: zygo-work and zygo-plugins S-C4/S§8.7; zygo-workspace the mail store, M§9a; zygo-redact S§8.6 (MS-7); zygo-config S§11 settings and `scrub_userinfo` (MS-3, MS-7, MS-8); serde/serde_json state, registry, ack JSON (S§7.3, S§7.4); ulid S§8.5; rand the identity (S§7.5); sha2/hex the public cache (MS-5, MS-8); fs4 the lock (S§8.3 step 3); libc `setsid` (S§8.1, MS-16).) `crates/zygo-mail-sync/src/lib.rs`:

  ```rust
  //! Mail sync between machines over a side branch of the repository's remote (S§1).
  pub mod address;
  pub mod branch;
  pub mod env;
  pub mod git;
  pub mod identity;
  pub mod import;
  pub mod ops;
  pub mod outbox;
  pub mod prune;
  pub mod public;
  pub mod registry;
  pub mod secret;
  pub mod spawn;
  pub mod state;
  pub mod sync;
  ```

  Create each of the 15 module files empty (`env.rs` and `git.rs` too, for now). Check: `cargo build -p zygo-mail-sync` succeeds, and `git diff Cargo.lock` adds only the `[[package]] name = "zygo-mail-sync"` entry (no version change of another package).

- [x] **2. Write the failing tests** in `crates/zygo-mail-sync/tests/git_builder.rs`. The file starts with `#![cfg(unix)]` (review focus 54: on Windows the crate compiles and its tests compile to nothing; S§3 OP5). Helpers as in `crates/zygo-work/tests/git_runner.rs:14`: `fn far() -> Instant { Instant::now() + Duration::from_secs(3600) }`, and `fn init() -> TempDir` that makes a `TempDir` and runs `run(t.path(), &["init", "-q"], far())`, asserting `Ok(w)` with `w.ok`.

  ```rust
  use std::collections::BTreeMap;
  use std::ffi::{OsStr, OsString};
  use std::process::Command;
  use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
  use tempfile::TempDir;
  use zygo_mail_sync::env::{Clock, Crash, Env, NoSeam, Probe, Seam, Spawner, SystemClock, Verdict};
  use zygo_mail_sync::git::{has_text, run, stream, sync_git};
  use zygo_work::repo::{Flow, RunError, Streamed};

  fn parts(c: &Command) -> (OsString, Vec<OsString>, BTreeMap<OsString, Option<OsString>>, Option<std::path::PathBuf>) {
      (c.get_program().to_owned(),
       c.get_args().map(OsStr::to_owned).collect(),
       c.get_envs().map(|(k, v)| (k.to_owned(), v.map(OsStr::to_owned))).collect(),
       c.get_current_dir().map(|d| d.to_owned()))
  }

  #[test]
  fn sync_git_is_git_command_plus_the_two_maintenance_overrides() {
      let t = TempDir::new().unwrap();
      let (p0, mut a0, e0, d0) = parts(&zygo_plugins::sync::git_command(t.path()));
      let (p1, a1, e1, d1) = parts(&sync_git(t.path()));
      a0.extend(["-c", "gc.auto=0", "-c", "maintenance.auto=false"].map(OsString::from));
      assert_eq!((p1, a1, e1, d1), (p0, a0, e0, d0));
  }

  #[test]
  fn run_applies_the_maintenance_overrides() {
      let t = init();
      let w = run(t.path(), &["config", "--get", "gc.auto"], far()).unwrap();
      assert!(w.ok);
      assert_eq!(w.out, b"0\n");
      let w = run(t.path(), &["config", "--get", "maintenance.auto"], far()).unwrap();
      assert_eq!(w.out, b"false\n");
  }

  #[test]
  fn run_non_zero_exit_gives_stderr() {
      let t = TempDir::new().unwrap();
      let w = run(t.path(), &["rev-parse", "--verify", "no-such-ref"], far()).unwrap();
      assert!(!w.ok);
      assert!(!w.err.is_empty());
  }

  #[test]
  fn run_past_deadline_times_out() {
      let t = TempDir::new().unwrap();
      assert_eq!(run(t.path(), &["--version"], Instant::now()), Err(RunError::TimedOut));
  }

  #[test]
  fn stream_passes_each_nul_ended_entry() {
      let t = init();
      let mut seen: Vec<Vec<u8>> = Vec::new();
      let r = stream(t.path(), &["config", "-z", "--get-regexp", r"^(gc|maintenance)\.auto$"], far(),
          &mut |e| { seen.push(e.to_vec()); Flow::Go });
      assert_eq!(r, Ok(Streamed::Done { ok: true, err: Vec::new() }));
      // The command-line entries come last; a user's global config may add earlier ones.
      assert!(seen.ends_with(&[b"gc.auto\n0".to_vec(), b"maintenance.auto\nfalse".to_vec()]), "{seen:?}");
  }

  #[test]
  fn stream_stop_returns_stopped_after_one_entry() {
      let t = init();
      let mut n = 0;
      let r = stream(t.path(), &["config", "-z", "--list"], far(), &mut |_| { n += 1; Flow::Stop });
      assert_eq!(r, Ok(Streamed::Stopped));
      assert_eq!(n, 1);
  }

  #[test]
  fn has_text_matches_without_case() {
      assert!(has_text(b"ERROR: Repository Not Found.", &["repository not found"]));
      assert!(has_text(b"fatal: could not read Username", &["no such thing", "Could Not Read"]));
      assert!(has_text(b"\xff\xfeREPOSITORY NOT FOUND", &["repository not found"]));
      assert!(!has_text(b"everything up-to-date", &["repository not found"]));
      assert!(!has_text(b"anything", &[]));
      assert!(!has_text(b"anything", &[""]));
      assert!(!has_text(b"", &["a"]));
  }

  struct Fixed(SystemTime);
  impl Clock for Fixed { fn now(&self) -> SystemTime { self.0 } }
  struct Says(Verdict);
  impl Probe for Says { fn probe(&self, _: &str) -> Verdict { self.0.clone() } }
  struct CrashAt(&'static str);
  impl Seam for CrashAt { fn at(&self, p: &'static str) -> Result<(), Crash> { if p == self.0 { Err(Crash(p)) } else { Ok(()) } } }
  struct Count(std::rc::Rc<std::cell::Cell<u32>>);
  impl Spawner for Count { fn spawn_sync(&self, _: &str) -> std::io::Result<()> { self.0.set(self.0.get() + 1); Ok(()) } }

  #[test]
  fn env_carries_the_injected_seams() {
      let n = std::rc::Rc::new(std::cell::Cell::new(0));
      let at = UNIX_EPOCH + Duration::from_secs(100);
      let env = Env { paths: zygo_core::paths::Paths::from_env(), clock: Box::new(Fixed(at)),
          probe: Box::new(Says(Verdict::Unknown("x".into()))), seam: Box::new(CrashAt("after_push")),
          spawner: Box::new(Count(n.clone())) };
      assert_eq!(env.clock.now(), at);
      assert_eq!(env.probe.probe("https://h/p"), Verdict::Unknown("x".into()));
      assert_eq!(env.seam.at("before_push"), Ok(()));
      assert_eq!(env.seam.at("after_push"), Err(Crash("after_push")));
      env.spawner.spawn_sync("ws").unwrap();
      assert_eq!(n.get(), 1);
  }

  #[test]
  fn production_seams_are_inert() {
      assert_eq!(NoSeam.at("after_push"), Ok(()));
      let _: SystemTime = SystemClock.now(); // no assertion on its value (S-C9: no wall-clock test)
      assert_ne!(Verdict::Public, Verdict::Private);
      assert_ne!(Verdict::Local, Verdict::Unknown(String::new()));
  }
  ```

- [x] **3. Run and see it fail.** `cargo test -p zygo-mail-sync --test git_builder` → compile error: unresolved imports `zygo_mail_sync::env::*` and `zygo_mail_sync::git::*`.

- [x] **4. `crates/zygo-mail-sync/src/env.rs`** (the S-C9 seams; S§18 leaves their form free). Exact items:
  - `pub trait Clock { fn now(&self) -> SystemTime; }`
  - `#[derive(Debug, Clone, Copy, Default)] pub struct SystemClock;` with `impl Clock` returning `SystemTime::now()`.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum Verdict { Public, Private, Local, Unknown(String) }` (S§8.2 results; `Unknown` carries the reason).
  - `pub trait Probe { fn probe(&self, https_url: &str) -> Verdict; }`
  - `pub trait Spawner { fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()>; }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Crash(pub &'static str);` (the named point at which a test seam stopped the run).
  - `pub trait Seam { fn at(&self, point: &'static str) -> Result<(), Crash>; }`
  - `#[derive(Debug, Clone, Copy, Default)] pub struct NoSeam;` with `impl Seam` returning `Ok(())`.
  - `pub struct Env { pub paths: Paths, pub clock: Box<dyn Clock>, pub probe: Box<dyn Probe>, pub seam: Box<dyn Seam>, pub spawner: Box<dyn Spawner> }` (`Paths` = `zygo_core::paths::Paths`).

  A one-line `///` doc on each item names its S§ source. No production `Probe` or `Spawner` here (MS-8 `LsRemote`, MS-16 `ChildSpawner`).

- [x] **5. `crates/zygo-mail-sync/src/git.rs`** (S-C4, S§8.7):
  - `pub fn sync_git(cwd: &Path) -> Command`: `zygo_plugins::sync::git_command(cwd)` then `.args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])`.
  - `pub fn run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<Whole, RunError>`: `sync_git(cwd)` plus `args`, through `zygo_work::repo::run_whole`.
  - `pub fn stream(cwd: &Path, args: &[&str], deadline: Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>`: `sync_git(cwd)` plus `args`, through `zygo_work::repo::run_stream`.
  - `pub fn has_text(hay: &[u8], needles: &[&str]) -> bool`: true when some non-empty needle occurs in `hay` under ASCII case folding (git runs with `LC_ALL=C`, `crates/zygo-plugins/src/sync.rs:48`); `hay` need not be UTF-8; an empty needle matches nothing (`<[u8]>::windows(0)` would panic).

- [x] **6. Run and see it pass.** `cargo test -p zygo-mail-sync --locked --test git_builder` → 9 passed, 0 failed.

- [x] **7. Workspace and the unchanged builder.** `cargo test --workspace --locked --no-run` → builds every crate and test (the Windows build is proved by the `windows-latest` CI job of the pull request). `git diff --exit-code origin/phase-3 -- crates/zygo-plugins` → exit 0 (`git_command` and its tests `crates/zygo-plugins/tests/sync.rs:66`, `:116` unchanged). `cargo test -p zygo-plugins --locked --test sync` → all pass.

- [x] **8. Commit** `Cargo.toml`, `Cargo.lock`, and every file under `crates/zygo-mail-sync/`, message `MS-2: zygo-mail-sync crate, hardened git calls and test seams`, with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:MS-2`.

### Review round 1

- [x] **F1** (S-G2) A sync fetch wrote commit-graph files when user config set `fetch.writeCommitGraph=true`. Test `sync_fetch_writes_no_commit_graph` failed first; `sync_git` now adds `-c fetch.writeCommitGraph=false`; `sync_git_is_git_command_plus_the_maintenance_overrides` expects the three overrides. Spec S-C4 and the change log updated (SP6 names only the gc and maintenance flags, so it is unchanged; S§17 lists no overrides).
- [x] **F2** `Clock`, `Probe`, `Spawner` and `Seam` are `Send + Sync`; the `Count` fake uses `Arc<AtomicUsize>`; compile-time test `env_is_send_and_sync`.
- [x] **F3** The `SystemClock.now()` call with no assertion is removed from `production_seams_are_inert`.
- [x] **F4** `tempfile` moved from `[dev-dependencies]` to `[dependencies]` (workspace version).
- [x] **F5** Tests `run_non_zero_exit_gives_stdout_and_stderr` (`git rev-parse --git-dir no-such-ref`: `.git` on stdout, exit 128, message on stderr) and `stream_past_deadline_times_out`.
