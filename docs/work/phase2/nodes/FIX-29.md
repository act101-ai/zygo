# FIX-29 — up finishes or reports cleanly when a restored pane exits at once

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§8.1 steps 9-10 (the session build). Created 2026-11-23 from the reproduction of the M5 implementer (M5 diff review, controller decision). Problem: reproduction: (1) a harness with a private tmux server, and `zygo up --no-attach` in two repositories; (2) `kill_server`, then wait until no launcher is live; (3) `zygo up --all`, with a `$SHELL` that exits at once. Each restored pane is `ws run … --shell`. The shell exits, so the session and then the server end before `up` finishes the build. The tmux log shows `new-session … EXIT: 0`, then `resize-window -t =zygo-repo:0 …` with `no current target` (EXIT 1); for the second workspace it shows `no server running on <sock>`. `up --all` printed `zygo: tmux: no current target`, `zygo: repo skipped`, `zygo: tmux: no server running on <sock>` and `zygo: repo2 skipped`. The first tmux call after `new-session` (`crates/zygo-workspace/src/up.rs:1121`) is `resize_window` at `up.rs:1143` (`:1141` on the M5 branch head `f8c00c4`; FIX-27 added 2 lines before it). Change: the planner decides the design. Constraints: `up` either finishes the build, or reports one clear line per pane that exited, and it never prints a raw tmux error as the only reason. No workspace state is lost. A dead pane must not make `up` hang. No TTL or time limit may end user work. Edge to M3: M3 also changes `crates/zygo-workspace/src/up.rs`, so M3 runs first. tmux tests use W3's `mux::testing::require_tmux` and a private `-L` socket: they run when tmux 3.2 or later is on `PATH`, skip with a printed reason otherwise, and are `cfg(unix)` (operator, 2026-11-21).
**Files:** `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/src/mux/mod.rs`, `crates/zygo-workspace/src/mux/tmux.rs`, `crates/zygo-workspace/src/repair.rs`, `crates/zygo-workspace/src/snapshot.rs`, `crates/zygo-workspace/src/down.rs`, `crates/zygo-workspace/tests/up.rs`, `crates/zygo-workspace/tests/mux.rs`, `crates/zygo-workspace/tests/snapshot.rs`, `crates/zygo-workspace/tests/down.rs`, `docs/specs/2026-11-20-workspaces-design.md`
**Consumes:** `up::up` (W7), `mux::testing::require_tmux` (W3), `mux::testing::PrivateServer::start` (W3)
**Produces:** `NewWindow`: `pub hold: bool` (field of `mux::NewWindow`)
**Acceptance:** `cargo test -p zygo-workspace --locked` → all pass (tmux cases run or print their skip reason), including a test in `crates/zygo-workspace/tests/up.rs` on a private `-L` socket that reproduces the case with a `$SHELL` that exits at once: `up` finishes the build or prints one clear line per pane that exited, prints no raw tmux error as the only reason, loses no workspace state, and does not hang

## Plan

Planned 2026-11-23 on `worktree-zygo-phase-2` at `0228f29`. Revised 2026-11-23 after the plan review: items 1-13 of the coordinator are applied as author's calls. Added 2026-11-23 from the M5 implementer's reproduction (controller decision).

### Design (author's call): hold each window during the build, release it before the `phase = up` write, report dead panes

**Decision.** `up` holds every window that it builds with the window option `remain-on-exit on`. It sets the option in the same tmux invocation that creates the window. Thus a pane whose command exits at once becomes a dead pane, and the window and the session stay.

At the end of the build the order is:
1. Select the panes and the zoom, then the cwd wait.
2. The final snapshot. It runs during the hold, so it records every pane.
3. `release`: `remain-on-exit off` on each built window.
4. `dead_lines`: read `#{pane_dead}` of each zygo pane.
5. The `phase = up` write.

So a stop of `up` at any point of the hold (crash, error) leaves `phase = building`, and the AU67 repair of the next `up` covers it. `release` and `dead_lines` are best effort for each window and each pane:
- A failure prints one line with context and goes on to the next item.
- A window or pane that is gone (`NoTarget`, `Gone`) prints nothing.
- `up` keeps its report lines and its exit code.

A dead pane stays in the session, with its key, its `panes[]` entry and its saved cwd. Its line names the server: `tmux -S '<socket>' respawn-pane -t <pane id>`. A snapshot keeps the saved cwd of a pane when tmux gives an empty `#{pane_current_path}`, and a dead pane always gives an empty path. A repair from `building` prints the same dead-pane lines. `down` does not list a dead pane in its AU24 "is running" warning.

**Why this option, not "detect the end after each step".** Detection cannot meet "no workspace state is lost". When the session ends during the build, every built pane is lost, also the panes whose commands did not exit. Then `up` must undo or leave `building`, and the user gets no session. Detection also cannot catch a window that closes between `new-window` and `key_first`. There, `display -t =s:<w>` on a missing window gives the value of the current window (trait doc, `crates/zygo-workspace/src/mux/mod.rs:366-369`), so `set_pane_option` writes the key on the wrong pane. The hold removes the race at its cause: no pane can close while `up` builds.

**Evidence: live tmux runs.** All runs are on private `-L` servers. The scripts are in the planner's scratchpad: `e1.sh`-`e5.sh`, and `build32a.sh` for the 3.2a build.
- tmux 3.8 (`/usr/bin/tmux`) and tmux 3.2a give the same result in every check below.
- The 3.2a binary was built 2026-11-23 from the release file `tmux-3.2a.tar.gz` (`https://github.com/tmux/tmux/releases/download/3.2a/tmux-3.2a.tar.gz`). Its SHA-256 `551553a4f82beaa8dadc9256800bcc284d7c000081e47aa6ecbb6ff36eacd05f` was checked before the build, and it is equal to `docs/premises.md:18`.
- The build is `./configure --prefix=…/prefix && make && make install`, with gcc, bison, pkg-config, libevent 2.1.13 and ncurses 6.6 already on the machine. No system package was installed.
- The binary is `/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/tmux32a/prefix/bin/tmux`, and `tmux -V` gives `tmux 3.2a`.

| Check | 3.8 | 3.2a |
|---|---|---|
| E1 baseline: `new-session -d … -- sh -c 'exit 3'` exits 0, then `has-session` | `no server running on …` | `no server running on …` |
| E2 hold in the same invocation: `… ; set-option -w -t =h: remain-on-exit on`, sessions that stay | 50/50; `pane_dead=1`, `pane_dead_status=3`, empty path; global stays `off` | 50/50; same |
| E3 `remain-on-exit off` after the death | dead pane stays | dead pane stays |
| E4 `split-window` in a held window | held, `status=4` | held, `status=4` |
| E5 chained `set-option -w` with no `-t` | goes to window 0 | goes to window 0 |
| E6, E7 `-t =s:{end}`, `-t =s:$` | new window held | new window held |
| F1, G1 window case: `new-window -a -d -t '=g:{end}' -- sh -c 'exit 5' ; set-option -w -t '=g:{end}' remain-on-exit on`, with a gap at 1 and zygo's `run-shell -b` `after-new-window` hook set | 50/50 held; new windows after the highest index; gap stays | 50/50 held; same |
| G2 control: the same `new-window` with no chained item | 0 of 50 windows stay | 0 of 50 windows stay |
| G3 held pane: hook that fires | `pane-died`, not `pane-exited` | `pane-died`, not `pane-exited` |
| F2 dead pane reads | `1\|9\|p1\|` (dead, status, `@zygo-pane`, empty path) | same |
| F3 pane ended by `kill -9` | `pane_dead_status` empty | empty |
| F5 `respawn-pane -t <id>` | runs the start command again | same |
| F6 `kill-session` of a session with only dead panes | exit 0 | exit 0 |
| e4 `resize-window`, `rename-window`, `split-window`, `set-option -p`, `select-layout`, `select-pane`, `resize-pane -Z`, `set-option -w -u window-size`, `select-window` on dead panes | all exit 0 | all exit 0 |
| e5 `#{pane_current_command}` of a dead pane started as `<…>/zygo-exits ws run` | `zygo-exits` | `zygo-exits` |

**Evidence: tmux source (3.2a release files).**
- `cmd-queue.c:704-745`: `cmdq_next` fires each queued item in one loop and leaves it only for an item that waits. A pane exit is handled from a libevent callback outside that loop. So a chained `set-option` runs before tmux handles the exit of the new pane. `new-session` with a command does start the command before the chained items run, but tmux handles its exit only after them.
- `options-table.c:961-965`: `remain-on-exit` has `.scope = OPTIONS_TABLE_WINDOW|OPTIONS_TABLE_PANE`, so a pane gets the option from its window. `server-fn.c:326-330`: tmux reads it only at the exit.
- `cmd-find.c:54`, `:419-425`: `{end}` is `$`, the `RB_MAX` window. `cmd-new-window.c:41`, `:99-100`: `-a` exists.
- `cmd-new-window.c:151`: `after-new-window` is inserted right after the item, so before the chained `set-option`. zygo's hook is `run-shell -b` (`mod.rs:715-727`), which does not make the queue wait.
- `server-fn.c:336-369`: a held pane notifies `pane-died` and prints `Pane is dead (status <n>, <time>)`. `format.c:1693-1716`: `pane_dead`, `pane_dead_status`.

**Evidence: code.**
- `Builder::window_setup` sets `remain-on-exit off` (`crates/zygo-workspace/src/up.rs:478-484`). `build` calls it on the first window in step 10 (`up.rs:1169`), and `add_window` calls it on each later window (`up.rs:524`). So step 10 must keep the hold.
- `Tmux::new_session` already chains owner items after the argument list (`crates/zygo-workspace/src/mux/tmux.rs:326-340`). `Tmux::new_window` has no chain (`tmux.rs:397-414`).
- The snapshot copies the read cwd (`crates/zygo-workspace/src/snapshot.rs:336-347`). So without the empty-path rule, a dead pane loses its saved cwd at the final snapshot (`up.rs:1254-1259`) and at every later snapshot.
- Repair from `building` reads every window index from `list_panes` and calls `window_setup` on each with a `Builder` that does not hold (`crates/zygo-workspace/src/repair.rs:132-145`). With `hold: false`, that sets `remain-on-exit off` on each window of the session (confirmed for item 1; step 7 adds an assertion).
- `down`'s `busy_shells` (`crates/zygo-workspace/src/down.rs:1311-1333`) warns for each non-member pane whose `#{pane_current_command}` is not a shell. A dead `ws run` pane gives its program name (e5), so today `down` prints `… zygo-exits is running; it stops with the session` for a pane that runs nothing.
- The attach hint quotes the socket as `tmux -S '{}' …` (`up.rs:819`). The socket passed the AU54 check (`up.rs:1152`), so it holds no `'`.
- M5's `-e ZYGO_WORKSPACE=<id>` (`tmux.rs:326-331`) is a `new-session` flag before `--`. The hold item comes after the argument list, with the other owner items, so the two do not interact. Its test `first_pane_of_a_new_session_gets_zygo_workspace_in_session_scope_only` (`crates/zygo-workspace/tests/up.rs:2075-2097`) stays green. Later panes get `ZYGO_WORKSPACE` from step 10, which runs before step 11.

**W-C3.** Each `remain-on-exit` write targets a window of `zygo-<name>`: `=zygo-<name>:` in the `new-session` invocation (its only window), `=zygo-<name>:{end}` in the `new-window` invocation, and `=zygo-<name>:<w>` at the release. No global option or hook changes. The tests compare `show-options -g`/`-gw` and `show-hooks -g`/`-gw` before and after (`globals`, `tests/up.rs:278-283`), and the mux live test reads `show-options -gwv remain-on-exit`.

**What the user sees.** `up` (and each workspace of `up --all`, and a repair from `building`) finishes the build. On stderr, after the other step-12 lines, comes one line for each pane that exited while held:
`zygo: pane <key> exited during up (exit <n>); its dead pane stays: tmux -S '<socket>' respawn-pane -t <pane id> starts it again`.
When a signal ended the process, the text in brackets is `no exit status`. A release or read failure prints `zygo: <name>: could not release the build hold of window <w>: <error>` or `zygo: <name>: could not read pane <key>: <error>`, and `up` goes on.

Then comes `zygo: <name> is up` (with `--no-attach`, inside a zygo agent, or `up --all`), the attach, or AU100. The exit code is 0 without attach. With attach it is the code of step 13 (`attach_step`, `up.rs:822-836`). No `zygo: tmux:` line comes from a pane exit during the hold, and `up --all` skips no workspace for it.

In tmux the dead pane shows `Pane is dead (status <n>, <time>)`. When every pane is dead, the session stays with dead panes only. `up` never waits for a dead pane. With a dead pane, the step 12 cwd wait runs its full 2 s (`CWD_WAIT`, `up.rs:43`), because a dead pane reports no folder. That wait is bounded and ends no user work. This plan adds no time limit, no TTL and no clean-up.

**What the workspace state holds after `up`.** `phase = up`, `down_all = false`, `build_plan` and `fresh_members` cleared, as today. `windows[]` and `panes[]` come from the final snapshot taken during the hold: every built pane is in `panes[]` with its planned key and its saved cwd (empty-path rule). Member records are as `ws run` wrote them. `up` writes no member change for a dead pane. A later `down` saves the dead pane as a pane, and the next `up` builds it again.

**Residual risks (in the spec as known limits).**
- **After the release.** A pane that exits after the release closes as usual (W§7.5). When it was the last pane, the session ends before the `phase = up` write. `release` and `dead_lines` print nothing for a gone session, and the write still sets `phase = up`. The next `up` (or `up --all`, AU52) finds the session gone and no live launcher, and restores the layout that the final snapshot saved (W§8.1 steps 6-7). In this case the attach of this `up` fails with tmux's error and returns that code.
- **A user's global `after-new-window` hook.**
  - A hook that creates a window runs before the chained `set-option` (`cmd-new-window.c:151`). Then `{end}` names the user's window, and the new zygo window is not held.
  - A hook that makes the queue wait (a `run-shell` without `-b`) lets tmux handle a pane exit first.
  - zygo adds no code for either case. W7's `@zygo-ws` chain has the same assumption for `after-new-session`.
- **Repair builds without a hold.** A pane that a repair creates and that exits at once closes as usual. The node scope is W§8.1 steps 9-10.

### Premise drift (for the controller)

- **Files.** The coordinator applies these and adds `crates/zygo-workspace/src/down.rs`. The full list:
  - `crates/zygo-workspace/src/up.rs`
  - `crates/zygo-workspace/src/mux/mod.rs`: the `NewWindow.hold` field and the trait docs.
  - `crates/zygo-workspace/src/mux/tmux.rs`: the hold items.
  - `crates/zygo-workspace/src/repair.rs`: `hold: false`, and the dead-pane report.
  - `crates/zygo-workspace/src/snapshot.rs`: the empty-path rule.
  - `crates/zygo-workspace/src/down.rs`: `busy_shells` skips dead panes.
  - `crates/zygo-workspace/tests/up.rs`
  - `crates/zygo-workspace/tests/mux.rs`: 12 `NewWindow` literals, two command-form updates, two new tests.
  - `crates/zygo-workspace/tests/snapshot.rs`: the literal at `:933`, one new test.
  - **New drift, not in the coordinator's message: `crates/zygo-workspace/tests/down.rs`** (the item 5 test).
  - `docs/specs/2026-11-20-workspaces-design.md`
- **Produces.** The contract says `—`. The plan adds the public field `mux::NewWindow.hold`. No other crate builds `NewWindow`: `rg "NewWindow \{" crates/` finds only `crates/zygo-workspace`.
- **Acceptance command.** `cargo test -p zygo-workspace --locked` covers every changed test file. Unchanged.
- **Spec text.** W§8.1 step 11 says `new-window … -n <name>`. The code names later windows with `rename-window` since W3 round 2 (`tmux.rs:410-412`; change log 2026-11-22, AU99). The new step 11 text says `rename-window`.
- **FIX-24 seam (item 10).**
  - FIX-24 no longer changes `src/up.rs` (FIX-24 plan, P10 and D1).
  - FIX-24 adds one test to `tests/up.rs`, directly after `loadout_argument_paths_use_the_pane_folder`. This node adds its new `tests/up.rs` tests at the END of the file.
  - FIX-24 changes `down.rs` only in `drain_tick` (`down.rs:1130-1161`). This node changes `busy_shells` (`down.rs:1311-1333`).
  - Both nodes append one change-log row at the end of the spec. The merge keeps both rows, so no action is needed.

### Consumes / Produces (live signatures)

**Consumes**
- `up::up(opts: &UpOpts) -> i32` (W7; `crates/zygo-workspace/src/up.rs:1307-1310`). The tests call it through:
  - `up::testing::run_with(opts: &UpOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32` (`up.rs:145`)
  - `up::testing::run_all_with(opts: &UpOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32` (`up.rs:148`)
  - `up::testing::hook(dir: &Path, point: &'static str, f: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>)` (`up.rs:126-136`), at the points `after_new_session` (`up.rs:1142`), `before_final_snapshot` (`up.rs:1254`) and the new point `release`.
- `mux::testing::require_tmux() -> Option<PathBuf>` (W3; `crates/zygo-workspace/src/mux/mod.rs:59`).
- `mux::testing::PrivateServer::start() -> PrivateServer` (W3; `mod.rs:95`). The tests also use:
  - `run(&self, args: &[&str]) -> Result<String, MuxError>` (`mod.rs:164`)
  - `wrapper(&self) -> &Path` (`mod.rs:157`)
  - `socket(&self) -> &Path` (`mod.rs:151`)
  - `tmux(&self) -> Tmux` (`mod.rs:160`)
  - `kill_server(&self)` (`mod.rs:170`)

**Produces**
- `NewWindow.hold` — `pub hold: bool` on `mux::NewWindow<'a>` (`mod.rs:232`). With `true`, `new_window` runs `new-window -d -P -F #{window_index} -a -t =<s>:{end} -c <dir> -- <argv> ; set-option -w -t =<s>:{end} -- remain-on-exit on`, then the rename. With `false`, the command line is the same as today.
- `Builder.hold` — crate-private `pub hold: bool` on `up::Builder` (`up.rs:459`). `up` passes `true`, and `repair` passes `false`.
- `Builder::release` — `pub fn release(&self, windows: &[u32], name: &str, err: &mut dyn Write)`.
- `Builder::dead_lines` — `pub fn dead_lines(&self, socket: &Path, name: &str, err: &mut dyn Write) -> Vec<String>`.
- `exited_line` — `pub(crate) fn exited_line(key: &str, status: &str, socket: &Path, pane: &str) -> String` in `up.rs`.
- Test point `release` — fired by `Builder::release` before each window. An `Err(text)` there is that window's release error.

### Implementer build rules

- `export CARGO_TARGET_DIR=/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/FIX-29-target` for every cargo command. Never build under `/tmp`.
- tmux only on private sockets: `PrivateServer` (its own `-S` socket) in tests. For a probe by hand, use `tmux -L fix29-<unique> -f /dev/null …`, and kill only that server.
- Scoped runs only: `cargo test -p zygo-workspace --locked [--test <file>]`. The last step adds one `cargo check` of the whole workspace for the dependents.
- No pipes or filters on command output (no `head`, `tail`, `grep`, `sed`, `awk`, `cut`, `tee`).
- tmux 3.2a for step 10: `/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/tmux32a/prefix/bin/tmux` (built by the planner, hash-checked). Do not change CI.
- Commit trailers per the controller brief. No Claude attribution.

### Steps

- [x] **1. Test: tmux command forms and the live hold (`crates/zygo-workspace/tests/mux.rs`).**
  - In `new_session_sets_the_owner_in_the_same_call` (`tests/mux.rs:1157-1173`), the expected line `want` ends with `… ; set-option -t =zygo-t: -- destroy-unattached off ; set-option -w -t =zygo-t: -- remain-on-exit on`. Change only that string.
  - In `every_call_passes_the_socket_the_c_locale_and_u`, the `NewWindow` literal gets `hold: false`. Its expected line `new-window -d -P -F #{window_index} -t =zygo-t: -c /w/d##x -- sleep 600` does not change.
  - New `#[cfg(unix)] #[test] fn new_window_with_hold_targets_the_end_in_the_same_call()`:
    - `let _g = forks();`, then a `log_stub` `Tmux` with no socket, `argv = os(&["sleep", "600"])`, and `let _ = t.new_window(&NewWindow { session: "zygo-t", name: None, start_dir: Path::new("/w"), argv: &argv, hold: true });`.
    - `assert_eq!(log_lines(dir.path()), ["LC_ALL=C -u new-window -d -P -F #{window_index} -a -t =zygo-t:{end} -c /w -- sleep 600 ; set-option -w -t =zygo-t:{end} -- remain-on-exit on"]);`.
  - New `#[cfg(unix)] #[test] fn held_panes_stay_dead_and_an_unheld_window_closes()`:
    - `let _g = forks(); let Some(_) = require_tmux() else { return };`, a `tempdir`, `let srv = PrivateServer::start(); let t = srv.tmux();`.
    - A local `fn until(f: impl Fn() -> bool) -> bool` polls every 10 ms for at most 5 s.
    - `srv.run(&["new-session", "-d", "-s", "base", "sleep", "600"]).unwrap();`
    - `assert_eq!(t.new_session(&NewSession { name: "zygo-t", start_dir: dir.path(), width: 80, height: 24, env: &env(), argv: &os(&["sh", "-c", "exit 3"]), owner: Some(("0123456789abcdef", "kaaaaaaaa")) }), Ok(0));`
    - `assert!(until(|| t.display("=zygo-t:0", "#{pane_dead}|#{pane_dead_status}") == Ok("1|3".into())));`
    - `assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Exists);`
    - `assert_eq!(srv.run(&["show-options", "-wv", "-t", "=zygo-t:0", "remain-on-exit"]).unwrap(), "on");`
    - `assert_eq!(srv.run(&["show-options", "-gwv", "remain-on-exit"]).unwrap(), "off");`
    - `assert_eq!(t.new_window(&NewWindow { session: "zygo-t", name: Some("w"), start_dir: dir.path(), argv: &os(&["sh", "-c", "exit 4"]), hold: true }), Ok(1));`
    - `assert!(until(|| t.display("=zygo-t:1", "#{pane_dead}|#{pane_dead_status}|#{window_name}") == Ok("1|4|w".into())));`
    - `assert_eq!(t.new_window(&NewWindow { session: "zygo-t", name: None, start_dir: dir.path(), argv: &os(&["sh", "-c", "exit 5"]), hold: false }), Ok(2));`
    - `assert!(until(|| srv.run(&["list-windows", "-t", "=zygo-t:", "-F", "#{window_index}"]).unwrap() == "0\n1"));`. This is the control: without a hold the window closes.
  - Check: `cargo test -p zygo-workspace --locked --test mux` fails to compile: `NewWindow` has no field `hold`.

- [x] **2. Code: `NewWindow.hold`, the new_session hold item, the hold form of new_window.**
  - `crates/zygo-workspace/src/mux/mod.rs`, `pub struct NewWindow<'a>` (`:232-238`): add the last field
    ```rust
    /// FIX-29 (W§8.1 step 11): with `true`, the window is made after the highest index (`-a -t =<s>:{end}`) and the
    /// same invocation goes on with `; set-option -w -t =<s>:{end} -- remain-on-exit on`, so a first pane that exits
    /// at once stays as a dead pane and the window stays (the build hold). `false`: the plain form.
    pub hold: bool,
    ```
  - `mod.rs` doc of `fn new_session`: after the `destroy-unattached off` sentence, add ``; then `; set-option -w -t =<s>: -- remain-on-exit on` (FIX-29, the build hold of the first window)``.
  - `mod.rs` doc of `fn new_window`: ``With `spec.hold`: `new-window -d -P -F #{window_index} -a -t =<s>:{end} -c <dir> -- <argv> ; set-option -w -t =<s>:{end} -- remain-on-exit on`, then the rename.``
  - `NewSession.owner` doc (`mod.rs:222-228`): add ``With `Some`, the same invocation also sets `remain-on-exit on` on the first window (FIX-29).``
  - `crates/zygo-workspace/src/mux/tmux.rs`, `fn new_session`, in the `if let Some((id, key)) = spec.owner` block after the `destroy-unattached` item (`:339`):
    ```rust
    // FIX-29, W§8.1 step 9: the build hold. tmux runs this item in the same command-queue pass as `new-session`,
    // before it handles the exit of the new pane (3.2a cmd-queue.c:704-745; live on 3.2a and 3.8), so a first pane
    // that exits at once stays as a dead pane and the session stays. `up` releases it before its `phase = up` write.
    c.args([";", "set-option", "-w", "-t", &target, "--", "remain-on-exit", "on"]);
    ```
  - `tmux.rs`, `fn new_window` (`:397-414`): build the arguments as
    ```rust
    let end = format!("={}:{{end}}", spec.session);
    let mut args: Vec<OsString> = ["new-window", "-d", "-P", "-F", "#{window_index}"].iter().map(OsString::from).collect();
    if spec.hold {
        // FIX-29: `-a` after `{end}` makes the new window the highest index, so `{end}` names it in the chained item
        // (3.2a cmd-find.c:419-425, cmd-new-window.c:99-100).
        args.extend(["-a".into(), "-t".into(), end.clone().into()]);
    } else {
        args.extend(["-t".into(), session_target(spec.session).into()]);
    }
    args.extend(["-c".into(), dir.into(), "--".into()]);
    args.extend(spec.argv.iter().cloned());
    if spec.hold {
        args.extend([";", "set-option", "-w", "-t", end.as_str(), "--", "remain-on-exit", "on"].iter().map(OsString::from));
    }
    ```
    The rest stays as it is: `window_index(self.call(&args)?)?`, then the rename.
  - Add `hold: false` to each `NewWindow` literal in `crates/zygo-workspace/tests/mux.rs` (lines 264, 494, 513, 533, 761, 787, 790, 797, 870, 892, 978, 1065), in `crates/zygo-workspace/tests/snapshot.rs:933`, and for now in `up.rs:521`. Step 8 changes `up.rs:521` to `hold: self.hold`.
  - Check: `cargo test -p zygo-workspace --locked --test mux` passes (the tmux cases run or print `tmux tests skipped: …`). `cargo test -p zygo-workspace --locked --test snapshot` passes.

- [ ] **3. Test: the snapshot keeps a saved cwd when tmux gives an empty path (`crates/zygo-workspace/tests/snapshot.rs`, at the end of the file).**
  - New `#[test] // FIX-29, W§7.4 step 5: a dead pane reads an empty path; the stored cwd stays` `fn empty_pane_path_keeps_the_stored_cwd()`:
    - `let (_d, p, store) = up(&["kaaaaaaaa", "kbbbbbbbb"]);`. Both stored panes have cwd `/srv/app` (`state`, `tests/snapshot.rs:38-51`).
    - `let fake = Fake::new(Some(ID), vec![Ok(vec![line(0, "main", 0, "%0", 0, 1, "kaaaaaaaa", ""), line(0, "main", 0, "%1", 1, 0, "kbbbbbbbb", "/b"), line(0, "main", 0, "%2", 2, 0, "", "")])]); fake.opt("%2", "");`
    - `assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);`
    - `let ws = store.read().unwrap().unwrap(); assert_eq!(ws.panes.iter().map(|x| x.cwd.clone()).collect::<Vec<_>>(), [PathBuf::from("/srv/app"), PathBuf::from("/b"), PathBuf::new()]);`. Kept, replaced, and a new pane with no stored entry stays empty.
  - Check: `cargo test -p zygo-workspace --locked --test snapshot empty_pane_path_keeps_the_stored_cwd` fails at the `cwd` assertion: the first cwd is `""`.

- [ ] **4. Code: the empty-path rule (`crates/zygo-workspace/src/snapshot.rs`, `fn apply`, `:336-347`).**
  - Replace `cwd: p.cwd.clone(),` with
    ```rust
    // FIX-29: tmux gives no `#{pane_current_path}` for a dead pane (3.2a and 3.8 probe); keep the stored cwd.
    cwd: if p.cwd.as_os_str().is_empty() {
        ws.panes.iter().find(|x| x.key == *key).map(|x| x.cwd.clone()).unwrap_or_default()
    } else {
        p.cwd.clone()
    },
    ```
  - Update the doc comment of `apply` (`:307-311`): ``A pane whose read path is empty keeps the `cwd` of the stored pane with its key (FIX-29).``
  - Check: `cargo test -p zygo-workspace --locked --test snapshot` passes.

- [ ] **5. Test: `down` does not call a dead pane busy (`crates/zygo-workspace/tests/down.rs`, at the end of the file).**
  - New `#[test] // FIX-29, AU24: a dead pane runs nothing; no "is running" line` `fn live_down_does_not_warn_about_a_dead_pane()`:
    - `let _s = serial(); let Some(_) = require_tmux() else { return }; let l = live(); let o = live_opts(&l);`
    - `assert!(wait_for(S5, || l.srv.run(&["display-message", "-p", "-t", "=zygo-app:", "#{pane_current_command}"]).is_ok_and(|c| c == "sleep")));`
    - `let key = l.store().read().unwrap().unwrap().panes[0].key.clone();`
    - `l.srv.run(&["set-option", "-w", "-t", "=zygo-app:", "remain-on-exit", "on"]).unwrap();`
    - `let dead = l.srv.run(&["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", "=zygo-app:", "--", "sleep", "0"]).unwrap();`
    - `assert!(wait_for(S5, || l.srv.run(&["display-message", "-p", "-t", &dead, "#{pane_dead}|#{pane_current_command}"]).is_ok_and(|c| c == "1|sleep")));`. tmux still names the last program of a dead pane (e5).
    - `let (code, out, err) = run(&o, &hooks());`
    - `assert_eq!((code, out.as_str()), (0, "zygo: app is down (0 agents saved)\n"), "{err}");`
    - `assert_eq!(count(&err, &format!("zygo: app pane {key}: sleep is running; it stops with the session")), 1, "{err}");`
    - `assert_eq!(count(&err, &format!("zygo: app pane {dead}: sleep is running; it stops with the session")), 0, "{err}");`
  - Check: `cargo test -p zygo-workspace --locked --test down live_down_does_not_warn_about_a_dead_pane` fails at the last assertion (count 1).

- [ ] **6. Code: `busy_shells` skips dead panes (`crates/zygo-workspace/src/down.rs:1311-1333`).**
  - Replace the `display` call and its check with
    ```rust
    let Ok(v) = mux.display(&p.id, "#{pane_dead}|#{pane_current_command}") else {
        continue;
    };
    let (dead, command) = v.split_once('|').unwrap_or(("", v.as_str()));
    // FIX-29: a dead pane (held by `remain-on-exit`) runs nothing; tmux still names its last program.
    if dead == "1" || command.is_empty() || is_shell(command) {
        continue;
    }
    ```
    The warning line uses `esc(command)` as today.
  - Doc of `busy_shells`: ``… and runs something other than a shell; a dead pane (`#{pane_dead}` = 1) is skipped (FIX-29).``
  - Check: `cargo test -p zygo-workspace --locked --test down` passes.

- [ ] **7. Test: `up` and repair with panes that exit at once (`crates/zygo-workspace/tests/up.rs`).** Put the new helpers and the five new tests at the END of `tests/up.rs`, after `first_pane_of_a_new_session_gets_zygo_workspace_in_session_scope_only`. FIX-24 adds its test after `loadout_argument_paths_use_the_pane_folder`, so the two nodes do not change the same lines.
  - Helpers:
    ```rust
    /// FIX-29: a zygo stand-in whose `ws run` runs `body` at once; every other call exits 0.
    fn ws_run_does(l: &Live, name: &str, body: &str) -> PathBuf {
        script(l.t.path(), name, &format!("if [ \"$1\" = ws ] && [ \"$2\" = run ]; then {body}; fi\nexit 0"))
    }

    /// FIX-29: true when `zygo-app` has `n` panes and all are dead, or when the session or its server is gone (the
    /// defect: then there is nothing to wait for).
    fn all_dead(wrapper: &Path, n: usize) -> bool {
        let out = Command::new(wrapper).args(["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{pane_dead}"]).output().unwrap();
        if !out.status.success() {
            return true;
        }
        let s = String::from_utf8_lossy(&out.stdout).into_owned();
        let v: Vec<&str> = s.lines().collect();
        v.len() == n && v.iter().all(|x| *x == "1")
    }

    /// FIX-29: hooks that make the panes exit before `up` goes on: at `after_new_session` the first pane, at
    /// `before_final_snapshot` all `n` panes (each wait at most 5 s). `None` clears both.
    fn wait_for_dead_panes(dir: &Path, wrapper: Option<&Path>, n: usize) {
        let (a, b) = match wrapper {
            Some(w) => {
                let (w1, w2) = (w.to_path_buf(), w.to_path_buf());
                let a: Box<dyn Fn() -> Result<(), String> + Send + Sync> = Box::new(move || { wait_for(S5, || all_dead(&w1, 1)); Ok(()) });
                let b: Box<dyn Fn() -> Result<(), String> + Send + Sync> = Box::new(move || { wait_for(S5, || all_dead(&w2, n)); Ok(()) });
                (Some(a), Some(b))
            }
            None => (None, None),
        };
        up::testing::hook(dir, "after_new_session", a);
        up::testing::hook(dir, "before_final_snapshot", b);
    }

    /// FIX-29: the line of a pane that exited while held; `how` is `exit <n>` or `no exit status`.
    fn exited(key: &str, how: &str, socket: &Path, pane: &str) -> String {
        format!("zygo: pane {key} exited during up ({how}); its dead pane stays: tmux -S '{}' respawn-pane -t {pane} starts it again\n", socket.display())
    }

    /// FIX-29: `remain-on-exit` of window `w` of `zygo-app`.
    fn remain(l: &Live, w: u32) -> String {
        l.srv.run(&["show-options", "-wv", "-t", &format!("=zygo-app:{w}"), "remain-on-exit"]).unwrap()
    }
    ```
  - Extend `repair_after_a_failure_after_new_session` (`tests/up.rs:1216-1246`). This is a guard for items 1 and 7a: the hold from step 2 is already in, so the extension passes before step 8.
    - After `assert_eq!(ws.phase, Phase::Building);` add `assert_eq!(remain(&l, 0), "on"); // the build hold that the stopped up left`.
    - At the end of the test add `for w in by_key(&l.srv).values().map(|(w, _)| *w).collect::<BTreeSet<u32>>() { assert_eq!(remain(&l, w), "off", "window {w}"); } // AU67 repair releases every window`. Add `BTreeSet` to the `std::collections` import.
  - New `#[test] // FIX-29, W§8.1 steps 9-12, AU68, W-C3: panes whose command exits at once stay as dead panes; one line each` `fn restored_panes_that_exit_at_once_stay_dead_and_are_reported()`:
    - `let _g = serial(); let Some(_) = require_tmux() else { return }; let mut l = live("0"); let store = l.store();`
    - `saved(&store, &l.repo.clone(), vec![win(0, Some("one"), "tiled", 120, 40), win(1, Some("two"), "tiled", 120, 40)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo), pane("kbbbbbbbb", 0, 1, PaneKind::Shell, &l.wt), pane("kcccccccc", 1, 0, PaneKind::Shell, &l.wt)], vec![]);`
    - `l.o.zygo = ws_run_does(&l, "zygo-exits", "exit 3");`
    - `l.srv.run(&["new-session", "-d", "-s", "base", "sleep", "600"]).unwrap(); let g0 = globals(&l.srv);`
    - `wait_for_dead_panes(store.dir(), Some(l.srv.wrapper()), 3); let t0 = Instant::now(); let (code, out, err) = run(&l.o); let took = t0.elapsed(); wait_for_dead_panes(store.dir(), None, 0);`
    - `assert!(took < Duration::from_secs(30), "up took {took:?}");`. The test bound proves no hang. The product has no time limit.
    - `assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");` and `assert!(!err.contains("zygo: tmux:"), "{err}");`
    - `let sock = read(&store).tmux_socket.unwrap(); assert_eq!(sock, l.srv.socket());`. The hint names the private `-S` socket.
    - `let ids = by_key(&l.srv); assert_eq!(ids.keys().cloned().collect::<Vec<_>>(), ["kaaaaaaaa", "kbbbbbbbb", "kcccccccc"]);`
    - `for (k, (_, id)) in &ids { assert_eq!(err.matches(&exited(k, "exit 3", &sock, id)).count(), 1, "{err}"); }`
    - `assert_eq!(l.srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{@zygo-pane} #{pane_dead} #{pane_dead_status}"]).unwrap(), "kaaaaaaaa 1 3\nkbbbbbbbb 1 3\nkcccccccc 1 3");`
    - `assert_eq!((remain(&l, 0), remain(&l, 1)), ("off".to_string(), "off".to_string()));`
    - `assert_eq!(globals(&l.srv), g0);`
    - `let ws = read(&store); assert_eq!((ws.phase, ws.down_all, ws.build_plan, ws.fresh_members.len()), (Phase::Up, false, None, 0));`
    - `assert_eq!(ws.windows.iter().map(|w| w.name.clone()).collect::<Vec<_>>(), [Some("one".to_string()), Some("two".to_string())]);`
    - `assert_eq!(ws.panes.iter().map(|p| (p.key.clone(), p.cwd.clone())).collect::<Vec<_>>(), [("kaaaaaaaa".to_string(), l.repo.clone()), ("kbbbbbbbb".to_string(), l.wt.clone()), ("kcccccccc".to_string(), l.wt.clone())]);`
  - New `#[test] // FIX-29 reproduction (M5 implementer): up --all after a server death, panes whose command exits at once` `fn up_all_after_a_server_death_reports_panes_that_exit_at_once()`:
    - `let _g = serial(); let Some(_) = require_tmux() else { return }; let mut l = live("0"); up_pair(&mut l); let store = l.store();`
    - `let before = read(&store); let keys: Vec<String> = before.panes.iter().map(|p| p.key.clone()).collect(); let cwds: Vec<PathBuf> = before.panes.iter().map(|p| p.cwd.clone()).collect(); assert_eq!(keys.len(), 3);`
    - `l.srv.kill_server(); l.o.zygo = ws_run_does(&l, "zygo-exits", "exit 3");`. There is no `base` session, so the server ends with the workspace session, as in the reproduction.
    - `wait_for_dead_panes(store.dir(), Some(l.srv.wrapper()), 3); let (mut o, mut e) = (Vec::new(), Vec::new()); let code = up::testing::run_all_with(&l.o, &mut o, &mut e); wait_for_dead_panes(store.dir(), None, 0); let (out, err) = (text(o), text(e));`
    - `assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");` and `assert!(!err.contains("zygo: tmux:") && !err.contains("skipped"), "{err}");`
    - `let sock = read(&store).tmux_socket.unwrap(); let ids = by_key(&l.srv); for k in &keys { assert_eq!(err.matches(&exited(k, "exit 3", &sock, &ids[k].1)).count(), 1, "{err}"); }`
    - `let ws = read(&store); assert_eq!(ws.phase, Phase::Up); assert_eq!(ws.panes.iter().map(|p| p.key.clone()).collect::<Vec<_>>(), keys); assert_eq!(ws.panes.iter().map(|p| p.cwd.clone()).collect::<Vec<_>>(), cwds);`
  - New `#[test] // FIX-29 (review item 7b): a pane ended by a signal has no exit status` `fn a_pane_ended_by_a_signal_is_reported_with_no_exit_status()`:
    - `let _g = serial(); let Some(_) = require_tmux() else { return }; let mut l = live("0"); let store = l.store();`
    - `saved(&store, &l.repo.clone(), vec![win(0, None, "tiled", 120, 40)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);`
    - `l.o.zygo = ws_run_does(&l, "zygo-killed", "kill -9 $$");`
    - `wait_for_dead_panes(store.dir(), Some(l.srv.wrapper()), 1); let (code, out, err) = run(&l.o); wait_for_dead_panes(store.dir(), None, 0);`
    - `assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");`
    - `let sock = read(&store).tmux_socket.unwrap(); let ids = by_key(&l.srv);`
    - `assert_eq!(err.matches(&exited("kaaaaaaaa", "no exit status", &sock, &ids["kaaaaaaaa"].1)).count(), 1, "{err}");`
  - New `#[test] // FIX-29 (review items 2, 7c): a release failure is one line; up goes on and keeps its exit code` `fn a_failed_release_is_reported_and_up_goes_on()`:
    - `let _g = serial(); let Some(_) = require_tmux() else { return }; let mut l = live("0"); let store = l.store(); fs::create_dir_all(store.dir()).unwrap();`
    - `let calls = Arc::new(AtomicUsize::new(0)); let c = calls.clone(); up::testing::hook(store.dir(), "release", Some(Box::new(move || if c.fetch_add(1, Ordering::SeqCst) == 0 { Err("injected".into()) } else { Ok(()) })));`
    - `l.o.config = pair(); l.o.loadout = Some("pair".into()); let (code, out, err) = run(&l.o); up::testing::hook(store.dir(), "release", None);`
    - `assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");`
    - `assert_eq!(calls.load(Ordering::SeqCst), 2);`. Both windows were tried: there was no early return.
    - `assert_eq!(err.matches("zygo: app: could not release the build hold of window 0: injected\n").count(), 1, "{err}");`
    - `assert_eq!((remain(&l, 0), remain(&l, 1)), ("on".to_string(), "off".to_string()));`
    - `assert_eq!(read(&store).phase, Phase::Up);`
  - New `#[test] // FIX-29 (review item 8), AU67: a repair prints the line of a pane that died during the hold` `fn repair_reports_a_pane_that_died_during_the_hold()`:
    - `let _g = serial(); let Some(_) = require_tmux() else { return }; let mut l = live("0"); let store = l.store(); let normal = l.o.zygo.clone();`
    - `saved(&store, &l.repo.clone(), vec![win(0, Some("one"), "tiled", 120, 40)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo), pane("kbbbbbbbb", 0, 1, PaneKind::Shell, &l.wt)], vec![]);`
    - `l.o.zygo = ws_run_does(&l, "zygo-exits", "exit 3"); let w = l.srv.wrapper().to_path_buf();`
    - `up::testing::hook(store.dir(), "after_new_session", Some(Box::new(move || { wait_for(S5, || all_dead(&w, 1)); Err("injected".into()) })));`
    - `let r1 = run(&l.o); up::testing::hook(store.dir(), "after_new_session", None);`
    - `assert_eq!((r1.0, r1.2.as_str()), (1, "zygo: tmux: injected\n")); assert_eq!(read(&store).phase, Phase::Building); assert_eq!(remain(&l, 0), "on");`
    - `l.o.zygo = normal; let (code, out, err) = run(&l.o);`
    - `assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");`
    - `let sock = read(&store).tmux_socket.unwrap(); let ids = by_key(&l.srv);`
    - `assert_eq!(err.matches(&exited("kaaaaaaaa", "exit 3", &sock, &ids["kaaaaaaaa"].1)).count(), 1, "{err}");`
    - `assert_eq!(err.matches("exited during up").count(), 1, "{err}");`. `kbbbbbbbb` was built alive by the repair.
    - `assert_eq!(remain(&l, 0), "off"); assert_eq!(read(&store).phase, Phase::Up);`
  - Check: `cargo test -p zygo-workspace --locked --test up` runs. After step 2 the real order of a build is: the hold from `new_session` on window 0, then step 10's `window_setup` sets window 0 to `off`, then later panes and windows are made without a hold, and no dead-pane line exists.
    - `restored_panes_that_exit_at_once_stay_dead_and_are_reported` fails at `assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"))` when a pane closes before the next tmux call, else at the first `exited(…)` count.
    - `up_all_after_a_server_death_reports_panes_that_exit_at_once` fails at the code/out assertion or the `skipped` check, else at the first `exited(…)` count.
    - `a_pane_ended_by_a_signal_is_reported_with_no_exit_status` fails at the `exited(…)` count.
    - `a_failed_release_is_reported_and_up_goes_on` fails at `assert_eq!(calls.load(Ordering::SeqCst), 2)`: the point `release` does not exist yet, so `calls` is 0.
    - `repair_reports_a_pane_that_died_during_the_hold` fails at the `exited(…)` count.
    - The extended `repair_after_a_failure_after_new_session` passes. It is a guard.

- [ ] **8. Code: hold, release, report in `crates/zygo-workspace/src/up.rs`; `hold: false` and the report in `crates/zygo-workspace/src/repair.rs`.**
  - `up.rs` imports: add `valid_pane_key` to the `crate::identity::{…}` use (`up.rs:20`). `parse_pane_line` is already imported (`up.rs:24`).
  - `testing::hook` doc (`up.rs:116-125`): add the point ``release (before each window of `Builder::release`; an error there is that window's error)``.
  - After `not_built_line` (`up.rs:192-197`):
    ```rust
    /// The FIX-29 line of a pane whose command exited while `up` held it (W§8.1 step 12). `status` is
    /// `#{pane_dead_status}`, empty when a signal ended it; `socket` passed AU54, so it holds no `'` (as at AU100).
    pub(crate) fn exited_line(key: &str, status: &str, socket: &Path, pane: &str) -> String {
        let how = if status.is_empty() { "no exit status".to_string() } else { format!("exit {status}") };
        format!(
            "zygo: pane {key} exited during up ({how}); its dead pane stays: tmux -S '{}' respawn-pane -t {pane} starts it again",
            socket.display()
        )
    }
    ```
  - `pub(crate) struct Builder` (`up.rs:459-467`): add the last field
    ```rust
    /// FIX-29: the build holds each window (`remain-on-exit on`) until [`Builder::release`]; `repair` passes `false`.
    pub hold: bool,
    ```
  - `fn window_setup` (`up.rs:477-484`): set `remain-on-exit` to `if self.hold { "on" } else { "off" }`. Doc: ``The build hold (`on`, FIX-29) or `remain-on-exit off` (AU68), and the window hooks of W§7.4 on one window.``
  - `fn add_window` (`up.rs:521`): `NewWindow { session: self.session, name: w.name.as_deref(), start_dir: self.main_root, argv: &argv, hold: self.hold }`.
  - New methods in `impl Builder<'_>`:
    ```rust
    /// FIX-29, W§8.1 step 12: ends the build hold: `remain-on-exit off` on each window, best effort. A window that is
    /// gone prints nothing; any other error prints one line and the next window is tried. A dead pane stays dead.
    pub fn release(&self, windows: &[u32], name: &str, err: &mut dyn Write) {
        for &w in windows {
            let r = fire(self.dir, "release")
                .and_then(|()| self.mux.set_window_option(&window_target(self.session, w), "remain-on-exit", "off"));
            match r {
                Ok(()) | Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => {}
                Err(e) => say(err, &format!("zygo: {}: could not release the build hold of window {w}: {e}", esc(name))),
            }
        }
    }

    /// FIX-29: one [`exited_line`] for each dead pane of the session that has a valid pane key, in `list-panes` order,
    /// best effort. A gone session or pane gives no line; any other error prints one line and goes on.
    pub fn dead_lines(&self, socket: &Path, name: &str, err: &mut dyn Write) -> Vec<String> {
        let rows = match self.mux.list_panes(self.session) {
            Ok(rows) => rows,
            Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => return Vec::new(),
            Err(e) => {
                say(err, &format!("zygo: {}: could not read the dead panes: {e}", esc(name)));
                return Vec::new();
            }
        };
        let mut lines = Vec::new();
        for row in rows {
            let Ok((_, p)) = parse_pane_line(&row) else { continue };
            if !valid_pane_key(&p.key) {
                continue;
            }
            match self.mux.display(&p.id, "#{pane_dead}|#{pane_dead_status}") {
                Ok(v) => {
                    if let Some(status) = v.strip_prefix("1|") {
                        lines.push(exited_line(&p.key, status, socket, &p.id));
                    }
                }
                Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => {}
                Err(e) => say(err, &format!("zygo: {}: could not read pane {}: {e}", esc(name), p.key)),
            }
        }
        lines
    }
    ```
  - `fn build`: the literals `b9` (`up.rs:1107`) and `b` (`up.rs:1167`) get `hold: true`. Step 10 stays `b.window_setup(first_index)`, which now keeps the hold.
  - `fn build`: right after `fire(store.dir(), "after_final_snapshot").map_err(tmux_err)?;` and before `let done = store` (`up.rs:1259-1260`):
    ```rust
    // FIX-29: end the build hold before `phase = up`, so a stop during the hold leaves `building` (AU67); then one
    // line per pane that exited while held (W§8.1 step 12).
    b.release(&real, &name, err);
    let exited = b.dead_lines(&socket, &name, err);
    ```
    `socket` is the checked socket of `up.rs:1147` (AU54 check at `up.rs:1152`). The report loop (`up.rs:1288`) becomes `….chain(&detached).chain(&au96).chain(&exited)`.
  - `crates/zygo-workspace/src/repair.rs:132`: `Builder { mux, session: &ws.tmux_session, zygo, id: &ws.id, main_root: &ws.main_root, dir: store.dir(), hold: false }`.
  - `repair.rs`, `fn building`, after the `socket` block (`:180-188`) and before `write(store, …)` (`:189`):
    ```rust
    // FIX-29: the dead panes of the stopped build (W§8.1 step 12 lines); window_setup above released the hold.
    if let Some(sock) = socket.clone().or_else(|| ws.tmux_socket.clone()) {
        for line in b.dead_lines(&sock, &ws.name, err) {
            let _ = writeln!(err, "{line}");
        }
    }
    ```
    `socket` is `Some` exactly when `ws.tmux_socket` is `None` (`repair.rs:180-188`), so one of the two is always set.
  - Doc of `fn building` (`repair.rs:129-130`): add ``, then one line per dead pane (FIX-29)``.
  - Check: `cargo test -p zygo-workspace --locked --test up` passes. All five new tests and the extended repair test pass. `first_up_builds_the_loadout` (`remain-on-exit off` on each window while the global value is `on`, `tests/up.rs:565-587`) and `first_pane_of_a_new_session_gets_zygo_workspace_in_session_scope_only` stay green.

- [ ] **9. Spec: final text in `docs/specs/2026-11-20-workspaces-design.md`.**
  - W§7.2, the `panes[]` row (`:287`): replace ``cwd` (`#{pane_current_path}`)`` with ``cwd` (`#{pane_current_path}`; an empty read, as for a dead pane, keeps the saved `cwd` of the same key, W§7.4 step 5)``.
  - W§7.4 step 5: after ``and a pane keeps those of the stored pane with the same key (AU78);``, insert ``a pane whose read path is empty (tmux gives no `#{pane_current_path}` for a dead pane) keeps the `cwd` of the stored pane with the same key, else its `cwd` is empty;``.
  - W§8.1 step 9: in the command form, replace ``; set-option -t =zygo-<name>: destroy-unattached off` (AU55, AU68, AU96)`` with ``; set-option -t =zygo-<name>: destroy-unattached off ; set-option -w -t =zygo-<name>: remain-on-exit on` (AU55, AU68, AU96)``. After the sentence that ends ``does not end it.``, insert: ``The `remain-on-exit on` of the first window is the build hold: tmux runs the items of one invocation in one command-queue pass, before it handles the exit of the new pane (tmux 3.2a `cmd-queue.c`; checked live on tmux 3.2a and 3.8), so a first pane whose command exits at once stays as a dead pane, and the session stays until step 12 ends the hold.``
  - W§8.1 step 10, whole step: ``10. On this session only: `set-environment -r` for each name of AU64, `set-environment ZYGO_WORKSPACE <id>`, user option `@zygo-ws <id>`, `destroy-unattached off` (AU68), the hooks of W§7.4 (AU54). The first window keeps `remain-on-exit on`, the build hold of step 9, until step 12.``
  - W§8.1 step 11: replace ``For each later window, set `default-size` (AU96), then `new-window` (with `-c` as in step 9 and `-n` with `#` doubled when it has a saved name) with its first pane, then `split-window` for the rest.`` with ``For each later window, set `default-size` (AU96), then `new-window -a -t =zygo-<name>:{end}` (with `-c` as in step 9) with its first pane and, in the same invocation, `; set-option -w -t =zygo-<name>:{end} remain-on-exit on`: the build hold of step 9 for this window, which `-a` after `{end}` makes the window with the highest index. Then `rename-window` with `#` doubled when it has a saved name (AU99), then `split-window` for the rest; a split pane has the hold of its window (`remain-on-exit` is a window and pane option, tmux 3.2a `options-table.c`). Known limit: a user's global `after-new-window` hook runs before the chained `set-option`; when it creates a window, `{end}` names that window and the new zygo window is not held, and when it makes the command queue wait (a `run-shell` without `-b`), tmux can handle a pane exit first. zygo adds no code for these cases.``
  - W§8.1 step 12: in the first sentence, replace ``Wait at most 2 s, reading `list-panes` each 50 ms, until each built pane whose planned cwd is an existing folder reports that folder as `#{pane_current_path}`; then run one snapshot with `--force` (AU38) in every case.`` with ``Wait at most 2 s, reading `list-panes` each 50 ms, until each built pane whose planned cwd is an existing folder reports that folder as `#{pane_current_path}`; a dead pane reports no folder, so with a dead pane the wait runs its full 2 s, which is bounded and ends no user work. Then run one snapshot with `--force` (AU38) in every case; it runs during the build hold, so it records every pane.``
  - W§8.1 step 12: replace ``Take `lock`; set `phase = up`, `down_all = false`; write; release. Print the permission flags of each resumed member (AU70), one warning per dropped flag (AU30), and one line per detached member (W§7.5) and per pane not built (AU96). Release `op.lock`.`` with ``Then end the build hold, best effort for each window: `set-option -w -t =zygo-<name>:<window> remain-on-exit off` on each built window (AU68); a window that is gone prints nothing, and any other error prints `zygo: <name>: could not release the build hold of window <window>: <error>` and the next window is tried. A pane whose command exited while held stays as a dead pane, with its key, its `panes[]` entry and its saved cwd. Read `#{pane_dead}` and `#{pane_dead_status}` of each pane with a pane key, best effort for each pane (a gone pane prints nothing; another error prints `zygo: <name>: could not read pane <key>: <error>`). Take `lock`; set `phase = up`, `down_all = false`; write; release. A stop of `up` before this write, also during the hold, leaves `phase = building` for the repair of AU67. Print the permission flags of each resumed member (AU70), one warning per dropped flag (AU30), one line per detached member (W§7.5) and per pane not built (AU96), and one line per dead pane: `zygo: pane <key> exited during up (exit <n>); its dead pane stays: tmux -S '<socket>' respawn-pane -t <pane id> starts it again` (`no exit status` in place of `exit <n>` when a signal ended it; the socket is quoted as in AU100). Release `op.lock`. `up` never waits for a dead pane, and a dead pane changes no exit code: `up` exits 0 without attach, and with the code of step 13 with attach. Known limit: a pane that exits after the hold ends closes as in W§7.5; when it was the last pane, the session ends before the `phase = up` write, the write still sets `phase = up`, and the attach of step 13 fails with tmux's error. The next `up` (or `up --all`, AU52) finds the session gone and no live launcher, and restores the layout that the final snapshot saved (steps 6-7).``
  - AU67 row: after ``then sets `phase = up` and removes `build_plan` and `fresh_members`.``, insert ``Before that write it prints the W§8.1 step 12 line of each dead pane of the session (a pane that died during the build hold of the stopped `up`); its `window_setup` has already set `remain-on-exit off` on each window.``
  - AU68 row text: ``Session options set by `up` on the workspace session only: `destroy-unattached off` and `remain-on-exit off`. `up` sets `destroy-unattached off` in the same tmux invocation as `new-session` (W§8.1 step 9), and again in step 10. `remain-on-exit` is a window option: `on` in the invocation that creates each window (W§8.1 steps 9 and 11, the build hold), and `off` on each built window when the build ends, before the `phase = up` write (step 12); a repair from `building` (AU67) sets `off` on each window of the session.``
  - AU24 row: after ``(`#{pane_current_command}`).``, insert ``A dead pane (`#{pane_dead}` is 1) is not listed: it runs no program, though tmux still names its last one.``
  - Change log, append the row: ``| 2026-11-23 | W§7.2 (`panes[]` cwd), W§7.4 step 5, W§8.1 steps 9-12, AU24, AU67, AU68 | author's call (node FIX-29 and its plan review) | A pane command that exited at once ended the session during the build, and `up` failed with a raw tmux error (M5 reproduction). The build now holds each window with `remain-on-exit on` from the invocation that creates it (checked live on tmux 3.2a and 3.8), releases the hold best effort before the `phase = up` write, and prints one line per dead pane with the server in the respawn hint; a repair prints the same lines; the dead pane stays, a snapshot keeps the saved cwd of a pane whose path tmux leaves empty, and `down` does not call a dead pane busy. Step 11 names later windows with `rename-window`, as the code does since W3 round 2 (AU99). |``
  - Check:
    - `rg -n "build hold" docs/specs/2026-11-20-workspaces-design.md` shows W§8.1 steps 9, 10, 11 and 12, AU67, AU68 and the change log.
    - `rg -n "respawn-pane|exited during up|could not release the build hold|runs its full 2 s" docs/specs/2026-11-20-workspaces-design.md` shows step 12.
    - `rg -n "keeps the saved .cwd. of the same key|keeps the .cwd. of the stored pane" docs/specs/2026-11-20-workspaces-design.md` shows W§7.2 and W§7.4 step 5.
    - `rg -n "pane_dead" docs/specs/2026-11-20-workspaces-design.md` shows AU24 and step 12.

- [ ] **10. Acceptance, tmux 3.2a run, dependents.**
  - `cargo test -p zygo-workspace --locked`: all pass. The tmux cases run on the tmux on `PATH` (3.8 here), or print `tmux tests skipped: …`.
  - tmux 3.2a, as W3 and W7 did (`docs/work/phase2/graph.md:194`, `:206`). Run with `PATH=/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/tmux32a/prefix/bin:$PATH`:
    - `cargo test -p zygo-workspace --locked --test mux new_window_with_hold held_panes_stay_dead new_session_sets_the_owner`
    - `cargo test -p zygo-workspace --locked --test up restored_panes_that_exit_at_once up_all_after_a_server_death a_pane_ended_by_a_signal a_failed_release repair_reports_a_pane repair_after_a_failure_after_new_session first_up_builds_the_loadout`
    - `cargo test -p zygo-workspace --locked --test down live_down_does_not_warn_about_a_dead_pane`

    Expect all to pass. No line `tmux tests skipped: …` may appear. Record the counts for 3.8 and 3.2a in the node result. Do not change CI.
  - `cargo check --workspace --all-targets --locked`: no error. `zygo` and `zygo-hooks` depend on `zygo-workspace`, and no `NewWindow` literal is outside it.
  - `cargo clippy -p zygo-workspace --all-targets --locked -- -D warnings`: no warning.

### Self-review (run again after the review revision)

- **Spec coverage.**
  - "Finishes the build, or reports one line per pane that exited": steps 8 and 9. Tests in step 7: restore, `up --all`, signal, repair.
  - "Never a raw tmux error as the only reason": while the hold is on, no pane exit can end the session. The tests assert that `err` has no `zygo: tmux:` line and that `up --all` skips nothing. The remaining case after the release is a stated known limit (item 1).
  - "No workspace state lost": `panes[]` keeps every key and saved cwd (steps 3-4; the step 7 assertions; W§7.2 and W§7.4 text). Member records are not touched. A stop during the hold leaves `building` for AU67 (item 1).
  - "No hang": `up` does not wait for a pane. The only waits are the existing 2 s cwd wait (it now runs in full with a dead pane, stated in the plan and the spec, item 13) and the 2 s tmux call bound. The test asserts a 30 s test bound.
  - "No TTL": the plan adds none.
  - W-C3: per-window targets only. `globals` is equal before and after (step 7), and the global `remain-on-exit` is still `off` (step 1).
  - M5 `-e`: unchanged, and its test stays green.
  - The review items map to the plan as follows:

    | Item | Where |
    |---|---|
    | 1 | Steps 7, 8, 9 |
    | 2 | Step 8 `release`/`dead_lines`, step 7 test |
    | 3 | `exited_line`, step 7 exact lines on the `-S` socket |
    | 4 | Evidence table, step 10 |
    | 5 | Steps 5-6, AU24 |
    | 6 | Step 7 check |
    | 7 | Step 7 signal and release tests |
    | 8 | Step 8 repair, step 7 test, AU67 |
    | 9 | Residual risks, step 11 text |
    | 10 | Drift section, step 7 placement |
    | 11 | Step 9 W§7.2 |
    | 12 | Step 9 step 12 text |
    | 13 | What the user sees, step 9 step 12 text |
- **Step scan.** Each test step comes before its code step: 1→2, 3→4, 5→6, 7→8. The step 7 check gives the real red point of each test, and it names the one guard test that is green before step 8. Each code step names the file, the signature and the values. The plan has no placeholder, no TODO and no stub.
- **Interface match.**
  - The Consumes signatures were read at `0228f29`: `up.rs:1307`, `up.rs:126-148`, `mod.rs:59`, `mod.rs:95-170`.
  - `NewWindow` gets one field. Every literal is listed (from `rg "NewWindow \{" crates/`). `Builder` literals: `up.rs:1107`, `up.rs:1167`, `repair.rs:132` (from `rg "Builder \{" crates/`).
  - `Multiplexer` does not change, so the fakes (`tests/admin.rs:273`, `tests/snapshot.rs:151`) do not change.
  - `dead_lines` uses only the existing trait calls `list_panes`, `display`, `parse_pane_line` and `identity::valid_pane_key`. `release` uses `set_window_option` and the existing `fire`.
  - `MuxError`'s Display prints the bare message (`mod.rs:417-425`), so the injected error prints as `injected`.
  - Consumers:
    - E1: restore checks. It gains the fix, and its `remain-on-exit off` checks still hold after the release.
    - FIX-24: no `src/up.rs` change. Its test goes at another anchor. Its `down.rs` change is in `drain_tick`, a different function.
    - D1: cites W§8.1 text. This plan changes no base-spec text.
- **Review focus.**
  - Item 1: every new tmux test starts with `require_tmux()` and returns when it gives `None`. The snapshot test needs no tmux.
  - Item 2: the new code is target-neutral, in files that already build on windows-latest. The new tmux tests are `cfg(unix)`: `tests/up.rs` is `#![cfg(unix)]`, the mux tests have `#[cfg(unix)]`, and `tests/down.rs` is `#![cfg(unix)]` (`tests/down.rs:9`).
  - Items 3-12: not touched.
- **Proportion.**
  - Code: one tmux item in `new_session`, one hold form in `new_window`, one field, two best-effort `Builder` methods, a 5-line snapshot rule, a 3-line `busy_shells` change, a 5-line repair report.
  - Tests: 7 new tests and 3 updated tests.
  - Rejected as larger: a placeholder first pane plus `respawn-pane` (it delays every first pane and needs a portable placeholder program), a new `Multiplexer` method (it changes two fakes), and code for a user hook that moves `{end}` (item 9: known limit).
