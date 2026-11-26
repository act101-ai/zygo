#!/usr/bin/env python3
"""Re-scrub and privacy check of the committed Codex fixtures (node PC-11).

Usage:
  python3 -I scripts/premises/rescrub_codex.py --check
      -> "rescrub: ok (<n> files)", exit 0; else one "rescrub: <rel path>: <class>" line per finding,
         a last line "rescrub: <k> files to fix", exit 1.
  python3 -I scripts/premises/rescrub_codex.py --write
      -> rewrites each file whose scrub.py output differs from its text, "rescrub: wrote <k> files", exit 0.

Every regular file under fixtures/codex is read, sorted. VERBATIM files (the verbatim schema,
pinned by sha256 in scripts/check-premises.sh) are never scrubbed or written, but are still
scanned for LEAKS.
"""
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SCRUB = os.path.join(HERE, "scrub.py")
ROOT = os.path.join(os.path.dirname(os.path.dirname(HERE)), "fixtures", "codex")
VERBATIM = ("config.schema.json",)
LEAKS = {
    "os key": re.compile(r'\\?"platform(?:Os|Family)\\?"\s*:\s*\\?"(?!\(os omitted\))'),
    "login shell tag": re.compile(r"(?:<|\\u003[cC]|&lt;)shell(?:>|\\u003[eE]|&gt;)(?!\(omitted\))"),
    "macos path": re.compile(r"/private/(?:tmp|var)\b|/var/folders/"),
    "item id": re.compile(r"(?:msg|fco|ctco)_[0-9a-f]{8}-"),
    "macos sandbox": re.compile(r"(?i)seatbelt"),
    "marketplace os": re.compile(r"marketplace-(?:macos|linux|windows)-"),
    "login shell command": re.compile(r"/bin/(?:zsh|bash|fish|sh|dash|ksh|tcsh|csh) -lc"),
    "macos env": re.compile(r"__CF[A-Za-z_]"),
}


def leaks(text: str) -> list:
    return [k for k, p in LEAKS.items() if p.search(text)]


def scrubbed(path: str, run: str) -> str:
    return subprocess.run([sys.executable, "-I", SCRUB, run, path], check=True,
                          capture_output=True, text=True).stdout


def files() -> list:
    out = []
    for d, _, names in os.walk(ROOT):
        for n in names:
            p = os.path.join(d, n)
            if os.path.isfile(p) and not os.path.islink(p):
                out.append(os.path.relpath(p, ROOT))
    return sorted(out)


def main(argv: list) -> int:
    if argv[1:] not in (["--check"], ["--write"]):
        sys.stderr.write("usage: rescrub_codex.py --check | --write\n")
        return 2
    write = argv[1] == "--write"
    rels = files()
    fix = 0
    wrote = 0
    with tempfile.TemporaryDirectory() as run:
        for rel in rels:
            p = os.path.join(ROOT, rel)
            with open(p, encoding="utf-8", errors="replace") as f:
                text = f.read()
            found = leaks(text)
            if rel not in VERBATIM:
                new = scrubbed(p, run)
                if new != text:
                    if write:
                        with open(p, "w", encoding="utf-8") as f:
                            f.write(new)
                        wrote += 1
                        continue
                    found.append("scrub changes it")
            if found and not write:
                for c in found:
                    print("rescrub: %s: %s" % (rel, c))
                fix += 1
    if write:
        print("rescrub: wrote %d files" % wrote)
        return 0
    if fix:
        print("rescrub: %d files to fix" % fix)
        return 1
    print("rescrub: ok (%d files)" % len(rels))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
