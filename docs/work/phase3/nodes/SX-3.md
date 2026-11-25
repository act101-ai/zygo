# SX-3 — `zygo hook pre-compact` sets the per-subagent re-inject flag

**Spec:** docs/specs/2026-11-24-session-text-design.md §9.2 item 1, §14 item 2, §16 rows `pre_compact.rs` and `session_dir.rs`, §17 check 7; T-C1
**Files:** `crates/zygo-core/src/session_dir.rs`, `crates/zygo-hooks/src/pre_compact.rs`, `crates/zygo-hooks/tests/reinject.rs` (new)
**Consumes:** —
**Produces:** `SessionDir::reinject(&self) -> PathBuf`, `pre_compact::flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf>`
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test reinject -- pre_compact --list 2>/dev/null | grep -c ': test$')" -ge 5 && cargo test -p zygo-hooks --test reinject -- pre_compact` → every count holds (at least 5 named `pre_compact…` in zygo-hooks `reinject`; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows: T§17 check 7 (`PreCompact-sub.json` prints the §10.6 text and creates `reinject/a1a80dd100f197242`, with file `0600` and directory `0700` asserted under `cfg(unix)`; no file for `PreCompact-main.json`, for `agent_type` `zygo-read`, or for `agent_id` `../x`; `flag_path` is `None` for an id that fails the 1-64 `[A-Za-z0-9_-]` rule) and T§14 item 2 (flag cannot be created: the §10.6 text is still printed, one stderr line, exit 0)

## Plan

**Premises checked against the live code (branch `phase3/SX-3` at `3764722`, equal to `origin/phase-3`):**

- `pre_compact::run` (`crates/zygo-hooks/src/pre_compact.rs:9-11`) ignores `_input` and calls `run_in(dir: Option<&SessionDir>)` (`:13-19`), which gives `HookOutput::Text(text(&units))`, the §10.6 text. §10.6 is in the base spec (`docs/specs/2026-11-18-zygo-design.md:518-520`). `run_in` is called by 10 tests in `crates/zygo-hooks/tests/session_text.rs` (`:309-400`), a file outside this node's Files, so its signature stays.
- `zygo hook pre-compact` is `TABLE` row `crates/zygo/src/cmd/hook.rs:29` (`reads_stdin: true`). It runs through `guarded` (`crates/zygo-core/src/hook.rs:76-92`). On `Err`, `guarded` prints nothing on stdout and writes one `error_line`. So a create failure that must still print the text (T§14 item 2) calls `error_line` (`hook.rs:95-97`) itself and returns `Ok`, as `session_start::run_member` does (`crates/zygo-hooks/src/session_start.rs:95-102`).
- `HookInput` (`hook.rs:9-18`): `agent_id` is `Some` only for a string value. There is no `agent_type` field, so the check reads `input.raw["agent_type"]`, as `session_start::subagent` does (`session_start.rs:37`).
- The `zygo-` rule `zygo_work::hooks::is_zygo` (`crates/zygo-work/src/hooks.rs:159-161`) and the agent-name rule `zygo_work::log::check_agent` (`crates/zygo-work/src/log.rs:140-146`, 1-64 of `[A-Za-z0-9_-]`) are both `pub(crate)`. `crates/zygo-work` is not in Files, so `pre_compact.rs` repeats both rules, as SX-1 did for `is_zygo`.
- 0700 directory: `SessionDir::create` (`crates/zygo-core/src/session_dir.rs:36-52`) is recursive, uses mode 0700 and calls `set_permissions(0o700)`. `SessionDir::new(dir.reinject()).create()` reuses it. 0600 file: no public helper exists. The crate pattern is `OpenOptionsExt::mode(&mut o, 0o600)` under `cfg(unix)` (`crates/zygo-hooks/src/statusline/data.rs:250-253`). `create(true)` needs an explicit `truncate(false)` (clippy `suspicious_open_options`), as in `write_token` (`session_dir.rs:118`).
- `SessionDir` path methods: `results()` and `work()` are at `session_dir.rs:74-79`. `reinject()` goes right after `work()` (T§16).
- Fixtures: `fixtures/claude-code/hooks/PreCompact-sub.json` (`agent_id` `a1a80dd100f197242`, `agent_type` `claude`) and `fixtures/claude-code/hooks/PreCompact-main.json` (no `agent_id`).
- Test pattern: `crates/zygo-hooks/tests/subagent_text.rs` has `fixture` (`:34-37`), `with` (`:39-42`), `input` (`:49-51`) and `sd` (`:71-76`, `SessionDir::new(tempdir/"s")` and `create()`). For the stderr line it runs the test binary as a child with an env var and an `#[ignore]` child test (`:162-205`). `KEEP_NO_UNIT` is `crates/zygo-hooks/tests/session_text.rs:31`.
- A regular file at `<session>/reinject` makes `DirBuilder::create` (recursive) fail on every OS. This failure is deterministic: no clock and no sleep.
- CI runs `cargo test --workspace --locked --no-fail-fast` on ubuntu, macOS and Windows (`.github/workflows/ci.yml:45`). Mode asserts are under `cfg(unix)`.
- `crates/zygo-hooks/tests/reinject.rs` does not exist yet. SX-4 adds `trim…` tests to it later (`docs/work/phase3/nodes/SX-4.md:4`).
- Review focus 71 (`docs/work/phase3/graph.md:184`) → tests `pre_compact_main_creates_no_flag`, `pre_compact_zygo_type_creates_no_flag`, `pre_compact_bad_agent_id_creates_no_file` and `pre_compact_flag_create_failure_still_prints_text`.

### Interfaces

Consumes: — (uses the existing `pre_compact::run_in`, `SessionDir::create`, `zygo_core::hook::{guarded, error_line}`).

Produces:

```rust
// crates/zygo-core/src/session_dir.rs, after work()
/// Per-subagent re-inject flags (session text design T§9.2): directory 0700, files 0600.
pub fn reinject(&self) -> PathBuf; // self.root.join("reinject")

// crates/zygo-hooks/src/pre_compact.rs
/// `reinject/<agent_id>` when `agent_id` passes the work-log agent-name rule (1-64 of `[A-Za-z0-9_-]`, the rule of
/// `zygo_work::log::check_agent`); else `None`.
pub fn flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf>;
/// T§9.2 item 1: the output of `run_in(dir)`. For an input whose `agent_id` is a string and whose `agent_type` does not
/// start with `zygo-`, it also creates the empty flag `flag_path(dir, agent_id)`. If the flag cannot be created, it
/// writes one stderr line and still returns the text (T§14 item 2).
pub fn run_with(input: &HookInput, dir: Option<&SessionDir>) -> anyhow::Result<HookOutput>;
```

### Author's calls

- New `pub fn run_with(input, dir)` is the testable entry. `run(input)` becomes `run_with(input, SessionDir::from_env().as_ref())`. `run_in` stays as it is, because `tests/session_text.rs` (not in Files) calls it.
- The text is computed first (`run_in(dir)?`), then the flag. If `run_in` fails (unreadable work directory), the result is today's C6 error and no flag is created.
- The flag is opened with `create(true)` and `truncate(false)`, not `create_new`. A second compaction before the next trim-matched call keeps one flag and is not an error.
- The stderr text is `pre-compact: cannot create <path>: <error>`, so `error_line` writes `zygo hook: pre-compact: cannot create <path>: <error>`. The test asserts the prefix up to `<path>: ` and exactly one line. The OS error text differs per OS.
- An `agent_id` that is not a string (`7`, `null`) creates no file and writes nothing to stderr ("another value creates no file", T§9.2 item 1).
- The C6 child test is named `child_flag_blocked`. Its name does not contain `pre_compact`, so the Acceptance count and filter do not include it.

### Steps

- [ ] **Step 1: write the tests.** Create `crates/zygo-hooks/tests/reinject.rs`:

  ```rust
  //! SX-3: `zygo hook pre-compact` sets the per-subagent re-inject flag (session text design T§9.2 item 1, T§14
  //! item 2, T§17 check 7). No test spawns `claude` or `opencode` or reads a clock; the only child process is this
  //! test binary.

  use std::path::Path;
  use std::process::{Command, Stdio};

  use serde_json::{json, Value};
  use zygo_core::{hook::{guarded, HookInput}, session_dir::SessionDir, Agent};
  use zygo_hooks::pre_compact;

  /// The §10.6 text with no open unit (`docs/specs/2026-11-18-zygo-design.md:520`), byte for byte as
  /// `tests/session_text.rs:31`.
  const KEEP_NO_UNIT: &str = "Keep in the summary: the plan or tracker path and the current step; the pending briefs and what each subagent reported; the result ids zygo stored (result_view reads them again). Drop: tool output that is in the result store; file contents already edited and committed.";
  const ID: &str = "a1a80dd100f197242";
  const CHILD: &str = "ZYGO_SX3_C6_CHILD";

  // fixture, with, input, sd: copied from tests/subagent_text.rs:34-42, :49-51, :71-76.
  fn run(d: &SessionDir, v: &Value) -> String {
      pre_compact::run_with(&input(v), Some(d)).unwrap().to_stdout()
  }

  #[test]
  fn pre_compact_sub_prints_text_and_creates_flag() {
      // T§17 check 7
      let (_t, d) = sd();
      let sub = fixture("PreCompact-sub");
      assert_eq!((&sub["agent_id"], &sub["agent_type"]), (&json!(ID), &json!("claude")));
      assert_eq!(d.reinject(), d.root.join("reinject"));
      assert_eq!(run(&d, &sub), KEEP_NO_UNIT);
      let f = d.root.join("reinject").join(ID);
      let m = std::fs::metadata(&f).unwrap();
      assert!(m.is_file());
      assert_eq!(m.len(), 0);
      #[cfg(unix)]
      {
          use std::os::unix::fs::PermissionsExt;
          assert_eq!(m.permissions().mode() & 0o777, 0o600);
          assert_eq!(std::fs::metadata(d.reinject()).unwrap().permissions().mode() & 0o777, 0o700);
      }
      // A second compaction of the same subagent: same text, still one flag.
      assert_eq!(run(&d, &sub), KEEP_NO_UNIT);
      assert_eq!(std::fs::read_dir(d.reinject()).unwrap().count(), 1);
  }

  #[test]
  fn pre_compact_main_creates_no_flag() {
      let (_t, d) = sd();
      assert_eq!(run(&d, &fixture("PreCompact-main")), KEEP_NO_UNIT);
      assert!(!d.reinject().exists());
  }

  #[test]
  fn pre_compact_zygo_type_creates_no_flag() {
      let (_t, d) = sd();
      assert_eq!(run(&d, &with(fixture("PreCompact-sub"), "agent_type", json!("zygo-read"))), KEEP_NO_UNIT);
      assert!(!d.reinject().exists());
  }

  #[test]
  fn pre_compact_bad_agent_id_creates_no_file() {
      let (_t, d) = sd();
      for id in [json!("../x"), json!(""), json!("a".repeat(65)), json!("a/b"), json!(7), Value::Null] {
          assert_eq!(run(&d, &with(fixture("PreCompact-sub"), "agent_id", id.clone())), KEEP_NO_UNIT, "{id}");
          assert!(!d.reinject().exists(), "{id}");
          assert!(!d.root.join("x").exists(), "{id}");
      }
  }

  #[test]
  fn pre_compact_flag_path_checks_the_id() {
      let (_t, d) = sd();
      for ok in ["a", ID, "A-z_9", &"a".repeat(64)] {
          assert_eq!(pre_compact::flag_path(&d, ok), Some(d.reinject().join(ok)), "{ok}");
      }
      for bad in ["", "../x", "..", "a/b", "a\\b", "a b", "a.b", "é", &"a".repeat(65)] {
          assert_eq!(pre_compact::flag_path(&d, bad), None, "{bad}");
      }
  }

  #[test]
  fn pre_compact_flag_create_failure_still_prints_text() {
      // T§14 item 2: a regular file where `reinject/` must be makes the create fail on every OS.
      let (t, d) = sd();
      std::fs::write(d.reinject(), "").unwrap();
      let o = Command::new(std::env::current_exe().unwrap())
          .args(["--ignored", "--exact", "child_flag_blocked", "--nocapture", "--test-threads=1"])
          .env(CHILD, t.path())
          .stdin(Stdio::null())
          .output()
          .unwrap();
      assert!(o.status.success(), "{o:?}");
      assert!(String::from_utf8(o.stdout).unwrap().contains("child_flag_blocked ... ok"));
      let err = String::from_utf8(o.stderr).unwrap();
      let flag = d.reinject().join(ID);
      assert!(err.starts_with(&format!("zygo hook: pre-compact: cannot create {}: ", flag.display())), "{err:?}");
      assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
      assert!(std::fs::metadata(d.reinject()).unwrap().is_file());
  }

  /// Child half of `pre_compact_flag_create_failure_still_prints_text`; returns at once without `CHILD`.
  #[test]
  #[ignore]
  fn child_flag_blocked() {
      let Some(root) = std::env::var_os(CHILD) else { return };
      let d = SessionDir::new(Path::new(&root).join("s"));
      let stdin = serde_json::to_vec(&fixture("PreCompact-sub")).unwrap();
      let got = guarded(&|b| pre_compact::run_with(&HookInput::parse(Agent::ClaudeCode, b)?, Some(&d)), &stdin);
      assert_eq!(got, (KEEP_NO_UNIT.to_string(), 0));
  }
  ```

  Check: `grep -c '^fn pre_compact' crates/zygo-hooks/tests/reinject.rs` → 6. `grep -nE "Instant|SystemTime|sleep|Command::new" crates/zygo-hooks/tests/reinject.rs` → only the one `Command::new(std::env::current_exe()…)`.

- [ ] **Step 2: run it red.** `cargo test -p zygo-hooks --test reinject` → it does not compile: `no method named \`reinject\`` (E0599) and `cannot find function \`run_with\`` / `\`flag_path\`` in module `pre_compact` (E0425).

- [ ] **Step 3: add the signatures.** In `crates/zygo-core/src/session_dir.rs`, after `work()` (`:77-79`), add `reinject()` with the doc line of Interfaces. In `crates/zygo-hooks/src/pre_compact.rs`, change the import to `std::path::{Path, PathBuf}`, then add `flag_path` with its full body and `run_with` with the body `run_in(dir)` only (no flag yet):

  ```rust
  pub fn flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf> {
      let ok = (1..=64).contains(&agent_id.len())
          && agent_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
      ok.then(|| dir.reinject().join(agent_id))
  }
  ```

  Check: `cargo test -p zygo-hooks --test reinject` compiles. `pre_compact_sub_prints_text_and_creates_flag` fails on `metadata(&f).unwrap()` (NotFound). `pre_compact_flag_create_failure_still_prints_text` fails on `err.starts_with` (stderr empty). The other 4 pass. 4 passed, 2 failed, 1 ignored.

- [ ] **Step 4: create the flag.** In `pre_compact.rs`, import `serde_json::Value` and `zygo_core::hook::error_line`. Then add:

  ```rust
  pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
      run_with(input, SessionDir::from_env().as_ref())
  }

  pub fn run_with(input: &HookInput, dir: Option<&SessionDir>) -> anyhow::Result<HookOutput> {
      let out = run_in(dir)?;
      let zygo = input.raw.get("agent_type").and_then(Value::as_str).is_some_and(|t| t.starts_with("zygo-"));
      if !zygo
          && let (Some(d), Some(id)) = (dir, input.agent_id.as_deref())
          && let Some(p) = flag_path(d, id)
          && let Err(e) = create_flag(d, &p)
      {
          error_line(&format!("pre-compact: cannot create {}: {e:#}", p.display()));
      }
      Ok(out)
  }

  /// The empty flag `p` in `reinject/` (directory 0700, file 0600 on unix).
  fn create_flag(dir: &SessionDir, p: &Path) -> anyhow::Result<()> {
      SessionDir::new(dir.reinject()).create()?;
      let mut o = std::fs::OpenOptions::new();
      o.write(true).create(true).truncate(false);
      #[cfg(unix)]
      std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
      o.open(p)?;
      Ok(())
  }
  ```

  Check: `cargo test -p zygo-hooks --test reinject` → 6 passed, 0 failed, 1 ignored. Mutations: remove `!zygo &&` → `pre_compact_zygo_type_creates_no_flag` fails; use `dir.reinject().join(id)` in place of `flag_path` → `pre_compact_bad_agent_id_creates_no_file` fails; make the error branch return `Err` → `pre_compact_flag_create_failure_still_prints_text` fails.

- [ ] **Step 5: run the crate's tests and lints.** `cargo test -p zygo-hooks --locked --no-fail-fast` → all pass, and the `pre_compact_*` tests of `tests/session_text.rs` are unchanged. `cargo clippy -p zygo-core -p zygo-hooks --tests --locked` → no warning in `session_dir.rs`, `pre_compact.rs` or `tests/reinject.rs`. `cargo fmt --check -p zygo-core -p zygo-hooks` → clean.

- [ ] **Step 6: commit.** `git add crates/zygo-core/src/session_dir.rs crates/zygo-hooks/src/pre_compact.rs crates/zygo-hooks/tests/reinject.rs && git commit` with the subject `SX-3: pre-compact sets the re-inject flag for a non-zygo subagent`, a blank line, then `Zygo-Session: <id of the implementing session>` and `Zygo-Work: phase3:SX-3`, each on its own line. No Claude attribution. Check: `git show --stat HEAD` lists only those 3 files.

- [ ] **Step 7: verify (node Acceptance).** `test "$(cargo test -q -p zygo-hooks --test reinject -- pre_compact --list 2>/dev/null | grep -c ': test$')" -ge 5 && cargo test -p zygo-hooks --test reinject -- pre_compact` → exit 0. The count is 6, and the run shows 6 passed, 0 failed, 0 ignored (the filter excludes `child_flag_blocked`). CI repeats it on ubuntu, macOS and Windows through `cargo test --workspace`.
