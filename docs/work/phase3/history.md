# Phase 3 Work Graph — History

Closed node rows and Log entries moved from graph.md, verbatim. Append-only.

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| DR-1 | Record format: parse and validate one DDR file, and `docs/decisions/` in place (README with a valid example) | — | DONE | judgment | 26be671 9612477 85e0c43 · decision_record 30/30 · PR #17 |
| DR-2 | Record set: directory reader, set checks and the dependency graph | DR-1 | DONE | implementation | 7e67ae4 d4007c2 168426f · decision_set 20/20 · PR #20 |
| DR-3 | `zygo decision` verb: `ls`, `show`, `deps`, `check` and the slash form | DR-2 | DONE | implementation | 683c800 46baa07 2334e49 f1ffb87 0b60e99 · decision_cli 17p/0f/6i, cli_surface 20p/0f/3i; floor green; decision check 0 · PR #22 |
| DR-4 | `zygo decision new`: write a record template | DR-3 | DONE | implementation | f0d049d beeaa1d 3965c61 · decision_new 12p/0f/1i; floor green · PR #25 |
| DR-5 | Operator verbs `ratify` and `overturn`: byte-preserving outcome writes | DR-4 | DONE | judgment | 397edc5 aba92f9 d3e3ff7 0db609f 4f86964 2b072a7 · decision_verbs 25p/0f; floor green · PR #29 |
| DR-6 | `zygo decision review`: the phase-end review | DR-5 | DONE | implementation | 732af80 82d2e65 cb609a4 · decision_review 21/0; floor green · PR #34 |
| DR-8 | Session view `repo_root`, written by the launcher | DR-1 | DONE | mechanical | c806e21 88441c1 · repo_root 2/2 core, 3/3 launcher · PR #19 |
| DR-9 | The `dispositions` guard key, default `enforce` | DR-1 | DONE | mechanical | 34a088d fb104db · dispositions 4 tests · PR #18 |
| DR-10 | Agent transitions: the D§6.6 rule for one record change | DR-1 | DONE | judgment | 9efee7e 281dd03 · decision_transition 45/45 · PR #21 |
| DR-11 | Verdict git reader: four bounded git processes behind an injectable trait (PD1) | DR-10, MS-1 | DONE | judgment | 52a5006 b306476 5e3b797 196e814 0add171 · decision_git 33/33; floor green · PR #35 |
| DR-12 | Keep check: the D§9.4 steps over the node range | DR-11 | DONE | judgment | e0e426b 05c4148 43c44e7 7acba21 · decision_verdict 41/41; floor green · PR #37 |
| ML-1 | Mail config keys, the loadout pane `name` and the member-name grammar | DR-9, DR-8 | DONE | implementation | c3c7337 da263a1 · config_layers 90p/0f/16i, core_basics 24p/0f/3i; floor green · PR #23 |
| ML-2 | Maildir store: message file, atomic send, read by rename, limits, sweep | DR-1 | DONE | judgment | 2550471 e7fed8c 24c9124 · mail_store 38p/0f/0i; floor green · PR #24 |
| ML-16 | Live-check script for PM1-PM3 and acceptance #12 | DR-1 | DONE | implementation | a8add6a 3b33343 957db39 · live selftest ok (76); floor green · PR #26 |
| OS-1 | Result meta records the session that stored it | DR-1 | DONE | implementation | 7d285bd f733e6c 8a21ec2 · results 50p/0f, hooks_basic 55p/0f; floor green · PR #27 |
| OS-6 | Carry computation for one event over its context's requests | DR-1 | DONE | judgment | 32286a7 2ac0b35 · carry 19p/0f; floor green · PR #30 |
| OS-10 | `shape` module: step names, shaper names and byte bounds | DR-1 | DONE | mechanical | 9f2cd74 · shape tests 3 passed; floor green · PR #31 |
| OS-16 | Shaping fixture harness: byte-exact files, capture script, format and secret checks | DR-1 | DONE | implementation | a09ce9f ee1a671 3686f01 bca8cad def8f45 3b71a13 701d9e1 · shaping_fixtures 22 pass; floor green · PR #32 |
| MS-1 | Bounded git runner: whole and stream modes, public in zygo-work | DR-1 | DONE | judgment | e7d5023 821a65f 7017674 ab87c30 · git_runner 17/0, zygo-work ok; floor green · PR #33 |
| MS-3 | Mail sync config keys, forms and per-workspace resolution | ML-1, WD-5 | DONE | implementation | 96b5bf95 · acc: mail_sync_config 13 passed, zygo-config 258 passed 0 failed; floor green · PR #68 |
| WD-1 | Git read of `.zygo/` from the default branch and at a pinned commit | DR-1 | DONE | judgment | 35ffa73 e7c51d9 · repo_def_read 27/27; floor green · PR #36 |
| WD-5 | Config keys: pane `job`/`pool`, `[workspace] pool`, `[limits] max_members`, `.zygo.toml` filter, `config check` rules | DR-3, ML-1, RT-1 | DONE | implementation | 2d7c5e70 · definition_keys 11 passed, config_layers 90 passed · PR #58 |
| RT-1 | Member record `first_started_at`, set by each commit of a pending start | DR-1 | DONE | implementation | 7efa8d9 441e170 ff23c48 · first_started_at 11 tests, 3 crates pass; floor green · PR #38 |
| RT-2 | Run-time start rule, `<form>` text, and the `runtime:` line of `zygo status` | ML-1, WD-5 | DONE | implementation | 2f2c20a6 · acc: counts 2/1/7/5, 721 passed 0 failed; floor green · PR #67 |
| SX-1 | Claude Code subagent branch of `hook session-start`: mechanics text on `SubagentStart` for every non-`zygo-` subagent (T§6.3 item 2) | DR-1 | DONE | implementation | 6a979de 346fa1a · subagent_text 10 pass (9 subagent_start, 1 main_compact); floor green · PR #39 |
| SX-3 | `hook pre-compact` sets `reinject/<agent_id>` for a non-`zygo-` subagent (T§9.2 item 1) | DR-1 | DONE | implementation | 530b14d 08c2e37 b669a74 · reinject pre_compact 10 pass; floor green · PR #41 |
| SX-8 | `docs/live-checks.md` section P3-15 (T§12.2) | DR-1 | DONE | mechanical | 5bcdc27 · check-premises exit 0; floor green (ws 2592/0) · PR #44 |
| MS-2 | zygo-mail-sync crate: hardened git calls and test seams | MS-1, ML-2, DR-5 | DONE | implementation | e1dd701 · git_builder 13p; floor green · PR #48 |
| DR-7 | `zygo work report`: the two record lines | DR-2, DR-3, RT-2 | DONE | implementation | 81693f1d · decision_report 9/9; floor green · PR #77 |
| OS-2 | Usage store: `readbacks` table and three additive `trims` columns | OS-1 | DONE | judgment | 7a9afc1 · store 43p, usage_doctor 37p; floor green · PR #49 |
| OS-11 | Command parse: segments, file printers, stored name, runners, reuse text | OS-10 | DONE | judgment | 3ae61ee · shape_command 18 passed; floor green · PR #53 |
| OS-14 | Size view over any text, and the shaped marker | OS-1, OS-10 | DONE | implementation | b834d51 · results 54p/0f; floor green · PR #45 |
| OS-17 | Capture clean and dedup fixtures from real command output | OS-16 | DONE | implementation | 40f864d 2311dca d0cfe8a · acceptance 22p/0f, 5 cases; floor green (ws 2626p/0f) · PR #52 |
| OS-18 | Capture diff, json and log fixtures from real command output | OS-16 | DONE | implementation | 0c6335f1 · shaping_fixtures 22 passed; floor green · PR #55 |
| OS-19 | Capture libtest fixtures from a scratch crate | OS-16 | DONE | implementation | 821a1bb1 · shaping_fixtures 22 passed; floor green · PR #56 |
| DR-13 | `work_verdict`: `head`, `base`, `dispositions` input and the `dispositions` guard | DR-7, DR-8, DR-9, DR-12 | DONE | judgment | 5c96374f · acc 18/0 (1 ignored); floor green · PR #85 |
| OS-20 | Capture pytest fixtures from a scratch project | OS-16 | DONE | implementation | bb01285a · shaping_fixtures 22 passed; floor green · PR #57 |

## Log

| Date | Event |
|------|-------|
