# HUD and Auto-update Work Graph

<!-- work-graph -->

**Goal:** Build the zygo status line HUD and the auto-update check on start, as the two approved designs specify, on top of the workspaces build.
**Authority:** `docs/specs/2026-11-21-statusline-hud-design.md` (H§n; approved by the operator 2026-11-21), `docs/specs/2026-11-21-auto-update-design.md` (approved by the operator 2026-11-21), `docs/specs/2026-11-18-zygo-design.md` (§n; base spec), `docs/specs/2026-11-20-workspaces-design.md` (W§n; for the W§15 rows that HD owns)
**Run:** parallel=3 · worktrees=.worktrees/hud-update/ · disk-per-node=—
**Next:** HP

To continue in any session: "resume the work in `docs/work/hud-update/graph.md`".

## Global constraints

- Cross-graph order: the format has no edge to another graph, so gate nodes G1-G5 stand for workspaces nodes of `docs/work/workspaces/graph.md` (G1 X1; G2 L1, implying C1; G3 W5; G4 M2 and M4, implying C1, W1, W2, W7, H1, M1; G5 E2). A gate is `DONE` only when its check finds those rows `DONE` — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 notes 1, 3, 6; graph-format §4
- Base constraints C1-C10 apply; C6 for the status line hook: a hook never breaks the agent — `docs/specs/2026-11-21-statusline-hud-design.md` H§4; base spec §4
- `[statusline]` and `[update]` come from user config only; the team layer drops them — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 6, H§9 row §6.1; `docs/specs/2026-11-21-auto-update-design.md` "Opt-out"
- No configured price leaves a server; a team session's spend uses built-in list prices — `docs/specs/2026-11-21-statusline-hud-design.md` AU14, H§9 row §7.2
- The update check sends only the GET for the public VERSION file, ignores the system proxy, and never asks inside a zygo session or without a TTY — `docs/specs/2026-11-21-auto-update-design.md` "Conflicts", "When the check is skipped", "Proxy"
- tmux-dependent tests run when tmux 3.2 or later is on `PATH` and skip with a printed reason otherwise; on Windows they are `cfg(unix)`-gated; no CI install step — operator, 2026-11-21 (no automated test in this graph uses tmux today; the live scripts of HP and HL use it by hand)

## Verification floor

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` — `.github/workflows/ci.yml` job `test`
- Install script, unix: `sh scripts/test-install.sh` — `.github/workflows/ci.yml` (non-Windows)
- Install script, Windows: `pwsh scripts/test-install.ps1` — `.github/workflows/ci.yml` (windows-latest)
- Live checks (manual, recorded): HP probe and P-HUD-1 on the pinned Claude Code — `docs/specs/2026-11-21-statusline-hud-design.md` H§12, H§14 #37, #38

## Review focus

1. A status reply from an older server with none of the new fields, and one with only the WS AU91 fields — every new field `None`, no error — test in HD2 (H§14 #17) — `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2
2. Price rounding near the long-context boundary and non-finite costs — `Prices::cost` counts them as not priced — test in HPR (H§14 #11) — `docs/specs/2026-11-21-statusline-hud-design.md` AU32
3. A team-session status reply leaking a configured price through `spend_usd ÷ tokens` — built-in list prices only — test in HTS (H§14 #15) — `docs/specs/2026-11-21-statusline-hud-design.md` AU14
4. A repository that configures programs (fsmonitor, hooks, filters, uploadpack) — no marker after a render — test in HD2 (H§14 #25) — `docs/specs/2026-11-21-statusline-hud-design.md` H§7.5
5. The update prompt on Windows with live sessions — install deferred — test in UC (auto-update #7, windows-latest job) — `docs/specs/2026-11-21-auto-update-design.md` "Other sessions"
6. Literal sites of `SessionStatus` and `SessionView` broken by new fields — HS/HSP own the status sites after WS X1; HV relies on WS C1's `..Default::default()` sites — HS, HSP, HV — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 1

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| G1 | Gate: workspaces X1 DONE | — | TODO | lookup | — |
| G2 | Gate: workspaces L1 DONE | — | TODO | lookup | — |
| G3 | Gate: workspaces W5 DONE | — | TODO | lookup | — |
| G4 | Gate: workspaces M2 and M4 DONE | — | TODO | lookup | — |
| G5 | Gate: workspaces E2 DONE | — | TODO | lookup | — |
| HP | Probe: temporary statusLine command for PH1, PH2, PH4, PH5, PH9, PH11, PH12 | — | TODO | implementation | — |
| HU | Usage store: Store::bytes_trimmed | — | TODO | implementation | — |
| HG | zygo-work: bounded HUD git function (rev-parse, rev-list only) | — | TODO | judgment | — |
| HC | Config tables [statusline] and [update]: user config only | G3 | TODO | implementation | — |
| HPR | Prices: long_context tier, cache_write_5m, Prices::cost, price sanity | HC | TODO | judgment | — |
| HS | Proxy status: model, context_window, threshold (AU28, AU29); BYTES_PER_TOKEN | G1 | TODO | judgment | — |
| HSP | Proxy session spend and tokens: local, passthrough notional, counts, AU33 tee | HS, HPR | TODO | judgment | — |
| HV | SessionView statusline_style, overrides statusline, launcher writes the style | G2, HC | TODO | implementation | — |
| HR | HUD render: segments, colors, icons, width fit, golden files | HS, HSP, HP, G4 | TODO | implementation | — |
| HD2 | HUD data: bounded stdin, cache v2, lease, workers, deadline, failure display | HR, HG, HU, HV, HP | TODO | judgment | — |
| HTS | Team-server session spend at built-in list prices (AU14) | HSP, HR | TODO | judgment | — |
| HPH | Workspace phase segment | HD2, G4 | TODO | implementation | — |
| UC | Auto-update: split check/install; start check for zygo claude and zygo opencode | HC, G4 | TODO | judgment | — |
| UU | Auto-update: one check for zygo up | UC, G4 | TODO | implementation | — |
| HVB | statusline verb: CLI, slash command, overrides, --save, help, skill text | HV, HD2, UC, G4 | TODO | implementation | — |
| HSU | zygo status and zygo usage in tokens and spend; doctor icon line | HU, HSP, HD2 | TODO | implementation | — |
| HL | Live check P-HUD-1, W-SHELL part 2 text, remaining PH results | HD2, HVB | TODO | implementation | — |
| HD | Base-spec amendments: W§15, H§9, auto-update | HL, HTS, HSU, UU, HPH, G5 | TODO | mechanical | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-21 | Graph created from `docs/specs/2026-11-21-statusline-hud-design.md` and `docs/specs/2026-11-21-auto-update-design.md` (23 nodes; gates G1-G5 stand for workspaces nodes; probe HP first; fallback nodes only on a failed premise). |
