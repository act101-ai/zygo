# FIX-30 — Every use of a saved tmux_socket passes the AU54 check

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` AU54 (the quote rule: a tmux socket path that holds `'`, `"`, `\`, `$` or a control character is refused with `UNSAFE_PATH_MESSAGE`) and AU62 (every tmux call for a workspace passes `-S <tmux_socket>`). Created 2026-11-23 at the FIX-29 merge (controller decision). Problem: FIX-29 made `repair::check_saved_socket` (`crates/zygo-workspace/src/repair.rs:64`, `pub(crate)`) run before every use of a saved socket in `up`, `down`, `repair` and `zygo attach`. These uses of a saved `tmux_socket` still have no AU54 check: (1) `crates/zygo-workspace/src/admin.rs:248` `ask` (the `zygo ls` session check) takes `ws.tmux_socket`, then calls `socket_is_private` at `:257` and `mux_of(sock)` at `:263`; (2) `admin.rs:617` `check_readable` (`zygo ws forget`) calls `socket_is_private` at `:621`, then `server(tmux, ws.tmux_socket.as_deref(), tmux_env)` at `:626`; (3) `admin.rs:650` `check_invalid` reads the raw JSON `tmux_socket` and calls `socket_is_private` at `:654` and tmux calls after it, with no `check_path`; (4) `admin.rs:806` `forget_pane_run` gives `w.tmux_socket.clone()` to `Tmux::new` for `snapshot::after_write`; (5) `crates/zygo/src/cmd/member.rs:286` `mux_of` gives `ws.tmux_socket.clone()` to `Tmux::new` for the member `after_write` snapshot; (6) `member.rs:835-842` `hand_start` compares the saved socket with the socket of `$TMUX` and prints the saved socket in the not-saved line, then calls `socket_is_private` at `:845` and `mux.display` at `:850`, with no AU54 check of the saved socket. Change: call the same `check_saved_socket` before each such use, with the same refusal (the AU54 line `UNSAFE_PATH_MESSAGE`, before any tmux call); for the raw JSON socket of (3), which is not a `Workspace`, call the `mux::check_path` that `check_saved_socket` wraps. `member.rs` is in the crate `zygo`, so `check_saved_socket` becomes `pub` in `repair.rs`. Edges: FIX-24 also changes `crates/zygo/src/cmd/member.rs`, so FIX-24 runs first; E1 and E2 also change `docs/specs/2026-11-20-workspaces-design.md`, so E2 (after E1) runs first.
**Files:** `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/src/down.rs`, `crates/zygo-workspace/src/repair.rs`, `crates/zygo-workspace/src/snapshot.rs`, `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/tests/admin.rs`, `crates/zygo-workspace/tests/down.rs`, `crates/zygo-workspace/tests/snapshot.rs`, `crates/zygo-workspace/tests/up.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/tests/session_verbs.rs`, `crates/zygo/tests/workspace_member.rs`, `docs/specs/2026-11-20-workspaces-design.md` (`down.rs`, `snapshot.rs`, `up.rs`, `session_verbs.rs` and their tests added at planning and at the plan review: see Plan, "Premise check")
**Consumes:** —
**Produces:** `check_saved_socket`: `pub fn check_saved_socket(ws: &Workspace) -> Result<(), UnsafePath>` (visibility only, `repair.rs`); `check_server`: `pub(crate) fn check_server(mux: &dyn Multiplexer) -> Result<(), UnsafePath>` (new, `up.rs`)
**Acceptance:** `cargo test -p zygo-workspace --locked` and `cargo test -p zygo --locked --test workspace_member` → all pass, including one test for each path above with a saved socket that holds `'`: the path refuses with the AU54 line and starts no tmux call (plan review: also `cargo test -p zygo --locked --test session_verbs`, for the verb-mirror test of coordinator item 10; the refusal of each path is the one in Plan, "Refusal behavior per path")

## Plan

Planned 2026-11-23 on `worktree-zygo-phase-2` at `e49d5c5` (FIX-24 merged). Revised 2026-11-23 after the plan review: coordinator items 1-10 are applied as author's calls.

### Premise check

The cited uses, against the live code (FIX-24 moved the `member.rs` lines):

| # | Contract cite | Live place | AU54 check now |
|---|---|---|---|
| 1 | `admin.rs:248` `ask` | `admin.rs:248` (`ws.tmux_socket`), `:254` `parent_missing`, `:257` `socket_is_private`, `:263` `mux_of(sock)` | none |
| 2 | `admin.rs:617` `check_readable` | `admin.rs:617-621`, `:626` `server(...)`, first tmux call `:627` | none |
| 3 | `admin.rs:650` `check_invalid` | `admin.rs:650` (raw JSON), `:653` `parent_missing`, `:654` `socket_is_private`, first tmux call `:669` | none |
| 4 | `admin.rs:806` `forget_pane_run` | `admin.rs:806-807` `Tmux::new(.., w.tmux_socket)` then `snapshot::after_write` | none |
| 5 | `member.rs:286` `mux_of` | moved to `member.rs:291-293`; callers `after_write` `:297-299` and `agent_snapshot` `:303-305`, called at `:136-137` and `:257-259` | none |
| 6 | `member.rs:835-850` `hand_start` | moved to `member.rs:852-869`: compare and print at `:853-861`, `socket_is_private` at `:864`, `display` at `:869` | none |

Every other use of a saved `tmux_socket` in every crate (`git grep -n tmux_socket -- crates`, plus each `Tmux::new`, `up::server` and `snapshot::run`/`after_write` call):

| Place | Use | AU54 check now |
|---|---|---|
| `crates/zygo/src/cmd/ws.rs:371-376` `snapshot_cmd` (`zygo ws snapshot`: every tmux hook, the slash child) | `Tmux::new(.., ws.tmux_socket)`, `snapshot::run` | **none** (not in the contract) |
| `crates/zygo/src/cmd/verbs.rs:257-258` (verb mirror, CLI form) | `Tmux::new(.., written.tmux_socket)`, `snapshot::after_write` | **none** (not in the contract) |
| `up.rs:975-989` step 5, `:992-1025` step 6, `:1275-1276`, `:1334`, `:1374-1375` (`snapshot::run` at `:1375`), `:1393` | after `check_saved_socket` at `:977`, or the step 9 check at `:1263` | yes |
| `up.rs:1531-1550` `session_check`, `:1557-1597` `clear_down_all` | after `check_saved_socket` at `:1533` | yes |
| `up.rs:1629-1642` `attach_to` | after `check_saved_socket` at `:1634` | yes |
| `down.rs:480-494` `check_socket`, used by `detach_wanted` `:516`, `guard` `:562`, `any_session` `:602` | `check_saved_socket` at `:482` | yes |
| `down.rs:964-1018` `prepare`; jobs `:1092`, `:1338`, `:1395` (the `ws` of the job comes from `prepare`) | `check_saved_socket` at `:993` | yes |
| `repair.rs:84-91` `repair`; `:229-250` (a socket read from the server) | `check_saved_socket` at `:91`; `check_path` at `:234` | yes |
| `snapshot.rs:582-587` (`attempt`), `:365` (`under_lock`) | compare only, then the tmux calls of `owner` `:592` and `read_panes` `:604` on the multiplexer of the caller | none of its own |
| `crates/zygo-hooks/src/workspace.rs:439`, `:508`, `:533` | `take_pending`, `queue_pending`: files only, no tmux call | no use |
| `crates/zygo-mcp` | no `tmux_socket` read, no `Tmux` | no use |

The `$TMUX` fallback (coordinator items 4-5). `up::server` (`up.rs:274-277`) uses the socket of `$TMUX` when no socket is saved (D18). Each place where that socket reaches tmux:

| Place | First tmux call on the `$TMUX` socket | AU54 check now |
|---|---|---|
| `up.rs:981` step 5 (`mux` given to `repair`) | `repair.rs:92` `session_exists` (phases `building`, `draining`, `stopping`) | none |
| `up.rs:992-993` step 6 (`mux6`) | `up.rs:994` `session_exists` | none |
| `up.rs:1067-1073` step 8 (`chosen`, written into `tmux_socket`) | after step 6 | covered by the step 6 check (same socket: `saved_before` `:1067` and `saved_socket` `:992` are both the `tmux_socket` of `ws`) |
| `up.rs:1541` `session_check` (`up --all`) | `up.rs:1542` `session_exists` | none |
| `down.rs:510` `env_server`, `:518` `mux` in `detach_wanted` | `down.rs:519` `display` (`:513` only compares paths) | none |
| `down.rs:563` `guard` | `down.rs:564` `server_pid` (`list-sessions`) | none |
| `down.rs:603` `any_session` | `down.rs:603` `session_exists` | none |
| `down.rs:1003` `prepare` (`mux` given to `repair`), `:1018` | `repair.rs:92`, `down.rs:1019` `session_exists` | none |
| `admin.rs:626` `check_readable` | `admin.rs:627` `session_exists` | none |
| `member.rs:848` `hand_start` (`sock` from `$TMUX`) | `member.rs:869` `display` (after `socket_is_private` `:864`) | none; for `Ok(Some(ws))` the compare at `:853-855` makes `sock` equal to the saved socket |
| `admin.rs` `ask` (`zygo ls`), `check_invalid`, `up.rs` `attach_to` | no fallback: no saved socket gives `unreachable` (`admin.rs:248-250`), a refusal (`admin.rs:650-652`) or the down line (`up.rs:1629-1632`) | not applicable |

Premise drift:
1. Uses not in the contract: `ws snapshot` (`ws.rs:376`), the verb mirror (`verbs.rs:257`), and every `$TMUX` fallback above. Decisions (author's calls): one AU54 check inside `snapshot::attempt` covers every snapshot caller (`ws.rs:376`, `verbs.rs:258`, `member.rs:298`, `:304`, `admin.rs:807`, and the `up`/`down` snapshots again), on every state read, before the first tmux call. One helper `up::check_server` checks the socket that a multiplexer passes to tmux, saved or from `$TMUX`; it runs right before the first tmux call at each fallback place. The coordinator named `down.rs:510`; the `$TMUX` socket also reaches tmux in `guard`, `any_session` and `prepare`, and `prepare` runs without `detach_wanted` when `TMUX_PANE` is unset (`down.rs:507-509`, `:1466-1520`), so all four get the check. `up` step 5 gets it too (`repair.rs:92` is a tmux call on that socket before step 6).
2. `zygo ls` "never fails" (`crates/zygo/src/cmd/ls.rs:8-9`; W§12 row `zygo ls`). Its refusal is the row state `unreachable` with no tmux call, as for a socket that fails AU62 (`admin.rs:257-258`).
3. `forget` gives exit 1 for every `Err` (`admin.rs:550-553`). The plan carries the AU54 refusal as a typed error (`ForgetError::Unsafe`), which gives exit 2.
4. FIX-30 depends on E2 (`graph.md:87`); E2 is `PLANNED` (`graph.md:60`). Both change `docs/specs/2026-11-20-workspaces-design.md`. The controller schedules the run.
5. The verb-mirror test needs the member fixture of `crates/zygo/tests/session_verbs.rs` (`mod member_fixture`, `:19-21`; the CLI mirror test `au105_cli_verb_runs_the_pending_snapshot_inline_after_the_write`, `:809-828`). So `session_verbs.rs` is added to Files and its test run to the acceptance commands.

How `check_saved_socket` is reached from the crate `zygo`: `repair` is `pub mod repair` (`crates/zygo-workspace/src/lib.rs:11`). The function is `pub(crate)` (`repair.rs:64`). Step 11 makes it `pub`; `member.rs` imports `zygo_workspace::repair`. The `$TMUX` check of the hand start uses `zygo_workspace::mux::check_path`, which is already `pub` (`mux/mod.rs:711`).

### Refusal behavior per path

| Path | Behavior | Reason |
|---|---|---|
| `zygo ws forget`, saved socket (readable state or raw JSON) | right before the first tmux call (after `parent_missing`, AU62, `no_tmux`): `<UNSAFE_PATH_MESSAGE>; remove <state folder> by hand to forget this workspace`, exit 2, nothing deleted | user command (AU54 exit 2); `up` and `down` also refuse this state, so the line names the only way out; the path is escaped with `zygo_core::esc` (C6) |
| `zygo ws forget`, no saved socket, `$TMUX` socket fails | `UNSAFE_PATH_MESSAGE`, exit 2, nothing deleted | the state is not at fault: a `forget` outside that tmux server works |
| `zygo ws forget`, socket folder missing, or no tmux | no AU54 check; `forget` goes on as today | no tmux call is made (`admin.rs:618-619`, `:623-624`, `:653`, `:666`) |
| `zygo ws forget-pane` | no refusal; the record goes; the `snapshot.pending` check logs one line, no tmux call | its only socket use is the snapshot, which `snapshot::attempt` guards |
| `zygo ls` | row `unreachable`, no tmux call, after the `no tmux` check, before `parent_missing` | `ls` never fails (W§12); same state as an AU62 failure |
| `up` steps 5 and 6 (`$TMUX` socket), `zygo down` (`$TMUX` socket in the detach rule, the guard, the session check, step 3) | `UNSAFE_PATH_MESSAGE`, exit 2, nothing written, no tmux call on that socket | user commands, as the saved-socket refusals of FIX-29 |
| `up --all` (`$TMUX` socket) | `zygo: <name> skipped: <AU54 text without "zygo: ">` | the `up --all` mode of AU54 |
| hand start, saved socket fails | `zygo: this agent is not saved by the workspace: <AU54 text without "zygo: ">`, v1 launch, no tmux call; right after the state read, before the compare that prints the saved socket | M3 rule (W§8.4), C6, W-C6 |
| hand start, `$TMUX` socket fails (state `Ok(None)` or `Err`) | the same line, v1 launch, before `socket_is_private` and `display` | the same |
| `zygo ws snapshot` (tmux hooks) | one `errors.log` line (`snapshot (<event>): <AU54 text without "zygo: ">`), exit 0, no tmux call | a hook never breaks the agent (C6, W-C1) |
| a snapshot that the member wrapper, a verb, `up`, `down` or `forget-pane` runs | the same line (`snapshot: ...`), `Skipped(Logged)`, no write, no tmux call; the caller goes on | the same |

### Interfaces

Consumed (moved word for word from the header Consumes line; no node that FIX-30 depends on lists these under Produces):

- `check_saved_socket`: `pub(crate) fn check_saved_socket(ws: &Workspace) -> Result<(), UnsafePath>` (FIX-29, `crates/zygo-workspace/src/repair.rs:64`)
- `check_path`: `pub fn check_path(path: &Path) -> Result<&str, UnsafePath>` (W3, `crates/zygo-workspace/src/mux/mod.rs:711`)
- `UNSAFE_PATH_MESSAGE`: `pub const UNSAFE_PATH_MESSAGE: &str` (`mux/mod.rs:697`)
- `UnsafePath`: `pub struct UnsafePath` (`mux/mod.rs:702`)
- `server`: `pub fn server(tmux: &Path, tmux_socket: Option<&Path>, tmux_env: Option<&OsStr>) -> Tmux` (`up.rs:274`)
- `Multiplexer::socket`: `fn socket(&self) -> Option<&Path>` (`mux/mod.rs:309-310`)

### Rules for the implementer

- Work in the node checkout under `/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/`.
- The new tests use stand-in tmux scripts and start no server.
- Scoped test runs only (`-p zygo-workspace`, `-p zygo --test workspace_member`, `-p zygo --test session_verbs`, `-p zygo --lib cmd::member`).
- No pipes and no filters (`head`, `tail`, `grep`, `sed`, `awk`, `cut`, `tee`, `rg`) on command output.

Below, `T` is `/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/FIX-30-target`. `AU54_REASON` in the tests is the AU54 text without `zygo: `: `the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it` (`mux/mod.rs:697-698`).

### Steps

- [x] **1. Test (red): the snapshot refuses a saved socket that fails AU54.** In `crates/zygo-workspace/tests/snapshot.rs`, after `a_multiplexer_bound_to_another_socket_makes_no_call`, add:
  ```rust
  /// The AU54 text without `zygo: ` (`mux/mod.rs:697-698`).
  const AU54_REASON: &str =
      "the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";

  #[test] // FIX-30, AU54, W§7.4 step 1: a saved socket that fails AU54: one log line, no write, no tmux call
  fn au54_a_saved_socket_with_a_quote_logs_one_line_and_makes_no_tmux_call() {
      let (_d, p, store) = up(&["kaaaaaaaa"]);
      set_socket(&store, "/tmp/so'ck/s");
      let mut f = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
      f.socket = Some(PathBuf::from("/tmp/so'ck/s"));
      let before = raw(&store);
      // as a tmux hook runs it
      assert_eq!(
          run(&p, &f, ID, Some("after-split-window"), false, HOOK_WAIT).unwrap(),
          SnapshotOutcome::Skipped(SkipReason::Logged)
      );
      assert_eq!(f.calls(""), 0);
      assert_eq!(raw(&store), before);
      let lines = log_lines(&store);
      assert_eq!(lines.len(), 1, "{lines:?}");
      assert!(lines[0].ends_with(&format!("snapshot (after-split-window): {AU54_REASON}")), "{lines:?}");
      assert_eq!(pending(&store), None);
      // the `snapshot.pending` check of a writer
      fs::write(store.dir().join("snapshot.pending"), "").unwrap();
      assert_eq!(after_write(&p, &f, ID, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Logged));
      assert_eq!(f.calls(""), 0);
      assert_eq!(raw(&store), before);
      let lines = log_lines(&store);
      assert_eq!(lines.len(), 2, "{lines:?}");
      assert!(lines[1].ends_with(&format!("snapshot: {AU54_REASON}")), "{lines:?}");
      assert_eq!(pending(&store), None);
  }
  ```
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test snapshot au54_` fails: the outcome is not `Skipped(Logged)`, or `f.calls("")` is not 0 (the `owner` call `show_session_option` runs).

- [x] **2. Test (red): the member wrapper, `zygo ws snapshot` and the verb mirror make no tmux call.**
  - In `crates/zygo/tests/workspace_member.rs`, add a section at the end of the file:
    ```rust
    // ---- FIX-30: a tmux socket that fails AU54 ----

    /// The AU54 text without `zygo: ` (`crates/zygo-workspace/src/mux/mod.rs:697-698`).
    const AU54_REASON: &str =
        "the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";

    /// A harness with no tmux server whose `bin/tmux` makes `root/tmux-called` at each call (as
    /// `w18_54_outside_tmux_is_a_v1_launch_with_no_tmux_call`), and the folder `root/so'ck` with mode 0700, so the socket
    /// `root/so'ck/s` passes AU62 and fails AU54. Returns the harness and that socket.
    fn quoted_socket_harness() -> (Harness, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let h = Harness::no_tmux();
        let t = h.bin.join("tmux");
        std::fs::write(&t, "#!/bin/sh\ntouch \"$HOME/tmux-called\"\nexit 1\n").unwrap();
        std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o755)).unwrap();
        ready_exe(&t);
        let _ = std::fs::remove_file(h.root().join("tmux-called")); // the probe of `ready_exe` ran it once
        let dir = h.root().join("so'ck");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        (h, dir.join("s"))
    }

    /// The lines of `errors.log` of WS_ID; empty when it is missing.
    fn errors_log(h: &Harness) -> Vec<String> {
        std::fs::read_to_string(h.store(WS_ID).dir().join("errors.log"))
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    #[test] // FIX-30, AU54, C6: the member wrapper's snapshots log the AU54 text and make no tmux call; the agent runs
    fn au54_the_member_wrapper_logs_its_snapshots_and_makes_no_tmux_call() {
        let (h, sock) = quoted_socket_harness();
        up_state(&h, &[(KEY, MemberRecord::new(Agent::ClaudeCode))], &mut |w| w.tmux_socket = Some(sock.clone()));
        std::fs::write(h.store(WS_ID).dir().join("snapshot.pending"), "").unwrap();
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        let o = Guarded::spawn(h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", KEY], &pairs(&env))).output("ws run");
        assert!(o.status.success(), "{}", text(&o.stderr));
        assert_eq!(stub_env(&out).get("ZYGO_WORKSPACE").and_then(Value::as_str), Some(WS_ID));
        assert!(!h.root().join("tmux-called").exists());
        let lines = errors_log(&h);
        let snaps: Vec<&String> = lines.iter().filter(|l| l.contains(" snapshot")).collect();
        assert!(!snaps.is_empty(), "{lines:?}");
        assert!(snaps.iter().all(|l| l.ends_with(&format!("snapshot: {AU54_REASON}"))), "{lines:?}");
        assert!(!h.store(WS_ID).dir().join("snapshot.pending").exists());
    }

    #[test] // FIX-30, AU54, W§7.4 step 1, C6: `zygo ws snapshot` (the tmux hook command) logs one line, exits 0, no tmux call
    fn au54_ws_snapshot_logs_one_line_and_makes_no_tmux_call() {
        let (h, sock) = quoted_socket_harness();
        up_state(&h, &[], &mut |w| w.tmux_socket = Some(sock.clone()));
        let before = state_bytes(&h);
        let o = Guarded::spawn(h.zygo(&["ws", "snapshot", "--ws-id", WS_ID, "--event", "after-split-window"], &[]))
            .output("ws snapshot");
        assert_eq!((o.status.code(), text(&o.stderr).as_str()), (Some(0), ""));
        assert!(!h.root().join("tmux-called").exists());
        let lines = errors_log(&h);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].ends_with(&format!("snapshot (after-split-window): {AU54_REASON}")), "{lines:?}");
        assert_eq!(state_bytes(&h), before);
    }
    ```
  - In `crates/zygo/tests/session_verbs.rs`, after `au105_cli_verb_runs_the_pending_snapshot_inline_after_the_write` (`:809-828`), add:
    ```rust
    /// FIX-30, AU54, AU105, C6: the CLI verb mirror (`verbs.rs:257-258`) with a saved socket that fails AU54: the verb's
    /// normal reply and record write, one `errors.log` line from the `snapshot.pending` check, no tmux call.
    #[cfg(unix)]
    #[test]
    fn au54_cli_verb_mirror_logs_one_line_and_makes_no_tmux_call() {
        let d = home();
        let h = d.path();
        let s = h.join("s");
        mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
        let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::SID))], Some(&h.join("so'ck").join("tmux.sock")));
        std::fs::write(store.dir().join("snapshot.pending"), b"").unwrap();
        std::fs::write(h.join("tmux-go"), b"").unwrap();
        let vars = mf::fake_tmux(h, &store.dir().join("workspace.json"));
        let vars = as_vars(&vars);
        let env = Env { session: Some(mf::SID), dir: Some(&s), vars: &vars, ..Default::default() };
        assert_eq!(cli(h, &env, "units off"), (0, "units off for this session\n".into(), String::new()));
        assert!(mf::tmux_calls(h).is_empty(), "{:?}", mf::tmux_calls(h));
        assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(json!({ "work_units": false, "guards": {} })));
        let log = std::fs::read_to_string(store.dir().join("errors.log")).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].ends_with(
                "snapshot: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it"
            ),
            "{lines:?}"
        );
        assert!(!store.dir().join("snapshot.pending").exists());
    }
    ```
    (The reply and the `overrides` value are those of `au105_cli_verb_runs_the_pending_snapshot_inline_after_the_write`, `:820`, and `au105_slash_verb_starts_the_pending_snapshot_as_a_detached_child`, `:865`.)
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test workspace_member au54_` fails in both tests: `root/tmux-called` exists (the snapshot `owner` call ran `bin/tmux`). `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test session_verbs au54_` fails: `mf::tmux_calls(h)` is not empty.

- [x] **3. Code (green): the AU54 check in `snapshot::attempt`.** In `crates/zygo-workspace/src/snapshot.rs`:
  - Add the import `use crate::repair::check_saved_socket;` after line 13.
  - In `fn attempt(store: &Store, mux: &dyn Multiplexer, event: Option<&str>, force: bool, wait: Duration) -> Attempt`, right after the block `if mux.socket() != Some(state_socket) { return done(SkipReason::Changed); }` (`:585-587`), add:
    ```rust
    // FIX-30 (AU54): a saved socket that fails the quote rule gets no tmux call, on every read; one log line (C6).
    if let Err(e) = check_saved_socket(&ws) {
        log(store, event, e.to_string().trim_start_matches("zygo: "));
        return done(SkipReason::Logged);
    }
    ```
  - In the doc comment of `attempt` (`:565-568`), after "gives `Skipped(Changed)` before any tmux call (round 2 M4, round 3 D3).", add: "A saved socket that fails AU54 gives one log line and `Skipped(Logged)` before any tmux call (FIX-30)."
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test snapshot` passes (the step 1 test and every earlier test); `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test workspace_member au54_` passes both step 2 tests; `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test session_verbs` passes.

- [x] **4. Test (red): `up` and `up --all` refuse a `$TMUX` socket that fails AU54 before its first tmux call.** In `crates/zygo-workspace/tests/up.rs`, after `repair_refuses_a_saved_socket_that_holds_a_quote` (`:2649-2663`), add:
  ```rust
  #[test] // FIX-30, AU54, W§8.1 steps 5-6: no saved socket; the `$TMUX` socket with a quote is refused before its first tmux call
  fn au54_up_refuses_a_tmux_env_socket_with_a_quote_before_its_first_use() {
      let _g = serial();
      // `building`: the first call on the `$TMUX` socket is the repair's session check (step 5); `down`: step 6
      for phase in [Phase::Building, Phase::Down] {
          let td = TempDir::new().unwrap();
          let t = &canon(td.path());
          let (mut o, store, sock) = quoted_socket_state_in(t, phase);
          set(&store, |ws| ws.tmux_socket = None);
          o.tmux_env = Some(OsString::from(format!("{},1,0", sock.display())));
          let before = raw(&store);
          assert_eq!(run(&o), (2, String::new(), format!("{AU54}\n")), "{phase:?}");
          assert_eq!(raw(&store), before, "{phase:?}");
          assert_eq!(tmux_log(t), ["-u -V"], "{phase:?}");
      }
  }

  #[test] // FIX-30, AU54: `up --all` skips a workspace whose server is the `$TMUX` socket with a quote
  fn au54_up_all_skips_a_tmux_env_socket_with_a_quote() {
      let _g = serial();
      let td = TempDir::new().unwrap();
      let t = &canon(td.path());
      let (mut o, store, sock) = quoted_socket_state_in(t, Phase::Up);
      set(&store, |ws| ws.tmux_socket = None);
      o.tmux_env = Some(OsString::from(format!("{},1,0", sock.display())));
      let before = raw(&store);
      let (mut out, mut e) = (Vec::new(), Vec::new());
      let code = up::testing::run_all_with(&o, &mut out, &mut e);
      let (out, err) = (text(out), text(e));
      assert_eq!((code, out.as_str()), (1, ""), "{err}");
      let reason = AU54.trim_start_matches("zygo: ");
      assert_eq!(err, format!("zygo: app skipped: {reason}\nzygo: up --all found no workspace to restore\n"));
      assert_eq!(tmux_log(t), ["-u -V"]);
      assert_eq!(raw(&store), before);
  }
  ```
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test up au54_` fails in both tests: the tmux log holds a `has-session` call on the `$TMUX` socket (step 5 `repair.rs:92`, step 6 `up.rs:994`, `up --all` `up.rs:1542`).

- [x] **5. Code (green): `check_server` in `up.rs`; steps 5 and 6 and `session_check` call it.** In `crates/zygo-workspace/src/up.rs`:
  - After `pub fn server` (`:274-277`), add:
    ```rust
    /// FIX-30 (AU54): the socket that `mux` passes to tmux with `-S`, saved or from `$TMUX` (D18), must pass
    /// [`check_path`] before the first tmux call on it; no socket (tmux's own choice) passes.
    pub(crate) fn check_server(mux: &dyn Multiplexer) -> Result<(), UnsafePath> {
        mux.socket().map_or(Ok(()), |s| check_path(s).map(|_| ()))
    }
    ```
    (`check_path`, `Multiplexer` and `UnsafePath` are already imported: `up.rs:26-27`, and `UnsafePath` is used at `:1264`.)
  - Step 5, after `let mux = server(&opts.tmux, cur.tmux_socket.as_deref(), opts.tmux_env.as_deref());` (`:981`), add:
    ```rust
    check_server(&mux).map_err(|e| Exit { code: 2, message: e.to_string() })?;
    ```
  - Step 6, after `let mux6 = server(&opts.tmux, saved_socket.as_deref(), opts.tmux_env.as_deref());` (`:993`), add:
    ```rust
    // FIX-30 (AU54): with no saved socket this is the socket of `$TMUX`, before its first tmux call.
    check_server(&mux6).map_err(|e| Exit { code: 2, message: e.to_string() })?;
    ```
  - `session_check`, after `let mux = server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());` (`:1541`), add:
    ```rust
    // FIX-30 (AU54): with no saved socket this is the socket of `$TMUX`, before its first tmux call.
    if let Err(e) = check_server(&mux) {
        return Found::Skip(e.to_string().trim_start_matches("zygo: ").to_string());
    }
    ```
  - Step 8 comment (`:1065-1066`): after "a `$TMUX` socket gets the AU62 check before it is written." add " Its AU54 check ran at step 6 (FIX-30)."
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test up --test up_all` passes (the step 4 tests and every earlier test; tmux cases run or print their skip reason).

- [x] **6. Test (red): `down` refuses a `$TMUX` socket that fails AU54.** In `crates/zygo-workspace/tests/down.rs`, after `down_refuses_a_saved_socket_that_holds_a_quote_before_the_repair` (`:2276-2297`), add:
  ```rust
  #[test] // FIX-30, AU54: no saved socket; step 3 refuses the `$TMUX` socket with a quote before its session check
  fn au54_down_refuses_a_tmux_env_socket_with_a_quote() {
      let _s = serial();
      let t = TempDir::new().unwrap();
      let t = t.path();
      let mut o = opts(t, &fake_tmux(t));
      let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
      no_socket(&store);
      o.tmux_env = Some(std::ffi::OsString::from(format!("{},1,0", t.join("so'ck").join("s").display())));
      let before = raw(&store);
      let got = run(&o, &hooks());
      assert_eq!(tmux_log(t), ["-u -V"]);
      assert_eq!(got, (2, String::new(), format!("{AU54}\n")));
      assert_eq!(raw(&store), before);
  }

  #[test] // FIX-30, AU54: the detach rule refuses the `$TMUX` socket with a quote before its `display` call (down.rs:510-519)
  fn au54_down_detach_rule_refuses_a_tmux_env_socket_with_a_quote() {
      let _s = serial();
      let t = TempDir::new().unwrap();
      let t = t.path();
      let _g = Detached::new(t);
      let (mut o, store) = detach_case(t);
      no_socket(&store);
      o.tmux_pane = Some("%1".into());
      o.tmux_env = Some(std::ffi::OsString::from(format!("{},1,0", t.join("so'ck").join("s").display())));
      let before = raw(&store);
      assert_eq!(run(&o, &hooks()), (2, String::new(), format!("{AU54}\n")));
      assert_eq!(raw(&store), before);
      assert_eq!(tmux_log(t), ["-u -V"]);
      assert!(!t.join("child.pid").exists(), "a detached down was started");
  }
  ```
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test down au54_` fails in both tests: the tmux log holds `has-session` (`down.rs:1019`) or `display-message` (`down.rs:519`) on the `$TMUX` socket.

- [x] **7. Code (green): `check_server` at each `$TMUX` fallback of `down.rs`.** In `crates/zygo-workspace/src/down.rs`:
  - Line 32: `use crate::up::{self, check_server, not_owned, preflight, socket_is_private, unreachable, Exit};`
  - `detach_wanted`, after `let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());` (`:518`), add:
    ```rust
    // FIX-30 (AU54): with no saved socket this is the socket of `$TMUX` (`env_server`, `:510`), before its first call.
    check_server(&mux).map_err(|e| Exit { code: 2, message: e.to_string() })?;
    ```
  - `guard`, after `let mux = up::server(&opts.tmux, socket, opts.tmux_env.as_deref());` (`:563`), add `check_server(&mux).map_err(|e| Exit { code: 2, message: e.to_string() })?;`.
  - `any_session`, replace `match up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref()).session_exists(&ws.tmux_session) {` (`:603`) with:
    ```rust
    let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
    check_server(&mux).map_err(|e| Exit { code: 2, message: e.to_string() })?;
    match mux.session_exists(&ws.tmux_session) {
    ```
  - `prepare`, after `let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());` (`:1003`), add:
    ```rust
    // FIX-30 (AU54): with no saved socket this is the socket of `$TMUX`, before the repair and the session check use it.
    if let Err(e) = check_server(&mux) {
        self.e(&e.to_string());
        return self.fail(2);
    }
    ```
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test down` passes (the step 6 tests and every earlier test).

- [x] **8. Test (red): `ls`, `ws forget` and `ws forget-pane`.** In `crates/zygo-workspace/tests/admin.rs`:
  - After `const TIMED_OUT` (`:35`), add:
    ```rust
    /// The AU54 line (`mux/mod.rs:697-698`).
    const AU54: &str =
        "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";
    ```
  - After `fn private_dir` (`:172-176`), add:
    ```rust
    /// FIX-30: the socket `<t>/so'ck/s` in a 0700 folder: it passes AU62 and fails AU54.
    fn quoted_socket(t: &Path) -> PathBuf { private_dir(&t.join("so'ck")).join("s") }

    /// FIX-30: the line of a `forget` that a saved socket refuses by AU54 (W§12 `ws forget`).
    fn forget_au54_line(p: &Paths, id: &str) -> String {
        format!("{AU54}; remove {} by hand to forget this workspace\n", p.workspaces_dir().join(id).display())
    }
    ```
  - At the end of the file, add:
    ```rust
    // ---- FIX-30: a tmux socket that fails AU54 ----

    #[test] // FIX-30, AU54, W§12 `ls`: the row is unreachable and no tmux call runs
    fn au54_ls_shows_a_quoted_saved_socket_as_unreachable_with_no_tmux_call() {
        let _s = serial();
        let td = TempDir::new().unwrap();
        let t = td.path();
        let p = paths(t);
        let sock = quoted_socket(t);
        ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
            ws.phase = Phase::Up;
            ws.tmux_socket = Some(sock.clone());
        });
        let (rows, calls, _) = fake_list(&p, vec![ListFake::new(&sock, SessionAnswer::Exists, Duration::ZERO)], false);
        assert_eq!(states(&rows), [("app".to_string(), ListState::Unreachable)]);
        assert!(calls.is_empty(), "{calls:?}");
    }

    #[test] // FIX-30, AU54, W§12 `ws forget`, D17: exit 2, the line names the state folder, nothing deleted; other errors exit 1
    fn au54_forget_refuses_a_quoted_saved_socket_and_names_the_state_folder() {
        let _s = serial();
        let td = TempDir::new().unwrap();
        let t = td.path();
        let p = paths(t);
        let tmux = answer_tmux(t);
        let sock = quoted_socket(t);
        ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
            ws.phase = Phase::Up;
            ws.tmux_socket = Some(sock.clone());
        });
        let bytes = state_bytes(&p, ID);
        assert_eq!(forget_run(&p, &tmux, None, "app"), (2, String::new(), forget_au54_line(&p, ID)));
        assert_eq!(state_bytes(&p, ID), bytes);
        assert_eq!(tmux_log(t), ["-u -V"]); // only the `no_tmux` probe, no call on the socket
        let _ = fs::remove_file(t.join("tmux.log"));
        // the raw JSON socket of a state that cannot be read
        put_raw(&p.workspaces_dir().join(OTHER), &invalid_doc(OTHER, json!("bad"), Some(&sock)));
        assert_eq!(forget_run(&p, &tmux, None, OTHER), (2, String::new(), forget_au54_line(&p, OTHER)));
        assert!(p.workspaces_dir().join(OTHER).join("workspace.json").exists());
        assert_eq!(tmux_log(t), ["-u -V"]);
        // every other refusal still exits 1
        assert_eq!(forget_run(&p, &tmux, None, "nosuch"), (1, String::new(), "zygo: no workspace named nosuch\n".to_string()));
    }

    #[test] // FIX-30, AU54, W§12 `ws forget`: no saved socket; the `$TMUX` socket with a quote: exit 2, the AU54 line
    fn au54_forget_refuses_a_quoted_tmux_env_socket_with_exit_2() {
        let _s = serial();
        let td = TempDir::new().unwrap();
        let t = td.path();
        let p = paths(t);
        let tmux = answer_tmux(t);
        ws_state(&p, ID, "app", &t.join("r/app"), |ws| ws.phase = Phase::Up);
        let env = format!("{},1,0", t.join("so'ck").join("s").display());
        let bytes = state_bytes(&p, ID);
        assert_eq!(forget_run(&p, &tmux, Some(&env), "app"), (2, String::new(), format!("{AU54}\n")));
        assert_eq!(state_bytes(&p, ID), bytes);
        assert_eq!(tmux_log(t), ["-u -V"]);
    }

    #[test] // FIX-30, AU54, W§12 `ws forget`: a quoted socket whose folder is gone gets no tmux call, so no check: forgotten
    fn au54_forget_with_a_quoted_socket_whose_folder_is_gone_succeeds() {
        let _s = serial();
        let td = TempDir::new().unwrap();
        let t = td.path();
        let p = paths(t);
        let tmux = answer_tmux(t);
        let gone = t.join("so'ck").join("s"); // `so'ck` is not made
        ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
            ws.phase = Phase::Up;
            ws.tmux_socket = Some(gone.clone());
        });
        assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
        put_raw(&p.workspaces_dir().join(OTHER), &invalid_doc(OTHER, json!("bad"), Some(&gone)));
        assert_eq!(forget_run(&p, &tmux, None, OTHER), (0, format!("zygo: forgot {OTHER}\n"), String::new()));
        assert!(tmux_log(t).is_empty(), "{:?}", tmux_log(t));
    }

    #[test] // FIX-30, AU54, W§12 `ws forget-pane`, W§7.4 step 1: the record goes; the pending snapshot logs one line, no tmux call
    fn au54_forget_pane_with_a_quoted_saved_socket_removes_the_record_and_the_snapshot_logs() {
        let _s = serial();
        let PaneSetup { td, p, tmux, id, store, plain, .. } = pane_setup();
        let t = td.path();
        let sock = quoted_socket(t);
        store
            .update(S5, &mut |ws| {
                ws.phase = Phase::Up;
                ws.tmux_socket = Some(sock.clone());
                Ok(())
            })
            .unwrap();
        fs::write(store.dir().join("snapshot.pending"), "").unwrap();
        assert_eq!(
            pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
            (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
        );
        assert!(!store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb"));
        assert!(!cache(&p, &id).join("results/kbbbbbbbb").exists());
        let log = fs::read_to_string(store.dir().join("errors.log")).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].ends_with(&format!("snapshot: {}", AU54.trim_start_matches("zygo: "))), "{lines:?}");
        assert!(!store.dir().join("snapshot.pending").exists());
        assert!(tmux_log(t).is_empty(), "{:?}", tmux_log(t));
    }
    ```
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test admin au54_` fails in `au54_ls_...` (`calls` holds `session_exists`), `au54_forget_refuses_a_quoted_saved_socket_...` (exit 0, `zygo: forgot app`), `au54_forget_refuses_a_quoted_tmux_env_socket_...` (exit 0) and `au54_forget_pane_...` (the tmux log holds the snapshot's `show-options` call, and `errors.log` has no AU54 line). `au54_forget_with_a_quoted_socket_whose_folder_is_gone_succeeds` passes before and after the fix: it guards item 3 (no check without a tmux call).

  Note: `au54_forget_pane_...` passes once step 3 is done (the snapshot check). If the implementer runs step 8 after step 3, the red check of this one test is the run before step 3; record which order ran.

- [x] **9. Code (green): the typed `forget` refusal and the `ls` check in `admin.rs`.** In `crates/zygo-workspace/src/admin.rs`:
  - Line 21: `use crate::mux::{check_path, session_target, Multiplexer, MuxError, SessionAnswer, BOUND, UNSAFE_PATH_MESSAGE};`
  - After line 21, add `use crate::repair::check_saved_socket;`
  - Line 24: `use crate::up::{check_server, find_checkout, fire, member_live, server, socket_is_private, Exit, GIT_BOUND};`
  - After `fn say` (`:161`), add:
    ```rust
    /// FIX-30: the error of `forget`: the AU54 refusal (exit 2, as `up`, `down` and `attach`) or any other line (exit 1).
    enum ForgetError {
        Unsafe(String),
        Line(String),
    }

    impl From<String> for ForgetError {
        fn from(line: String) -> ForgetError { ForgetError::Line(line) }
    }

    impl ForgetError {
        fn text(&self) -> &str {
            match self {
                ForgetError::Unsafe(t) | ForgetError::Line(t) => t,
            }
        }
    }

    /// FIX-30: the exit code of a `forget` error: 2 for the AU54 refusal, else 1.
    fn exit_code(e: &ForgetError) -> i32 {
        match e {
            ForgetError::Unsafe(_) => 2,
            ForgetError::Line(_) => 1,
        }
    }

    /// FIX-30 (AU54, W§12 `ws forget`): the refusal of a socket that fails AU54 right before the first tmux call. A saved
    /// socket also refuses `up` and `down`, so the line names the state folder to remove by hand (escaped, C6); the socket
    /// of `$TMUX` is not the state's fault, so its line is the AU54 line alone.
    fn unsafe_socket(store: &Store, saved: bool) -> ForgetError {
        if saved {
            ForgetError::Unsafe(format!(
                "{UNSAFE_PATH_MESSAGE}; remove {} by hand to forget this workspace",
                esc(&store.dir().display().to_string())
            ))
        } else {
            ForgetError::Unsafe(UNSAFE_PATH_MESSAGE.to_string())
        }
    }
    ```
  - `fn ask` (`:241`): after the `if no_tmux { return Seen::Unknown("no tmux"); }` block, add:
    ```rust
    // FIX-30 (AU54): the saved socket before any use; `ls` never fails, so the row is `unreachable`, with no call.
    if check_saved_socket(ws).is_err() {
        return Seen::Unreachable;
    }
    ```
    In its doc comment (`:239-240`), change "on a failed socket, on a socket whose folder is missing or that fails AU62" to "on a socket that fails AU54, on a failed socket, on a socket whose folder is missing or that fails AU62".
  - `pub fn forget` (`:543`): replace the `Err(line) => { say(err, &line); 1 }` arm with:
    ```rust
    Err(e) => {
        say(err, e.text());
        exit_code(&e)
    }
    ```
    Doc comment (`:541-542`): change "or exit 1 with one line on `err`" to "or exit 1 with one line on `err` (exit 2 for the AU54 refusal, FIX-30)".
  - `fn forget_run` (`:557`): signature `fn forget_run(paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, arg: &str) -> Result<String, ForgetError>`. Changes: `:562` `Found::Link(id) => return forget_link(paths, id).map_err(ForgetError::Line),`; `:569` `Kind::Link => return forget_link(paths, &id).map_err(ForgetError::Line),`; each `return Err(<String>)` at `:570`, `:574`, `:575`, `:579`, `:580` (`NEWER.into()`), `:581` becomes `return Err(ForgetError::Line(<String>))`; `:591` `remove(paths, &store, &id, &label, op).map_err(ForgetError::Line)`. The `?` at `:558` and `:565` convert through `From<String>`.
  - `fn check_readable` (`:612`): signature `-> Result<(), ForgetError>`. Keep the order: live launcher, then `parent_missing` early `Ok` and the AU62 check (`:617-622`), then `if no_tmux(tmux) { return Ok(()); }` (`:623-625`). Then:
    ```rust
    let mux = server(tmux, ws.tmux_socket.as_deref(), tmux_env);
    // FIX-30 (AU54): the socket of the calls below, saved or from `$TMUX`, right before the first tmux call.
    if check_server(&mux).is_err() {
        return Err(unsafe_socket(store, ws.tmux_socket.is_some()));
    }
    ```
    Each `Err(<String>)` at `:614`, `:629`, `:631`, `:633` becomes `Err(ForgetError::Line(<String>))`; the `?` at `:621` converts through `From<String>`. Doc comment (`:611`): "D15: a live launcher, a socket that fails AU62, the workspace's own session, or a tmux error refuses; FIX-30: the socket of the session check (saved or from `$TMUX`) that fails AU54 refuses with exit 2, right before that call."
  - `fn check_invalid` (`:639`): signature `-> Result<(), ForgetError>`. Each `Err(<String>)` at `:648`, `:651`, `:655`, `:664`, `:671`, `:672` becomes `Err(ForgetError::Line(<String>))`; the `?` at `:646` and `:659` convert through `From<String>`. After `if gone || no_tmux(tmux) { return Ok(()); }` (`:666-668`), before the `match server(tmux, Some(&sock), tmux_env).session_exists(...)` (`:669`), add:
    ```rust
    // FIX-30 (AU54): the raw socket is not a `Workspace`, so `check_path`, which `check_saved_socket` wraps; right before
    // the first tmux call.
    if check_path(&sock).is_err() {
        return Err(unsafe_socket(store, true));
    }
    ```
  - `forget_pane`, `forget_pane_run`: no change.
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked --test admin` passes (the five step 8 tests and every earlier test).

- [x] **10. Test (red): the hand start with a saved socket, and with a `$TMUX` socket, that fails AU54.** In `crates/zygo/tests/workspace_member.rs`, in the FIX-30 section of step 2, add:
  ```rust
  #[test] // FIX-30, AU54, W§8.4, C6: a saved socket with a quote: the not-saved line, a v1 launch, no tmux call, no write
  fn au54_a_hand_start_with_a_quoted_saved_socket_is_a_v1_launch_with_the_not_saved_line() {
      let (h, sock) = quoted_socket_harness();
      up_state(&h, &[], &mut |w| w.tmux_socket = Some(sock.clone()));
      let before = state_bytes(&h);
      let (out, env) = h.stub(Agent::ClaudeCode, "a");
      let t = OsString::from(format!("{},1,0", sock.display()));
      let mut extra = pairs(&env);
      extra.push(("TMUX", t.as_os_str()));
      let o = Guarded::spawn(hand(&h, "%0", &["claude"], &extra)).output("hand start");
      assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
      let err = text(&o.stderr);
      let want = format!("{NOT_SAVED}{AU54_REASON}");
      assert_eq!(err.lines().filter(|l| *l == want).count(), 1, "{want}\n{err}");
      assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
      assert!(!h.root().join("tmux-called").exists());
      assert_eq!(state_bytes(&h), before);
  }

  #[test] // FIX-30, AU54, W§8.4, C6: no state and a `$TMUX` socket with a quote: the not-saved line, a v1 launch, no tmux call
  fn au54_a_hand_start_on_a_quoted_tmux_socket_is_a_v1_launch_with_the_not_saved_line() {
      let (h, sock) = quoted_socket_harness();
      let (out, env) = h.stub(Agent::ClaudeCode, "a");
      let t = OsString::from(format!("{},1,0", sock.display()));
      let mut extra = pairs(&env);
      extra.push(("TMUX", t.as_os_str()));
      let o = Guarded::spawn(hand(&h, "%0", &["claude"], &extra)).output("hand start");
      assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
      let err = text(&o.stderr);
      let want = format!("{NOT_SAVED}{AU54_REASON}");
      assert_eq!(err.lines().filter(|l| *l == want).count(), 1, "{want}\n{err}");
      assert!(stub_env(&out).get("ZYGO_WORKSPACE").is_none());
      assert!(!h.root().join("tmux-called").exists());
  }
  ```
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test workspace_member au54_a_hand_start` fails in both: no AU54 line, and `root/tmux-called` exists (the `display` call at `member.rs:869` ran; `socket_is_private` passes, because `root/so'ck` has mode 0700).

- [x] **11. Code (green): `check_saved_socket` is `pub`; the hand start checks both sockets.**
  - `crates/zygo-workspace/src/repair.rs:64`: `pub fn check_saved_socket(ws: &Workspace) -> Result<(), UnsafePath> {` (body unchanged). Doc comment `:60-63`: after "`zygo attach`, `down` and `repair`." add " FIX-30: also `zygo ls` (the row is `unreachable`), the snapshot (W§7.4 step 1) and the hand start of the crate `zygo` (W§8.4), so it is `pub`."
  - `crates/zygo/src/cmd/member.rs:22`: `use zygo_workspace::mux::{check_path, Multiplexer, UNSAFE_PATH_MESSAGE};`
  - `member.rs:23`: `use zygo_workspace::{repair, snapshot, up};`
  - `member.rs`, in `hand_start`, right after `let read = store.read();` (`:852`) and before the `if let Ok(Some(ws)) = &read && ws.tmux_socket.as_deref() != Some(sock.as_path())` block, add:
    ```rust
    // FIX-30 (AU54): the saved socket before any use; the compare below prints it.
    if let Ok(Some(ws)) = &read
        && let Err(e) = repair::check_saved_socket(ws)
    {
        not_saved(e.to_string().trim_start_matches("zygo: "));
        return HandStart::V1;
    }
    ```
  - `member.rs`, right before `if let Err(t) = up::socket_is_private(&sock, false) {` (`:864`), add:
    ```rust
    // FIX-30 (AU54): the socket of `$TMUX` before its first use (AU62, then `display` at the next lines). A saved
    // socket reaches here only when it equals `sock` and passed the check above; this one matters for a state that is
    // missing or cannot be read.
    if check_path(&sock).is_err() {
        not_saved(UNSAFE_PATH_MESSAGE.trim_start_matches("zygo: "));
        return HandStart::V1;
    }
    ```
  - Doc comment of `hand_start` (`member.rs:833-834`): change "except a state whose `tmux_socket` is not the socket of `$TMUX` and a socket that fails AU62" to "except a state whose `tmux_socket` fails AU54 or is not the socket of `$TMUX`, and a socket of `$TMUX` that fails AU54 or AU62".
  Check: `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test workspace_member` passes (the five FIX-30 tests and every earlier test; tmux cases run or print their skip reason).

- [x] **12. Spec text.** In `docs/specs/2026-11-20-workspaces-design.md`, make these exact replacements:
  1. AU54 row (`:195`): replace the text from "The saved `tmux_socket` gets the same check in every phase," through "and `up` and `down` print that refusal once and exit 2." with: "Every socket that zygo passes to tmux with `-S` gets the same check before its first tmux call: the saved `tmux_socket`, in every phase and before its AU62 check, because the dead-pane lines (W§8.1 step 12) and the line of AU100 quote it; and, when no socket is saved, the socket of `$TMUX` that the call falls back to (D18). By path: `up` (W§8.1 step 5, before steps 6 and 13; the `$TMUX` socket before the first tmux call of step 5 or 6), `zygo attach` and `down` (W§9.3 steps 2 and 3; the `$TMUX` socket before each tmux call of the detach rule, the guard, the session check and step 3) exit 2 with the line below; `up --all` skips that workspace with `zygo: <name> skipped: <the line below without its "zygo: ">` (W§8.1); `repair` refuses it before any tmux call, and `up` and `down` print that refusal once and exit 2; `zygo ws forget` checks the socket of its session check right before that call and exits 2 (W§12); `zygo ls` shows the row `unreachable` with no tmux call; a hand start prints `zygo: this agent is not saved by the workspace: <the line below without its "zygo: ">` and goes on as a v1 launch (W§8.4); a snapshot writes one `errors.log` line with that text and makes no tmux call (W§7.4 step 1): `zygo ws snapshot` exits 0, and a snapshot that the member wrapper, a verb, `up`, `down` or `zygo ws forget-pane` runs returns to its caller, which goes on."
  2. W§7.4 step 1 (`:326`): replace "If `tmux_socket` is not the socket that the multiplexer of this snapshot is bound to (`Multiplexer::socket`), exit 0 with no tmux call." with "If `tmux_socket` is not the socket that the multiplexer of this snapshot is bound to (`Multiplexer::socket`), exit 0 with no tmux call. If `tmux_socket` fails AU54, write one line to `errors.log` (the AU54 text without `zygo: `) and stop with no write and no tmux call: `zygo ws snapshot` exits 0; a snapshot that the member wrapper, a verb, `up`, `down` or `zygo ws forget-pane` runs returns to its caller."
  3. W§8.1 step 6 (`:362`): replace "Check the tmux session `zygo-<name>` on `tmux_socket` (AU62), without `lock`." with "Check the tmux session `zygo-<name>` on `tmux_socket` (AU62), without `lock`; when no socket is saved, on the server of `$TMUX`, whose socket gets the AU54 check before this call (exit 2)."
  4. W§8.4 (`:403`): replace "when it exists, its `tmux_socket` must be the socket of `$TMUX`; then the socket gets the AU62 check." with "when it exists, its `tmux_socket` must pass AU54 and be the socket of `$TMUX`; then the socket of `$TMUX` gets the AU54 check and the AU62 check."
  5. W§8.4 (`:408`): replace "except a state whose `tmux_socket` is not the socket of `$TMUX`, or a socket that fails AU62." with "except a state whose `tmux_socket` fails AU54 or is not the socket of `$TMUX`, or a socket of `$TMUX` that fails AU54 or AU62."; and replace "`the workspace state names tmux server <a>, not <b>` (`<a>` is `none` when unset);" with "the AU54 text without its `zygo: ` (no socket is printed); `the workspace state names tmux server <a>, not <b>` (`<a>` is `none` when unset);".
  6. W§12 row `zygo ls` (`:560`): replace "a socket that fails AU62, or no saved socket" with "a socket that fails AU54 or AU62 (no tmux call), or no saved socket".
  7. W§12 row `zygo ws forget` (`:563`): after "and for a name that two states share (the message lists their ids)." insert " Right before its first tmux call (the session check), `forget` checks the socket of that call by AU54: the saved `tmux_socket` (also the raw one of a state that cannot be read), else the socket of `$TMUX`. A failure refuses with exit 2 and nothing deleted. For a saved socket the line is `zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it; remove <state folder> by hand to forget this workspace` (the path escaped, C6), because `up` and `down` refuse that state too; for the socket of `$TMUX` it is the AU54 line alone. A socket whose folder is missing, or no tmux, gives no tmux call, so no AU54 check: `forget` goes on."
  8. W§12 row `zygo ws forget-pane` (`:564`): after "(`zygo: the state folder of workspace <id> is a link; it is not changed`)." insert " A saved `tmux_socket` that fails AU54 does not refuse `forget-pane`: the record goes, and the `snapshot.pending` check writes its one `errors.log` line with no tmux call (W§7.4 step 1)."
  9. Change log: add this row at the end of the table (after `:1026`):
     `| 2026-11-23 | AU54, W§7.4 step 1, W§8.1 step 6, W§8.4, W§12 (rows `zygo ls`, `zygo ws forget`, `zygo ws forget-pane`) | author's call (FIX-30 plan, plan review items 1-10) | Every socket that zygo passes to tmux, saved or from `$TMUX`, gets the AU54 check before its first tmux call: `up`, `down` and `ws forget` exit 2 (the `forget` line names the state folder to remove by hand for a saved socket), `up --all` skips, `ls` shows `unreachable`, a hand start goes on as a v1 launch with the not-saved line, and a snapshot logs one line with no tmux call (C6, W-C1); `ws forget-pane` is not refused. |`
  Check: `git diff -- docs/specs/2026-11-20-workspaces-design.md` shows only the nine changes above. If E2 changed one of these lines first, merge the FIX-30 words into the E2 text and record that in the results.

- [ ] **13. Acceptance and gates.**
  - `CARGO_TARGET_DIR=T cargo test -p zygo-workspace --locked`: all pass (tmux cases run or print their skip reason), including `au54_a_saved_socket_with_a_quote_logs_one_line_and_makes_no_tmux_call`, `au54_up_refuses_a_tmux_env_socket_with_a_quote_before_its_first_use`, `au54_up_all_skips_a_tmux_env_socket_with_a_quote`, `au54_down_refuses_a_tmux_env_socket_with_a_quote`, `au54_down_detach_rule_refuses_a_tmux_env_socket_with_a_quote`, `au54_ls_shows_a_quoted_saved_socket_as_unreachable_with_no_tmux_call`, `au54_forget_refuses_a_quoted_saved_socket_and_names_the_state_folder`, `au54_forget_refuses_a_quoted_tmux_env_socket_with_exit_2`, `au54_forget_with_a_quoted_socket_whose_folder_is_gone_succeeds`, `au54_forget_pane_with_a_quoted_saved_socket_removes_the_record_and_the_snapshot_logs`.
  - `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test workspace_member`: all pass, including `au54_the_member_wrapper_logs_its_snapshots_and_makes_no_tmux_call`, `au54_ws_snapshot_logs_one_line_and_makes_no_tmux_call`, `au54_a_hand_start_with_a_quoted_saved_socket_is_a_v1_launch_with_the_not_saved_line`, `au54_a_hand_start_on_a_quoted_tmux_socket_is_a_v1_launch_with_the_not_saved_line`.
  - `CARGO_TARGET_DIR=T cargo test -p zygo --locked --test session_verbs`: all pass, including `au54_cli_verb_mirror_logs_one_line_and_makes_no_tmux_call`.
  - `CARGO_TARGET_DIR=T cargo test -p zygo --locked --lib cmd::member`: all pass (the unit tests of `member.rs`).
  - `CARGO_TARGET_DIR=T cargo clippy -p zygo-workspace -p zygo --all-targets --locked -- -D warnings`: exit 0, no warning.
  - Record each command, its exit code and its pass count under "Results" below.

Path to test (acceptance "one test for each path"): contract (1) `ls` `au54_ls_...`; (2) `ws forget` `au54_forget_refuses_a_quoted_saved_socket_...` first part; (3) raw JSON, same test, second part; (4) `forget-pane` `au54_forget_pane_...` (not refused: the snapshot logs; coordinator item 7); (5) member `mux_of` `au54_the_member_wrapper_...`; (6) hand start `au54_a_hand_start_with_a_quoted_saved_socket_...`. Added: `ws snapshot` `au54_ws_snapshot_...`; the snapshot function `au54_a_saved_socket_...`; the verb mirror `au54_cli_verb_mirror_...`; the `$TMUX` fallbacks: `up` steps 5-6 `au54_up_refuses_a_tmux_env_socket_...`, `up --all` `au54_up_all_skips_...`, `down` step 3 `au54_down_refuses_a_tmux_env_socket_...`, the down detach rule `au54_down_detach_rule_...` (`guard` and `any_session` run only after `detach_wanted` on the same targets, `down.rs:1466-1497`, so the detach-rule test covers the first call of that path), `ws forget` `au54_forget_refuses_a_quoted_tmux_env_socket_...`, hand start `au54_a_hand_start_on_a_quoted_tmux_socket_...`; the item 3 guard `au54_forget_with_a_quoted_socket_whose_folder_is_gone_succeeds`; exit 1 kept for other `forget` errors: the `nosuch` case.

### Self-review (after the plan review)

- Spec coverage: AU54 now covers every socket that zygo passes to tmux, saved or from `$TMUX`; each place in the three premise tables either had a check or gets one right before its first tmux call. The saved-socket AU54 check stays before AU62 in `up`, `down`, `attach` and the hand start (`up.rs:977-980`, `down.rs:482-493`); `ws forget` checks right before its tmux call (coordinator item 3), so a missing socket folder or no tmux still forgets. W§8.4: the saved-socket check runs at the state read, before the compare; the `$TMUX` check runs before AU62 and `display`. W§7.4 step 1: the new stop is in step 1, on every read (`attempt` runs for the first read, the retry, the probe run and each `snapshot.pending` check). C6 and W-C1: no hook, wrapper or verb path refuses or changes its exit code; `ws forget-pane` is not refused.
- Step scan: every code step names the file, the place by line and anchor text, and the exact code; every test step gives the test name and its assertions; no placeholder. Each test fails before its code step (the red check names the failing assertion), except `au54_forget_with_a_quoted_socket_whose_folder_is_gone_succeeds`, which is a guard of unchanged behavior and says so, and `au54_forget_pane_...`, whose red check is the run before step 3 (stated in step 8).
- Interface match: `check_saved_socket(ws: &Workspace) -> Result<(), UnsafePath>`, `check_path(path: &Path) -> Result<&str, UnsafePath>`, `Multiplexer::socket(&self) -> Option<&Path>` and the new `check_server(mux: &dyn Multiplexer) -> Result<(), UnsafePath>` are used with those types; `&Tmux` coerces to `&dyn Multiplexer`. `UnsafePath`'s `Display` is `UNSAFE_PATH_MESSAGE` (`mux/mod.rs:704-706`). `ForgetError` has `From<String>`, so the existing `?` on `Result<_, String>` in `forget_run`, `check_readable` and `check_invalid` compile unchanged; `forget_link` and `remove` keep `Result<String, String>` and are mapped. `Store::dir()` is `<workspaces_dir>/<id>` (as `state_bytes` in `tests/admin.rs:226` reads it), so `forget_au54_line` matches `unsafe_socket`. `log` (`snapshot.rs:188`) and `Store::log_error` (`state.rs:609-619`) write `<secs> snapshot[ (<event>)]: <text>`, which the `ends_with` assertions match.
- Proportion: thirteen steps for six contract paths, three found snapshot callers, and the `$TMUX` fallbacks. One check in `snapshot::attempt` replaces five call-site checks; one helper `check_server` serves eight fallback places in `up.rs`, `down.rs` and `admin.rs`. `forget-pane` has no code change.

### Results
