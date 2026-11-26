# Phase 4 Work Graph — History

Closed node rows and Log entries moved from graph.md, verbatim. Append-only.

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| CF-1 | Conformance input model and check: `Agent::ALL`, registry and grade parsing with the input rules, evidence kinds, check items 1-9 on fixture inputs | — | DONE | judgment | PR #87 merged 74562e3b; acceptance 65 check_/input_ tests pass @ 3aceaac3; CI run 45605170068 green (ubuntu, macos, windows) |
| CF-2 | Conformance derived values: cell, tier and total score, bar, rollup, release state, on fixture inputs | CF-1 | DONE | implementation | PR #100 merged 3e98ae7c; acceptance 15 derive_ tests, conformance 84 pass @ 11c0dd70; CI run 45610449986 green (ubuntu, macos, windows) |
| CF-3 | Conformance outputs: feature matrix, rollup and score in Markdown and JSON, the write mode and check item 10, on fixture inputs | CF-2 | DONE | implementation | PR #102 merged 1c147219; acceptance 25 render_ tests, conformance 109 pass @ 80118dc2; CI run 45618690411 green (ubuntu, macos, windows) |
| CF-4 | Registry `capabilities.toml`, grade files for Claude Code and OpenCode, generated `docs/conformance.md` and `.json`, the tree test, CI paths, old scripts deleted (PR stops for the operator, K-D26) | CF-3, OC-1 | DONE | judgment | PR #104 merged 278f2cd4; acceptance exit 0 on merged tree (conformance 120 pass); CI run 45707423597 green (ubuntu, macos, windows) |
| OC-1 | OpenCode: session token out of the environment and config (apiKey from the token file), providers limited to zygo | — | DONE | judgment | PR #128 merged 53eabe25; acceptance exit 0 on merged tree; CI run 45693613400 green (ubuntu, macos, windows) |
| PC-1 | `scripts/check-premises.sh` driven by data, with `scripts/test-check-premises.sh` | — | DONE | judgment | PR #91 merged adac6bc0; acceptance exit 0 @ 54039f94 (test-check-premises 50 cases); CI run 45678686867 green (ubuntu, macos, windows) |
| PC-2 | Codex premise harness: every mock and scrub change of PC-3 to PC-6, codex-only `lib.sh` with an isolation record, `run-codex.sh`, help and schema captures | PC-1 | DONE | judgment | PR #126 merged 1206befa; acceptance exit 0 @ 4784e06b (real codex 0.172.0 run, ~/.codex unchanged); CI run 45698780636 green (ubuntu, macos, windows) |
| PC-3 | Codex proxy-facing captures: PC1, PC2, PC7, PC13, PC14 (requests, hosts, headers, models, injected items, context error) | PC-2, PC-10 | DONE | implementation | PR #137 merged 0d665799; acceptance exit 0 @ e554d2ca (real codex 0.172.0; PC1 PC2 PC7 PC13 PC14 verified); CI run 45725231426 green (ubuntu, macos, windows) |
| PC-4 | Codex config, trust and hook captures: PC3, PC4, PC5, PC12; the `--strict-config` run of a config with zygo's keys | PC-2, PC-10 | DONE | implementation | PR #155 merged d4ac5d2f; acceptance: real codex 0.172.0 run @ 685d55ef (PC3 PC12 verified; PC4 partial, replay only, Observed failed; PC5 failed on the replacement shape); CI run 45781607906 green on f16762a7 |
| PC-5 | Codex agents, compaction, MCP, status line and skills captures: PC6, PC8, PC9, PC11, PC15 | PC-2, PC-10 | DONE | implementation | PR #147 merged 08774771; acceptance exit 0 @ 6cbfc673 (real codex 0.172.0; PC6 PC8 PC9 PC11 verified, PC15 failed); CI run 45735709999 green (ubuntu, macos, windows) |

## Log

| Date | Event |
|------|-------|
