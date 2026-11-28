#!/usr/bin/env python3
"""CC-1 acceptance: the client-branch inventory in a CC-1 node file is complete and well formed.

Usage: python3 docs/work/phase4/cc1_check.py <CC-1 node file>   (run inside the repository)

Hermetic: every source line is read at the commit of the file's `Base:` line (`git grep`, `git show`);
nothing is fetched. The owners' node files (`CC-<n>.md`, for their **Files** lines) are read from the folder
of the given file. Exit 0 and one `ok:` line when every check passes; otherwise the first failing check is
printed as `FAIL <step>: ...` and the exit code is 1.
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
# The search (git grep -E, one -e each). Completeness of the inventory is bounded by these patterns.
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
# Owners of rows and the concerns each takes (its contract; K§6.1.2 concerns 1-11).
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
# Symbols that a CC node changes, deletes or moves: (node, concern, scope, file glob, regex). The first entry that
# matches decides. Scope `test`: a test-code line, matched with its string literals and trailing comment removed;
# scope `comment`: a `//` line, matched whole; `any`: both. A test line that matches is that node's row, and one
# that matches nothing is `stays: test`; a `stays: comment` line must match nothing.
SYMBOLS = [
    ("CC-13", "1", "test", "*", r'Agent::(ClaudeCode|OpenCode|\{|\*)'),
    ("CC-13", "1", "test", "crates/zygo/src/cmd/member.rs", r'(^|[^A-Za-z0-9_])(ClaudeCode|OpenCode)([^A-Za-z0-9_]|$)'),
    ("CC-8", "8", "any", "*", r'PaneKind::(ClaudeCode|OpenCode)'),
    ("CC-8", "8", "any", "*", r'(^|[^A-Za-z0-9_])(LOADOUT_KINDS|AGENT_ONLY|CLAUDE_ONLY|pool_member)([^A-Za-z0-9_]|$)'),
    ("CC-10", "1", "any", "*", r'Cmd::(Claude|Opencode)|super::super::(claude|opencode)::|(claude|opencode)::launch_opts|cmd/(claude|opencode)\.rs|(^|[^A-Za-z0-9_])(LaunchOpenCode|launch_open_code_of)([^A-Za-z0-9_]|$)'),
    ("CC-9", "6", "any", "*", r'(^|[^A-Za-z0-9_])(Plugins|claude_config_dir)([^A-Za-z0-9_]|$)|opencode-plugin\.js'),
    ("CC-9", "7", "any", "*", r'(^|[^A-Za-z0-9_])simple_mode([^A-Za-z0-9_]|$)'),
    ("CC-9", "8", "any", "*", r'(^|[^A-Za-z0-9_])(saved_config_dir|prepare_resume)([^A-Za-z0-9_]|$)'),
    ("CC-9", "1", "any", "*", r'zygo_launch::(claude_code|opencode)|(^|[^A-Za-z0-9_])(ClaudeCode|for_zygo)([^A-Za-z0-9_]|$)|claude_code\.rs'),
    ("CC-9", "1", "test", "*", r'(^|[^A-Za-z0-9_])OpenCode([^A-Za-z0-9_]|$)'),
    ("CC-9", "1", "comment", "*", r'OpenCode::|`OpenCode`'),
    ("CC-4", "9", "any", "*", r'(^|[^A-Za-z0-9_])(CLAUDE_AI|ANTHROPIC_VERSION|skipped_spans)([^A-Za-z0-9_]|$)'),
    ("CC-5", "5", "any", "crates/zygo-proxy/*", r'(^|[^A-Za-z0-9_])compact_tool([^A-Za-z0-9_]|$)'),
    ("CC-7", "7", "any", "*", r'(^|[^A-Za-z0-9_])(opencode_subcommand|opencode_takes_value|OPENCODE_COMMAND_BOOLEANS|OPENCODE_GLOBAL_BOOLEANS|CLAUDE_CODE|OPENCODE)([^A-Za-z0-9_]|$)'),
    ("CC-3", "2", "any", "*", r'hook_opencode'),
    ("CC-3", "1", "any", "*", r'agents\.toml'),
]
GUARD_FILE, GUARD_ITEM = "crates/zygo-work/src/guards.rs", "claude_attribution"
G = "stays: exception list (CC-15, K§6.1.4)"
STAYS = {"stays: test", "stays: comment", G}
ROW = re.compile(r"^\| (crates/[A-Za-z0-9_./-]+) \| ([1-9][0-9]*) \| ([1-9][0-9]?|—) \| (CC-[0-9]+|stays: [^|]+?) \|$")
PATH_CELL = re.compile(r"^\s*\|\s*[A-Za-z0-9_.-]+/[^|\s]*\s*\|")
TABLE_HEAD = "| File | Line | Concern | Node |"
# Test region of a src file: from the inline `mod <name> {` that directly follows a column-0 test attribute to the
# end of the file. The CC-11 and CC-13 awk programs use the same two expressions.
TEST_ATTR = re.compile(r'^#\[cfg\((test|all\(test, [^"]*\)|all\([^"]*, test\))\)\]')
TEST_MOD = re.compile(r"^(pub(\([^)]*\))? )?mod [A-Za-z_][A-Za-z0-9_]* \{")
STRING = re.compile(r'b?r(#*)"[^"]*"\1|"(\\.|[^"\\])*"')


class Fail(Exception):
    pass


def fail(step, msg):
    raise Fail(f"FAIL {step}: {msg}")


def git(*args):
    r = subprocess.run(["git", *args], capture_output=True)
    return r.returncode, r.stdout


def test_code(text):
    """A test line as the symbol rule reads it: each string literal replaced by its inline format arguments
    (`"{AGENT_ONLY}"` keeps `AGENT_ONLY`), trailing `//` comment cut."""
    code = STRING.sub(lambda m: '"" ' + " ".join(re.findall(r"(?<![$])\{([A-Za-z_][A-Za-z0-9_]*)", m.group(0))), text)
    return code.split(" //")[0] if " //" in code else code


def symbol_for(f, text, comment):
    """The first SYMBOLS entry that matches the line, as (node, concern), or None."""
    subject = text if comment else test_code(text)
    for node, concern, scope, glob, rx in SYMBOLS:
        if scope == ("test" if comment else "comment"):
            continue
        if glob != "*" and not fnmatch.fnmatch(f, glob):
            continue
        if re.search(rx, subject):
            return node, concern
    return None


def test_region_start(lines):
    """First line of the test region (the `mod` line after a column-0 test attribute), or None."""
    for i in range(len(lines) - 1):
        if TEST_ATTR.match(lines[i]) and TEST_MOD.match(lines[i + 1]):
            return i + 2
    return None


def item_range(lines, name):
    """Line range of `fn <name>` (braces counted outside strings, chars and comments)."""
    start = next((i for i, l in enumerate(lines) if re.search(r"\bfn " + name + r"\(", l)), None)
    if start is None:
        return None
    depth, seen = 0, False
    for i in range(start, len(lines)):
        code = re.sub(r"//.*$", "", STRING.sub('""', re.sub(r"'(\\.|[^\\'])'", "' '", lines[i])))
        for ch in code:
            if ch == "{":
                depth, seen = depth + 1, True
            elif ch == "}":
                depth -= 1
                if seen and depth == 0:
                    return start + 1, i + 1
    return None


def check(path):
    try:
        text = open(path, encoding="utf-8").read()
    except OSError as e:
        fail("read", f"{path}: {e}")
    lines = text.split("\n")
    nodes_dir = os.path.dirname(os.path.abspath(path))

    # 1. base
    bases = [m.group(1) for m in (re.match(r"^Base: ([0-9a-f]{40})$", l) for l in lines) if m]
    if len(bases) != 1:
        fail("base", f"expected one 'Base: <40-hex>' line, found {len(bases)}")
    base = bases[0]
    if git("cat-file", "-e", base + "^{commit}")[0] != 0:
        fail("base", f"{base} is not a commit here")
    if git("merge-base", "--is-ancestor", PHASE3, base)[0] != 0:
        fail("base", f"{PHASE3} (phase 3) is not an ancestor of {base}")

    # 2. the lists in the node file equal the check's
    doc_pats = [m.group(1) for m in (re.match(r"^- Pattern: `(.*)`$", l) for l in lines) if m]
    if doc_pats != PATTERNS:
        fail("patterns", f"the node file lists {len(doc_pats)} patterns; they differ from the {len(PATTERNS)} of the check")
    doc_sym = [m.groups() for m in (re.match(r"^- Symbol: (CC-[0-9]+) ([0-9]+) (test|comment|any) (\S+) `(.*)`$", l) for l in lines) if m]
    if doc_sym != SYMBOLS:
        fail("symbols", f"the node file lists {len(doc_sym)} symbol entries; they differ from the {len(SYMBOLS)} of the check")
    doc_con = {}
    for l in lines:
        m = re.match(r"^\| (CC-[0-9]+) \| ([0-9]+(?:, [0-9]+)*) \|$", l)
        if m:
            if m.group(1) in doc_con:
                fail("concern-table", f"two rows for {m.group(1)}")
            doc_con[m.group(1)] = {int(x) for x in m.group(2).split(", ")}
    if doc_con != CONCERNS:
        fail("concern-table", "the 'Concerns per node' table differs from the one of the check")

    # 3. the search at the base
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

    # 4. row form: every line of the Observed table, and every row-like line anywhere, is a row at column 0
    heads = [i for i, l in enumerate(lines) if l == TABLE_HEAD]
    if len(heads) != 1 or heads[0] + 1 >= len(lines) or lines[heads[0] + 1] != "|---|---|---|---|":
        fail("row-form", "expected one inventory table header and its separator")
    table = set()
    k = heads[0] + 2
    while k < len(lines) and lines[k].strip():
        if not ROW.match(lines[k]):
            fail("row-form", f"line {k + 1} of the inventory table is not a row: {lines[k][:120]!r}")
        table.add(k)
        k += 1
    rows = []
    for i, l in enumerate(lines):
        if i in table:
            m = ROW.match(l)
            rows.append((i + 1, m.group(1), int(m.group(2)), m.group(3), m.group(4)))
        elif PATH_CELL.match(l) or ROW.match(l.strip()):
            fail("row-form", f"line {i + 1} is a row outside the inventory table or not at column 0: {l[:120]!r}")
    if not rows:
        fail("row-form", "no row")

    # 5. owner and concern values
    for i, f, n, c, node in rows:
        if node in STAYS:
            if c != "—":
                fail("node", f"line {i}: a stays row takes concern —, not {c}")
        elif node in CONCERNS:
            if c == "—" or not 1 <= int(c) <= 11:
                fail("node", f"line {i}: {node} needs a concern 1-11, not {c}")
        else:
            fail("node", f"line {i}: owner {node!r} is not one of {sorted(CONCERNS)} or a stays reason")

    # 6. no duplicate file:line
    seen = {}
    for i, f, n, c, node in rows:
        if (f, n) in seen:
            fail("duplicate", f"lines {seen[(f, n)]} and {i} are both {f}:{n}")
        seen[(f, n)] = i

    # 7. rows equal the search
    if set(seen) != found:
        miss, extra = sorted(found - set(seen)), sorted(set(seen) - found)
        fail("rows-vs-search", f"{len(miss)} sites without a row (first {miss[:3]}), {len(extra)} rows the search does not find (first {extra[:3]})")

    # 8. the row's file is on the owner's Files line (a folder entry covers the files under it)
    files = {}
    for node in sorted({r[4] for r in rows if r[4] in CONCERNS}):
        try:
            nt = open(os.path.join(nodes_dir, node + ".md"), encoding="utf-8").read().split("\n")
        except OSError as e:
            fail("files", f"cannot read {node}.md next to {path}: {e}")
        fl = [l for l in nt if l.startswith("**Files:**")]
        if len(fl) != 1:
            fail("files", f"{node}.md has {len(fl)} Files lines")
        files[node] = re.findall(r"`([^`]+)`", fl[0])
    for i, f, n, c, node in rows:
        if node in CONCERNS and not any(e == f or (e.endswith("/") and (f.startswith(e) or fnmatch.fnmatch(f, e + "*"))) for e in files[node]):
            fail("files", f"line {i}: {f} is not on the Files line of {node}")

    # 9. the concern is one the owner takes
    for i, f, n, c, node in rows:
        if node in CONCERNS and int(c) not in CONCERNS[node]:
            fail("concern", f"line {i}: {node} does not take concern {c} (takes {sorted(CONCERNS[node])})")

    cache = {}

    def src(f):
        if f not in cache:
            rc, out = git("show", f"{base}:{f}")
            if rc != 0:
                fail("source", f"cannot read {f} at {base}")
            cache[f] = out.decode("utf-8", "replace").split("\n")
        return cache[f]

    def is_comment(f, n):
        return src(f)[n - 1].lstrip().startswith("//")

    # 10. a stays: comment line is a // comment at the base
    for i, f, n, c, node in rows:
        if node == "stays: comment" and not is_comment(f, n):
            fail("comment", f"line {i}: {f}:{n} is not a // comment at the base")

    # 11. the exception list holds only the approved item and its calls
    guard = item_range(src(GUARD_FILE), GUARD_ITEM)
    if guard is None:
        fail("exception", f"no fn {GUARD_ITEM} in {GUARD_FILE} at the base")
    for i, f, n, c, node in rows:
        if node == G and not (f == GUARD_FILE and (guard[0] <= n <= guard[1] or GUARD_ITEM + "(" in src(f)[n - 1])):
            fail("exception", f"line {i}: {f}:{n} is neither inside {GUARD_ITEM} ({GUARD_FILE}:{guard[0]}-{guard[1]}) nor a call of it")

    # 12. listed test ranges: one per file, each the file's test region at the base
    ranges = {}
    for l in lines:
        m = re.match(r"^- Test range: (crates/[A-Za-z0-9_./-]+) ([0-9]+)-([0-9]+)$", l)
        if m:
            if m.group(1) in ranges:
                fail("test-ranges", f"two Test range lines for {m.group(1)}")
            ranges[m.group(1)] = (int(m.group(2)), int(m.group(3)))
    for f, (a, b) in ranges.items():
        s = src(f)
        start = test_region_start(s)
        last = len(s) - 1 if s and s[-1] == "" else len(s)
        if start != a or b != last:
            fail("test-ranges", f"{f}: listed {a}-{b}; at the base the test region is {start}-{last}")
    for i, f, n, c, node in rows:
        if "/src/" in f and f.endswith(".rs") and f not in ranges:
            start = test_region_start(src(f))
            if start is not None and n >= start:
                fail("test-ranges", f"line {i}: {f}:{n} is in the test region from {start}, but {f} has no Test range line")

    def is_test(f, n):
        if re.match(r"^crates/[^/]+/tests/", f):
            return True
        return f in ranges and ranges[f][0] <= n <= ranges[f][1]

    # 13. a stays: test or CC-13 row is test code
    for i, f, n, c, node in rows:
        if node in ("stays: test", "CC-13") and not is_test(f, n):
            fail("test", f"line {i}: {f}:{n} ({node}) is not under crates/*/tests/ or in a listed test range")

    # 14. the symbol rule: a test line goes to the node of the first symbol it names, else stays: test;
    #     a stays: comment line names no symbol
    for i, f, n, c, node in rows:
        t = src(f)[n - 1]
        if is_comment(f, n):
            hit = symbol_for(f, t, True)
            if node == "stays: comment" and hit:
                fail("symbol-rule", f"line {i}: {f}:{n} names a symbol of {hit[0]}; it is {hit[0]}'s row, not stays: comment")
        elif is_test(f, n) and f.endswith(".rs"):
            hit = symbol_for(f, t, False)
            want = hit[0] if hit else "stays: test"
            if node != want:
                fail("symbol-rule", f"line {i}: {f}:{n} is test code; the symbol rule gives {want}, the row gives {node}")

    return f"ok: base {base[:8]}, {len(rows)} rows = {len(found)} sites, {len({r[1] for r in rows})} files"


def main():
    if len(sys.argv) != 2:
        print("FAIL usage: cc1_check.py <CC-1 node file>")
        sys.exit(1)
    try:
        print(check(sys.argv[1]))
    except Fail as e:
        print(e)
        sys.exit(1)
    except (OSError, UnicodeDecodeError) as e:
        print(f"FAIL io: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
