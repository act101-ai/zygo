# CC-1 — Client-branch inventory at the P4-6 base commit

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.9 (last paragraph), K§1 item 2 (the grep), K§6.1.2 (concerns 1-11), K§6.1.4 (the names)
**Files:** `docs/work/phase4/nodes/CC-1.md` (its `## Observed` section), and the **Files** lines of `docs/work/phase4/nodes/CC-3.md` to `CC-11.md`, `CC-13.md` and `CC-15.md` (only to add a file that an inventory row assigns to that node)
**Consumes:** `phase_3_in_phase_4` (CC-0)
**Produces:** `base` (the line `Base: <40-hex commit>` under `## Observed`: the CC-0 merge commit), `inventory` (the table under `## Observed`, one row per site: `| <file> | <line> | <concern 1-11 or —> | <CC-n or stays: reason> |`, with no backtick in a row)
**Acceptance:** `T=$(mktemp -d) && B=$(sed -n 's/^Base: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-1.md) && test -n "$B" && git fetch origin && git merge-base --is-ancestor b7e9d45d "$B" && git merge-base --is-ancestor "$B" origin/phase-4 && git grep -nE 'Agent::(ClaudeCode|OpenCode|\{|\*)|PaneKind::(ClaudeCode|OpenCode)|Plugins::|"(claude-code|claude|opencode|\.claude|\.opencode|CLAUDE_CONFIG_DIR|OPENCODE_CONFIG_DIR)"|(^|[^A-Za-z0-9_])(claude|opencode|claude_code|open_code)[a-z_]*: bool' "$B" -- 'crates/*/src/**' | sed "s/^$B://" | cut -d: -f1,2 | sort -u > "$T/grep" && grep -E '^\| crates/[^ ]+ \| [0-9]+ \| ([0-9]+|—) \| (CC-[0-9]+|stays: [^|]+) \|$' docs/work/phase4/nodes/CC-1.md | awk -F'|' '{gsub(/ /,"",$2); gsub(/ /,"",$3); print $2":"$3}' | sort -u > "$T/rows" && cmp "$T/grep" "$T/rows"` → every command exits 0: the recorded base holds phase 3 and is on `phase-4`; the set of `file:line` sites that the grep finds at the base equals the set of rows, each naming a CC node or `stays: <reason>`. The check reads only the recorded base, so it gives the same result after the node is DONE. After a Files line is extended, the work-graph validator (`work_graph.py check docs/work/phase4/graph.md`) exits 0

## Planning notes

- Capability `judgment` (coordinator ruling I4/M5, 2026-11-25): each row is a concern call, and the rows set the scope of CC-3 to CC-15.
- No product code changes. The writes are this file's `## Observed` section and, for a row whose node does not yet list its file, that node's Files line (operator brief for P4-5; ruling I4).
- The grep is the K§1 item 2 grep made exact (author; extended by ruling I4): a variant path of `Agent`, `PaneKind` or `Plugins`; a `use Agent::{…}` or `use Agent::*` import; a string literal whose whole value is a client id, program, CLI verb, config folder or config-folder variable; a boolean client flag such as `opencode: bool` (`crates/zygo-redact/src/harness.rs:45`). At `c21b32e0` it finds 209 sites. These counts show the pattern; they are not the work list.
- One row per site. A site in a `#[cfg(test)]` item is `stays: test` unless its node rewrites it (CC-13 sweeps the `Agent::` uses left in tests); a site in a doc comment is `stays: comment`.
- Node per concern (author): concerns 1 and 2, `Dialect` and the minimum version → CC-3; 9, the per-agent usage id and the harness-span flag → CC-4; 5 → CC-5; 3 and 4 → CC-6; 7 → CC-7; 8, workspace definition kinds and pool rule → CC-8; 6, 11 and the adapters → CC-9; 10 and the CLI sites → CC-10; a site in the descriptor or adapter folders → `stays: contract`; the no-attribution guard (`crates/zygo-work/src/guards.rs`, item `claude_attribution`) → `stays: exception list (CC-15, K§6.1.4)`; any other literal that equals a registered name but does not branch on a client → CC-15, which stops for the operator before it adds an exception (K§6.1.4).
- Under the table, list each phase 3 client feature for CC-12 (output shaping, mail wake and delivery, the run-time status line), each with its files and tests at the base.

## Observed

Base: b324e1dc7f00b38d410d577a307c05c80068b353

| File | Line | Concern | Node |
|---|---|---|---|
| crates/zygo-config/src/check.rs | 253 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 254 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/loadout.rs | 19 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/loadout.rs | 38 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/shape.rs | 123 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/shape.rs | 139 | 8 | CC-8 |
| crates/zygo-config/src/schema.rs | 470 | 9 | CC-4 |
| crates/zygo-config/src/schema.rs | 809 | 8 | CC-8 |
| crates/zygo-config/src/schema.rs | 879 | — | stays: comment |
| crates/zygo-config/src/schema.rs | 885 | — | stays: comment |
| crates/zygo-config/src/schema.rs | 888 | — | stays: comment |
| crates/zygo-core/src/hook.rs | 23 | 2 | CC-3 |
| crates/zygo-core/src/lib.rs | 38 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 40 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 49 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 50 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 56 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 57 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 81 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 82 | 1 | CC-3 |
| crates/zygo-core/src/session_view.rs | 61 | 1 | CC-3 |
| crates/zygo-hooks/src/prompt.rs | 173 | — | stays: test |
| crates/zygo-hooks/src/prompt.rs | 449 | — | stays: test |
| crates/zygo-hooks/src/session_start.rs | 79 | 3 | CC-6 |
| crates/zygo-hooks/src/session_text.rs | 83 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 325 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 335 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 749 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 768 | 3 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 786 | 3 | CC-6 |
| crates/zygo-launch/src/claude_code.rs | 70 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 90 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 95 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 101 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 573 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 578 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 587 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 619 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 630 | 6 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 701 | 7 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 592 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 593 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 606 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 608 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 616 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 809 | 7 | CC-7 |
| crates/zygo-launch/src/launcher.rs | 810 | 7 | CC-7 |
| crates/zygo-launch/src/opencode.rs | 78 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 172 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 286 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 430 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 433 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 465 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 470 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 496 | 11 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 512 | 7 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 572 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 591 | 1 | CC-9 |
| crates/zygo-mcp/src/tools/mod.rs | 22 | 5 | CC-5 |
| crates/zygo-mcp/src/tools/work.rs | 322 | — | stays: test |
| crates/zygo-plugins/src/snapshot.rs | 264 | 6 | CC-9 |
| crates/zygo-proxy/src/passthrough.rs | 35 | 9 | CC-4 |
| crates/zygo-proxy/src/window.rs | 132 | 9 | CC-4 |
| crates/zygo-redact/src/content.rs | 262 | 9 | CC-4 |
| crates/zygo-redact/src/harness.rs | 45 | 9 | CC-4 |
| crates/zygo-usage/src/savings.rs | 77 | 9 | CC-4 |
| crates/zygo-work/src/guards.rs | 1047 | — | stays: exception list (CC-15, K§6.1.4) |
| crates/zygo-work/src/guards.rs | 1059 | — | stays: exception list (CC-15, K§6.1.4) |
| crates/zygo-work/src/hooks.rs | 371 | 5 | CC-5 |
| crates/zygo-work/src/hooks.rs | 372 | 5 | CC-5 |
| crates/zygo-work/src/join.rs | 16 | 9 | CC-4 |
| crates/zygo-workspace/src/args.rs | 358 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 359 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 402 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 406 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 507 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 597 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 598 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 654 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 655 | 7 | CC-7 |
| crates/zygo-workspace/src/env.rs | 43 | 11 | CC-9 |
| crates/zygo-workspace/src/loadout.rs | 113 | 8 | CC-8 |
| crates/zygo-workspace/src/snapshot.rs | 263 | 8 | CC-8 |
| crates/zygo-workspace/src/snapshot.rs | 264 | 8 | CC-8 |
| crates/zygo-workspace/src/state.rs | 242 | 8 | CC-8 |
| crates/zygo-workspace/src/state.rs | 244 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 932 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 933 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 1014 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 1039 | 8 | CC-8 |
| crates/zygo/src/cmd/claude.rs | 34 | 1 | CC-10 |
| crates/zygo/src/cmd/claude.rs | 61 | 1 | CC-10 |
| crates/zygo/src/cmd/claude.rs | 68 | 1 | CC-10 |
| crates/zygo/src/cmd/claude.rs | 140 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 207 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 209 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 234 | — | stays: test |
| crates/zygo/src/cmd/compact.rs | 61 | 10 | CC-10 |
| crates/zygo/src/cmd/compact.rs | 173 | — | stays: test |
| crates/zygo/src/cmd/compact.rs | 269 | — | stays: test |
| crates/zygo/src/cmd/config.rs | 131 | 1 | CC-10 |
| crates/zygo/src/cmd/config.rs | 132 | 1 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 200 | 1 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 352 | 8 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 459 | 1 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 460 | 1 | CC-10 |
| crates/zygo/src/cmd/hook.rs | 106 | 1 | CC-10 |
| crates/zygo/src/cmd/loadout.rs | 330 | 8 | CC-10 |
| crates/zygo/src/cmd/loadout.rs | 590 | — | stays: test |
| crates/zygo/src/cmd/loadout.rs | 624 | — | stays: test |
| crates/zygo/src/cmd/loadout.rs | 648 | — | stays: test |
| crates/zygo/src/cmd/loadout.rs | 733 | — | stays: test |
| crates/zygo/src/cmd/loadout.rs | 851 | — | stays: test |
| crates/zygo/src/cmd/mail.rs | 893 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 95 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 107 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 108 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 236 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 573 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 581 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 582 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 645 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 664 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 691 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 692 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 693 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 759 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 760 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 1173 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1175 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1204 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1307 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1318 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1322 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1326 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1330 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1335 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1339 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1341 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1344 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1347 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1357 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1360 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1361 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1363 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1367 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1372 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1431 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1432 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1433 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1434 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1435 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1436 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1437 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1438 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1439 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1440 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1466 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1493 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1523 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1543 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1544 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1545 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1546 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1547 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1557 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1558 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1559 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1560 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1561 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1562 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1563 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1565 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1569 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1573 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1575 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1579 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1616 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1626 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1654 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1674 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1721 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1738 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1745 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1754 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1762 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1775 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1785 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1791 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1796 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1823 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1824 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1827 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1832 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1846 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1861 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1862 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1873 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1881 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1909 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1923 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1936 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1941 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1968 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2000 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2012 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2029 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2030 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2031 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2032 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2033 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 2039 | — | stays: test |
| crates/zygo/src/cmd/mod.rs | 705 | — | stays: test |
| crates/zygo/src/cmd/mod.rs | 707 | — | stays: test |
| crates/zygo/src/cmd/mod.rs | 709 | — | stays: test |
| crates/zygo/src/cmd/opencode.rs | 34 | 1 | CC-10 |
| crates/zygo/src/cmd/opencode.rs | 55 | 1 | CC-10 |
| crates/zygo/src/cmd/opencode.rs | 108 | — | stays: test |
| crates/zygo/src/cmd/opencode.rs | 115 | — | stays: test |
| crates/zygo/src/cmd/opencode.rs | 123 | — | stays: test |
| crates/zygo/src/cmd/verbs.rs | 451 | — | stays: test |
| crates/zygo/src/cmd/workspace_approve.rs | 180 | 8 | CC-8 |
| crates/zygo/src/cmd/ws.rs | 406 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 417 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 471 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 612 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 613 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 651 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 673 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 708 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 725 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 737 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 751 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 776 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 778 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 780 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 781 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 792 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 801 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 803 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 804 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 805 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 820 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 821 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 875 | — | stays: test |

Counts: 245 sites in 44 files (the acceptance grep at the base). By node: CC-3 10, CC-4 7, CC-5 3, CC-6 7, CC-7 11, CC-8 17, CC-9 28, CC-10 35, stays: comment 3, stays: exception list 2, stays: test 122. By concern (CC rows only): 1 36, 2 1, 3 3, 4 4, 5 3, 6 10, 7 20, 8 31, 9 7, 10 1, 11 2. No row goes to CC-15 as a new exception.

Test items at the base (each site in one of these ranges is stays: test): `crates/zygo-hooks/src/prompt.rs` 147-469; `crates/zygo-mcp/src/tools/work.rs` 263-328; `crates/zygo/src/cmd/claude.rs` 88-239; `cmd/compact.rs` 132-333; `cmd/loadout.rs` 480-1003; `cmd/mail.rs` 763-934 (`all(test, unix)`); `cmd/member.rs` 1117-2047; `cmd/mod.rs` 647-783; `cmd/opencode.rs` 59-128; `cmd/verbs.rs` 338-766; `cmd/ws.rs` 683-1034. The other test items in the 44 files hold no site.

Adapter rows (author's call at the node run): the rows of `crates/zygo-launch/src/claude_code.rs` and `opencode.rs` go to CC-9, which moves the two adapters whole into `crates/zygo-launch/src/client/`. Their concern is the one each line serves, not 6 for each as the Assignment table said at planning: the program name, the client id, `agent()` and the config folder names (`.claude`, `CLAUDE_CONFIG_DIR`, the `opencode` folders) are 1; the argument walk and `has_session_arg` are 7; `Plugins` is 6; the `OPENCODE_CONFIG_DIR` row that the adapter sets is 11.

Phase 3 client features (for CC-12), at the base:

- Output shaping: `crates/zygo-results/src/shape/` (`mod.rs`, `clean.rs`, `command.rs`, `dedup.rs`, `kind_view.rs`, `runner.rs`). Tests: `crates/zygo-results/tests/shape_*.rs` (9 files), `crates/zygo-hooks/tests/shaping.rs`, `shaping_hook.rs`, `shaping_off.rs`, `shaping_fixtures.rs`, `shaping_latency.rs`, and the golden folders `crates/zygo-hooks/tests/golden/shape_off/claude-code/` (5 files) and `.../opencode/` (1 file).
- Mail wake and delivery: `crates/zygo-workspace/src/mail/` (`mod.rs`, `names.rs`, `store.rs`, `wake.rs`), `crates/zygo-hooks/src/mail.rs`, the delivery sites `crates/zygo-hooks/src/workspace.rs:335` and `:749`-`:786`, `crates/zygo-mcp/src/tools/mail.rs`, `crates/zygo/src/cmd/mail.rs`, `crates/zygo-mail-sync/src/` (16 files). Tests: `crates/zygo-workspace/tests/mail_*.rs` (4 files), `crates/zygo-hooks/tests/mail_sync_text.rs`, `mail_sync_trigger.rs`, `crates/zygo-mcp/tests/mail_tools.rs`, `mail_remote.rs`, `crates/zygo/tests/mail_*.rs` (5 files), `workspace_mail.rs`, `crates/zygo-mail-sync/tests/` (15 entries).
- Run-time status line: `crates/zygo-hooks/src/statusline/` (`mod.rs`, `data.rs`, `mail.rs`, `phase.rs`, `render.rs`, `width.rs`), `crates/zygo/src/cmd/statusline.rs`, `status.rs`. Tests: `crates/zygo-hooks/tests/statusline_*.rs` (5 files), `crates/zygo/tests/statusline_cmd.rs`, `statusline_git.rs`.

## Test logic edits

None.

## Plan

**Base commit: `b324e1dc7f00b38d410d577a307c05c80068b353`** (planning, 2026-11-27). It is the CC-0 merge commit (PR #245; parents `33fd7bc5`, `060c4c2d`) that the contract names. Why this commit and not the head of `phase-4` at planning (`68ccb89e`): `git diff --stat b324e1dc HEAD` names only `docs/` files, so no `crates/` path has changed since the merge, and the contract fixes the base as the CC-0 merge. `origin/phase-3` is at `4ed521d7`, two commits past the pin `74748ae7`. `git diff --stat 74748ae7 origin/phase-3` names only `docs/` files, so the next phase-3 merge layer adds no client site. The merge base of `phase-4` and `origin/phase-3` is now `74748ae7`. Both `b7e9d45d` (the acceptance's phase 3 commit) and the pin `74748ae7` are ancestors of `b324e1dc`.

Interfaces:

- Consumes `phase_3_in_phase_4` (CC-0): phase 3 at `74748ae7` is in `phase-4` through `b324e1dc`.
- Produces `base`: the line `Base: b324e1dc7f00b38d410d577a307c05c80068b353` under `## Observed`.
- Produces `inventory`: the table under `## Observed` with the header `| File | Line | Concern | Node |` and one row per site in the form `| <file> | <line> | <concern 1-11 or —> | <CC-n or stays: reason> |`, with no backtick in a row.

Premise check (planning, at `b324e1dc`): the acceptance grep finds **245 sites in 44 files** (209 at `c21b32e0`). Phase 3 added `crates/zygo/src/cmd/member.rs` (98 sites, 84 of them in its `#[cfg(test)] mod tests` from line 1117 to the end of the file), `crates/zygo-config/src/repo_def/shape.rs` and `repo_def/loadout.rs`, `crates/zygo-workspace/src/env.rs`, `crates/zygo-hooks/src/prompt.rs`, `crates/zygo-mcp/src/tools/work.rs`, `crates/zygo/src/cmd/mail.rs`, `verbs.rs` and `workspace_approve.rs`. `crates/zygo-proxy/src/cache.rs` has no match at the base, because its per-client default is now in `crates/zygo-config/src/schema.rs:470`. The `#[cfg(test)]` forms at the base are `#[cfg(test)]`, `#[cfg(all(test, unix))]` and `#[cfg(all(test, windows))]`. No out-of-line `#[cfg(test)] mod x;` exists. `#[cfg(feature = "test-hooks")]` is production code and its sites are not `stays: test`.

Assignment (the planning notes' node-per-concern rule applied to each file at the base; the code at the base decides if a line differs):

| File | Lines | Concern | Node |
|---|---|---|---|
| `crates/zygo-core/src/lib.rs` | 38, 40, 49, 50, 56, 57, 81, 82 | 1 | CC-3 |
| `crates/zygo-core/src/hook.rs` | 23 | 2 | CC-3 |
| `crates/zygo-core/src/session_view.rs` | 61 (`Default`, becomes `Agent::DEFAULT`) | 1 | CC-3 |
| `crates/zygo-config/src/schema.rs` | 470 (`insert_markers` default) | 9 | CC-4 |
| `crates/zygo-config/src/schema.rs` | 809 (`LOADOUT_KINDS`) | 8 | CC-8 |
| `crates/zygo-config/src/schema.rs` | 879, 885, 888 (doc comments) | — | stays: comment |
| `crates/zygo-proxy/src/passthrough.rs` | 35 | 9 | CC-4 |
| `crates/zygo-proxy/src/window.rs` | 132 | 9 | CC-4 |
| `crates/zygo-redact/src/content.rs` | 262 | 9 | CC-4 |
| `crates/zygo-redact/src/harness.rs` | 45 (`opencode: bool`) | 9 | CC-4 |
| `crates/zygo-usage/src/savings.rs` | 77 | 9 | CC-4 |
| `crates/zygo-work/src/join.rs` | 16 | 9 | CC-4 |
| `crates/zygo-work/src/hooks.rs` | 371, 372 | 5 | CC-5 |
| `crates/zygo-mcp/src/tools/mod.rs` | 22 | 5 | CC-5 |
| `crates/zygo-hooks/src/session_start.rs` | 79 | 3 | CC-6 |
| `crates/zygo-hooks/src/session_text.rs` | 83 | 4 | CC-6 |
| `crates/zygo-hooks/src/workspace.rs` | 325 (prompt blocked while stopping), 335 (delivery line per prompt), 749 (per-request session start) | 4 | CC-6 |
| `crates/zygo-hooks/src/workspace.rs` | 768, 786 (session-start reply shape) | 3 | CC-6 |
| `crates/zygo-workspace/src/args.rs` | 358, 359, 402, 406, 507, 597, 598, 654, 655 | 7 | CC-7 |
| `crates/zygo-launch/src/launcher.rs` | 809, 810 (insertion point of session arguments) | 7 | CC-7 |
| `crates/zygo-config/src/check.rs` | 253, 254 | 8 | CC-8 |
| `crates/zygo-config/src/repo_def/shape.rs` | 123, 139 (kinds, pool rule) | 8 | CC-8 |
| `crates/zygo-config/src/repo_def/loadout.rs` | 19, 38 (default member kind) | 8 | CC-8 |
| `crates/zygo-workspace/src/loadout.rs` | 113 (`builtin_solo`) | 8 | CC-8 |
| `crates/zygo-workspace/src/state.rs` | 242, 244 (`PaneKind` serde names) | 8 | CC-8 |
| `crates/zygo-workspace/src/snapshot.rs` | 263, 264 | 8 | CC-8 |
| `crates/zygo-workspace/src/up.rs` | 932, 933, 1014 (`claude_code: bool` of `pool_member`), 1039 | 8 | CC-8 |
| `crates/zygo/src/cmd/workspace_approve.rs` | 180 (calls `up::pool_member`, which CC-8 changes) | 8 | CC-8 |
| `crates/zygo-launch/src/claude_code.rs` | 70, 573, 578, 587, 619 (program, id, config folder) | 1 | CC-9 |
| `crates/zygo-launch/src/claude_code.rs` | 90, 95, 101, 701 (argument walk) | 7 | CC-9 |
| `crates/zygo-launch/src/claude_code.rs` | 630 (`Plugins`) | 6 | CC-9 |
| `crates/zygo-launch/src/opencode.rs` | 172, 430, 433, 465, 572, 591 (program, id, config folder) | 1 | CC-9 |
| `crates/zygo-launch/src/opencode.rs` | 78, 286, 470 (`Plugins`) | 6 | CC-9 |
| `crates/zygo-launch/src/opencode.rs` | 496 (`OPENCODE_CONFIG_DIR` set row) | 11 | CC-9 |
| `crates/zygo-launch/src/opencode.rs` | 512 (argument walk) | 7 | CC-9 |
| `crates/zygo-launch/src/launcher.rs` | 592, 593, 606, 608, 616 (plugin delivery, `Plugins`) | 6 | CC-9 |
| `crates/zygo-plugins/src/snapshot.rs` | 264 | 6 | CC-9 |
| `crates/zygo-workspace/src/env.rs` | 43 (`OPENCODE_CONFIG_DIR` row) | 11 | CC-9 |
| `crates/zygo/src/cmd/claude.rs` | 34, 61, 68 | 1 | CC-10 |
| `crates/zygo/src/cmd/opencode.rs` | 34, 55 | 1 | CC-10 |
| `crates/zygo/src/cmd/compact.rs` | 61 | 10 | CC-10 |
| `crates/zygo/src/cmd/config.rs` | 131, 132 | 1 | CC-10 |
| `crates/zygo/src/cmd/doctor.rs` | 200, 459, 460 | 1 | CC-10 |
| `crates/zygo/src/cmd/doctor.rs` | 352 (pool rule, inline) | 8 | CC-10 |
| `crates/zygo/src/cmd/hook.rs` | 106 (`Agent::DEFAULT`) | 1 | CC-10 |
| `crates/zygo/src/cmd/loadout.rs` | 330 (`Agent::DEFAULT`) | 8 | CC-10 |
| `crates/zygo/src/cmd/member.rs` | 95, 107, 108, 236, 573, 581, 582, 645, 664, 691, 692, 693, 759, 760 | 1, 7 or 8 by the line | CC-10 |
| `crates/zygo/src/cmd/ws.rs` | 406, 417, 471, 612, 613, 651, 673 | 8 | CC-10 |
| `crates/zygo-work/src/guards.rs` | 1047, 1059 | — | stays: exception list (CC-15, K§6.1.4) |
| each site inside a `#[cfg(test)]` or `#[cfg(all(test, …))]` item: `prompt.rs` 173, 449; `tools/work.rs` 322; `cmd/claude.rs` 140, 207, 209, 234; `cmd/opencode.rs` 108, 115, 123; `cmd/compact.rs` 173, 269; `cmd/loadout.rs` 590, 624, 648, 733, 851; `cmd/mail.rs` 893; `cmd/mod.rs` 705, 707, 709; `cmd/verbs.rs` 451; `cmd/member.rs` from 1117 (84 sites); `cmd/ws.rs` from 683 (16 sites) | | — | stays: test |

Expected counts: CC-3 10, CC-4 7, CC-5 3, CC-6 7, CC-7 11, CC-8 17, CC-9 28, CC-10 35, stays: comment 3, stays: exception list 2, stays: test 122; total 245. No row goes to CC-15 as a new exception (every non-branching literal at the base is either the guard or a test or comment site).

Plan fix at the node run (2026-11-27): the planning table gave concern 6 to each adapter line of `claude_code.rs` and `opencode.rs`. At the base only 5 of those 21 lines touch `Plugins`; the others hold the program name, the client id, the config folder names, the argument walk or a set environment row. Each row now names the concern its line serves (see `## Observed`). The node of each row (CC-9) and every node count are unchanged.

Steps:

- [x] 1. Check the base. Run `git merge-base --is-ancestor b7e9d45d b324e1dc7f00b38d410d577a307c05c80068b353 && git merge-base --is-ancestor 74748ae7 b324e1dc7f00b38d410d577a307c05c80068b353 && git merge-base --is-ancestor b324e1dc7f00b38d410d577a307c05c80068b353 origin/phase-4` → exit 0. Run `git diff --stat b324e1dc HEAD -- crates` → no output. If a later merge layer on `phase-4` changed `crates/` before this node starts, the base stays `b324e1dc` (the contract), and each site that the grep finds at the new head and not at the base is a found issue (§5.4): write it into K§6.9 and add a node for it.
- [x] 2. In `docs/work/phase4/nodes/CC-1.md`, replace the line `Written when the node runs.` under `## Observed` with `Base: b324e1dc7f00b38d410d577a307c05c80068b353`, a blank line, then the table header `| File | Line | Concern | Node |` and `|---|---|---|---|`.
- [x] 3. Run the acceptance grep alone with the base, its output to a file in a temporary directory outside the checkout: `T=$(mktemp -d) && git grep -nE 'Agent::(ClaudeCode|OpenCode|\{|\*)|PaneKind::(ClaudeCode|OpenCode)|Plugins::|"(claude-code|claude|opencode|\.claude|\.opencode|CLAUDE_CONFIG_DIR|OPENCODE_CONFIG_DIR)"|(^|[^A-Za-z0-9_])(claude|opencode|claude_code|open_code)[a-z_]*: bool' b324e1dc7f00b38d410d577a307c05c80068b353 -- 'crates/*/src/**' > "$T/sites"` (the pattern of **Acceptance**, unchanged). Check: `wc -l < "$T/sites"` prints `245`.
- [x] 4. Find the test items. For each of the 44 files, read it at the base (`git show b324e1dc:<file>`) and take the line range of each item whose `cfg` predicate requires `test` (`test`, or `all(…)` with `test` in it). Check: the ranges agree with the last row of the Assignment table; a site inside a range is `stays: test`, with concern `—`.
- [x] 5. Write one row per site, sorted by file and then line, as the Assignment table says. A site on a `///` or `//!` line is `stays: comment`. Where the code at a line differs from its Assignment note, the code decides, by the planning notes' node-per-concern rule. Check: each `crates/zygo/src/cmd/member.rs` row before line 1117 names concern 1 (program name and CLI verb: 581, 582, 691, 692, 693), 7 (session flags and the bare rule: 95, 236, 759, 760) or 8 (resume command and the config folder saved for resume: 107, 108, 573, 645, 664).
- [x] 6. Add the files that rows give to a node whose **Files** line does not list them: append `` `crates/zygo-config/src/repo_def/loadout.rs` `` and `` `crates/zygo/src/cmd/workspace_approve.rs` `` to the **Files** line of `docs/work/phase4/nodes/CC-8.md` (CC-8 changes `up::pool_member`, which `workspace_approve.rs:180` calls). Every other file in a CC-n row is already on that node's **Files** line (checked at planning). Check: `python3 /home/kagemartin/.cache/zygo/sessions/01M8CKYAD9C5W51MRT0M1T8299/plugins/act101/skills/work-graph/scripts/work_graph.py check docs/work/phase4/graph.md` (the act101 plugin's `skills/work-graph/scripts/work_graph.py`) → exit 0.
- [x] 7. Under the table, write `Phase 3 client features (for CC-12), at the base:` and one bullet for each feature, with its files and tests at the base:
  - output shaping: `crates/zygo-results/src/shape/` (`mod.rs`, `clean.rs`, `command.rs`, `dedup.rs`, `kind_view.rs`, `runner.rs`); tests `crates/zygo-results/tests/shape_*.rs` (9 files), `crates/zygo-hooks/tests/shaping.rs`, `shaping_hook.rs`, `shaping_off.rs`, `shaping_fixtures.rs`, `shaping_latency.rs`, the golden folders `crates/zygo-hooks/tests/golden/shape_off/claude-code/` and `.../opencode/`;
  - mail wake and delivery: `crates/zygo-workspace/src/mail/` (`mod.rs`, `names.rs`, `store.rs`, `wake.rs`), `crates/zygo-hooks/src/mail.rs`, the delivery sites `crates/zygo-hooks/src/workspace.rs:335` and `:749`-`:786`, `crates/zygo-mcp/src/tools/mail.rs`, `crates/zygo/src/cmd/mail.rs`, `crates/zygo-mail-sync/src/`; tests `crates/zygo-workspace/tests/mail_*.rs` (4 files), `crates/zygo-hooks/tests/mail_sync_text.rs`, `mail_sync_trigger.rs`, `crates/zygo-mcp/tests/mail_tools.rs`, `mail_remote.rs`, `crates/zygo/tests/mail_*.rs` (5 files), `workspace_mail.rs`, `crates/zygo-mail-sync/tests/`;
  - run-time status line: `crates/zygo-hooks/src/statusline/` (`mod.rs`, `data.rs`, `mail.rs`, `phase.rs`, `render.rs`, `width.rs`), `crates/zygo/src/cmd/statusline.rs`, `status.rs`; tests `crates/zygo-hooks/tests/statusline_*.rs` (5 files), `crates/zygo/tests/statusline_cmd.rs`, `statusline_git.rs`.
  Check: `git ls-tree -r --name-only b324e1dc -- <each path>` lists each named path.
- [x] 8. Run the **Acceptance** command alone, as one command → exit 0 (`cmp` silent).
- [x] 9. Commit `docs/work/phase4/nodes/CC-1.md` and `CC-8.md` on the node branch: `docs(work): CC-1 inventory at b324e1dc`, with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:CC-1`.
