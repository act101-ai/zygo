# Phase 3 Work Graph — History

Closed node rows and Log entries moved from graph.md, verbatim. Append-only.

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| DR-1 | Record format: parse and validate one DDR file, and `docs/decisions/` in place (README with a valid example) | — | DONE | judgment | 26be671 9612477 85e0c43 · decision_record 30/30 · PR #17 |
| DR-2 | Record set: directory reader, set checks and the dependency graph | DR-1 | DONE | implementation | 7e67ae4 d4007c2 168426f · decision_set 20/20 · PR #20 |
| DR-3 | `zygo decision` verb: `ls`, `show`, `deps`, `check` and the slash form | DR-2 | DONE | implementation | 683c800 46baa07 2334e49 f1ffb87 0b60e99 · decision_cli 17p/0f/6i, cli_surface 20p/0f/3i; floor green; decision check 0 · PR #22 |
| DR-4 | `zygo decision new`: write a record template | DR-3 | DONE | implementation | f0d049d beeaa1d 3965c61 · decision_new 12p/0f/1i; floor green · PR #25 |

## Log

| Date | Event |
|------|-------|
