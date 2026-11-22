# Phase 2 Backlog

Open rows that wait for a slot in `graph.md` (full-machinery: at most 25 open rows there).

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| M4 | Override mirror into member records; /zygo:down, /zygo:ls, /zygo:workspace | M1 | TODO | judgment | — |
| M5 | Auto-update check for zygo up (moved from the HUD graph) | W7, M1, M2, UPD-1 | TODO | implementation | — |
| E1 | End-to-end up/restore checks and PW11 re-check | M2, M3, H1, W10, L3 | TODO | implementation | — |
| E2 | End-to-end down, drain, slash, overrides, OpenCode checks; PW15 re-check | E1, M4 | TODO | implementation | — |
| D1 | Base-spec amendments of W§15 | E2, DOC-1, M5 | TODO | implementation | — |
