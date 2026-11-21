# Workspaces Work Graph

<!-- work-graph -->

**Goal:** Build zygo phase 2 workspaces (`zygo up`/`down` with drain, snapshot and resume of each agent in its own conversation) as the approved design specifies.
**Authority:** `docs/specs/2026-11-20-workspaces-design.md` (W§n; approved by the operator 2026-11-21), `docs/specs/2026-11-18-zygo-design.md` (§n; base spec, changed only where W§15 says)
**Run:** parallel=3 · worktrees=.worktrees/workspaces/ · disk-per-node=—
**Next:** P1, P2

To continue in any session: "resume the work in `docs/work/workspaces/graph.md`".

## Global constraints

- Base constraints C1-C10 apply; C6 applies to every new hook: a hook never breaks the agent — docs/specs/2026-11-20-workspaces-design.md W§4 W-C1
- No daemon; tmux is the only long-lived process — docs/specs/2026-11-20-workspaces-design.md W§4 W-C2, W§2 W-G6, AU1
- tmux options and hooks only on zygo's own sessions; never a global option or hook — docs/specs/2026-11-20-workspaces-design.md W§4 W-C3
- Every state write is atomic (temporary file, then rename) — docs/specs/2026-11-20-workspaces-design.md W§4 W-C4, W§7.3
- Loadouts and `[workspace]` come from user config only — docs/specs/2026-11-20-workspaces-design.md W§4 W-C5, AU10, AU22
- `zygo claude`/`zygo opencode` outside a workspace stay a clean v1 launch — docs/specs/2026-11-20-workspaces-design.md W§4 W-C6, OP9
- Workspace state holds no secret and no content — docs/specs/2026-11-20-workspaces-design.md W§4 W-C7, W§7.2
- No user-controlled text reaches a tmux format argument unescaped — docs/specs/2026-11-20-workspaces-design.md W§4 W-C8, AU54, AU55
- Never resume a conversation whose launcher is live — docs/specs/2026-11-20-workspaces-design.md W§4 W-C9, AU87
- Native Windows: every workspace verb exits 2 with `workspaces need tmux; run zygo in WSL`; the code still builds there — docs/specs/2026-11-20-workspaces-design.md OP5; `.github/workflows/ci.yml` (windows-latest)
- Minimum tmux 3.2 — docs/specs/2026-11-20-workspaces-design.md AU7
- State dir `0700`, files `0600`; wait bounds hooks/snapshots 1 s, launchers/wrapper 5 s, `up`/`down` 5 s — docs/specs/2026-11-20-workspaces-design.md AU2, W§7.3
- Crate graph: `zygo-workspace` depends on `zygo-core`, `zygo-config`, `zygo-work`, `zygo-proxy` only; `zygo-hooks` depends on `zygo-workspace`; `ws run`, member wrapper and membership decision in `crates/zygo/src/cmd/` — docs/specs/2026-11-20-workspaces-design.md AU103, AU102, AU45

## Verification floor

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` — `.github/workflows/ci.yml` job `test`
- Install script, unix: `sh scripts/test-install.sh` — `.github/workflows/ci.yml` (non-Windows)
- Install script, Windows: `pwsh scripts/test-install.ps1` — `.github/workflows/ci.yml` (windows-latest)
- Live checks (manual, recorded): PW premises on the pinned agents and tmux 3.2 — docs/specs/2026-11-20-workspaces-design.md W§18 #76

## Review focus

1. CI runners without tmux (macos-latest has none by default) running private-server tests — the harness states what such a test does there; the spec sets no rule for it — test in E1 (harness), W3 (`PrivateServer`) — docs/specs/2026-11-20-workspaces-design.md W§18 preamble; `.github/workflows/ci.yml`
2. `zygo-workspace` and `zygo-hooks` build on windows-latest with unix-only code behind `cfg` — compiles; verbs exit 2 — test in M1 (W§18 #64) — docs/specs/2026-11-20-workspaces-design.md OP5
3. A streamed body that the client drops before its end — `requests_in_flight` returns to 0 and `last_request_end` is set — test in X1 — docs/specs/2026-11-20-workspaces-design.md AU91, W§19
4. The session-argument list in the adapters and the AU30 filter list drift apart — both match the pinned `--help` fixtures — tests in L2 and W4 — docs/specs/2026-11-20-workspaces-design.md AU30, W§8.3
5. A snapshot write and a member-record write on the same `workspace.json` at once — both changes kept — test in W6 (W§18 #8, #9) — docs/specs/2026-11-20-workspaces-design.md W§7.3
6. A hook waiting on `activity.lock` or `workspace.json` past its bound — one log line, exit 0, agent unaffected — test in H1 — docs/specs/2026-11-20-workspaces-design.md AU56, W-C1
7. An older zygo reading a shared user config that holds `[loadout]`/`[workspace]` — result recorded — W5 — docs/specs/2026-11-20-workspaces-design.md W§11 last bullet
8. `Cargo.lock` gains the new crate and edges; CI runs `--locked` — lock committed with W1, H1, M1 — `.github/workflows/ci.yml`

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| P1 | Live premises PW6, PW11, PW15 (tmux, pane start, detached down) | — | TODO | implementation | — |
| P2 | Live premises PW1-PW5, PW7, PW9, PW10, PW16 (agents) | P1 | TODO | implementation | — |
| C1 | Core: workspace paths, session-dir accessors, session-view member and note | — | TODO | judgment | — |
| X1 | Proxy session status: requests_in_flight, last_request_end (AU91) | — | TODO | judgment | — |
| L3 | Stub agent behaviors for W§18 and stub helpers | — | TODO | implementation | — |
| W1 | zygo-workspace crate scaffold and identity | — | TODO | judgment | — |
| W2 | Workspace state store: types, atomic write, locks, live check | W1, C1 | TODO | judgment | — |
| W3 | Multiplexer trait and tmux implementation | W1, P1 | TODO | judgment | — |
| W4 | Argument filter, bypass and user-info checks, environment cleaning | W1 | TODO | judgment | — |
| W5 | Loadout config schema, team-layer drop, structural checks, resolution | W1 | TODO | implementation | — |
| W6 | Snapshot: list-panes read, shrink rule, key assignment | W2, W3 | TODO | judgment | — |
| W7 | zygo up for one workspace: plan, build, repair, attach | W2, W3, W4, W5, W6 | TODO | judgment | — |
| W8 | up --all and workspace admin: ls, status, forget, forget-pane | W7 | TODO | judgment | — |
| W9 | activity.json under lock and the drain idle rule | W2, C1, X1 | TODO | judgment | — |
| W10 | zygo down: detach, drain loop, timeout, stop, --now, --all | W7, W9, W5, P1, P2 | TODO | judgment | — |
| L1 | Launcher member support: member, callback, resume note, LaunchOutcome | C1, L3 | TODO | judgment | — |
| L2 | Member events for Claude Code and OpenCode; WA-1/WA-2 conformance cells | L1, P2 | TODO | judgment | — |
| H1 | Member hooks: workspace events, blocks, busy, session start and resume note | W2, W6, W9, C1, P2 | TODO | judgment | — |
| R1 | Results sweep: workspace-linked session dirs and results cache | C1 | TODO | judgment | — |
| M1 | CLI verbs up/down/ls/attach/loadout/workspace/ws, doctor line, loadout checks | W8, W10, W5, W6, W4, H1 | TODO | implementation | — |
| M2 | zygo ws run and the member wrapper | M1, L1, L2, L3, W4, P1, P2 | TODO | judgment | — |
| M3 | Hand-started membership for zygo claude and zygo opencode | M2, W6 | TODO | judgment | — |
| M4 | Override mirror into member records; /zygo:down, /zygo:ls, /zygo:workspace | M1 | TODO | implementation | — |
| E1 | End-to-end harness and up/restore checks | M2, M3, H1, R1, W10, L3 | TODO | implementation | — |
| E2 | End-to-end down, drain, slash, overrides and OpenCode checks | E1, M4 | TODO | implementation | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-21 | Graph created from `docs/specs/2026-11-20-workspaces-design.md` (25 nodes; premises P1, P2 gate the nodes that depend on them; fallback nodes enter only on a failed premise, W§19). |
