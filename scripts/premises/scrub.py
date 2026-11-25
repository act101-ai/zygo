#!/usr/bin/env python3
r"""Fixture hygiene for N2: scrub.py <run dir> <file> -> scrubbed copy on stdout.

- Paths of the run: <run>/home -> /home/user, <run>/repo -> /repo, <run> -> /tmp/run
  (both the /tmp and /private/tmp forms, and /var/folders, /private/var temp paths).
- The zygo checkout holding this script -> /zygo; the operator's home (/Users/<name>) ->
  /home/user; the operator's user name -> user.
- A captured request (an object with "body"): body.system -> "[omitted]", each tool
  "description" -> "[omitted]"; messages kept as captured.
- UUIDs (session, prompt ids) and OpenCode ses_ ids -> numbered placeholders per file;
  a request's body.metadata.user_id (device id) -> "[omitted: ...]".
- authorization / x-api-key header values -> "[omitted]"; e-mail addresses -> "[e-mail omitted]";
  any "Bearer <word>" -> "Bearer [omitted]" (also in quoted schema text).
- The agent's commit-attribution reminder text -> "Harness text." (the <system-reminder> tags stay).
- Key forms (N38): any "sk-<4+ chars>" (OpenAI-style keys, masked fragments) -> "[key omitted]";
  "org-<8+>" -> "[org omitted]"; "proj_<8+>" -> "[project omitted]"; zygo tokens "zyg_..." ->
  "[zygo token omitted]"; a Windows user path (C:\\Users\\<name>, JSON-escaped, \ or /; the name may
  hold spaces) -> /home/user.
- Round 2 (N38): GitHub tokens (ghp_, gho_, ghs_, ghu_, ghr_, github_pat_) -> "[github token omitted]";
  JWTs (three base64url parts, the first starting eyJ) -> "[jwt omitted]"; the machine's host names
  (hostname, full and short, and on macOS scutil LocalHostName and ComputerName; any case) -> "host",
  a name shorter than 4 characters or "localhost" skipped with a note on stderr; a plain-text
  "x-api-key: <value>" (or "=", quoted, backslash-quoted, any case) -> "x-api-key: [omitted]".
- Round 3 (N38): the key, id, token, user and host rules also match right after a backslash escape
  (\n, \t, \r, \b, \f, \uXXXX) in nested JSON text. A Windows user path (C:\Users\<name>, any case;
  /c/Users/<name>) ends at the name: a name with spaces only when a separator follows it within
  40 characters, else the first word.
- Round 4 (N38): a name with spaces that ends the text, or is followed by a quote or a line end
  within 40 characters, is removed whole (C:\Users\Jo Ann -> /home/user). Host-name edges: "-" and
  "_" are part of a name, as in check-premises.sh.
- PC-10: the OS name, version and architecture in a user agent ("product/1.2 (Mac OS 27.2.1; arm64)",
  "(Linux 6.8.0; x86_64)", "(Windows 10.0.26100)") -> "(os omitted)". A run path with a doubled
  "/" (a TMPDIR that ends in "/", as in .../T//tmp.X) maps like the plain path.
- PC-10 review round 1: a user agent's parenthesis that names an OS (macOS, a Linux distribution,
  a Windows edition, a browser's "Macintosh; Intel Mac OS X 10_15_7"; with or without a blank after
  the product) -> "(os omitted)", and that Mac OS X text alone; os_type, os_version, os_arch,
  os_name and os_release values (JSON keys, and in JSON text) -> "(os omitted)"; uname -a lines
  (Darwin ... Darwin Kernel Version ..., Linux <host> <version> #<n> ...) -> "(os omitted)"; a
  <timezone> other than UTC or Etc/UTC -> "<timezone>(omitted)</timezone>" (a backstop: codex_env
  sets TZ=UTC); Claude's dashed path form of a run path with "//" ("--") maps like the plain form.
"""
import getpass
import json
import os
import re
import socket
import subprocess
import sys

run, path = sys.argv[1], sys.argv[2]
run = os.path.realpath(run)
user = getpass.getuser()
home = os.path.expanduser("~")


def host_names():
    names = [socket.gethostname()]
    for k in ("LocalHostName", "ComputerName"):
        try:
            names.append(subprocess.run(["scutil", "--get", k], capture_output=True, text=True,
                                        timeout=5).stdout.strip())
        except (OSError, subprocess.SubprocessError):
            pass
    out = set()
    for n in names:
        for h in (n, n.split(".")[0]):
            if not h:
                continue
            if len(h) < 4 or h.lower() == "localhost":
                sys.stderr.write("scrub: a host name shorter than 4 characters or localhost; not scrubbed\n")
                continue
            out.add(h)
    return sorted(out, key=len, reverse=True)


# The machine's host names, full and short.
HOSTS = host_names()
# Left boundary of a key, id or name: no word character before it, or a JSON backslash escape
# (\n, \t, \r, \b, \f, \uXXXX) right before it in nested JSON text.
B = r"(?:(?<![A-Za-z0-9_])|(?<=\\[nrtbf])|(?<=\\u[0-9A-Fa-f]{4}))"
# A Windows user folder name: with spaces when a separator, a quote, a line end or the end of the
# text follows within 40 characters, else one word.
WNAME = r"(?:[^\\/\"'\r\n\t]{1,40}?(?=[\\/\"'\r\n\t]|\Z)|[^\\/\"'\s]+)"


def forms(p):
    out = {p}
    for a, b in (("/private/tmp/", "/tmp/"), ("/private/var/", "/var/")):
        if p.startswith(a):
            out.add(b + p[len(a):])
        if p.startswith(b):
            out.add(a + p[len(b):])
    return sorted(out, key=len, reverse=True)


PAIRS = []
for sub, repl in (("home", "/home/user"), ("repo", "/repo"), ("", "/tmp/run")):
    src = os.path.join(run, sub) if sub else run
    for f in forms(src):
        PAIRS.append((f, repl))
# The zygo checkout that holds these scripts -> /zygo (before the home rule).
PAIRS.append((os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "/zygo"))
PAIRS.append((home, "/home/user"))
# Claude Code also names directories after a path with every other character as "-".
for src, repl in ((os.path.join(run, "repo"), "-repo"), (os.path.join(run, "home"), "-home-user"), (run, "-tmp-run")):
    for f in forms(src):
        PAIRS.append((re.sub(r"[^A-Za-z0-9]", "-", f), repl))


# Session, prompt and device ids -> fixed placeholders, numbered per file (same id, same placeholder).
UUID = re.compile(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
SES = re.compile(r"\bses_[A-Za-z0-9]{20,}")
IDS = {}


def ids(kind, v):
    if v not in IDS:
        n = sum(1 for k in IDS.values() if k[0] == kind) + 1
        IDS[v] = (kind, "00000000-0000-4000-8000-%012d" % n if kind == "uuid" else "ses_placeholder%04d" % n)
    return IDS[v][1]


# Each path pair also matches with any "/" doubled (a TMPDIR that ends in "/": .../T//tmp.X), and
# a dashed pair with any "-" doubled (the same path in Claude's dashed form).
PAIRS = [(re.compile(re.escape(a).replace("/", "/+").replace("\\-", "-+")), b) for a, b in PAIRS]
# The host OS in a user agent: "<product>/<version>[ ](... <an OS name> ...)".
OS_NAMES = (r"(?:Mac OS|macOS|Macintosh|Darwin|Linux|Ubuntu|Debian|Fedora|CentOS|Red Hat|Alpine|Arch|"
            r"openSUSE|SUSE|Gentoo|NixOS|Manjaro|Mint|Windows|FreeBSD|OpenBSD|NetBSD|Android|iOS|CYGWIN|"
            r"MINGW|MSYS)")
UA_OS = re.compile(r"(\S/[0-9][^\s/()]*)( ?)\((?=[^()]*\b%s\b)[^()]*\)" % OS_NAMES)
MAC_OS_X = re.compile(r"(?:Macintosh; )?(?:Intel |PPC )?Mac OS X [0-9][0-9_.]*")
OS_KEYS = ("os_type", "os_version", "os_arch", "os_name", "os_release")
OS_JSON = re.compile(r"((?:\\?\")(?:%s)(?:\\?\")\s*:\s*(?:\\?\"))[^\"\\]*" % "|".join(OS_KEYS))
UNAME = re.compile(r"\b(?:Darwin \S+ [0-9]+\.[0-9]+\.[0-9]+ Darwin Kernel Version|Linux \S+ [0-9]+\.[0-9]+\.[0-9]+\S* #[0-9]+)[^\n\"\\]*")
TIMEZONE = re.compile(r"<timezone>(?!(?:UTC|Etc/UTC)</timezone>)[^<]*</timezone>")


def s(text):
    for a, b in PAIRS:
        text = a.sub(lambda m: b, text)
    text = UA_OS.sub(lambda m: m.group(1) + m.group(2) + "(os omitted)", text)
    text = MAC_OS_X.sub("(os omitted)", text)
    text = OS_JSON.sub(lambda m: m.group(1) + "(os omitted)", text)
    text = UNAME.sub("(os omitted)", text)
    text = TIMEZONE.sub("<timezone>(omitted)</timezone>", text)
    text = re.sub(r"(/private)?/var/folders/[^\s\"'`)]*", "/tmp/x", text)
    text = re.sub(r"/private/var/[^\s\"'`)]*", "/tmp/x", text)
    text = re.sub(r"(?i)[A-Za-z]:(?:\\\\|\\|/)Users(?:\\\\|\\|/)" + WNAME, lambda m: "/home/user", text)
    text = re.sub(r"(?i)(?<![A-Za-z0-9_.-])/[a-z]/Users/" + WNAME, lambda m: "/home/user", text)
    text = re.sub(r"/Users/[^/\s\"']+", "/home/user", text)
    text = re.sub(r"/private/tmp/", "/tmp/", text)
    for h in HOSTS:  # before the user rule: a host name may hold the user name
        text = re.sub(r"(?:(?<![A-Za-z0-9_-])|(?<=\\[nrtbf]))%s(?![A-Za-z0-9_-])" % re.escape(h), "host", text, flags=re.I)
    text = re.sub(B + r"%s\b" % re.escape(user), "user", text)
    text = re.sub(r"Attribution for git commits.*?(?=</system-reminder>)", "Harness text.\n", text, flags=re.S)
    text = UUID.sub(lambda m: ids("uuid", m.group(0)), text)
    text = SES.sub(lambda m: ids("ses", m.group(0)), text)
    text = re.sub(r"eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*", lambda m: "[jwt omitted]", text)
    text = re.sub(r"Bearer [A-Za-z0-9][^\s\"'\\]*", "Bearer [omitted]", text)
    text = re.sub(r"(?i)(x-api-key(?:\\?[\"'])?\s*[:=]\s*(?:\\?[\"'])?)(?!\[omitted\])[^\s\"',\\]+", lambda m: m.group(1) + "[omitted]", text)
    text = re.sub(B + r"(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})", lambda m: "[github token omitted]", text)
    text = re.sub(B + r"sk-[A-Za-z0-9_*.-]{4,}", lambda m: "[key omitted]", text)
    text = re.sub(B + r"org-[A-Za-z0-9]{8,}", lambda m: "[org omitted]", text)
    text = re.sub(B + r"proj_[A-Za-z0-9]{8,}", lambda m: "[project omitted]", text)
    text = re.sub(r"zyg_[A-Za-z0-9_]+", lambda m: "[zygo token omitted]", text)
    text = re.sub(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}", "[e-mail omitted]", text)
    return text


def walk(o):
    if isinstance(o, dict):
        return {s(k): "(os omitted)" if k in OS_KEYS and isinstance(v, str) else walk(v) for k, v in o.items()}
    if isinstance(o, list):
        return [walk(v) for v in o]
    if isinstance(o, str):
        return s(o)
    return o


raw = open(path, encoding="utf-8", errors="replace").read()
try:
    data = json.loads(raw)
except ValueError:
    sys.stdout.write(s(raw))
    sys.exit(0)

if isinstance(data, dict) and "headers" in data and isinstance(data.get("headers"), dict):
    for k in list(data["headers"]):
        if k.lower() in ("authorization", "x-api-key"):
            data["headers"][k] = "[omitted]"
if isinstance(data, dict) and isinstance(data.get("body"), dict):
    b = data["body"]
    if isinstance(b.get("metadata"), dict) and "user_id" in b["metadata"]:
        b["metadata"]["user_id"] = "[omitted: device, account and session ids]"
    if "system" in b:
        b["system"] = "[omitted]"
    for t in b.get("tools") or []:
        if isinstance(t, dict) and "description" in t:
            t["description"] = "[omitted]"
json.dump(walk(data), sys.stdout, indent=2, ensure_ascii=False)
sys.stdout.write("\n")
