# SX-2 — OpenCode `session-start --event subagent` branch (mechanics text as plain text, no member part)

**Spec:** docs/specs/2026-11-24-session-text-design.md §7.2 item 2, §17 check 11; T-C5
**Files:** `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/subagent_text.rs`
**Consumes:** `mechanics_with_overrides` (SX-1)
**Produces:** `zygo hook session-start --agent opencode --event subagent`
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test subagent_text -- opencode_subagent --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-hooks --test subagent_text -- opencode_subagent` → every count holds (at least 1 named `opencode_subagent…` in zygo-hooks `subagent_text`; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows, including T§17 check 11: in a member session view that holds resume-note inputs the output is M as plain text and `view.json` is byte-equal before and after

## Plan

### Premises (checked against `origin/phase-3` at `b36b0eb`)

- SX-1 landed (PR #39). `crates/zygo-hooks/src/session_start.rs:52` defines `pub fn mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String`: `mechanics(v)` with the `results` and `work_units` overrides of `overrides.json` applied (T-C5).
- `zygo hook <name> --agent <a> --event <e>` copies `--event` into `HookInput::cli_event` for every hook (`crates/zygo/src/cmd/hook.rs:118-125`); the plugin calls `zygo hook session-start --agent opencode [extra…]` with `{input, output, cwd}` on stdin (`crates/zygo-launch/assets/opencode-plugin.js:23,29`). No CLI change is needed.
- Today an OpenCode input with `cli_event == Some("subagent")` takes the ordinary path of `run_member` (`session_start.rs:88-133`): the full session text, and in a member view also the resume note (`crates/zygo-hooks/src/workspace.rs:617-644`; a Claude Code member view also writes `view.json` and the member record there, `:637-640`). So the new tests fail before the change.
- The member fixture of SX-1 (`member_fx`, `crates/zygo-hooks/tests/subagent_text.rs:228-255`) holds resume-note inputs and a pending member record. It passes on windows-latest (PR #39 CI), so the new tests need no `cfg(unix)`.
- `crates/zygo-hooks/src/session_text.rs:33` `pub fn build(v: &SessionView) -> String` and `crates/zygo-hooks/src/workspace.rs:542` `pub fn note(n: &ResumeNote) -> Option<String>` are public; the contrast assertion uses them.
- No drift from T§7.2 item 2 or T§17 check 11.

### Interfaces

- Consumes (SX-1, as in the code now): `pub fn mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String` (`crates/zygo-hooks/src/session_start.rs:52`).
- Produces: `zygo hook session-start --agent opencode --event subagent`: stdout = `mechanics_with_overrides(dir, &SessionView::load(dir)?)` as plain text (`HookOutput::Text`), with no member part, no resume note and no write. A `view.json` load error is the hook's error (the C6 wrapper prints nothing and writes one stderr line, T§14 item 1). SX-6 consumes this command from the plugin (`["--event", "subagent"]`).
- Library surface (unchanged signatures): `pub fn run_in(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput>`; `pub fn run_member(input: &HookInput, dir: &SessionDir, paths: &Paths, zygo: &Path, out: &mut dyn Write) -> anyhow::Result<HookOutput>`. Both take the new branch.

### Steps

All test code goes in `crates/zygo-hooks/tests/subagent_text.rs`. Every new test name starts with `opencode_subagent`. No test spawns a process or reads a clock (review focus 75).

- [x] **1. Make the member fixture take the agent.** Rename `fn member_fx(t: &Path) -> (SessionDir, Paths)` to `fn member_fx_as(t: &Path, agent: Agent) -> (SessionDir, Paths)`. It sets `agent` in the saved `SessionView` (`SessionView { session: SID.into(), workspace: Some(ws), agent, ..view() }`) and uses `MemberRecord::new(agent)`. Add `fn member_fx(t: &Path) -> (SessionDir, Paths) { member_fx_as(t, Agent::ClaudeCode) }` so the SX-1 tests do not change. Check: `cargo test -p zygo-hooks --test subagent_text` passes, with the same count as before.

- [x] **2. Add the OpenCode helpers.**
  ```rust
  /// The recorded transform payload (`fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json`) as an
  /// OpenCode input with `--event subagent`. Constructed: `input.modelID` changed from `lead` to `read`, a request
  /// that the plugin sends with `--event subagent` (T§7.2 item 1). The hook does not read the model.
  fn oc_subagent() -> HookInput {
      let p = Path::new(env!("CARGO_MANIFEST_DIR"))
          .join("../../fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json");
      let mut v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
      v["input"]["modelID"] = json!("read");
      HookInput { cli_event: Some("subagent".into()), ..HookInput::parse(Agent::OpenCode, &serde_json::to_vec(&v).unwrap()).unwrap() }
  }

  /// `run_member` (`zygo` = a path that does not exist) and `run_in` on `i`. Asserts that both return
  /// `HookOutput::Text` with equal text and that `run_member` wrote nothing to `out`. Returns the text.
  fn oc_text(t: &Path, d: &SessionDir, p: &Paths, i: &HookInput) -> String {
      let mut out = Vec::new();
      let a = session_start::run_member(i, d, p, &t.join("no-zygo"), &mut out).unwrap();
      assert!(out.is_empty(), "the member part printed");
      let b = session_start::run_in(i, d).unwrap();
      let (HookOutput::Text(a), HookOutput::Text(b)) = (a, b) else { panic!("not plain text") };
      assert_eq!(a, b);
      a
  }
  ```
  Add `HookOutput` to the `zygo_core::hook` import, and import `zygo_hooks::session_text::build` and `zygo_hooks::workspace::note`.

- [x] **3. Write the failing tests.**
  ```rust
  #[test]
  fn opencode_subagent_prints_mechanics_as_plain_text() {
      // T§7.2 item 2, T-C5: a view that is not a member; plain text for either view agent.
      for agent in [Agent::OpenCode, Agent::ClaudeCode] {
          let (t, d) = sd();
          SessionView { agent, ..view() }.save(&d).unwrap();
          let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
          assert_eq!(m.len(), 721);
          assert_eq!(oc_text(t.path(), &d, &paths(t.path()), &oc_subagent()), m, "{agent:?}");
          assert!(!d.log().exists());
      }
  }

  #[test]
  fn opencode_subagent_overrides_apply() {
      // T-C5, T§17 check 4 form: overrides results and work_units off, navigation off.
      let (t, d) = sd();
      SessionView { agent: Agent::OpenCode, navigation: false, ..view() }.save(&d).unwrap();
      Overrides { results: Some(false), work_units: Some(false), ..Default::default() }.save(&d).unwrap();
      let m = format!("{REDACT} {ATTRIBUTION}");
      assert_eq!(m.len(), 317);
      assert_eq!(oc_text(t.path(), &d, &paths(t.path()), &oc_subagent()), m);
  }

  #[test]
  fn opencode_subagent_member_view_prints_only_mechanics() {
      // T§17 check 11, review focus 70: a member view that holds resume-note inputs gives M as plain text, and no
      // file under the root changes (view.json, the workspace state, the session log, errors.log).
      let m = [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" ");
      for agent in [Agent::OpenCode, Agent::ClaudeCode] {
          let t = tempfile::tempdir().unwrap();
          let (d, p) = member_fx_as(t.path(), agent);
          let view = std::fs::read(d.view_json()).unwrap();
          let before = tree(t.path());
          assert_eq!(oc_text(t.path(), &d, &p, &oc_subagent()), m, "{agent:?}");
          assert_eq!(std::fs::read(d.view_json()).unwrap(), view, "{agent:?}: view.json changed");
          assert_eq!(tree(t.path()), before, "{agent:?}: a file changed");
          assert!(!d.log().exists());
      }
      // The same OpenCode input without `--event subagent` reaches the member part: session text, blank line, note.
      let t = tempfile::tempdir().unwrap();
      let (d, p) = member_fx_as(t.path(), Agent::OpenCode);
      let v = SessionView::load(&d).unwrap();
      let n = note(v.workspace.as_ref().unwrap().resume_note.as_ref().unwrap()).unwrap();
      let i = HookInput { cli_event: None, ..oc_subagent() };
      let got = session_start::run_member(&i, &d, &p, &t.path().join("no-zygo"), &mut Vec::new()).unwrap().to_stdout();
      assert_eq!(got, format!("{}\n\n{n}", build(&v)));
  }

  #[test]
  fn opencode_subagent_unparseable_view_is_an_error() {
      // T§14 item 1: the load error goes to the C6 wrapper; nothing printed to `out`, no session-log line.
      let (t, d) = sd();
      std::fs::write(d.view_json(), "{").unwrap();
      let i = oc_subagent();
      let mut out = Vec::new();
      assert!(session_start::run_member(&i, &d, &paths(t.path()), &t.path().join("no-zygo"), &mut out).is_err());
      assert!(session_start::run_in(&i, &d).is_err());
      assert!(out.is_empty());
      assert!(!d.log().exists());
  }
  ```

- [x] **4. Run and see them fail.** `cargo test -p zygo-hooks --test subagent_text -- opencode_subagent` → `opencode_subagent_prints_mechanics_as_plain_text`, `opencode_subagent_overrides_apply` and `opencode_subagent_member_view_prints_only_mechanics` fail (today the output is the full session text, a JSON envelope for a Claude Code view, and for an OpenCode member also the note). `opencode_subagent_unparseable_view_is_an_error` passes already: it pins today's load-error path for the new branch.

- [x] **5. Implement the branch** in `crates/zygo-hooks/src/session_start.rs`.
  - Add:
    ```rust
    /// T§7.2 item 2: `session-start --event subagent` (the OpenCode transform for a request on a `zygo/` model that is
    /// not the main role): the mechanics text with the overrides applied (T-C5), as plain text for every view agent;
    /// no member part, no resume note, no write. A load error is the caller's error (T§14 item 1).
    fn subagent_text(dir: &SessionDir) -> anyhow::Result<HookOutput>
    ```
    It returns `Ok(HookOutput::Text(mechanics_with_overrides(dir, &SessionView::load(dir)?)))`.
  - In `run_in`, right after the `if input.has_agent_id { … }` block, and in `run_member`, right after its `if input.has_agent_id { … }` block and before `let member = …`, add:
    ```rust
    if input.cli_event.as_deref() == Some("subagent") {
        return subagent_text(dir);
    }
    ```
  - In the doc comment of `run_member`, after the sentence about `session-start --event member`, add: "`--event subagent` (`input.cli_event == Some(\"subagent\")`, the OpenCode transform for another role, T§7.2) gives `subagent_text` before any member part." Extend the module doc (lines 1-2) with: "for `--event subagent` the mechanics text only (session text design T§7.2 item 2)."

- [x] **6. Run and see them pass.** `cargo test -p zygo-hooks --test subagent_text -- opencode_subagent` → 4 passed, 0 failed. Then `cargo test -p zygo-hooks` → all pass (the SX-1 tests in `subagent_text.rs`, `session_text.rs` and `workspace.rs` do not change).

- [x] **7. Run the Acceptance line** of this node as written → the count is 4 (at least 1), then all pass. Run `cargo clippy -p zygo-hooks --tests -- -D warnings` → no warnings.

- [x] **8. Commit** `crates/zygo-hooks/src/session_start.rs` and `crates/zygo-hooks/tests/subagent_text.rs` with message `SX-2: OpenCode session-start --event subagent` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:SX-2`.

### Spec coverage

| Requirement | Step |
|---|---|
| T§7.2 item 2: mechanics text (T-C5) as plain text | 3 (`…prints_mechanics_as_plain_text`, `…overrides_apply`), 5 |
| T§7.2 item 2: returns before any member part, no resume note, no view write | 3 (`…member_view_prints_only_mechanics`), 5 |
| T§17 check 11 / review focus 70: member view with resume-note inputs, M as plain text, `view.json` byte-equal | 3 (`…member_view_prints_only_mechanics`) |
| T-C1, T§14 item 1: load error goes to the C6 wrapper | 3 (`…unparseable_view_is_an_error`); the wrapper's one stderr line is pinned by SX-1 `subagent_start_unparseable_view_one_stderr_line` |
| T-C3, review focus 75: no real agent, no clock | all tests are in-process; no `Command`, no clock |
