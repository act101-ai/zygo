# Phase 2 Work Graph — History

Closed node rows and Log entries moved from graph.md, verbatim. Append-only.

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| PRB-2 | Premise docs: pins are minimums; live scratch dir removed on exit | — | DONE | implementation | 9d1f6c6; acceptance ver 50, lib 27, check-premises ok, live selftest 54 (merged tree) |
| HUD-1 | Usage store: Store::bytes_trimmed | — | DONE | implementation | 0f7ec77; acceptance zygo-usage 29/29, workspace --no-run ok (merged tree) |
| HUD-2 | zygo-work: bounded HUD git function (rev-parse, rev-list only) | — | DONE | judgment | 1454eab; acceptance hud_git 21/21 (+3 ignored), workspace --no-run ok (merged tree); CI green 3 OS (run 44964062674) |
| HUD-3 | Config tables [statusline] and [update] on today's schema; user config only | — | DONE | implementation | 77323a8, merge 71181de; acceptance zygo-config 176/176 (merged tree) |

## Log

| Date | Event |
|------|-------|
