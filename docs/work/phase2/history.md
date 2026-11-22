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

## Log

| Date | Event |
|------|-------|
