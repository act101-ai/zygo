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

Written when the node runs.

## Test logic edits

None.

## Plan

Written when the node starts.
