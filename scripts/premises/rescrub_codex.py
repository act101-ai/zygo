#!/usr/bin/env python3
"""Re-scrub and privacy check of the committed Codex fixtures (node PC-11), and of the committed
Claude Code and OpenCode fixtures (node PC-13).

Usage:
  python3 -I scripts/premises/rescrub_codex.py --check [codex|claude-code|opencode]
      -> "rescrub: ok (<n> files)", exit 0; else one "rescrub: <rel path>: <class>" line per finding,
         a last line "rescrub: <k> files to fix", exit 1.
  python3 -I scripts/premises/rescrub_codex.py --write [codex|claude-code|opencode]
      -> rewrites each file whose scrub.py output differs from its text, "rescrub: wrote <k> files", exit 0.

The folder argument (PC-13) selects fixtures/<folder>; the default is codex. Paths are printed
relative to fixtures/<folder>. Every regular file under that folder is read, sorted. VERBATIM files
(per folder: codex config.schema.json, claude-code settings-schema.json; the pinned upstream schemas,
never captures; opencode has none) are never scrubbed or written, but are still scanned for leaks.
The leak classes are LEAKS for codex and ENV_LEAKS (the Claude Code environment block lines and the
Stainless arch and runtime version headers) for claude-code and opencode; a JSON file whose object
repeats a key is the class "duplicate keys". A scrub.py output that differs from the text only in
JSON layout (both parse with no repeated key, same canonical form) is no change: no finding and no
write (PC-13, review round 1 F1). Each capture folder (the
first folder under fixtures/<folder>, or the root's own files) has one id map (scrub.py SCRUB_IDMAP):
an id gets the same placeholder in all its files.
--check runs scrub.py with SCRUB_FIXED=1, no user name rule (SCRUB_USER=user) and no host names, so
the result is the same on every machine. Each word of N2_FORBIDDEN_NAMES is a leak class instead (a
whole word, any case; never a rewrite); a word that is ordinary text of the folder's verbatim schema
is noted on stderr as a common word and not scanned (review round 2 G4). The output never names a word.
"""
import json
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SCRUB = os.path.join(HERE, "scrub.py")
FIXTURES = os.path.join(os.path.dirname(os.path.dirname(HERE)), "fixtures")
FOLDERS = ("codex", "claude-code", "opencode")
# Review round 1 F2: OpenCode's config-schema.json is not pinned; it is scrubbed like any capture.
VERBATIM = {"codex": ("config.schema.json",), "claude-code": ("settings-schema.json",), "opencode": ()}
# Left edge of an id or name (scrub.py B): no word character before it, or a JSON backslash escape.
B = r"(?:(?<![A-Za-z0-9_])|(?<=\\[nrtbf])|(?<=\\u[0-9A-Fa-f]{4}))"
ECX_OPEN = r"(?:<|\\u003[cC]|&lt;)environment_context(?:>|\\u003[eE]|&gt;)"
ECX_CLOSE = r"(?:<|\\u003[cC]|&lt;)\\?/environment_context"
# The scope of each rule is that of scrub.py and scripts/check-premises.sh (review round 2 G1, G3):
# the <shell> tag only in an environment_context block, a login shell only at the start of an exec
# "command" or "cmd" field, /private/tmp only as a whole path part, seatbelt only as a JSON key or value.
LEAKS = {
    "os key": re.compile(r'\\?"platform(?:Os|Family)\\?"\s*:\s*\\?"(?!\(os omitted\))'),
    "login shell tag": re.compile(ECX_OPEN + r"(?:(?!" + ECX_CLOSE + r").)*?(?:<|\\u003[cC]|&lt;)shell"
                                  r"(?:>|\\u003[eE]|&gt;)(?!\(omitted\))", re.S),
    "macos path": re.compile(r"/private/tmp(?![A-Za-z0-9_.-])|/private/var/|/var/folders/"),
    "item id": re.compile(r"(?i)(?:msg|fco|ctco)_[0-9a-f]{8}-"),
    "macos sandbox": re.compile(r'(?i)\\?"seatbelt\\?"'),
    "marketplace os": re.compile(r"marketplace-(?:macos|linux|windows)-"),
    "login shell command": re.compile(r'\\?"(?:command|cmd)\\?"\s*:\s*\\?"(?:/[^\s"\'\\/]+)*/'
                                      r"(?:zsh|bash|fish|sh|dash|ksh|mksh|tcsh|csh) -lc\b"),
    "macos env": re.compile(r"__CF"),
    "time-bearing id": re.compile(B + r"[a-z]{1,8}_(?!0{10})[0-7][0-9A-HJKMNP-TV-Z]{25}(?![0-9A-Za-z])"),
}
# PC-13: the leak classes of claude-code and opencode. The shell name and OS version lines of a Claude
# Code environment block (the forms of scrub.py ENV_SHELL, ENV_OS; review round 1 F5, F8: any case,
# with or without "- ", blanks or escaped tabs after the colon, at a line start or after a JSON "\n"
# escape that is not an escaped backslash then "n"); only the exact placeholder, at the value's end,
# passes (F6). The Stainless arch and runtime version headers (scrub.py STAINLESS_RE; F3).
ENV_PFX = r"(?:^|(?<!\\)(?:\\\\)*\\n)[ \t]*(?:-[ \t]*)?"
ENV_WS = r"(?:[ \t]|\\t)*"
ENV_END = r"(?:[\n\r\\\"]|$)"
ENV_LEAKS = {
    "shell name": re.compile(r"(?m)" + ENV_PFX + r"(?i:shell):" + ENV_WS + r"(?!\(omitted\)" + ENV_END + r")[^\s\\\"]"),
    "os version": re.compile(r"(?m)" + ENV_PFX + r"(?i:os version):" + ENV_WS + r"(?!\(os omitted\)" + ENV_END +
                             r")[^\s\\\"]"),
    "arch": re.compile(r"(?i)x-stainless-arch\\?[\"']?\s*[:=]\s*\\?[\"']?(?!\(arch omitted\))[^\s\"',\\]"),
    "runtime version": re.compile(r"(?i)x-stainless-runtime-version\\?[\"']?\s*[:=]\s*\\?[\"']?"
                                  r"(?!\(version omitted\))[^\s\"',\\]"),
}


def name_rule(word: str):
    """A word of N2_FORBIDDEN_NAMES as a whole word, any case (check-premises.sh name_scan)."""
    return re.compile(r"(?:(?<![A-Za-z0-9_])|(?<=\\[nrtbf]))%s(?![A-Za-z0-9_])" % re.escape(word), re.I)


def names(root: str, rels: list, verbatim: tuple) -> list:
    """The name rules of --check: one per word of N2_FORBIDDEN_NAMES (review round 2 G4). A word that
    is ordinary text of a VERBATIM file (an upstream schema, never a capture) is a common word: it is
    reported as a collision on stderr and not scanned, and no word ever rewrites a fixture."""
    text = ""
    for rel in rels:
        if rel in verbatim:
            with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as f:
                text += f.read()
    out = []
    for i, w in enumerate(os.environ.get("N2_FORBIDDEN_NAMES", "").split(), 1):
        r = name_rule(w)
        if r.search(text):
            sys.stderr.write("rescrub: note: word %d of N2_FORBIDDEN_NAMES is a common word (in the verbatim "
                             "schema); not scanned\n" % i)
            continue
        out.append(r)
    return out


def leaks(text: str, rules: list = (), classes: dict = LEAKS) -> list:
    found = [k for k, p in classes.items() if p.search(text)]
    if any(r.search(text) for r in rules):
        found.append("forbidden name from N2_FORBIDDEN_NAMES")
    return found


class DuplicateKey(ValueError):
    pass


def _pairs(pairs: list) -> dict:
    keys = [k for k, _ in pairs]
    if len(set(keys)) != len(keys):
        raise DuplicateKey("duplicate key")
    return dict(pairs)


def canon(text: str) -> str:
    """The canonical form of a JSON text: sorted keys, true, 1 and 1.0 kept apart (json.dumps writes
    them differently). ValueError when it does not parse; DuplicateKey when an object repeats a key."""
    return json.dumps(json.loads(text, object_pairs_hook=_pairs), sort_keys=True, ensure_ascii=False)


def dup_keys(text: str) -> bool:
    """True when the text is JSON with an object that repeats a key (review round 1 F1)."""
    try:
        canon(text)
    except DuplicateKey:
        return True
    except ValueError:
        return False
    return False


def same(text: str, new: str) -> bool:
    """True when new equals text byte for byte, or both parse with no repeated key and have the same
    canonical form (a JSON layout change only; review round 1 F1). A text with a repeated key is never
    the same: a parse keeps only the last value, so a leak in an earlier one would pass."""
    if dup_keys(text) or dup_keys(new):
        return False
    if new == text:
        return True
    try:
        return canon(text) == canon(new)
    except ValueError:
        return False


def scrubbed(path: str, run: str, env: dict) -> str:
    return subprocess.run([sys.executable, "-I", SCRUB, run, path], check=True,
                          capture_output=True, text=True, env=env).stdout


def group(rel: str) -> str:
    return rel.split(os.sep, 1)[0] if os.sep in rel else "."


def files(root: str) -> list:
    out = []
    for d, _, names in os.walk(root):
        for n in names:
            p = os.path.join(d, n)
            if os.path.isfile(p) and not os.path.islink(p):
                out.append(os.path.relpath(p, root))
    return sorted(out)


def main(argv: list) -> int:
    args = argv[1:]
    if not (len(args) in (1, 2) and args[0] in ("--check", "--write") and args[1:] in ([], *[[f] for f in FOLDERS])):
        sys.stderr.write("usage: rescrub_codex.py --check | --write [codex|claude-code|opencode]\n")
        return 2
    write = args[0] == "--write"
    folder = args[1] if len(args) == 2 else "codex"
    root = os.path.join(FIXTURES, folder)
    verbatim = VERBATIM[folder]
    classes = LEAKS if folder == "codex" else ENV_LEAKS
    env = dict(os.environ)
    if not write:
        env.update(SCRUB_FIXED="1", SCRUB_USER="user", SCRUB_HOSTS="")
    rels = files(root)
    rules = [] if write else names(root, rels, verbatim)
    groups = sorted({group(r) for r in rels})
    fix = 0
    wrote = 0
    with tempfile.TemporaryDirectory() as run:
        for rel in rels:
            p = os.path.join(root, rel)
            with open(p, encoding="utf-8", errors="replace") as f:
                text = f.read()
            found = leaks(text, rules, classes)
            if dup_keys(text):
                found.append("duplicate keys")
            if rel not in verbatim:
                env["SCRUB_IDMAP"] = os.path.join(run, "ids-%d.json" % groups.index(group(rel)))
                new = scrubbed(p, run, env)
                if not same(text, new):
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
