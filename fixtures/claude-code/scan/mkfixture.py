"""Write the root-option list of a Claude Code binary (FIX-31): fixtures/claude-code/options-<version>.txt.

Usage: python3 -I mkfixture.py <claude code binary> <output file>

The script reads the binary as bytes and runs nothing. It finds the commander chain of the root command between its
two anchors, `.name("claude").description(` and `us("run_main_options_built")`, and lists:
- each `.option("…")`, `new Ns("…")` and `.requiredOption("…")` in the chain;
- the option `new Ns(ela, …)`, where the binary defines `var ela="--ignore-persisted-trust"`;
- `.helpOption("-h, --help"` and `"-v, --version"` (the `.version(…)` call), searched from 2000 bytes before the chain.
An option is hidden when `.hideHelp()` follows it before the next option. The output is sorted by byte offset. Every
line of the output, the header included, is written by this script; nobody edits the output by hand.
"""
import os
import re
import sys

binary, out = sys.argv[1], sys.argv[2]
data = open(binary, "rb").read()
start = data.find(b'.name("claude").description(')
end = data.find(b'us("run_main_options_built")', start)
assert start != -1 and end != -1, "chain anchors not found"
text = data[start:end].decode("utf-8", "replace")
ela = re.search(rb'var ela="(-[^"]+)"', data)
assert ela is not None, "ela not found"
rows = []
pat = re.compile(r'(?:\.option|new Ns|\.requiredOption)\((?:(["\'`])(-[^"\'`]*)\1|(ela),)')
hits = list(pat.finditer(text))
for i, m in enumerate(hits):
    stop = hits[i + 1].start() if i + 1 < len(hits) else len(text)
    flags = m.group(2) if m.group(2) else ela.group(1).decode()
    rows.append((start + m.start(), "H" if ".hideHelp()" in text[m.start():stop] else "-", flags))
for needle, flags in [(b'.helpOption("-h, --help"', "-h, --help"), (b'"-v, --version","Output the version number"', "-v, --version")]:
    at = data.find(needle, start - 2000, end)
    assert at != -1, needle
    rows.append((at, "-", flags))
rows.sort()
version = os.path.basename(binary)
with open(out, "w", encoding="utf-8") as f:
    f.write(f"# Every option of the root command of the Claude Code binary {version}.\n")
    f.write("# Written by fixtures/claude-code/scan/mkfixture.py from the binary (static read, no run); do not edit by hand.\n")
    f.write(f"# Chain: byte offsets {start}-{end}, from .name(\"claude\") to us(\"run_main_options_built\"); {len(rows)} options.\n")
    f.write("# Columns, tab-separated: byte offset, H = hidden (.hideHelp()) or -, the flags text.\n")
    f.write("# Value mark: <x> required, <x...> variadic, [x] optional, none = boolean.\n")
    for at, hidden, flags in rows:
        f.write(f"{at}\t{hidden}\t{flags}\n")
print(len(rows))
