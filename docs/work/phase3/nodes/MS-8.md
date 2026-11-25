# MS-8 — Public check: URL map, anonymous probe, verdict lists, 1 h cache

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§8.2, S§13 limit 7, S§18 (the `private` message list, with a test per message); S§19 #6
**Files:** `crates/zygo-mail-sync/src/public.rs`, `crates/zygo-mail-sync/tests/public.rs`
**Consumes:** `zygo_work::repo::Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }` (MS-1), `zygo_work::repo::RunError` (MS-1); `zygo_mail_sync::git::sync_git(cwd: &Path) -> std::process::Command` (MS-2), `zygo_mail_sync::git::run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<Whole, RunError>` (MS-2), `zygo_mail_sync::git::has_text(hay: &[u8], needles: &[&str]) -> bool` (MS-2), `zygo_mail_sync::env::Verdict` (MS-2), `zygo_mail_sync::env::Probe` (MS-2), `zygo_mail_sync::env::Clock` (MS-2); `zygo_mail_sync::state::SyncPaths` (MS-5), `zygo_mail_sync::state::State` (MS-5), `zygo_mail_sync::state::PublicCache { pub url_sha256: Vec<String>, pub verdict: String, pub at: u64 }` (MS-5), `zygo_mail_sync::state::load(sp: &SyncPaths) -> std::io::Result<State>` (MS-5), `zygo_mail_sync::state::save(sp: &SyncPaths, s: &State) -> std::io::Result<()>` (MS-5); zygo_config::display::scrub_userinfo(text: &str) -> String (existing, crates/zygo-config/src/display.rs:38)
**Produces:** `zygo_mail_sync::public::Mapped` (enum: `Https(String)`, `Local`, `Unknown`), `zygo_mail_sync::public::map_url(url: &str) -> Mapped`, `zygo_mail_sync::public::PRIVATE_TEXT: &[&str]`, `zygo_mail_sync::public::classify(w: &Whole) -> Verdict`, `zygo_mail_sync::public::probe_command(https_url: &str, empty_home: &Path) -> std::process::Command`, `zygo_mail_sync::public::LsRemote` (the production `Probe`; deadline 10 s), `zygo_mail_sync::public::remote_urls(main_root: &Path, remote: &str, deadline: Instant) -> Result<Vec<String>, RunError>`, `zygo_mail_sync::public::check(sp: &SyncPaths, urls: &[String], probe: &dyn Probe, clock: &dyn Clock) -> Verdict`
**Acceptance:** `cargo test -p zygo-mail-sync --test public` → all pass: a table test maps `https://u:p@h/p` to `https://h/p`, `ssh://user@h:22/p` and `user@h:p` to `https://h/p`, a local path and `file://` to `Local`, other schemes to `Unknown`; `classify` gives `Public` for exit 0, `Private` for each entry of `PRIVATE_TEXT` (one test per entry, any case), `Unknown` otherwise; `probe_command` has `credential.helper=` and `core.askPass=` empty, `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` set to the null device, `GIT_CONFIG_NOSYSTEM=1`, `HOME` the given empty folder, `GIT_TERMINAL_PROMPT=0`, and no `GIT_ASKPASS` or `SSH_ASKPASS`; `check` with the injected clock uses the cache at +59 min and calls the probe again at +61 min and after a URL change; the cache holds SHA-256 hex only, no URL.

## Plan

### Premise check (live code at `origin/phase-3`, `14936bea`; this branch has it)

- MS-1 landed in `crates/zygo-work/src/repo.rs`: `pub enum RunError { NotRun, TimedOut, Failed, Absent }` (`:26`; `Debug, Clone, Copy, PartialEq, Eq`), `pub struct Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }` (`:74`; `Debug, Clone, PartialEq, Eq`, so tests build literals), `pub fn run_whole(cmd: Command, deadline: Instant) -> Result<Whole, RunError>` (`:85`): stdin null, own process group, killed at the deadline, `TimedOut` with no spawn when the deadline is past, up to 64 KiB of stderr on a non-zero exit (S§8.2 step 3, S§8.7). A non-zero git exit is `Ok(Whole { ok: false, .. })`, never `Err(Failed)`. `RunError::reason` is `pub(crate)` (`:35`), so this crate writes its own reason text.
- MS-2 landed `crates/zygo-mail-sync/src/git.rs`: `sync_git(cwd: &Path) -> Command` (`:10`) = `zygo_plugins::sync::git_command(cwd)` plus `-c gc.auto=0 -c maintenance.auto=false -c fetch.writeCommitGraph=false` (S-C4); `run(cwd, args, deadline) -> Result<Whole, RunError>` (`:17`); `has_text(hay: &[u8], needles: &[&str]) -> bool` (`:33`): ASCII case-folded substring, empty needle matches nothing (S§8.7 last bullet).
- Drift 1: `Verdict`, `Probe` and `Clock` are in `zygo_mail_sync::env` (`crates/zygo-mail-sync/src/env.rs:8,18,21`), not in `git`. `Verdict` is `Public | Private | Local | Unknown(String)`; `Clock::now` returns `SystemTime`; `Probe: Send + Sync`. `Env.probe: Box<dyn Probe>` (`env.rs:43`), so MS-17 and MS-19 write `Box::new(LsRemote)`: `LsRemote` is a unit struct.
- `git_command` (`crates/zygo-plugins/src/sync.rs:32-58`) removes every `GIT_CONFIG*` variable, sets `GIT_TERMINAL_PROMPT=0`, `GIT_ALLOW_PROTOCOL=file:https:ssh`, `LC_ALL=C`, and `GIT_CEILING_DIRECTORIES` to the parent of `cwd`. `std::process::Command` keeps one entry per key and the last call wins, so `.env("GIT_CONFIG_GLOBAL", null)` after `sync_git` sets it. The null device there is the private `const NULL` (`sync.rs:21`): `"NUL"` on Windows, else `"/dev/null"`; this node writes the same expression.
- Drift 2 (MS-5 merged after the node file was cut): `SyncPaths::new(paths: &Paths, ws_id: &str) -> std::io::Result<SyncPaths>` (`state.rs:51`), `load(sp: &SyncPaths) -> std::io::Result<State>` (`:309`; missing file → default, a bad file → `Err`), `save(sp: &SyncPaths, s: &State) -> std::io::Result<()>` (`:327`; refuses a verdict outside `["public", "private", "local", "unknown"]` and a hash that is not 64 lower-case hex, `:252,271-276`), `PublicCache { url_sha256: Vec<String>, verdict: String, at: u64 }` (`:214`; `at` is Unix seconds). The Consumes line above now holds these live signatures.
- `zygo_config::display::scrub_userinfo(text: &str) -> String` (`crates/zygo-config/src/display.rs:38`) removes the userinfo of every `<scheme>://` token and leaves the scp form as is.
- `zygo_work::repo::normalize_remote` (`repo.rs:582`) parses remote URLs but lower-cases, drops `.git` and the port, and refuses `file://`: an identity key, not a URL git can fetch. `map_url` does not reuse it; it reuses its Windows-drive rule (`repo.rs:599-601`).
- `sha2`, `hex`, `tempfile`, `zygo-config` are normal dependencies of the crate (`crates/zygo-mail-sync/Cargo.toml`); no `Cargo.toml` change. `src/public.rs` exists and is empty; `pub mod public;` is in `src/lib.rs:11`.
- Consumers: MS-13 (`check`, `remote_urls`), MS-17 and MS-19 (`LsRemote`), MS-24 (`probe_command`, `classify`, `PRIVATE_TEXT`). Each names an item of Produces below; none needs a change.
- The graph's Review focus has no line for MS-8.

### Author's calls (the spec text does not decide them)

- **Hash input.** The cache key of a URL is `hex(sha256(scrub_userinfo(url)))` of the URL as git reports it: "without user info" is the existing scrubber (S§8.2 last bullet names it). The list is sorted and deduplicated, so order does not count.
- **Distinct URL** (S§8.2 step 1) is decided on the mapped form: `https://u@h/p` and `ssh://h/p` give one probe of `https://h/p`.
- **Fold of several URLs:** any `Public` → `Public`; else the first `Unknown` → it; else any `Private` → `Private`; else `Local`. An empty list → `Unknown("the remote has no URL")`.
- **Cache age:** fresh when `at <= now && now - at <= 3600` (S§8.2 "older than 1 h runs again"). A clock before `at` runs the check again.
- **All four verdicts are cached** (S§8.2 "the verdict is cached"). A cached `unknown` comes back as `Unknown("the last check, under 1 h ago, could not tell")`; the reason of the first run is not stored (W-C7: no free text in `state.json`).
- **Unreadable `state.json`:** `check` runs the probe, returns its verdict and does not save, so the other fields (the S§7.5 `pushed`/`pending` of the twin check) are never overwritten with defaults. A failed `save` is ignored; the verdict still stands.
- **Unknown reasons:** a failed probe is `Unknown("git ls-remote failed: <first non-empty stderr line, trimmed, through scrub_userinfo>")` (`"git ls-remote failed"` with empty stderr); `TimedOut` → `"git ls-remote timed out"`; `NotRun` → `"git ls-remote did not start"`; `Failed`/`Absent` → `"git ls-remote failed"`; a temp folder that cannot be made → `"cannot make an empty home folder"`; a scheme with no https form → `"the remote URL has no https form"`.
- **`PRIVATE_TEXT`** (S§18; git 2.50 with libcurl, and the GitHub and GitLab remote lines; MS-24 checks them live, MS-25 records the git version): `could not read Username`, `could not read Password`, `Authentication failed`, `Repository not found`, `' not found`, `returned error: 401`, `returned error: 404`, `HTTP Basic: Access denied`, `could not be found or you don't have permission`. HTTP 403 is not in the list: forges also send it for rate limits, so it stays `unknown`.
- **Transport-helper form** `<transport>::<address>` (`ext::…`) is "any other scheme" → `Unknown`.
- **Scheme case:** `https`, `ssh`, `file` match without case.
- Pure tests (`map_url`, `classify`, the list, `probe_command`) run on every OS; tests that run git or write workspace state are in `#[cfg(unix)] mod unix` (Global constraints).

### Coverage

| Spec | Step |
|---|---|
| S§8.2 step 1 (fetch and push URL, each distinct) | 7–9 (`remote_urls`), 10–12 (distinct on the mapped form) |
| S§8.2 step 2 (URL map) | 1–4 |
| S§8.2 step 3 (anonymous `ls-remote`, environment, 10 s, proxies stay, 64 KiB stderr) | 7–9 (`probe_command`, `LsRemote`) |
| S§8.2 step 4, S§18 (verdict, `private` list, a test per message, any case) | 1–4 (`classify`, `PRIVATE_TEXT`) |
| S§8.2 cache bullet, S§13 limit 7 (SHA-256 only, 1 h of the injected clock, URL change) | 10–12 |
| S§8.2 printing bullet (URL only through `scrub_userinfo`, never in a log) | 3 (reason scrubbed); this module writes no log |
| S§19 #6 table, 59/61 min, probe command | 1–12 |
| S§19 #6 sync exit 1 and unchanged refs, refused remote send, printed `unknown` line; the S§8.2 `public`/`private`/`unknown` handling | MS-13, MS-9, MS-17 (they consume `check`) |

### Interfaces

Consumes: the **Consumes** line above, exactly, with these shapes: `pub enum RunError { NotRun, TimedOut, Failed, Absent }` (`crates/zygo-work/src/repo.rs:26`); `pub enum Verdict { Public, Private, Local, Unknown(String) }` (`crates/zygo-mail-sync/src/env.rs:18`); `pub trait Probe: Send + Sync { fn probe(&self, https_url: &str) -> Verdict; }` (`env.rs:21`); `pub trait Clock: Send + Sync { fn now(&self) -> SystemTime; }` (`env.rs:8`).

Produces (all in `crates/zygo-mail-sync/src/public.rs`, module `zygo_mail_sync::public`; one `///` line each naming its S§ source):
- `#[derive(Debug, Clone, PartialEq, Eq)] pub enum Mapped { Https(String), Local, Unknown }`
- `pub fn map_url(url: &str) -> Mapped`
- `pub const PRIVATE_TEXT: &[&str]` — the nine entries above, in that order
- `pub fn classify(w: &Whole) -> Verdict`
- `pub fn probe_command(https_url: &str, empty_home: &Path) -> std::process::Command`
- `#[derive(Debug, Clone, Copy, Default)] pub struct LsRemote;` and `impl Probe for LsRemote`
- `pub fn remote_urls(main_root: &Path, remote: &str, deadline: Instant) -> Result<Vec<String>, RunError>`
- `pub fn check(sp: &SyncPaths, urls: &[String], probe: &dyn Probe, clock: &dyn Clock) -> Verdict` — the caller holds `mail/sync/lock` (S§15: the pass under the lock is the only writer of `state.json`)

### Steps

- [ ] 1. Create `crates/zygo-mail-sync/tests/public.rs` with the pure tests:
  ```rust
  //! MS-8: the public check (mail sync design S§8.2, S§13 limit 7, S§18, S§19 #6). No test depends on wall-clock time.
  use zygo_mail_sync::env::Verdict;
  use zygo_mail_sync::public::*;
  use zygo_work::repo::Whole;

  fn fail(err: &str) -> Whole { Whole { ok: false, out: vec![], err: err.as_bytes().to_vec() } }

  #[test]
  fn map_url_table() {
      use Mapped::*;
      let h = |s: &str| Https(s.to_string());
      for (url, want) in [
          ("https://u:p@h/p", h("https://h/p")), ("https://h/p", h("https://h/p")),
          ("HTTPS://h/p", h("https://h/p")), ("https://h:8443/p", h("https://h:8443/p")),
          ("ssh://user@h:22/p", h("https://h/p")), ("ssh://h/p", h("https://h/p")),
          ("user@h:p", h("https://h/p")), ("h:p", h("https://h/p")),
          ("git@h:/srv/p.git", h("https://h/srv/p.git")),
          ("/srv/r.git", Local), ("./r", Local), ("../r", Local), ("r", Local), ("./a:b", Local),
          ("C:/r", Local), ("file:///srv/r.git", Local),
          ("http://h/p", Unknown), ("git://h/p", Unknown), ("ext::sh -c x", Unknown), ("https:///p", Unknown),
      ] {
          assert_eq!(map_url(url), want, "{url}");
      }
  }

  #[test]
  fn classify_exit_0_is_public() {
      assert_eq!(classify(&Whole { ok: true, out: b"abc\trefs/heads/main\n".to_vec(), err: vec![] }), Verdict::Public);
  }

  #[test]
  fn classify_other_failure_is_unknown_with_a_scrubbed_reason() {
      assert_eq!(classify(&fail("\nfatal: unable to access 'https://u:secret@h/p/': Could not resolve host: h\n")),
          Verdict::Unknown("git ls-remote failed: fatal: unable to access 'https://h/p/': Could not resolve host: h".into()));
      assert_eq!(classify(&fail("")), Verdict::Unknown("git ls-remote failed".into()));
      assert!(matches!(classify(&fail("fatal: unable to access 'https://h/p/': SSL certificate problem")), Verdict::Unknown(_)));
      assert!(matches!(classify(&fail("The requested URL returned error: 403")), Verdict::Unknown(_)));
  }

  #[test]
  fn private_text_is_this_list() {
      assert_eq!(PRIVATE_TEXT, &["could not read Username", "could not read Password", "Authentication failed",
          "Repository not found", "' not found", "returned error: 401", "returned error: 404",
          "HTTP Basic: Access denied", "could not be found or you don't have permission"]);
  }

  /// `line` as git prints it, upper-cased and lower-cased, is `Private`.
  fn assert_private(line: &str) {
      for l in [line.to_string(), line.to_uppercase(), line.to_lowercase()] {
          assert_eq!(classify(&fail(&l)), Verdict::Private, "{l}");
      }
  }
  ```
  and one `#[test]` per entry, each calling `assert_private` with one line:
  `private_could_not_read_username` ← `"fatal: could not read Username for 'https://h': terminal prompts disabled"`;
  `private_could_not_read_password` ← `"fatal: could not read Password for 'https://u@h': terminal prompts disabled"`;
  `private_authentication_failed` ← `"fatal: Authentication failed for 'https://h/p/'"`;
  `private_repository_not_found` ← `"remote: Repository not found."`;
  `private_quoted_not_found` ← `"fatal: repository 'https://h/p/' not found"`;
  `private_error_401` ← `"fatal: unable to access 'https://h/p/': The requested URL returned error: 401"`;
  `private_error_404` ← `"fatal: unable to access 'https://h/p/': The requested URL returned error: 404"`;
  `private_http_basic_access_denied` ← `"remote: HTTP Basic: Access denied. If a password was provided for Git authentication, it was incorrect"`;
  `private_gitlab_not_found_or_no_permission` ← `"remote: The project you were looking for could not be found or you don't have permission to view it."`.
- [ ] 2. Run `cargo test -p zygo-mail-sync --test public` and see it fail to compile (`map_url`, `classify`, `PRIVATE_TEXT`, `Mapped` not found).
- [ ] 3. In `crates/zygo-mail-sync/src/public.rs` add `Mapped`, `map_url`, `PRIVATE_TEXT` (the nine entries, as git prints them) and `classify` with the Produces signatures. `classify`: `ok` → `Public`; `has_text(&w.err, PRIVATE_TEXT)` → `Private`; else `Unknown` with the reason of the author's calls (stderr through `String::from_utf8_lossy`). `map_url` rules, in order, on `s = url.trim()`:
  1. `s` holds `://`: the scheme is the text before it, matched without case. `https` and `ssh`: the authority runs to the first `/` after `://` (or the end), the path is the rest; the host is the authority after its last `@`. `https` keeps the port: `https://<host><path>`. `ssh` drops a trailing `:<digits>` from the host: `https://<host>/<path without leading />`. `file` → `Local`. Any other scheme → `Unknown`. An empty host → `Unknown`.
  2. Else `s` holds `::` and the text before it has no `/` → `Unknown` (transport helper).
  3. Else split at the first `:`. No `:`, or a `/` before it → `Local`. The host is the text before the `:` after its last `@`; a host of one ASCII letter is a Windows drive → `Local` (as `crates/zygo-work/src/repo.rs:599-601`); an empty host → `Unknown`; else `https://<host>/<rest without leading />`.
- [ ] 4. Run `cargo test -p zygo-mail-sync --test public`: 13 passed. Commit `crates/zygo-mail-sync/src/public.rs` and `crates/zygo-mail-sync/tests/public.rs` with message `MS-8: URL map and the private list` and the trailers `Zygo-Session:` and `Zygo-Work:` with the values of the implementer's brief, each on its own line at the end.
- [ ] 5. Append to `tests/public.rs`:
  ```rust
  const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

  #[test]
  fn probe_command_is_anonymous() {
      use std::ffi::OsStr;
      let home = tempfile::TempDir::new().unwrap();
      let c = probe_command("https://h/p", home.path());
      let args: Vec<&OsStr> = c.get_args().collect();
      let tail: Vec<&OsStr> = ["-c", "credential.helper=", "-c", "core.askPass=", "ls-remote", "--heads", "https://h/p"]
          .iter().map(OsStr::new).collect();
      assert!(args.ends_with(&tail), "{args:?}");
      for kv in ["gc.auto=0", "maintenance.auto=false", "fetch.writeCommitGraph=false"] {
          assert!(args.contains(&OsStr::new(kv)), "{kv}");
      }
      let envs: std::collections::BTreeMap<&OsStr, Option<&OsStr>> = c.get_envs().collect();
      let e = |k: &str| envs.get(OsStr::new(k)).copied();
      assert_eq!(e("GIT_CONFIG_GLOBAL"), Some(Some(OsStr::new(NULL))));
      assert_eq!(e("GIT_CONFIG_SYSTEM"), Some(Some(OsStr::new(NULL))));
      assert_eq!(e("GIT_CONFIG_NOSYSTEM"), Some(Some(OsStr::new("1"))));
      assert_eq!(e("HOME"), Some(Some(home.path().as_os_str())));
      assert_eq!(e("GIT_TERMINAL_PROMPT"), Some(Some(OsStr::new("0"))));
      assert_eq!(e("GIT_ASKPASS"), Some(None));
      assert_eq!(e("SSH_ASKPASS"), Some(None));
      for k in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"] {
          assert_eq!(e(k), None, "{k} stays as the environment has it");
      }
      assert_eq!(c.get_current_dir(), Some(home.path()));
  }
  ```
  and, in a new `#[cfg(unix)] mod unix { use super::*; ... }` with `fn far() -> std::time::Instant { std::time::Instant::now() + std::time::Duration::from_secs(3600) }` and `fn git(dir: &std::path::Path, args: &[&str])` (runs `git` in `dir`, asserts success):
  - `ls_remote_closed_port_is_unknown`: `assert!(matches!(LsRemote.probe("https://127.0.0.1:1/p"), Verdict::Unknown(_)))` (`use zygo_mail_sync::env::Probe;`).
  - `remote_urls_gives_fetch_then_push`: in a `git init -q` temp folder, `remote add origin https://u:p@h/p`, `remote set-url --push origin ssh://git@h/p`; `assert_eq!(remote_urls(d, "origin", far()), Ok(vec!["https://u:p@h/p".to_string(), "ssh://git@h/p".to_string()]))`.
  - `remote_urls_gives_one_url_when_push_is_fetch`: only `remote add origin https://h/p`; `assert_eq!(remote_urls(d, "origin", far()), Ok(vec!["https://h/p".to_string()]))`.
  - `remote_urls_missing_remote_fails`: `assert_eq!(remote_urls(d, "nope", far()), Err(zygo_work::repo::RunError::Failed))`.
- [ ] 6. Run `cargo test -p zygo-mail-sync --test public` and see it fail to compile (`probe_command`, `LsRemote`, `remote_urls` not found).
- [ ] 7. Add `probe_command`: `sync_git(empty_home)`, then args `-c credential.helper= -c core.askPass= ls-remote --heads <https_url>`, env `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` = the null device (`if cfg!(windows) { "NUL" } else { "/dev/null" }`), `GIT_CONFIG_NOSYSTEM=1`, `HOME=empty_home`, `GIT_TERMINAL_PROMPT=0`, `env_remove` `GIT_ASKPASS` and `SSH_ASKPASS`; nothing else (proxy variables stay).
- [ ] 8. Add `LsRemote`: `probe` makes a `tempfile::TempDir`, runs `run_whole(probe_command(https_url, dir.path()), Instant::now() + Duration::from_secs(10))` (S§8.2 step 3, 10 s) and returns `classify` of the `Whole`, or the `Unknown` reason of the author's calls for an `Err` or a folder that cannot be made. Add `remote_urls`: `git::run(main_root, &["remote", "get-url", remote], deadline)`, then the same with `"--push"` before `remote`; `?` passes a `RunError` on; a `Whole` with `ok == false`, stdout that is not UTF-8 or is empty after `trim_end()` → `Err(RunError::Failed)`; return the fetch URL, then the push URL when it differs.
- [ ] 9. Run `cargo test -p zygo-mail-sync --test public`: 18 passed. Commit both files, message `MS-8: anonymous probe and remote URLs`, trailers as in step 4.
- [ ] 10. Append to `mod unix` in `tests/public.rs`:
  ```rust
  use sha2::{Digest, Sha256};
  use std::sync::Mutex;
  use std::time::{Duration, SystemTime, UNIX_EPOCH};
  use zygo_core::paths::Paths;
  use zygo_mail_sync::env::{Clock, Probe};
  use zygo_mail_sync::state::{load, save, State, SyncPaths};

  struct At(SystemTime);
  impl Clock for At { fn now(&self) -> SystemTime { self.0 } }
  /// Minutes after T0 (negative: before).
  fn at(min: i64) -> At {
      let t0 = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
      At(if min >= 0 { t0 + Duration::from_secs(min as u64 * 60) } else { t0 - Duration::from_secs((-min) as u64 * 60) })
  }
  /// Answers `Private` unless `by` names the URL; records each URL it is asked.
  struct Fake { by: Vec<(&'static str, Verdict)>, seen: Mutex<Vec<String>> }
  impl Probe for Fake {
      fn probe(&self, u: &str) -> Verdict {
          self.seen.lock().unwrap().push(u.to_string());
          self.by.iter().find(|(k, _)| *k == u).map_or(Verdict::Private, |(_, v)| v.clone())
      }
  }
  fn fake(by: &[(&'static str, Verdict)]) -> Fake { Fake { by: by.to_vec(), seen: Mutex::new(vec![]) } }
  fn seen(f: &Fake) -> Vec<String> { f.seen.lock().unwrap().clone() }
  fn urls(u: &[&str]) -> Vec<String> { u.iter().map(|s| s.to_string()).collect() }
  fn sha(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }
  fn setup() -> (tempfile::TempDir, SyncPaths) {
      let t = tempfile::TempDir::new().unwrap();
      let p = t.path();
      let paths = Paths {
          config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
          sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
          plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"),
          usage_db: p.join("data/usage.db"),
      };
      let sp = SyncPaths::new(&paths, "0123456789abcdef").unwrap();
      (t, sp)
  }
  ```
  (`std::fs` is imported in `mod unix` for the tests below.) Tests:
  - `check_uses_the_cache_at_59_minutes`: `check(&sp, &urls(&["https://h/p"]), &f, &at(0))` → `Private`, `seen(&f).len() == 1`; at `at(59)` → `Private`, still 1.
  - `check_probes_again_at_61_minutes`: at `at(0)` then `at(61)` → `seen(&f).len() == 2`.
  - `check_probes_again_after_a_url_change`: `["https://h/p"]` at `at(0)`, `["https://h/q"]` at `at(1)` → `seen(&f) == ["https://h/p", "https://h/q"]`.
  - `check_probes_again_when_the_clock_is_before_the_cache`: `at(0)` then `at(-1)` → 2.
  - `cache_holds_sha256_hex_only`: `check(&sp, &urls(&["https://u:secret@h/p", "ssh://git@h:22/p"]), &f, &at(0))`; the text of `sp.state` contains none of `"secret"`, `"h/p"`, `"https"`, `"ssh"`; `load(&sp).unwrap().public.unwrap()` has `url_sha256 == { let mut v = vec![sha("https://h/p"), sha("ssh://h:22/p")]; v.sort(); v }`, `verdict == "private"`, `at == 1_800_000_000`.
  - `check_probes_each_distinct_mapped_url_once`: `["https://u@h/p", "ssh://git@h:22/p", "git@h:p"]` → `seen(&f) == ["https://h/p"]`.
  - `public_for_any_url_wins`: `fake(&[("https://h/b", Verdict::Public)])`, `["https://h/a", "https://h/b"]` → `Public`.
  - `local_and_unknown_need_no_probe`: `["/srv/r.git", "file:///srv/r.git"]` → `Local`; `["git://h/p"]` → `Unknown("the remote URL has no https form".into())`; `[]` → `Unknown("the remote has no URL".into())`; `seen(&f)` empty.
  - `cached_unknown_keeps_unknown`: `fake(&[("https://h/p", Verdict::Unknown("x".into()))])`; at `at(0)` → `Unknown("x".into())`; at `at(30)` → `Unknown("the last check, under 1 h ago, could not tell".into())`, `seen(&f).len() == 1`.
  - `unreadable_state_runs_the_probe_and_writes_nothing`: `fs::create_dir_all(&sp.sync)`, write `b"not json"` to `sp.state`; `check` → `Private`, `seen(&f).len() == 1`, `fs::read(&sp.state) == b"not json"`.
  - `check_keeps_the_other_state_fields`: `save(&sp, &State { last_ok: Some(5), ci_note_shown: true, ..Default::default() })`; after `check`, `load` has `last_ok == Some(5)`, `ci_note_shown`, and `public.is_some()`.
- [ ] 11. Run `cargo test -p zygo-mail-sync --test public` and see it fail to compile (`check` not found).
- [ ] 12. Add `check` with the Produces signature, by the author's calls: `now` = Unix seconds of `clock.now()` (0 before the epoch); the sorted, deduplicated key list; `load(sp)`; a fresh cache with equal keys and a known verdict name returns at once; else map each URL, probe each distinct `Https` URL once, fold, and when `load` succeeded set `public = Some(PublicCache { url_sha256: keys, verdict: <"public" | "private" | "local" | "unknown">, at: now })` and `save` (a `save` error is ignored). Run `cargo test -p zygo-mail-sync --test public`: 29 passed. Commit both files, message `MS-8: cached public check`, trailers as in step 4.
- [ ] 13. Verify: `cargo test -p zygo-mail-sync --test public` → 29 passed, 0 failed; `cargo test --workspace --locked --no-run` → builds; `grep -rn "eprintln\|errors.log\|log_line" crates/zygo-mail-sync/src/public.rs` → no match (S§8.2: a URL never goes into a log).
