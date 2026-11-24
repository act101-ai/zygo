"""List the option strings of a commander chain in a binary between two byte offsets (FIX-31 evidence).

Usage: python3 -I rootopts2.py <binary> <start> <end> [full]
Each line: byte offset, H when `.hideHelp()` follows the option before the next one, the flags text; with `full`, also
the first 260 characters after the flags (the description). The script reads the binary as bytes and runs nothing.
For Claude Code 2.1.331 the root chain is at 348994202-349021969 (see the header of ../options-2.1.331.txt).
"""
import re
import sys

path, start, end = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
full = len(sys.argv) > 4
data = open(path, "rb").read()[start:end].decode("utf-8", "replace")
pat = re.compile(r'(?:\.option|new Ns|\.requiredOption)\((["\'`])(-[^"\'`]*)\1')
hits = list(pat.finditer(data))
for i, m in enumerate(hits):
    stop = hits[i + 1].start() if i + 1 < len(hits) else len(data)
    seg = data[m.start():stop]
    hidden = "H" if ".hideHelp()" in seg else " "
    line = f"{start + m.start()} {hidden} {m.group(2)}"
    if full:
        line += " ::: " + seg[len(m.group(0)):][:260].replace("\n", " ")
    print(line)
print(f"total {len(hits)}")
