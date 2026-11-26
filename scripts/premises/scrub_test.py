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
# PC-11: Codex OS keys, login shell, macOS writable roots, item ids, sandbox, marketplace and macOS env names.
I1 = "da690f77-7707-5fdf-97a5-850943e7a34c"
I2 = "8e32dd58-d565-50b3-83f4-72b17fc59ae7"
PC11 = [
    ("codex platformOs", '{"platformFamily":"unix","platformOs":"macos"}',
     '{"platformFamily":"(os omitted)","platformOs":"(os omitted)"}', ["macos", "unix"]),
    ("codex platformOs escaped", '{\\"platformOs\\":\\"linux\\"}', '{\\"platformOs\\":\\"(os omitted)\\"}', ["linux"]),
    ("login shell tag", "<environment_context>\n  <cwd>/repo</cwd>\n  <shell>zsh</shell>\n</environment_context>",
     "<environment_context>\n  <cwd>/repo</cwd>\n  <shell>(omitted)</shell>\n</environment_context>", ["zsh"]),
    ("login shell tag, unicode escapes",
     "\\u003cenvironment_context\\u003e\\u003cshell\\u003efish\\u003c/shell\\u003e\\u003c/environment_context\\u003e",
     "\\u003cenvironment_context\\u003e\\u003cshell\\u003e(omitted)\\u003c/shell\\u003e\\u003c/environment_context\\u003e",
     ["fish"]),
    ("login shell tag, entities", "&lt;environment_context&gt;&lt;shell&gt;bash&lt;/shell&gt;&lt;/environment_context&gt;",
     "&lt;environment_context&gt;&lt;shell&gt;(omitted)&lt;/shell&gt;&lt;/environment_context&gt;", ["bash"]),
    # Review round 1 F6: the shell rules apply only to the environment_context block and command fields.
    ("shell tag outside environment_context kept", "The <shell>bash</shell> tag names a shell.",
     "The <shell>bash</shell> tag names a shell.", []),
    ("login shell in model text kept", "Run /bin/bash -lc 'ls' to list files.", "Run /bin/bash -lc 'ls' to list files.", []),
    ("macos writable root", "The writable roots are `/private/tmp`, `/repo`.", "The writable roots are `/tmp`, `/repo`.",
     ["/private"]),
    ("macos temp writable root", "roots `/private/var/folders/ab/cd_ef/T`, `/repo`", "roots `/tmp/x`, `/repo`",
     ["/private", "cd_ef"]),
    ("codex item ids", '{"a":"msg_' + I1 + '","b":"fco_' + I2 + '","c":"msg_' + I1 + '","d":"ctco_' + I2 + '"}',
     '{"a":"msg_placeholder0001","b":"fco_placeholder0001","c":"msg_placeholder0001","d":"ctco_placeholder0001"}',
     [I1, I2]),
    ("codex item id after an escape", "a\\nmsg_" + I1, "a\\nmsg_placeholder0001", [I1]),
    ("macos sandbox name", '{\\"sandbox\\":\\"seatbelt\\",\\"sandbox_mode\\":\\"workspace-write\\"}',
     '{\\"sandbox\\":\\"(os omitted)\\",\\"sandbox_mode\\":\\"workspace-write\\"}', ["seatbelt"]),
    ("linux sandbox name", '"sandbox":"linux_seccomp"', '"sandbox":"(os omitted)"', ["seccomp"]),
    ("no sandbox kept", '"sandbox":"none"', '"sandbox":"none"', []),
    ("marketplace os name", ".tmp/marketplace-macos-1a2b3c/x", ".tmp/marketplace-os-omitted/x", ["macos", "1a2b3c"]),
    ("login shell in a command", '"command":"/bin/zsh -lc \'echo hi\'"', '"command":"(shell omitted) -lc \'echo hi\'"', ["zsh"]),
    ("login shell in exec json", '{"command":"/opt/homebrew/bin/fish -lc \'ls\'"}',
     '{"command":"(shell omitted) -lc \'ls\'"}', ["fish", "homebrew"]),
    ("login shell in an escaped cmd field", '{\\"cmd\\":\\"/bin/bash -lc ls\\"}', '{\\"cmd\\":\\"(shell omitted) -lc ls\\"}',
     ["bash"]),
    ("macos env line", "__CF_USER_TEXT_ENCODING=0x1F5:0x0:0x0\nPATH=/bin", "(os env omitted)\nPATH=/bin",
     ["0x1F5", "__CF"]),
    ("macos env name in a list", '["HOME","__CF_USER_TEXT_ENCODING"]', '["HOME","(os env omitted)"]', ["__CF"]),
]
CASES += PC11
# PC-13: the shell name and OS version lines of a Claude Code environment block.
CASES += [
    ("claude code env block", "Platform: darwin\n - Shell: zsh\n - OS Version: Darwin 25.6.0\n - Downloaded files",
     "Platform: darwin\n - Shell: (omitted)\n - OS Version: (os omitted)\n - Downloaded files", ["zsh", "25.6.0"]),
    ("claude code env block, json-escaped", "x\\n - Shell: bash\\n - OS Version: Linux 6.8.0-49-generic\\n - y",
     "x\\n - Shell: (omitted)\\n - OS Version: (os omitted)\\n - y", ["bash", "6.8.0"]),
    ("shell outside an env block kept", 'The Shell: zsh is the default; "shell": "/bin/zsh"',
     'The Shell: zsh is the default; "shell": "/bin/zsh"', []),
    ("env block placeholders kept", " - Shell: (omitted)\n - OS Version: (os omitted)",
     " - Shell: (omitted)\n - OS Version: (os omitted)", []),
    # Review round 1 F5: any case, any whitespace after the colon (space, tab, escaped \t), with or
    # without the leading "- ", at a line start or after a literal or escaped newline.
    ("env block, lower case", "x\n - shell: zsh\n - os version: Darwin 25.6.0",
     "x\n - shell: (omitted)\n - os version: (os omitted)", ["zsh", "25.6.0"]),
    ("env block, upper case", "x\n - SHELL: zsh\n - OS VERSION: Darwin 25.6.0",
     "x\n - SHELL: (omitted)\n - OS VERSION: (os omitted)", ["zsh", "25.6.0"]),
    ("env block, tab after the colon", "x\n - Shell:\tzsh\n - OS Version:\tDarwin 25.6.0",
     "x\n - Shell:\t(omitted)\n - OS Version:\t(os omitted)", ["zsh", "25.6.0"]),
    ("env block, escaped tab after the colon", "x\\n - Shell:\\tzsh\\n - OS Version:\\tDarwin 25.6.0",
     "x\\n - Shell:\\t(omitted)\\n - OS Version:\\t(os omitted)", ["zsh", "25.6.0"]),
    ("env block, no dash", "Shell: zsh\nOS Version: Darwin 25.6.0",
     "Shell: (omitted)\nOS Version: (os omitted)", ["zsh", "25.6.0"]),
    ("env block, no dash after an escaped newline", "x\\nShell: fish\\nOS Version: Linux 6.8.0",
     "x\\nShell: (omitted)\\nOS Version: (os omitted)", ["fish", "6.8.0"]),
    # Review round 2 G1 (replaces round 1 F8, fail closed): one or more backslashes then "n" or "r", a
    # carriage return, or a \u000a escape starts a line.
    ("G1: escaped backslash then n", "x\\\\n - Shell: zsh", "x\\\\n - Shell: (omitted)", ["zsh"]),
    ("env block, escaped backslash then escaped newline", "x\\\\\\n - Shell: zsh",
     "x\\\\\\n - Shell: (omitted)", ["zsh"]),
    ("G1: plant.sse partial_json",
     'data: {"delta":{"type":"input_json_delta","partial_json":"{\\"text\\": \\"a\\\\n - Shell: zsh\\\\n - OS Version: '
     'Darwin 25.6.0\\"}"}}',
     'data: {"delta":{"type":"input_json_delta","partial_json":"{\\"text\\": \\"a\\\\n - Shell: (omitted)\\\\n - '
     'OS Version: (os omitted)\\"}"}}', ["zsh", "25.6.0"]),
    ("G1: .summary.json doubly escaped", '{"summary": "a\\\\n - Shell: zsh\\\\n - OS Version: Darwin 25.6.0"}',
     '{"summary": "a\\\\n - Shell: (omitted)\\\\n - OS Version: (os omitted)"}', ["zsh", "25.6.0"]),
    ("G1: \\u000a escape", "x\\u000a - Shell: zsh\\u000A - OS Version: Darwin 25.6.0",
     "x\\u000a - Shell: (omitted)\\u000A - OS Version: (os omitted)", ["zsh", "25.6.0"]),
    ("G1: carriage return", "x\r - Shell: zsh\rOS Version: Darwin 25.6.0",
     "x\r - Shell: (omitted)\rOS Version: (os omitted)", ["zsh", "25.6.0"]),
    ("G1: escaped carriage return", "x\\r - Shell: zsh", "x\\r - Shell: (omitted)", ["zsh"]),
    # Review round 2 G2: the dash form for any value; the dashless form only with a capital first
    # letter and a host value. Prose and a YAML key are kept.
    ("G2: dash form, any value", " - Shell: anything at all", " - Shell: (omitted)", ["anything"]),
    ("G2: prose shell line kept", "Shell: run make first", "Shell: run make first", []),
    ("G2: prose OS line kept", "OS version: check it is 14 or later", "OS version: check it is 14 or later", []),
    ("G2: YAML shell key kept", "x\n  shell: bash\n", "x\n  shell: bash\n", []),
    ("G2: dashless shell path", "Shell: /bin/zsh\nShell: /opt/homebrew/bin/fish",
     "Shell: (omitted)\nShell: (omitted)", ["/bin/zsh", "fish"]),
    ("G2: dashless OS values", "OS Version: macOS 15.1\nOS Version: Windows 10.0.26100",
     "OS Version: (os omitted)\nOS Version: (os omitted)", ["15.1", "26100"]),
    # Review round 2 G5: the key's own placeholder as the whole value; CRLF; anchored Stainless values.
    ("G5: wrong placeholder on the shell line", " - Shell: (os omitted)", " - Shell: (omitted)", []),
    ("G5: placeholder then text", " - Shell: (omitted) zsh", " - Shell: (omitted)", ["zsh"]),
    ("G5: placeholder in upper case", " - OS Version: (OS OMITTED)", " - OS Version: (os omitted)", []),
    ("G5: CRLF", "x\r\n - Shell: zsh\r\n - OS Version: Darwin 25.6.0\r\n",
     "x\r\n - Shell: (omitted)\r\n - OS Version: (os omitted)\r\n", ["zsh", "25.6.0"]),
    ("G5: Stainless wrong placeholder", '"x-stainless-arch": "(version omitted)"', '"x-stainless-arch": "(arch omitted)"',
     []),
    ("G5: Stainless placeholder then text", '"x-stainless-runtime-version": "(version omitted)v26"',
     '"x-stainless-runtime-version": "(version omitted)"', ["v26"]),
    ("G5: Stainless CRLF header line", "x-stainless-arch: arm64\r\nx-stainless-arch: (arch omitted)\r\n",
     "x-stainless-arch: (arch omitted)\r\nx-stainless-arch: (arch omitted)\r\n", ["arm64"]),
    # Review round 2 G8: quoted values and no-break spaces.
    ("G8: quoted value", ' - Shell: "zsh"\nShell: "bash"', ' - Shell: "(omitted)"\nShell: "(omitted)"', ["zsh", "bash"]),
    ("G8: escaped quoted value", 'x\\n - Shell: \\"zsh\\"', 'x\\n - Shell: \\"(omitted)\\"', ["zsh"]),
    ("G8: no-break space", " - Shell: zsh\n - OS Version:\\u00a0Darwin 25.6.0",
     " - Shell: (omitted)\n - OS Version:\\u00a0(os omitted)", ["zsh", "25.6.0"]),
    # Review round 1 F3: the Stainless SDK's architecture and runtime version headers.
    ("stainless arch and runtime version", '"x-stainless-arch": "arm64", "X-Stainless-Runtime-Version": "v26.12.1"',
     '"x-stainless-arch": "(arch omitted)", "X-Stainless-Runtime-Version": "(version omitted)"', ["arm64", "26.12.1"]),
    ("stainless headers, escaped", '{\\"x-stainless-arch\\":\\"x64\\",\\"x-stainless-runtime-version\\":\\"v22.1.0\\"}',
     '{\\"x-stainless-arch\\":\\"(arch omitted)\\",\\"x-stainless-runtime-version\\":\\"(version omitted)\\"}',
     ["x64", "22.1.0"]),
    ("stainless header line", "x-stainless-arch: arm64\nx-stainless-os: MacOS",
     "x-stainless-arch: (arch omitted)\nx-stainless-os: MacOS", ["arm64"]),
    ("stainless placeholders kept", '"x-stainless-arch": "(arch omitted)", "x-stainless-runtime-version": "(version omitted)"',
     '"x-stainless-arch": "(arch omitted)", "x-stainless-runtime-version": "(version omitted)"', []),
    # Review round 1 F7: the process id in a Claude Code messaging socket path.
    ("messaging socket pid", "/tmp/cc-socks/95709.sock", "/tmp/cc-socks/<pid>.sock", ["95709"]),
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


def scrub_env(path, env):
    with tempfile.TemporaryDirectory() as run:
        return subprocess.run([sys.executable, "-I", SCRUB, run, path], check=True, capture_output=True,
                              text=True, env=dict(os.environ, **env)).stdout


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
        # PC-11: descriptions of tools inside a namespace tool; parameter descriptions kept.
        t = os.path.join(d, "ns.json")
        with open(t, "w") as f:
            json.dump({"body": {"tools": [{"type": "namespace", "name": "multi_agent_v1", "description": "d", "tools": [
                {"type": "function", "name": "spawn_agent", "description": "spawn-desc", "parameters": {
                    "type": "object", "properties": {"agent_type": {"type": "string", "description": "role list"}}}}]}]}}, f)
        got = json.loads(scrub(t))
        ns = got["body"]["tools"][0]
        inner = ns["tools"][0]
        if (ns["description"] != "[omitted]" or inner["description"] != "[omitted]"
                or inner["parameters"]["properties"]["agent_type"]["description"] != "role list"):
            bad.append("namespace tool descriptions: got %r" % (got,))
        # PC-11: a harness tool listing (a top-level list of tools) gets its descriptions omitted.
        listing = [{"type": "function", "name": "a", "description": "x", "parameters": {}},
                   {"type": "namespace", "name": "n", "description": "y",
                    "tools": [{"type": "function", "name": "b", "description": "z", "parameters": {}}]},
                   {"type": "web_search"}]
        t = os.path.join(d, "tools.json")
        with open(t, "w") as f:
            json.dump(listing, f)
        got = json.loads(scrub(t))
        want = json.loads(json.dumps(listing).replace('"x"', '"[omitted]"').replace('"y"', '"[omitted]"')
                          .replace('"z"', '"[omitted]"'))
        if got != want:
            bad.append("tool listing descriptions: got %r" % (got,))
        # PC-11: a list that is not a tool listing keeps its descriptions.
        t = os.path.join(d, "events.json")
        with open(t, "w") as f:
            json.dump([{"event": "x", "description": "kept"}], f)
        got = json.loads(scrub(t))
        if got != [{"event": "x", "description": "kept"}]:
            bad.append("non-listing list kept: got %r" % (got,))
        # PC-11: a second scrub of the PC-11 inputs changes nothing.
        t = os.path.join(d, "idem.txt")
        with open(t, "w") as f:
            f.write("\n".join(c[1] for c in PC11) + "\n")
        a = scrub(t)
        t2 = os.path.join(d, "idem2.txt")
        with open(t2, "w") as f:
            f.write(a)
        if scrub(t2) != a:
            bad.append("pc-11 rules idempotent: a second scrub changes the text")
        # Review round 1 F6: in a captured JSON object, only an exec command field loses its shell.
        t = os.path.join(d, "exec.json")
        with open(t, "w") as f:
            json.dump({"item": {"type": "command_execution", "command": "/bin/zsh -lc 'x'",
                                "aggregated_output": "/bin/bash -lc y"}}, f)
        got = json.loads(scrub(t))["item"]
        if got["command"] != "(shell omitted) -lc 'x'" or got["aggregated_output"] != "/bin/bash -lc y":
            bad.append("exec command field: got %r" % (got,))
        # Review round 1 F2: one id map per capture folder (SCRUB_IDMAP, shared by the folder's files):
        # the same id gets the same placeholder in two files, different ids get different placeholders.
        with tempfile.TemporaryDirectory() as m:
            mp = os.path.join(m, "ids.json")
            a, b = os.path.join(d, "ida.json"), os.path.join(d, "idb.json")
            with open(a, "w") as f:
                json.dump({"x": "msg_" + I1}, f)
            with open(b, "w") as f:
                json.dump({"x": "msg_" + I2, "y": "msg_" + I1, "z": "fco_" + I2}, f)
            got = [json.loads(scrub_env(p, {"SCRUB_IDMAP": mp})) for p in (a, b)]
        want = [{"x": "msg_placeholder0001"},
                {"x": "msg_placeholder0002", "y": "msg_placeholder0001", "z": "fco_placeholder0001"}]
        if got != want:
            bad.append("item ids, one map per folder: got %r" % (got,))
        # Review round 1 F3: time-bearing ULIDs of response and item ids (resp_<ulid>, msg_<ulid>_<n>,
        # fc_<ulid>_<n>) -> a ULID of time 0, one map per folder; a response and its items stay paired.
        u = ["01" + "ABCDEFGHJK" + "MNPQRSTVWXYZ" + "0" + c for c in "123"]
        p = ["0" * 22 + "%04d" % n for n in (1, 2, 3)]
        with tempfile.TemporaryDirectory() as m:
            mp = os.path.join(m, "ids.json")
            a, b = os.path.join(d, "ua.sse"), os.path.join(d, "ub.sse")
            with open(a, "w") as f:
                f.write('data: {"id":"resp_%s","item":{"id":"msg_%s_0"}} fc_%s_1\n' % (u[0], u[0], u[1]))
            with open(b, "w") as f:
                f.write('{\\"id\\":\\"msg_%s_0\\"} resp_%s\n' % (u[0], u[2]))
            got = [scrub_env(x, {"SCRUB_IDMAP": mp}) for x in (a, b)]
            with open(a, "w") as f:
                f.write(got[0])
            again = scrub_env(a, {"SCRUB_IDMAP": mp})
        want = ['data: {"id":"resp_%s","item":{"id":"msg_%s_0"}} fc_%s_1\n' % (p[0], p[0], p[1]),
                '{\\"id\\":\\"msg_%s_0\\"} resp_%s\n' % (p[0], p[2])]
        if got != want or again != want[0]:
            bad.append("ulid ids, one map per folder: got %r, again %r" % (got, again))
        # Review round 1 F5: SCRUB_FIXED=1 takes the user name from SCRUB_USER and the host names from
        # SCRUB_HOSTS, not from the machine, so a check gives the same result on every machine.
        t = os.path.join(d, "fixed.txt")
        with open(t, "w") as f:
            f.write("zzuserq on zzhostq; %s on %s\n" % (user, host))
        got = scrub_env(t, {"SCRUB_FIXED": "1", "SCRUB_USER": "zzuserq", "SCRUB_HOSTS": "zzhostq"})
        if got != "user on host; %s on %s\n" % (user, host):
            bad.append("fixed user and host names: output not as wanted [not shown]")
        # Review round 2 G4: the user-name rule is case-insensitive and matches whole words only;
        # a user name of "user" (any case) changes nothing.
        with open(t, "w") as f:
            f.write("Zzuserq ZZUSERQ zzuserqx x_zzuserq User\n")
        got = [scrub_env(t, {"SCRUB_FIXED": "1", "SCRUB_USER": n, "SCRUB_HOSTS": ""}) for n in ("zzuserq", "user")]
        if got != ["user user zzuserqx x_zzuserq User\n", "Zzuserq ZZUSERQ zzuserqx x_zzuserq User\n"]:
            bad.append("user name any case, whole word: got %r" % (got,))
        # Review round 2 G2: a Codex item id in upper-case hex is scrubbed like a lower-case one.
        t = os.path.join(d, "upper.json")
        with open(t, "w") as f:
            json.dump({"x": "msg_" + I1.upper(), "y": "fco_" + I1}, f)
        got = json.loads(scrub(t))
        if got != {"x": "msg_placeholder0001", "y": "fco_placeholder0001"}:
            bad.append("upper-case item id: got %r" % (got,))
        # PC-13 review round 1 F3: Stainless headers of a captured request (keys of a parsed object, any
        # case); the OS family header is kept.
        t = os.path.join(d, "stainless.json")
        with open(t, "w") as f:
            json.dump({"headers": {"x-stainless-arch": "arm64", "X-Stainless-Runtime-Version": "v26.12.1",
                                   "x-stainless-os": "MacOS"}}, f)
        got = json.loads(scrub(t))
        if got != {"headers": {"x-stainless-arch": "(arch omitted)", "X-Stainless-Runtime-Version": "(version omitted)",
                               "x-stainless-os": "MacOS"}}:
            bad.append("stainless header keys: got %r" % (got,))
    for b in bad:
        print("scrub test fail: " + b)
    if bad:
        sys.exit(1)
    print("scrub tests: ok (%d)" % (len(CASES) + 17))


main()
