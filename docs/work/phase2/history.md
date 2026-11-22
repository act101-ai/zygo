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

## Log

| Date | Event |
|------|-------|
