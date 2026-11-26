#!/usr/bin/env python3
"""Tests of rescrub_codex.py (node PC-13, review round 1 F4):
python3 -I scripts/premises/rescrub_test.py -> "rescrub tests: ok (<n>)", exit 0; else one
"rescrub test fail: <case>" line per failure, exit 1.

It tests same() (duplicate keys, the int, float and bool types, a layout-only change, a change of a
blank in a string), the folder argument and the per-folder verbatim lists (on a temporary fixtures
tree), and each ENV_LEAKS rule. No agent, no network.
"""
import contextlib
import importlib.util
import io
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("rescrub_codex", os.path.join(HERE, "rescrub_codex.py"))
R = importlib.util.module_from_spec(spec)
spec.loader.exec_module(R)

# (name, text, new, same() wanted)
SAME = [
    ("equal bytes", '{"a": 1}', '{"a": 1}', True),
    ("equal bytes, not json", "a b\n", "a b\n", True),
    ("layout only", '{"a": 1, "b": [true, 1.5]}', '{\n  "b": [\n    true,\n    1.5\n  ],\n  "a": 1\n}\n', True),
    ("duplicate key in text", '{"a": 1, "a": 2}', '{\n  "a": 2\n}\n', False),
    ("duplicate key in both, equal bytes", '{"a": 1, "a": 1}', '{"a": 1, "a": 1}', False),
    ("duplicate key in new", '{"a": 1}', '{"a": 1, "a": 1}', False),
    ("nested duplicate key", '[{"b": {"c": 1, "c": 1}}]', '[{"b": {"c": 1}}]', False),
    ("true and 1", '{"a": true}', '{"a": 1}', False),
    ("1 and 1.0", '{"a": 1}', '{"a": 1.0}', False),
    ("true and 1.0", '{"a": true}', '{"a": 1.0}', False),
    ("false and 0", '[false]', '[0]', False),
    ("a blank to a tab in a string", '{"a": "x y"}', '{"a": "x\\ty"}', False),
    ("one blank to two in a string", '{"a": "x y"}', '{"a": "x  y"}', False),
    ("a no-break space in a string", '{"a": "x\\u00a0y"}', '{"a": "x y"}', False),
    ("a trailing blank in a string", '{"a": "x"}', '{"a": "x "}', False),
    ("not json, a blank changed", "a b\n", "a  b\n", False),
    # Review round 2 G6: numbers compared by their text.
    ("1e400 and 2e400", "[1e400]", "[2e400]", False),
    ("1E2 and 100.0", "[1E2]", "[100.0]", False),
    ("-0 and 0", "[-0]", "[0]", False),
    ("a number and its string", "[1]", '["1"]', False),
    ("NaN and its string", "[NaN]", '["NaN"]', False),
    ("number text, layout only", "[1E2, -0]", "[\n  1E2,\n  -0\n]", True),
    ("100000 nested lists", "[" * 100000 + "]" * 100000, "[ " + "[" * 99999 + "]" * 100000, False),
]

# (rule, text, a leak wanted)
LEAK = [
    ("shell name", "x\n - Shell: zsh", True),
    ("shell name", "x\\n - Shell: zsh\\n", True),
    ("shell name", "Shell: zsh", True),
    ("shell name", "x\nShell: zsh", True),
    ("shell name", "x\\nShell: zsh", True),
    ("shell name", " - shell: zsh", True),
    ("shell name", " - SHELL: zsh", True),
    ("shell name", " - Shell:\tzsh", True),
    ("shell name", "x\\n - Shell:\\tzsh", True),
    ("shell name", " - Shell:zsh", True),
    ("shell name", "x\\\\\\n - Shell: zsh", True),
    ("shell name", " - Shell: (zsh)", True),
    ("shell name", " - Shell: (os omitted)", True),
    ("shell name", " - Shell: (omitted) zsh", True),
    ("shell name", " - Shell: (omitted)", False),
    ("shell name", "x\\n - Shell: (omitted)\\n - y", False),
    ("shell name", " - Shell:\t(omitted)", False),
    ("shell name", "x\\n - shell:\\t(omitted)\\n", False),
    ("shell name", "The Shell: zsh is the default", False),
    ("shell name", '"shell": "/bin/zsh"', False),
    # Review round 2 G1 (fail closed; replaces the round 1 F8 rule).
    ("shell name", "x\\\\n - Shell: zsh", True),
    ("shell name", 'data: {"partial_json":"{\\"t\\": \\"a\\\\n - Shell: zsh\\"}"}', True),
    ("shell name", "x\\u000a - Shell: zsh", True),
    ("shell name", "x\\u000A - Shell: zsh", True),
    ("shell name", "x\r - Shell: zsh", True),
    ("shell name", "x\\r - Shell: zsh", True),
    ("shell name", "x\\\\n - Shell: (omitted)", False),
    ("shell name", " - Shell: ", False),
    # Review round 2 G2: dashless lines need a capital first letter and a host value.
    ("shell name", "Shell: run make first", False),
    ("shell name", "x\n  shell: bash\n", False),
    ("shell name", "x\nshell: zsh", False),
    ("shell name", "Shell: /bin/zsh", True),
    ("shell name", "Shell: /usr/local/bin/bash", True),
    ("shell name", "Shell: /opt/homebrew/bin/fish", True),
    ("shell name", "Shell: pwsh", True),
    ("shell name", "Shell: zshrc is read", False),
    ("shell name", " - shell: anything", True),
    # Review round 2 G5: the shell placeholder only, case-sensitive, the whole value; CRLF.
    ("shell name", " - Shell: (OMITTED)", True),
    ("shell name", " - Shell: (omitted)\r", False),
    ("shell name", "x\r\n - Shell: (omitted)\r\n", False),
    ("shell name", "x\r\n - Shell: zsh\r\n", True),
    ("shell name", " - Shell: (arch omitted)", True),
    ("shell name", " - Shell: (version omitted)", True),
    # Review round 2 G8: quoted values, no-break spaces.
    ("shell name", ' - Shell: "zsh"', True),
    ("shell name", 'Shell: "zsh"', True),
    ("shell name", 'x\\n - Shell: \\"zsh\\"', True),
    ("shell name", " - Shell: zsh", True),
    ("shell name", " - Shell:\\u00a0zsh", True),
    ("shell name", ' - Shell: "(omitted)"', False),
    ("shell name", 'x\\n - Shell: \\"(omitted)\\"', False),
    ("shell name", " - Shell: (omitted)", False),
    ("os version", " - OS Version: Darwin 25.6.0", True),
    ("os version", "x\\n - OS Version: Darwin 25.6.0\\n", True),
    ("os version", "OS Version: Darwin 25.6.0", True),
    ("os version", " - os version:\tLinux 6.8.0", True),
    ("os version", "x\\nOS VERSION:\\tDarwin 25.6.0", True),
    ("os version", " - OS Version: (Darwin 25.6.0)", True),
    ("os version", " - OS Version: (omitted)", True),
    ("os version", " - OS Version: (os omitted)", False),
    ("os version", "x\\n - OS Version: (os omitted)\\n", False),
    ("os version", "x\\\\n - OS Version: Darwin 25.6.0", True),
    ("os version", "The OS Version: Darwin 25.6.0", False),
    ("os version", "OS version: check it is 14 or later", False),
    ("os version", "os version: Darwin 25.6.0", False),
    ("os version", "OS Version: macOS 15.1", True),
    ("os version", "OS Version: Windows 10.0.26100", True),
    ("os version", "OS Version: Linux6", True),
    ("os version", " - OS Version: (os omitted)\r", False),
    ("os version", " - OS Version: (OS omitted)", True),
    ("os version", " - OS Version: (os omitted) Darwin", True),
    ("os version", ' - OS Version: "Darwin 25.6.0"', True),
    ("os version", "x\\u000a - OS Version: Darwin 25.6.0", True),
    ("arch", '"x-stainless-arch": "arm64"', True),
    ("arch", '{\\"X-Stainless-Arch\\":\\"x64\\"}', True),
    ("arch", "x-stainless-arch: arm64", True),
    ("arch", '"x-stainless-arch": "(arm64)"', True),
    ("arch", '"x-stainless-arch": "(arch omitted)"', False),
    ("arch", '"x-stainless-os": "MacOS"', False),
    # Review round 2 G5: anchored, case-sensitive, the key's own placeholder; CRLF.
    ("arch", '"x-stainless-arch": "(arch omitted)x"', True),
    ("arch", '"x-stainless-arch": "(ARCH OMITTED)"', True),
    ("arch", '"x-stainless-arch": "(version omitted)"', True),
    ("arch", "x-stainless-arch: (arch omitted)\r\n", False),
    ("arch", "x-stainless-arch: (arch omitted) arm64", True),
    ("arch", '{\\"x-stainless-arch\\":\\"(arch omitted)\\"}', False),
    ("runtime version", '"x-stainless-runtime-version": "v26.12.1"', True),
    ("runtime version", '{\\"X-Stainless-Runtime-Version\\":\\"v22.1.0\\"}', True),
    ("runtime version", '"x-stainless-runtime-version": "(version omitted)"', False),
    ("runtime version", '"x-stainless-runtime": "node"', False),
    ("runtime version", '"x-stainless-runtime-version": "(version omitted)v26"', True),
    ("runtime version", '"x-stainless-runtime-version": "(arch omitted)"', True),
    ("runtime version", "x-stainless-runtime-version: (version omitted)\r\n", False),
    # Review round 2 G7: the messaging socket pid.
    ("socket pid", "/tmp/cc-socks/95709.sock", True),
    ("socket pid", "/tmp/cc-socks/<pid>.sock", False),
]

# Review round 2 G1: the reviewer's forms, a Claude Code environment block inside JSON inside a JSON
# string (an .sse partial_json line; a .summary.json string holding an escaped newline).
PLANT_SSE = ('data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":'
             '"{\\"text\\": \\"a\\\\n - Shell: zsh\\\\n - OS Version: Darwin 25.6.0\\"}"}}\n')
PLANT_SUMMARY = '{"summary": "a\\\\n - Shell: zsh\\\\n - OS Version: Darwin 25.6.0"}\n'

# A text that scrub.py changes (a macOS home path) and that no leak class of any folder matches.
CHANGED = '{"p": "/Users/bob/x"}\n'


def put(root, rel, text):
    p = os.path.join(root, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", encoding="utf-8") as f:
        f.write(text)


def run(argv):
    out = io.StringIO()
    err = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        rc = R.main(["rescrub_codex.py"] + argv)
    return rc, out.getvalue()


def main():
    bad = []
    n = 0
    for name, text, new, want in SAME:
        n += 1
        if R.same(text, new) is not want:
            bad.append("same(): %s: got %r" % (name, not want))
    for name, text, want in (("duplicate keys", '{"a": {"b": 1, "b": 2}}', True),
                             ("duplicate keys in a list", '[1, {"x": 1, "x": 1}]', True),
                             ("no duplicate keys", '{"a": {"b": 1}, "b": 2}', False),
                             ("not json", "a: 1\na: 2\n", False),
                             ("100000 nested lists", "[" * 100000 + "]" * 100000, False)):
        n += 1
        if R.dup_keys(text) is not want:
            bad.append("dup_keys(): %s: got %r" % (name, not want))
    n += 1
    if set(R.ENV_LEAKS) != {r for r, _, _ in LEAK}:
        bad.append("ENV_LEAKS: rules %r, tested %r" % (sorted(R.ENV_LEAKS), sorted({r for r, _, _ in LEAK})))
    for rule, text, want in LEAK:
        n += 1
        got = rule in R.leaks(text, (), R.ENV_LEAKS)
        if got is not want:
            bad.append("ENV_LEAKS %s: %r: got %r" % (rule, text, got))
    # The folder argument and the per-folder verbatim lists.
    n += 1
    if R.FOLDERS != ("codex", "claude-code", "opencode") or R.VERBATIM != {
            "codex": ("config.schema.json",), "claude-code": ("settings-schema.json",), "opencode": ()}:
        bad.append("folders and verbatim lists: got %r, %r" % (R.FOLDERS, R.VERBATIM))
    for argv in ([], ["--check", "nope"], ["--check", "codex", "x"], ["--scan"], ["--write", "fixtures"]):
        n += 1
        rc, _ = run(argv)
        if rc != 2:
            bad.append("usage %r: exit %d, expected 2" % (argv, rc))
    saved = R.FIXTURES, os.environ.pop("N2_FORBIDDEN_NAMES", None)
    try:
        with tempfile.TemporaryDirectory() as fx:
            R.FIXTURES = fx
            # codex: its verbatim schema is not scrubbed; a.json differs from the scrub only in layout.
            put(fx, "codex/config.schema.json", CHANGED)
            put(fx, "codex/a.json", '{"a": 1}')
            # claude-code: its verbatim schema is not scrubbed, but is scanned for leaks; a file with
            # duplicate keys is reported.
            put(fx, "claude-code/settings-schema.json", '{"p": "/Users/bob/x", "d": "x\\n - Shell: zsh"}\n')
            put(fx, "claude-code/q/dup.json", '{"a": 1, "a": 1}')
            put(fx, "claude-code/r/plant.sse", PLANT_SSE)
            put(fx, "claude-code/r/plant.summary.json", PLANT_SUMMARY)
            # opencode: no verbatim file; config-schema.json is scrubbed like any other file.
            put(fx, "opencode/config-schema.json", CHANGED)
            put(fx, "opencode/settings-schema.json", CHANGED)
            want = {
                (): (0, "rescrub: ok (2 files)\n"),
                ("codex",): (0, "rescrub: ok (2 files)\n"),
                ("claude-code",): (1, "rescrub: q/dup.json: duplicate keys\nrescrub: q/dup.json: scrub changes it\n"
                                      "rescrub: r/plant.sse: shell name\nrescrub: r/plant.sse: os version\n"
                                      "rescrub: r/plant.sse: scrub changes it\n"
                                      "rescrub: r/plant.summary.json: shell name\n"
                                      "rescrub: r/plant.summary.json: os version\n"
                                      "rescrub: r/plant.summary.json: scrub changes it\n"
                                      "rescrub: settings-schema.json: shell name\nrescrub: 4 files to fix\n"),
                ("opencode",): (1, "rescrub: config-schema.json: scrub changes it\n"
                                   "rescrub: settings-schema.json: scrub changes it\nrescrub: 2 files to fix\n"),
            }
            for folder, (rc_want, out_want) in want.items():
                n += 1
                rc, out = run(["--check", *folder])
                if (rc, out) != (rc_want, out_want):
                    bad.append("--check %s: exit %d, output %r" % (" ".join(folder) or "(no folder)", rc, out))
            # G1: --write masks both planted forms; only the verbatim schema's leak is left.
            n += 1
            rc, out = run(["--write", "claude-code"])
            rc2, out2 = run(["--check", "claude-code"])
            if (rc, out, rc2, out2) != (0, "rescrub: wrote 3 files\n", 1,
                                        "rescrub: settings-schema.json: shell name\nrescrub: 1 files to fix\n"):
                bad.append("--write claude-code then --check: %r" % ((rc, out, rc2, out2),))
    finally:
        R.FIXTURES = saved[0]
        if saved[1] is not None:
            os.environ["N2_FORBIDDEN_NAMES"] = saved[1]
    for b in bad:
        print("rescrub test fail: " + b)
    if bad:
        return 1
    print("rescrub tests: ok (%d)" % n)
    return 0


if __name__ == "__main__":
    sys.exit(main())
