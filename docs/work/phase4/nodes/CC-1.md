# CC-1 — Client-branch inventory at the P4-6 base commit

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.9 (last paragraph), K§1 item 2 (the grep), K§6.1.2 (concerns 1-11), K§6.1.4 (the names)
**Files:** `docs/work/phase4/nodes/CC-1.md` (its `## Observed` section only)
**Consumes:** `phase_3_in_phase_4` (CC-0)
**Produces:** `base` (the line `Base: <40-hex commit>` under `## Observed`: the CC-0 merge commit), `inventory` (the table under `## Observed`, one row per site: `| <file> | <line> | <concern 1-11 or —> | <CC-n or stays: reason> |`, with no backtick in a row)
**Acceptance:** `T=$(mktemp -d) && B=$(sed -n 's/^Base: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-1.md) && test -n "$B" && git fetch origin && git merge-base --is-ancestor origin/main "$B" && git merge-base --is-ancestor "$B" origin/phase-4 && git grep -lE 'Agent::(ClaudeCode|OpenCode)|PaneKind::(ClaudeCode|OpenCode)|Plugins::|"(claude-code|claude|opencode|\.claude|\.opencode|CLAUDE_CONFIG_DIR|OPENCODE_CONFIG_DIR)"' "$B" -- 'crates/*/src/**' | sed "s/^$B://" | sort -u > "$T/grep" && grep -E '^\| crates/[^ ]+ \| [0-9]+ \| ([0-9]+|—) \| (CC-[0-9]+|stays: [^|]+) \|$' docs/work/phase4/nodes/CC-1.md | awk -F'|' '{gsub(/ /,"",$2); print $2}' | sort -u > "$T/rows" && cmp "$T/grep" "$T/rows"` → every command exits 0: the base is on `phase-4` and holds `main`, and the set of files that the grep finds at the base equals the set of files in the table, each row naming a CC node or `stays: <reason>`

## Planning notes

- Read-only on the code. The only write is this file's `## Observed` section (operator brief for P4-5, 2026-11-25).
- The grep pattern is the K§1 item 2 grep made exact (author): a variant path of `Agent`, `PaneKind` or `Plugins`, or a string literal whose whole value is a client id, program, CLI verb, config folder or config-folder variable. At `c21b32e0` it finds 37 files; at `b7e9d45d` (phase 3 head) 38 (`crates/zygo-usage/src/savings.rs:77` is new). These counts show the pattern; they are not the work list.
- One row per site (a line). A site in a `#[cfg(test)]` item or a doc comment is `stays: test (K-C7)` or `stays: comment`, unless the move of its file changes it.
- Node per concern (author): concerns 1 and 2, `Dialect` and the minimum version → CC-3; 9 and the per-agent usage id → CC-4; 5 → CC-5; 3 and 4 → CC-6; 7 → CC-7; 8 → CC-8; 6, 11 and the adapters → CC-9; 10 and the CLI sites → CC-10; a literal that equals a registered name but is not a client branch (for example the attribution guard at `crates/zygo-work/src/guards.rs:1047`, `:1059` at `c21b32e0`) → CC-14; a site inside the descriptor or adapter folders → `stays: contract`.
- A row that names a node whose **Files** line does not list that file: add the file to that node's Files line in the same commit, then run `work_graph.py check` (two nodes with no path between them must not share a file).
- Under the table, list each phase 3 client feature for CC-12 (output shaping, mail wake and delivery, the run-time status line), each with its files and tests at the base.

## Observed

Written when the node runs.

## Plan

Written when the node starts.
