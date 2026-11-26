# SX-4 — `zygo hook trim` gives the mechanics text once after a subagent's compaction

**Spec:** docs/specs/2026-11-24-session-text-design.md §9.2 items 2-4, §14 item 2, §17 check 8; T-C1, T-C5
**Files:** `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/reinject.rs`
**Consumes:** `mechanics_with_overrides` (SX-1), `flag_path` (SX-3), `SessionDir::reinject` (SX-3)
**Produces:** `zygo hook trim` output field `hookSpecificOutput.additionalContext`
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test reinject -- trim --list 2>/dev/null | grep -c ': test$')" -ge 10 && cargo test -p zygo-hooks --test reinject -- trim` → every count holds (at least 10 named `trim…` in zygo-hooks `reinject`; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows: the flag is taken and `additionalContext` = M given, once, for each hooked tool from a subagent input (`fixtures/claude-code/hooks/PostToolUse-Read-sub.json`, `PostToolUse-Grep-sub.json`, `PostToolUse-Glob-sub.json`, `PostToolUse-WebFetch-sub.json`, each under the threshold) and for an under-threshold Bash input with results off (`overrides.json` `results = false`), so a flag check placed after the trim's early returns fails (T§9.2 item 2); T§17 check 8 (after the flag, under-threshold `PostToolUse-Bash-sub.json` prints exactly `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":M}}`, the flag is gone, a second run prints nothing; over-threshold gives one object with both `updatedToolOutput` and `additionalContext`; `PostToolUse-Bash-main.json` reads no flag), a flag that cannot be removed gives no text and one stderr line with the trim output unchanged, and an `agent_id` that fails the name rule removes no file; then `cargo test -p zygo-hooks` → all pass with no existing trim test changed

## Plan

**Premises checked against the live code (branch `phase3/SX-4` at `23cf1ddf`):**

- `trim::run` (`crates/zygo-hooks/src/trim.rs:24-30`) returns `HookOutput::None` when `target(input)` is `None` (`:25-27`), before any session folder is read. `trim_in_with` (`:226-280`) has four more early returns: no target (`:227`), results off (`:232-234`), no pending view (`:235`), view not used (`:237-239`). A flag check placed after any of these is skipped for that input (review focus 115). `trim_in` and `trim_in_with` keep their signatures: `tests/shaping_hook.rs`, `tests/shaping_off.rs` and others call them.
- `zygo hook trim` is `TABLE` row `crates/zygo/src/cmd/hook.rs:25` and runs through `guarded` (`crates/zygo-core/src/hook.rs:76-92`): `Err` prints nothing and writes one `error_line` (`:95-97`).
- `HookInput.agent_id` (`hook.rs:15`) is `Some` only for a string value; a main-loop input has none.
- `flag_path` (`crates/zygo-hooks/src/pre_compact.rs:24-28`) applies the agent-name rule before the join. `mechanics_with_overrides` (`crates/zygo-hooks/src/session_start.rs:63-65`) applies the `results` and `work_units` overrides (T-C5). Both are `pub` in this crate. No other function removes a flag.
- serde_json has `preserve_order` in the build (`crates/zygo-results/Cargo.toml:14`), so `hookEventName` stays before `additionalContext` in the output bytes.
- Fixtures: `fixtures/claude-code/hooks/PostToolUse-{Bash,Read,Grep,Glob,WebFetch}-sub.json`, `PostToolUse-Read-image-sub.json` and `PostToolUseFailure-Read-sub.json` all have `agent_id` `a1a80dd100f197242`; each measured text is under 200 bytes, under the 16384 threshold. `PostToolUse-Bash-main.json` has no `agent_id`. No fixture is missing.
- `tests/reinject.rs` (SX-3) has `fixture`, `with`, `input`, `sd`, `ID`, `KEEP_NO_UNIT`, `CHILD`, `CASE`. The stderr check uses its child-process pattern: `Command::env` sets the variables of the child only, never of the test process.
- With `view()` below, M = `mechanics(&view())` is 721 bytes (T§17 check 2); with `results = false` it drops the STORE sentence (164 bytes and one space), 556 bytes.

### Interfaces

Consumes (as they are now):

```rust
// crates/zygo-hooks/src/session_start.rs:63 (SX-1)
pub fn mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String;
// crates/zygo-hooks/src/pre_compact.rs:24 (SX-3)
pub fn flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf>;
// crates/zygo-core/src/session_dir.rs:81 (SX-3)
pub fn reinject(&self) -> PathBuf;
```

Produces: `zygo hook trim` output field `hookSpecificOutput.additionalContext` = M, once per flag. Under the threshold the output is exactly `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":M}}`; over it, one object with `hookEventName`, `updatedToolOutput`, `additionalContext` in that order.

### Author's calls

- The flag is taken first in `trim_in_with`, so all of its early returns follow it; `run` loses its own `target` early return (`:25-27`) and only needs the session folder.
- Only a `PostToolUse` input takes a flag (the output's `hookEventName` is `PostToolUse`); a `PostToolUseFailure` input keeps it.
- The view is loaded before the remove: a view that does not load is today's C6 error, and the flag stays for the next call.
- If the trim itself fails after the flag was taken (store failure), the text is still given: one stderr line, output `additionalContext` only. Without a flag the error is returned as today.
- The stderr text is `trim: cannot remove <path>: <error>`; the test checks the prefix up to `<path>: ` and one line (OS error text differs).

### Steps

- [ ] **Step 1: write the tests.** Append to `crates/zygo-hooks/tests/reinject.rs`. Add `std::path::PathBuf`, `zygo_core::{overrides::Overrides, session_view::SessionView}` and `zygo_hooks::{session_text::mechanics, trim::{self, TrimOpts}}` to the imports. Helpers:

  ```rust
  /// Results, navigation and units on; shape off; threshold 16384, head 60, tail 20.
  fn view() -> SessionView {
      SessionView { session: "S1".into(), agent: Agent::ClaudeCode, results: true, navigation: true, work_units: true,
          shape: false, trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }
  }
  /// `sd()` with `view()` saved and the usage db at `<t>/usage.db` opened once.
  fn tenv() -> (tempfile::TempDir, SessionDir, PathBuf);
  /// stdout of `trim::trim_in_with(input(v), d, db, &TrimOpts::default())` through `guarded`; asserts exit 0.
  fn trim_run(d: &SessionDir, db: &Path, v: &Value) -> String;
  fn m_full() -> String { let m = mechanics(&view()); assert_eq!(m.len(), 721); m }
  fn envelope(m: &str) -> String {
      format!(r#"{{"hookSpecificOutput":{{"hookEventName":"PostToolUse","additionalContext":{}}}}}"#, json!(m))
  }
  /// The flag of `ID`, set by `pre_compact::run_with` with `PreCompact-sub` (T§17 check 7); returns its path.
  fn set_flag(d: &SessionDir) -> PathBuf;
  /// `PostToolUse-Bash-sub` with `stdout` = "line 0\n" .. "line 2999\n" (over 16384 bytes, asserted).
  fn big_bash() -> Value;
  /// set_flag, then `name` gives `envelope(m)` and the flag is gone, then a second run gives "".
  fn once(d: &SessionDir, db: &Path, name: &str, m: &str) {
      let f = set_flag(d);
      assert_eq!(fixture(name)["agent_id"], json!(ID), "{name}");
      assert_eq!(trim_run(d, db, &fixture(name)), envelope(m), "{name}");
      assert!(!f.exists(), "{name}");
      assert_eq!(trim_run(d, db, &fixture(name)), "", "{name}");
  }
  ```

  Tests (each `let (_t, d, db) = tenv();`):

  | Test | Arrange and assert |
  |---|---|
  | `trim_bash_sub_after_pre_compact_gives_mechanics_once` | T§17 check 8: `once(&d, &db, "PostToolUse-Bash-sub", &m_full())`; also `assert_eq!(envelope("M"), r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"M"}}"#)` |
  | `trim_read_sub_gives_mechanics_once` | `once(…, "PostToolUse-Read-sub", &m_full())` |
  | `trim_grep_sub_gives_mechanics_once` | `once(…, "PostToolUse-Grep-sub", &m_full())` |
  | `trim_glob_sub_gives_mechanics_once` | `once(…, "PostToolUse-Glob-sub", &m_full())` |
  | `trim_webfetch_sub_gives_mechanics_once` | `once(…, "PostToolUse-WebFetch-sub", &m_full())` |
  | `trim_results_off_bash_sub_gives_mechanics_once` | `Overrides { results: Some(false), ..Default::default() }.save(&d)`; `let m = mechanics(&SessionView { results: false, ..view() }); assert_eq!(m.len(), 556); assert!(!m.contains("result_view"));` `once(…, "PostToolUse-Bash-sub", &m)` (review focus 115: results off) |
  | `trim_non_hooked_shape_gives_mechanics_once` | review focus 115: `once(…, "PostToolUse-Read-image-sub", &m_full())` |
  | `trim_over_threshold_merges_view_and_mechanics` | `let f = set_flag(&d); let out = trim_run(&d, &db, &big_bash()); assert_eq!(out.lines().count(), 1);` parse `h = v["hookSpecificOutput"]`; `assert_eq!(h.as_object().unwrap().keys().collect::<Vec<_>>(), ["hookEventName", "updatedToolOutput", "additionalContext"]); assert_eq!(h["additionalContext"], json!(m_full())); assert_ne!(h["updatedToolOutput"]["stdout"], big_bash()["tool_response"]["stdout"]); assert!(!f.exists());` second run: keys `["hookEventName", "updatedToolOutput"]` |
  | `trim_main_input_reads_no_flag` | `let f = set_flag(&d); std::fs::write(d.reinject().join("main"), "")`; `assert!(fixture("PostToolUse-Bash-main").get("agent_id").is_none()); assert_eq!(trim_run(&d, &db, &fixture("PostToolUse-Bash-main")), "");` `f` and `reinject/main` exist |
  | `trim_bad_agent_id_removes_no_file` | review focus 114: `set_flag(&d); std::fs::write(d.root.join("x"), "keep")`; for `id` in `[json!("../x"), json!("../view.json"), json!("a/b"), json!(7)]`: `assert_eq!(trim_run(&d, &db, &with(fixture("PostToolUse-Bash-sub"), "agent_id", id.clone())), "", "{id}")`; `d.root.join("x")`, `d.view_json()` and `d.reinject().join(ID)` still exist |
  | `trim_flag_remove_failure_gives_no_text` | T§14 item 2: `std::fs::create_dir_all(d.reinject().join(ID).join("x"))` (a non-empty directory: `remove_file` fails on every OS). Child (`trim_child_case`, case `under`): stdout `""`, `err.starts_with(&format!("zygo hook: trim: cannot remove {}: ", d.reinject().join(ID).display()))`, `err.ends_with('\n') && err.lines().count() == 1`. In process: `big_bash()` output has keys `["hookEventName", "updatedToolOutput"]` only. The directory still exists |
  | `trim_without_flag_output_unchanged` | no flag: `PostToolUse-Bash-sub` → `""`; `big_bash()` → keys `["hookEventName", "updatedToolOutput"]` |
  | `trim_view_load_error_keeps_flag` | `let f = set_flag(&d); std::fs::write(d.view_json(), "{")`; `PostToolUse-Bash-sub` → `""`; `f.exists()` |
  | `trim_store_failure_still_gives_mechanics` | `let f = set_flag(&d); std::fs::write(d.results(), b"in the way")`; `trim_run(&d, &db, &big_bash()) == envelope(&m_full())`; `!f.exists()` |
  | `trim_failure_event_keeps_flag` | `let f = set_flag(&d)`; `PostToolUseFailure-Read-sub` → `""`; `f.exists()` |

  Child: `#[test] #[ignore] fn trim_child_case()` returns at once without `CHILD`; else `d = SessionDir::new(root/"s")`, `db = root/"usage.db"`, runs `guarded(&|b| trim::trim_in_with(&HookInput::parse(Agent::ClaudeCode, b)?, &d, &db, &TrimOpts::default()), &stdin)` on `PostToolUse-Bash-sub` and asserts `(String::new(), 0)`. The parent spawns it as `child` does, with `--exact trim_child_case`, and checks `trim_child_case ... ok`.

- [ ] **Step 2: run red.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks --test reinject -- trim` → compiles; 10 fail: the seven `…_gives_mechanics_once` tests, `trim_over_threshold_merges_view_and_mechanics`, `trim_store_failure_still_gives_mechanics` (no `additionalContext`, flag kept) and `trim_flag_remove_failure_gives_no_text` (no stderr line). The other five pass (they pin today's behavior).

- [ ] **Step 3: implement.** In `crates/zygo-hooks/src/trim.rs`:
  - `run`: delete the `target(input).is_none()` return (`:25-27`); the body is the `SessionDir::from_env` line and `trim_in(...)`.
  - Move today's body of `trim_in_with` unchanged into `fn trim_out(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput>`.
  - Add `fn with_context(out: HookOutput, m: Option<String>) -> HookOutput`: `None` → `out`; `HookOutput::None` → `{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": m}}`; `HookOutput::Json(v)` → `v["hookSpecificOutput"]["additionalContext"] = m`.
  - Add, with imports `crate::pre_compact::flag_path`, `crate::session_start::mechanics_with_overrides`, `zygo_core::hook::error_line`:

    ```rust
    /// T§9.2 item 2: for a `PostToolUse` input whose `agent_id` passes `flag_path`, removes the flag and returns M.
    /// No flag, or a remove that fails, gives `None`; a failed remove writes one stderr line (T§14 item 2).
    fn take_flag(input: &HookInput, dir: &SessionDir) -> anyhow::Result<Option<String>> {
        if input.event.as_deref() != Some("PostToolUse") { return Ok(None) }
        let Some(p) = input.agent_id.as_deref().and_then(|id| flag_path(dir, id)) else { return Ok(None) };
        if std::fs::symlink_metadata(&p).is_err() { return Ok(None) }
        let v = SessionView::load(dir)?;
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(Some(mechanics_with_overrides(dir, &v))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => { error_line(&format!("trim: cannot remove {}: {e}", p.display())); Ok(None) }
        }
    }
    ```
  - `trim_in_with` (signature unchanged): `let m = take_flag(input, dir)?;` then `trim_out(...)`: `Ok(out)` → `Ok(with_context(out, m))`; `Err(e)` with `m` `Some` → `error_line(&format!("{e:#}"))` and `Ok(with_context(HookOutput::None, m))`; `Err(e)` with no `m` → `Err(e)`. Doc line: `T§9.2 item 2: the flag first, then the trim of §10.1, O§7.8.`

- [ ] **Step 4: run green.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks --test reinject` → all pass (15 `trim…`, 6 `pre_compact…`; 3 ignored children). `env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks` → all pass; `git diff --stat` shows no change to any other test file.

- [ ] **Step 5: commit.** `git add crates/zygo-hooks/src/trim.rs crates/zygo-hooks/tests/reinject.rs`, subject `SX-4: hook trim gives the mechanics text once after a subagent's compaction`, blank line, `Zygo-Session: <implementing session>` and `Zygo-Work: phase3:SX-4`, each on its own line. No Claude attribution. Check: `git show --stat HEAD` lists only those 2 files.

- [ ] **Step 6: verify (node Acceptance).** `test "$(env -u ZYGO_SESSION_DIR cargo test -q -p zygo-hooks --test reinject -- trim --list 2>/dev/null | grep -c ': test$')" -ge 10 && env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks --test reinject -- trim` → exit 0; the list count is 16 (15 tests and `trim_child_case`), the run shows 15 passed, 1 ignored. Then `env -u ZYGO_SESSION_DIR cargo test -p zygo-hooks` → all pass. CI repeats it on ubuntu, macOS and Windows.
