# Phase 2 Work Graph — History

Closed node rows and Log entries moved from graph.md, verbatim. Append-only.

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| PRB-2 | Premise docs: pins are minimums; live scratch dir removed on exit | — | DONE | implementation | 9d1f6c6; acceptance ver 50, lib 27, check-premises ok, live selftest 54 (merged tree) |
| HUD-1 | Usage store: Store::bytes_trimmed | — | DONE | implementation | 0f7ec77; acceptance zygo-usage 29/29, workspace --no-run ok (merged tree) |
| HUD-2 | zygo-work: bounded HUD git function (rev-parse, rev-list only) | — | DONE | judgment | 1454eab; acceptance hud_git 21/21 (+3 ignored), workspace --no-run ok (merged tree); CI green 3 OS (run 44964062674) |
| HUD-3 | Config tables [statusline] and [update] on today's schema; user config only | — | DONE | implementation | 77323a8, merge 71181de; acceptance zygo-config 176/176 (merged tree) |
| HUD-4 | Prices: long_context tier, cache_write_5m, Prices::cost, price sanity | HUD-3 | DONE | judgment | 69b5822, merge 624dcd2; acceptance prices 27, builtin 13, workspace --no-run ok (merged tree) |
| HUD-5 | Proxy status: model, context_window, threshold (AU28, AU29); SessionStatus Default; BYTES_PER_TOKEN | HUD-2 | DONE | judgment | commits 4379678 98d9a9d c0407fa; CI 44968316690 green (ubuntu, macos, windows); acceptance on merged tree: session_api 40, window 37, zygo-hooks lib 22, workspace --no-run ok, no div_ceil(4) |
| HUD-6 | Proxy session spend and tokens: local, passthrough notional, counts, AU33 tee | HUD-5, HUD-4 | DONE | judgment | edb0f1c, merge 2899612; acceptance zygo-proxy 350/350, workspace --no-run ok (merged tree) |
| HUD-7 | SessionView statusline_style with Default; overrides statusline; launcher writes the style | HUD-3 | DONE | implementation | 5c180cc, merge 1f1967b; acceptance all 17 commands 0 failed (merged tree) |
| PRB-1 | Probe: temporary statusLine command for PH1-PH7, PH9, PH11-PH13 | PRB-2, HUD-2, LIV-1, FIX-5, FIX-7 | DONE | implementation | b318a6d; 11 LIVE lines, PH4 failed (FIX-5 fallback), PH12 observed, 9 verified; review round 2 fixed B1-B3 and 9 minor |
| HUD-8 | HUD render: segments, colors, icons, width sources and fit, golden files | HUD-5, HUD-6, HUD-2 | DONE | judgment | 320d2b2, merge 5d919a3; acceptance render 23, hooks lib 24, prices 28, session_api 45, passthrough 30, window 38, compact_cmd 11 (merged tree) |
| HUD-9 | HUD data: bounded stdin, cache v2, lease, workers, deadline, failure display | HUD-8, HUD-2, HUD-1, HUD-7 | DONE | judgment | 2a5f5ff, merge e7d95d1; acceptance zygo-hooks, zygo, zygo-config, zygo-launch all targets 0 failed; session_api 47, passthrough 31 (merged tree, Linux) |
| HUD-10 | Team-server session spend at built-in list prices (AU14) | HUD-6, HUD-8, HUD-9 | DONE | judgment | 7e76153; acceptance in the full workspace run, 0 failed (merged tree) |
| UPD-1 | Auto-update: split check/install; start check at today's zygo claude / zygo opencode dispatch | HUD-3 | DONE | judgment | d74bd3f, merge c3f3c55; acceptance update 26/26, workspace --no-run ok (merged tree); Windows branch of #7 runs in CI only |
| HUD-11 | statusline verb: CLI, slash command, overrides, --save, help, skill text | HUD-7, HUD-9, UPD-1 | DONE | implementation | dd5c2dd; acceptance in the full workspace run, 0 failed (merged tree) |
| HUD-12 | zygo status and zygo usage in tokens and spend; doctor icon line | HUD-1, HUD-6, HUD-9 | DONE | implementation | 586fd49, merge 142d14f; acceptance usage_doctor 46, compact_cmd 16, zygo lib 159, statusline_render 24, statusline_data 36, store 29 (merged tree) |
| LIV-1 | Live check P-HUD-1 on Linux or macOS | HUD-9, HUD-11, PRB-2 | DONE | implementation | c60383a; live run 4 (Claude Code 2.1.330, tmux 3.7c): PH1 PH2 PH3 PH5 PH9 PH12 C7 verified, PH4 failed; acceptance exit 0 in the node checkout; forbidden_content, check-premises, live-selftest ok (merged tree) |
| LIV-2 | Windows part without a Windows machine: PH8/PH10 fallback, W-SHELL part 2 text, not-run records | LIV-1, PRB-1, FIX-5, FIX-7, P2 | DONE | implementation | 37c78eb; statusline_render 25, usage_doctor 46; PH8, PH10, W-SHELL part 2 not run: no Windows machine, fallback applied; review round 2 fixed 8 record defects |
| DOC-1 | Base-spec notes for the HUD (H§9) and auto-update | LIV-1, HUD-10, HUD-12, UPD-1, FIX-5 | DONE | implementation | fdcea4a; acceptance empty; review round 2 fixed 12 findings |
| P1 | Live premises PW6, PW11, PW15 (tmux, pane start, detached down) | PRB-2 | DONE | implementation | 93f71af; live on tmux 3.2a: PW6 failed (6.11, 6.32, 6.34, 6.36, 6.38), PW11 verified, PW15 failed (sandbox on); acceptance exit 0 in the node checkout; non-live checks ok (merged tree) |
| P2 | Live premises PW1-PW5, PW7, PW9, PW10, PW16 (agents) | P1 | DONE | implementation | 7657585; 9 LIVE lines: PW7 and PW16 failed, 7 verified; review round 2 fixed B1, B2, M1-M9; offline checks on merged tree |
| C1 | Core: workspace paths, session-dir accessors, session-view workspace field | HUD-7, HUD-9, HUD-10, HUD-11, HUD-12 | DONE | judgment | 4955b82; acceptance zygo-core lib 1, core_basics 17, hook_guard 5, workspace --no-run ok (merged tree) |
| X1 | Proxy session status: requests_in_flight, last_request_end (AU91) | HUD-5, HUD-6, HUD-10, HUD-12 | DONE | judgment | b3e7031; acceptance session_api 52, workspace --no-run ok (merged tree) |
| L3 | Stub agent behaviors for W§18 and stub helpers | — | DONE | implementation | 1bc76ba; acceptance stub_member 16, workspace --no-run ok (merged tree) |
| W1 | zygo-workspace crate scaffold and identity | HUD-8, HUD-10 | DONE | judgment | 5f2cdae; acceptance identity 9, workspace --no-run ok (merged tree) |
| W2 | Workspace state store: types, atomic write, locks, live check | W1, C1 | DONE | judgment | c0ea544; acceptance state 22, zygo-workspace all 51, workspace --no-run ok (merged tree) |
| W3 | Multiplexer trait, tmux implementation, tmux test gate | W1, P1, P2 | DONE | judgment | f945872; mux 42 on 3.7c and 3.2a; review round 2 fixed B1 (3.2a window names) and M1-M7 |
| W4 | Argument filter, bypass and user-info checks, environment cleaning | W1 | DONE | judgment | 3b98d88; acceptance args 10, env 4, workspace --no-run ok (merged tree) |
| W5 | Loadout config schema, team-layer drop, structural checks, resolution | W1, HUD-4, HUD-7, HUD-9 | DONE | judgment | f9a568a; acceptance zygo-config 214, loadout 6, workspace --no-run ok (merged tree) |
| W6 | Snapshot: list-panes read, shrink rule, key assignment, pending rule | W2, W3 | DONE | judgment | 8978541; acceptance snapshot 33, state 22, workspace --no-run ok (merged tree); review rounds 2-3 fixed B1, M1-M7, D1-D6 |
| W7 | zygo up for one workspace: plan, build, repair, attach | W2, W3, W4, W5, W6 | DONE | judgment | 3d51ad4; acceptance up 45, mux 46, snapshot 33, state 23, args 13, workspace --no-run ok (merged tree); review rounds 2-3 fixed B1, M1-M5, R3-1..R3-8 |
| W8 | up --all and workspace admin: ls, status, forget, forget-pane | W7 | DONE | judgment | 0921fb6; acceptance admin 21, up 45, up_all 7, workspace --no-run ok (merged tree); review round 2 fixed 1 blocking, 5 minor |
| W9 | activity.json under lock and the drain idle rule | W1, C1, X1, P2, HUD-5, W2 | DONE | judgment | e167624; activity 33 + 1 ignored, drain 27, workspace 111; review round 2 fixed B1 (newer-version files) and 5 minor |

## Log

| Date | Event |
|------|-------|
