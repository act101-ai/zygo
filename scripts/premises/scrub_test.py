#!/usr/bin/env python3
"""Tests of scrub.py (node N38): python3 -I scripts/premises/scrub_test.py -> "scrub tests: ok (<n>)".

Every key-like literal is built at run time from two parts, so this file holds none.
Each case: an input text (as a JSON string value and as plain text), the scrubbed value wanted,
and the secret parts that must be gone.
"""
import getpass
import json
import os
import re
import socket
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SCRUB = os.path.join(HERE, "scrub.py")
A = "abcDEF123ghiJKL456mnoPQR789stu"  # 30 base62 characters

host = socket.gethostname()
short = host.split(".")[0]
user = getpass.getuser()
K = "sk" + "-" + A[:25]  # a legacy-form key
# Cases whose input or output holds a machine name: a failure does not print the output.
PRIVATE = ("host name", "host name (scutil", "user name after an escape")
CASES = [
    # (name, input, wanted output, parts that must be gone)
    ("openai project key", "sk" + "-proj-" + A, "[key omitted]", [A]),
    ("legacy key", "sk" + "-" + A[:25], "[key omitted]", [A[:25]]),
    ("masked key", "sk" + "-abc" + "****" + "wxyz", "[key omitted]", ["****wxyz"]),
    ("organization id", "org" + "-" + A[:12], "[org omitted]", [A[:12]]),
    ("project id", "proj" + "_" + A[:12], "[project omitted]", [A[:12]]),
    ("zygo token", "zyg" + "_s_" + A, "[zygo token omitted]", [A]),
    ("windows path", "C:" + "\\Users\\name\\x", "/home/user\\x", ["name"]),
    ("windows path, name with spaces", "C:" + "\\Users\\Jo Ann Smith\\Documents\\a.txt",
     "/home/user\\Documents\\a.txt", ["Jo Ann", "Smith"]),
    ("windows path, slashes, name with spaces", "D:" + "/Users/Jo Ann/x", "/home/user/x", ["Jo Ann"]),
    ("github classic token", "token gh" + "p_" + A + "xyz", "token [github token omitted]", [A]),
    ("github oauth token", "gh" + "o_" + A, "[github token omitted]", [A]),
    ("github server token", "gh" + "s_" + A, "[github token omitted]", [A]),
    ("github fine-grained token", "github" + "_pat_" + "11ABC_" + A, "[github token omitted]", [A]),
    ("jwt", "id " + "eyJ" + "hbGciOiJIUzI1NiJ9." + "eyJ" + "zdWIiOiIxMjMifQ." + "sig_" + A,
     "id [jwt omitted]", ["hbGciOiJIUzI1NiJ9", A]),
    ("host name", "host is " + host + " and " + short.upper(), "host is host and host", [host]),
    ("x-api-key plain text", "x-api" + "-key: " + A, "x-api-key: [omitted]", [A]),
    ("x-api-key equals", "X-Api" + "-Key=" + A + ", next", "X-Api-Key=[omitted], next", [A]),
    # Round 3 (N38). T4: right after a backslash escape in nested JSON text (\\n, \\t, \\", \\uXXXX).
    ("key after an escaped newline", '{"m":"a\\n' + K + '"}', '{"m":"a\\n[key omitted]"}', [A[:25]]),
    ("key after an escaped tab", "a\\t" + K, "a\\t[key omitted]", [A[:25]]),
    ("key after an escaped quote", '{\\"k\\":\\"' + K + '\\"}', '{\\"k\\":\\"[key omitted]\\"}', [A[:25]]),
    ("key after a unicode escape", "a\\u0022" + K, "a\\u0022[key omitted]", [A[:25]]),
    ("organization id after an escape", "a\\n" + "org" + "-" + A[:12], "a\\n[org omitted]", [A[:12]]),
    ("project id after an escape", "a\\t" + "proj" + "_" + A[:12], "a\\t[project omitted]", [A[:12]]),
    ("github token after an escape", "a\\n" + "gh" + "p_" + A, "a\\n[github token omitted]", [A]),
    ("user name after an escape", "a\\n" + user + " b", "a\\nuser b", [user]),
    # T5: x-api-key backslash-quoted (nested JSON), single-quoted, upper case.
    ("x-api-key backslash-quoted", '{\\"x-api' + '-key\\":\\"' + A + '\\"}', '{\\"x-api-key\\":\\"[omitted]\\"}', [A]),
    ("x-api-key single-quoted", "{'x-api" + "-key': '" + A + "'}", "{'x-api-key': '[omitted]'}", [A]),
    ("x-api-key upper case", "X-API" + "-KEY: " + A, "X-API-KEY: [omitted]", [A]),
    # T8: a Windows path ends at the name; any case; an MSYS path with a space in the name.
    # (Round 4: a short tail at the end of the text is taken as the name; the line here is longer.)
    ("windows path ends at the name", "C:" + "\\Users\\bob said hi to all of the people in the room today",
     "/home/user said hi to all of the people in the room today", ["bob"]),
    ("windows path, lower case", "c:" + "\\users\\name\\x", "/home/user\\x", ["name"]),
    ("msys path, name with spaces", "/c/" + "Users/Jo Ann/x and more", "/home/user/x and more", ["Jo Ann", "Ann"]),
    # Round 4 (U1): a name with spaces at the end of the text, or before a quote, is removed whole.
    ("windows name with spaces at the end", "C:" + "\\Users\\Jo Ann", "/home/user", ["Jo", "Ann"]),
    ("windows name, slashes, three words at the end", "C:" + "/Users/Mary Jane Smith", "/home/user",
     ["Mary", "Jane", "Smith"]),
    ("windows name with spaces before a quote", 'path "C:' + '\\Users\\Jo Ann" next', 'path "/home/user" next',
     ["Jo", "Ann"]),
    ("windows name in nested json at the end", '{"p":"C:' + '\\\\Users\\\\Jo Ann"}', '{"p":"/home/user"}',
     ["Jo", "Ann"]),
    ("msys name with spaces at the end", "/c/" + "Users/Mary Jane Smith", "/home/user", ["Mary", "Jane", "Smith"]),
]
# PC-2: Codex rollout file names and session and turn ids (UUIDv7, lower case).
U1 = "0199b0c2-7c1d-7a3e-9f00-1234567890ab"
U2 = "0199b0c3-0000-7000-8000-00000000abcd"
CASES += [
    ("codex rollout file name", "rollout-2026-11-25T10-20-30-" + U1 + ".jsonl",
     "rollout-2026-11-25T10-20-30-00000000-0000-4000-8000-000000000001.jsonl", [U1]),
    ("codex session and turn ids", '{"session_id":"' + U1 + '","turn_id":"' + U2 + '"}',
     '{"session_id":"00000000-0000-4000-8000-000000000001","turn_id":"00000000-0000-4000-8000-000000000002"}',
     [U1, U2]),
]
# PC-10: the host OS name, version and architecture in a user agent -> "(os omitted)".
CASES += [
    ("user agent, macos", "codex_exec/0.172.0 (Mac OS 27.2.1; arm64) xterm-256color",
     "codex_exec/0.172.0 (os omitted) xterm-256color", ["27.2.1", "arm64"]),
    ("user agent, linux", "codex_cli_rs/0.172.0 (Linux 6.8.0-49-generic; x86_64) dumb",
     "codex_cli_rs/0.172.0 (os omitted) dumb", ["6.8.0", "x86_64"]),
    ("user agent, ubuntu", "codex_exec/0.172.0 (Ubuntu 24.04; aarch64) xterm",
     "codex_exec/0.172.0 (os omitted) xterm", ["24.04", "aarch64"]),
    ("user agent, windows", "codex_exec/0.172.0 (Windows 10.0.26100; x86_64) unknown",
     "codex_exec/0.172.0 (os omitted) unknown", ["26100", "x86_64"]),
    ("user agent, windows without arch", "codex_exec/0.172.0 (Windows 10.0.26100)",
     "codex_exec/0.172.0 (os omitted)", ["26100"]),
    ("a parenthesis that is not a user agent", "see note (Mac OS 27.2.1; arm64) and x/y (b 2)",
     "see note (Mac OS 27.2.1; arm64) and x/y (b 2)", []),
]
# PC-10 review round 1, F4: more OS forms (a user agent with macOS, a distribution name, a Windows
# edition, no blank before the parenthesis, a browser's Macintosh text), JSON os_type / os_version
# values, and uname -a lines. F3: a <timezone> other than UTC (a backstop; TZ=UTC is the fix).
CASES += [
    ("user agent, macos lower case", "codex/1.0 (macOS 15.1; arm64) t", "codex/1.0 (os omitted) t", ["15.1", "arm64"]),
    ("user agent, arch linux", "codex/1.0 (Arch Linux Rolling Release; x86_64) t", "codex/1.0 (os omitted) t",
     ["Rolling", "x86_64"]),
    ("user agent, ubuntu with arch", "codex/1.0 (Ubuntu 22.04.5 LTS; aarch64)", "codex/1.0 (os omitted)", ["22.04", "aarch64"]),
    ("user agent, windows edition", "codex/1.0 (Windows 10 Pro 10.0.19045; x86_64)", "codex/1.0 (os omitted)",
     ["19045", "Pro"]),
    ("user agent, no blank", "x/1.0(Mac OS 27.2.1; arm64) t", "x/1.0(os omitted) t", ["27.2.1", "arm64"]),
    ("browser user agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
     "Mozilla/5.0 (os omitted) AppleWebKit/537.36", ["10_15_7", "Intel"]),
    ("macintosh text alone", "platform Macintosh; Intel Mac OS X 10_15_7 end", "platform (os omitted) end",
     ["10_15_7", "Intel"]),
    ("json os_type and os_version", '{"os_type":"Mac OS","os_version":"27.2.1","x":1}',
     '{"os_type":"(os omitted)","os_version":"(os omitted)","x":1}', ["Mac OS", "27.2.1"]),
    ("nested json os_version", '{\\"os_version\\": \\"6.8.0\\"}', '{\\"os_version\\": \\"(os omitted)\\"}', ["6.8.0"]),
    ("uname -a, darwin", "Darwin zzbox.local 25.2.0 Darwin Kernel Version 25.2.0: Tue Oct 1; root:xnu-123/RELEASE_ARM64_T6041 arm64",
     "(os omitted)", ["25.2.0", "zzbox", "arm64"]),
    ("uname -a, linux", "Linux zzbox 6.8.0-49-generic #49-Ubuntu SMP PREEMPT_DYNAMIC Thu x86_64 x86_64 GNU/Linux",
     "(os omitted)", ["6.8.0", "zzbox", "x86_64"]),
    ("prose with linux and a version", "Linux and 2.0 support", "Linux and 2.0 support", []),
    ("timezone other than utc", "<timezone>America/Los_Angeles</timezone>", "<timezone>(omitted)</timezone>",
     ["Los_Angeles"]),
    ("timezone utc kept", "<timezone>UTC</timezone> <timezone>Etc/UTC</timezone>",
     "<timezone>UTC</timezone> <timezone>Etc/UTC</timezone>", []),
    # Round 2 (G6): the same forms as codex_record's check.
    ("timezone with escaped slashes", "<timezone>America\\/Los_Angeles<\\/timezone>",
     "<timezone>(omitted)<\\/timezone>", ["Los_Angeles"]),
    ("timezone as entities", "&lt;timezone&gt;Europe/Paris&lt;/timezone&gt;",
     "&lt;timezone&gt;(omitted)&lt;/timezone&gt;", ["Paris"]),
    ("timezone as unicode escapes", "\\u003ctimezone\\u003eAsia/Tokyo\\u003c/timezone\\u003e",
     "\\u003ctimezone\\u003e(omitted)\\u003c/timezone\\u003e", ["Tokyo"]),
    ("timezone utc forms kept", "<timezone>utc</timezone> <timezone>Etc/GMT</timezone> <timezone>GMT</timezone> "
     "<timezone>UTC+0</timezone> <timezone>UTC+00:00</timezone>",
     "<timezone>utc</timezone> <timezone>Etc/GMT</timezone> <timezone>GMT</timezone> "
     "<timezone>UTC+0</timezone> <timezone>UTC+00:00</timezone>", []),
]
# T8: the macOS LocalHostName and ComputerName, when scutil gives them (4 characters or more).
for k in ("LocalHostName", "ComputerName"):
    try:
        n = subprocess.run(["scutil", "--get", k], capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        n = ""
    if len(n) >= 4 and n.lower() != "localhost":
        CASES.append(("host name (scutil %s)" % k, "on " + n + ".", "on host.", [n]))


def scrub(path):
    with tempfile.TemporaryDirectory() as run:
        return subprocess.run([sys.executable, "-I", SCRUB, run, path], check=True,
                              capture_output=True, text=True).stdout


def main():
    bad = []
    with tempfile.TemporaryDirectory() as d:
        for name, text, want, gone in CASES:
            j = os.path.join(d, "c.json")
            with open(j, "w") as f:
                json.dump({"v": text}, f)
            out = scrub(j)
            try:
                got = json.loads(out)["v"]
            except ValueError:
                bad.append("%s: output is not valid JSON" % name)
                continue
            if got != want:
                bad.append("%s (json): got %r" % (name, "[not shown]" if name.startswith(PRIVATE) else got))
            t = os.path.join(d, "c.txt")
            with open(t, "w") as f:
                f.write(text + "\n")
            plain = scrub(t)
            for g in gone:
                if g.lower() in out.lower() or g.lower() in plain.lower():
                    bad.append("%s: a part is left" % name)
                    break
        # T8: a host name shorter than 4 characters is not scrubbed, and a note says so.
        j = os.path.join(d, "short.json")
        with open(j, "w") as f:
            json.dump({"v": "on abc."}, f)
        with tempfile.TemporaryDirectory() as run:
            r = subprocess.run([sys.executable, "-I", "-c",
                                "import runpy, socket, sys; socket.gethostname = lambda: 'abc'; "
                                "sys.argv = ['scrub.py', sys.argv[1], sys.argv[2]]; "
                                "runpy.run_path(%r, run_name='__main__')" % SCRUB,
                                run, j], capture_output=True, text=True)
        if r.returncode != 0 or json.loads(r.stdout)["v"] != "on abc." or "shorter than 4" not in r.stderr:
            bad.append("short host name: not skipped with a note")
        # U5: "-" and "_" are part of a host name (the same edges as check-premises.sh).
        with open(j, "w") as f:
            json.dump({"v": "zzhostq_x zzhostq-y a\\nzzhostq. x_zzhostq"}, f)
        with tempfile.TemporaryDirectory() as run:
            r = subprocess.run([sys.executable, "-I", "-c",
                                "import runpy, socket, sys; socket.gethostname = lambda: 'zzhostq'; "
                                "sys.argv = ['scrub.py', sys.argv[1], sys.argv[2]]; "
                                "runpy.run_path(%r, run_name='__main__')" % SCRUB,
                                run, j], capture_output=True, text=True)
        if r.returncode != 0 or json.loads(r.stdout)["v"] != "zzhostq_x zzhostq-y a\\nhost. x_zzhostq":
            bad.append("host name edges: got %r" % (r.stdout,))
        # PC-2: CODEX_HOME paths under the run folder (the default under home, and another one).
        with tempfile.TemporaryDirectory() as run:
            rr = os.path.realpath(run)
            t = os.path.join(d, "cx.txt")
            with open(t, "w") as f:
                f.write(rr + "/home/.codex/sessions/2026/11/25/x.jsonl " + rr + "/cx/config.toml\n")
            got = subprocess.run([sys.executable, "-I", SCRUB, rr, t], check=True,
                                 capture_output=True, text=True).stdout
        want = "/home/user/.codex/sessions/2026/11/25/x.jsonl /tmp/run/cx/config.toml\n"
        if got != want:
            bad.append("codex home paths: got %r" % (got,))
        # PC-10: a TMPDIR that ends in "/" gives run folders like .../T//tmp.X; such paths map to
        # /tmp/run too (with and without /private), not to /tmp/x.
        with tempfile.TemporaryDirectory() as run:
            rr = os.path.realpath(run)
            head, tail = os.path.split(rr)
            dbl = head + "//" + tail
            alt = dbl[len("/private"):] if dbl.startswith("/private/") else dbl
            t = os.path.join(d, "dbl.txt")
            with open(t, "w") as f:
                f.write(dbl + "/tmp/x.txt " + alt + "/home/.codex/a " + alt + "//repo/b\n")
            got = subprocess.run([sys.executable, "-I", SCRUB, dbl, t], check=True,
                                 capture_output=True, text=True).stdout
        want = "/tmp/run/tmp/x.txt /home/user/.codex/a /repo/b\n"
        if got != want:
            bad.append("double-slash run paths: got %r" % (got,))
        # F4: os_type / os_version as keys of a captured JSON object.
        t = os.path.join(d, "os.json")
        with open(t, "w") as f:
            json.dump({"client": {"os_type": "Mac OS", "os_version": "27.2.1", "os_arch": "arm64", "k": "v"}}, f)
        with tempfile.TemporaryDirectory() as run:
            got = json.loads(subprocess.run([sys.executable, "-I", SCRUB, run, t], check=True,
                                            capture_output=True, text=True).stdout)
        want = {"client": {"os_type": "(os omitted)", "os_version": "(os omitted)", "os_arch": "(os omitted)", "k": "v"}}
        if got != want:
            bad.append("json os keys: got %r" % (got,))
        # F9: Claude's dashed form of a run path that holds "//" (so "--").
        with tempfile.TemporaryDirectory() as run:
            rr = os.path.realpath(run)
            head, tail = os.path.split(rr)
            dbl = head + "//" + tail
            alt = dbl[len("/private"):] if dbl.startswith("/private/") else dbl
            dash = lambda x: re.sub(r"[^A-Za-z0-9]", "-", x)
            t = os.path.join(d, "dash.txt")
            with open(t, "w") as f:
                f.write(dash(dbl + "/repo") + " " + dash(alt + "/home") + " " + dash(alt) + "-x\n")
            got = subprocess.run([sys.executable, "-I", SCRUB, dbl, t], check=True,
                                 capture_output=True, text=True).stdout
        if got != "-repo -home-user -tmp-run-x\n":
            bad.append("dashed run paths with --: got %r" % (got,))
    for b in bad:
        print("scrub test fail: " + b)
    if bad:
        sys.exit(1)
    print("scrub tests: ok (%d)" % (len(CASES) + 6))


main()
