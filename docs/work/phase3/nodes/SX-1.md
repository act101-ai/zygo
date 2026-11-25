# SX-1 — Claude Code subagent branch of `zygo hook session-start` (mechanics text on `SubagentStart`)

**Spec:** docs/specs/2026-11-24-session-text-design.md §6.3 item 2, §9.1, §14 item 1, §17 checks 2-6 and 12; T-C1, T-C5
**Files:** `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/subagent_text.rs` (new)
**Consumes:** —
**Produces:** `session_start::mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String`
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test subagent_text -- subagent_start --list 2>/dev/null | grep -c ': test$')" -ge 5 && test "$(cargo test -q -p zygo-hooks --test subagent_text -- main_compact --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-hooks --test subagent_text -- subagent_start main_compact` → every count holds (at least 5 named `subagent_start…` in zygo-hooks `subagent_text`; at least 1 named `main_compact…` in zygo-hooks `subagent_text`; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows, with at least one test each for T§17 checks 2, 3, 4, 5, 6 and 12, driven through `run_member` (the path `zygo hook session-start` runs) and `run_in`

## Plan

**Premises checked against the live code (branch `phase3/SX-1` at `33085c9`, equal to `origin/phase-3`):**

- The two early returns are `crates/zygo-hooks/src/session_start.rs:25-27` (`run_in`) and `:67-69` (`run_member`), as T§6.3 item 2 cites. `run` (`:18-21`) is the path of `zygo hook session-start` and calls `run_member`.
- The overrides are applied in the private `text_of` (`session_start.rs:33-39`), as T-C5 cites; `mechanics(v: &SessionView) -> String` is `pub` in `crates/zygo-hooks/src/session_text.rs:18-30`. No function today gives the mechanics text with the overrides applied (`grep -rn "mechanics(" crates/*/src`: only `session_text.rs:18,60` and `crates/zygo-launch/src/claude_code.rs:397`), so `mechanics_with_overrides` is new and reuses `mechanics` and the override lines of `text_of`.
- `HookInput` (`crates/zygo-core/src/hook.rs:9-18`) has `event` (= `hook_event_name` when it is a string), `has_agent_id` and `raw`; it has no `agent_type` field, so the branch reads `input.raw["agent_type"]`.
- `zygo_work::hooks::is_zygo` (`crates/zygo-work/src/hooks.rs:159-161`) is `pub(crate)`, so `zygo-hooks` cannot call it, and `crates/zygo-work/src/hooks.rs` is not in this node's Files. The branch repeats its rule (an `Option<&str>` that `starts_with("zygo-")`).
- The C6 wrapper `guarded` (`hook.rs:76-92`) writes its one line with `error_line` to the process stderr. The existing way to read that line in a `zygo-hooks` test is to run the test binary itself as a child with an env var and read the child's stderr (`crates/zygo-hooks/tests/workspace.rs:1875-1891` and `prompt_hook_child`); SX-1 uses it. The child is the test binary, not `claude`/`opencode`.
- `serde_json` has no `preserve_order` (root `Cargo.toml:60`, no feature), so `HookOutput::Json(..).to_stdout()` prints keys sorted; the tests compare the stdout with `json!(..).to_string()` of the same value, which is the exact byte string the hook prints.
- Fixtures: `fixtures/claude-code/hooks/SubagentStart-sub.json` (`agent_id` `a1a80dd100f197242`, `agent_type` `claude`, `hook_event_name` `SubagentStart`), `SessionStart-sub.json` (`agent_id`, `source` `compact`), `SessionStart-main.json` (`source` `startup`, no `agent_id`).
- `mechanics` of a view with results, navigation and units on is 721 bytes; the redaction sentence, one space and the attribution sentence is 317 bytes (computed from the constants of `session_text.rs:6-11`).
- The existing test `session_start_skips_subagent_input` (`crates/zygo-hooks/tests/session_text.rs:428-434`) uses `SessionStart-sub` (`source: "compact"`); it still holds after SX-1 (T§6.3 item 2, third case).
- CI runs `cargo test --workspace --locked --no-fail-fast` on ubuntu, macOS and Windows (`.github/workflows/ci.yml:45`). The new test file has no `cfg(unix)`: it uses no tmux, file mode or workspace state.
- Review focus assigned to SX-1: 67 → tests `subagent_start_other_types_get_the_same`, `subagent_start_zygo_type_gets_nothing`, `subagent_start_sub_session_start_compact_gets_nothing`, each through `run_in` and `run_member`; 68 → `main_compact_output_equals_startup`; 69 → `subagent_start_unparseable_view_one_stderr_line`; 75 → no test spawns `claude`/`opencode` or reads a clock (the only child process is the test binary itself; no `Instant`, `SystemTime` or `sleep` in the file).

### Interfaces

Consumes: —. (Uses the existing `zygo_hooks::session_text::mechanics(v: &SessionView) -> String`, `zygo_core::session_view::SessionView::load(dir: &SessionDir) -> anyhow::Result<SessionView>`, `zygo_core::overrides::Overrides::load(dir: &SessionDir) -> Overrides`, `zygo_core::hook::guarded(f: &dyn Fn(&[u8]) -> anyhow::Result<HookOutput>, stdin: &[u8]) -> (String, i32)`, `session_start::run_in(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput>` and `session_start::run_member(input: &HookInput, dir: &SessionDir, paths: &Paths, zygo: &Path, out: &mut dyn Write) -> anyhow::Result<HookOutput>`.)

Produces (`crates/zygo-hooks/src/session_start.rs`):

```rust
/// `mechanics(v)` with the `results` and `work_units` overrides of the session folder applied (T-C5).
pub fn mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String;
```

### Author's calls

- One private function `subagent(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput>` replaces both early returns (T§18 leaves the name and place to the builder).
- The `zygo-` and `hook_event_name` checks run before the view is loaded: a `zygo-` subagent or a subagent `SessionStart` prints nothing even when `view.json` does not load (no stderr line). Only the `SubagentStart` case that prints loads the view, and its load error goes to the C6 wrapper (T§14 item 1).
- The `SubagentStart` output is always the JSON envelope, whatever `v.agent` is: `SubagentStart` is a Claude Code event, and T§6.3 item 2 gives only the JSON form.
- The override lines of `text_of` move into one private `with_overrides(dir, v) -> SessionView`, used by `text_of` and `mechanics_with_overrides`, so the two texts cannot apply the overrides differently.
- The C6 child test is named `c6_child_unparseable_view`, so it adds to neither Acceptance count (its name holds neither `subagent_start` nor `main_compact`).

### Steps

- [x] **Step 1: write the tests.** Create `crates/zygo-hooks/tests/subagent_text.rs`. The constants `REDACT`, `ATTRIBUTION`, `STORE`, `NAV` and `TRAILER` are copied byte for byte from `crates/zygo-hooks/tests/session_text.rs:15-19` (the five sentences of `crates/zygo-hooks/src/session_text.rs:6-11`; the test pins them independently of the source).

  ```rust
  use std::path::Path;
  use std::process::{Command, Stdio};

  use serde_json::{json, Value};
  use zygo_core::{hook::{guarded, HookInput}, overrides::Overrides, paths::Paths,
      session_dir::SessionDir, session_view::SessionView, Agent};
  use zygo_hooks::{session_start, session_text::mechanics};

  // REDACT, ATTRIBUTION, STORE, NAV, TRAILER: tests/session_text.rs:15-19, byte for byte.
  const CHILD: &str = "ZYGO_SX1_C6_CHILD";

  fn fixture(name: &str) -> Value // reads ../../fixtures/claude-code/hooks/<name>.json from CARGO_MANIFEST_DIR
  fn with(v: Value, key: &str, val: Value) -> Value; fn without(v: Value, key: &str) -> Value // as tests/workspace.rs:130-138
  fn input(v: &Value) -> HookInput { HookInput::parse(Agent::ClaudeCode, &serde_json::to_vec(v).unwrap()).unwrap() }
  /// Results, navigation and units on; no workspace.
  fn view() -> SessionView { SessionView { session: "S1".into(), agent: Agent::ClaudeCode, main_role: "lead".into(),
      role_prompt: Some("ROLE".into()), results: true, navigation: true, work_units: true, compact_tool: true,
      trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() } }
  fn sd() -> (tempfile::TempDir, SessionDir) // tempdir, SessionDir::new(t.path().join("s")), create()
  fn paths(p: &Path) -> Paths // as tests/workspace.rs:45-49
  /// The stdout of `run_in` and of `run_member` (no `--event member`, `zygo` = a path that does not exist); asserts
  /// that both are equal and that `run_member` wrote nothing to `out`.
  fn both(t: &Path, d: &SessionDir, i: &HookInput) -> String {
      let a = session_start::run_in(i, d).unwrap().to_stdout();
      let mut out = Vec::new();
      let b = session_start::run_member(i, d, &paths(t), &t.join("no-zygo"), &mut out).unwrap().to_stdout();
      assert!(out.is_empty());
      assert_eq!(a, b);
      a
  }
  fn envelope(m: &str) -> String {
      json!({"hookSpecificOutput": {"hookEventName": "SubagentStart", "additionalContext": m}}).to_string()
  }
  ```

  Tests (each `#[test]`, comment `// T§17 check n`):

  ```rust
  fn subagent_start_claude_type_gets_mechanics() { // check 2
      let (t, d) = sd(); let v = view(); v.save(&d).unwrap();
      let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
      assert_eq!(m.len(), 721);
      assert_eq!(mechanics(&v), m);
      assert_eq!(session_start::mechanics_with_overrides(&d, &v), m);
      assert_eq!(fixture("SubagentStart-sub")["agent_type"], json!("claude"));
      assert_eq!(both(t.path(), &d, &input(&fixture("SubagentStart-sub"))), envelope(&m));
  }

  fn subagent_start_other_types_get_the_same() { // check 3, review focus 67
      let (t, d) = sd(); view().save(&d).unwrap();
      let want = envelope(&[REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" "));
      let sub = fixture("SubagentStart-sub");
      for ty in [json!("Explore"), json!("Plan"), json!("general-purpose"), json!("p315:probe"), json!(1)] {
          assert_eq!(both(t.path(), &d, &input(&with(sub.clone(), "agent_type", ty.clone()))), want, "{ty}");
      }
      assert_eq!(both(t.path(), &d, &input(&without(sub, "agent_type"))), want, "no agent_type");
  }

  fn subagent_start_zygo_type_gets_nothing() { // check 3, review focus 67
      let (t, d) = sd(); view().save(&d).unwrap();
      assert_eq!(both(t.path(), &d, &input(&with(fixture("SubagentStart-sub"), "agent_type", json!("zygo-code")))), "");
  }

  fn subagent_start_overrides_off_gives_redaction_and_attribution() { // check 4
      let (t, d) = sd();
      let v = SessionView { navigation: false, ..view() }; v.save(&d).unwrap();
      Overrides { results: Some(false), work_units: Some(false), ..Default::default() }.save(&d).unwrap();
      let m = format!("{REDACT} {ATTRIBUTION}");
      assert_eq!(m.len(), 317);
      assert_eq!(session_start::mechanics_with_overrides(&d, &v), m);
      assert_eq!(both(t.path(), &d, &input(&fixture("SubagentStart-sub"))), envelope(&m));
  }

  fn subagent_start_sub_session_start_compact_gets_nothing() { // check 5, review focus 67
      let (t, d) = sd(); view().save(&d).unwrap();
      let sub = fixture("SessionStart-sub");
      assert_eq!((sub["source"].as_str(), sub.get("agent_id").is_some()), (Some("compact"), true));
      let before = std::fs::read(d.view_json()).unwrap();
      assert_eq!(both(t.path(), &d, &input(&sub)), "");
      assert_eq!(std::fs::read(d.view_json()).unwrap(), before); // the member part did not run
      assert!(!d.log().exists());
  }

  fn subagent_start_unparseable_view_one_stderr_line() { // check 12, T§14 item 1, review focus 69
      let (t, d) = sd();
      std::fs::write(d.view_json(), "{").unwrap();
      let i = input(&fixture("SubagentStart-sub"));
      let mut out = Vec::new();
      assert!(session_start::run_member(&i, &d, &paths(t.path()), &t.path().join("no-zygo"), &mut out).is_err());
      assert!(session_start::run_in(&i, &d).is_err());
      assert!(out.is_empty());
      // `guarded` writes to the process stderr: run the child test in a child process and read its stderr.
      let o = Command::new(std::env::current_exe().unwrap())
          .args(["--ignored", "--exact", "c6_child_unparseable_view", "--nocapture", "--test-threads=1"])
          .env(CHILD, t.path()).stdin(Stdio::null()).output().unwrap();
      assert!(o.status.success(), "{o:?}");
      assert!(String::from_utf8(o.stdout).unwrap().contains("c6_child_unparseable_view ... ok"));
      let err = String::from_utf8(o.stderr).unwrap();
      assert_eq!(err.lines().count(), 1, "{err:?}");
      assert!(err.starts_with("zygo hook: ") && err.ends_with('\n'), "{err:?}");
  }

  #[ignore] // child half of subagent_start_unparseable_view_one_stderr_line; returns at once without CHILD
  fn c6_child_unparseable_view() {
      let Some(root) = std::env::var_os(CHILD) else { return };
      let t = Path::new(&root);
      let d = SessionDir::new(t.join("s"));
      let stdin = serde_json::to_vec(&fixture("SubagentStart-sub")).unwrap();
      let mut out = Vec::new();
      let got = guarded(&|b| session_start::run_member(&HookInput::parse(Agent::ClaudeCode, b)?, &d, &paths(t),
          &t.join("no-zygo"), &mut out), &stdin);
      assert_eq!(got, (String::new(), 0));
      assert!(out.is_empty());
  }

  fn main_compact_output_equals_startup() { // check 6, T§9.1, review focus 68
      let (t, d) = sd(); view().save(&d).unwrap();
      let startup = fixture("SessionStart-main");
      assert_eq!(startup["source"], json!("startup"));
      // Constructed input: SessionStart-main.json with `source` changed to "compact".
      let compact = with(startup.clone(), "source", json!("compact"));
      let a = both(t.path(), &d, &input(&startup));
      assert!(!a.is_empty());
      assert_eq!(both(t.path(), &d, &input(&compact)), a);
      let got: Value = serde_json::from_str(&a).unwrap();
      assert_eq!(got["hookSpecificOutput"]["hookEventName"], json!("SessionStart"));
  }
  ```

  Check: the file holds 6 test names that start `subagent_start` and 1 that starts `main_compact`; `grep -nE "Instant|SystemTime|sleep|Command::new" crates/zygo-hooks/tests/subagent_text.rs` shows only the one `Command::new(std::env::current_exe()…)`.

- [x] **Step 2: run it red.** `cargo test -p zygo-hooks --test subagent_text` → does not compile: `cannot find function \`mechanics_with_overrides\` in module \`session_start\`` (E0425).

- [x] **Step 3: add `mechanics_with_overrides`.** In `crates/zygo-hooks/src/session_start.rs`: import `use crate::session_text::{build, mechanics};`; add a private `fn with_overrides(dir: &SessionDir, v: &SessionView) -> SessionView` that holds the lines of `:34-37` (clone, `Overrides::load(dir)`, `results`, `work_units`) and returns the copy; `text_of` becomes `build(&with_overrides(dir, v))`; add `pub fn mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String { mechanics(&with_overrides(dir, v)) }` with the doc line of Interfaces. Check: `cargo test -p zygo-hooks --test subagent_text` compiles; `subagent_start_claude_type_gets_mechanics`, `subagent_start_other_types_get_the_same` and `subagent_start_overrides_off_gives_redaction_and_attribution` fail on the `both(..)` assertion (left `""`), `subagent_start_unparseable_view_one_stderr_line` fails on the first `is_err()`; `subagent_start_zygo_type_gets_nothing`, `subagent_start_sub_session_start_compact_gets_nothing` and `main_compact_output_equals_startup` pass (they pin behavior that does not change).

- [x] **Step 4: replace both early returns.** In `session_start.rs` add `use serde_json::Value;` (next to `json`) and:

  ```rust
  /// T§6.3 item 2: the output for an input with `agent_id`. A `zygo-` agent type (the rule of
  /// `zygo_work::hooks::is_zygo`) gets nothing: its `--agents` prompt holds the mechanics text (§8.5). Any other
  /// subagent gets the mechanics text on `SubagentStart`; a missing or non-string `agent_type` is not `zygo-`. Any
  /// other event (a subagent `SessionStart` with `source: "compact"`) gets nothing; T§9.2 owns that copy.
  fn subagent(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput> {
      let zygo = input.raw.get("agent_type").and_then(Value::as_str).is_some_and(|t| t.starts_with("zygo-"));
      if zygo || input.event.as_deref() != Some("SubagentStart") {
          return Ok(HookOutput::None);
      }
      let v = SessionView::load(dir)?;
      Ok(HookOutput::Json(json!({"hookSpecificOutput": {"hookEventName": "SubagentStart",
          "additionalContext": mechanics_with_overrides(dir, &v)}})))
  }
  ```

  In `run_in` (`:25-27`) and `run_member` (`:67-69`) replace `return Ok(HookOutput::None);` by `return subagent(input, dir);`, and replace the `run_in` comment at `:24` by `// T§6.3 item 2: one branch for every input with \`agent_id\`; the member part never runs for it.` Check: `cargo test -p zygo-hooks --test subagent_text` → 7 passed, 0 failed, 1 ignored.

- [x] **Step 5: run the crate's tests.** `cargo test -p zygo-hooks --locked --no-fail-fast` → all pass (among them `session_start_skips_subagent_input` and the `run_member` tests of `tests/workspace.rs`); `cargo build --workspace --locked` → no warning from `crates/zygo-hooks/src/session_start.rs`.

- [x] **Step 6: commit.** `git add crates/zygo-hooks/src/session_start.rs crates/zygo-hooks/tests/subagent_text.rs && git commit` with subject `SX-1: mechanics text on SubagentStart for every non-zygo subagent`, a blank line, then the trailers `Zygo-Session: <id of the implementing session>` and `Zygo-Work: phase3:SX-1`, each on its own line; no Claude attribution. Check: `git show --stat HEAD` lists only those two files.

- [x] **Step 7: verify (node Acceptance).** `test "$(cargo test -q -p zygo-hooks --test subagent_text -- subagent_start --list 2>/dev/null | grep -c ': test$')" -ge 5 && test "$(cargo test -q -p zygo-hooks --test subagent_text -- main_compact --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-hooks --test subagent_text -- subagent_start main_compact` → exit 0: the counts are 6 and 1, and the run shows 7 passed, 0 failed (CI repeats it on ubuntu, macOS and Windows through `cargo test --workspace`).
