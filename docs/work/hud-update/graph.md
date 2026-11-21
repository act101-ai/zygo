# HUD and Auto-update Work Graph

<!-- work-graph -->

**Goal:** Build the zygo status line HUD and the auto-update check on start, as the two approved designs specify, on today's code.
**Authority:** `docs/specs/2026-11-21-statusline-hud-design.md` (H§n; approved by the operator 2026-11-21), `docs/specs/2026-11-21-auto-update-design.md` (approved by the operator 2026-11-21), `docs/specs/2026-11-18-zygo-design.md` (§n; base spec)
**Run:** parallel=3 · worktrees=.worktrees/hud-update/ · disk-per-node=—
**Next:** PRB-1

To continue in any session: "resume the work in `docs/work/hud-update/graph.md`".

## Global constraints

- This graph runs first and depends on no node of another graph; `docs/work/workspaces/graph.md` starts after it is complete and builds on its code — lead, 2026-11-21 (sequential programs)
- Base constraints C1-C10 apply; C6 for the status line hook: a hook never breaks the agent — `docs/specs/2026-11-21-statusline-hud-design.md` H§4; base spec §4
- `[statusline]` and `[update]` come from user config only; the team layer drops them — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 6, H§9 row §6.1; `docs/specs/2026-11-21-auto-update-design.md` "Opt-out"
- No configured price leaves a server; a team session's spend uses built-in list prices — `docs/specs/2026-11-21-statusline-hud-design.md` AU14, H§9 row §7.2
- The update check sends only the GET for the public VERSION file, ignores the system proxy, and never asks inside a zygo session or without a TTY — `docs/specs/2026-11-21-auto-update-design.md` "Conflicts", "When the check is skipped", "Proxy"

## Verification floor

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` — `.github/workflows/ci.yml` job `test`
- Install script, unix: `sh scripts/test-install.sh` — `.github/workflows/ci.yml` (non-Windows)
- Install script, Windows: `pwsh scripts/test-install.ps1` — `.github/workflows/ci.yml` (windows-latest)
- Live checks (manual, recorded on the operator's machine): PRB-1 and LIV-1 — `docs/specs/2026-11-21-statusline-hud-design.md` H§12, H§14 #37, #38

## Review focus

1. A status reply from an older server with none of the new fields — every new field `None`, no error — test in HUD-9 (H§14 #17) — `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2
2. Price rounding near the long-context boundary and non-finite costs — not priced — test in HUD-4 (H§14 #11) — `docs/specs/2026-11-21-statusline-hud-design.md` AU32
3. A team-session reply leaking a configured price through `spend_usd ÷ tokens` — built-in list prices only — test in HUD-10 (H§14 #15) — `docs/specs/2026-11-21-statusline-hud-design.md` AU14
4. A repository that configures programs (fsmonitor, hooks, filters, uploadpack) — no marker after a render — test in HUD-9 (H§14 #25) — `docs/specs/2026-11-21-statusline-hud-design.md` H§7.5
5. The update prompt on Windows with live sessions — install deferred — test in UPD-1 (auto-update #7, windows-latest job) — `docs/specs/2026-11-21-auto-update-design.md` "Other sessions"
6. Literal sites broken by new fields — `SessionStatus` and `SessionView` gain `Default` here and their literals end in `..Default::default()` — HUD-5, HUD-7 — `docs/specs/2026-11-21-statusline-hud-design.md` H§17 note 1

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
| HUD-10 | Team-server session spend at built-in list prices (AU14) | HUD-6, HUD-8 | TODO | judgment | — |
| UPD-1 | Auto-update: split check/install; start check at today's zygo claude / zygo opencode dispatch | HUD-3 | TODO | judgment | — |
| HUD-11 | statusline verb: CLI, slash command, overrides, --save, help, skill text | HUD-7, HUD-9, UPD-1 | TODO | implementation | — |
| HUD-12 | zygo status and zygo usage in tokens and spend; doctor icon line | HUD-1, HUD-6, HUD-9 | TODO | implementation | — |
| LIV-1 | Live check P-HUD-1, PH8/PH10 on Windows, W-SHELL part 2 text | HUD-9, HUD-11 | TODO | implementation | — |
| DOC-1 | Base-spec notes for the HUD (H§9) and auto-update | LIV-1, HUD-10, HUD-12, UPD-1 | TODO | implementation | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-21 | Graph created from the HUD and auto-update designs; probe PRB-1 first; fallback nodes only on a failed premise. |
| 2026-11-21 | Sequential programs (lead): gates removed, ids renumbered, phase segment and `zygo up` check moved to the workspaces graph; 16 nodes on today's code. |
