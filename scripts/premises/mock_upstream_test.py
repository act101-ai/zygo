#!/usr/bin/env python3
"""Tests for mock_upstream.py (node PC-2). Run: python3 -I scripts/premises/mock_upstream_test.py

Starts the mock as a child process on 127.0.0.1 with a rules file and checks the Responses
answers, the rule sequence, the usage override, the /v1/models answer, CONNECT and absolute-URI
refusal, the Anthropic default path, and the credential abort. No network beyond 127.0.0.1.
"""
import http.client
import json
import os
import socket
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
MOCK = os.path.join(HERE, "mock_upstream.py")
KEY = "dummy-" + "key"
ALT = "dummy-" + "alt-" + "key"  # PC-10: the second accepted credential
FAILS = []
CHECKS = [0]


def ev(name, data):
    return "event: %s\ndata: %s\n\n" % (name, json.dumps(data))


A_SSE = (ev("response.created", {"type": "response.created", "response": {"id": "resp_a", "model": "{{MODEL}}"}})
         + ev("response.output_text.delta", {"type": "response.output_text.delta", "delta": "a"})
         + ev("response.completed", {"type": "response.completed", "response": {
             "id": "resp_a", "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}}))
S1_SSE = ev("response.created", {"type": "response.created", "response": {"id": "s1"}})
S2_SSE = ev("response.created", {"type": "response.created", "response": {"id": "s2"}})
M_SSE = ev("message_start", {"type": "message_start", "message": {"id": "m"}})
B_JSON = {"id": "resp_b", "output": [{"type": "message", "content": [{"type": "output_text", "text": "b"}]}]}
ERR_JSON = {"error": {"code": "context_length_exceeded", "message": "too long"}}
MODELS_JSON = {"models": [{"slug": "gpt-mock"}]}
USAGE = {"input_tokens": 900000, "output_tokens": 5, "total_tokens": 900005}

RULES = [
    {"name": "r-sse", "path": "/v1/responses", "body": "m1", "sse": "a.sse", "times": 1},
    {"name": "r-json", "path": "/v1/responses", "body": "m2", "json": "b.json"},
    {"name": "r-err", "path": "/v1/responses", "body": "m3", "status": 400, "json": "err.json"},
    {"path": "/v1/responses", "body": "m4", "sse": "s1.sse", "times": 1},
    {"path": "/v1/responses", "body": "m4", "sse": "s2.sse", "times": 1},
    {"path": "/v1/responses", "body": "m5", "sse": "a.sse", "usage": USAGE},
    {"path": "/v1/models", "json": "models.json"},
    {"name": "msg", "sse": "m.sse"},
]


def fail(case, why):
    FAILS.append(case)
    print("mock test fail: %s: %s" % (case, why))


def events(text):
    """SSE text -> list of (event, data JSON)."""
    out = []
    for block in text.strip().split("\n\n"):
        name = data = None
        for line in block.split("\n"):
            if line.startswith("event: "):
                name = line[7:]
            elif line.startswith("data: "):
                data = json.loads(line[6:])
        out.append((name, data))
    return out


class Mock:
    def __init__(self, tmp):
        self.dir = os.path.join(tmp, "rec")
        rules = os.path.join(tmp, "rules.json")
        with open(rules, "w") as f:
            json.dump(RULES, f)
        port_file = os.path.join(tmp, "port")
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "MOCK_KEY": KEY, "MOCK_ALT_KEY": ALT}
        self.proc = subprocess.Popen([sys.executable, "-I", MOCK, "--dir", self.dir, "--rules", rules,
                                      "--port-file", port_file], env=env)
        for _ in range(600):
            if os.path.exists(port_file) or self.proc.poll() is not None:
                break
            time.sleep(0.1)
        if not os.path.exists(port_file):
            self.stop()
            raise RuntimeError("the mock did not start (exit %r)" % self.proc.returncode)
        with open(port_file) as f:
            self.port = int(f.read())

    def req(self, method, path, body=None, auth=KEY):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        headers = {"authorization": "Bearer " + auth}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["content-type"] = "application/json"
        c.request(method, path, body=data, headers=headers)
        r = c.getresponse()
        out = (r.status, r.getheader("content-type"), r.read().decode())
        c.close()
        return out

    def raw(self, text):
        """Send raw request bytes; return (status line, the whole reply) after the server closed."""
        s = socket.create_connection(("127.0.0.1", self.port), timeout=10)
        s.sendall(text.encode())
        buf = b""
        while True:
            chunk = s.recv(4096)
            if not chunk:
                break
            buf += chunk
        s.close()
        return buf.split(b"\r\n", 1)[0].decode(), buf.decode("latin-1")

    def records(self):
        return sorted(n for n in os.listdir(self.dir) if n.endswith(".json"))

    def connect_txt(self):
        p = os.path.join(self.dir, "connect.txt")
        if not os.path.exists(p):
            return ""
        with open(p) as f:
            return f.read()

    def stop(self):
        if self.proc.poll() is None:
            self.proc.kill()
        self.proc.wait()


def write(tmp, name, text):
    with open(os.path.join(tmp, name), "w") as f:
        f.write(text)


def check(case, cond, why):
    CHECKS[0] += 1
    if not cond:
        fail(case, why)
        return False
    return True


def run(m):
    # M1: an SSE rule on /v1/responses.
    st, ct, body = m.req("POST", "/v1/responses", {"model": "m", "stream": True, "input": "m1"})
    check("M1", st == 200 and ct == "text/event-stream" and body == A_SSE.replace("{{MODEL}}", "m"),
          "got %s %s %r" % (st, ct, body[:200]))

    # M2: a JSON rule.
    st, ct, body = m.req("POST", "/v1/responses", {"model": "m", "input": "m2"})
    ok = st == 200 and ct == "application/json"
    try:
        ok = ok and json.loads(body) == B_JSON
    except ValueError:
        ok = False
    check("M2", ok, "got %s %s %r" % (st, ct, body[:200]))

    # M3: a status with a body.
    st, ct, body = m.req("POST", "/v1/responses", {"model": "m", "input": "m3"})
    ok = st == 400
    try:
        ok = ok and json.loads(body) == ERR_JSON
    except ValueError:
        ok = False
    check("M3", ok, "got %s %r" % (st, body[:200]))

    # M4: a rule sequence, then the default.
    got = [m.req("POST", "/v1/responses", {"model": "m", "stream": True, "input": "m4"}) for _ in range(3)]
    check("M4", got[0][2] == S1_SSE and got[1][2] == S2_SSE
          and "event: response.completed" in got[2][2] and '"output_tokens": 1' in got[2][2],
          "got %r" % [g[2][:120] for g in got])

    # M5: the rule's usage replaces response.usage in response.completed only.
    st, ct, body = m.req("POST", "/v1/responses", {"model": "m", "stream": True, "input": "m5"})
    try:
        got_ev = events(body)
        want_ev = events(A_SSE.replace("{{MODEL}}", "m"))
        done = [d for e, d in got_ev if e == "response.completed"]
        ok = (len(got_ev) == len(want_ev) and len(done) == 1 and done[0]["response"]["usage"] == USAGE
              and [x for x in got_ev if x[0] != "response.completed"]
              == [x for x in want_ev if x[0] != "response.completed"])
    except (ValueError, KeyError, TypeError):
        ok = False
    check("M5", st == 200 and ok, "got %s %r" % (st, body[:300]))

    # M6: GET /v1/models from a rule; the record has method GET.
    before = set(m.records())
    st, ct, body = m.req("GET", "/v1/models?client_version=0.172.0")
    new = sorted(set(m.records()) - before)
    ok = st == 200
    try:
        ok = ok and json.loads(body) == MODELS_JSON
        with open(os.path.join(m.dir, new[-1])) as f:
            ok = ok and json.load(f)["method"] == "GET"
    except (ValueError, IndexError, KeyError):
        ok = False
    check("M6", ok, "got %s %r, new records %r" % (st, body[:200], new))

    # M7: CONNECT is refused and recorded; no request record.
    before = m.records()
    line, _ = m.raw("CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid:443\r\n\r\n")
    check("M7", line.startswith("HTTP/1.1 403") and m.connect_txt() == "example.invalid:443 refused\n"
          and m.records() == before,
          "status %r, connect.txt %r, records %d -> %d" % (line, m.connect_txt(), len(before), len(m.records())))

    # M8: a rule with no path is not used on /v1/responses; it is on /v1/messages.
    st1, _, b1 = m.req("POST", "/v1/responses", {"model": "m", "stream": True, "input": "m8"})
    st2, _, b2 = m.req("POST", "/v1/messages", {"model": "m", "stream": True, "messages": []})
    check("M8", st1 == 200 and "event: response.completed" in b1 and "message_start" not in b1
          and st2 == 200 and b2 == M_SSE, "got %s %r / %s %r" % (st1, b1[:120], st2, b2[:120]))

    # M10: a plain-HTTP proxied request (absolute URI) is refused and recorded; no request record.
    before = m.records()
    line, _ = m.raw("GET http://example.invalid/x HTTP/1.1\r\nHost: example.invalid\r\n\r\n")
    check("M10", line.startswith("HTTP/1.1 403")
          and m.connect_txt() == "example.invalid:443 refused\nexample.invalid:80 GET /x refused\n"
          and m.records() == before,
          "status %r, connect.txt %r, records %d -> %d" % (line, m.connect_txt(), len(before), len(m.records())))

    # M11: an absolute-URI request to the mock's own address is served as an origin request.
    before_txt, before = m.connect_txt(), set(m.records())
    st, ct, body = m.req("POST", "http://127.0.0.1:%d/v1/responses" % m.port,
                         {"model": "m", "stream": True, "input": "m11"})
    new = sorted(set(m.records()) - before)
    ok = st == 200 and ct == "text/event-stream" and "event: response.completed" in body \
        and '"output_tokens": 1' in body and m.connect_txt() == before_txt and len(new) == 1
    try:
        with open(os.path.join(m.dir, new[-1])) as f:
            ok = ok and json.load(f)["path"] == "/v1/responses"
    except (ValueError, IndexError, KeyError):
        ok = False
    check("M11", ok, "got %s %s %r, connect.txt %r, new records %r" % (st, ct, body[:120], m.connect_txt(), new))

    # M12-M17 (review round 1, F7): every method and every request-target that is not origin-form
    # and not the mock's own address is refused (403) and recorded; no request record.
    def refused(case, text, want_line):
        before_txt, before = m.connect_txt(), m.records()
        try:
            line, _ = m.raw(text)
        except OSError as e:
            line = "error %r" % e
        got = m.connect_txt()[len(before_txt):]
        check(case, line.startswith("HTTP/1.1 403") and got == want_line + "\n" and m.records() == before,
              "status %r, new connect.txt %r, records %d -> %d" % (line, got, len(before), len(m.records())))

    # M12: PATCH, OPTIONS, TRACE and an unknown method go through the same handler.
    for meth in ("PATCH", "OPTIONS", "TRACE", "FOO"):
        refused("M12 " + meth, "%s http://example.invalid:8080/x HTTP/1.1\r\nHost: example.invalid\r\n\r\n" % meth,
                "example.invalid:8080 %s /x refused" % meth)
    # M13: a malformed authority is recorded verbatim with ? for the port.
    refused("M13", "GET http://[::1/x HTTP/1.1\r\nHost: x\r\n\r\n", "[::1:? GET /x refused")
    # M14: a port out of range is ?, not the scheme default.
    refused("M14", "GET http://evil.example:99999/ HTTP/1.1\r\nHost: x\r\n\r\n", "evil.example:? GET / refused")
    # M15: any scheme counts: ws (port 80), wss (443), ftp (21), an unknown scheme (?).
    refused("M15 ws", "GET ws://example.invalid/x HTTP/1.1\r\nHost: x\r\n\r\n", "example.invalid:80 GET /x refused")
    refused("M15 wss", "GET wss://example.invalid/x HTTP/1.1\r\nHost: x\r\n\r\n", "example.invalid:443 GET /x refused")
    refused("M15 ftp", "GET ftp://example.invalid/x HTTP/1.1\r\nHost: x\r\n\r\n", "example.invalid:21 GET /x refused")
    refused("M15 zz", "GET zz://example.invalid/x HTTP/1.1\r\nHost: x\r\n\r\n", "example.invalid:? GET /x refused")
    # M16: userinfo is never recorded.
    refused("M16", "GET http://user:secret@example.invalid/ HTTP/1.1\r\nHost: x\r\n\r\n",
            "example.invalid:80 GET / refused")
    # M17: a target that is neither origin-form nor an absolute URI (asterisk form, a bare word).
    refused("M17 *", "OPTIONS * HTTP/1.1\r\nHost: x\r\n\r\n", "*:? OPTIONS ? refused")
    refused("M17 word", "GET u:p@foo HTTP/1.1\r\nHost: x\r\n\r\n", "foo:? GET ? refused")
    # M18: ws:// to the mock's own address is not served (only http and https are).
    refused("M18", "GET ws://127.0.0.1:%d/v1/models HTTP/1.1\r\nHost: x\r\n\r\n" % m.port,
            "127.0.0.1:%d GET /v1/models refused" % m.port)

    # M26 (PC-10; review round 1, F5): the recorded path drops the whole query and the fragment; a
    # path to another local port is recorded with its path; a segment that looks like a token
    # (sk-..., long hex, long base64 with a digit, bot<digits>:...) is ?; a path with a byte outside
    # visible ASCII is ?.
    refused("M26 query", "GET http://example.invalid/a/b?token=s3cret&x&y=2#frag HTTP/1.1\r\nHost: x\r\n\r\n",
            "example.invalid:80 GET /a/b refused")
    # Round 2 (G8): any segment of 20 or more token characters (letters, digits, . ~ % - _ + =),
    # as a JWT or a percent-encoded key.
    tokens = ["sk" + "-abcd1234", "0123456789abcdef0123", "QUJDREVGR0hJSktMTU5PUFFS", "bot" + "123456:ABCdef",
              "eyJ" + "hbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln", "sk" + "%2Dabcdefghijklmnopqrst",
              "a.b~c_d-e%2Ff.g~h_i-j", "chat-completions-streaming"]
    for i, tok in enumerate(tokens):
        refused("M26 token %d" % i, "GET http://example.invalid/v1/%s/send HTTP/1.1\r\nHost: x\r\n\r\n" % tok,
                "example.invalid:80 GET /v1/?/send refused")
    refused("M26 plain segments", "GET http://example.invalid/v1/chat-completions/x1 HTTP/1.1\r\nHost: x\r\n\r\n",
            "example.invalid:80 GET /v1/chat-completions/x1 refused")
    refused("M26 other port", "POST http://127.0.0.1:1/some/path HTTP/1.1\r\nHost: x\r\ncontent-length: 0\r\n\r\n",
            "127.0.0.1:1 POST /some/path refused")
    refused("M26 query only", "GET http://example.invalid?k=v HTTP/1.1\r\nHost: x\r\n\r\n",
            "example.invalid:80 GET / refused")
    refused("M26 byte", "GET http://example.invalid/\xe9 HTTP/1.1\r\nHost: x\r\n\r\n",
            "example.invalid:80 GET ? refused")

    # M19-M21 (review round 2, G8): a request line over 64 KiB (414) or a header line too long
    # (431) is answered by the base class before any handler; a proxy request among them is still
    # recorded (host and port ? when the target is cut off or does not parse; the method as read).
    def overlong(case, text, want_status, want_line):
        before_txt, before = m.connect_txt(), m.records()
        try:
            line, _ = m.raw(text)
        except OSError as e:
            line = "error %r" % e
        got = m.connect_txt()[len(before_txt):]
        check(case, line.startswith("HTTP/1.1 %d" % want_status) and got == want_line
              and m.records() == before,
              "status %r, new connect.txt %r, records %d -> %d" % (line, got[:200], len(before), len(m.records())))

    big = "a" * 70000
    overlong("M19", "GET http://evil.example/%s HTTP/1.1\r\nHost: x\r\n\r\n" % big, 414,
             "evil.example:80 GET ? refused\n")
    overlong("M20", "CONNECT %s:443 HTTP/1.1\r\nHost: x\r\n\r\n" % ("e" * 70000), 414, "?:? refused\n")
    overlong("M21", "GET http://evil4.example/ HTTP/1.1\r\nHost: x\r\nX-A: %s\r\n\r\n" % big, 431,
             "evil4.example:80 GET / refused\n")
    overlong("M21 origin", "GET /v1/models HTTP/1.1\r\nHost: x\r\nX-A: %s\r\n\r\n" % big, 431, "")

    # M22-M25 (review round 3, H3): every other error reply of the base class (400, 505) to a
    # request line with a target that is not origin-form and not the mock's own address is recorded
    # too. A request that fails before its version is accepted gets an HTTP/0.9 reply (no status
    # line): the error code is read from the reply body.
    def base_error(case, text, want_code, want_line):
        before_txt, before = m.connect_txt(), m.records()
        try:
            _, reply = m.raw(text)
        except OSError as e:
            reply = "error %r" % e
        got = m.connect_txt()[len(before_txt):]
        check(case, "Error code: %d" % want_code in reply and got == want_line and m.records() == before,
              "reply %r, new connect.txt %r, records %d -> %d" % (reply[:200], got, len(before), len(m.records())))

    base_error("M22 CONNECT syntax", "CONNECT evil5.example:443 HTTP/1.1 x\r\nHost: x\r\n\r\n", 400,
               "evil5.example:443 refused\n")
    base_error("M22 GET syntax", "GET http://evil5.example/ HTTP/1.1 x\r\nHost: x\r\n\r\n", 400,
               "evil5.example:80 GET / refused\n")
    base_error("M23 HTTP/0.9 CONNECT", "CONNECT evil6.example:443\r\n\r\n", 400, "evil6.example:443 refused\n")
    base_error("M23 CONNECT alone", "CONNECT\r\n\r\n", 400, "?:? refused\n")
    base_error("M24 HTTP/2.0", "GET http://evil7.example:8080/ HTTP/2.0\r\nHost: x\r\n\r\n", 505,
               "evil7.example:8080 GET / refused\n")
    base_error("M24 CONNECT HTTP/2.0", "CONNECT evil7.example:443 HTTP/2.0\r\nHost: x\r\n\r\n", 505,
               "evil7.example:443 refused\n")
    base_error("M25 HTTP/1.1x", "POST https://evil8.example/v1 HTTP/1.1x\r\nHost: x\r\n\r\n", 400,
               "evil8.example:443 POST /v1 refused\n")
    base_error("M25 unparsable", "GET http://[::1/x HTTP/1.1x\r\nHost: x\r\n\r\n", 400, "[::1:? GET /x refused\n")
    # Not recorded: an origin-form target, and the mock's own address.
    base_error("M25 origin", "GET /v1/models HTTP/2.0\r\nHost: x\r\n\r\n", 505, "")
    base_error("M25 own address", "GET http://127.0.0.1:%d/v1/models HTTP/1.1x\r\nHost: x\r\n\r\n" % m.port, 400, "")

    # M27 (PC-10): MOCK_ALT_KEY is accepted like MOCK_KEY (Bearer or x-api-key); each record says
    # which of the two a request carried (credential primary or alt), never the value.
    def cred_of(st, case):
        new = sorted(set(m.records()) - before)
        try:
            with open(os.path.join(m.dir, new[-1])) as f:
                text = f.read()
            return st, json.loads(text).get("credential"), ALT in text or KEY in text
        except (ValueError, IndexError) as e:
            return st, "error %r" % e, False
    before = set(m.records())
    got = cred_of(m.req("POST", "/v1/responses", {"model": "m", "input": "m27a"}, auth=ALT)[0], "alt")
    check("M27 alt", got == (200, "alt", False), "got %r" % (got,))
    before = set(m.records())
    got = cred_of(m.req("POST", "/v1/responses", {"model": "m", "input": "m27p"})[0], "primary")
    check("M27 primary", got == (200, "primary", False), "got %r" % (got,))
    before = set(m.records())
    c = http.client.HTTPConnection("127.0.0.1", m.port, timeout=10)
    c.request("POST", "/v1/messages", body=json.dumps({"model": "m", "messages": []}).encode(),
              headers={"x-api-key": ALT, "content-type": "application/json"})
    r = c.getresponse()
    r.read()
    c.close()
    got = cred_of(r.status, "alt x-api-key")
    check("M27 alt x-api-key", got == (200, "alt", False), "got %r" % (got,))
    # M28 (PC-10): a MOCK_ALT_KEY equal to MOCK_KEY is refused at start (the record could not say
    # which one a request carried).
    with tempfile.TemporaryDirectory() as t2:
        r2 = subprocess.run([sys.executable, "-I", MOCK, "--dir", os.path.join(t2, "rec")],
                            env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "MOCK_KEY": KEY, "MOCK_ALT_KEY": KEY},
                            capture_output=True, text=True, timeout=30)
    check("M28", r2.returncode != 0 and "MOCK_ALT_KEY equals MOCK_KEY" in r2.stderr,
          "exit %r, stderr %r" % (r2.returncode, r2.stderr))

    # M9 (last: it stops the mock): another credential gives 401 and ABORT.
    st, _, _ = m.req("POST", "/v1/responses", {"model": "m", "input": "m9"}, auth="other")
    check("M9", st == 401 and os.path.exists(os.path.join(m.dir, "ABORT")), "got %s" % st)


def main():
    with tempfile.TemporaryDirectory() as tmp:
        write(tmp, "a.sse", A_SSE)
        write(tmp, "s1.sse", S1_SSE)
        write(tmp, "s2.sse", S2_SSE)
        write(tmp, "m.sse", M_SSE)
        write(tmp, "b.json", json.dumps(B_JSON))
        write(tmp, "err.json", json.dumps(ERR_JSON))
        write(tmp, "models.json", json.dumps(MODELS_JSON))
        m = None
        try:
            m = Mock(tmp)
            run(m)
        except Exception as e:  # a crash is a failure, not a traceback
            fail("run", repr(e))
        finally:
            if m is not None:
                m.stop()
    if FAILS:
        sys.exit(1)
    print("mock tests: ok (%d)" % CHECKS[0])


if __name__ == "__main__":
    main()
