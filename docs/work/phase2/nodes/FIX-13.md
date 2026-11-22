# FIX-13 — Stub agent reads the one-token `=` session arguments

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` AU3 as amended 2026-11-22 (L2 round 2: Claude Code `--session-id=<id>` and `--resume=<id>`, OpenCode `-s <id>`); found by L2 round 2. Defect: the stub agent's `session_arg` (`crates/zygo-launch/tests/stub_agent/stub.rs:63`) finds only the split forms `--session-id <id>`, `--resume <id>` and `-s <id>`, so a stub launch of a Claude Code member with `LaunchOpts.agent_session` sees no session argument: `session-arg.json` is not written and the `SessionStart` input has `source` `startup` for a resume. Fix: `session_arg` also reads `--session-id=<id>` and `--resume=<id>` as one token, with the flag name `--session-id` or `--resume` and the value after the first `=`; the split forms and `-s <id>` keep working; the first session argument in `argv` that has a value wins, as today. M2's stub tests need it
**Files:** `crates/zygo-launch/tests/stub_agent/stub.rs`, `crates/zygo-launch/tests/stub_launch.rs`
**Consumes:** `AgentAdapter::preassign_args` (L2), `AgentAdapter::resume_args` (L2), `LaunchOpts.agent_session` (L2)
**Produces:** stub agent reads = session forms
**Acceptance:** `cargo test -p zygo-launch --locked --test stub_launch --test stub_member` → all pass, including a stub launch with `--session-id=U1` that writes `session-arg.json` naming `--session-id` with the id `U1`, and one with `--resume=U1` whose `SessionStart` input has `source` `resume` (both in `stub_launch`), and the existing split-form cases `session_argument_is_recorded`, `session_start_hook_gets_the_session_id` and `launched_member_calls_the_generated_session_start_hook_and_returns_the_exit_step` (in `stub_member`)

## Plan

The fix is one function of the stub agent, `session_arg` in `crates/zygo-launch/tests/stub_agent/stub.rs`, and one new test in `crates/zygo-launch/tests/stub_launch.rs` that launches the stub through the real launcher with `LaunchOpts.agent_session`, as M2 will. The `stub-agent` binary is a `[[bin]]` of `zygo-launch` (`Cargo.toml`, `path = "tests/stub_agent/stub.rs"`), so `cargo test` rebuilds it for each run below.

**Builds on:** (signatures in the code now)
- `AgentAdapter::preassign_args(&self, id: &str) -> Vec<String>` (`crates/zygo-launch/src/adapter.rs:46`); `ClaudeCode` gives `vec![format!("--session-id={id}")]` (`crates/zygo-launch/src/claude_code.rs:444`).
- `AgentAdapter::resume_args(&self, id: &str) -> Vec<String>` (`crates/zygo-launch/src/adapter.rs:52`); `ClaudeCode` gives `vec![format!("--resume={id}")]` (`crates/zygo-launch/src/claude_code.rs:449`); `OpenCode` gives `-s <id>` (`crates/zygo-launch/src/opencode.rs:486`).
- `LaunchOpts.agent_session: Option<AgentSession>` with `pub enum AgentSession { Preassign(String), Resume(String) }` (`crates/zygo-launch/src/launcher.rs:92`, `:115`).
- Harness: `launch`, `rig`, `claude`, `shell`, `stub_control`, `CLAUDE_VERSION`, `Cap.session_arg: Option<(String, String)>` (`crates/zygo-launch/tests/stub_agent/harness.rs`).

**Plan produces:**
- `fn session_arg(argv: &[String]) -> Option<(&'static str, String)>` in `crates/zygo-launch/tests/stub_agent/stub.rs`, same signature, which also reads `--session-id=<id>` and `--resume=<id>`; so `session-arg.json` (`{"flag": …, "id": …}`) and the `SessionStart` `source` are right for a Claude Code launch with `LaunchOpts.agent_session`.

Decisions (author's call):
- D1. Parse rule. Walk `argv` by position; at each token, for each flag of `["--session-id", "--resume", "-s"]` in that order: the token equal to the flag with a next token gives `(flag, next)` (the split form, as today); else, for `--session-id` and `--resume` only, the token that starts with `<flag>=` gives `(flag, the text after the first =)` when that text is not empty. The first token that gives a pair wins, so a `=` form before a split form wins and the reverse too. `-s=<id>` is not read: AU3 keeps `-s <id>` for OpenCode, and OpenCode does not receive `-s=`.
- D2. `--session-id=` and `--resume=` with an empty value are not a session argument (the contract's "has a value"); the walk goes on to the next token.
- D3. The new test is in `stub_launch.rs` and goes through `Launcher::run`, because the defect is seen on that path (L2 round 2). It uses control mode (`ZYGO_STUB_CONTROL`, one `exit 0` step) because the stub calls the `SessionStart` hooks only in control mode (`control` in `stub.rs`). It sets `ZYGO_STUB_SHELL` to `shell()` as `launched_member_calls_the_generated_session_start_hook_and_returns_the_exit_step` does, so the hook runs on every CI OS.
- D4. The split-form cases are in `crates/zygo-launch/tests/stub_member.rs` (`session_argument_is_recorded`, `session_start_hook_gets_the_session_id`, `launched_member_calls_the_generated_session_start_hook_and_returns_the_exit_step`), not in `stub_launch.rs`. The contract's Acceptance runs only `--test stub_launch`, so it does not run them; see "Spec changes for the controller". `stub_member.rs` is not changed.

Steps:

- [ ] Write the failing test. Append to `crates/zygo-launch/tests/stub_launch.rs`, after `agent_session_args_go_before_the_users_args_and_after_opencode_run`:

  ```rust
  #[test]
  fn stub_reads_the_one_token_session_args_of_a_claude_code_launch() { // AU3, FIX-13
      use zygo_launch::launcher::AgentSession;
      let rig = rig();
      for (session, flag, source) in [(AgentSession::Preassign("U1".into()), "--session-id", "startup"), (AgentSession::Resume("U1".into()), "--resume", "resume")] {
          let ctl = rig.root.join(format!("ctl{flag}"));
          stub_control(&ctl, "exit 0").unwrap();
          let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| {
              o.agent_session = Some(session.clone());
              o.env.0.push(("ZYGO_STUB_CONTROL".into(), ctl.clone().into_os_string()));
              o.env.0.push(("ZYGO_STUB_SHELL".into(), shell().into_os_string()));
          });
          assert_eq!(r.code, 0, "{}", r.stderr);
          assert!(r.cap.argv.contains(&format!("{flag}=U1")), "{:?}", r.cap.argv);
          assert_eq!(r.cap.session_arg, Some((flag.to_string(), "U1".to_string())), "{:?}", r.cap.argv);
          let h: Vec<Value> = std::fs::read_to_string(rig.root.join("out/hooks.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
          assert_eq!(h.len(), 1, "{h:?}");
          assert_eq!((&h[0]["event"], &h[0]["input"]["session_id"], &h[0]["input"]["source"]), (&json!("SessionStart"), &json!("U1"), &json!(source)));
      }
  }
  ```

- [ ] Run it and see it fail: `cargo test -p zygo-launch --locked --test stub_launch stub_reads_the_one_token_session_args_of_a_claude_code_launch`. Expected: 1 failed, at the `session_arg` assertion with `left: None`, `right: Some(("--session-id", "U1"))` (the argv assertion before it passes, so the launcher is right and the stub is wrong).
- [ ] Implement D1 and D2 in `session_arg`, `crates/zygo-launch/tests/stub_agent/stub.rs:62-65`; replace the doc line and the body:

  ```rust
  /// The session argument: the first token of `argv` that is `--session-id <id>`, `--resume <id>` (Claude Code) or
  /// `-s <id>` (OpenCode) with a next token, or `--session-id=<id>` or `--resume=<id>` (one token, AU3) with an id that
  /// is not empty. The id of a one-token form is the text after the first `=`.
  fn session_arg(argv: &[String]) -> Option<(&'static str, String)> {
      argv.iter().enumerate().find_map(|(i, a)| {
          ["--session-id", "--resume", "-s"].into_iter().find_map(|f| {
              if a == f {
                  return argv.get(i + 1).map(|v| (f, v.clone()));
              }
              a.strip_prefix(f).and_then(|r| r.strip_prefix('=')).filter(|v| f != "-s" && !v.is_empty()).map(|v| (f, v.to_string()))
          })
      })
  }
  ```

  In the head comment of `stub.rs` (line 3), change "`session-arg.json` names the session argument (`--session-id`, `--resume` or `-s`) when the argv has one." to "`session-arg.json` names the session argument (`--session-id`, `--resume` or `-s`, split or, for the first two, as `<flag>=<id>`) when the argv has one."
- [ ] Run it and see it pass: `cargo test -p zygo-launch --locked --test stub_launch stub_reads_the_one_token_session_args_of_a_claude_code_launch` → 1 passed.
- [ ] Run the Acceptance as amended (see below): `cargo test -p zygo-launch --locked --test stub_launch --test stub_member` → all pass: stub_launch 17 (16 before, plus the new test), and stub_member with its three split-form cases. Then `cargo clippy -p zygo-launch --locked --tests -- -D warnings` → exit 0.
- [ ] Commit, in the FIX-13 checkout: `git add crates/zygo-launch/tests/stub_agent/stub.rs crates/zygo-launch/tests/stub_launch.rs`, then commit with `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com` and this message (subject, one blank line, trailers):

  ```text
  fix(stub): read --session-id=<id> and --resume=<id> as one token

  Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
  Zygo-Work: FIX-13
  ```

  Check: `git show --stat HEAD` names only the two files.

## Spec changes for the controller

The Files line is right: the fix and the new test change only `stub.rs` and `stub_launch.rs`. The Acceptance does not run the split-form cases it names, because they are in `crates/zygo-launch/tests/stub_member.rs` (D4). Replace the Acceptance line of this contract with:

```text
**Acceptance:** `cargo test -p zygo-launch --locked --test stub_launch --test stub_member` → all pass, including a stub launch with `--session-id=U1` that writes `session-arg.json` naming `--session-id` with the id `U1`, and one with `--resume=U1` whose `SessionStart` input has `source` `resume` (both in `stub_launch`), and the existing split-form cases `session_argument_is_recorded`, `session_start_hook_gets_the_session_id` and `launched_member_calls_the_generated_session_start_hook_and_returns_the_exit_step` (in `stub_member`)
```
