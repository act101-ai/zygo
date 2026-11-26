# SX-5 — Claude Code launch registers `SubagentStart` → `hook session-start` on every launch

**Spec:** docs/specs/2026-11-24-session-text-design.md §6.3 items 1 and 3, §8.4, §17 checks 1 and 9; T-C2
**Files:** `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`
**Consumes:** —
**Produces:** settings key `hooks.SubagentStart`
**Acceptance:** `test "$(cargo test -q -p zygo-launch --test claude_code -- subagent_start --list 2>/dev/null | grep -c ': test$')" -ge 2 && test "$(cargo test -q -p zygo-launch --test claude_code -- zygo_role_prompt --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-launch --test claude_code -- subagent_start zygo_role_prompt` → every count holds (at least 2 named `subagent_start…` in zygo-launch `claude_code`; at least 1 named `zygo_role_prompt…` in zygo-launch `claude_code`; a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows: T§17 check 1 (plain launch: one entry, command `<zygo> hook session-start`, no matcher; member launch: two entries in the order `session-start`, `workspace --event subagent-start`) and check 9 (each `zygo-<role>` prompt of a default launch equals the role prompt, one space, M); then `cargo test -p zygo-launch --test claude_code` → all pass

## Plan

### What the code does today (read 2026-11-25, after OS-33 merged into `phase-3`)

- `crates/zygo-launch/src/claude_code.rs:462` `fn hooks(&self, ctx: &LaunchCtx) -> Value` (in `impl ClaudeCode`, line 442) builds the `hooks` settings value. Its closure `entry(matcher: Option<&str>, name: &str)` (lines 464-470) gives `{"hooks":[{"type":"command","command":"<zygo> hook <name>"}]}`, with a `matcher` key only for `Some`.
- `SubagentStart` is set only for a member: line 497, inside `if member` (line 492), to `[entry(None, "workspace --event subagent-start")]`. A launch that is not a member has no `SubagentStart` key.
- The `UserPromptSubmit` list (lines 517-521) is the pattern this node copies: a base entry for every launch, then a member-only entry.
- `PostToolUseFailure` (lines 480-503) has no `trim` entry: OS-33 took the O§7.13 "If PO1 fails" branch (`crates/zygo-hooks/src/trim.rs` `PO1_HOLDS = false`). This node does not touch it.
- The hook side of T§6.3 item 2 is already in place: `crates/zygo-hooks/src/session_start.rs:24-27` sends every input with `agent_id` to `fn subagent(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput>` (line 40). This node changes no `zygo-hooks` code.
- The `zygo-<role>` prompt: `crates/zygo-launch/src/claude_code.rs:395` `fn agents(ctx: &LaunchCtx, user: Map<String, Value>) -> Value`; `mechanics(&ctx.view)` at line 397; `format!("{prompt} {mech}")` at line 406; user keys merged last at line 418. T§6.3 item 3 and T§8.4: no change.

### Consumes

— (no node contract). Existing code used as is:

- `crates/zygo-launch/src/claude_code.rs:462`: `fn hooks(&self, ctx: &LaunchCtx) -> Value` (private; signature unchanged).
- `crates/zygo-hooks/src/session_text.rs:18`: `pub fn mechanics(v: &SessionView) -> String` (test oracle for T§17 check 9; already imported in the test file at line 16).
- Test helpers in `crates/zygo-launch/tests/claude_code.rs`: `fn fx() -> Fx` (36), `fn ctx(fx: &Fx, cfg_text: &str) -> LaunchCtx` (71), `fn cc(fx: &Fx) -> ClaudeCode` (190), `fn settings(fx: &Fx) -> Value` (236), `fn agents_of(args: &[String]) -> Value` (244), `fn hook_commands(s: &Value, event: &str) -> Vec<(Option<String>, String)>` (220), `fn member_ctx(fx: &Fx, work_units: bool) -> LaunchCtx` (1660), `shell_quote` (imported, line 17).

### Produces

- Settings key `hooks.SubagentStart` in `<session dir>/settings.json` of every Claude Code launch:
  - not a member: `[{"hooks":[{"type":"command","command":"<zygo> hook session-start"}]}]`
  - member: `[{"hooks":[{"type":"command","command":"<zygo> hook session-start"}]}, {"hooks":[{"type":"command","command":"<zygo> hook workspace --event subagent-start"}]}]`
  - No `matcher` key on either entry; no `--event` flag on the `session-start` entry (the hook reads `hook_event_name`). `<zygo>` is `shell_quote(&self.zygo)`.

### Added files (outside the Files list)

Adding a key to every launch's settings breaks three exact-equality checks of the whole `hooks` value. Each must gain the plain-launch `SubagentStart` entry, or `cargo test -p zygo-launch` fails:

- `crates/zygo-launch/tests/fixtures/claude_code/settings.json` (compared at `tests/claude_code.rs:295`, `settings_env_agents_and_mcp_match_fixtures`).
- `crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json` (compared at `tests/claude_code.rs:392`, `passthrough_drops_model_rows_and_adds_the_token_header`).
- `crates/zygo-launch/tests/stub_launch.rs:446-455` (`claude_code_flags_settings_agents_and_mcp_match_the_spec`, `assert_eq!(s["hooks"], json!({...}))`).

### Tests: which fail first

| Test | File | Fails first? | Why |
|---|---|---|---|
| `subagent_start_plain_launch_is_one_session_start_entry` | `tests/claude_code.rs` | yes | no `SubagentStart` key today (line 497 is member-only) |
| `subagent_start_member_launch_is_session_start_then_workspace` | `tests/claude_code.rs` | yes | the member list has one entry today |
| `zygo_role_prompt_is_role_prompt_space_mechanics` | `tests/claude_code.rs` | no (guard) | T§17 check 9 holds today (line 406); the test keeps it after the change (T§6.3 item 3) |
| `member_settings_hold_every_workspace_hook` (edited, line 1734) | `tests/claude_code.rs` | yes | expects the two-entry list |
| `non_member_settings_hold_no_workspace_hook` via `no_workspace_hook` (edited, line 1697) | `tests/claude_code.rs` | no | no longer asserts that `SubagentStart` is absent; lines 1694-1696 still assert no `hook workspace` command under any event |
| `settings_env_agents_and_mcp_match_fixtures`, `passthrough_drops_model_rows_and_adds_the_token_header` | `tests/claude_code.rs` | yes | fixtures gain the entry |
| `claude_code_flags_settings_agents_and_mcp_match_the_spec` | `tests/stub_launch.rs` | yes | expected `hooks` gains the entry |

No test sets an env var, opens the keychain or reads a clock. All tests are platform-neutral (no `cfg`), so they run on ubuntu, macOS and Windows (`shell_quote` gives the platform's quoting for `<zygo>`).

## Steps

**Cycle 1: tests**

- [x] 1. In `crates/zygo-launch/tests/claude_code.rs`, after `non_member_settings_hold_no_workspace_hook` (ends line 1754), add:

  ```rust
  #[test] // T§17 check 1, T§6.3 item 1: every launch that is not a member; review focus 72
  fn subagent_start_plain_launch_is_one_session_start_entry() {
      for (units, passthrough) in [(true, false), (false, false), (true, true), (false, true)] {
          let fx = fx();
          let cc = cc(&fx);
          let mut c = ctx(&fx, "");
          c.view.work_units = units;
          c.passthrough = passthrough;
          assert!(c.view.workspace.is_none());
          cc.build(&c).unwrap();
          let s = settings(&fx);
          let cmd = format!("{} hook session-start", shell_quote(&cc.zygo));
          assert_eq!(s["hooks"]["SubagentStart"], json!([{"hooks": [{"type": "command", "command": cmd}]}]), "units {units} passthrough {passthrough}");
      }
  }

  #[test] // T§17 check 1, T§6.3 item 1: member order `session-start`, then `workspace --event subagent-start`; no matcher, no `--event` on the first; review focus 72
  fn subagent_start_member_launch_is_session_start_then_workspace() {
      for work_units in [true, false] {
          let fx = fx();
          let cc = cc(&fx);
          cc.build(&member_ctx(&fx, work_units)).unwrap();
          let s = settings(&fx);
          let z = shell_quote(&cc.zygo);
          assert_eq!(
              s["hooks"]["SubagentStart"],
              json!([
                  {"hooks": [{"type": "command", "command": format!("{z} hook session-start")}]},
                  {"hooks": [{"type": "command", "command": format!("{z} hook workspace --event subagent-start")}]}
              ]),
              "{work_units}"
          );
      }
  }

  #[test] // T§17 check 9, T§6.3 item 3, T§8.4: the launch-time prompt is unchanged
  fn zygo_role_prompt_is_role_prompt_space_mechanics() {
      let fx = fx();
      let c = ctx(&fx, "");
      let spec = cc(&fx).build(&c).unwrap();
      let m = mechanics(&c.view);
      assert!(!m.is_empty());
      let a = agents_of(&spec.args);
      let names: BTreeSet<&str> = a.as_object().unwrap().keys().map(String::as_str).collect();
      assert_eq!(names, BTreeSet::from(["zygo-code", "zygo-design", "zygo-probe", "zygo-read"]));
      for (name, p) in [("zygo-code", "P-code"), ("zygo-design", "P-design"), ("zygo-probe", "P-probe"), ("zygo-read", "P-read")] {
          assert_eq!(a[name]["prompt"], json!(format!("{p} {m}")), "{name}");
      }
  }
  ```

- [x] 2. Edit the existing tests in `crates/zygo-launch/tests/claude_code.rs`:
  - line 1697 (`no_workspace_hook`): the list becomes `["Stop", "StopFailure", "SessionEnd"]`.
  - line 1734 (`member_settings_hold_every_workspace_hook`): `assert_eq!(hook_commands(&s, "SubagentStart"), [(None, format!("{z} hook session-start")), (None, ws("subagent-start"))], "{work_units}");`

- [x] 3. Add the plain-launch entry to the exact `hooks` expectations:
  - `crates/zygo-launch/tests/fixtures/claude_code/settings.json`: after the `"SubagentStop"` line, add `  "SubagentStart":[{"hooks":[{"type":"command","command":"<ZYGO> hook session-start"}]}],`
  - `crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json`: after the `"SubagentStop"` array, add a `"SubagentStart"` array with one object `{"hooks": [{"type": "command", "command": "<ZYGO> hook session-start"}]}`, in the file's indented style.
  - `crates/zygo-launch/tests/stub_launch.rs:453`: after `"SubagentStop": [h(None, "work")],` add `"SubagentStart": [h(None, "session-start")],`.

- [x] 4. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --test claude_code -- subagent_start zygo_role_prompt member_settings_hold_every_workspace_hook settings_env_agents_and_mcp_match_fixtures passthrough_drops_model_rows` → pass: it compiles; `zygo_role_prompt_is_role_prompt_space_mechanics` passes; the two `subagent_start_*` tests, `member_settings_hold_every_workspace_hook`, `settings_env_agents_and_mcp_match_fixtures` and `passthrough_drops_model_rows_and_adds_the_token_header` fail on the `SubagentStart` value (plain launch: `Null` against the one-entry array; member: one entry against two).

**Cycle 2: the registration**

- [x] 5. In `crates/zygo-launch/src/claude_code.rs`, `fn hooks(&self, ctx: &LaunchCtx) -> Value` (line 462; signature unchanged): delete line 497 from the `if member` block, and insert `SubagentStart` for every launch as a list, in the form of the `prompt` list (lines 517-521): first `entry(None, "session-start")`, then, when `member`, `entry(None, "workspace --event subagent-start")`. Comment it `// T§6.3 item 1: every launch; a member's workspace entry comes second.` No matcher, no `--event` on the `session-start` entry. No other entry changes (`PostToolUseFailure` keeps no `trim`, review focus 24).

- [x] 6. Run the node's acceptance check: `env -u ZYGO_SESSION_DIR sh -c 'test "$(cargo test -q -p zygo-launch --test claude_code -- subagent_start --list 2>/dev/null | grep -c ": test$")" -ge 2 && test "$(cargo test -q -p zygo-launch --test claude_code -- zygo_role_prompt --list 2>/dev/null | grep -c ": test$")" -ge 1 && cargo test -p zygo-launch --test claude_code -- subagent_start zygo_role_prompt'` → exit 0; counts 2 and 1; 3 tests pass.

- [x] 7. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --test claude_code` → all pass (includes `po1_failed_bash_has_no_trim_entry` for review focus 24, `user_agents_merge_and_win_by_name` (line 606) for T-C2, `non_member_settings_hold_no_workspace_hook`, `member_settings_hold_every_workspace_hook`, both fixture tests).

- [x] 8. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-launch` → all pass (includes `stub_launch` `claude_code_flags_settings_agents_and_mcp_match_the_spec` and `premise`, which checks each settings event against `fixtures/claude-code/settings-schema.json`; `SubagentStart` is in it, `tests/claude_code.rs:1705-1707`). tmux-dependent tests skip with a printed reason when tmux 3.2 is absent.

- [x] 9. Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --test workspace_down` → all pass or skip with the printed tmux reason (member stub runs now call `hook session-start` before `hook workspace --event subagent-start` on `SubagentStart`; activity state is written only by the workspace entry).
  Ran 0 tests: every workspace_down test is #[ignore] (timing); the member SubagentStart order is checked only in the settings by `subagent_start_member_launch_is_session_start_then_workspace`.

- [x] 10. Commit the six files (`crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs`, `crates/zygo-launch/tests/stub_launch.rs`, the two fixtures, this node file with steps ticked): `git commit -m "SX-5: register SubagentStart -> hook session-start on every launch"` with one more `-m` that holds the two trailer lines the implementer's handoff gives: `Zygo-Session:` with that session's id, and `Zygo-Work: phase3:SX-5`. No other trailer, no Claude attribution.
