#!/usr/bin/env python3
"""Tests of scripts/premises/tls_recorder.py (nodes WD-21, WD-22). No agent; loopback only.
Usage: python3 -I scripts/premises/tls_recorder_test.py   ->  "tls_recorder tests: ok (<n>)", exit 0

The pure parts (hash, record format, target rule, forwarded headers, /proc parsing) always run. The loopback
case (a CONNECT through the recorder with a CA made by openssl, to a local upstream) runs when openssl is on
PATH; without it the line "skip: loopback (no openssl)" is printed. Every token-like literal is built at run
time from parts, so this file holds none."""
import hashlib
import http.client
import importlib.util
import json
import os
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("tls_recorder", os.path.join(HERE, "tls_recorder.py"))
tr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tr)
n = bad = 0
TOKEN = "sk-ant" + "-oat01-" + "test" + "0123456789abcdef" * 4
DUMMY = "dummy-" + "key"


def check(name, cond, why=""):
    global n, bad
    n += 1
    if cond:
        print("ok:", name)
    else:
        bad += 1
        print("FAIL: %s (%s)" % (name, why))


# --- pure parts ------------------------------------------------------------------------------------
want = hashlib.sha256(("Bearer " + TOKEN).encode()).hexdigest()[:12]
check("hash: 12 hex digits of sha256 of the whole Authorization value", tr.auth_hash("Bearer " + TOKEN) == want, tr.auth_hash("Bearer " + TOKEN))
check("hash: no value gives None", tr.auth_hash(None) is None)
check("hash: two tokens give two hashes", tr.auth_hash("Bearer " + TOKEN) != tr.auth_hash("Bearer " + TOKEN + "x"))
check("scheme: first word", tr.auth_scheme("Bearer " + TOKEN) == "Bearer" and tr.auth_scheme(None) is None)

c = {"pid": 42, "prog": "zygo", "who": "launcher", "pane": "p1", "ws": "w1"}
r = tr.record("request", c, target="api.anthropic.com:443", method="POST", path="/v1/messages",
              auth_sha256_12=tr.auth_hash("Bearer " + TOKEN), auth_scheme="Bearer", x_api_key=False, status=200)
check("record: exactly the fixed fields, in order", list(r) == list(tr.FIELDS), list(r))
check("record: client fields copied", (r["client_pid"], r["client_prog"], r["client_who"], r["pane"], r["ws"]) == (42, "zygo", "launcher", "p1", "w1"))
line = json.dumps(r)
check("record: the token is in no record line", TOKEN not in line and TOKEN[10:30] not in line, line)
try:
    tr.record("request", c, authorization="Bearer " + TOKEN)
    check("record: an unknown field is refused", False, "no error")
except ValueError:
    check("record: an unknown field is refused", True)
check("record: no client gives null client fields", tr.record("refused")["client_pid"] is None)

check("target: api.anthropic.com:443", tr.allowed_target("api.anthropic.com:443"))
check("target: host in any case", tr.allowed_target("API.Anthropic.COM:443"))
check("target: other port refused", not tr.allowed_target("api.anthropic.com:8443"))
check("target: other host refused", not tr.allowed_target("statsig.anthropic.com:443"))
check("target: no port refused", not tr.allowed_target("api.anthropic.com"))
check("target: suffix trick refused", not tr.allowed_target("api.anthropic.com.example:443"))

fh = tr.forward_headers([("Authorization", "Bearer " + TOKEN), ("X-Api-Key", "k"), ("Host", "api.anthropic.com"),
                         ("Content-Length", "3"), ("anthropic-beta", "b1"), ("Connection", "keep-alive")], DUMMY, "p2", "launcher")
names = [k.lower() for k, _ in fh]
check("forward: credential and hop-by-hop headers removed", "x-api-key" not in names and "host" not in names
      and "content-length" not in names and "connection" not in names, names)
check("forward: one authorization, the dummy", [v for k, v in fh if k.lower() == "authorization"] == ["Bearer " + DUMMY], fh)
check("forward: other headers kept, pane and who added", ("anthropic-beta", "b1") in fh and ("x-live-pane", "p2") in fh
      and ("x-live-who", "launcher") in fh, fh)
check("forward: the token is in no forwarded header", not any(TOKEN in v for _, v in fh))

tcp = ("  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n"
       "   0: 0100007F:1F90 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 111 1 0\n"
       "   1: 0100007F:D431 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 222 1 0\n")
check("proc: the client row of a loopback pair", tr.socket_inode(tcp, ("127.0.0.1", 0xD431), ("127.0.0.1", 0x1F90)) == "222")
check("proc: the server row is not the client row", tr.socket_inode(tcp, ("127.0.0.1", 0x1F90), ("127.0.0.1", 0xD431)) == "111")
check("proc: no row gives None", tr.socket_inode(tcp, ("127.0.0.1", 1), ("127.0.0.1", 2)) is None)
check("launcher: ws run argv", tr.launcher_of(["/x/zygo", "ws", "run", "--ws-id", "abc", "--pane", "k1"]) == ("abc", "k1"))
check("launcher: other argv", tr.launcher_of(["/x/zygo", "hook", "statusline"]) is None
      and tr.launcher_of(["claude", "run", "ws"]) is None)

# This process as a client: a loopback pair found through /proc gives this pid.
ls = socket.socket()
ls.bind(("127.0.0.1", 0))
ls.listen(1)
cs = socket.create_connection(ls.getsockname())
ci = tr.find_client(cs.getsockname()[:2], ls.getsockname()[:2])
check("proc: find_client names this process", ci["pid"] == os.getpid() and ci["who"] == "other", ci)
cs.close()
ls.close()

# A real process tree: a parent whose argv is a fake `ws run --ws-id wfake --pane kfake` launcher opens a socket, and its
# child (another program) opens one too. find_client gives launcher for the parent, under-launcher for the child.
ls = socket.socket()
ls.bind(("127.0.0.1", 0))
ls.listen(4)
CHILD = ("import socket, sys\ns = socket.create_connection(('127.0.0.1', int(sys.argv[1])))\n"
         "print('U %d' % s.getsockname()[1], flush=True)\nsys.stdin.read()\n")
PARENT = ("import socket, subprocess, sys\nport = int(sys.argv[1])\ns = socket.create_connection(('127.0.0.1', port))\n"
          "print('L %d' % s.getsockname()[1], flush=True)\n"
          "c = subprocess.Popen([sys.executable, '-I', '-c', " + repr(CHILD) + ", str(port)], stdin=subprocess.PIPE)\n"
          "sys.stdin.read()\nc.stdin.close()\nc.wait()\n")
tree = subprocess.Popen([sys.executable, "-I", "-c", PARENT, str(ls.getsockname()[1]), "ws", "run", "--ws-id", "wfake", "--pane", "kfake"],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
ports = {}
try:
    for _ in range(2):
        kind, port = tree.stdout.readline().split()
        ports[kind] = int(port)
    lc = tr.find_client(("127.0.0.1", ports["L"]), ls.getsockname()[:2])
    uc = tr.find_client(("127.0.0.1", ports["U"]), ls.getsockname()[:2])
    check("proc: the fake launcher's own socket gives who launcher, its pane and ws id",
          lc["pid"] == tree.pid and lc["who"] == "launcher" and lc["pane"] == "kfake" and lc["ws"] == "wfake", lc)
    check("proc: its child's socket gives who under-launcher and the launcher's pane",
          uc["pid"] not in (None, tree.pid, os.getpid()) and uc["who"] == "under-launcher" and uc["pane"] == "kfake" and uc["ws"] == "wfake", uc)
finally:
    tree.stdin.close()
    tree.wait(10)
    ls.close()

# --- loopback: CONNECT, TLS, record, forward ------------------------------------------------------
SEEN = []


class Up(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length") or 0))
        SEEN.append({"path": self.path, "auth": self.headers.get("authorization"), "xkey": self.headers.get("x-api-key"),
                     "pane": self.headers.get("x-live-pane"), "body": body.decode()})
        d = b'{"ok": true}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(d)))
        self.end_headers()
        self.wfile.write(d)


def openssl(*args, cwd):
    subprocess.run(["openssl", *args], cwd=cwd, check=True, capture_output=True)


if shutil.which("openssl") is None:
    print("skip: loopback (no openssl)")
else:
    T = tempfile.mkdtemp()
    rec = None
    up = None
    try:
        openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=test CA",
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign",
                "-keyout", "ca-key.pem", "-out", "ca.pem", cwd=T)
        openssl("req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=api.anthropic.com", "-keyout", "leaf-key.pem",
                "-out", "leaf.csr", cwd=T)
        open(os.path.join(T, "ext"), "w").write("subjectAltName=DNS:api.anthropic.com\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n"
                                             "subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n")
        openssl("x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca-key.pem", "-CAcreateserial", "-days", "1",
                "-extfile", "ext", "-out", "leaf.pem", cwd=T)
        up = ThreadingHTTPServer(("127.0.0.1", 0), Up)
        threading.Thread(target=up.serve_forever, daemon=True).start()
        out = os.path.join(T, "rec.jsonl")
        rec = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "tls_recorder.py"), "--cert", os.path.join(T, "leaf.pem"),
                                "--key", os.path.join(T, "leaf-key.pem"), "--upstream", "http://127.0.0.1:%d" % up.server_address[1],
                                "--out", out, "--port-file", os.path.join(T, "port")], env={"RECORDER_DUMMY": DUMMY, "PATH": "/usr/bin:/bin"})
        for _ in range(100):
            if os.path.exists(os.path.join(T, "port")):
                break
            time.sleep(0.1)
        port = int(open(os.path.join(T, "port")).read())
        ctx = ssl.create_default_context(cafile=os.path.join(T, "ca.pem"))
        # Two requests on one tunnel (keep-alive), then one to a path the recorder does not forward.
        c = http.client.HTTPSConnection("127.0.0.1", port, context=ctx, timeout=10)
        c.set_tunnel("api.anthropic.com", 443)
        for i in range(2):
            c.request("POST", "/v1/messages?beta=true", body='{"i": %d}' % i,
                      headers={"authorization": "Bearer " + TOKEN, "content-type": "application/json"})
            resp = c.getresponse()
            resp.read()
            check("loopback: request %d answered by the upstream" % i, resp.status == 200, resp.status)
        c.request("GET", "/api/oauth/profile", headers={"authorization": "Bearer " + TOKEN})
        r404 = c.getresponse()
        r404.read()
        check("loopback: other path answered 404 by the recorder", r404.status == 404, r404.status)
        c.close()
        # A CONNECT to another host is refused.
        s = socket.create_connection(("127.0.0.1", port))
        s.sendall(b"CONNECT statsig.anthropic.com:443 HTTP/1.1\r\nHost: statsig.anthropic.com:443\r\n\r\n")
        first = s.recv(100).decode()
        s.close()
        check("loopback: other CONNECT host refused with 403", first.startswith("HTTP/1.1 403"), first)
        # An untrusted client (default CA store) fails the handshake; the recorder records tls_error.
        bad_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)  # verification on, no CA loaded
        s = socket.create_connection(("127.0.0.1", port))
        s.sendall(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n")
        s.recv(200)
        try:
            bad_ctx.wrap_socket(s, server_hostname="api.anthropic.com").close()
            check("loopback: untrusted client fails TLS", False, "handshake passed")
        except ssl.SSLError:
            check("loopback: untrusted client fails TLS", True)
        for _ in range(50):
            lines = [json.loads(x) for x in open(out)] if os.path.exists(out) else []
            if any(x["kind"] == "tls_error" for x in lines):
                break
            time.sleep(0.1)
        reqs = [x for x in lines if x["kind"] == "request"]
        check("loopback: three request lines, each with the token hash", len(reqs) == 3 and all(x["auth_sha256_12"] == want for x in reqs), reqs)
        check("loopback: the request lines name this process", all(x["client_pid"] == os.getpid() for x in reqs), reqs)
        check("loopback: one connect line per tunnel, one refused, one tls_error",
              [x["kind"] for x in lines if x["kind"] != "request"] == ["connect", "refused", "connect", "tls_error"], [x["kind"] for x in lines])
        check("loopback: statuses 200, 200, 404", [x["status"] for x in reqs] == [200, 200, 404], reqs)
        raw = open(out).read()
        check("loopback: the token is in no record byte", TOKEN not in raw and TOKEN[7:] not in raw)
        check("loopback: the upstream got two /v1/messages with the dummy bearer only",
              len(SEEN) == 2 and all(x["auth"] == "Bearer " + DUMMY and x["xkey"] is None and x["pane"] == "none" for x in SEEN), SEEN)
        check("loopback: the bodies reached the upstream unchanged", [x["body"] for x in SEEN] == ['{"i": 0}', '{"i": 1}'], SEEN)
    finally:
        if rec:
            rec.terminate()
            rec.wait(5)
        if up:
            up.shutdown()
        shutil.rmtree(T, ignore_errors=True)

if bad:
    print("tls_recorder tests: %d failed" % bad)
    sys.exit(1)
print("tls_recorder tests: ok (%d)" % n)
