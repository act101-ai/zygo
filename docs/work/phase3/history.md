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
| ML-17 | Live premises PM1-PM3 recorded (operator run) | ML-16 | DONE | mechanical | 339e7e04, d58c0c2e · acc check-premises: ok, LIVE PM1/PM2/PM3 verified ; floor green · PR #134 |
| OS-21 | Capture go test fixtures from a scratch module | OS-16 | DONE | implementation | d0ca20df 3c6b5aa9 22ca6760 · shaping_fixtures 22 passed ; floor green · PR #131 |
| OS-12 | Clean step: ANSI escapes and carriage-return progress | OS-11 | DONE | implementation | 6e1b9387 5ff2466e · shape_clean 23 passed ; floor green · PR #132 |
| WD-10 | Plan panes carry `job`, `pool` and the pin; `Source::Repository` | WD-6, ML-3, WD-8 | DONE | implementation | 8c8dd676, 7b189dbd, a08afc56 · loadout 15/0, state 43/0 ; floor green · PR #115 |
| DR-15 | Live check: PD2 and PD3 on the pinned Claude Code (operator, by hand) | SX-8 | DONE | mechanical | e02fc394, d8c6c053 · acc PD2/PD3 rows = 2, LIVE PD2 verified (513 chunks, raw bytes equal), LIVE PD3 verified ; floor green · PR #133 |
| SX-9 | Live check P3-15, run by hand by the operator: evidence for ST1-ST4 | SX-8 | DONE | mechanical | a300fa02, 875d61fe · acc 4/4 ST verified · PR #135 |
| OS-13 | Dedup step: runs of equal consecutive lines, lossless | OS-12 | DONE | implementation | PR #139; node 492ed380, c842cfab; acc 15/15 |
| OS-15 | Shaping pipeline: step order, reuse rule, byte bound | OS-13 | DONE | judgment | 0ea6c6c4, da13fbda, 38457d6f · acc 11/11 · PR #145 |
| OS-23 | Premise PO1: where a failed Bash call's output arrives, and what replaces it (operator, live Claude Code) | OS-22, ML-17 | DONE | mechanical | a2327414, a172b600, 48605b89 · acc 4/4 · PR #142 |
| ML-6 | Wake of an idle member after a send | ML-4, ML-5, ML-17 | DONE | judgment | 4cf4c8c1, f313648b, 845cb854 · acc 22/22 · PR #143 |
| ML-10 | Delivery by hooks: Stop block, prompt line, SessionStart line, OpenCode per-request line | ML-9, ML-17 | DONE | judgment | 5f0212b1, 3643f93d, 1efbd8a3, bc17d413, 9946594d, 9fe2d6ef, 9a57d777, 68c3fa62 · acc 14 mail_ tests, 26/26 · PR #144 |
| RT-3 | Run-time HUD segment in compact and verbose, with its place in the width-removal order | RT-2, ML-15 | DONE | implementation | fab29ce1, 72c3d03a, da38c41a · zygo-hooks 247/0 ; floor green · PR #118 |
| ML-11 | MCP tools `mail_send`, `mail_read`, `mail_list` | ML-6, ML-7 | DONE | implementation | ec5fda7c, 590b9c00 · acc 17/17, 42/42 · PR #146 |
| OS-29 | Kind views for diff, json and log (pipeline step 4) | OS-15, OS-18 | DONE | judgment | node commits 2438240c, 89da3536; acceptance: shape_kind_view 24/24, shape_pipeline 17/17; floor ws 3424/0 |
| MS-9 | Outbox: send-time rules, refusal and notice, counts, local retention | MS-6, MS-7, MS-8, ML-4 | DONE | judgment | e7987782, f30700e9, bec4c680 · outbox 29/0 ; floor green · PR #111 |
| ML-12 | CLI `zygo mail send\|read\|ls`, slash `/zygo:mail`, mail sweep at command start | ML-6, ML-7, DR-3 | DONE | implementation | d19e88f5, 0b457de5 · acc 12/12, 1/1, 234/234 · PR #149 |
| OS-24 | Premise PO2: an empty `stderr` in `updatedToolOutput` removes the stderr (operator, live Claude Code) | OS-23 | DONE | mechanical | node commits 9633f4b9, 3b88581f; acceptance: grep -c "^\| PO2 \|" docs/premises.md → 1, po2-result.json present; floor ws 3394/0 |
| ML-13 | `zygo mail watch` | ML-12 | DONE | implementation | plan cf7c0dae; 57bda37c, d70c468c · watch 12 tests, mail_cli 24/24 · PR #159 |
| OS-25 | Premise PO3: OpenCode `output.output` holds stdout and stderr (operator, live OpenCode) | OS-24 | DONE | mechanical | plan 4eaccaa2 · node 306d2bbf, 69416146 · acceptance `grep -c "^\| PO3 \|" docs/premises.md && test -s fixtures/...after-bash-stderr.json` → 1, exit 0 · PR #160 |
| MS-11 | Tip metadata, own hosts, collision and twin checks | MS-6, MS-10 | DONE | judgment | 53f75bbc 95931099 eaf0f3ee 71ba6b9c · identity 17/0 ; floor green · PR #113 |
| MS-12 | Import: candidates, checks, crash-safe delivery | MS-6, MS-10, ML-2, ML-4 | DONE | judgment | 68fa626f, dc8f3ad4, cf388671 · import 23/0 ; floor green · PR #114 |
| OS-26 | Premise PO4: the model sees a `result_view` reply plus at most a fixed wrapper (operator, both agents) | OS-25 | DONE | mechanical | plan a8ac4f72 · commits 3e4c8baa, 0b4b6865 · PR #170 |
| MS-16 | Sync child start and rate limit; member hooks start it | MS-3, MS-5, ML-10, DR-5, OS-4 | DONE | implementation | ea149eab, ad7d3736 · acc 18/18, 26/26 · PR #148 |
| OS-27 | Shape-off baseline: hook output and stored file of the `97d8767` behavior | OS-9, OS-17, OS-18, OS-19, OS-20, OS-21 | DONE | implementation | abea1612, 74976374 · acc 5/5 · PR #140 |
| OS-30 | Runner views: dispatch and the libtest view (pipeline step 3) | OS-29, OS-19 | DONE | judgment | 974fef24 plan; 6f4959d7, d1f02107, eef74481; shape_runner_libtest 20/20, shape_pipeline 24/24; PR #158 |
| WD-11 | `zygo up`: loadout choice, checked read, pin, member maximum | WD-7, WD-8, WD-10, ML-14, MS-5 | DONE | judgment | a1f4eb0c, f16c93f8, 767b954a, e0905dec, 8cd02ad7, 6fe57c61, e6fca6e4 · up_definition 17/0 ; floor green · PR #120 |
| WD-13 | Launcher: repository profile by a default-branch read (not pinned) | WD-7, DR-13, OS-9 | DONE | judgment | b00e2819, ffe10789, 777c51d6, d5c92329, 93add253 · repo_profile 15/0 ; floor green · PR #121 |
| WD-17 | `zygo doctor` lines: workspace files, pool, member maximum | WD-7, WD-9 | DONE | implementation | d31d6c56, 8a15ab61, 2ec94a9a, 3a944a2d · definition_doctor 15/0 ; floor green · PR #119 |
| SX-5 | Claude Code launch registers `SubagentStart` → `hook session-start` on every launch (T§6.3 item 1) | OS-33 | DONE | implementation | plan 25427cb5 · node d4c77b98, 99aa5725 · acceptance pass (claude_code 62 passed) · PR #181 |
| ML-18 | End-to-end mail on tmux with stub agents | ML-10, ML-11, ML-13, ML-14 | DONE | implementation | plan 8ca46aed · node 133f55bc, 37bd93c8 · acceptance pass (workspace_mail 7 passed, --include-ignored) · PR #171 |
| MS-13 | The sync: gate, mark, lock, passes, push retry, wake | MS-9, MS-11, MS-12, ML-6 | DONE | judgment | node commits 717f910e, ab5fc14b, 91b95bcb, dba2a089, 5dd04b76; acceptance: sync_pass 39/39; floor ws 3422/0; PR #150 |
| OS-28 | Trim hook runs the shaping pipeline on Bash output | OS-14, OS-15, OS-27 | DONE | judgment | node commits 81332899, b442555c, 095ee7bb, 63e6f26a, fe154009, 725e2609; acceptance: shaping_hook 21/21, shaping_off 5/5, hooks_basic 60/60, hook_trim 6/6; floor ws 3432/0; PR #154 |
| OS-41 | Kind views leave room for the shaped marker | OS-28, OS-29, OS-30 | DONE | implementation | 8f22ddd9, 453a297c, 68d99c41 · PR #167 |
| OS-31 | pytest runner view | OS-30, OS-20 | DONE | judgment | 86aaac5a, 1b22b0ca, f6759d10, 7544ce89 · acceptance 18 passed · PR #166 |
| WD-12 | `zygo up`: pool check, assignment and passthrough at a fresh build | WD-9, WD-11 | DONE | judgment | bae5b7af, 11d71492 · up_definition 35/0 ; floor green · PR #125 |
| ML-19 | Live acceptance #12: a member mails by name with no prompt about mail (operator run) | ML-18, SX-8, DR-15 | DONE | mechanical | plan 0cbac05c · node b16ce79f, f13fe09b · acceptance LIVE M12 verified (agent 2.1.332): 3 of 3 ; check-premises ok · PR #178 |
| MS-14 | Prune: orphan rewrite of acknowledged files, lease push | MS-13 | DONE | judgment | plan d2642ab4; node 51e0a828, ba8f8542, af8c6af8, f0cb91a7, f7a2bd2a; prune 18 passed; PR #179 |
| MS-20 | Session text: "Other machines" clause of item 8 | MS-6, MS-16, MS-13 | DONE | mechanical | plan fb76dfa8 · node 0febbe3b, 7e7d53a3, e63e4c6c, 8d5469a7 · acceptance pass (mail_sync_text 12 passed, session_text 43 passed) · PR #177 |
| OS-32 | go test runner view with interleaved parallel output | OS-31, OS-21 | DONE | judgment | plan 677eb448 · node eee53a92, e5e55c30 · acceptance pass (gotest 19 passed) · PR #182 |
| OS-33 | Failed Bash calls on Claude Code, as PO1 decided | OS-28, OS-23 | DONE | judgment | plan 911576e7 · node f632354c, 662bb328, 425bb080, 03d0f9c1 · acceptance pass (hooks 4 passed, launch 1 passed) · PR #168 |
| OS-42 | Runner dispatch under wrappers and unseen live output | OS-31, OS-32, FX-19 | DONE | judgment | 6d559322 · 30a091c8 · d286417c · acceptance pass (dispatch 9, libtest 20, pytest 18) · PR #196 |
| OS-34 | Set the stderr branch as PO2 decided | OS-33, OS-24 | DONE | mechanical | plan bba3b8b4 · node 9cd2b7fa, 399ed252, 7b268ebc · acceptance pass (po2_ 3 passed; shaping_hook 29 passed) · PR #184 |
| WD-18 | Session text: each member's job and the job sentence | WD-11, ML-9, ML-10, MS-20 | DONE | implementation | plan 97283bcc · node 445b4399, 875618e6, f4fcd2fd, dbff60d9 · acceptance pass (job_ 8 listed; session_text 51 passed) · PR #189 |
| OS-35 | Fixture views through the hook: goldens, floors, recovery, bless guard | OS-34, OS-32, OS-42 | DONE | judgment | 3a9dbffb · 3c10f93d · f788f46d · acceptance pass (shaping 14) · PR #198 |
| SX-4 | `hook trim` takes the flag and adds `additionalContext` = mechanics text, once (T§9.2 item 2) | SX-1, SX-3, OS-34 | DONE | judgment | plan 5441dd4c · node d37419f1, 927b1246, 86db8ba5 · acceptance pass (trim 17 listed; reinject trim 17 passed; zygo-hooks 344 passed) · PR #191 |
| OS-36 | The never-worse check over every case and the built inputs | OS-35 | DONE | judgment | 1fb09e17 · a4501591 · 65a63246 · acceptance pass (never_worse 22, shaping 36) · PR #201 |
| OS-44 | Read-back bytes count what the agent kept, not the reply sent | OS-26, OS-5 | DROPPED(the server cannot know what an agent keeps above its cut: OpenCode config limits and whole lines; Claude Code API token count; review of 90e88962) | judgment | b14a4225 · 6217a6ff · round 1 discarded (90e88962) · PR #207 |
| OS-43 | Cap every shaped view at 8 KiB, kind view or not | OS-41, SX-4, OS-46 | DROPPED(premise false: O§8.2 floors are per shaper, a clean view has no 8 KiB floor; a cut under the threshold breaks O-C9; review of 0fd61644) | judgment | 8883d724 · 65cdeb63 · round 1 discarded (0fd61644) · PR #195 |
| FX-1 | Process-group kill tests: anonymous pipe in place of a FIFO (Darwin FIFO lost wakeup) | — | DONE | implementation | 38ca329 · git_runner 17 + decision_git 34 pass; floor green · PR #40 |
| FX-2 | Decision writer lock: unlock before the close (a forked child keeps a closed flock) | — | DONE | implementation | 8a6191d c4584c2 · decisions::edit 4 pass; floor green · PR #42 |
| FX-3 | Test stdin helpers tolerate a child that exits before it reads stdin (statusline BrokenPipe flake) | — | DONE | implementation | 1636973 · compact_cmd 5p + statusline_cmd 4p; floor green · PR #46 |
| FX-4 | Proxy rows-bound test waits for the written row before the complete-flag GET (Windows flake) | — | DONE | mechanical | bbe99ca · serve 51p; floor green · PR #50 |
| FX-5 | Piped send test asserts send-then-read fails (Linux foreign-reader flake) | — | DONE | mechanical | 460b7cf · decision_git 35p, piped_epipe 1p; floor green · PR #51 |
| FX-6 | Dead-port test helpers: a port no parallel test can take (CI run 45557591675) | — | DONE | implementation | 690ea805 · zygo lib 38/compact_cmd 5/session_verbs 11/serve_cli 4/workspace_member 2/serve_client 40 passed · PR #65 |
| WD-24 | `loadout show` and `config show` pass pane `job` through `hide_secrets` (W§12) | WD-5 | DONE | mechanical | 9b719fa8 · cmd::loadout+config 32p; floor green · PR #66 |
| WD-23 | `config check`: `job` only on an agent pane (`claude-code`, `opencode`); R§9.1 text and change log | WD-5, MS-3 | DONE | implementation | 20cc6480 · definition_keys 14/14, config_layers 90/0 (16 ignored); floor green · PR #75 |
| MS-26 | `state::load` refuses a state.json value that `save` refuses (S§8.3) | MS-5 | DONE | implementation | 2c17b835 · acc 20/0; floor green · PR #83 |
| DR-16 | Unreadable `docs/decisions`: own error and line (`cannot read docs/decisions`), not "not a directory"; D§6.8 | DR-2, DR-3, DR-7 | DONE | implementation | c50a3dc9 · acc 5/0; floor green · PR #84 |
| FX-7 | Exec-failure relaunch test runs in a child process (environ swap breaks parallel PATH lookups) | — | DONE | implementation | 365436c6 · zygo lib ws/work/plugins 22p, down 8p; --include-ignored 54p; floor green · PR #97 |
| FX-8 | zygo usage reads summary and savings from one snapshot | OS-8 | DONE | implementation | 4636d2ee, 73cffb55 · report_snapshot 1/0, usage_doctor 45/0 ; floor green · PR #105 |
| FX-11 | zygo doctor icon line shows the ✉ mail glyph | ML-15, RT-3 | DONE | implementation | 1921d4ca, a7ac8924 · usage_doctor 45/0, statusline_render 37/0 ; floor green · PR #122 |
| FX-10 | HUD spec lists the member name and unread mail segments and the ✉ glyph | ML-15, RT-3 | DONE | mechanical | 8b19ed30, daf5faed · ✉ 4 ; floor green · PR #123 |
| FX-12 | HUD unread count uses the store's definition (regular <ULID>.json files; missing new/ is 0) | ML-15, ML-2, FX-11 | DONE | implementation | 233290fe, 9335d48e · statusline_mail 9/0 ; floor green · PR #124 |
| FX-15 | `zygo doctor` pool lines skip the members `zygo up` skips | WD-12 | DONE | implementation | 0b4ee021, f046b2b7 · definition_doctor 17/0 ; floor green · PR #129 |
| FX-14 | `zygo up` team read: CLI test of `with_team`, team-layer warnings only, R§9.4 sentence, `verbs::team` server tests, `up --all` exit code | WD-11, WD-12, ML-12 | DONE | implementation | 64d9685c, 3dc62442, 34467d50 · up_definition, workspace_restore, serve_cli, cmd::up all pass ; floor green · PR #162 |
| FX-17 | `zygo up` refuses a `[limits] max_members` outside 1-32 from the team or user layer (`config check` rule, `check_jobs` path) | FX-14, WD-11 | DONE | implementation | plan 7a124777 · node da3062c7, e437fde9 · acceptance pass (workspace_restore 7 passed, cmd::up 16 passed) · PR #169 |
| FX-18 | live-mail.sh fixture head line doubles the word zygo | ML-19 | DONE | mechanical | plan 02a12d2d · node fad4e150 · acceptance pass (sh scripts/premises/live-selftest.sh: live selftest: ok (77)) · PR #187 |
| FX-19 | PO2 row states what its run observed | OS-34 | DONE | mechanical | plan b2c8ca97 · node b5fa4c27, efbcccae · acceptance pass (check-premises ok; po2_ 3 passed) · PR #188 |
| MS-15 | Operator commands: take-host, new-identity, rename, drop-host | MS-14 | DONE | judgment | e8f19e67..5bc5249b · acceptance pass (operator 22 passed) · PR #190 |
| FX-20 | live-detach.sh PW15 record head line doubles the word zygo | — | DONE | mechanical | 96868305 · c98ea5fa · cc948a96 · acceptance pass (live selftest: ok (85)) · PR #194 |
| OS-37 | On-demand latency test, ignored in CI | OS-35 | DONE | implementation | 98d82be5 · b5c27cbd · 77c6726f · acceptance pass (shaping_latency 5 ignored) · PR #200 |
| OS-45 | Bound the usage-store wait in elapsed time | — | DONE | judgment | fb717aba · aecf74d2 · a0378951 · acceptance pass (busy_wait 7, zygo-usage, zygo-hooks, readbacks) · PR #203 |
| MS-17 | `zygo mail sync` verb, options, hidden background form | MS-15, MS-16, ML-12, ML-13 | DONE | implementation | 0167648d..826c59f6 · acceptance pass (mail_sync_cli 14 passed, cli_surface 21 passed) · PR #210 |
| OS-46 | Bound the hook's usage-store wait per Bash call | — | DONE | judgment | 75339a21 · d6be5653 · c727ff0e · 68f8e48c · acceptance pass (busy_wait_total/open_bounded_total 7, rollback_journal 1, zygo-usage, zygo-hooks, readbacks) · PR #205 |
| OS-47 | Runner view cost on large inputs, output unchanged | — | DONE | judgment | f1d69ae0 · d56988df · c37c66d4 · acceptance pass (runner_fast_path 5, zygo-results, shaping 36, shaping_fixtures 22, shaping_hook 29) · PR #206 |
| FX-16 | Clean removes charset designations (`ESC ( B`) and the escape after a lone `ESC` (`ESC ESC [31m`) | OS-12 | DONE | implementation | 19fbf580^..b0212f6c · acceptance pass (shape_clean 29 passed) · PR #209 |
| FX-21 | Clean removes the other designation forms: `ESC *`, `ESC +`, `ESC $` multi-byte, and intermediates before the final byte | FX-16 | DONE | implementation | 569e61a9^..9ac65902 · acceptance pass (shape_clean 35 passed) · PR #211 |
| MS-19 | MCP mail tools: addresses, send-time pass, remote mail_list | MS-17, ML-11, WD-18 | DONE | implementation | 37139e44^..e21d5e0b · acceptance pass (mail_remote 16 passed) · PR #212 |
| WD-14 | Pinned launches: resume and `ws run` resolve the profile from the pin | WD-12, WD-13, MS-17, FX-7 | DONE | judgment | 1324716e^..e1276ac2 · acceptance pass (workspace_definition 23 passed with tmux) · PR #214 |
| MS-18 | CLI send/read/ls/watch with addresses; escaped text; doctor twin line | MS-17, ML-13, MS-19, WD-17, FX-15 | DONE | implementation | 5e2be987^..1de15e3e · acceptance pass (mail_remote_cli 20 passed) · PR #215 |

## Log

| Date | Event |
|------|-------|
