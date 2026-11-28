# CC-1 — Client-branch inventory at the P4-6 base commit

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.9 (last paragraph), K§1 item 2 (the grep), K§6.1.2 (concerns 1-11), K§6.1.4 (the names)
**Files:** `docs/work/phase4/nodes/CC-1.md` (its `## Observed` section; in review round 1 also its Acceptance, Produces and Plan text and its `## Round 1 review` section), the **Files** lines of `docs/work/phase4/nodes/CC-3.md` to `CC-11.md`, `CC-13.md` and `CC-15.md` (only to add a file that an inventory row assigns to that node), the contract lines of `docs/work/phase4/nodes/CC-7.md` (kept signatures, round 1 F8) and `docs/work/phase4/nodes/CC-13.md` (Files, Produces and Acceptance, round 1 F4/F5)
**Consumes:** `phase_3_in_phase_4` (CC-0)
**Produces:** `base` (the line `Base: <40-hex commit>` under `## Observed`: the CC-0 merge commit), `inventory` (the table under `## Observed`, one row per site that the search of **Acceptance** finds at the base: `| <file> | <line> | <concern> | <node> |`, where the concern is 1-11 and the node one of CC-3 to CC-15, or the concern is `—` and the node `stays: test`, `stays: comment` or `stays: exception list (CC-15, K§6.1.4)`; no backtick in a row)
**Acceptance:** `T=$(mktemp -d) && F=docs/work/phase4/nodes/CC-1.md && B=$(sed -n 's/^Base: \([0-9a-f]\{40\}\)$/\1/p' "$F") && test -n "$B" && git fetch origin && git merge-base --is-ancestor b7e9d45d "$B" && git merge-base --is-ancestor "$B" origin/phase-4 && git grep -nE -e 'Agent::(ClaudeCode|OpenCode|\{|\*)' -e 'PaneKind::(ClaudeCode|OpenCode)' -e 'Plugins::' -e '"(claude-code|claude|opencode|\.claude|\.opencode|CLAUDE_CONFIG_DIR|OPENCODE_CONFIG_DIR|Claude Code|OpenCode)"' -e '(^|[^A-Za-z0-9_])(claude|opencode|claude_code|open_code)[a-z_]*: bool' -e '(^|[^A-Za-z0-9_])!(claude|opencode|claude_code)([^A-Za-z0-9_]|$)' -e '(^|[^A-Za-z0-9_])(CLAUDE[A-Z_]*|OPENCODE_[A-Z_]+|CODEX_[A-Z_]+)([^A-Za-z0-9_]|$)' -e '(^|[^A-Za-z0-9_])(LOADOUT_KINDS|AGENT_ONLY|CLAUDE_ONLY)([^A-Za-z0-9_]|$)' -e '[xX]-[cC]laude-[cC]ode-' -e 'claude-code' -e '^\[(claude-code|opencode)\]' -e 'agents\.toml' -e '(^|[^A-Za-z0-9_])(PreToolUse|PostToolUse|PostToolUseFailure|PreCompact|SessionStart|SessionEnd|SubagentStart|SubagentStop|UserPromptSubmit)([^A-Za-z0-9_]|$)' -e '"(Stop|StopFailure|Notification)"' -e 'mcp__' -e 'pub fn compact_tool\(|is_some_and\(compact_tool\)|window::compact_tool' -e '(^|[^A-Za-z0-9_])(ClaudeCode|OpenCode)([^A-Za-z0-9_]|$)' -e '(^|[^A-Za-z0-9_])(Claude|Opencode)\(' -e '(^|[^A-Za-z0-9_])(claude|opencode|claude_code)::|mod (claude|opencode|claude_code);' "$B" -- 'crates/*/src/**' 'crates/*/tests/**' crates/zygo-config/data/agents.toml crates/zygo-launch/assets/opencode-plugin.js | sed "s/^$B://" | cut -d: -f1,2 | sort > "$T/grep" && test -s "$T/grep" && grep '^| crates/' "$F" > "$T/all" && grep -E '^\| crates/[A-Za-z0-9_./-]+ \| [1-9][0-9]* \| (([1-9]|1[01]) \| CC-([3-9]|1[0-5])|— \| stays: (test|comment|exception list \(CC-15, K§6\.1\.4\))) \|$' "$T/all" > "$T/ok" && cmp "$T/all" "$T/ok" && awk -F'|' '{gsub(/ /,"",$2); gsub(/ /,"",$3); print $2":"$3}' "$T/ok" | sort > "$T/rows" && test -s "$T/rows" && test -z "$(uniq -d "$T/rows")" && cmp "$T/grep" "$T/rows"` → every command exits 0: the recorded base holds phase 3 and is on `phase-4`; the search finds at least one site; every line of this file that starts with `| crates/` is a well-formed row (concern 1-11 with a node CC-3 to CC-15, or `—` with a `stays:` node of the three); no `file:line` has two rows; and the `file:line` sites that the search finds at the base equal the rows. The check reads only the recorded base, so it gives the same result after the node is DONE. Completeness is bounded by the search patterns (listed under `## Observed`). After a node file is edited, the work-graph validator (`work_graph.py check docs/work/phase4/graph.md`) exits 0

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
| crates/zygo-config/data/agents.toml | 2 | 1 | CC-3 |
| crates/zygo-config/data/agents.toml | 5 | 1 | CC-3 |
| crates/zygo-config/src/builtin.rs | 49 | — | stays: comment |
| crates/zygo-config/src/builtin.rs | 51 | 1 | CC-3 |
| crates/zygo-config/src/check.rs | 8 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 219 | — | stays: comment |
| crates/zygo-config/src/check.rs | 222 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 223 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 253 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 254 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 255 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 256 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 259 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 260 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 262 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 263 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 267 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 278 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 279 | 8 | CC-8 |
| crates/zygo-config/src/check.rs | 291 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/loadout.rs | 19 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/loadout.rs | 35 | — | stays: comment |
| crates/zygo-config/src/repo_def/loadout.rs | 38 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/shape.rs | 123 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/shape.rs | 139 | 8 | CC-8 |
| crates/zygo-config/src/repo_def/shape.rs | 140 | 8 | CC-8 |
| crates/zygo-config/src/schema.rs | 470 | 9 | CC-4 |
| crates/zygo-config/src/schema.rs | 801 | — | stays: comment |
| crates/zygo-config/src/schema.rs | 809 | 8 | CC-8 |
| crates/zygo-config/src/schema.rs | 867 | — | stays: comment |
| crates/zygo-config/src/schema.rs | 879 | — | stays: comment |
| crates/zygo-config/src/schema.rs | 885 | — | stays: comment |
| crates/zygo-config/src/schema.rs | 888 | — | stays: comment |
| crates/zygo-config/tests/builtin.rs | 229 | 1 | CC-13 |
| crates/zygo-config/tests/builtin.rs | 230 | 1 | CC-13 |
| crates/zygo-config/tests/builtin.rs | 231 | 1 | CC-13 |
| crates/zygo-config/tests/config_layers.rs | 111 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1571 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1590 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1613 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1675 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1679 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1684 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1685 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1728 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1730 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1735 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1739 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1745 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1827 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1828 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1830 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1832 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1834 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1838 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1856 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1857 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1863 | — | stays: test |
| crates/zygo-config/tests/config_layers.rs | 1865 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 11 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 12 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 76 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 89 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 97 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 107 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 108 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 119 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 121 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 131 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 132 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 136 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 142 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 149 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 150 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 153 | — | stays: test |
| crates/zygo-config/tests/definition_keys.rs | 156 | — | stays: test |
| crates/zygo-config/tests/repo_def_apply.rs | 91 | — | stays: test |
| crates/zygo-config/tests/repo_def_loadout.rs | 34 | — | stays: test |
| crates/zygo-config/tests/repo_def_loadout.rs | 110 | — | stays: test |
| crates/zygo-config/tests/repo_def_loadout.rs | 118 | — | stays: test |
| crates/zygo-config/tests/repo_def_loadout.rs | 180 | — | stays: test |
| crates/zygo-config/tests/repo_def_shape.rs | 40 | — | stays: test |
| crates/zygo-config/tests/repo_def_shape.rs | 54 | — | stays: test |
| crates/zygo-config/tests/repo_def_shape.rs | 81 | — | stays: test |
| crates/zygo-config/tests/repo_def_shape.rs | 190 | — | stays: test |
| crates/zygo-config/tests/repo_def_shape.rs | 202 | — | stays: test |
| crates/zygo-config/tests/repo_def_shape.rs | 212 | — | stays: test |
| crates/zygo-core/src/hook.rs | 23 | 2 | CC-3 |
| crates/zygo-core/src/hook_opencode.rs | 3 | — | stays: comment |
| crates/zygo-core/src/hook_opencode.rs | 29 | — | stays: comment |
| crates/zygo-core/src/hook_opencode.rs | 39 | 2 | CC-3 |
| crates/zygo-core/src/hook_opencode.rs | 43 | 2 | CC-3 |
| crates/zygo-core/src/lib.rs | 38 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 39 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 40 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 41 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 49 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 50 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 56 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 57 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 81 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 82 | 1 | CC-3 |
| crates/zygo-core/src/lib.rs | 83 | 1 | CC-3 |
| crates/zygo-core/src/session_view.rs | 61 | 1 | CC-3 |
| crates/zygo-core/tests/conformance.rs | 823 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 824 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 1862 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 1888 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 1890 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 1895 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 1896 | — | stays: test |
| crates/zygo-core/tests/conformance.rs | 1938 | — | stays: comment |
| crates/zygo-core/tests/conformance.rs | 1943 | — | stays: test |
| crates/zygo-core/tests/conformance/fixtures/repo/crates/zygo-config/data/agents.toml | 1 | — | stays: test |
| crates/zygo-core/tests/conformance/fixtures/repo/crates/zygo-config/data/agents.toml | 4 | — | stays: test |
| crates/zygo-core/tests/conformance/model.rs | 1228 | 1 | CC-13 |
| crates/zygo-core/tests/conformance/model.rs | 1229 | 1 | CC-13 |
| crates/zygo-core/tests/conformance/model.rs | 1281 | — | stays: comment |
| crates/zygo-core/tests/conformance/model.rs | 1283 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 92 | 1 | CC-13 |
| crates/zygo-core/tests/core_basics.rs | 153 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 185 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 202 | 1 | CC-13 |
| crates/zygo-core/tests/core_basics.rs | 208 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 309 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 418 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 586 | — | stays: test |
| crates/zygo-core/tests/core_basics.rs | 645 | — | stays: test |
| crates/zygo-core/tests/hook_guard.rs | 6 | 1 | CC-13 |
| crates/zygo-core/tests/hook_guard.rs | 23 | 1 | CC-13 |
| crates/zygo-core/tests/hook_guard.rs | 24 | — | stays: test |
| crates/zygo-core/tests/hook_guard.rs | 33 | 1 | CC-13 |
| crates/zygo-hooks/src/mail.rs | 113 | — | stays: comment |
| crates/zygo-hooks/src/prompt.rs | 1 | — | stays: comment |
| crates/zygo-hooks/src/prompt.rs | 24 | — | stays: comment |
| crates/zygo-hooks/src/prompt.rs | 143 | 3 | CC-6 |
| crates/zygo-hooks/src/prompt.rs | 165 | — | stays: test |
| crates/zygo-hooks/src/prompt.rs | 173 | 1 | CC-13 |
| crates/zygo-hooks/src/prompt.rs | 180 | — | stays: test |
| crates/zygo-hooks/src/prompt.rs | 447 | — | stays: test |
| crates/zygo-hooks/src/prompt.rs | 449 | 1 | CC-13 |
| crates/zygo-hooks/src/role_guard.rs | 29 | 3 | CC-6 |
| crates/zygo-hooks/src/session_start.rs | 38 | — | stays: comment |
| crates/zygo-hooks/src/session_start.rs | 39 | — | stays: comment |
| crates/zygo-hooks/src/session_start.rs | 42 | 2 | CC-6 |
| crates/zygo-hooks/src/session_start.rs | 46 | 3 | CC-6 |
| crates/zygo-hooks/src/session_start.rs | 50 | — | stays: comment |
| crates/zygo-hooks/src/session_start.rs | 79 | 3 | CC-6 |
| crates/zygo-hooks/src/session_start.rs | 80 | 3 | CC-6 |
| crates/zygo-hooks/src/session_start.rs | 87 | — | stays: comment |
| crates/zygo-hooks/src/session_start.rs | 94 | — | stays: comment |
| crates/zygo-hooks/src/session_start.rs | 96 | — | stays: comment |
| crates/zygo-hooks/src/session_text.rs | 83 | 4 | CC-6 |
| crates/zygo-hooks/src/statusline/width.rs | 12 | — | stays: comment |
| crates/zygo-hooks/src/trim.rs | 88 | — | stays: comment |
| crates/zygo-hooks/src/trim.rs | 90 | 2 | CC-6 |
| crates/zygo-hooks/src/trim.rs | 106 | — | stays: comment |
| crates/zygo-hooks/src/trim.rs | 240 | — | stays: comment |
| crates/zygo-hooks/src/trim.rs | 244 | 2 | CC-6 |
| crates/zygo-hooks/src/trim.rs | 272 | 3 | CC-6 |
| crates/zygo-hooks/src/trim.rs | 328 | 3 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 7 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 221 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 324 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 325 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 333 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 335 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 340 | 3 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 351 | 2 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 364 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 460 | 3 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 703 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 725 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 733 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 748 | — | stays: comment |
| crates/zygo-hooks/src/workspace.rs | 749 | 4 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 768 | 3 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 770 | 3 | CC-6 |
| crates/zygo-hooks/src/workspace.rs | 786 | 3 | CC-6 |
| crates/zygo-hooks/tests/common/shaping.rs | 81 | — | stays: comment |
| crates/zygo-hooks/tests/common/shaping.rs | 88 | — | stays: test |
| crates/zygo-hooks/tests/common/shaping.rs | 99 | — | stays: test |
| crates/zygo-hooks/tests/common/shaping.rs | 100 | — | stays: test |
| crates/zygo-hooks/tests/common/shaping.rs | 104 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/diff/git-diff-repo/hook_stdout | 1 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/diff/git-diff-repo/stored | 298 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/diff/git-diff-repo/stored | 305 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/diff/git-diff-repo/stored | 1423 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/diff/git-diff-repo/stored | 1424 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/json/cargo-metadata/hook_stdout | 1 | — | stays: test |
| crates/zygo-hooks/tests/golden/shape_off/shaping/log/errors-in-middle/hook_stdout | 1 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 18 | 1 | CC-13 |
| crates/zygo-hooks/tests/hooks_basic.rs | 40 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 44 | 1 | CC-13 |
| crates/zygo-hooks/tests/hooks_basic.rs | 45 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 50 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 57 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 61 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 65 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 68 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 72 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 77 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 101 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 104 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 225 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 234 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 242 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 253 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 260 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 268 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 269 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 270 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 277 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 278 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 285 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 286 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 287 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 288 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 294 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 322 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 323 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 343 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 387 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 489 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 506 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 507 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 516 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 524 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 525 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 530 | — | stays: test |
| crates/zygo-hooks/tests/hooks_basic.rs | 538 | — | stays: test |
| crates/zygo-hooks/tests/mail_sync_text.rs | 70 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 77 | — | stays: comment |
| crates/zygo-hooks/tests/mail_sync_text.rs | 193 | — | stays: comment |
| crates/zygo-hooks/tests/mail_sync_text.rs | 194 | — | stays: comment |
| crates/zygo-hooks/tests/mail_sync_text.rs | 201 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 211 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 220 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 233 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 236 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 238 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 245 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 252 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 259 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_text.rs | 266 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 206 | — | stays: test |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 215 | — | stays: comment |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 243 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 261 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 277 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 284 | — | stays: test |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 286 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 302 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 311 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 322 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 332 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 342 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 352 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 370 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 371 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 372 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 373 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 393 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 394 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 395 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 396 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 426 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 441 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 442 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 468 | 1 | CC-13 |
| crates/zygo-hooks/tests/mail_sync_trigger.rs | 469 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 29 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 39 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 57 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 58 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 79 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 86 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 94 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 137 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 138 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 161 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 162 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 163 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 164 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 178 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 193 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 203 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 217 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 242 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 267 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 279 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 282 | — | stays: comment |
| crates/zygo-hooks/tests/reinject.rs | 284 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 290 | — | stays: comment |
| crates/zygo-hooks/tests/reinject.rs | 294 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 319 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 320 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 326 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 332 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 338 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 344 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 355 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 362 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 384 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 385 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 397 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 435 | — | stays: comment |
| crates/zygo-hooks/tests/reinject.rs | 436 | — | stays: comment |
| crates/zygo-hooks/tests/reinject.rs | 445 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 447 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 453 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 454 | 1 | CC-13 |
| crates/zygo-hooks/tests/reinject.rs | 491 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 500 | — | stays: test |
| crates/zygo-hooks/tests/reinject.rs | 517 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 46 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 99 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 100 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 139 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 294 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 404 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 416 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 420 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 428 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 430 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 438 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 439 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 440 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 444 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 489 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 553 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 554 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 632 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 644 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 646 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 678 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 679 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 694 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 701 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 709 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 710 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 713 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 714 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 721 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 723 | — | stays: test |
| crates/zygo-hooks/tests/session_text.rs | 724 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 732 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 746 | — | stays: comment |
| crates/zygo-hooks/tests/session_text.rs | 747 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 755 | 1 | CC-13 |
| crates/zygo-hooks/tests/session_text.rs | 759 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping.rs | 35 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 39 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 70 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 72 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 101 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 170 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 172 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping.rs | 238 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping.rs | 258 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 269 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 281 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 283 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 287 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 288 | — | stays: comment |
| crates/zygo-hooks/tests/shaping.rs | 293 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 371 | — | stays: test |
| crates/zygo-hooks/tests/shaping.rs | 700 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping.rs | 804 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 22 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 52 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 53 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 56 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 61 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 62 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 68 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 72 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 73 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 81 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 86 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 87 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 150 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 158 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 159 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 230 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 231 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 244 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 245 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 246 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 249 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 401 | — | stays: test |
| crates/zygo-hooks/tests/shaping_fixtures.rs | 402 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 35 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 51 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_hook.rs | 57 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 83 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 98 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 129 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 158 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 161 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 164 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 165 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 175 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 177 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 183 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_hook.rs | 186 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 205 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 206 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 209 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 233 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 248 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 252 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 269 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 277 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 280 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 298 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 315 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 326 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_hook.rs | 338 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 357 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 365 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 383 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 387 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 397 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 409 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 412 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 420 | — | stays: test |
| crates/zygo-hooks/tests/shaping_hook.rs | 422 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 435 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 441 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 443 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 479 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 495 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 510 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 520 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_hook.rs | 525 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 537 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 549 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 571 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 574 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 591 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 612 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_hook.rs | 634 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_latency.rs | 67 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_latency.rs | 70 | — | stays: test |
| crates/zygo-hooks/tests/shaping_latency.rs | 117 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_latency.rs | 120 | — | stays: test |
| crates/zygo-hooks/tests/shaping_latency.rs | 121 | — | stays: test |
| crates/zygo-hooks/tests/shaping_latency.rs | 132 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_latency.rs | 273 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_off.rs | 142 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_off.rs | 147 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 152 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 158 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 159 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_off.rs | 161 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_off.rs | 176 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 181 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_off.rs | 191 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_off.rs | 197 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_off.rs | 199 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 215 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 218 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_off.rs | 219 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 236 | 1 | CC-13 |
| crates/zygo-hooks/tests/shaping_off.rs | 241 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_off.rs | 247 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 261 | — | stays: comment |
| crates/zygo-hooks/tests/shaping_off.rs | 265 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 271 | — | stays: test |
| crates/zygo-hooks/tests/shaping_off.rs | 273 | — | stays: test |
| crates/zygo-hooks/tests/statusline_team.rs | 287 | — | stays: test |
| crates/zygo-hooks/tests/statusline_team.rs | 345 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 39 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 54 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 61 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 101 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 114 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 115 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 124 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 136 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 149 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 157 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 171 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 199 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 204 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 218 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 220 | — | stays: comment |
| crates/zygo-hooks/tests/subagent_text.rs | 226 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 233 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 304 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 306 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 326 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 336 | — | stays: test |
| crates/zygo-hooks/tests/subagent_text.rs | 348 | — | stays: comment |
| crates/zygo-hooks/tests/subagent_text.rs | 355 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 373 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 387 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 399 | 1 | CC-13 |
| crates/zygo-hooks/tests/subagent_text.rs | 409 | — | stays: comment |
| crates/zygo-hooks/tests/subagent_text.rs | 411 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 54 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 128 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 151 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 155 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 202 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 204 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 244 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 461 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 463 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 464 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 465 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 466 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 467 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 476 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 479 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 480 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 483 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 484 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 486 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 491 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 492 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 494 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 496 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 504 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 526 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 529 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 532 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 546 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 547 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 549 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 553 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 568 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 581 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 608 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 609 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 611 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 612 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 617 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 621 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 622 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 623 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 682 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 697 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 708 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 718 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 737 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 753 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 754 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 755 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 767 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 768 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 769 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 793 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 826 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 827 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 837 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 870 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 871 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 878 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 879 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 900 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 906 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 940 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 945 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 967 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 978 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 979 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 981 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1024 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1032 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 1033 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 1038 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1039 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1042 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1056 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1070 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1083 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1121 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1127 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1137 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 1138 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1154 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1169 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1183 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1191 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1197 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1231 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1245 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1254 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1258 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1269 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1306 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1341 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1348 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1349 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1351 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1361 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1365 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1369 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1381 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1408 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1415 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1421 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1436 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1445 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1454 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1462 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1468 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1475 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1483 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1487 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1488 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1494 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1497 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1498 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1504 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1511 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1514 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1515 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1522 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1526 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1527 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1543 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1549 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1550 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1563 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1566 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1583 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1585 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1592 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1594 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1595 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1603 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1610 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 1612 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1613 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1615 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1616 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1618 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1626 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1643 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1647 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1648 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1656 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 1657 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1661 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1671 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1672 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1675 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1676 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1678 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1680 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1682 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1691 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1706 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1707 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1726 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1750 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1793 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1798 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1804 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1812 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1830 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1847 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1890 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1898 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1931 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 1935 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1943 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 1980 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2012 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2025 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2036 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 2041 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2053 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2060 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 2063 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2068 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2096 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2130 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2131 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2184 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2200 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 2201 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2223 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 2236 | — | stays: test |
| crates/zygo-hooks/tests/workspace.rs | 2248 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 2249 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2258 | — | stays: comment |
| crates/zygo-hooks/tests/workspace.rs | 2259 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2269 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2286 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2288 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2312 | 1 | CC-13 |
| crates/zygo-hooks/tests/workspace.rs | 2323 | 1 | CC-13 |
| crates/zygo-launch/assets/opencode-plugin.js | 23 | 1 | CC-9 |
| crates/zygo-launch/src/adapter.rs | 72 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 19 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 20 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 34 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 36 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 38 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 40 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 41 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 48 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 50 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 53 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 57 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 61 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 63 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 64 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 67 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 69 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 70 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 74 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 76 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 90 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 95 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 101 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 145 | 9 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 158 | 4 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 358 | 9 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 442 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 483 | 5 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 484 | 5 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 495 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 496 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 497 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 501 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 504 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 506 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 507 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 508 | — | stays: comment |
| crates/zygo-launch/src/claude_code.rs | 510 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 520 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 526 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 527 | 2 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 573 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 578 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 585 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 587 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 619 | 1 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 630 | 6 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 652 | 7 | CC-9 |
| crates/zygo-launch/src/claude_code.rs | 701 | 7 | CC-9 |
| crates/zygo-launch/src/env.rs | 24 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 25 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 26 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 27 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 29 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 30 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 31 | 11 | CC-9 |
| crates/zygo-launch/src/env.rs | 32 | 11 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 117 | — | stays: comment |
| crates/zygo-launch/src/launcher.rs | 592 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 593 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 606 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 608 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 616 | 6 | CC-9 |
| crates/zygo-launch/src/launcher.rs | 727 | — | stays: comment |
| crates/zygo-launch/src/launcher.rs | 804 | — | stays: comment |
| crates/zygo-launch/src/launcher.rs | 809 | 7 | CC-7 |
| crates/zygo-launch/src/launcher.rs | 810 | 7 | CC-7 |
| crates/zygo-launch/src/lib.rs | 2 | 1 | CC-9 |
| crates/zygo-launch/src/lib.rs | 5 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 1 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 17 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 26 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 30 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 31 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 32 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 33 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 37 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 39 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 78 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 108 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 109 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 112 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 172 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 202 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 262 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 281 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 286 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 423 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 428 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 430 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 433 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 436 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 465 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 470 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 471 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 476 | 6 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 496 | 11 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 512 | 7 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 514 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 521 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 527 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 570 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 572 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 575 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 577 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 579 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 583 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 591 | 1 | CC-9 |
| crates/zygo-launch/src/opencode.rs | 600 | — | stays: comment |
| crates/zygo-launch/src/opencode.rs | 606 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 17 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 90 | 1 | CC-13 |
| crates/zygo-launch/tests/claude_code.rs | 119 | 1 | CC-13 |
| crates/zygo-launch/tests/claude_code.rs | 120 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 142 | 6 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 190 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 191 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 204 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 255 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 336 | 6 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 430 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 447 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 448 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 493 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 497 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 498 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 511 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 534 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 539 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 553 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 556 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 559 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 584 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 838 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1186 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1189 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1190 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1213 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1219 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1225 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1230 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1326 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 1327 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 1339 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 1342 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1353 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1363 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1368 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1370 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 1373 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1376 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1382 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1385 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 1399 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1409 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1423 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1436 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1465 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1476 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1504 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1507 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1512 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1521 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1522 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1547 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1586 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 1604 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1605 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1608 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1617 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1619 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1647 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1670 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1681 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 1684 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1697 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1704 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1705 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1708 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1719 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1720 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1721 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1722 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1728 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1729 | — | stays: comment |
| crates/zygo-launch/tests/claude_code.rs | 1730 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1731 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1732 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1733 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1734 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1735 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1768 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1781 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1821 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 1822 | 1 | CC-13 |
| crates/zygo-launch/tests/claude_code.rs | 1835 | — | stays: test |
| crates/zygo-launch/tests/claude_code.rs | 1845 | 1 | CC-13 |
| crates/zygo-launch/tests/claude_code.rs | 1881 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 1892 | 1 | CC-13 |
| crates/zygo-launch/tests/claude_code.rs | 1921 | 1 | CC-9 |
| crates/zygo-launch/tests/claude_code.rs | 1922 | 1 | CC-9 |
| crates/zygo-launch/tests/fixtures/claude_code/env-passthrough.json | 2 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/env.json | 3 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/env.json | 4 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/mcp.json | 1 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 15 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 35 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 44 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 53 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 73 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 84 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 94 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 104 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 114 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings-passthrough.json | 124 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 13 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 16 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 17 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 18 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 21 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 22 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 23 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 24 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 25 | — | stays: test |
| crates/zygo-launch/tests/fixtures/claude_code/settings.json | 26 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 16 | 1 | CC-9 |
| crates/zygo-launch/tests/launcher.rs | 98 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 99 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 100 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 101 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 105 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 108 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 144 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 145 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 148 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 149 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 220 | 1 | CC-13 |
| crates/zygo-launch/tests/launcher.rs | 251 | 6 | CC-9 |
| crates/zygo-launch/tests/launcher.rs | 252 | 6 | CC-9 |
| crates/zygo-launch/tests/launcher.rs | 517 | — | stays: test |
| crates/zygo-launch/tests/launcher.rs | 674 | 1 | CC-13 |
| crates/zygo-launch/tests/launcher.rs | 703 | — | stays: comment |
| crates/zygo-launch/tests/launcher.rs | 724 | 1 | CC-9 |
| crates/zygo-launch/tests/launcher.rs | 818 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 1 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 18 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 30 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 43 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 73 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 80 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 96 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 104 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 140 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 146 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 161 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 162 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 226 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 227 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 240 | 6 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 254 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 255 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 259 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 327 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 349 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 359 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 541 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 545 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 562 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 574 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 610 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 645 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 657 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 704 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 771 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 772 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 773 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1068 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1093 | 6 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 1105 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1128 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1159 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1213 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1243 | 6 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 1290 | 6 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 1301 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1305 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 1315 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 1327 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1371 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1441 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 1452 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1458 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1497 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 1513 | 1 | CC-9 |
| crates/zygo-launch/tests/opencode.rs | 1515 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 1609 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1610 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1613 | — | stays: test |
| crates/zygo-launch/tests/opencode.rs | 1867 | 1 | CC-13 |
| crates/zygo-launch/tests/opencode.rs | 1995 | — | stays: comment |
| crates/zygo-launch/tests/opencode.rs | 2791 | 1 | CC-9 |
| crates/zygo-launch/tests/premise.rs | 52 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 55 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 64 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 65 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 67 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 111 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 121 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 124 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 141 | — | stays: comment |
| crates/zygo-launch/tests/premise.rs | 163 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 165 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 176 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 178 | — | stays: test |
| crates/zygo-launch/tests/premise.rs | 211 | — | stays: test |
| crates/zygo-launch/tests/repo_profile.rs | 147 | 1 | CC-13 |
| crates/zygo-launch/tests/serve_client.rs | 20 | 1 | CC-9 |
| crates/zygo-launch/tests/serve_client.rs | 243 | 1 | CC-13 |
| crates/zygo-launch/tests/serve_client.rs | 793 | 1 | CC-13 |
| crates/zygo-launch/tests/serve_client.rs | 1241 | 1 | CC-9 |
| crates/zygo-launch/tests/serve_client.rs | 1249 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/harness.rs | 15 | 1 | CC-9 |
| crates/zygo-launch/tests/stub_agent/harness.rs | 18 | 1 | CC-9 |
| crates/zygo-launch/tests/stub_agent/harness.rs | 21 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/harness.rs | 22 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/harness.rs | 23 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/harness.rs | 69 | 1 | CC-9 |
| crates/zygo-launch/tests/stub_agent/harness.rs | 70 | 1 | CC-9 |
| crates/zygo-launch/tests/stub_agent/harness.rs | 73 | 1 | CC-9 |
| crates/zygo-launch/tests/stub_agent/harness.rs | 74 | 1 | CC-9 |
| crates/zygo-launch/tests/stub_agent/harness.rs | 237 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/harness.rs | 238 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 7 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 12 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 13 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 63 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 101 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 102 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 110 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 121 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 123 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 124 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 125 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 127 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 130 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 131 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 132 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 144 | — | stays: comment |
| crates/zygo-launch/tests/stub_agent/stub.rs | 151 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 233 | — | stays: test |
| crates/zygo-launch/tests/stub_agent/stub.rs | 323 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 16 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 17 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 30 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 31 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 43 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 54 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 55 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 66 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 97 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 99 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 105 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 122 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 166 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 178 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 193 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 204 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 214 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 224 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 232 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 260 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 280 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 296 | — | stays: comment |
| crates/zygo-launch/tests/stub_launch.rs | 302 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 309 | — | stays: comment |
| crates/zygo-launch/tests/stub_launch.rs | 310 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 324 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 334 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 346 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 357 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 366 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 388 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 394 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 410 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 449 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 450 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 451 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 452 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 453 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 454 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 455 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 477 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 491 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 506 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 521 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 534 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 546 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 577 | — | stays: comment |
| crates/zygo-launch/tests/stub_launch.rs | 582 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 598 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 600 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 602 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 603 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 616 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 620 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 638 | — | stays: test |
| crates/zygo-launch/tests/stub_launch.rs | 657 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 28 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 113 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 158 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 176 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 185 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 188 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 219 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 224 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 228 | — | stays: comment |
| crates/zygo-launch/tests/stub_member.rs | 235 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 245 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 250 | — | stays: comment |
| crates/zygo-launch/tests/stub_member.rs | 262 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 269 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 271 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 272 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 278 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 279 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 282 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 290 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 292 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 293 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 298 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 300 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 301 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 306 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 308 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 309 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 314 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 316 | — | stays: test |
| crates/zygo-launch/tests/stub_member.rs | 317 | — | stays: test |
| crates/zygo-mail-sync/tests/common/mod.rs | 266 | 1 | CC-13 |
| crates/zygo-mail-sync/tests/import.rs | 139 | 1 | CC-13 |
| crates/zygo-mail-sync/tests/sync_pass.rs | 1422 | 1 | CC-13 |
| crates/zygo-mcp/src/tools/mod.rs | 22 | 5 | CC-5 |
| crates/zygo-mcp/src/tools/work.rs | 322 | 1 | CC-13 |
| crates/zygo-mcp/tests/mail_remote.rs | 59 | 1 | CC-13 |
| crates/zygo-mcp/tests/mail_remote.rs | 111 | 1 | CC-13 |
| crates/zygo-mcp/tests/mail_tools.rs | 39 | 1 | CC-13 |
| crates/zygo-mcp/tests/mail_tools.rs | 59 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 45 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 126 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 245 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 291 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 298 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 302 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 310 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 317 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 324 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 330 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 332 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 338 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 340 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 357 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 364 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 464 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 545 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 550 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 554 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 556 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 693 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 875 | 1 | CC-13 |
| crates/zygo-mcp/tests/mcp_server.rs | 901 | 1 | CC-13 |
| crates/zygo-mcp/tests/readbacks.rs | 236 | 1 | CC-13 |
| crates/zygo-plugins/src/check.rs | 629 | — | stays: comment |
| crates/zygo-plugins/src/check.rs | 632 | 6 | CC-9 |
| crates/zygo-plugins/src/snapshot.rs | 2 | — | stays: comment |
| crates/zygo-plugins/src/snapshot.rs | 25 | 6 | CC-9 |
| crates/zygo-plugins/src/snapshot.rs | 264 | 6 | CC-9 |
| crates/zygo-plugins/src/snapshot.rs | 265 | 6 | CC-9 |
| crates/zygo-plugins/tests/builtin.rs | 796 | 1 | CC-13 |
| crates/zygo-plugins/tests/builtin.rs | 813 | 1 | CC-13 |
| crates/zygo-plugins/tests/builtin.rs | 819 | 1 | CC-13 |
| crates/zygo-plugins/tests/builtin.rs | 839 | 1 | CC-13 |
| crates/zygo-plugins/tests/check.rs | 228 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 249 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 415 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 625 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 635 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 696 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 697 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 698 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 699 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 753 | — | stays: test |
| crates/zygo-plugins/tests/check.rs | 761 | — | stays: test |
| crates/zygo-plugins/tests/snapshot.rs | 103 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 122 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 127 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 141 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 145 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 157 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 189 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 206 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 219 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 232 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 239 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 265 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 308 | — | stays: test |
| crates/zygo-plugins/tests/snapshot.rs | 310 | — | stays: test |
| crates/zygo-plugins/tests/snapshot.rs | 311 | — | stays: test |
| crates/zygo-plugins/tests/snapshot.rs | 312 | — | stays: test |
| crates/zygo-plugins/tests/snapshot.rs | 330 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 354 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 367 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 368 | — | stays: test |
| crates/zygo-plugins/tests/snapshot.rs | 397 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 410 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 419 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 442 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 553 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 579 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 615 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 630 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 644 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 668 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 688 | 1 | CC-13 |
| crates/zygo-plugins/tests/snapshot.rs | 700 | 1 | CC-13 |
| crates/zygo-proxy/src/forward.rs | 65 | 9 | CC-4 |
| crates/zygo-proxy/src/forward.rs | 310 | 9 | CC-4 |
| crates/zygo-proxy/src/forward.rs | 311 | 9 | CC-4 |
| crates/zygo-proxy/src/forward.rs | 312 | 9 | CC-4 |
| crates/zygo-proxy/src/forward.rs | 313 | 9 | CC-4 |
| crates/zygo-proxy/src/passthrough.rs | 33 | 9 | CC-4 |
| crates/zygo-proxy/src/passthrough.rs | 35 | 9 | CC-4 |
| crates/zygo-proxy/src/passthrough.rs | 46 | 9 | CC-4 |
| crates/zygo-proxy/src/redact_stage.rs | 188 | 9 | CC-4 |
| crates/zygo-proxy/src/translate/responses_client.rs | 708 | — | stays: comment |
| crates/zygo-proxy/src/window.rs | 94 | — | stays: comment |
| crates/zygo-proxy/src/window.rs | 95 | 5 | CC-5 |
| crates/zygo-proxy/src/window.rs | 97 | 5 | CC-5 |
| crates/zygo-proxy/src/window.rs | 132 | 9 | CC-4 |
| crates/zygo-proxy/src/window.rs | 133 | 9 | CC-4 |
| crates/zygo-proxy/src/window.rs | 163 | 5 | CC-5 |
| crates/zygo-proxy/src/window.rs | 229 | 9 | CC-4 |
| crates/zygo-proxy/tests/data/compaction-request.json | 47 | — | stays: test |
| crates/zygo-proxy/tests/facts_schema.rs | 1024 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 21 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 39 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 350 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 351 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 352 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 353 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 354 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 359 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 365 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 370 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 376 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 381 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 384 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 408 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 418 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 422 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 439 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 442 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 450 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 459 | — | stays: comment |
| crates/zygo-proxy/tests/passthrough.rs | 463 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 469 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 483 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 496 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 512 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 531 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 544 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 554 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 568 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 588 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 608 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 625 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 638 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 654 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 668 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 681 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 695 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 708 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 731 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 748 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 777 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 797 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 828 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 842 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 860 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 871 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 885 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 896 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 924 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 925 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 943 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 946 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 955 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 963 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 975 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 987 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1003 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1023 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1064 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1078 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1085 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1094 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1113 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1125 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1136 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1150 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1153 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1167 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1177 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1213 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1219 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 1248 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1249 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1261 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1262 | — | stays: test |
| crates/zygo-proxy/tests/passthrough.rs | 1276 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1299 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1323 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1352 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1382 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1416 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough.rs | 1447 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 107 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 117 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 139 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 155 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 170 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 184 | — | stays: comment |
| crates/zygo-proxy/tests/passthrough_key.rs | 187 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 200 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 215 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 227 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 241 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 255 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 270 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 282 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 292 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 305 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 331 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 353 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 371 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 402 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 410 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 496 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 523 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 539 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 567 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 600 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 618 | 1 | CC-13 |
| crates/zygo-proxy/tests/passthrough_key.rs | 649 | 1 | CC-13 |
| crates/zygo-proxy/tests/redaction_e2e.rs | 144 | 1 | CC-13 |
| crates/zygo-proxy/tests/redaction_e2e.rs | 488 | — | stays: test |
| crates/zygo-proxy/tests/redaction_e2e.rs | 559 | — | stays: test |
| crates/zygo-proxy/tests/redaction_e2e.rs | 563 | 1 | CC-13 |
| crates/zygo-proxy/tests/redaction_e2e.rs | 645 | — | stays: test |
| crates/zygo-proxy/tests/redaction_e2e.rs | 692 | — | stays: test |
| crates/zygo-proxy/tests/redaction_e2e.rs | 747 | 1 | CC-13 |
| crates/zygo-proxy/tests/refusal_calls.rs | 24 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 603 | — | stays: test |
| crates/zygo-proxy/tests/request_path.rs | 693 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 788 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 805 | — | stays: test |
| crates/zygo-proxy/tests/request_path.rs | 821 | — | stays: test |
| crates/zygo-proxy/tests/request_path.rs | 827 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 849 | — | stays: test |
| crates/zygo-proxy/tests/request_path.rs | 852 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 879 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 885 | — | stays: test |
| crates/zygo-proxy/tests/request_path.rs | 904 | — | stays: test |
| crates/zygo-proxy/tests/request_path.rs | 925 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 935 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 950 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 963 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 973 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 995 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1008 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1035 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1063 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1090 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1103 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1127 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1139 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1157 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1184 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1207 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1234 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1259 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1273 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1286 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1307 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1308 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1345 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1358 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1374 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1401 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1414 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1436 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1458 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1493 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1506 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1536 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1558 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1571 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1576 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1590 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1606 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1634 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1644 | 1 | CC-13 |
| crates/zygo-proxy/tests/request_path.rs | 1683 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 14 | — | stays: test |
| crates/zygo-proxy/tests/responses_cache.rs | 38 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 39 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 53 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 57 | — | stays: comment |
| crates/zygo-proxy/tests/responses_cache.rs | 60 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 65 | — | stays: comment |
| crates/zygo-proxy/tests/responses_cache.rs | 68 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 80 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 93 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_cache.rs | 125 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_forward.rs | 24 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_models.rs | 31 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_models.rs | 166 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 23 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 172 | — | stays: comment |
| crates/zygo-proxy/tests/responses_passthrough.rs | 176 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 233 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 244 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 302 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 319 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 325 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 357 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 391 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 445 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_passthrough.rs | 483 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_request.rs | 950 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 951 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 965 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 973 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 974 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 976 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1050 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1102 | — | stays: comment |
| crates/zygo-proxy/tests/responses_request.rs | 1104 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1106 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1107 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1108 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1111 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1112 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1128 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1130 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1135 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1139 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1141 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1145 | — | stays: test |
| crates/zygo-proxy/tests/responses_request.rs | 1146 | — | stays: test |
| crates/zygo-proxy/tests/responses_route.rs | 20 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_route.rs | 214 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_route.rs | 240 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1313 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1316 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1335 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1337 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1370 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1371 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1372 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1375 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1384 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1385 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1388 | — | stays: test |
| crates/zygo-proxy/tests/responses_stream.rs | 1392 | — | stays: test |
| crates/zygo-proxy/tests/responses_text_format.rs | 27 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_text_format.rs | 267 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 231 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 261 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 277 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 284 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 302 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 305 | — | stays: test |
| crates/zygo-proxy/tests/responses_window.rs | 314 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 320 | — | stays: test |
| crates/zygo-proxy/tests/responses_window.rs | 331 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 332 | — | stays: test |
| crates/zygo-proxy/tests/responses_window.rs | 338 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 339 | — | stays: test |
| crates/zygo-proxy/tests/responses_window.rs | 349 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 355 | — | stays: test |
| crates/zygo-proxy/tests/responses_window.rs | 382 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 388 | — | stays: test |
| crates/zygo-proxy/tests/responses_window.rs | 428 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 447 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 459 | 1 | CC-13 |
| crates/zygo-proxy/tests/responses_window.rs | 465 | — | stays: test |
| crates/zygo-proxy/tests/serve.rs | 580 | — | stays: test |
| crates/zygo-proxy/tests/serve.rs | 1467 | 1 | CC-13 |
| crates/zygo-proxy/tests/session_api.rs | 96 | 1 | CC-13 |
| crates/zygo-proxy/tests/session_api.rs | 191 | — | stays: test |
| crates/zygo-proxy/tests/support/responses.rs | 289 | — | stays: test |
| crates/zygo-proxy/tests/translate.rs | 304 | — | stays: test |
| crates/zygo-proxy/tests/translate.rs | 1099 | 1 | CC-13 |
| crates/zygo-proxy/tests/translate.rs | 1259 | — | stays: test |
| crates/zygo-proxy/tests/translate.rs | 1265 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 42 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 344 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 345 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 452 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 453 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 454 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 470 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 561 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 584 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 602 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 614 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 615 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 635 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 644 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 655 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 672 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 675 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 686 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 690 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 694 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 702 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 705 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 708 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 736 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 756 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 771 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 781 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 792 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 813 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 831 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 882 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 905 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 932 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 966 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 971 | — | stays: comment |
| crates/zygo-proxy/tests/window.rs | 972 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 984 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 985 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 999 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1002 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 1025 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1102 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1110 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 1116 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1122 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1145 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1158 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1167 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1177 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1180 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 1189 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1207 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1227 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1242 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1256 | — | stays: test |
| crates/zygo-proxy/tests/window.rs | 1258 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1277 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1286 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1296 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1327 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1333 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1338 | 1 | CC-13 |
| crates/zygo-proxy/tests/window.rs | 1385 | — | stays: comment |
| crates/zygo-proxy/tests/window.rs | 1386 | 1 | CC-13 |
| crates/zygo-redact/src/content.rs | 34 | — | stays: comment |
| crates/zygo-redact/src/content.rs | 69 | — | stays: comment |
| crates/zygo-redact/src/content.rs | 262 | 9 | CC-4 |
| crates/zygo-redact/src/harness.rs | 12 | 9 | CC-4 |
| crates/zygo-redact/src/harness.rs | 43 | — | stays: comment |
| crates/zygo-redact/src/harness.rs | 45 | 9 | CC-4 |
| crates/zygo-redact/tests/content.rs | 17 | 1 | CC-13 |
| crates/zygo-redact/tests/content.rs | 18 | 1 | CC-13 |
| crates/zygo-redact/tests/content.rs | 19 | 1 | CC-13 |
| crates/zygo-redact/tests/content.rs | 58 | — | stays: test |
| crates/zygo-redact/tests/content.rs | 155 | — | stays: test |
| crates/zygo-redact/tests/content.rs | 192 | — | stays: test |
| crates/zygo-redact/tests/content.rs | 364 | — | stays: test |
| crates/zygo-redact/tests/content.rs | 371 | — | stays: test |
| crates/zygo-results/tests/results.rs | 142 | — | stays: test |
| crates/zygo-results/tests/results.rs | 831 | 1 | CC-13 |
| crates/zygo-usage/src/carry.rs | 36 | — | stays: comment |
| crates/zygo-usage/src/savings.rs | 65 | — | stays: comment |
| crates/zygo-usage/src/savings.rs | 77 | 9 | CC-4 |
| crates/zygo-usage/tests/report_snapshot.rs | 23 | — | stays: test |
| crates/zygo-usage/tests/savings.rs | 26 | — | stays: test |
| crates/zygo-usage/tests/savings.rs | 136 | 1 | CC-13 |
| crates/zygo-usage/tests/store.rs | 26 | — | stays: test |
| crates/zygo-usage/tests/store.rs | 174 | — | stays: test |
| crates/zygo-work/src/guards.rs | 1 | — | stays: comment |
| crates/zygo-work/src/guards.rs | 40 | — | stays: comment |
| crates/zygo-work/src/guards.rs | 42 | 2 | CC-6 |
| crates/zygo-work/src/guards.rs | 57 | — | stays: comment |
| crates/zygo-work/src/guards.rs | 119 | 3 | CC-6 |
| crates/zygo-work/src/guards.rs | 153 | 3 | CC-6 |
| crates/zygo-work/src/guards.rs | 1047 | — | stays: exception list (CC-15, K§6.1.4) |
| crates/zygo-work/src/guards.rs | 1059 | — | stays: exception list (CC-15, K§6.1.4) |
| crates/zygo-work/src/guards.rs | 1173 | — | stays: comment |
| crates/zygo-work/src/guards.rs | 1204 | 3 | CC-6 |
| crates/zygo-work/src/hooks.rs | 1 | — | stays: comment |
| crates/zygo-work/src/hooks.rs | 171 | 3 | CC-6 |
| crates/zygo-work/src/hooks.rs | 183 | — | stays: comment |
| crates/zygo-work/src/hooks.rs | 196 | 2 | CC-6 |
| crates/zygo-work/src/hooks.rs | 197 | 2 | CC-6 |
| crates/zygo-work/src/hooks.rs | 371 | 5 | CC-5 |
| crates/zygo-work/src/hooks.rs | 372 | 5 | CC-5 |
| crates/zygo-work/src/hooks.rs | 436 | — | stays: comment |
| crates/zygo-work/src/hooks.rs | 445 | 2 | CC-6 |
| crates/zygo-work/src/hooks.rs | 514 | — | stays: comment |
| crates/zygo-work/src/join.rs | 13 | — | stays: comment |
| crates/zygo-work/src/join.rs | 16 | 9 | CC-4 |
| crates/zygo-work/src/log.rs | 71 | — | stays: comment |
| crates/zygo-work/tests/collection.rs | 23 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 30 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 34 | 1 | CC-13 |
| crates/zygo-work/tests/collection.rs | 71 | 1 | CC-13 |
| crates/zygo-work/tests/collection.rs | 87 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 88 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 93 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 98 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 139 | 1 | CC-13 |
| crates/zygo-work/tests/collection.rs | 401 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 497 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 498 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 499 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 511 | — | stays: comment |
| crates/zygo-work/tests/collection.rs | 557 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 558 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 559 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 560 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 561 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 562 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 564 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 579 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 598 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 623 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 633 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 653 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 654 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 665 | — | stays: comment |
| crates/zygo-work/tests/collection.rs | 679 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 701 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 705 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 712 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 723 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 733 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 767 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 818 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 864 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 867 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 868 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 939 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 951 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1061 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1071 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1078 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1095 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1134 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1164 | — | stays: test |
| crates/zygo-work/tests/collection.rs | 1166 | — | stays: comment |
| crates/zygo-work/tests/collection.rs | 1186 | — | stays: test |
| crates/zygo-work/tests/decision_report.rs | 27 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 11 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 27 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 28 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 33 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 37 | 1 | CC-13 |
| crates/zygo-work/tests/guards.rs | 77 | 1 | CC-13 |
| crates/zygo-work/tests/guards.rs | 92 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 93 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 97 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 153 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 546 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 594 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 597 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 652 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 845 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 846 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1131 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1139 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1140 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1143 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1152 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1156 | — | stays: test |
| crates/zygo-work/tests/guards.rs | 1225 | — | stays: test |
| crates/zygo-workspace/src/activity.rs | 22 | — | stays: comment |
| crates/zygo-workspace/src/activity.rs | 29 | — | stays: comment |
| crates/zygo-workspace/src/activity.rs | 30 | — | stays: comment |
| crates/zygo-workspace/src/activity.rs | 47 | — | stays: comment |
| crates/zygo-workspace/src/activity.rs | 58 | — | stays: comment |
| crates/zygo-workspace/src/activity.rs | 60 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 57 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 58 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 70 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 130 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 358 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 359 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 391 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 396 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 402 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 406 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 507 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 591 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 597 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 598 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 611 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 613 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 615 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 620 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 626 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 650 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 651 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 654 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 655 | 7 | CC-7 |
| crates/zygo-workspace/src/args.rs | 659 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 662 | — | stays: comment |
| crates/zygo-workspace/src/args.rs | 676 | 7 | CC-7 |
| crates/zygo-workspace/src/drain.rs | 108 | — | stays: comment |
| crates/zygo-workspace/src/env.rs | 22 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 23 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 24 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 25 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 38 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 39 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 42 | 11 | CC-9 |
| crates/zygo-workspace/src/env.rs | 43 | 11 | CC-9 |
| crates/zygo-workspace/src/loadout.rs | 12 | 8 | CC-8 |
| crates/zygo-workspace/src/loadout.rs | 98 | 8 | CC-8 |
| crates/zygo-workspace/src/loadout.rs | 111 | — | stays: comment |
| crates/zygo-workspace/src/loadout.rs | 113 | 8 | CC-8 |
| crates/zygo-workspace/src/snapshot.rs | 263 | 8 | CC-8 |
| crates/zygo-workspace/src/snapshot.rs | 264 | 8 | CC-8 |
| crates/zygo-workspace/src/state.rs | 190 | — | stays: comment |
| crates/zygo-workspace/src/state.rs | 242 | 8 | CC-8 |
| crates/zygo-workspace/src/state.rs | 243 | 8 | CC-8 |
| crates/zygo-workspace/src/state.rs | 244 | 8 | CC-8 |
| crates/zygo-workspace/src/state.rs | 245 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 932 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 933 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 1013 | — | stays: comment |
| crates/zygo-workspace/src/up.rs | 1014 | 8 | CC-8 |
| crates/zygo-workspace/src/up.rs | 1029 | — | stays: comment |
| crates/zygo-workspace/src/up.rs | 1039 | 8 | CC-8 |
| crates/zygo-workspace/tests/activity.rs | 29 | — | stays: test |
| crates/zygo-workspace/tests/activity.rs | 239 | — | stays: test |
| crates/zygo-workspace/tests/activity.rs | 240 | — | stays: test |
| crates/zygo-workspace/tests/activity.rs | 250 | — | stays: test |
| crates/zygo-workspace/tests/admin.rs | 447 | 8 | CC-8 |
| crates/zygo-workspace/tests/admin.rs | 449 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 450 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 451 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 452 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 465 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 473 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 710 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 711 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 712 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 718 | — | stays: test |
| crates/zygo-workspace/tests/admin.rs | 772 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 944 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 992 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1134 | 8 | CC-8 |
| crates/zygo-workspace/tests/admin.rs | 1135 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1136 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1160 | 8 | CC-8 |
| crates/zygo-workspace/tests/admin.rs | 1282 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1417 | — | stays: test |
| crates/zygo-workspace/tests/admin.rs | 1436 | — | stays: test |
| crates/zygo-workspace/tests/admin.rs | 1626 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1808 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1811 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 1815 | 8 | CC-8 |
| crates/zygo-workspace/tests/admin.rs | 1942 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2004 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2005 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2006 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2017 | — | stays: test |
| crates/zygo-workspace/tests/admin.rs | 2028 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2029 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2058 | 1 | CC-13 |
| crates/zygo-workspace/tests/admin.rs | 2353 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 12 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 15 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 37 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 43 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 45 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 58 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 64 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 70 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 73 | — | stays: comment |
| crates/zygo-workspace/tests/args.rs | 82 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 91 | — | stays: comment |
| crates/zygo-workspace/tests/args.rs | 93 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 101 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 106 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 108 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 109 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 111 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 113 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 116 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 118 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 120 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 123 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 126 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 127 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 129 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 138 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 140 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 141 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 142 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 150 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 152 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 153 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 159 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 169 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 191 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 200 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 202 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 212 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 234 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 255 | — | stays: comment |
| crates/zygo-workspace/tests/args.rs | 258 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 265 | — | stays: comment |
| crates/zygo-workspace/tests/args.rs | 353 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 370 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 384 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 409 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 412 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 414 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 423 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 426 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 482 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 483 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 485 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 488 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 490 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 491 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 492 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 493 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 494 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 495 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 502 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 516 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 517 | — | stays: comment |
| crates/zygo-workspace/tests/args.rs | 521 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 533 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 534 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 535 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 536 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 537 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 538 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 539 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 540 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 541 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 542 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 543 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 544 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 549 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 551 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 556 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 558 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 563 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 564 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 565 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 566 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 567 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 572 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 573 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 578 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 585 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 592 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 594 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 618 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 619 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 620 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 631 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 634 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 635 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 636 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 637 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 638 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 639 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 640 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 641 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 642 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 643 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 644 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 650 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 651 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 652 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 653 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 654 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 655 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 656 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 657 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 658 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 659 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 660 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 661 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 662 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 663 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 670 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 679 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 686 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 690 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 751 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 754 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 756 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 765 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 767 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 768 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 771 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 779 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 785 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 789 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 793 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 795 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 797 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 799 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 803 | — | stays: test |
| crates/zygo-workspace/tests/args.rs | 816 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 822 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 824 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 825 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 826 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 827 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 835 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 842 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 850 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 853 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 857 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 858 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 860 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 863 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 870 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 872 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 888 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 891 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 892 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 900 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 915 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 917 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 923 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 939 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 941 | 1 | CC-13 |
| crates/zygo-workspace/tests/args.rs | 943 | 1 | CC-13 |
| crates/zygo-workspace/tests/down.rs | 140 | 1 | CC-13 |
| crates/zygo-workspace/tests/down.rs | 2496 | 1 | CC-13 |
| crates/zygo-workspace/tests/down.rs | 2528 | 1 | CC-13 |
| crates/zygo-workspace/tests/down.rs | 2537 | 1 | CC-13 |
| crates/zygo-workspace/tests/env.rs | 16 | — | stays: test |
| crates/zygo-workspace/tests/env.rs | 20 | — | stays: test |
| crates/zygo-workspace/tests/env.rs | 21 | — | stays: test |
| crates/zygo-workspace/tests/env.rs | 29 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 4 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 33 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 36 | 1 | CC-13 |
| crates/zygo-workspace/tests/loadout.rs | 49 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 53 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 57 | 1 | CC-13 |
| crates/zygo-workspace/tests/loadout.rs | 59 | 1 | CC-13 |
| crates/zygo-workspace/tests/loadout.rs | 63 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 71 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 75 | 1 | CC-13 |
| crates/zygo-workspace/tests/loadout.rs | 81 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 82 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 92 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 101 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 110 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 139 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 174 | 1 | CC-13 |
| crates/zygo-workspace/tests/loadout.rs | 198 | — | stays: test |
| crates/zygo-workspace/tests/loadout.rs | 199 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 168 | 8 | CC-8 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 169 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 170 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 192 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 293 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 784 | — | stays: comment |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 787 | — | stays: test |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 844 | — | stays: comment |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 851 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 853 | — | stays: test |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 869 | 8 | CC-8 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 871 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 872 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 873 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 902 | 8 | CC-8 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 903 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 922 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 925 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 931 | 8 | CC-8 |
| crates/zygo-workspace/tests/mail_lifecycle.rs | 958 | — | stays: test |
| crates/zygo-workspace/tests/mail_names.rs | 37 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_wake.rs | 65 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_wake.rs | 360 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_wake.rs | 459 | 1 | CC-13 |
| crates/zygo-workspace/tests/mail_wake.rs | 577 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 483 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 484 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 485 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 486 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 516 | 8 | CC-8 |
| crates/zygo-workspace/tests/snapshot.rs | 534 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 832 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 1045 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 1068 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 1096 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 1210 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 1211 | 1 | CC-13 |
| crates/zygo-workspace/tests/snapshot.rs | 1212 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 46 | 8 | CC-8 |
| crates/zygo-workspace/tests/state.rs | 48 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 81 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 86 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 100 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 101 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 103 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 104 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 117 | 8 | CC-8 |
| crates/zygo-workspace/tests/state.rs | 212 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 244 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 256 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 296 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 327 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 328 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 383 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 385 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 386 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 391 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 394 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 399 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 402 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 410 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 411 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 442 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 460 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 473 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 474 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 479 | 8 | CC-8 |
| crates/zygo-workspace/tests/state.rs | 490 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 491 | 8 | CC-8 |
| crates/zygo-workspace/tests/state.rs | 494 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 505 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 506 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 541 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 553 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 621 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 751 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 1137 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 1140 | — | stays: test |
| crates/zygo-workspace/tests/state.rs | 1142 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 1143 | 1 | CC-13 |
| crates/zygo-workspace/tests/state.rs | 1150 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 243 | — | stays: comment |
| crates/zygo-workspace/tests/up.rs | 246 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 858 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 860 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1020 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1022 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1023 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1024 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1030 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1031 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1165 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1167 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1176 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1177 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1183 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1184 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1185 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1191 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1194 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1272 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1276 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1277 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1279 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1313 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1341 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1363 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1381 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1382 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 1388 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1663 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1671 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 1751 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1910 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 1911 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2198 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2201 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2203 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 2204 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 2213 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2252 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2255 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2257 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 2265 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 2266 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 2267 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 3082 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 3083 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 3090 | — | stays: test |
| crates/zygo-workspace/tests/up.rs | 3111 | 8 | CC-8 |
| crates/zygo-workspace/tests/up.rs | 3112 | 1 | CC-13 |
| crates/zygo-workspace/tests/up.rs | 3118 | 8 | CC-8 |
| crates/zygo-workspace/tests/up_all.rs | 488 | 1 | CC-13 |
| crates/zygo-workspace/tests/up_definition.rs | 355 | 1 | CC-13 |
| crates/zygo-workspace/tests/up_definition.rs | 438 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 455 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 555 | — | stays: comment |
| crates/zygo-workspace/tests/up_definition.rs | 557 | 1 | CC-13 |
| crates/zygo-workspace/tests/up_definition.rs | 559 | 8 | CC-8 |
| crates/zygo-workspace/tests/up_definition.rs | 914 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 916 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 919 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 920 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 927 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 949 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 1008 | — | stays: test |
| crates/zygo-workspace/tests/up_definition.rs | 1045 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 4 | 1 | CC-9 |
| crates/zygo/src/cmd/claude.rs | 21 | 1 | CC-9 |
| crates/zygo/src/cmd/claude.rs | 34 | 1 | CC-10 |
| crates/zygo/src/cmd/claude.rs | 61 | 1 | CC-10 |
| crates/zygo/src/cmd/claude.rs | 66 | — | stays: comment |
| crates/zygo/src/cmd/claude.rs | 68 | 1 | CC-10 |
| crates/zygo/src/cmd/claude.rs | 140 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 207 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 209 | — | stays: test |
| crates/zygo/src/cmd/claude.rs | 234 | 1 | CC-13 |
| crates/zygo/src/cmd/compact.rs | 61 | 10 | CC-10 |
| crates/zygo/src/cmd/compact.rs | 62 | 10 | CC-10 |
| crates/zygo/src/cmd/compact.rs | 173 | 1 | CC-13 |
| crates/zygo/src/cmd/compact.rs | 269 | 1 | CC-13 |
| crates/zygo/src/cmd/compact.rs | 270 | — | stays: test |
| crates/zygo/src/cmd/config.rs | 122 | — | stays: comment |
| crates/zygo/src/cmd/config.rs | 131 | 1 | CC-10 |
| crates/zygo/src/cmd/config.rs | 132 | 1 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 200 | 1 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 352 | 8 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 459 | 1 | CC-10 |
| crates/zygo/src/cmd/doctor.rs | 460 | 1 | CC-10 |
| crates/zygo/src/cmd/hook.rs | 106 | 1 | CC-10 |
| crates/zygo/src/cmd/loadout.rs | 330 | 1 | CC-10 |
| crates/zygo/src/cmd/loadout.rs | 493 | — | stays: comment |
| crates/zygo/src/cmd/loadout.rs | 590 | — | stays: test |
| crates/zygo/src/cmd/loadout.rs | 593 | — | stays: comment |
| crates/zygo/src/cmd/loadout.rs | 624 | 1 | CC-13 |
| crates/zygo/src/cmd/loadout.rs | 648 | 1 | CC-13 |
| crates/zygo/src/cmd/loadout.rs | 733 | 1 | CC-13 |
| crates/zygo/src/cmd/loadout.rs | 851 | 1 | CC-13 |
| crates/zygo/src/cmd/loadout.rs | 986 | — | stays: test |
| crates/zygo/src/cmd/mail.rs | 893 | 1 | CC-13 |
| crates/zygo/src/cmd/mcp.rs | 6 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 17 | 1 | CC-9 |
| crates/zygo/src/cmd/member.rs | 19 | 1 | CC-9 |
| crates/zygo/src/cmd/member.rs | 95 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 107 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 108 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 234 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 235 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 236 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 237 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 390 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 469 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 470 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 570 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 573 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 581 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 582 | 1 | CC-10 |
| crates/zygo/src/cmd/member.rs | 631 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 635 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 636 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 645 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 662 | 1 | CC-9 |
| crates/zygo/src/cmd/member.rs | 664 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 678 | 1 | CC-9 |
| crates/zygo/src/cmd/member.rs | 691 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 692 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 693 | 8 | CC-10 |
| crates/zygo/src/cmd/member.rs | 754 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 759 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 760 | 7 | CC-10 |
| crates/zygo/src/cmd/member.rs | 1173 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1175 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1204 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1307 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1318 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1319 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1322 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1326 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1330 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1331 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1335 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1339 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1341 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1344 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1347 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1357 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1358 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1360 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1361 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1363 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1364 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1367 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1368 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1372 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1373 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1431 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1432 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1433 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1434 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1435 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1436 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1437 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1438 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1439 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1440 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1466 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1493 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1523 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1543 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1544 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1545 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1546 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1547 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1555 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1557 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1558 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1559 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1560 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1561 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1562 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1563 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1565 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1569 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1573 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1575 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1579 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1580 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1616 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1626 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1654 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1674 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1721 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1738 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1745 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1754 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1762 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1775 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1785 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1791 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1796 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1798 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1799 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1800 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1801 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1802 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1803 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1823 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1824 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1827 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1832 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1846 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1861 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1862 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1873 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1881 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1909 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1913 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1916 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1918 | — | stays: test |
| crates/zygo/src/cmd/member.rs | 1922 | — | stays: comment |
| crates/zygo/src/cmd/member.rs | 1923 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1936 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1941 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 1968 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2000 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2012 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2029 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2030 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2031 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2032 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2033 | 1 | CC-13 |
| crates/zygo/src/cmd/member.rs | 2039 | 1 | CC-13 |
| crates/zygo/src/cmd/mod.rs | 12 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 29 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 205 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 238 | — | stays: comment |
| crates/zygo/src/cmd/mod.rs | 279 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 280 | — | stays: comment |
| crates/zygo/src/cmd/mod.rs | 281 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 406 | — | stays: comment |
| crates/zygo/src/cmd/mod.rs | 609 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 610 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 705 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 707 | 1 | CC-10 |
| crates/zygo/src/cmd/mod.rs | 709 | 1 | CC-10 |
| crates/zygo/src/cmd/opencode.rs | 5 | 1 | CC-9 |
| crates/zygo/src/cmd/opencode.rs | 11 | — | stays: comment |
| crates/zygo/src/cmd/opencode.rs | 21 | 1 | CC-9 |
| crates/zygo/src/cmd/opencode.rs | 34 | 1 | CC-10 |
| crates/zygo/src/cmd/opencode.rs | 55 | 1 | CC-10 |
| crates/zygo/src/cmd/opencode.rs | 108 | — | stays: test |
| crates/zygo/src/cmd/opencode.rs | 115 | — | stays: test |
| crates/zygo/src/cmd/opencode.rs | 123 | 1 | CC-13 |
| crates/zygo/src/cmd/statusline.rs | 5 | — | stays: comment |
| crates/zygo/src/cmd/verbs.rs | 451 | 1 | CC-13 |
| crates/zygo/src/cmd/workspace_approve.rs | 180 | 8 | CC-8 |
| crates/zygo/src/cmd/ws.rs | 15 | 1 | CC-9 |
| crates/zygo/src/cmd/ws.rs | 18 | 1 | CC-9 |
| crates/zygo/src/cmd/ws.rs | 406 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 407 | 1 | CC-9 |
| crates/zygo/src/cmd/ws.rs | 413 | 1 | CC-9 |
| crates/zygo/src/cmd/ws.rs | 417 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 418 | 1 | CC-9 |
| crates/zygo/src/cmd/ws.rs | 471 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 479 | — | stays: comment |
| crates/zygo/src/cmd/ws.rs | 608 | — | stays: comment |
| crates/zygo/src/cmd/ws.rs | 612 | 10 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 613 | 10 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 618 | — | stays: comment |
| crates/zygo/src/cmd/ws.rs | 648 | — | stays: comment |
| crates/zygo/src/cmd/ws.rs | 651 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 667 | — | stays: comment |
| crates/zygo/src/cmd/ws.rs | 673 | 8 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 708 | 1 | CC-13 |
| crates/zygo/src/cmd/ws.rs | 725 | 1 | CC-13 |
| crates/zygo/src/cmd/ws.rs | 728 | 1 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 730 | 1 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 737 | 1 | CC-13 |
| crates/zygo/src/cmd/ws.rs | 751 | 1 | CC-13 |
| crates/zygo/src/cmd/ws.rs | 776 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 778 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 780 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 781 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 792 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 801 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 803 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 804 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 805 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 810 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 820 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 821 | — | stays: test |
| crates/zygo/src/cmd/ws.rs | 875 | 1 | CC-13 |
| crates/zygo/src/cmd/ws.rs | 882 | 1 | CC-10 |
| crates/zygo/src/cmd/ws.rs | 883 | 1 | CC-10 |
| crates/zygo/tests/cli_surface.rs | 33 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 92 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 96 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 97 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 326 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 384 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 385 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 402 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 403 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 406 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 411 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 417 | — | stays: comment |
| crates/zygo/tests/cli_surface.rs | 423 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 424 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 433 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 460 | — | stays: comment |
| crates/zygo/tests/cli_surface.rs | 471 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 485 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 487 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 523 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 533 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 554 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 620 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 626 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 645 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 646 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 649 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 659 | — | stays: comment |
| crates/zygo/tests/cli_surface.rs | 666 | 1 | CC-13 |
| crates/zygo/tests/cli_surface.rs | 670 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 693 | — | stays: test |
| crates/zygo/tests/cli_surface.rs | 740 | — | stays: test |
| crates/zygo/tests/common/e2e.rs | 30 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 177 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 185 | 1 | CC-13 |
| crates/zygo/tests/common/e2e.rs | 186 | 1 | CC-13 |
| crates/zygo/tests/common/e2e.rs | 212 | 1 | CC-13 |
| crates/zygo/tests/common/e2e.rs | 274 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 280 | — | stays: test |
| crates/zygo/tests/common/e2e.rs | 608 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 611 | 1 | CC-13 |
| crates/zygo/tests/common/e2e.rs | 614 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 616 | — | stays: test |
| crates/zygo/tests/common/e2e.rs | 621 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 622 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 625 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 629 | — | stays: test |
| crates/zygo/tests/common/e2e.rs | 859 | — | stays: comment |
| crates/zygo/tests/common/e2e.rs | 867 | — | stays: test |
| crates/zygo/tests/common/e2e.rs | 877 | — | stays: comment |
| crates/zygo/tests/common/workspace.rs | 306 | — | stays: test |
| crates/zygo/tests/compact_cmd.rs | 57 | 1 | CC-13 |
| crates/zygo/tests/compact_cmd.rs | 159 | — | stays: test |
| crates/zygo/tests/compact_cmd.rs | 169 | — | stays: test |
| crates/zygo/tests/compact_cmd.rs | 361 | — | stays: test |
| crates/zygo/tests/compact_cmd.rs | 363 | — | stays: test |
| crates/zygo/tests/decision_cli.rs | 313 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 6 | — | stays: comment |
| crates/zygo/tests/decision_gate.rs | 144 | 1 | CC-13 |
| crates/zygo/tests/decision_gate.rs | 442 | — | stays: comment |
| crates/zygo/tests/decision_gate.rs | 449 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 488 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 498 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 502 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 512 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 541 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 545 | — | stays: test |
| crates/zygo/tests/decision_gate.rs | 549 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 345 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 371 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 501 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 502 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 503 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 555 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 567 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 579 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 580 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 581 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 582 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 602 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 603 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 604 | — | stays: test |
| crates/zygo/tests/definition_doctor.rs | 667 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 5 | — | stays: comment |
| crates/zygo/tests/definition_hooks.rs | 663 | 1 | CC-13 |
| crates/zygo/tests/definition_hooks.rs | 730 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 734 | — | stays: comment |
| crates/zygo/tests/definition_hooks.rs | 739 | — | stays: comment |
| crates/zygo/tests/definition_hooks.rs | 741 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 765 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 766 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 769 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 770 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 771 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 772 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 773 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 774 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 775 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 776 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 777 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 778 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 779 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 780 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 781 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 782 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 783 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 784 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 785 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 786 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 796 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 810 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 833 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 848 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 849 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 850 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 851 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 852 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 853 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 854 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 863 | — | stays: comment |
| crates/zygo/tests/definition_hooks.rs | 1078 | 1 | CC-13 |
| crates/zygo/tests/definition_hooks.rs | 1105 | 1 | CC-13 |
| crates/zygo/tests/definition_hooks.rs | 1132 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 1145 | 1 | CC-13 |
| crates/zygo/tests/definition_hooks.rs | 1154 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 1160 | 1 | CC-13 |
| crates/zygo/tests/definition_hooks.rs | 1202 | — | stays: test |
| crates/zygo/tests/definition_hooks.rs | 1203 | 1 | CC-13 |
| crates/zygo/tests/definition_surfaces.rs | 583 | — | stays: test |
| crates/zygo/tests/definition_surfaces.rs | 641 | — | stays: test |
| crates/zygo/tests/definition_surfaces.rs | 732 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 10 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 17 | 1 | CC-13 |
| crates/zygo/tests/guard_hook.rs | 40 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 45 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 92 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 110 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 115 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 192 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 212 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 239 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 443 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 465 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 473 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 493 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 506 | — | stays: test |
| crates/zygo/tests/guard_hook.rs | 511 | — | stays: test |
| crates/zygo/tests/hook_session.rs | 12 | — | stays: test |
| crates/zygo/tests/hook_session.rs | 21 | 1 | CC-13 |
| crates/zygo/tests/hook_session.rs | 73 | — | stays: test |
| crates/zygo/tests/hook_session.rs | 74 | — | stays: test |
| crates/zygo/tests/hook_session.rs | 121 | — | stays: test |
| crates/zygo/tests/hook_trim.rs | 10 | 1 | CC-13 |
| crates/zygo/tests/hook_trim.rs | 24 | — | stays: test |
| crates/zygo/tests/hook_trim.rs | 25 | — | stays: test |
| crates/zygo/tests/hook_trim.rs | 40 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 14 | — | stays: comment |
| crates/zygo/tests/loadout_secrets.rs | 100 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 103 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 106 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 109 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 112 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 125 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 130 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 139 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 149 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 152 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 160 | — | stays: comment |
| crates/zygo/tests/loadout_secrets.rs | 166 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 488 | — | stays: comment |
| crates/zygo/tests/loadout_secrets.rs | 496 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 499 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 512 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 513 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 524 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 559 | — | stays: test |
| crates/zygo/tests/loadout_secrets.rs | 596 | — | stays: test |
| crates/zygo/tests/mail_cli.rs | 37 | 1 | CC-13 |
| crates/zygo/tests/mail_cli.rs | 527 | 1 | CC-13 |
| crates/zygo/tests/mail_remote_cli.rs | 91 | 1 | CC-13 |
| crates/zygo/tests/mail_remote_cli.rs | 411 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_cli.rs | 82 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_forget.rs | 71 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_no_content.rs | 211 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_no_content.rs | 487 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_no_content.rs | 1816 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_no_content.rs | 1984 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_no_content.rs | 1989 | — | stays: test |
| crates/zygo/tests/mail_sync_no_content.rs | 2440 | 1 | CC-13 |
| crates/zygo/tests/mail_sync_no_content.rs | 2537 | — | stays: test |
| crates/zygo/tests/mail_sync_no_content.rs | 2571 | — | stays: test |
| crates/zygo/tests/mail_sync_no_content.rs | 2584 | — | stays: comment |
| crates/zygo/tests/mcp_stdio.rs | 21 | 1 | CC-13 |
| crates/zygo/tests/mcp_stdio.rs | 45 | — | stays: test |
| crates/zygo/tests/mcp_stdio.rs | 56 | — | stays: test |
| crates/zygo/tests/mcp_stdio.rs | 60 | — | stays: test |
| crates/zygo/tests/mcp_stdio.rs | 63 | — | stays: test |
| crates/zygo/tests/mcp_stdio.rs | 69 | — | stays: test |
| crates/zygo/tests/member_fixture/mod.rs | 41 | 1 | CC-13 |
| crates/zygo/tests/results_sweep.rs | 78 | — | stays: test |
| crates/zygo/tests/results_sweep.rs | 109 | — | stays: test |
| crates/zygo/tests/session_verbs.rs | 56 | 1 | CC-13 |
| crates/zygo/tests/session_verbs.rs | 132 | — | stays: test |
| crates/zygo/tests/slash_parity.rs | 54 | 1 | CC-13 |
| crates/zygo/tests/slash_parity.rs | 118 | — | stays: test |
| crates/zygo/tests/slash_parity.rs | 128 | — | stays: test |
| crates/zygo/tests/statusline_cmd.rs | 4 | — | stays: comment |
| crates/zygo/tests/statusline_cmd.rs | 49 | — | stays: test |
| crates/zygo/tests/statusline_cmd.rs | 50 | — | stays: test |
| crates/zygo/tests/statusline_cmd.rs | 87 | — | stays: test |
| crates/zygo/tests/statusline_cmd.rs | 314 | — | stays: comment |
| crates/zygo/tests/update.rs | 798 | — | stays: test |
| crates/zygo/tests/update.rs | 803 | — | stays: test |
| crates/zygo/tests/update.rs | 809 | — | stays: test |
| crates/zygo/tests/update.rs | 815 | — | stays: test |
| crates/zygo/tests/update.rs | 896 | 1 | CC-13 |
| crates/zygo/tests/update.rs | 914 | 1 | CC-13 |
| crates/zygo/tests/update.rs | 1012 | 1 | CC-13 |
| crates/zygo/tests/update.rs | 1086 | 8 | CC-8 |
| crates/zygo/tests/update.rs | 1090 | 1 | CC-13 |
| crates/zygo/tests/usage_doctor.rs | 57 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 415 | 1 | CC-13 |
| crates/zygo/tests/usage_doctor.rs | 418 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 419 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 423 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 426 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 427 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 428 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 429 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 436 | 1 | CC-13 |
| crates/zygo/tests/usage_doctor.rs | 437 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 438 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 440 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 441 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 442 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 443 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 463 | 1 | CC-13 |
| crates/zygo/tests/usage_doctor.rs | 464 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 465 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 466 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 709 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 963 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 998 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 1023 | — | stays: test |
| crates/zygo/tests/usage_doctor.rs | 1135 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 13 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 20 | 1 | CC-13 |
| crates/zygo/tests/work_hook.rs | 52 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 53 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 57 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 59 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 90 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 103 | — | stays: test |
| crates/zygo/tests/work_hook.rs | 104 | — | stays: test |
| crates/zygo/tests/workspace_approve.rs | 258 | — | stays: test |
| crates/zygo/tests/workspace_approve.rs | 565 | — | stays: test |
| crates/zygo/tests/workspace_approve.rs | 566 | — | stays: test |
| crates/zygo/tests/workspace_approve.rs | 661 | — | stays: test |
| crates/zygo/tests/workspace_approve.rs | 846 | — | stays: test |
| crates/zygo/tests/workspace_cli.rs | 28 | — | stays: comment |
| crates/zygo/tests/workspace_cli.rs | 44 | — | stays: test |
| crates/zygo/tests/workspace_cli.rs | 300 | — | stays: test |
| crates/zygo/tests/workspace_cli.rs | 364 | 1 | CC-13 |
| crates/zygo/tests/workspace_cli.rs | 369 | — | stays: test |
| crates/zygo/tests/workspace_definition.rs | 166 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 523 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 585 | 8 | CC-8 |
| crates/zygo/tests/workspace_definition.rs | 594 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 645 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 668 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 687 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 710 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 726 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 730 | 1 | CC-13 |
| crates/zygo/tests/workspace_definition.rs | 788 | — | stays: test |
| crates/zygo/tests/workspace_definition.rs | 842 | — | stays: test |
| crates/zygo/tests/workspace_definition.rs | 1017 | — | stays: comment |
| crates/zygo/tests/workspace_down.rs | 22 | — | stays: comment |
| crates/zygo/tests/workspace_down.rs | 23 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 139 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 145 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 196 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 221 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 225 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 230 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 236 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 255 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 256 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 258 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 261 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 262 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 264 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 266 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 294 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 328 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 332 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 334 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 340 | — | stays: comment |
| crates/zygo/tests/workspace_down.rs | 374 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 485 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 581 | — | stays: test |
| crates/zygo/tests/workspace_down.rs | 582 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 19 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 76 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 81 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 83 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 145 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 146 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 150 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 153 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 158 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 162 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 164 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 259 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 260 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 261 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 263 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 264 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 266 | — | stays: test |
| crates/zygo/tests/workspace_mail.rs | 270 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 168 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 169 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 313 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 319 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 347 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 350 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 373 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 376 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 397 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 402 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 412 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 415 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 420 | 8 | CC-8 |
| crates/zygo/tests/workspace_member.rs | 450 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 451 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 462 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 474 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 475 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 495 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 496 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 513 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 517 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 535 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 543 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 544 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 545 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 546 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 547 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 548 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 549 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 550 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 551 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 552 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 553 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 559 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 563 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 572 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 573 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 575 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 580 | — | stays: comment |
| crates/zygo/tests/workspace_member.rs | 587 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 592 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 594 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 595 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 597 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 599 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 609 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 610 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 611 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 633 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 635 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 636 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 640 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 652 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 653 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 667 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 678 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 679 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 711 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 719 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 720 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 736 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 739 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 740 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 757 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 764 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 772 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 793 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 796 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 807 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 814 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 815 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 831 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 833 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 838 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 871 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 872 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 925 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 926 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 943 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 944 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 957 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 959 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 981 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 983 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 988 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1017 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1019 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1043 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1046 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1072 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1096 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1112 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1122 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1135 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1138 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1150 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1169 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1173 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1180 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1181 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1189 | — | stays: comment |
| crates/zygo/tests/workspace_member.rs | 1204 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1206 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1207 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1223 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1228 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1229 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1231 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1239 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1257 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1258 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1276 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1278 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1304 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1309 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1328 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1358 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1359 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1364 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1374 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1392 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1403 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1412 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1433 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1436 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1543 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1575 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1587 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1590 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1597 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1627 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1632 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1633 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1647 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1649 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1660 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1664 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1681 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1684 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1686 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1688 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1698 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1724 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1726 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1728 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1729 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1796 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1797 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1801 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1805 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1816 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1817 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1823 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1824 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1839 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1841 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1852 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1854 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1864 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1867 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1868 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1880 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1883 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1900 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1923 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1924 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1934 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1951 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1952 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1953 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 1963 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1965 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 1998 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 2002 | — | stays: test |
| crates/zygo/tests/workspace_member.rs | 2016 | 1 | CC-13 |
| crates/zygo/tests/workspace_member.rs | 2020 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 30 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 31 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 37 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 90 | — | stays: comment |
| crates/zygo/tests/workspace_restore.rs | 244 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 249 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 250 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 570 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 642 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 715 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 749 | — | stays: comment |
| crates/zygo/tests/workspace_restore.rs | 751 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 781 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 817 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 869 | — | stays: test |
| crates/zygo/tests/workspace_restore.rs | 1195 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 143 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 144 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 203 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 210 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 240 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 241 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 266 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 270 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 271 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 272 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 273 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 286 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 287 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 288 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 310 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 313 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 358 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 360 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 379 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 380 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 401 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 402 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 413 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 414 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 428 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 431 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 461 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 470 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 492 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 493 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 505 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 506 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 519 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 520 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 530 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 531 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 545 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 550 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 567 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 572 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 588 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 589 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 592 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 594 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 597 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 599 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 608 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 609 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 612 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 614 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 617 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 618 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 620 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 622 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 629 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 659 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 661 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 666 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 667 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 670 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 674 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 686 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 687 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 696 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 703 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 708 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 727 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 728 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 737 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 750 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 753 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 761 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 772 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 773 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 788 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 812 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 813 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 840 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 841 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 864 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 871 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 886 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 887 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 898 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 981 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 986 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 987 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 988 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 989 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 994 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1066 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 1068 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1072 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1085 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1090 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1106 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1111 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1112 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1113 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1117 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1125 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1131 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1154 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1157 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1165 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1172 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 1173 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 1182 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1202 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1228 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1322 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1325 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1337 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1371 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1376 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1393 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1396 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1402 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1408 | — | stays: comment |
| crates/zygo/tests/workspace_run.rs | 1415 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1424 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1425 | — | stays: test |
| crates/zygo/tests/workspace_run.rs | 1456 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1459 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1483 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1487 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1513 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1514 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1533 | 8 | CC-8 |
| crates/zygo/tests/workspace_run.rs | 1535 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1537 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1557 | 1 | CC-13 |
| crates/zygo/tests/workspace_run.rs | 1562 | 1 | CC-13 |
| crates/zygo/tests/workspace_session.rs | 1 | — | stays: comment |
| crates/zygo/tests/workspace_session.rs | 19 | — | stays: comment |
| crates/zygo/tests/workspace_session.rs | 20 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 45 | — | stays: comment |
| crates/zygo/tests/workspace_session.rs | 48 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 49 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 62 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 66 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 163 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 171 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 173 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 188 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 264 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 265 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 309 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 325 | — | stays: test |
| crates/zygo/tests/workspace_session.rs | 336 | — | stays: comment |

Counts (review round 1, widened search): 3011 sites in 198 files. By node: CC-3 18, CC-4 18, CC-5 6, CC-6 28, CC-7 16, CC-8 68, CC-9 143, CC-10 51, CC-13 1178, stays: test 1211, stays: comment 272, stays: exception list 2. By concern (CC rows): 1 1300, 2 22, 3 16, 4 5, 5 8, 6 24, 7 28, 8 82, 9 20, 10 4, 11 17. By node and concern: CC-3 (1: 15, 2: 3); CC-4 (9: 18); CC-5 (5: 6); CC-6 (2: 8, 3: 16, 4: 4); CC-7 (7: 16); CC-8 (8: 68); CC-9 (1: 78, 2: 11, 4: 1, 5: 2, 6: 24, 7: 8, 9: 2, 11: 17); CC-10 (1: 29, 7: 4, 8: 14, 10: 4); CC-13 (1: 1178). No row goes to CC-15 as a new exception.

Search (completeness is bounded by these patterns): the rows are exactly the lines that `git grep -nE` finds at the base with the patterns below, in `crates/*/src/**`, `crates/*/tests/**`, `crates/zygo-config/data/agents.toml` and `crates/zygo-launch/assets/opencode-plugin.js`. A client branch that matches none of them is not in this inventory. Not searched: a literal that only contains a client name (`"zygo claude: …"`, `"/b/claude"`), except the id `claude-code`; the words Claude, OpenCode and Codex in prose; the Codex names other than `CODEX_*` variables (no Codex client exists at the base).

- `Agent::(ClaudeCode|OpenCode|\{|\*)`
- `PaneKind::(ClaudeCode|OpenCode)`
- `Plugins::`
- `"(claude-code|claude|opencode|\.claude|\.opencode|CLAUDE_CONFIG_DIR|OPENCODE_CONFIG_DIR|Claude Code|OpenCode)"`
- `(^|[^A-Za-z0-9_])(claude|opencode|claude_code|open_code)[a-z_]*: bool`
- `(^|[^A-Za-z0-9_])!(claude|opencode|claude_code)([^A-Za-z0-9_]|$)`
- `(^|[^A-Za-z0-9_])(CLAUDE[A-Z_]*|OPENCODE_[A-Z_]+|CODEX_[A-Z_]+)([^A-Za-z0-9_]|$)`
- `(^|[^A-Za-z0-9_])(LOADOUT_KINDS|AGENT_ONLY|CLAUDE_ONLY)([^A-Za-z0-9_]|$)`
- `[xX]-[cC]laude-[cC]ode-`
- `claude-code`
- `^\[(claude-code|opencode)\]`
- `agents\.toml`
- `(^|[^A-Za-z0-9_])(PreToolUse|PostToolUse|PostToolUseFailure|PreCompact|SessionStart|SessionEnd|SubagentStart|SubagentStop|UserPromptSubmit)([^A-Za-z0-9_]|$)`
- `"(Stop|StopFailure|Notification)"`
- `mcp__`
- `pub fn compact_tool\(|is_some_and\(compact_tool\)|window::compact_tool`
- `(^|[^A-Za-z0-9_])(ClaudeCode|OpenCode)([^A-Za-z0-9_]|$)`
- `(^|[^A-Za-z0-9_])(Claude|Opencode)\(`
- `(^|[^A-Za-z0-9_])(claude|opencode|claude_code)::|mod (claude|opencode|claude_code);`

Rules for each row:

- Test sites: a site in a `#[cfg(test)]` item of a `src` file, or in `crates/*/tests/`. Test items at the base that hold a site: `crates/zygo-hooks/src/prompt.rs` 147-469; `crates/zygo-mcp/src/tools/work.rs` 263-328; `crates/zygo/src/cmd/claude.rs` 88-239; `cmd/compact.rs` 128-130, 132-333; `cmd/loadout.rs` 480-1003; `cmd/mail.rs` 763-934 (`all(test, unix)`); `cmd/member.rs` 1117-2047; `cmd/mod.rs` 647-783; `cmd/opencode.rs` 59-128; `cmd/verbs.rs` 338-766; `cmd/ws.rs` 683-1034. A test line with an `Agent` variant path or import, or a bare variant use after such an import (`crates/zygo/src/cmd/member.rs:1796`-`:1803`), is CC-13 (concern 1); a `PaneKind` variant path is CC-8 (8); `Plugins::` is CC-9 (6); the launch adapter type or module (`ClaudeCode`, `OpenCode`, `zygo_launch::claude_code`, `zygo_launch::opencode`) is CC-9 (1); the CLI shim (`Cmd::Claude`, `Cmd::Opencode`, `super::super::claude`, `super::super::opencode`) is CC-10 (1). Only a test line with a literal and nothing else stays (`stays: test`), as does each non-Rust fixture line under `tests/`.
- Comment sites: a line that starts with `//` (doc or plain comment) is `stays: comment`.
- Code sites: one concern each, by K§6.1.2, and the node per concern of the planning notes, with these calls (author, review round 1):
  - Hook event names. The hooks read and write Claude Code's event names, which are the common form that concern 2 makes. An event-name check on hook input is 2, a rendered `hookSpecificOutput` is 3. Both go to CC-6, which owns the hook crates' event handling (`zygo-hooks`, and the guard and work hooks of `zygo-work`); CC-3 owns only the conversion in `zygo-core` (`hook.rs`, `hook_opencode.rs`). The event registration in the Claude Code adapter (`claude_code.rs:495`-`:527`) is 2 and goes to CC-9 with the adapter.
  - Adapters (`crates/zygo-launch/src/claude_code.rs`, `opencode.rs`, `assets/opencode-plugin.js`) go to CC-9; each line takes the concern it serves: the program name, the client id, the adapter type, `agent()`, the install hint and the config folder names and managed-settings paths are 1; event registration is 2; the subagent model variable is 4; the `mcp__zygo__` tool names are 5; `Plugins` and `CLAUDE_PLUGIN_ROOT` are 6; the argument walk, `has_session_arg` and the bare mode (`CLAUDE_CODE_SIMPLE`) are 7; the gateway hint header variable and the passthrough credential variables are 9; a set environment row is 11.
  - Adapter selection outside the adapters (`cmd/claude.rs:4`, `:21`; `cmd/opencode.rs:5`, `:21`; `cmd/ws.rs:15`, `:18`, `:407`, `:413`, `:418`; `cmd/member.rs:17`, `:19`, `:662`, `:678`) is 1 and goes to CC-9, which adds `adapter_for`.
  - A default `Agent::ClaudeCode` that becomes `Agent::DEFAULT` is 1 everywhere (`session_view.rs:61`, `cmd/hook.rs:106`, `cmd/loadout.rs:330`).
  - The resume command and the `CLAUDE_CONFIG_DIR` saved for resume (AU61) are 8 everywhere (`cmd/member.rs:107`, `:108`, `:573`, `:645`, `:664`, `:691`-`:693`; `cmd/ws.rs:651`).
  - The dispatch from a member record to the CLI shim's `launch_opts` (`cmd/ws.rs:612`, `:613`) is 10 and stays with CC-10.
  - The CLI enum and its dispatch (`cmd/mod.rs:12`, `:29`, `:205`, `:279`, `:281`, `:609`, `:610`) are 1 (program name and CLI verb), CC-10. The review gave "(10)"; that is read as the node CC-10, because concern 10 (compaction) does not apply to these lines.
  - The `Agent` variant declarations (`crates/zygo-core/src/lib.rs:39`, `:41`) are 1, CC-3, which moves them into `client/mod.rs`; CC-14 then makes them private. The `PaneKind` variant declarations (`crates/zygo-workspace/src/state.rs:243`, `:245`) are 8 (the pane-kind id of K§6.1.2 concern 8, as their serde names at `:242`, `:244`), CC-8, whose contract replaces `PaneKind`.
  - `zygo_workspace::args::{occurrences, flags, has_session_arg}` keep their signatures in CC-7 (recorded in the CC-7 contract), so the CC-9 adapter rows that call them need no CC-7 edit.

Files added to a node's **Files** line in review round 1: CC-4 `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/redact_stage.rs`; CC-5 `crates/zygo-proxy/src/window.rs`; CC-6 `crates/zygo-hooks/src/role_guard.rs`, `crates/zygo-hooks/src/trim.rs`, `crates/zygo-work/src/guards.rs`, `crates/zygo-work/src/hooks.rs`; CC-9 `crates/zygo-plugins/src/check.rs`, `crates/zygo/src/cmd/claude.rs`, `crates/zygo/src/cmd/opencode.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/src/cmd/ws.rs`; CC-10 `crates/zygo/src/cmd/mcp.rs`; CC-13 the `#[cfg(test)]` items of `crates/zygo-hooks/src/prompt.rs`, `crates/zygo-mcp/src/tools/work.rs`, `crates/zygo/src/cmd/claude.rs`, `compact.rs`, `loadout.rs`, `mail.rs`, `member.rs`, `opencode.rs`, `verbs.rs`, `ws.rs`. In the first run: CC-8 `crates/zygo-config/src/repo_def/loadout.rs`, `crates/zygo/src/cmd/workspace_approve.rs`.

Phase 3 client features (for CC-12), at the base:

- Output shaping: `crates/zygo-results/src/shape/` (`mod.rs`, `clean.rs`, `command.rs`, `dedup.rs`, `kind_view.rs`, `runner.rs`). Tests: `crates/zygo-results/tests/shape_*.rs` (9 files), `crates/zygo-hooks/tests/shaping.rs`, `shaping_hook.rs`, `shaping_off.rs`, `shaping_fixtures.rs`, `shaping_latency.rs`, and the golden folders `crates/zygo-hooks/tests/golden/shape_off/claude-code/` (5 files) and `.../opencode/` (1 file).
- Mail wake and delivery: `crates/zygo-workspace/src/mail/` (`mod.rs`, `names.rs`, `store.rs`, `wake.rs`), `crates/zygo-hooks/src/mail.rs`, the delivery sites `crates/zygo-hooks/src/workspace.rs:335` and `:749`-`:786`, `crates/zygo-mcp/src/tools/mail.rs`, `crates/zygo/src/cmd/mail.rs`, `crates/zygo-mail-sync/src/` (16 files). Tests: `crates/zygo-workspace/tests/mail_*.rs` (4 files), `crates/zygo-hooks/tests/mail_sync_text.rs`, `mail_sync_trigger.rs`, `crates/zygo-mcp/tests/mail_tools.rs`, `mail_remote.rs`, `crates/zygo/tests/mail_*.rs` (5 files), `workspace_mail.rs`, `crates/zygo-mail-sync/tests/` (15 entries).
- Run-time status line: `crates/zygo-hooks/src/statusline/` (`mod.rs`, `data.rs`, `mail.rs`, `phase.rs`, `render.rs`, `width.rs`), `crates/zygo/src/cmd/statusline.rs`, `status.rs`. Tests: `crates/zygo-hooks/tests/statusline_*.rs` (5 files), `crates/zygo/tests/statusline_cmd.rs`, `statusline_git.rs`.

## Round 1 review

Review of PR #251 at `bbd8e77c`; coordinator decisions 2026-11-27. Each finding is fixed in `b58f48ef` (`docs(work): CC-1 round 1 F1-F8`).

- F1, F2, F3 (missing sites): fixed. The search now also finds client environment names (`CLAUDE*`, `OPENCODE_*`, `CODEX_*`), the `x-claude-code-` headers, the hook event names (unquoted for the client-specific names, quoted for `Stop`, `StopFailure` and `Notification`, which are also Rust names at the base, for example `enum Stop` in `crates/zygo-config/src/repo_def/read.rs:188`), `mcp__`, the `compact_tool` function of the proxy, the variant and adapter type names, `Cmd::Claude` and `Cmd::Opencode`, the module paths `claude::`, `opencode::` and `claude_code::`, `LOADOUT_KINDS`, `CLAUDE_ONLY`, `AGENT_ONLY`, a negated local client flag (`!claude`), the id `claude-code` inside any literal, the display names as whole literals, the `[claude-code]` and `[opencode]` tables of `agents.toml` and its `include_str!`. The pathspec adds `crates/*/tests/**`, `crates/zygo-config/data/agents.toml` and `crates/zygo-launch/assets/opencode-plugin.js`. Result: 3011 rows in 198 files (245 in 44 before). Each site the review named has a row. Where the review named a line range, the rows are the lines of the range that the search finds: `crates/zygo-launch/src/env.rs:24`-`:32` has no row for `:28` (`ZYGO_TEAM_TOKEN`, not a client name); `crates/zygo-config/src/builtin.rs:50`-`:55` is the row `:51` (the `include_str!` of `agents.toml`); `crates/zygo-proxy/src/window.rs:95`-`:99` is the rows `:95` and `:97`; `crates/zygo-hooks/src/session_start.rs:42`-`:47` is the rows `:42` and `:46`. Files added to the Files lines: listed under `## Observed`.
- F4, F5 (test rows): fixed. A test row with an `Agent` variant path or import, or a bare variant use after such an import (`crates/zygo/src/cmd/member.rs:1796`-`:1803`), is CC-13 (1178 rows). Other non-literal test rows go to the node that changes the name: `PaneKind` to CC-8, `Plugins` and the adapter type to CC-9, the CLI shim to CC-10. Only literal-only test rows stay. The CC-13 Acceptance now also fails on an `Agent::{…}` or `Agent::*` import and on a bare variant used as a tuple item, argument or match arm; at `b324e1dc` the bare-use grep finds only the lines named in the CC-13 Acceptance text. The CC-13 Files line names the ten `src` files whose `#[cfg(test)]` items hold a CC-13 row.
- F6 (acceptance): fixed. The Acceptance now (a) needs both sides non-empty, (b) fails on a duplicate `file:line` (no `sort -u`; `uniq -d` is empty), (c) fails on any line that starts with `| crates/` and is not a well-formed row: concern 1-11 with a node CC-3 to CC-15, or `—` with `stays: test`, `stays: comment` or `stays: exception list (CC-15, K§6.1.4)`, and (d) uses the widened search. Evidence, with the Acceptance run with the file path replaced: the reviewer's `bad.md` fails at the well-formedness compare (`cmp all ok`), and so does the current file with the same two-row corruption and the current file with `—` on a CC row; the current file with a duplicated row fails at `uniq -d`; the current file with a row removed fails at `cmp grep rows`. The real file passes (exit 0). Completeness is bounded by the search patterns; they are listed under `## Observed`.
- F7 (consistent concerns): fixed. A default that becomes `Agent::DEFAULT` is 1 (`crates/zygo/src/cmd/loadout.rs:330` changed from 8); the resume command and the saved `CLAUDE_CONFIG_DIR` (AU61) are 8 (`crates/zygo/src/cmd/member.rs:691`-`:693` changed from 1); `crates/zygo/src/cmd/ws.rs:612`, `:613` are 10 (changed from 8).
- F8 (CC-7 and the adapters): fixed. The CC-7 Produces line and planning notes record that `zygo_workspace::args::{occurrences, flags, has_session_arg}` keep their signatures, so the CC-9 adapter rows that call them need no CC-7 edit.

Author's calls in round 1 (each is also under `## Observed`):

- `crates/zygo/src/cmd/mod.rs:205`, `:279`, `:281`, `:609`, `:610`: the review wrote "(10)". These lines are the CLI enum and its dispatch, not compaction, so the concern is 1 and the node is CC-10.
- `crates/zygo-workspace/src/state.rs:243`, `:245`: the review wrote "(1, CC-3/CC-14 by the contracts)". These are `PaneKind` variants, whose serde names (`:242`, `:244`) are 8 and which the CC-8 contract replaces, so they are 8 and CC-8. `crates/zygo-core/src/lib.rs:39`, `:41` are 1 and CC-3, which moves them; CC-14 then makes them private.
- Hook event names in `zygo-hooks` and in the guard and work hooks of `zygo-work` go to CC-6 (input checks 2, rendered output 3); only the `zygo-core` conversion is CC-3.
- Each non-Rust fixture line that the search finds under `crates/*/tests/` (JSON and golden files) is `stays: test`.

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

Review round 1 (2026-11-27): the search of **Acceptance** was widened (see `## Observed`), so the table under `## Observed` supersedes the Assignment table above and the expected counts; the counts are under `## Observed`.

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
