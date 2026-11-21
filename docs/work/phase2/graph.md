# Phase 2 Work Graph

<!-- work-graph -->

**Goal:** Build the zygo status line HUD and the auto-update check on start, then phase 2 workspaces (`zygo up`/`down` with drain, snapshot and resume), as the approved designs specify.
**Authority:** `docs/specs/2026-11-21-statusline-hud-design.md` (H§n), `docs/specs/2026-11-21-auto-update-design.md`, `docs/specs/2026-11-20-workspaces-design.md` (W§n), each approved by the operator 2026-11-21; `docs/specs/2026-11-18-zygo-design.md` (§n; base spec)
**Run:** parallel=3 · worktrees=.worktrees/phase2/ · disk-per-node=—
**Next:** PRB-1, HUD-1, HUD-2

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
- Minimum tmux 3.2, or the oldest version P1 verified when no 3.2 build is available — `docs/specs/2026-11-20-workspaces-design.md` AU7, W§17 PW6, W§18 #76
- State dir `0700`, files `0600`; wait bounds hooks/snapshots 1 s, launchers/wrapper 5 s, `up`/`down` 5 s — `docs/specs/2026-11-20-workspaces-design.md` AU2, W§7.3
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
11. Literal sites broken by new fields — `SessionStatus` (HUD-5) and `SessionView` (HUD-7) gain `Default`; later fields change no literal — HUD-5, HUD-7, X1, C1 — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 1

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| PRB-1 | Probe: temporary statusLine command for PH1-PH7, PH9, PH11-PH13 | — | TODO | implementation | — |
| HUD-1 | Usage store: Store::bytes_trimmed | — | TODO | implementation | — |
| HUD-2 | zygo-work: bounded HUD git function (rev-parse, rev-list only) | — | TODO | judgment | — |
| HUD-3 | Config tables [statusline] and [update] on today's schema; user config only | — | TODO | implementation | — |
| HUD-4 | Prices: long_context tier, cache_write_5m, Prices::cost, price sanity | HUD-3 | TODO | judgment | — |
| HUD-5 | Proxy status: model, context_window, threshold (AU28, AU29); SessionStatus Default; BYTES_PER_TOKEN | — | TODO | judgment | — |
| HUD-6 | Proxy session spend and tokens: local, passthrough notional, counts, AU33 tee | HUD-5, HUD-4 | TODO | judgment | — |
| HUD-7 | SessionView statusline_style with Default; overrides statusline; launcher writes the style | HUD-3 | TODO | implementation | — |
| HUD-8 | HUD render: segments, colors, icons, width sources and fit, golden files | HUD-5, HUD-6, PRB-1 | TODO | judgment | — |
| HUD-9 | HUD data: bounded stdin, cache v2, lease, workers, deadline, failure display | HUD-8, HUD-2, HUD-1, HUD-7, PRB-1 | TODO | judgment | — |
| HUD-10 | Team-server session spend at built-in list prices (AU14) | HUD-6, HUD-8, HUD-9 | TODO | judgment | — |
| UPD-1 | Auto-update: split check/install; start check at today's zygo claude / zygo opencode dispatch | HUD-3 | TODO | judgment | — |
| HUD-11 | statusline verb: CLI, slash command, overrides, --save, help, skill text | HUD-7, HUD-9, UPD-1 | TODO | implementation | — |
| HUD-12 | zygo status and zygo usage in tokens and spend; doctor icon line | HUD-1, HUD-6, HUD-9 | TODO | implementation | — |
| LIV-1 | Live check P-HUD-1 on Linux or macOS | HUD-9, HUD-11 | TODO | implementation | — |
| LIV-2 | Windows live part: PH8, PH10, W-SHELL part 2 (manual) | LIV-1 | TODO | implementation | — |
| DOC-1 | Base-spec notes for the HUD (H§9) and auto-update | LIV-1, HUD-10, HUD-12, UPD-1 | TODO | implementation | — |
| P1 | Live premises PW6, PW11, PW15 (tmux, pane start, detached down) | — | TODO | implementation | — |
| P2 | Live premises PW1-PW5, PW7, PW9, PW10, PW16 (agents) | P1 | TODO | implementation | — |
| C1 | Core: workspace paths, session-dir accessors, session-view workspace field | HUD-7 | TODO | judgment | — |
| X1 | Proxy session status: requests_in_flight, last_request_end (AU91) | HUD-5, HUD-6, HUD-10 | TODO | judgment | — |
| L3 | Stub agent behaviors for W§18 and stub helpers | — | TODO | implementation | — |
| W1 | zygo-workspace crate scaffold and identity | HUD-8, HUD-10 | TODO | judgment | — |
| W2 | Workspace state store: types, atomic write, locks, live check | W1, C1 | TODO | judgment | — |
| W3 | Multiplexer trait, tmux implementation, tmux test gate | W1, P1, P2 | TODO | judgment | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-21 | Graph created by merging `docs/work/hud-update/` and `docs/work/workspaces/` (lead decision): HUD and update nodes first, workspaces nodes build on them by Depends edges. |
| 2026-11-21 | Review of the merged graphs: 18 findings fixed; 25 rows here, the rest in backlog.md (full-machinery cap). |
