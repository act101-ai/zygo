# FIX-16 — workspace_down passes the name after --

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§9.3 step 2 and W§12 (row `zygo down`) as amended 2026-11-22 (W10 plan review: the detached process gets the name after `--`), AU65, AU84 (`-` is allowed at the start of a name); found by W10 plan review round 1 (B3, W10 D8, controller item C5). Defect: `WorkspaceDown::call` refuses a workspace name that starts with `-` (`crates/zygo-mcp/src/tools/workspace_down.rs:98-104`), because `zygo down` had no `--` handling, and runs `<zygo> down <name> [--now]`. Fix: `WorkspaceDown::call` runs `<zygo> down [--now] -- <name>` and no longer refuses a name that starts with `-`; a test with the workspace name `-x` checks the argument list. Edge to M1: M1's `zygo down` parser accepts `[--] [<name>]` (M1 Spec, W10 controller item C4), so the tool's argument list reaches `down` as a name. Added by the controller decisions of 2026-11-23 (FIX-16 plan review): the W§12 `zygo down` synopsis row is rewritten in place to the form of `crates/zygo/src/cmd/down.rs:1`; the "is running" hints of `up` (AU37, `crates/zygo-workspace/src/up.rs:901`) and `ws forget` (`crates/zygo-workspace/src/admin.rs:631`) become `run zygo down -- <name> first`; the AU63 hint (`up.rs:178`, used by `up`, `attach`, `repair` and `down`) becomes `run zygo ws forget -- <name>`; the `attach` hint (W§12 `zygo attach` row, `up.rs:1469`) becomes `run zygo up -- <name>`; CLI tests run the real parsers of `zygo down`, `zygo up` and `zygo ws forget` with `--` and the names `--` and `-x`. Edges (controller, 2026-11-23): FIX-16 depends on FIX-19, and FIX-27 depends on FIX-16 (the workspaces spec; `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/tests/up.rs` and `crates/zygo-workspace/tests/down.rs` with FIX-27 and FIX-24)
**Files:** `crates/zygo-mcp/src/tools/workspace_down.rs`, `crates/zygo-mcp/tests/mcp_server.rs`, `docs/specs/2026-11-20-workspaces-design.md`, `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/tests/up.rs`, `crates/zygo-workspace/tests/admin.rs`, `crates/zygo-workspace/tests/down.rs`, `crates/zygo/tests/workspace_cli.rs`
**Consumes:** `tools::workspace_down::WorkspaceDown::new` (W11), `cmd::down::DownArgs` (M1) for the form zygo down [--] [<name>], `cmd::up::UpArgs` (M1) for the form zygo up [--] [<name>], `cmd::ws::WsCmd` (M1) for the form zygo ws forget [--] <name>
**Produces:** workspace_down passes the name after --
**Acceptance:** `cargo test -p zygo-mcp --locked --test mcp_server` → all pass, including a call for the workspace name `-x` whose stub receives argv `down -- -x` (no refusal), and the W§18 #78 cases with argv `down -- <name>` and `down --now -- <name>` with `now = true`; and (controller, 2026-11-23) `cargo test -p zygo-workspace --locked --test up --test admin --test down` → all pass, including for the name `-x` the hints `zygo: -x is running; run zygo down -- -x first` (`up` and `ws forget`), `zygo: tmux session zygo--x does not belong to this workspace; rename it or run zygo ws forget -- -x` (`up` and `attach`) and `zygo: -x is down; run zygo up -- -x` (`attach`); and `cargo test -p zygo --locked --test workspace_cli` → all pass, including `zygo down --now -- <name>`, `zygo up --no-attach -- <name>` and `zygo ws forget -- <name>` for the names `--` and `-x`, each giving `zygo: no workspace named <name>` (exit 1)

## Plan

Planned 2026-11-23 against head 6f9f1bf (M1 merged at 55f7737). Revised the same day with two sets of controller decisions from the plan review.

### Premise check (live code)

- The defect is live. `crates/zygo-mcp/src/tools/workspace_down.rs:102-104` refuses a name that starts with `-` with `zygo: workspace name `<name>` starts with `-`; zygo down would read it as a flag`. Lines 106-109 build the argument list `down <name>`, then `--now` when `now` is true.
- The three parsers accept `-- <name>`. All three are clap derive positionals with no extra attribute, parsed by the one `Cli::try_parse()` (`crates/zygo/src/cmd/mod.rs:533`) with no change to the tokens before it; clap reads every token after `--` as a positional value. `zygo down`: `DownArgs.name: Option<String>` (`crates/zygo/src/cmd/down.rs:35-36`; variant `Down(down::DownArgs)`, `mod.rs:352`); the M1 test `w12_names_reach_the_library_also_after_double_dash` (`crates/zygo/tests/workspace_cli.rs:133-152`) runs `zygo down --wait -- -x` and gets `zygo: no workspace named -x` (exit 1). `zygo up`: `UpArgs.name: Option<String>` (`crates/zygo/src/cmd/up.rs:14-15`; variant `Up(up::UpArgs)`, `mod.rs:350`), the same form as `DownArgs.name`. `zygo ws forget`: `WsCmd::Forget { name: String }` (`crates/zygo/src/cmd/ws.rs:32`; variant `Ws(ws::WsArgs)`, `mod.rs:362`), a required positional. No test runs `zygo up -- <name>` or `zygo ws forget -- <name>` today; step 13 adds them.
- W10 uses the same form for its own detached run: `spawn_detached` (`crates/zygo-workspace/src/down.rs:669-684`) passes `--` and the name.
- A name that starts with `-` passes AU84: `^[a-z0-9-]{1,32}$` matches `-x`, `--all`, `-` and `--`, and `zygo-<name>` then matches the `tmux_session` form.
- Four hints put the name after a `zygo` command with no `--`, so for the name `-x` the hint is a clap error (exit 2):
  - `crates/zygo-workspace/src/up.rs:901`: `zygo: {name} is running; run zygo down {name} first` (AU37; `up --reset`, `up --loadout`).
  - `crates/zygo-workspace/src/admin.rs:631`: the same text in `check_readable` (`ws forget`).
  - `crates/zygo-workspace/src/up.rs:177-179`: `not_owned`, `tmux session {session} does not belong to this workspace; rename it or run zygo ws forget {}` (AU63). Callers: `up.rs:797` (owner check of `up` and `attach`), `up.rs:1126` (the build undo), `crates/zygo-workspace/src/repair.rs:48`, `crates/zygo-workspace/src/down.rs:1037` and `:1377`. One change in `not_owned` covers all of them.
  - `crates/zygo-workspace/src/up.rs:1469`: `zygo: {} is down; run zygo up {}` (`attach_to`; W§12 `zygo attach` row).
- Tests that check these texts today: `reset_and_running` (`crates/zygo-workspace/tests/up.rs:730`), `session_of_another_workspace_is_not_touched` (`tests/up.rs:753`), `attach_paths` (`tests/up.rs:1045`), the constant `AU63` (`crates/zygo-workspace/tests/down.rs:53`, used by `refusals_change_nothing` at `:598`, `gone_repair_already_down_and_session_target` at `:769` and `owner_changed_before_the_kill` at `:1355`), `forget_refusals` (`crates/zygo-workspace/tests/admin.rs:723`) and `forget_refused_while_the_session_runs` (`tests/admin.rs:1019`). `tests/up.rs:1710` checks only the start of the AU63 text and does not change.
- Spec drift: AU37 (spec line 178), AU63 (line 204), AU65 (line 206), W§12 `zygo down` row (line 556), W§12 `zygo attach` row (line 559) and W§18 #78 (line 814) give the old forms. Step 15 rewrites them in place, with one change-log row.
- Three mcp tests assert the old argument list or the refusal: `workspace_down_runs_down_with_the_name_and_zygo_session` (`mcp_server.rs:586-606`), `workspace_down_refuses_a_name_that_starts_with_a_dash` (`mcp_server.rs:776-790`) and `workspace_down_tools_call_success_is_not_an_error` (`mcp_server.rs:847-865`).

### Interfaces

Consumes:
- `tools::workspace_down::WorkspaceDown::new` (W11): `pub fn new(program: PathBuf, paths: Paths, ws_id: String, session_id: String) -> WorkspaceDown` (`crates/zygo-mcp/src/tools/workspace_down.rs:56`). Unchanged.
- `zygo down [--] [<name>]` (M1): `pub struct DownArgs { pub now: bool, pub timeout: Option<String>, pub wait: bool, pub all: bool, pub name: Option<String> }` (`crates/zygo/src/cmd/down.rs:22-37`; `all` has `conflicts_with = "name"`).
- `zygo up [--] [<name>]` (M1): `pub struct UpArgs { pub name: Option<String>, pub loadout: Option<String>, pub reset: bool, pub no_attach: bool, pub all: bool }` (`crates/zygo/src/cmd/up.rs:12-28`).
- `zygo ws forget [--] <name>` (M1): `WsCmd::Forget { name: String }` in `cmd::ws::WsArgs` (`crates/zygo/src/cmd/ws.rs:13-40`).
- The tool and the hints reach these parsers through the `zygo` binary; no Rust call changes.

Produces:
- workspace_down passes the name after --: `WorkspaceDown::call` (`impl Tool for WorkspaceDown`, `fn call(&self, args: Value) -> Result<Value, ToolError>`) runs `<program> down -- <name>`, or `<program> down --now -- <name>` when `now` is true, for every name that `Store::read` gives. The signature does not change. `up::not_owned(session: &str, name: &str) -> String` (`pub(crate)`) keeps its signature.

### Steps

Part A: the `workspace_down` tool (`zygo-mcp`).

- [x] 1. Test, W§18 #78 argument list. In `crates/zygo-mcp/tests/mcp_server.rs`, in `workspace_down_runs_down_with_the_name_and_zygo_session` (the stub writes one argument per line), change only the three `read("argv")` assertions: line 599 to `assert_eq!(read("argv"), "down\n--\napp\n");`, line 603 to `assert_eq!(read("argv"), "down\n--now\n--\napp\n");`, line 605 to `assert_eq!(read("argv"), "down\n--\napp\n");`. Put this doc comment above `#[cfg(unix)]` of the test:
  ```rust
  /// W§18 #78 (AU65): `down -- <name>`, and `down --now -- <name>` with `now = true`; `ZYGO_SESSION` is set and
  /// stdin is `/dev/null`.
  ```
- [x] 2. Test, the JSON-RPC path. In `workspace_down_tools_call_success_is_not_an_error`, change the last assertion (the stub writes the arguments separated by a space) to:
  ```rust
  assert_eq!(std::fs::read_to_string(d.path().join("argv")).unwrap(), "down --now -- app ");
  ```
- [x] 3. Test, a name that starts with `-`. Delete `workspace_down_refuses_a_name_that_starts_with_a_dash` and its doc comment (`/// F5: …`, lines 776-777). In its place add:
  ```rust
  /// FIX-16 (AU65, AU84): a name that starts with `-` is passed after `--`, so `zygo down` reads it as the name.
  /// Nothing is refused.
  #[cfg(unix)]
  #[test]
  fn workspace_down_passes_a_name_that_starts_with_a_dash_after_double_dash() {
      let _forks = forks();
      let d = tempfile::tempdir().unwrap();
      let p = paths(d.path(), "");
      let prog = stub(d.path(), r#"printf '%s\n' "$@" > "$(dirname "$0")/argv"
  echo 'zygo: down started; end your turn'"#);
      let t = WorkspaceDown::new(prog, p.clone(), WS_ID.into(), "s".into());
      let read = || std::fs::read_to_string(d.path().join("argv")).unwrap();
      for name in ["-x", "--all", "-", "--"] {
          workspace_named(&p, name);
          assert_eq!(t.call(json!({})), Ok(json!("zygo: down started; end your turn")), "{name}");
          assert_eq!(read(), format!("down\n--\n{name}\n"));
          assert_eq!(t.call(json!({"now": true})), Ok(json!("zygo: down started; end your turn")), "{name}");
          assert_eq!(read(), format!("down\n--now\n--\n{name}\n"));
      }
  }
  ```
- [x] 4. Run `cargo test -p zygo-mcp --locked --test mcp_server workspace_down` alone. Result that means pass for this step: exactly three tests fail, `workspace_down_runs_down_with_the_name_and_zygo_session` (left `"down\napp\n"`, right `"down\n--\napp\n"`), `workspace_down_tools_call_success_is_not_an_error` (left `"down app --now "`) and `workspace_down_passes_a_name_that_starts_with_a_dash_after_double_dash` (left `Err(Failed("zygo: workspace name `-x` starts with `-`; zygo down would read it as a flag"))`). All other `workspace_down` tests pass.
- [x] 5. Code. In `crates/zygo-mcp/src/tools/workspace_down.rs`, in `fn call(&self, args: Value) -> Result<Value, ToolError>`: delete the comment and the `if name.starts_with('-') { … }` refusal (lines 98-104). Build the argument list in this order: `down`, then `--now` when `now` is true, then `--`, then the name:
  ```rust
  // `--` ends the options of `zygo down` (W§12, M1), so a name that starts with `-` (AU84) is read as the name.
  let mut cmd = Command::new(&self.program);
  cmd.arg("down");
  if now {
      cmd.arg("--now");
  }
  cmd.arg("--").arg(&name);
  ```
  The `ZYGO_SESSION` line, `run_bounded` and the result mapping do not change.
- [x] 6. Run `cargo test -p zygo-mcp --locked --test mcp_server` alone. Result that means pass: `test result: ok`, 0 failed, and the three tests of step 4 are in the passed list.

Part B: the hints (`zygo-workspace`).

- [x] 7. Tests in `crates/zygo-workspace/tests/up.rs`. Change three existing assertions:
  - line 730 (`reset_and_running`): `let running = (1, String::new(), "zygo: app is running; run zygo down -- app first\n".to_string());`
  - line 753 (`session_of_another_workspace_is_not_touched`): `let msg = (1, String::new(), "zygo: tmux session zygo-app does not belong to this workspace; rename it or run zygo ws forget -- app\n".to_string());`
  - line 1045 (`attach_paths`): `assert_eq!(attach_run(&l.o, None, None), (1, String::new(), "zygo: app is down; run zygo up -- app\n".to_string()));`

  After `session_of_another_workspace_is_not_touched`, add:
  ```rust
  #[test] // AU37, AU63, W§12 `zygo attach`, FIX-16: each hint gives the name after `--`, so a name that starts with `-` works
  fn dash_name_hints_pass_the_name_after_double_dash() {
      let _g = serial();
      let Some(_) = require_tmux() else { return };
      let mut l = live("0");
      let store = l.store();
      saved(&store, &l.repo, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);
      let sock = l.srv.socket().to_path_buf();
      set(&store, |ws| {
          ws.name = "-x".into();
          ws.tmux_session = "zygo--x".into();
          ws.tmux_socket = Some(sock.clone());
          ws.phase = Phase::Up;
      });
      l.srv.run(&["new-session", "-d", "-s", "zygo--x", "sleep", "600"]).unwrap();
      let before = raw(&store);
      // AU63: a session of another workspace.
      l.srv.run(&["set-option", "-t", "=zygo--x:", "@zygo-ws", OTHER_ID]).unwrap();
      let owner = (1, String::new(), "zygo: tmux session zygo--x does not belong to this workspace; rename it or run zygo ws forget -- -x\n".to_string());
      assert_eq!(run(&l.o), owner);
      assert_eq!(attach_run(&l.o, None, None), owner);
      assert_eq!(raw(&store), before);
      // AU37: the workspace's own session runs.
      l.srv.run(&["set-option", "-t", "=zygo--x:", "@zygo-ws", l.id.as_str()]).unwrap();
      let running = (1, String::new(), "zygo: -x is running; run zygo down -- -x first\n".to_string());
      l.o.reset = true;
      assert_eq!(run(&l.o), running);
      l.o.reset = false;
      l.o.loadout = Some("solo".into());
      assert_eq!(run(&l.o), running);
      assert_eq!(raw(&store), before);
      // W§12 `zygo attach`: a stopped workspace with no saved socket.
      l.srv.run(&["kill-session", "-t", "=zygo--x:"]).unwrap();
      set(&store, |ws| {
          ws.phase = Phase::Down;
          ws.tmux_socket = None;
      });
      assert_eq!(attach_run(&l.o, None, None), (1, String::new(), "zygo: -x is down; run zygo up -- -x\n".to_string()));
  }
  ```
- [x] 8. Test in `crates/zygo-workspace/tests/down.rs`: change line 53 to
  ```rust
  const AU63: &str ="zygo: tmux session zygo-app does not belong to this workspace; rename it or run zygo ws forget -- app";
  ```
- [x] 9. Tests in `crates/zygo-workspace/tests/admin.rs`: in `forget_refusals` change line 723 to `assert_eq!(run("app"), refused("zygo: app is running; run zygo down -- app first"));`; in `forget_refused_while_the_session_runs` change line 1019 to `(1, String::new(), "zygo: app is running; run zygo down -- app first\n".to_string())`. After `forget_refusals`, add (the stand-in tmux `answer_tmux` answers from the files `exists-<session>` and `ws-<session>`):
  ```rust
  #[test] // W§12 `ws forget`, FIX-16: the running hint gives the name after `--`
  fn forget_running_hint_passes_a_dash_name_after_double_dash() {
      let _s = serial();
      let td = TempDir::new().unwrap();
      let t = td.path();
      let p = paths(t);
      let tmux = answer_tmux(t);
      ws_state(&p, ID, "-x", &t.join("r/x"), |ws| ws.phase = Phase::Up);
      let bytes = state_bytes(&p, ID);
      fs::write(t.join("exists-zygo--x"), "").unwrap();
      fs::write(t.join("ws-zygo--x"), ID).unwrap();
      assert_eq!(forget_run(&p, &tmux, None, "-x"), (1, String::new(), "zygo: -x is running; run zygo down -- -x first\n".to_string()));
      assert_eq!(state_bytes(&p, ID), bytes);                          // nothing removed
  }
  ```
- [x] 10. Run `cargo test -p zygo-workspace --locked --test up --test admin --test down` alone. Result that means pass for this step: exactly these tests fail, each with an old hint (no `--` before the name) on the left: in `up`, `reset_and_running`, `session_of_another_workspace_is_not_touched`, `attach_paths`, `dash_name_hints_pass_the_name_after_double_dash`; in `admin`, `forget_refusals`, `forget_refused_while_the_session_runs`, `forget_running_hint_passes_a_dash_name_after_double_dash`; in `down`, `refusals_change_nothing`, `gone_repair_already_down_and_session_target`, `owner_changed_before_the_kill`. On a machine with no tmux 3.2 or later, the four `up` tests and `forget_refused_while_the_session_runs` print the skip reason and pass; the other five still fail.
- [x] 11. Code. Only the hint texts change; no other line changes:
  - `crates/zygo-workspace/src/up.rs:178` (`not_owned`): `format!("tmux session {session} does not belong to this workspace; rename it or run zygo ws forget -- {}", esc(name))`
  - `crates/zygo-workspace/src/up.rs:901`: `format!("zygo: {name} is running; run zygo down -- {name} first")`
  - `crates/zygo-workspace/src/up.rs:1469`: `format!("zygo: {} is down; run zygo up -- {}", t.name, t.name)`
  - `crates/zygo-workspace/src/admin.rs:631`: `format!("zygo: {} is running; run zygo down -- {} first", ws.name, ws.name)`
- [x] 12. Run `cargo test -p zygo-workspace --locked --test up --test admin --test down` alone. Result that means pass: `test result: ok` for the three binaries, 0 failed.

Part C: the real parsers (`zygo`).

- [x] 13. Test. In `crates/zygo/tests/workspace_cli.rs`, after `w12_names_reach_the_library_also_after_double_dash`, add:
  ```rust
  /// FIX-16: the argument list of the `workspace_down` tool and the forms that the hints print (`zygo down -- <name>`,
  /// `zygo up -- <name>`, `zygo ws forget -- <name>`) reach the library also for the names `--` and `-x`.
  #[cfg(unix)]
  #[test]
  fn fix16_double_dash_names_reach_the_library() {
      let Some(tmux) = zygo_workspace::mux::testing::require_tmux() else { return };
      let h = tempfile::tempdir().unwrap();
      let path = tmux.parent().unwrap().as_os_str().to_owned();
      for name in ["--", "-x"] {
          for args in [&["down", "--now", "--", name][..], &["up", "--no-attach", "--", name], &["ws", "forget", "--", name]] {
              let o = zygo_in(h.path(), "", &path, args, &[]);
              assert_eq!(
                  (o.status.code(), text(&o.stderr)),
                  (Some(1), format!("zygo: no workspace named {name}\n")),
                  "zygo {}",
                  args.join(" ")
              );
          }
      }
  }
  ```
- [x] 14. Run `cargo test -p zygo --locked --test workspace_cli` alone. Result that means pass: `test result: ok`, 0 failed, and `fix16_double_dash_names_reach_the_library` is in the passed list. This test checks M1's parsers, which need no change, so it passes at once. If it fails because a parser does not read the name after `--`, stop and report the clap output to the controller as an M1 parser defect; do not change `crates/zygo/src/cmd/`.

Part D: the spec, the floor, the commit.

- [x] 15. Spec. In `docs/specs/2026-11-20-workspaces-design.md`:
  - AU37: replace `zygo: <name> is running; run zygo down <name> first` with `zygo: <name> is running; run zygo down -- <name> first`.
  - AU63: replace `rename it or run zygo ws forget <name>` with `rename it or run zygo ws forget -- <name>`.
  - AU65: replace "It runs `<zygo> down <name>` (with `--now`) with `ZYGO_SESSION` set, so the AU18 detach applies." with "It runs `<zygo> down -- <name>` (`<zygo> down --now -- <name>` with `now`) with `ZYGO_SESSION` set, so the AU18 detach applies; the name comes after `--`, so a name that starts with `-` (AU84) is read as the name and no name is refused."
  - W§12, row `zygo down`: replace the first cell `` `zygo down [<name>] [--now] [--timeout <dur>] [--wait] [--all]` `` with `` `zygo down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]` ``, the form of `crates/zygo/src/cmd/down.rs:1`. The second cell does not change.
  - W§12, row `zygo attach [<name>]`: replace `zygo: <name> is down; run zygo up <name>` with `zygo: <name> is down; run zygo up -- <name>`.
  - W§18 #78: replace "A call runs `zygo down <name>` with `ZYGO_SESSION` set and returns `zygo: down started; end your turn`; `now = true` adds `--now`." with "A call runs `zygo down -- <name>` with `ZYGO_SESSION` set and returns `zygo: down started; end your turn`; `now = true` runs `zygo down --now -- <name>`. A name that starts with `-` (AU84) is passed after `--`, not refused."
  - Append one row at the end of `## Change log`:
    `| 2026-11-23 | AU37, AU63, AU65, W§12 (rows `zygo down`, `zygo attach`), W§18 #78 | author's call (FIX-16; W10 plan review round 1, controller item C5; controller decisions 2026-11-23, FIX-16 plan review) | `workspace_down` refused a workspace name that starts with `-`, which AU84 allows, because `zygo down` had no `--` handling. M1's `zygo down` takes `[--] [<name>]`, so the tool runs `<zygo> down [--now] -- <name>` and refuses no name. The W§12 `zygo down` synopsis now has the form of `zygo down --help`. The hints that name a command with a workspace name (AU37 and `ws forget`: `zygo down`; AU63: `zygo ws forget`; `attach`: `zygo up`) put the name after `--`, so a name that starts with `-` works; `zygo up` and `zygo ws forget` read a name after `--` (M1 parsers). |`
  Checks, each run alone:
  - `rg -n -F 'down <name>` (with' docs/specs/2026-11-20-workspaces-design.md` → no line;
  - `rg -n -F 'down --now -- <name>' docs/specs/2026-11-20-workspaces-design.md` → the AU65 line and the #78 line (206 and 814 at planning);
  - `rg -n -F 'run zygo down <name> first' docs/specs/2026-11-20-workspaces-design.md` → no line;
  - `rg -n -F 'run zygo down -- <name> first' docs/specs/2026-11-20-workspaces-design.md` → the AU37 line (178 at planning);
  - `rg -n -F 'run zygo ws forget <name>' docs/specs/2026-11-20-workspaces-design.md` → no line;
  - `rg -n -F 'run zygo ws forget -- <name>' docs/specs/2026-11-20-workspaces-design.md` → the AU63 line (204 at planning);
  - `rg -n -F 'run zygo up <name>' docs/specs/2026-11-20-workspaces-design.md` → no line;
  - `rg -n -F 'is down; run zygo up -- <name>' docs/specs/2026-11-20-workspaces-design.md` → the `zygo attach` row (559 at planning);
  - `rg -n -F '| `zygo down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]` | W§9.' docs/specs/2026-11-20-workspaces-design.md` → one line (556 at planning);
  - `rg -n -F '`zygo down [<name>]' docs/specs/2026-11-20-workspaces-design.md` → no line.
- [x] 16. Run `cargo clippy -p zygo-mcp -p zygo-workspace -p zygo --locked --tests -- -D warnings` alone. Result that means pass: exit 0, no warning.
- [x] 17. Run each alone (the floor, scoped to the changed packages): `cargo test -p zygo-mcp --locked`, `cargo test -p zygo-workspace --locked`, `cargo test -p zygo --locked --test workspace_cli`. Result that means pass: every test binary reports `test result: ok`, 0 failed.
- [x] 18. Tick steps 1-17 in this file. Commit the nine files of Files and `docs/work/phase2/nodes/FIX-16.md` in one commit with this message:
  ```
  fix(workspace): pass the workspace name to zygo commands after --

  workspace_down refused a workspace name that starts with `-` (AU84
  allows it) because zygo down had no `--` handling. M1's zygo down takes
  `[--] [<name>]`, so the tool now runs `<zygo> down [--now] -- <name>`
  and refuses no name. The hints of up, attach, down and ws forget give
  `zygo down -- <name>`, `zygo ws forget -- <name>` and
  `zygo up -- <name>`. Spec AU37, AU63, AU65, the W§12 zygo down and
  zygo attach rows and W§18 #78 state the new forms.

  Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ
  Zygo-Work: FIX-16
  ```
  The message has no other trailer and no attribution line.
