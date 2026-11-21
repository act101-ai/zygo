# Phase 2 Backlog

Open rows that wait for a slot in `graph.md` (full-machinery: at most 25 open rows there).

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| W3 | Multiplexer trait, tmux implementation, tmux test gate | W1, P1, P2 | TODO | judgment | — |
| W4 | Argument filter, bypass and user-info checks, environment cleaning | W1 | TODO | judgment | — |
| W5 | Loadout config schema, team-layer drop, structural checks, resolution | W1, HUD-4, HUD-7, HUD-9 | TODO | judgment | — |
| W6 | Snapshot: list-panes read, shrink rule, key assignment, pending rule | W2, W3 | TODO | judgment | — |
| W7 | zygo up for one workspace: plan, build, repair, attach | W2, W3, W4, W5, W6 | TODO | judgment | — |
| W8 | up --all and workspace admin: ls, status, forget, forget-pane | W7 | TODO | judgment | — |
| W9 | activity.json under lock and the drain idle rule | W1, C1, X1, P2, HUD-5, W2 | TODO | judgment | — |
| W10 | zygo down: detach, drain loop, timeout, stop, --now, --all | W7, W9, W5, P1, P2 | TODO | judgment | — |
| L1 | Launcher member support: member, callback, resume note, LaunchOutcome | C1, L3, HUD-7, HUD-9 | TODO | judgment | — |
| L2 | Member events for Claude Code and OpenCode; WA-1/WA-2 conformance cells | L1, P2 | TODO | judgment | — |
| H1 | Member hooks: workspace events, blocks, busy, session start and resume note | W2, W6, W9, C1, L2, P2, HUD-11, HUD-10, W10 | TODO | judgment | — |
| H2 | HUD workspace phase segment (moved from the HUD graph) | C1, W2, HUD-8, HUD-9 | TODO | implementation | — |
| M1 | CLI verbs, doctor line, loadout checks, results sweep additions | W8, W10, W5, W6, W4, H1, HUD-12 | TODO | judgment | — |
| M2 | zygo ws run, member wrapper, workspace test harness | M1, L1, L2, L3, W4, P1, P2 | TODO | judgment | — |
| M3 | Hand-started membership for zygo claude and zygo opencode | M2, W6 | TODO | judgment | — |
| M4 | Override mirror into member records; /zygo:down, /zygo:ls, /zygo:workspace | M1 | TODO | judgment | — |
| M5 | Auto-update check for zygo up (moved from the HUD graph) | W7, M1, M2, UPD-1 | TODO | implementation | — |
| E1 | End-to-end up/restore checks and PW11 re-check | M2, M3, H1, W10, L3 | TODO | implementation | — |
| E2 | End-to-end down, drain, slash, overrides, OpenCode checks; PW15 re-check | E1, M4 | TODO | implementation | — |
| D1 | Base-spec amendments of W§15 | E2, DOC-1, M5 | TODO | implementation | — |
