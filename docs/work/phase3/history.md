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
| DR-14 | End-to-end gate: the stop path, the merged-node keep and no stored text | DR-5, DR-13 | DONE | implementation | 31bc0dac · acc 4+4/0; floor ws 2977/1 (pre-existing zygo lib flake, also on phase-3) · PR #90 |
| OS-22 | Premise script for PO1 to PO4 | OS-16 | DONE | implementation | 5e4cd273 · bash -n ok, lib.sh 2 · PR #60 |
| WD-2 | Typed shapes, allow-list validation and digest of the workspace files; PR3 | WD-1 | DONE | judgment | adb3069b · cargo test -p zygo-config --test repo_def_shape 12 passed; floor green · PR #72 |
| SX-2 | OpenCode `session-start --event subagent`: mechanics text as plain text, no member part (T§7.2 item 2) | SX-1 | DONE | implementation | 9c8f2fb · subagent_text 4p, cli_surface 21p; floor green · PR #47 |
| ML-3 | Member name in the plan, the member record and `panes[]` | ML-1, RT-1 | DONE | implementation | 863b6b05 · loadout 10/state 41/snapshot 30 passed · PR #62 |
| MS-7 | Secret pass on one message | MS-2 | DONE | implementation | abe0ce4b · secret 21 passed · PR #61 |
| OS-3 | Bounded usage-store open for the hook and the MCP server | OS-2 | DONE | judgment | c403338f · store 50 passed (7 open_bounded) · PR #64 |
| OS-7 | Savings read: net figures, per-context carry, breakdown and ratio | OS-2, OS-6 | DONE | judgment | 6bb2ad8b · savings 17/17; floor green · PR #71 |
| OS-40 | Command parse: more wrappers and file printers, eval, `$(<f)`, bare shell stage | OS-11 | DONE | implementation | 5ba7eb02 · shape_command 43 passed · PR #59 |
| WD-3 | Workspace approvals in `approvals.toml` | WD-2 | DONE | judgment | aad86d39 · acc 9+2/0; floor green · PR #88 |
| SX-6 | OpenCode plugin transform: `--event subagent` for non-main `zygo/` models (T§7.2 item 1) | SX-2 | DONE | implementation | c3a465e9 · opencode 43p + 8 ignored node tests pass; floor green · PR #54 |
| ML-4 | Address resolution and the send operation | ML-2, ML-3 | DONE | implementation | 0f925e1c · acc 18/0; floor green ws 2967/0 · PR #89 |
| ML-5 | Multiplexer operations for the wake: literal keys and Enter | ML-3 | DONE | implementation | 867df914 · acc admin 34/mux 58/snapshot 41 · PR #94 |
| ML-7 | Member name in the session view marker, hand start `--name` | ML-3, DR-3, DR-8, FX-6, FX-7 | DONE | implementation | dc966a4c 24e7af32 1d672c48 894a0435 · core_basics 26/0, zygo --lib 226/0, workspace_member --include-ignored 53/0 ; floor green · PR #103 |
| MS-5 | Sync state files: paths, identity, state.json, imported log, marks | MS-2, WD-5 | DONE | judgment | 4aecace4 · state 18/18; floor green · PR #69 |
| OS-4 | Trim hook writes `result_id` and `shaper`, opens the store bounded | OS-3 | DONE | implementation | 6c8e57d1 · acc hooks_basic 60 (5 trim_row)/hook_trim 6 · PR #95 |
| OS-5 | MCP result tools record one read-back row per successful reply | OS-3 | DONE | implementation | aa64beea · acc readbacks 8/zygo-mcp 72/mcp_stdio 4 · PR #96 |
| OS-8 | `zygo usage` prints the net figures, breakdown and ratio | OS-7 | DONE | implementation | a7e1fa29 1773594e e906160f · usage_doctor 45/0, savings 8; floor green · PR #101 |
| WD-4 | Checked use: one read, one digest, one store lookup per use; `Pin` | WD-3 | DONE | judgment | af19550d e91ef159 6971ba06 · repo_def_check 9/0; floor green · PR #98 |
| WD-9 | Pool directory listing, assignment and shared lines | WD-5, ML-2 | DONE | judgment | 0d97709b · pool 16/16; floor green · PR #70 |
| ML-8 | `zygo ws name <key> <name>` | ML-5, ML-7 | DONE | judgment | 2fc1f821, 64486e85, 0bfa1c6d · admin 16/0 (7 rename_member), zygo lib 228/0 ; floor green · PR #107 |
| ML-9 | Session text item 8 (workspace mail) at main-loop SessionStart | ML-4, ML-7, DR-8, RT-1 | DONE | implementation | ed8193ef, 4bed6315 · session_text 43/0 (13 mail_item) ; floor green · PR #106 |
| ML-15 | HUD segments: member name and unread mail | ML-7, RT-2 | DONE | implementation | 0128c7fa, 63b94dd8 · statusline_mail 7/0, statusline_render 30/0 ; floor green · PR #108 |
| MS-4 | Address grammar and the ULID file form | MS-2, MS-3 | DONE | implementation | a0dd0435 · address 13/13; floor green · PR #76 |
| MS-8 | Public check: URL map, anonymous probe, verdict lists, cache | MS-5 | DONE | judgment | 8d5f951f · public 36/36; floor green · PR #73 |
| MS-10 | Side branch plumbing: index-only worktree, fetch, bootstrap, commit, push | MS-5 | DONE | judgment | 2384d84a · branch 36/36; floor green · PR #74 |
| WD-6 | The repository loadout from `[members]`, with the default set | WD-4, WD-5 | DONE | implementation | 3e0d705e, 6613f8d9 · repo_def_loadout 12/0 ; floor green · PR #109 |
| WD-8 | Member record fields `launch.job` and `launch.definition` | WD-4, ML-3 | DONE | mechanical | ef98f88f, 5a2d5117 · state 43/0, launch_definition 2 ; floor green · PR #110 |
| ML-14 | Unread counts in `zygo workspace`, `zygo ls` and `up`; `forget-pane` removes the maildir | ML-2, ML-8 | DONE | judgment | d27a16f9, e898f769, af9ba2c7, e8ef03ca, 65cc6726, 79dd9dd3 · admin 49/0, up 72/0 ; floor green · PR #116 |
| MS-6 | Registry and ack files, validation, member cache | MS-4, MS-5 | DONE | judgment | 39e9eb0e 64419fc6 bad0bc1c · registry 27/0; floor green · PR #99 |
| OS-9 | `[results] shape` in config and the session view | OS-4, ML-9, MS-3 | DONE | mechanical | 80d820d0, 04a9d343 · config_layers 93/0, core_basics 27/0, hooks_basic 60/0, launcher 14/0 ; floor green · PR #117 |
| WD-7 | Approved definition added to the merged config with source `workspace-files` | WD-6, MS-3 | DONE | judgment | f5a7e8cd f97e8031 bce0413f · repo_def_apply 18/0, config_layers 90/0 (16 ignored) ; floor green · PR #112 |
| MS-24 | Live premise SP2: anonymous probe on github.com and gitlab.com | MS-8 | DONE | lookup | 6a93103b · SP2 verified (git 2.50.1) ; floor n/a (docs only) · PR #130 |
| OS-12 | Clean step: ANSI escapes and carriage-return progress | OS-11 | DONE | implementation | 6e1b9387 5ff2466e · shape_clean 23 passed ; floor green · PR #132 |

## Log

| Date | Event |
|------|-------|
