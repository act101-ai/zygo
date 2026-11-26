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
  <timezone> other than UTC -> "<timezone>(omitted)</timezone>" (a backstop: codex_env sets TZ=UTC;
  the forms and UTC names of codex_record's check); Claude's dashed path form of a run path with "//" ("--") maps like the plain form.
- PC-11: Codex's platformOs and platformFamily values (JSON keys, and in JSON text) -> "(os omitted)";
  a <shell> element (the forms of <timezone>) -> "<shell>(omitted)</shell>"; a bare /private/tmp
  writable root -> /tmp; Codex item ids msg_, fco_, ctco_ + UUID -> "<prefix>_placeholder<n>",
  numbered per prefix per file; a "sandbox" value that names an OS sandbox (seatbelt, landlock,
  seccomp, bwrap, windows sandbox names; plain or \"-escaped) -> "(os omitted)"; marketplace-<os>-<id>
  -> marketplace-os-omitted; a login shell path before " -lc" -> "(shell omitted) -lc"; macOS __CF*
  env names, with an "=<value>", -> "(os env omitted)"; each tool "description" in a request body,
  also of tools nested in a namespace tool, and in a top-level tool listing (a list of tools, as in
  the harness listing files) -> "[omitted]"; parameter descriptions kept.
- PC-11 review round 1: SCRUB_IDMAP=<file>: the Codex item ids and the ULIDs below share one map
  with every other scrub that names the same file (one map per capture folder or part): the same id
  gets the same placeholder in every file, different ids always different ones (without it, one map
  per file). A time-bearing ULID after a lower-case prefix (resp_<ulid>, msg_<ulid>_0, fc_<ulid>_1)
  -> a ULID of time 0, "0"*22 + "<n>" (4 digits), so a response and its items stay paired. The
  login shell is masked only in an exec command field ("command", "cmd") and in the <shell> tag of an
  environment_context block. SCRUB_FIXED=1: the user name from SCRUB_USER (default "user"), the host
  names from SCRUB_HOSTS (blank-separated, default none), and no rule for the home or checkout path,
  so the result does not depend on the machine.
- PC-11 review round 2: the user-name rule matches the name in any case, as a whole word (a user
  name of "user", any case, changes nothing); a Codex item id matches in any case (msg_, fco_, ctco_
  + UUID; the same id in two cases gets one placeholder).
- PC-13: the shell name in a Claude Code environment block line (" - Shell: <name>", at a line start
  or after a JSON \n escape) -> "(omitted)"; the " - OS Version: <os> <version>" line ->
  "(os omitted)"; a "shell" JSON key and prose are kept.
- PC-13 review round 1: those lines in any case, with or without the leading "- ", with blanks or
  escaped tabs after the colon, at a line start or after a JSON \n escape (not after an escaped
  backslash then "n"); x-stainless-arch -> "(arch omitted)" and x-stainless-runtime-version ->
  "(version omitted)" (keys of a parsed object, JSON text and header lines; any case; x-stainless-os,
  the OS family, is kept, as is "Platform: darwin"); /cc-socks/<n>.sock -> /cc-socks/<pid>.sock.
- PC-13 review round 2: a line start is also a carriage return, any run of backslashes then "n" or
  "r", or a \u000a escape (fail closed); a dashless "Shell:" or "OS Version:" line (capital first
  letter) is masked only for a host value; quoted values and no-break spaces; a Stainless value is
  replaced whole by its own placeholder (see ENV_NL and STAINLESS_RE).
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
FIXED = os.environ.get("SCRUB_FIXED") == "1"
user = (os.environ.get("SCRUB_USER") or "user") if FIXED else getpass.getuser()
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
HOSTS = sorted(set(os.environ.get("SCRUB_HOSTS", "").split()), key=len, reverse=True) if FIXED else host_names()
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
if not FIXED:
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
# The kinds that SCRUB_IDMAP shares across files: Codex item ids and ULIDs.
SHARED = ("msg", "fco", "ctco", "ulid")
IDMAP = os.environ.get("SCRUB_IDMAP", "")
if IDMAP and os.path.exists(IDMAP):
    with open(IDMAP, encoding="utf-8") as _f:
        IDS.update((k, tuple(v)) for k, v in json.load(_f).items() if v[0] in SHARED)


def ids(kind, v):
    if v not in IDS:
        n = sum(1 for k in IDS.values() if k[0] == kind) + 1
        IDS[v] = (kind, "00000000-0000-4000-8000-%012d" % n if kind == "uuid" else
                  "0" * 22 + "%04d" % n if kind == "ulid" else "%s_placeholder%04d" % (kind, n))
    return IDS[v][1]


def save_ids():
    if not IDMAP:
        return
    tmp = "%s.%d" % (IDMAP, os.getpid())
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump({k: list(v) for k, v in IDS.items() if v[0] in SHARED}, f)
    os.replace(tmp, IDMAP)


# Each path pair also matches with any "/" doubled (a TMPDIR that ends in "/": .../T//tmp.X), and
# a dashed pair with any "-" doubled (the same path in Claude's dashed form).
PAIRS = [(re.compile(re.escape(a).replace("/", "/+").replace("\\-", "-+")), b) for a, b in PAIRS]
# The host OS in a user agent: "<product>/<version>[ ](... <an OS name> ...)".
OS_NAMES = (r"(?:Mac OS|macOS|Macintosh|Darwin|Linux|Ubuntu|Debian|Fedora|CentOS|Red Hat|Alpine|Arch|"
            r"openSUSE|SUSE|Gentoo|NixOS|Manjaro|Mint|Windows|FreeBSD|OpenBSD|NetBSD|Android|iOS|CYGWIN|"
            r"MINGW|MSYS)")
UA_OS = re.compile(r"(\S/[0-9][^\s/()]*)( ?)\((?=[^()]*\b%s\b)[^()]*\)" % OS_NAMES)
MAC_OS_X = re.compile(r"(?:Macintosh; )?(?:Intel |PPC )?Mac OS X [0-9][0-9_.]*")
OS_KEYS = ("os_type", "os_version", "os_arch", "os_name", "os_release", "platformOs", "platformFamily")
OS_JSON = re.compile(r"((?:\\?\")(?:%s)(?:\\?\")\s*:\s*(?:\\?\"))[^\"\\]*" % "|".join(OS_KEYS))
UNAME = re.compile(r"\b(?:Darwin \S+ [0-9]+\.[0-9]+\.[0-9]+ Darwin Kernel Version|Linux \S+ [0-9]+\.[0-9]+\.[0-9]+\S* #[0-9]+)[^\n\"\\]*")
# A <timezone> element in the forms lib.sh's codex_record checks: plain, JSON < > escapes or
# &lt; &gt; entities, \/ slashes; UTC in any case, Etc/UTC, GMT, Etc/GMT, UTC+0, UTC+00:00 kept.
TIMEZONE = re.compile(r"((?:<|\\u003[cC]|&lt;)timezone(?:>|\\u003[eE]|&gt;))((?:\\/|[^<&\\\n]){1,200})"
                      r"((?:<|\\u003[cC]|&lt;)\\?/timezone)")
UTC_NAMES = {"utc", "etc/utc", "gmt", "etc/gmt", "utc+0", "utc+00:00"}
# PC-11: the login shell in a <shell> element (the same bracket forms as <timezone>).
SHELL_TAG = re.compile(r"((?:<|\\u003[cC]|&lt;)shell(?:>|\\u003[eE]|&gt;))(?:\\/|[^<&\\\n]){1,200}"
                       r"((?:<|\\u003[cC]|&lt;)\\?/shell)")
# PC-11: Codex item ids (UUIDv7, time-bearing); "_" is a word character, so UUID does not match them.
ITEM_ID = re.compile(B + r"(msg|fco|ctco)_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", re.I)
# PC-11: an OS sandbox name as a "sandbox" value (x-codex-turn-metadata, plain or JSON-escaped).
SANDBOX = re.compile(r"(\\?\"sandbox\\?\"\s*:\s*\\?\")(?:seatbelt|landlock|linux_seccomp|seccomp|bwrap|bubblewrap|"
                     r"windows_sandbox|restricted_token|windows_restricted_token|elevated)(?=\\?\")")
MARKETPLACE = re.compile(r"marketplace-(?:macos|linux|windows)-[A-Za-z0-9_-]+")
# PC-11: the owner's login shell at the start of an exec command field ("command":"/bin/zsh -lc '...'",
# plain or JSON-escaped; in a parsed object, LEAD_SHELL on a "command" or "cmd" value). Elsewhere kept.
SHELL_PATH = r"(?:/[^\s\"'\\/]+)*/(?:zsh|bash|fish|sh|dash|ksh|mksh|tcsh|csh) -lc\b"
CMD_KEYS = ("command", "cmd")
CMD_SHELL = re.compile(r"(\\?\"(?:%s)\\?\"\s*:\s*\\?\")%s" % ("|".join(CMD_KEYS), SHELL_PATH))
LEAD_SHELL = re.compile(r"^" + SHELL_PATH)
# PC-11: an environment_context block (the bracket forms of <timezone>), where SHELL_TAG applies.
ENV_CTX = re.compile(r"(?:<|\\u003[cC]|&lt;)environment_context(?:>|\\u003[eE]|&gt;).*?"
                     r"(?:(?:<|\\u003[cC]|&lt;)\\?/environment_context|\Z)", re.S)
# Review round 1 F3: a time-bearing ULID after a lower-case id prefix (resp_, msg_, fc_, rs_); a
# placeholder (time 0) is not matched again.
ULID = re.compile(B + r"([a-z]{1,8}_)(?!0{10})([0-7][0-9A-HJKMNP-TV-Z]{25})(?![0-9A-Za-z])")
# PC-11: macOS __CF* env names (__CF_USER_TEXT_ENCODING=0x<uid>:... holds the user id).
CF_ENV = re.compile(B + r"__CF[A-Za-z_]*(?:=[^\s\"'\\]*)?")
# PC-13: the shell name and OS version lines of a Claude Code environment block (" - Shell: zsh",
# " - OS Version: Darwin 25.6.0"). The same forms as rescrub_codex.py ENV_LEAKS and check-premises.sh
# env_scan. Review round 2 G1 (fail closed): a line start is the text start, a real line feed or
# carriage return, one or more backslashes then "n" or "r" (any depth of JSON escaping; a literal
# backslash then "n" is a false positive, accepted), or a \u000a or \u000d escape (any case). Blanks or
# escaped tabs may follow it. G2: the dash form ("- Shell: <v>", the key in any case) is masked for any
# value; the dashless form only with a capital first letter ("Shell:", "OS Version:"; a YAML
# "shell: bash" key is kept) and a host value: a known shell name or its path, or Darwin, Linux, Windows
# or macOS then a version digit. G8: blanks, escaped tabs or no-break spaces after the colon, and a
# quoted or \"-quoted value.
ENV_NL = r"(?:^|\r|(?<!\\)\\+[nr]|\\[uU]000[aAdD])"
ENV_IND = r"(?:[ \t]|\\t)*"
ENV_WS = r"(?:[ \t\u00a0]|\\t|\\u00[aA]0)*"
ENV_Q = r"(?:\\?[\"'])?"
ENV_END = r"(?:$|[\r\n\\\"'])"
# Review round 3 H3: more shell names and paths (Linuxbrew, NixOS, the Nix store, a Windows path to a
# shell .exe, plain or with escaped backslashes) and OS names (Windows_NT, "Windows ", distributions).
SHELL_DIR = (r"(?:(?:/usr(?:/local)?)?/bin/|/opt/homebrew/bin/|/home/linuxbrew/\.linuxbrew/bin/|"
             r"/run/current-system/sw/bin/|/nix/store/[0-9a-z]+-[^/\s\"'\\]+/bin/)")
WIN_SHELL = r"[a-z]:(?:\\{1,2}[^\\\r\n\"']+)*?\\{1,2}(?:bash|pwsh|powershell|cmd)\.exe"
SHELL_VAL = (r"(?i:" + SHELL_DIR + r"?(?:zsh|bash|fish|sh|tcsh|csh|ksh|mksh|dash|nu|xonsh|pwsh|powershell|cmd)"
             r"(?:\.exe)?|" + WIN_SHELL + r")(?![A-Za-z0-9_./-])")
OS_VAL = r"(?:(?i:darwin|linux|windows|macos|ubuntu|debian|fedora|freebsd|openbsd|nixos)[ \t]*[0-9]|(?i:windows)[_ ])"
SHELL_KEY = r"(?:-" + ENV_WS + r"(?i:shell)|S(?i:hell)(?=:" + ENV_WS + ENV_Q + SHELL_VAL + r"))"
OS_KEY = r"(?:-" + ENV_WS + r"(?i:os version)|O(?i:s version)(?=:" + ENV_WS + ENV_Q + OS_VAL + r"))"
# A Windows shell path is replaced whole (its backslashes are not line ends).
ENV_SHELL = re.compile(r"(?m)(" + ENV_NL + ENV_IND + SHELL_KEY + ":" + ENV_WS + ENV_Q + r")(?:(?i:" + WIN_SHELL +
                       r")(?![A-Za-z0-9_./-])|[^\s\\\"'][^\n\r\\\"']*)")
ENV_OS = re.compile(r"(?m)(" + ENV_NL + ENV_IND + OS_KEY + ":" + ENV_WS + ENV_Q + r")[^\s\\\"'][^\n\r\\\"']*")
# PC-13 review round 1 F3: the Stainless SDK's architecture and runtime version request headers (a
# JSON key, plain or \"-escaped, or a header line; any case); x-stainless-os (the OS family) is kept.
# Review round 2 G5: the whole value (a token, or a parenthesis then a token) becomes the key's own
# placeholder, so a wrong placeholder or one with text after it is rewritten. Review round 3 H1: a
# quoted value (plain or \"-escaped) is replaced up to its closing quote; an unquoted one up to the line
# end, a quote or a backslash (a header line inside a JSON string); a "," or a blank does not end it, so
# text after a value is lost (fails closed). Groups: 1 the key and separator, 2 the key, 3 the opening
# quote.
STAINLESS = {"x-stainless-arch": "(arch omitted)", "x-stainless-runtime-version": "(version omitted)"}
STAINLESS_RE = re.compile(r"(\\?[\"']?((?i:x-stainless-(?:arch|runtime-version)))\\?[\"']?\s*[:=]\s*)"
                          r"(?:(\\?[\"'])(?:(?!\3)(?:\\[^\r\n]|[^\\\r\n]))*"
                          r"|[^\s\"'\\][^\r\n\"'\\]*)")
# PC-13 review round 1 F7: the process id in a Claude Code messaging socket path (review round 3 H5:
# also with escaped slashes).
CC_SOCK = re.compile(r"(/cc-socks\\*/)[0-9]+(\.sock)")


def s(text):
    for a, b in PAIRS:
        text = a.sub(lambda m: b, text)
    text = UA_OS.sub(lambda m: m.group(1) + m.group(2) + "(os omitted)", text)
    text = MAC_OS_X.sub("(os omitted)", text)
    text = OS_JSON.sub(lambda m: m.group(1) + "(os omitted)", text)
    text = UNAME.sub("(os omitted)", text)
    text = ENV_SHELL.sub(lambda m: m.group(1) + "(omitted)", text)
    text = ENV_OS.sub(lambda m: m.group(1) + "(os omitted)", text)
    text = STAINLESS_RE.sub(lambda m: m.group(1) + (m.group(3) or "") + STAINLESS[m.group(2).lower()], text)
    text = CC_SOCK.sub(lambda m: m.group(1) + "<pid>" + m.group(2), text)
    text = SANDBOX.sub(lambda m: m.group(1) + "(os omitted)", text)
    text = MARKETPLACE.sub("marketplace-os-omitted", text)
    text = CMD_SHELL.sub(lambda m: m.group(1) + "(shell omitted) -lc", text)
    text = TIMEZONE.sub(lambda m: m.group(0) if m.group(2).replace("\\/", "/").strip().lower() in UTC_NAMES
                        else m.group(1) + "(omitted)" + m.group(3), text)
    text = ENV_CTX.sub(lambda b: SHELL_TAG.sub(lambda m: m.group(1) + "(omitted)" + m.group(2), b.group(0)), text)
    text = re.sub(r"(/private)?/var/folders/[^\s\"'`)]*", "/tmp/x", text)
    text = re.sub(r"/private/var/[^\s\"'`)]*", "/tmp/x", text)
    text = re.sub(r"(?i)[A-Za-z]:(?:\\\\|\\|/)Users(?:\\\\|\\|/)" + WNAME, lambda m: "/home/user", text)
    text = re.sub(r"(?i)(?<![A-Za-z0-9_.-])/[a-z]/Users/" + WNAME, lambda m: "/home/user", text)
    text = re.sub(r"/Users/[^/\s\"']+", "/home/user", text)
    text = re.sub(r"/private/tmp/", "/tmp/", text)
    text = re.sub(r"/private/tmp(?![A-Za-z0-9_./-])", "/tmp", text)
    for h in HOSTS:  # before the user rule: a host name may hold the user name
        text = re.sub(r"(?:(?<![A-Za-z0-9_-])|(?<=\\[nrtbf]))%s(?![A-Za-z0-9_-])" % re.escape(h), "host", text, flags=re.I)
    if user.lower() != "user":
        text = re.sub(B + r"%s\b" % re.escape(user), "user", text, flags=re.I)
    text = re.sub(r"Attribution for git commits.*?(?=</system-reminder>)", "Harness text.\n", text, flags=re.S)
    text = CF_ENV.sub("(os env omitted)", text)
    text = ITEM_ID.sub(lambda m: ids(m.group(1).lower(), m.group(0).lower()), text)
    text = ULID.sub(lambda m: m.group(1) + ids("ulid", m.group(2)), text)
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
        return {s(k): "(os omitted)" if k in OS_KEYS and isinstance(v, str) else
                STAINLESS[k.lower()] if k.lower() in STAINLESS and isinstance(v, str) else
                LEAD_SHELL.sub("(shell omitted) -lc", s(v)) if k in CMD_KEYS and isinstance(v, str) else walk(v)
                for k, v in o.items()}
    if isinstance(o, list):
        return [walk(v) for v in o]
    if isinstance(o, str):
        return s(o)
    return o


def omit_tool_descriptions(tools):
    for t in tools:
        if isinstance(t, dict):
            if "description" in t:
                t["description"] = "[omitted]"
            if isinstance(t.get("tools"), list):
                omit_tool_descriptions(t["tools"])


def is_tool_listing(data):
    return (isinstance(data, list) and bool(data) and all(isinstance(t, dict) and "type" in t for t in data)
            and any("parameters" in t or "tools" in t for t in data))


raw = open(path, encoding="utf-8", errors="replace").read()
try:
    data = json.loads(raw)
except (ValueError, RecursionError):  # review round 3 H2: too deep to parse -> the raw-text scrub
    sys.stdout.write(s(raw))
    save_ids()
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
    omit_tool_descriptions(b.get("tools") or [])
if is_tool_listing(data):
    omit_tool_descriptions(data)
json.dump(walk(data), sys.stdout, indent=2, ensure_ascii=False)
sys.stdout.write("\n")
save_ids()
