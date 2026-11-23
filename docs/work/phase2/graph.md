# Phase 2 Work Graph

<!-- work-graph -->

**Goal:** Build the zygo status line HUD and the auto-update check on start, then phase 2 workspaces (`zygo up`/`down` with drain, snapshot and resume), as the approved designs specify.
**Authority:** `docs/specs/2026-11-21-statusline-hud-design.md` (H§n), `docs/specs/2026-11-21-auto-update-design.md`, `docs/specs/2026-11-20-workspaces-design.md` (W§n), each approved by the operator 2026-11-21; `docs/specs/2026-11-18-zygo-design.md` (§n; base spec)
**Run:** parallel=3 · worktrees=.worktrees/phase2/ · disk-per-node=—
**Next:** W10, H1, M1, M2, M3, M4, M5, E1, E2, D1, FIX-16, FIX-17, FIX-18

To continue in any session: "resume the work in `docs/work/phase2/graph.md`".

## Global constraints

- Base constraints C1-C10 apply; C6 applies to every new hook and to the status line: a hook never breaks the agent — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C1; `docs/specs/2026-11-21-statusline-hud-design.md` H§4
- No daemon; tmux is the only long-lived process — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C2, W§2 W-G6, AU1
- tmux options and hooks only on zygo's own sessions; never a global option or hook — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C3
- Every state write is atomic (temporary file, then rename) — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C4, W§7.3
- `[loadout]`, `[workspace]`, `[statusline]` and `[update]` come from user config only; the team layer drops them — `docs/specs/2026-11-20-workspaces-design.md` W-C5, AU10, AU22; `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 6; `docs/specs/2026-11-21-auto-update-design.md` "Opt-out"
- `zygo claude`/`zygo opencode` outside a workspace stay a clean v1 launch — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C6, OP9
- Workspace state holds no secret and no content — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C7, W§7.2
- No user-controlled text reaches a tmux format argument unescaped — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C8, AU54, AU55
- Never resume a conversation whose launcher is live — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C9, AU87
- Native Windows: every workspace verb exits 2 with `workspaces need tmux; run zygo in WSL`; the code still builds there — `docs/specs/2026-11-20-workspaces-design.md` OP5; `.github/workflows/ci.yml` (windows-latest)
- Minimum tmux 3.2 — `docs/specs/2026-11-20-workspaces-design.md` AU7
- State dir `0700`, files `0600`; wait bounds hooks, member events and their snapshots 1 s, snapshots run by `up`/`down` 5 s, launchers/wrapper 5 s, `up`/`down` 5 s — `docs/specs/2026-11-20-workspaces-design.md` AU2, W§7.3
- tmux-dependent tests run when tmux 3.2 or later is on `PATH` and skip with a printed reason otherwise; on Windows they are `cfg(unix)`-gated; no CI install step; gate helper in W3, used by later nodes — operator, 2026-11-21
- Crate graph: `zygo-workspace` depends on `zygo-core`, `zygo-config`, `zygo-work`, `zygo-proxy` only; `zygo-hooks` depends on `zygo-workspace`; `ws run`, member wrapper and membership decision in `crates/zygo/src/cmd/` — `docs/specs/2026-11-20-workspaces-design.md` AU103, AU102, AU45
- No configured price leaves a server; a team session's spend uses built-in list prices — `docs/specs/2026-11-21-statusline-hud-design.md` AU14, H§9 row §7.2
- The update check sends only the GET for the public VERSION file, ignores the system proxy, and never asks inside a zygo session or without a TTY — `docs/specs/2026-11-21-auto-update-design.md` "Conflicts", "When the check is skipped", "Proxy"

## Verification floor

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` — `.github/workflows/ci.yml` job `test`
- Install script, unix: `sh scripts/test-install.sh` — `.github/workflows/ci.yml` (non-Windows)
- Install script, Windows: `pwsh scripts/test-install.ps1` — `.github/workflows/ci.yml` (windows-latest)
- Live checks (manual, recorded on the operator's machine): PRB-1, LIV-1, LIV-2, P1, P2, E2 — `docs/specs/2026-11-21-statusline-hud-design.md` H§12, H§14 #37, #38; `docs/specs/2026-11-20-workspaces-design.md` W§18 #76

## Review focus

Convention: an acceptance item number (W§18, H§14, auto-update) appears only in its owner node's file; earlier nodes test their own contract and cite AU or section ids.

1. CI runners without tmux (macos-latest has none by default) — tmux tests print a skip reason and pass — test in W3 (`require_tmux`) — operator, 2026-11-21
2. `zygo-workspace` and `zygo-hooks` build on windows-latest with unix-only code behind `cfg` — compiles; verbs exit 2 — test in M1 (W§18 #64) — `docs/specs/2026-11-20-workspaces-design.md` OP5
3. A streamed body that the client drops before its end — `requests_in_flight` returns to 0 — test in X1 — `docs/specs/2026-11-20-workspaces-design.md` AU91, W§19
4. The session-argument list in the adapters and the AU30 filter list drift apart — both match the pinned `--help` fixtures — tests in L2 and W4 — `docs/specs/2026-11-20-workspaces-design.md` AU30, W§8.3
5. A snapshot write and a member-record write on the same `workspace.json` at once — both changes kept — test in W6 (W§18 #8, #9) — `docs/specs/2026-11-20-workspaces-design.md` W§7.3
6. A hook waiting on `activity.lock` or `workspace.json` past its bound — one log line, exit 0 — test in H1 — `docs/specs/2026-11-20-workspaces-design.md` AU56, W-C1
7. A status reply from an older server with none of the new fields — every new field `None`, no error — test in HUD-9 (H§14 #17) — `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2
8. A team-session reply leaking a configured price through `spend_usd ÷ tokens` — built-in list prices only — test in HUD-10 (H§14 #15) — `docs/specs/2026-11-21-statusline-hud-design.md` AU14
9. A repository that configures programs (fsmonitor, hooks, filters, uploadpack) — no marker after a render — test in HUD-9 (H§14 #25) — `docs/specs/2026-11-21-statusline-hud-design.md` H§7.5
10. The update prompt on Windows with live sessions — install deferred — test in UPD-1 (auto-update #7, windows-latest job) — `docs/specs/2026-11-21-auto-update-design.md` "Other sessions"
11. `SessionStatus` literals broken by new fields — `Default` and `..Default::default()` keep later fields (X1) off the literals — test in HUD-5 — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 1
12. `SessionView` literals broken by new fields — `Default` and `..Default::default()` keep later fields (C1) off the literals — test in HUD-7 — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 1

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| W2 | Workspace state store: types, atomic write, locks, live check | W1, C1 | DONE | judgment | c0ea544; acceptance state 22, zygo-workspace all 51, workspace --no-run ok (merged tree) |
| W3 | Multiplexer trait, tmux implementation, tmux test gate | W1, P1, P2 | DONE | judgment | f945872; mux 42 on 3.7c and 3.2a; review round 2 fixed B1 (3.2a window names) and M1-M7 |
| W4 | Argument filter, bypass and user-info checks, environment cleaning | W1 | DONE | judgment | 3b98d88; acceptance args 10, env 4, workspace --no-run ok (merged tree) |
| W5 | Loadout config schema, team-layer drop, structural checks, resolution | W1, HUD-4, HUD-7, HUD-9 | DONE | judgment | f9a568a; acceptance zygo-config 214, loadout 6, workspace --no-run ok (merged tree) |
| W6 | Snapshot: list-panes read, shrink rule, key assignment, pending rule | W2, W3 | DONE | judgment | 8978541; acceptance snapshot 33, state 22, workspace --no-run ok (merged tree); review rounds 2-3 fixed B1, M1-M7, D1-D6 |
| W7 | zygo up for one workspace: plan, build, repair, attach | W2, W3, W4, W5, W6 | DONE | judgment | 3d51ad4; acceptance up 45, mux 46, snapshot 33, state 23, args 13, workspace --no-run ok (merged tree); review rounds 2-3 fixed B1, M1-M5, R3-1..R3-8 |
| W8 | up --all and workspace admin: ls, status, forget, forget-pane | W7 | DONE | judgment | 0921fb6; acceptance admin 21, up 45, up_all 7, workspace --no-run ok (merged tree); review round 2 fixed 1 blocking, 5 minor |
| W9 | activity.json under lock and the drain idle rule | W1, C1, X1, P2, HUD-5, W2 | DONE | judgment | e167624; activity 33 + 1 ignored, drain 27, workspace 111; review round 2 fixed B1 (newer-version files) and 5 minor |
| W10 | zygo down: detach, drain loop, timeout, stop, --now, --all | W7, W9, W5, P1, P2 | IN-PROGRESS | judgment | .worktrees/phase2/W10 |
| W11 | MCP tool workspace_down behind [features] workspace_tool (AU65, PW15 fallback) | P1, C1, W2 | DONE | judgment | 0799b7d; mcp_server 50 ok, config_layers 96 ok; review round 2 fixed F1-F7 |
| L1 | Launcher member support: member, callback, resume note, LaunchOutcome | C1, L3, HUD-7, HUD-9 | DONE | judgment | 2d1c4cd; acceptance zygo-launch 200 passed over its test binaries (6 ignored, all older), zygo 378 passed, workspace --no-run ok (merged tree) |
| L2 | Member events for Claude Code and OpenCode; WA-1/WA-2 conformance cells | L1, P2, W4 | DONE | judgment | c5a1569; acceptance args 12, claude_code 36, opencode 51, stub_launch 16, check-conformance ok, test-check-conformance ok, workspace --no-run ok (merged tree); review round 2 fixed 9 minor |
| H1 | Member hooks: workspace events, blocks, busy, session start and resume note | W2, W6, W9, C1, L2, P2, HUD-11, HUD-10, W10, W11, FIX-17 | TODO | judgment | — |
| H2 | HUD workspace phase segment (moved from the HUD graph) | C1, W2, HUD-8, HUD-9 | DONE | implementation | c10c913; acceptance statusline_phase 13, statusline_render 24, workspace --no-run ok (merged tree) |
| M1 | CLI verbs, doctor line, loadout checks, results sweep additions | W8, W10, W5, W6, W4, H1, HUD-12, LIV-2, FIX-17 | TODO | judgment | — |
| M2 | zygo ws run, member wrapper, workspace test harness | M1, L1, L2, L3, W4, P1, P2, FIX-13 | TODO | judgment | — |
| M3 | Hand-started membership for zygo claude and zygo opencode | M2, W6 | TODO | judgment | — |
| M4 | Override mirror into member records; /zygo:down, /zygo:ls, /zygo:workspace | M1 | TODO | judgment | — |
| M5 | Auto-update check for zygo up (moved from the HUD graph) | W7, M1, M2, UPD-1 | TODO | implementation | — |
| E1 | End-to-end up/restore checks and PW11 re-check | M2, M3, H1, W10, L3 | TODO | implementation | — |
| E2 | End-to-end down, drain, slash, overrides, OpenCode checks; PW15 re-check (Bash, tool, slash) | E1, M4, W11, FIX-9 | TODO | implementation | — |
| D1 | Base-spec amendments of W§15 | E2, DOC-1, M5, FIX-10 | TODO | implementation | — |
| FIX-1 | request_path tests: a refused port that no other test can take | — | DONE | judgment | c86275b; acceptance request_path 20 runs, each 48 passed, 0 failed (merged tree) |
| FIX-3 | stub_member test: no cloned_ref_to_slice_refs warning | — | DONE | mechanical | 6921b2f; acceptance clippy -D warnings clean, stub_member 16 (merged tree) |
| FIX-4 | live scripts state the agents they need; lib.sh checks only those | — | DROPPED(premise false: OpenCode 1.19.6 is at ~/.opencode/bin/opencode, not on the default PATH; live runs put that directory on PATH; lib.sh needs no change) | implementation | — |
| FIX-5 | HUD fit width: status area = terminal width − 4 (PH4 fallback; changes HUD-8) | — | DONE | implementation | 7c26d5d; hooks 173 ok, compact_cmd 16 ok, golden unchanged (merged tree) |
| FIX-7 | Live framework: selftest socket and tmux config, live_end keeps a caller-set live-<pid>, live-hud comment path | — | DONE | implementation | 5e67820; lib 28 ok, selftest 2 ok, live-hud selfcheck 17 ok, check-premises ok (merged tree) |
| FIX-8 | Triage: one auxiliary 429 in each passthrough session (LIV-1 D8) | — | DONE | judgment | e79d418; passthrough 33 ok, root cause section present (merged tree); verdict not determined, follow-up FIX-10 |
| FIX-9 | live-tmux.sh 6.30: accept window-unlinked as a session hook on tmux 3.3 and later | P1, P2 | DONE | mechanical | 907b06d; 6.30 ok on 3.7c and 3.2a; PW6 3.7c 35 of 39 |
| FIX-10 | Record the upstream error type and message of a non-2xx passthrough answer (FIX-8 follow-up) | FIX-8, DOC-1 | DONE | implementation | f273eed; passthrough 53, store 34, proxy 391; review round 2 fixed B1 (secret pass before login scrub) and 9 minor |
| FIX-11 | HUD design: the statusline verb works the same in an OpenCode session | PRB-1, LIV-2 | DONE | mechanical | 7ee20d6; acceptance ok |
| FIX-12 | Python premise tests leave scripts/premises/__pycache__ | PRB-1 | DROPPED(resolved by PRB-1 round 2 c759e25: hud_probe_test.py sets sys.dont_write_bytecode) | mechanical | — |
| FIX-13 | Stub agent reads the one-token = session arguments --session-id=<id> and --resume=<id> | L2 | DONE | mechanical | 3be5519; acceptance stub_launch 17, stub_member 19 (merged tree); review round 2 fixed 3 minor |
| FIX-14 | state tests: the hold helper retries the lock (single try_lock gave WouldBlock in a parallel run) | W6 | DROPPED(premise false: tests/state.rs starts no process and each test has its own TempDir; the observed WouldBlock was in tests/snapshot.rs, fixed by W6 R3-D5) | mechanical | — |
| FIX-15 | HUD number form short rolls over to b after 999m (1000m today) | — | DONE | mechanical | 79a70fc; acceptance statusline lib 4, statusline_render 25 (merged tree); review round 2 added 3 minor tests |
| FIX-16 | workspace_down passes the name after -- | W10, W11, M1 | TODO | mechanical | — |
| FIX-17 | W2 state store hardening: refuse a linked state folder, cap the state read at 1 MiB | W8 | IN-PROGRESS | implementation | .worktrees/phase2/FIX-17 |
| FIX-18 | Local sessions do not expire by idle time; the idle TTL applies to team sessions only | — | PLANNED | implementation | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-21 | Graph created by merging `docs/work/hud-update/` and `docs/work/workspaces/` (lead decision): HUD and update nodes first, workspaces nodes build on them by Depends edges. |
| 2026-11-21 | Review of the merged graphs: 18 findings fixed; 25 rows here, the rest in backlog.md (full-machinery cap). |
| 2026-11-21 | Operator: a pinned agent version (docs/premises.md) is a minimum, not an exact version. The exact-match check pinned_agents in scripts/premises/live.sh is a defect; PRB-1 fixes it. |
| 2026-11-21 | HUD-1 and HUD-2 planned; plan verification passed (HUD-1 one round; HUD-2 three rounds, all confirmed findings fixed). New edge HUD-8 → HUD-2 (both change Cargo.lock). |
| 2026-11-21 | PRB-1 plan draft committed with its review open (22 findings). Held until the operator approves the OpenCode HUD design (docs/specs/2026-11-21-opencode-hud-design.md), which changes PH13 and HUD-3, -8, -9, -11. |
| 2026-11-21 | PRB-2 added from PRB-1 planning: docs/premises.md and docs/live-checks.md describe pins as exact; scripts/premises/lib.sh leaves N2_SCRATCH (with results.txt) after each live run. W3 moved to backlog.md (25-row cap). |
| 2026-11-21 | HUD-1 DONE (0f7ec77). Review: 4 low findings, all refuted (diff-range brief error; node-file ticks required; ANALYZE plan choice not a defect; error cases match summary). Floor scoped to zygo-usage per CLAUDE.md. |
| 2026-11-21 | Controller decision: PRB-2 owns the whole pin-minimum code change (scripts/premises/ver.sh, live.sh pinned_agents and live_end, check-premises.sh agent rows, docs). PRB-1 keeps only the LIVE_VER sites in live-*.sh; edge PRB-1 → PRB-2. |
| 2026-11-21 | PRB-2 planned; plan verification passed after three review rounds (all confirmed findings fixed). New edges PRB-1 → HUD-2 and HUD-5 → HUD-2: HUD-2 now edits the HUD spec (H§7.5, AU11), which both also change. |
| 2026-11-21 | HUD-2 round 2 (review fixes cdb0e63); CI green on ubuntu, macos and windows (run 44962380990). OpenCode HUD design: comprehension read passed after 3 runs; critic read running. |
| 2026-11-21 | HUD-2 DONE after 3 rounds (97e70a0, cdb0e63, 1454eab); CI green on ubuntu/macos/windows. Open: H§7.5 no-marker re-check with the new calls (sandbox refused git config); covered by HUD-9 H§14 #25. |
| 2026-11-21 | HUD-5 planned; plan verification passed after two review rounds (served row values stored at request time; status reply omits absent fields). |
| 2026-11-21 | PRB-2 DONE after 2 rounds (fcab8dc, 9d1f6c6). Acceptance test counts rose from ver 45/lib 21 to ver 50/lib 27 with review cases (SIGPIPE, live_end rm, long fields, C1 controls); the check itself is unchanged. |
| 2026-11-21 | PRB-1 re-plan note: its draft steps 1, 2, 8, 14 predate PRB-2 (ver.sh, agent_ok, results.txt appends). Re-plan PRB-1 against PRB-2's Produces and the OpenCode HUD design before it starts. |
| 2026-11-21 | HUD-5 DONE: 3 rounds; round 3 fixed a test race (read_then_drop closed before the usage data line). |
| 2026-11-21 | Operator: the OpenCode HUD design is "same as Claude Code". docs/specs/2026-11-21-opencode-hud-design.md is not authority; no node waits for it. |
| 2026-11-21 | Lead decision: PRB-1 no longer gates HUD-8 and HUD-9 (edges removed). Its plan needs operator answers in a terminal; every H§12 premise has a fallback; the premises are checked with the real HUD in LIV-1. |
| 2026-11-21 | Nodes run without per-node review rounds (operator: no review loops); the lead runs each acceptance check on the merged tree. |
| 2026-11-21 | HUD-3 DONE (77323a8, merge 71181de). |
| 2026-11-21 | HUD-4, HUD-7, UPD-1 and HUD-6 DONE. HUD-6: a team session's status reply has no spend_usd and no requests_priced until HUD-10 adds the built-in list price cost (AU14). |
| 2026-11-21 | Reported by nodes, for DOC-1: H§7.3 step 2 and the H§9 row do not list long_context.cache_write_5m (contract and H§11 do); stale line references in H§7.3 (schema.rs:216-223, catalog.toml:111); the fixed sessions line of the auto-update design reads "1 zygo sessions are running". |
| 2026-11-21 | HUD-8, HUD-9, HUD-10, HUD-11, HUD-12 DONE. Full floor on the merged tree (Linux): cargo test --workspace --locked --no-fail-fast 0 failed; sh scripts/test-install.sh all passed. Windows and macOS run in CI on the PR. |
| 2026-11-21 | Open, not run: PRB-1, LIV-1, LIV-2 (manual live checks on the operator's machine); DOC-1 waits for LIV-1. |
| 2026-11-21 | For DOC-1: H§3, H§8 and H§14 #33 still say the statusline session form exits 2 in an OpenCode session; the operator decision is "same as Claude Code" and HUD-11 built that. The verb stores the style in an OpenCode session, but no zygo surface shows a HUD there yet. |
| 2026-11-21 | H§9 base-spec notes for §10.7 and §17 doctor are not written. The contract text of HUD-9 names $ZYGO_SESSION_DIR/log; the real file is session.log. |
| 2026-11-21 | Found, not fixed (needs a node): crates/zygo-proxy/tests/request_path.rs fallback_all_failed_removes_pin failed once in three full runs (refused_port binds and releases a port that another test can take). |
| 2026-11-21 | zygo_hooks::statusline::trimmed (used by zygo status) still reads Store::summary, not Store::bytes_trimmed. |
| 2026-11-21 | Resume: graph check failed (PRB-1 shares files with LIV-1 and LIV-2 without an edge; 3 long Log entries). Controller decision: order LIV-1 → PRB-1 → LIV-2; long entries split verbatim. |
| 2026-11-21 | C1 DONE (one round). Plan label fix: a bold Consumes/Produces label in a Plan section is read as a contract field by the validator; plans use other labels. |
| 2026-11-21 | X1 DONE (one round). In-flight count is a drop guard held by Pending; the serve-mode case is tested as a team session on the shared router, not on a TLS listener (tests/serve.rs is not in X1 Files). |
| 2026-11-21 | W1 DONE (one round). identity::worktrees runs git itself, bounded by the checkout deadline (AU77: repo.rs unchanged; zygo-work has no public bounded git): a third copy of the bounded-run loop. No test covers a git that hangs past the deadline. |
| 2026-11-21 | L3 plan notes for later nodes: a held request is held by the server side (E2 #42 needs a provider that holds its reply); one control directory serves one live stub (M2 gives each member its own ZYGO_STUB_CONTROL and ZYGO_STUB_OUT); the stub runs no OpenCode plugin (E2 #74). |
| 2026-11-21 | L3 DONE (two rounds). Round 2: the held-request test waits for the held reply before it asserts the open state. cargo test -p zygo alone does not build stub-agent; common::stub_agent() names the build command. |
| 2026-11-21 | W5 plan design calls (author's call, spec fixes none): a loadout with no window is an error; LoadoutPane.kind is a string checked by config check; built-in solo lives in zygo-workspace; drain_timeout has no check (W10 or M1 handles a refused value). |
| 2026-11-21 | W4 plan, author's calls where AU30 is silent: every positional is removed for both agents (an OpenCode project path is not restored); --teleport is removed as an optional-value session flag. For L2: has_session_arg must count --teleport. |
| 2026-11-21 | W4 plan notes: args::filter resolves file paths against the process cwd (matters to W7 and to config check in M1). For W7: W§8.1 step 10 says each name of AU64, but the W7 contract consumes only env::REMOVE_ALWAYS. |
| 2026-11-21 | W2 plan, author's calls: validation also needs id equal to the directory name and version 1; errors.log is cut to its newest lines within 32 KiB when a line would pass 64 KiB; the .tmp sweep of W§7.3 is in Store::update. |
| 2026-11-21 | W5 DONE (one round). W§18 #63 is covered in part: the --settings warning and the checks that need parse_window, AU30 or AU98 are M1's. An older zygo (0.1.1) accepts a config with [workspace] and [loadout.x]: config check ok. |
| 2026-11-21 | L1 plan, author's calls: an AU34 callback error is one session-log line "session callback: <escaped text>" (the spec gives no text); LaunchOutcome and SessionCallback are reached as zygo_launch::launcher::… (lib.rs is not in L1 Files; M2 imports by that path). |
| 2026-11-21 | W4 DONE (two rounds). has_user_info also reads https:/u:p@host as user info (the plan's rule missed it). Round 2: a fixture test pins the short-flag set of both helps, so a new short flag in a later pin fails until it is classified. |
| 2026-11-21 | W2 DONE (one round). Store::update is a read-modify-write under lock (proved: without the lock the concurrent-writer test loses changes). log_error is not serialized between processes: two rewrites of a full errors.log at once can lose lines. |
| 2026-11-21 | H2 plan, author's calls: workspace.json is read up to 64 KiB (the STDIN_CAP value; the specs give no bound) and only when it is a regular file; the reader returns the phase string unfiltered and writes no stderr line; version must equal 1. |
| 2026-11-21 | L1 DONE (one round). Launcher::run returns LaunchOutcome; zygo claude and zygo opencode exit with .code, the same integer as before. received and signal are unix-only facts; on Windows they are false and none. |
| 2026-11-21 | Found, not fixed (needs a node): crates/zygo-launch/tests/stub_member.rs:297 has the clippy warning cloned_ref_to_slice_refs (from L3). No test fails; a clippy run with -D warnings would. |
| 2026-11-21 | H2 DONE (two rounds). Round 2: the phase reader opens workspace.json without blocking and checks the opened handle, so a FIFO at the path cannot make the HUD wait (C6). With O_NONBLOCK present, no test separates the handle check from it. |
| 2026-11-21 | Stop: every open node now waits for a live check. LIV-1 and P1 are ready and need the operator's machine and login; P2 needs opencode on PATH. PRB-1, LIV-2, DOC-1 follow LIV-1; W3 and the nodes behind it follow P1 and P2. |
| 2026-11-21 | FIX-1 and FIX-3 added from the found defects (execute-work §5.4): the refused_port race in request_path.rs and the clippy warning in stub_member.rs:297. Change-log lines are in the base spec and the workspaces spec. |
| 2026-11-21 | statusline::trimmed on Store::summary: not a defect (same value, store.rs:439-466; no spec text requires bytes_trimmed for zygo status; HUD-12 design call 8). No node. |
| 2026-11-21 | FIX-3 DONE. cargo clippy -p zygo-launch --all-targets --locked -- -D warnings is clean. |
| 2026-11-21 | FIX-1 DONE. refused_port is the fixed port 1 (a port 0 bind never gives a port below 1024); the proof test failed on the old helper. Assumption: nothing listens on TCP port 1 on a developer or CI machine. |
| 2026-11-21 | Observation (no node): the repository has no rustfmt.toml and cargo fmt --check reports differences across zygo-proxy; no format gate is in the verification floor. |
| 2026-11-21 | PR 7 opened for the branch at 45c4d96 (C1, X1, W1, W2, W4, W5, L3, L1, H2, FIX-1, FIX-3). CI green on ubuntu, macos and windows (runs 44984706007, 44984755165). Local floor on 45c4d96: workspace tests 0 failed; test-install.sh passed. |
| 2026-11-21 | Resume point: 25 DONE, 22 TODO. Ready: LIV-1 and P1 (live, operator's machine). Not run: PRB-1, LIV-1, LIV-2, P1, P2, E2. Open operator question: who runs the live checks; P2 needs opencode on PATH. |
| 2026-11-21 | PR 7 merged (41ede20). Operator: the live checks run from the executor's session on this machine, with passthrough and the operator's Claude Code login ("you can test with passthrough"). |
| 2026-11-21 | FIX-4 added: scripts/premises/lib.sh exits when opencode is not on PATH, so no live script runs on this machine (no opencode). A script now states the agents it needs. LIV-1 and P1 depend on it. A placeholder opencode on PATH was refused as a fake. |
| 2026-11-21 | Correction: this machine has OpenCode 1.19.6 at ~/.opencode/bin/opencode, not on the default PATH. The Log row above that says "no opencode" is wrong. FIX-4 DROPPED, its edges removed. Live runs put ~/.opencode/bin on PATH; P2 can run here. |
| 2026-11-21 | LIV-1 and P1 planned. Live runs are from the executor's session with the OpenCode directory on PATH. Expected from the planners' probes: PW6 failed on tmux 3.2a (first window is 80x23 after new-session -x -y) and PH4 failed. |
| 2026-11-22 | LIV-1 DONE: P-HUD-1 on the real Claude Code 2.1.330. PH4 failed: /dev/tty does not answer the hook; COLUMNS reaches it; the status area is 4 cells narrower than the pane (indent 2, right gap 2). Per the contract a new node changes HUD-8 before DOC-1. |
| 2026-11-22 | LIV-1 found: killed status line runs write no cache, so the 30 s back-off never starts (HUD-9, data.rs:654-657); live-selftest.sh leaves its tmux socket file; live-hud.sh:17 comment names the wrong path; every passthrough session gets one auxiliary 429. New nodes follow. |
| 2026-11-22 | LIV-1 cost: 4 live runs, 3 main-loop model requests with an answer. Claude Code wrote 3 trust entries in .claude.json and 6 folders under projects/ of the operator's Claude folder, for scratch paths. |
| 2026-11-22 | P1 DONE. PW6 failed: on 3.2a the first window after new-session -x -y is 80x23 (3.7c: 173x47); after-kill-pane, pane-exited and window-unlinked do not fire when their target is gone and another session is current, on 3.2a and 3.7c. |
| 2026-11-22 | P1: PW15 failed with the Claude Code sandbox on (the setsid sleeper dies; the command runs in its own mount namespace); with the sandbox off it passes. PW11 verified on both tmux versions. 20 main-loop requests over 5 runs of live-detach.sh. |
| 2026-11-22 | P1 found: live.sh live_end removes a caller-set N2_SCRATCH (lib.sh:2 says it is kept); live-selftest.sh:291 starts tmux -L without -f /dev/null. A tab in a tmux -F format prints as _ (AU99 must not use a tab separator). |
| 2026-11-22 | Hold: W3, W6, W7, W10, M1, M4, E2 wait for the PW6 and PW15 fallback decisions (W§19). The PW6 hook fallback is an operator question per the P1 contract. DOC-1 waits for FIX-5 (PH4). |
| 2026-11-22 | FIX-5, FIX-7, FIX-8 added from the LIV-1 and P1 findings. DOC-1 now depends on FIX-5 (LIV-1 contract). PRB-1 and LIV-2 run after FIX-5 and FIX-7, so the manual checks see the fixed HUD and framework. |
| 2026-11-22 | Not a defect (no node): a status line run killed by Claude Code writes no cache, so the next fetch comes after the 5 s lease retake, not the 30 s back-off. H§7.7 designs the retake for a dead run; at the shipped 30 s refresh the effect is small. Lead's call. |
| 2026-11-22 | FIX-7 reverses PRB-2's live_end choice for a caller-set N2_SCRATCH: live-<pid> is kept (docs/premises.md:17); lib_test C3 changes, C6 is new. |
| 2026-11-22 | PW6 6.11 (author's call, accepted): minimum stays tmux 3.2; up resizes the first window and unsets its window-size (AU96); W3, W7 re-planned; M1 unchanged for PW6. A 3.3 minimum was rejected: Ubuntu 22.04 LTS ships tmux 3.2a. |
| 2026-11-22 | PW6 hooks (6.32-6.38): still an operator question (W§17 note); W7 waits for the answer; W3, W6 and W10 do not. |
| 2026-11-22 | PW15: W11 added (AU65); edges W11 → H1 (Cargo.lock) and W11 → E2; W10 and M1 contracts gain the pipe case and the AU18 sandbox guard (mnt_ns); M4 unchanged (the slash path is a hook; E2 checks it with the sandbox on). |
| 2026-11-22 | FIX-9 added: the live-tmux.sh 6.30 pass test is wrong for tmux 3.3 and later (window-unlinked is a session hook there; 3.7c options-table.c:1710). Edges P2 → FIX-9 and FIX-9 → E2: shared docs/premises.md and W§17. The graph has 25 open rows. |
| 2026-11-22 | FIX-5 DONE (7c26d5d). HUD fit width is the terminal width less 4 (PH4 fallback). Acceptance on the merged tree: zygo-hooks 173 passed, compact_cmd 16 passed, golden files unchanged. |
| 2026-11-22 | PW6 hooks: option A applied, author's call; W7, M2, E1 no longer wait. W§7.4: window-layout-changed and the member wrapper exit snapshot record pane loss; a shell-only window closed elsewhere waits for the next trigger (W§18 #24, #77). |
| 2026-11-22 | FIX-10 added from FIX-8 (verdict not determined): a non-2xx passthrough answer writes its upstream error.type and error.message to a new usage table (base §14). Edges FIX-8, DOC-1 → FIX-10 and FIX-10 → D1: shared base spec. |
| 2026-11-22 | PRB-1 contract drift fixed: the executor runs the live check with passthrough (operator, 2026-11-21); Files gain docs/live-checks.md and lose live.sh; a failed line re-plans LIV-2 and gives one fix node for the DONE nodes. PRB-1 PLANNED. |
| 2026-11-22 | LIV-2 (author's call): no Windows machine; PH8 and PH10 fallback applied by default (plain icons on Windows unless ZYGO_NERD_FONT=1, AU2), W-SHELL part 2 not run. LIV-2 builds the code; edge LIV-2 → M1 (usage_doctor.rs). |
| 2026-11-22 | E2: the executor runs the PW15 re-check itself with passthrough (operator, 2026-11-21), as P1 did. |
| 2026-11-22 | FIX-7 DONE (5e67820). Selftest socket removed with its own tmux config; live_end keeps a caller-set live-<pid>; live-hud comment path fixed. Merged tree: lib tests 28, selftest 2, live-hud selfcheck 17, check-premises ok. |
| 2026-11-22 | FIX-8 DONE (e79d418). Verdict not determined; zygo is not the cause: the first auxiliary request of each Claude Code start was answered 429 in 24 of 24 passthrough sessions. Follow-up node FIX-10 (record upstream error type and message). |
| 2026-11-22 | P2 planned: 25 steps; 8 ids on the mock upstream, 5.5 on the stored login; its docs/live-checks.md entry moves to LIV-2, edge P2 → LIV-2 (author's call). |
| 2026-11-22 | W11 planned: 9 steps; plan review B1 (fork lock) and B2 (group kill via repo_file::kill_tree) fixed. |
| 2026-11-22 | DOC-1 planned: 19 steps (14 H§9 rows, 3 auto-update rows, change log, check); in-place rewrites marked 2026-11-22. Log findings applied: long_context.cache_write_5m, OpenCode statusline verb as Claude Code, sessions line as <n>. |
| 2026-11-22 | DOC-1 DONE (fdcea4a). Base-spec notes for the HUD (H§9) and auto-update written; review round 2 fixed 12 findings. Acceptance on the merged tree printed nothing. |
| 2026-11-22 | FIX-11 and FIX-12 added from DOC-1 and PRB-1 findings: HUD spec H§3, H§8, H§14 #33 still say the OpenCode statusline session form exits 2 (HUD-11 built it as Claude Code); hud_probe_test.py leaves scripts/premises/__pycache__. |
| 2026-11-22 | W11 DONE (0799b7d). MCP tool workspace_down behind [features] workspace_tool (AU65, PW15 fallback); review round 2 fixed F1-F7. Acceptance on the merged tree: mcp_server 50 passed, config_layers 96 passed, workspace --no-run ok. |
| 2026-11-22 | FIX-10 planned: 24 steps. Contract fixed: Files gain crates/zygo-proxy/src/tee.rs (usage writer, body copy); a 4xx other than 429 is relayed too. Any non-2xx passthrough answer gives a row; message secret-redacted, 300 chars; SSE error events not counted. |
| 2026-11-22 | FIX-10 plan review fixes: 27 steps. Parse and secret pass move to the usage writer (16 KiB copy, 2 KiB field cap), error.type redacted too, one transaction, login scrub with partial runs, §14 Never line rewritten. Files gain redact_stage.rs, server.rs. |
| 2026-11-22 | PRB-1 DONE (b318a6d) on Claude Code 2.1.330: PH4 failed (FIX-5 fallback), PH12 observed, 9 verified; merged-tree acceptance exit 0. Cost: 0 main-loop requests, about 8 auxiliary in all. Claude Code wrote 3 trust entries in the operator's config. |
| 2026-11-22 | FIX-12 DROPPED: PRB-1 round 2 (c759e25) sets sys.dont_write_bytecode in hud_probe_test.py, so no scripts/premises/__pycache__ is left. |
| 2026-11-22 | PRB-1 merge landed in c64acac by a concurrent commit; content verified, no history rewrite. |
| 2026-11-22 | P2 DONE (7657585): PW7, PW16 failed, 7 verified. Cost: 12 real requests. Side effect: 6 empty projects/<scratch>/memory folders in the operator's Claude folder, left in place. live-checks.md side-effect note added by round 2 M2, author's call. |
| 2026-11-22 | Author's call from P2 and the W3 probe: AU61 resume drops CLAUDE_CONFIG_DIR when the saved config dir is none (PW16); AU87 adds `no such session:`. W7, W10: read @zygo-ws only after session_exists; W7: window hooks with -w. H1 consumes the W9 subagent methods. |
| 2026-11-22 | LIV-2 planned: 21 steps; plan review B1-B3 and M1-M10 fixed; data.rs and the HUD spec added to Files (author's call) |
| 2026-11-22 | W3 planned: 19 steps; review B1-B4 and M1-M11 fixed; B5 found by probe: tmux -u needed under LC_ALL=C; spec AU87, W§13 updated (author's call) |
| 2026-11-22 | FIX-10 DONE (f273eed): a non-2xx passthrough answer records its upstream error type and message. Merged tree: passthrough 53, store 34, proxy 391 ok, forbidden_content ok. Review round 2 fixed B1 (secret pass before login scrub) and 9 minor. |
| 2026-11-22 | LIV-2 DONE (37c78eb): plain icons on a Windows build unless ZYGO_NERD_FONT=1 (PH8, PH10 fallback). PH8, PH10, W-SHELL part 2 not run: no Windows machine. Merged tree: statusline_render 25, usage_doctor 46, zygo-hooks all ok, check-premises ok. |
| 2026-11-22 | W9 planned: 22 steps; review B1-B3, M1-M12 fixed; AU26 rule 3 order and read-error busy, skipped rule 3 gives no interrupted-turn idle (author's call) |
| 2026-11-22 | L2 planned: 18 steps; review B1-B3, 12 minor fixed; OpenCode turn start = first session.status busy (AU27); session flags from W4 args.rs; Files add launcher.rs, args.rs, Cargo.lock (author's call) |
| 2026-11-22 | W9 DONE (e167624): activity.json under fs4 lock, drain idle rule. Merged tree: activity 33 + 1 ignored, drain 27, workspace 111, clippy clean. Round 2: Busy gets rule_skipped; W10 Spec prints the skip line for it. |
| 2026-11-22 | FIX-9 planned: 6 steps, test first. No offline test for live-tmux.sh, so live runs on tmux 3.7c, red then green, and 3.2a are the test; 6.30 uses ver_ge 3.3 for session 5, window 3. |
| 2026-11-22 | W3 DONE (f945872): tmux Multiplexer layer. Merged tree: mux 42 + 1 ignored on 3.7c and 3.2a, workspace 153, clippy clean. Round 2: windows named by rename-window; 3.2 -n name with escapes is a known limit (AU99); W6, W7 Spec notes. |
| 2026-11-22 | FIX-9 DONE (907b06d): 6.30 takes window-unlinked as a session hook on tmux 3.3+. Merged tree: 6.30 ok on 3.7c and 3.2a; PW6 3.7c 35 of 39, 3.2a 34 of 39; pw11 unchanged; check-premises ok, forbidden_content 6 ok. |
| 2026-11-22 | FIX-11 planned: 6 steps, test first. Spec text only, so the Acceptance rg is the test: red on 2 lines, then H§3, H§8, H§14 #33 rewritten as final text and one change-log row. |
| 2026-11-22 | FIX-11 DONE (7ee20d6): HUD spec H§3, H§8, H§14 #33 say the statusline verb works the same in an OpenCode session, as HUD-11 built it. Merged tree: acceptance ok; checkout: forbidden_content 6 ok, check-premises ok. |
| 2026-11-22 | L2 DONE (c5a1569): member hooks, session args from W4, launcher member drop, conformance check test. Merged tree: args 12, claude_code 36, opencode 51, stub_launch 16, conformance ok. R2 spec changes applied. FIX-13 added: stub reads = session forms; M2 depends on it. |
| 2026-11-22 | FIX-13 planned: 6 steps, test first. Acceptance adds --test stub_member, because the split-form cases are there; stub_member not changed. -s=<id> not read; empty = value is no session arg (author's call). |
| 2026-11-22 | W6 planned: 22 steps, test first. C1-C16 applied: W2, W3 Produces; W7, W10, M1, M2, M4 Spec; H1 runs the pending snapshot in a detached child; spec W§7.1-7.4, W§10, AU38, AU84, AU89. Adds snapshot_seq to state.rs (author's call). |
| 2026-11-22 | FIX-13 DONE (3be5519): stub reads --session-id=<id> and --resume=<id>. Merged tree: stub_launch 17, stub_member 19, zygo-launch all pass. R2: Files add stub_member.rs. Spec W§7.3 wait bounds: up/down snapshots 5 s (author's call). |
| 2026-11-22 | W6 DONE (8978541): snapshot. Merged tree: snapshot 33, state 22, zygo-workspace all pass, check --tests clean. I1-I5, R1-R8, S1-S8 applied: spec W§7.3, W§7.4; W3 gains Multiplexer::socket. FIX-14 added: state.rs hold retries the lock. W7 ready. |
| 2026-11-22 | FIX-14 planned: 6 steps, test first. The race is not forked in the test: a second handle holds the lock and a thread drops it after 50 ms, then hold must return (author's call). |
| 2026-11-22 | FIX-14 DROPPED after its diff review: premise false. tests/state.rs starts no process, each test binary is its own process, each test has its own TempDir; the observed WouldBlock was in tests/snapshot.rs, fixed by W6 R3-D5. Branch discarded unmerged (author's call). |
| 2026-11-22 | W7 planned: 39 steps, test first. C1-C15 applied: W3, W5 Produces; W8, W10 Consumes and Spec; spec AU30, AU96, AU98, W§7.4, W§8.1 steps 3, 5, 7, 9, 12, W§10, W§13. Adds Multiplexer::rename_window, NewSession.owner, args::filter_from (author's call). |
| 2026-11-22 | W7 DONE (3d51ad4): zygo up. Merged tree: up 45, mux 46 + 1 ignored, snapshot 33, state 23, args 13, 3.2a first_window 2. S1-S5, RS1-RS7, TS1-TS9 applied. The R3-7 pending check after the undo has no test (accepted). W8, W10 ready. |
| 2026-11-22 | FIX-15 added and planned (operator defect): short renders 1000m for 1e9. 6 steps, test first; spec H§6.6 gains 1b, 1.2b. status and help reuse short; no other formatter of the kind; no unit above b (author's call). |
| 2026-11-22 | FIX-15 DONE (79a70fc): short rolls over to b after 999m. Merged tree: statusline lib 4, statusline_render 25, zygo-hooks 173, zygo 378 pass. R2 added b tenths, b-range tokens and ctx tests; no compact 1b test, window cap 10m. |
| 2026-11-22 | W8 planned: 35 steps, test first. C1-C8 applied: W7 Produces (member_live, socket_is_private, fire), W3 Produces (mux::BOUND), M1 Spec; spec W§8.1 up --all, W§12 rows workspace, ls, ws forget, ws forget-pane. |
| 2026-11-22 | W10 planned: 30 steps, test first. C1-C4, C6-C18 applied: W2, W7, W9 Produces; M1 Spec, Consumes; spec AU18, AU97, W§9.3, W§11, W§12, W§18 #80, W§19. C5 as FIX-16 (FIX-15 taken), depends W10, W11, M1 (author's call). |
| 2026-11-22 | W8 DONE (0921fb6): up --all, ls, status, forget, forget-pane. Merged tree: admin 21, up 45, up_all 7, zygo-workspace 267 pass. I1, I2, I4, R2 spec rows applied. FIX-17 added: store refuses a linked state folder, caps reads at 1 MiB; H1, M1 depend on it. |
| 2026-11-22 | FIX-17 planned: 9 steps, test first. One variant StateError::NotADirectory { path, link }; read refuses a linked folder too; cap 1 MiB, exactly 1 MiB reads; session_dir.rs unchanged; up.rs:370 uncapped read reported, not changed (author's call). |
| 2026-11-22 | FIX-18 added and planned (operator question): a local session expired after 24 h idle while its launcher ran. 8 steps, test first; spec §7.2, §12.5. Rule keys on team_label; serve sessions always have one; local expires_at empty (author's call). |
