#!/usr/bin/env python3
"""CC-1 acceptance: the client-branch inventory in a CC-1 node file is complete and well formed.

Usage: python3 docs/work/phase4/cc1_check.py <CC-1 node file>   (run from the repository root)

Hermetic: every source line is read at the commit of the file's `Base:` line (`git grep`, `git show`);
nothing is fetched. The node files that hold the owners' **Files** lines are read from the working tree
next to the given file (`docs/work/phase4/nodes/`).
Exit 0 when every check passes; otherwise the first failing check is printed as `FAIL <step>: ...` and the
exit code is 1.
"""
import fnmatch
import os
import re
import subprocess
import sys

PHASE3 = "b7e9d45d"
PATHSPECS = [
    "crates/*/src/**",
    "crates/*/tests/**",
    "crates/zygo-config/data/agents.toml",
    "crates/zygo-launch/assets/opencode-plugin.js",
]
# The search. Completeness of the inventory is bounded by these patterns (git grep -E, one -e each).
PATTERNS = [
    r'Agent::(ClaudeCode|OpenCode|\{|\*)',
    r'PaneKind::(ClaudeCode|OpenCode)',
    r'Plugins::',
    r'(enum |: |, |\{)Plugins([,};]| \{|$)',
    r'"(claude-code|claude|opencode|\.claude|\.opencode|CLAUDE_CONFIG_DIR|OPENCODE_CONFIG_DIR|Claude Code|OpenCode|claude-ai)"',
    r'(^|[^A-Za-z0-9_])(claude|opencode|claude_code|open_code)[a-z_]*: bool',
    r'(^|[^A-Za-z0-9_])!(claude|opencode|claude_code)([^A-Za-z0-9_]|$)',
    r'(^|[^A-Za-z0-9_])(if|&&|\|\|) (claude|opencode)([^A-Za-z0-9_.-]|$)',
    r'(^|[^A-Za-z0-9_."/:-])(claude|opencode)(\)|\.[a-z_]+\()',
    r'(^|[^A-Za-z0-9_])(CLAUDE[A-Z_]*|OPENCODE[A-Z_]*|CODEX_[A-Z_]+|ANTHROPIC_[A-Z_]+|DISABLE_LOG(IN|OUT)_COMMAND)([^A-Za-z0-9_]|$)',
    r'[A-Za-z0-9_]([Cc]laude|[Oo]pen_?[Cc]ode)|([Cc]laude|[Oo]pen_?[Cc]ode)[A-Za-z0-9_]',
    r'(^|[^A-Za-z0-9_])(ClaudeCode|OpenCode)([^A-Za-z0-9_]|$)',
    r'(^|[^A-Za-z0-9_])(Claude|Opencode)\(',
    r'(^|[^A-Za-z0-9_])(claude|opencode|claude_code)::|mod (claude|opencode|claude_code);',
    r'(^|[^A-Za-z0-9_])(LOADOUT_KINDS|AGENT_ONLY)([^A-Za-z0-9_]|$)',
    r'[xX]-[cC]laude-[cC]ode-',
    r'claude-code',
    r'\.claude-plugin',
    r'"opencode/',
    r'^\[(claude-code|opencode)\]',
    r'agents\.toml',
    r'installed_plugins\.json',
    r'(^|[^A-Za-z0-9_])(PreToolUse|PostToolUse|PostToolUseFailure|PreCompact|SessionStart|SessionEnd|SubagentStart|SubagentStop|UserPromptSubmit)([^A-Za-z0-9_]|$)',
    r'"(Stop|StopFailure|Notification)"',
    r'hookSpecificOutput',
    r'mcp__',
    r'(^|[^A-Za-z0-9_.])compact_tool\(|\(compact_tool\)|, compact_tool[,}]|window::compact_tool',
    r'(^|[^A-Za-z0-9_])let (mut )?(claude|opencode|claude_code) =',
]
# Owners of code rows and the concerns each may take (its contract; K§6.1.2 concerns 1-11).
CONCERNS = {
    "CC-3": {1, 2},
    "CC-4": {9},
    "CC-5": {5},
    "CC-6": {2, 3, 4},
    "CC-7": {7},
    "CC-8": {8},
    "CC-9": {1, 2, 3, 4, 5, 6, 7, 8, 9, 11},
    "CC-10": {1, 7, 8, 10},
    "CC-11": {1},
    "CC-13": {1},
    "CC-15": {1},
}
STAYS = {"stays: test", "stays: comment", "stays: exception list (CC-15, K§6.1.4)"}
ROW = re.compile(r"^\| (crates/[A-Za-z0-9_./-]+) \| ([1-9][0-9]*) \| ([1-9][0-9]?|—) \| (CC-[0-9]+|stays: [^|]+?) \|$")
PATH_CELL = re.compile(r"^\s*\|\s*[A-Za-z0-9_.-]+/[^|\s]*\s*\|")
TEST_ATTR = re.compile(r'^#\[cfg\((test|all\((test, [^"\]]*|[^"\]]*, test)\))\)\]')


def fail(step, msg):
    print(f"FAIL {step}: {msg}")
    sys.exit(1)


def git(*args):
    r = subprocess.run(["git", *args], capture_output=True)
    return r.returncode, r.stdout


def main():
    if len(sys.argv) != 2:
        fail("usage", "cc1_check.py <CC-1 node file>")
    path = sys.argv[1]
    text = open(path, encoding="utf-8").read()
    lines = text.split("\n")
    nodes_dir = "docs/work/phase4/nodes"

    # 1. base
    bases = [m.group(1) for m in (re.match(r"^Base: ([0-9a-f]{40})$", l) for l in lines) if m]
    if len(bases) != 1:
        fail("base", f"expected one 'Base: <40-hex>' line, found {len(bases)}")
    base = bases[0]
    if git("cat-file", "-e", base + "^{commit}")[0] != 0:
        fail("base", f"{base} is not a commit here")
    if git("merge-base", "--is-ancestor", PHASE3, base)[0] != 0:
        fail("base", f"{PHASE3} (phase 3) is not an ancestor of {base}")

    # 2. the pattern list in the node file equals the search of this check
    doc_pats = [m.group(1) for m in (re.match(r"^- Pattern: `(.*)`$", l) for l in lines) if m]
    if doc_pats != PATTERNS:
        fail("patterns", f"the node file lists {len(doc_pats)} patterns; they differ from the {len(PATTERNS)} of the check")

    # 3. the concern table in the node file equals the one of the check
    doc_con = {}
    for l in lines:
        m = re.match(r"^\| (CC-[0-9]+) \| ([0-9]+(?:, [0-9]+)*) \|$", l)
        if m:
            doc_con[m.group(1)] = {int(x) for x in m.group(2).split(", ")}
    if doc_con != CONCERNS:
        fail("concern-table", "the 'Concerns per node' table differs from the one of the check")

    # 4. the search at the base
    args = ["grep", "-nE"]
    for p in PATTERNS:
        args += ["-e", p]
    rc, out = git(*args, base, "--", *PATHSPECS)
    if rc != 0:
        fail("search", f"git grep exited {rc}")
    found = set()
    for raw in out.decode("utf-8", "replace").splitlines():
        m = re.match(r"^" + base + r":([^:]+):([0-9]+):", raw)
        if not m:
            fail("search", f"unparsed git grep line: {raw[:120]}")
        found.add((m.group(1), int(m.group(2))))
    if not found:
        fail("search", "the search finds no site")

    # 5. (a) every row-like line is a row at column 0
    rows = []
    for i, l in enumerate(lines, 1):
        if not PATH_CELL.match(l):
            continue
        m = ROW.match(l)
        if not m:
            fail("row-form", f"line {i} is not a well-formed row at column 0: {l[:120]!r}")
        rows.append((i, m.group(1), int(m.group(2)), m.group(3), m.group(4)))
    if not rows:
        fail("row-form", "no row")

    # 6. (b) concern and node values
    for i, f, n, c, node in rows:
        if node in STAYS:
            if c != "—":
                fail("node", f"line {i}: a stays row takes concern —, not {c}")
        elif node in CONCERNS:
            if c == "—" or not 1 <= int(c) <= 11:
                fail("node", f"line {i}: {node} needs a concern 1-11, not {c}")
        else:
            fail("node", f"line {i}: owner {node!r} is not one of {sorted(CONCERNS)} or a stays reason")

    # 7. no duplicate file:line
    seen = {}
    for i, f, n, c, node in rows:
        if (f, n) in seen:
            fail("duplicate", f"lines {seen[(f, n)]} and {i} are both {f}:{n}")
        seen[(f, n)] = i

    # 8. rows equal the search
    keys = set(seen)
    if keys != found:
        miss, extra = sorted(found - keys), sorted(keys - found)
        fail("rows-vs-search", f"{len(miss)} sites without a row (first {miss[:3]}), {len(extra)} rows the search does not find (first {extra[:3]})")

    # 9. (c) the row's file is on the owner's Files line
    files = {}
    for node in {r[4] for r in rows if r[4] in CONCERNS}:
        nt = open(os.path.join(nodes_dir, node + ".md"), encoding="utf-8").read().split("\n")
        fl = [l for l in nt if l.startswith("**Files:**")]
        if len(fl) != 1:
            fail("files", f"{node}.md has {len(fl)} Files lines")
        files[node] = re.findall(r"`([^`]+)`", fl[0])
    for i, f, n, c, node in rows:
        if node not in CONCERNS:
            continue
        ok = any(e == f or (e.endswith("/") and (f.startswith(e) or fnmatch.fnmatch(f, e + "*"))) for e in files[node])
        if not ok:
            fail("files", f"line {i}: {f} is not on the Files line of {node}")

    # 10. (f) the concern is one the owner takes
    for i, f, n, c, node in rows:
        if node in CONCERNS and int(c) not in CONCERNS[node]:
            fail("concern", f"line {i}: {node} does not take concern {c} (takes {sorted(CONCERNS[node])})")

    # source lines at the base
    cache = {}

    def src(f):
        if f not in cache:
            rc, out = git("show", f"{base}:{f}")
            if rc != 0:
                fail("source", f"cannot read {f} at {base}")
            cache[f] = out.decode("utf-8", "replace").split("\n")
        return cache[f]

    # 11. (d) a stays: comment line is a // comment at the base
    for i, f, n, c, node in rows:
        if node == "stays: comment" and not src(f)[n - 1].lstrip().startswith("//"):
            fail("comment", f"line {i}: {f}:{n} is not a // comment at the base")

    # 12. the listed test ranges are the test code of their files at the base
    ranges = {}
    for l in lines:
        m = re.match(r"^- Test range: (crates/[A-Za-z0-9_./-]+) ([0-9]+)-([0-9]+)$", l)
        if m:
            ranges[m.group(1)] = (int(m.group(2)), int(m.group(3)))
    for f, (a, b) in ranges.items():
        s = src(f)
        firsts = [k for k, t in enumerate(s, 1) if TEST_ATTR.match(t)]
        last = len(s) - 1 if s and s[-1] == "" else len(s)
        if not firsts or firsts[0] != a or b != last:
            fail("test-ranges", f"{f}: listed {a}-{b}; at the base the test code is {firsts[:1]}-{last}")

    # 13. (e) a stays: test row (and every CC-13 row) is test code
    def is_test(f, n):
        if re.match(r"^crates/[^/]+/tests/", f):
            return True
        return f in ranges and ranges[f][0] <= n <= ranges[f][1]

    for i, f, n, c, node in rows:
        if node in ("stays: test", "CC-13") and not is_test(f, n):
            fail("test", f"line {i}: {f}:{n} ({node}) is not under crates/*/tests/ or in a listed test range")

    # every src file with a cfg(test) site must list its range
    for i, f, n, c, node in rows:
        if "/src/" in f and f.endswith(".rs") and f not in ranges:
            if any(TEST_ATTR.match(t) for t in src(f)[: n - 1]):
                fail("test-ranges", f"line {i}: {f}:{n} follows a test attribute but {f} has no listed test range")

    print(f"ok: base {base[:8]}, {len(rows)} rows = {len(found)} sites, {len({r[1] for r in rows})} files")


main()
