#!/usr/bin/env python3
"""TLS recorder for the pool live checks PR1 and PR2 (nodes WD-21, WD-22; Python 3 stdlib only).

Usage: RECORDER_DUMMY=<dummy key> tls_recorder.py --cert <leaf.pem> --key <leaf-key.pem>
           --upstream http://127.0.0.1:<mock port> --out <record.jsonl> --port-file <file>

A loopback HTTP proxy that stands in for api.anthropic.com:443:
- Listens on 127.0.0.1 (a free port, written to --port-file). Set it as HTTPS_PROXY of zygo.
- Accepts only `CONNECT api.anthropic.com:443` (host in any case). It answers 200, then terminates TLS with
  the leaf certificate (signed by a run-local CA that the client trusts through SSL_CERT_FILE). Any other
  CONNECT target and any plain request get 403 and are recorded.
- For each request inside the tunnel it writes one JSON line to --out: the time, the method and path,
  `auth_sha256_12` = the first 12 hex digits of sha256(<Authorization value>), the scheme word, whether an
  x-api-key header was present, the upstream status, and the client process of the tunnel (found at CONNECT
  time from /proc: the pid, the program name, and the zygo pane key and workspace id of the nearest
  `ws run --ws-id <id> --pane <key>` process at or above it). The Authorization value itself is never
  written, printed or forwarded.
- It forwards /v1/messages requests (count_tokens included) to --upstream with `Authorization: Bearer
  <dummy>` in place of the client's credential (x-api-key removed), and the headers x-live-pane and
  x-live-who added, and relays the reply. Other paths get 404 from the recorder itself.
- A failed TLS handshake is recorded (kind tls_error) with its client process.
"""
import argparse
import hashlib
import http.client
import json
import os
import socket
import ssl
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ALLOWED_HOST = "api.anthropic.com"
ALLOWED_PORT = 443
# Fields of every record line, in this order (absent values are null).
FIELDS = ("t", "kind", "target", "method", "path", "auth_sha256_12", "auth_scheme", "x_api_key",
          "status", "client_pid", "client_prog", "client_who", "pane", "ws")
HOP = {"connection", "keep-alive", "proxy-connection", "transfer-encoding", "te", "trailer", "upgrade",
       "host", "content-length", "authorization", "x-api-key", "proxy-authorization"}
LOCK = threading.Lock()


def auth_hash(value):
    """sha256(<value>)[:12] as hex, or None when there is no value."""
    if value is None:
        return None
    return hashlib.sha256(value.encode("utf-8", "surrogateescape")).hexdigest()[:12]


def auth_scheme(value):
    """The first word of an Authorization value (the scheme), or None."""
    if not value:
        return None
    return value.split(" ", 1)[0]


def record(kind, client=None, **kw):
    """One record line as a dict with exactly FIELDS; unknown keyword names are an error."""
    bad = set(kw) - set(FIELDS)
    if bad:
        raise ValueError("unknown record fields: %s" % sorted(bad))
    c = client or {}
    out = {f: None for f in FIELDS}
    out.update({"t": round(time.time(), 3), "kind": kind, "client_pid": c.get("pid"), "client_prog": c.get("prog"),
                "client_who": c.get("who"), "pane": c.get("pane"), "ws": c.get("ws")})
    out.update(kw)
    return out


def allowed_target(target):
    """True for `api.anthropic.com:443` (host in any case), the only CONNECT target the recorder opens."""
    host, sep, port = target.rpartition(":")
    return bool(sep) and host.lower() == ALLOWED_HOST and port == str(ALLOWED_PORT)


def forward_headers(headers, dummy, pane, who):
    """The headers sent to the upstream: the client's headers without hop-by-hop and credential headers, then
    `authorization: Bearer <dummy>`, `x-live-pane` and `x-live-who`. `headers` is a list of (name, value)."""
    out = [(k, v) for k, v in headers if k.lower() not in HOP]
    out.append(("authorization", "Bearer " + dummy))
    out.append(("x-live-pane", pane or "none"))
    out.append(("x-live-who", who or "none"))
    return out


def _hexaddr(ip, port):
    """The /proc/net/tcp form of an IPv4 address and port (little-endian address, big-endian port)."""
    a = socket.inet_aton(ip)
    return "%s:%04X" % ("".join("%02X" % b for b in reversed(a)), port)


def socket_inode(tcp_text, local, remote):
    """The inode of the /proc/net/tcp row with that local and remote (ip, port), or None."""
    want_l, want_r = _hexaddr(*local), _hexaddr(*remote)
    for line in tcp_text.splitlines()[1:]:
        p = line.split()
        if len(p) >= 10 and p[1] == want_l and p[2] == want_r:
            return p[9]
    return None


def pid_of_inode(inode, proc="/proc"):
    """The first pid with an open fd on socket:[inode] (only processes of this user can be read), or None."""
    target = "socket:[%s]" % inode
    for d in os.listdir(proc):
        if not d.isdigit():
            continue
        fd = os.path.join(proc, d, "fd")
        try:
            names = os.listdir(fd)
        except OSError:
            continue
        for n in names:
            try:
                if os.readlink(os.path.join(fd, n)) == target:
                    return int(d)
            except OSError:
                continue
    return None


def argv_of(pid, proc="/proc"):
    try:
        raw = open(os.path.join(proc, str(pid), "cmdline"), "rb").read()
    except OSError:
        return []
    return [a.decode("utf-8", "replace") for a in raw.split(b"\0") if a]


def ppid_of(pid, proc="/proc"):
    try:
        st = open(os.path.join(proc, str(pid), "stat")).read()
    except OSError:
        return None
    # The command name is in parentheses and may hold spaces; the fields after the last ")" are fixed.
    rest = st[st.rfind(")") + 2:].split()
    return int(rest[1]) if len(rest) > 1 else None


def launcher_of(argv):
    """(ws id, pane key) when argv is a `<zygo> ws run --ws-id <id> --pane <key>` command, else None."""
    if "ws" not in argv or "run" not in argv:
        return None
    i = argv.index("ws")
    if i + 1 >= len(argv) or argv[i + 1] != "run":
        return None
    val = lambda flag: argv[argv.index(flag) + 1] if flag in argv and argv.index(flag) + 1 < len(argv) else None
    return val("--ws-id"), val("--pane")


def client_info(pid, proc="/proc"):
    """{pid, prog, who, pane, ws}: prog is the base name of argv[0]; who is `launcher` when the process is a
    `ws run` launcher, `under-launcher` when one of its ancestors is, else `other`."""
    info = {"pid": pid, "prog": None, "who": "other", "pane": None, "ws": None}
    if pid is None:
        info["who"] = "unknown"
        return info
    argv = argv_of(pid, proc)
    info["prog"] = os.path.basename(argv[0]) if argv else None
    p, depth = pid, 0
    while p and p > 1 and depth < 32:
        lw = launcher_of(argv_of(p, proc))
        if lw:
            info["ws"], info["pane"] = lw
            info["who"] = "launcher" if p == pid else "under-launcher"
            break
        p, depth = ppid_of(p, proc), depth + 1
    return info


def find_client(peer, server_addr):
    """client_info of the process that owns the client end (peer) of a loopback connection to server_addr."""
    try:
        text = open("/proc/net/tcp").read()
    except OSError:
        return client_info(None)
    inode = socket_inode(text, peer, server_addr)
    return client_info(pid_of_inode(inode) if inode and inode != "0" else None)


def write(out, rec):
    with LOCK, open(out, "a") as f:
        f.write(json.dumps(rec, sort_keys=False) + "\n")


class Conn:
    """What the inner handler of one tunnel needs: the config and the tunnel's client process."""

    def __init__(self, cfg, client, target):
        self.cfg, self.client, self.target = cfg, client, target


class Inner(BaseHTTPRequestHandler):
    """Requests inside one TLS tunnel (HTTP/1.1, keep-alive)."""
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _body(self):
        if "chunked" in (self.headers.get("transfer-encoding") or "").lower():
            parts = []
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                if size == 0:
                    while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    break
                parts.append(self.rfile.read(size))
                self.rfile.readline()
            return b"".join(parts)
        n = int(self.headers.get("content-length") or 0)
        return self.rfile.read(n) if n else b""

    def _reply(self, code, ctype, data):
        self.send_response(code)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def handle_any(self):
        ctx = self.server
        cfg = ctx.cfg
        raw = self._body()
        auth = self.headers.get("authorization")
        names = {k.lower() for k in self.headers.keys()}
        status = None
        path = urllib.parse.urlsplit(self.path).path if self.path.startswith("http") else self.path.split("?")[0]
        try:
            if path.startswith("/v1/messages"):
                up = urllib.parse.urlsplit(cfg["upstream"])
                c = http.client.HTTPConnection(up.hostname, up.port, timeout=300)
                hdrs = forward_headers(list(self.headers.items()), cfg["dummy"], ctx.client.get("pane"), ctx.client.get("who"))
                c.putrequest(self.command, self.path, skip_host=True, skip_accept_encoding=True)
                c.putheader("host", "%s:%s" % (up.hostname, up.port))
                for k, v in hdrs:
                    c.putheader(k, v)
                c.putheader("content-length", str(len(raw)))
                c.endheaders(raw)
                r = c.getresponse()
                data = r.read()
                status = r.status
                self._reply(r.status, r.getheader("content-type") or "application/json", data)
                c.close()
            else:
                status = 404
                self._reply(404, "application/json", b'{"type":"error","error":{"type":"not_found_error","message":"recorder"}}')
        finally:
            write(cfg["out"], record("request", ctx.client, target=ctx.target, method=self.command, path=path,
                                     auth_sha256_12=auth_hash(auth), auth_scheme=auth_scheme(auth),
                                     x_api_key="x-api-key" in names, status=status))

    do_POST = do_GET = do_HEAD = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = handle_any


class Outer(BaseHTTPRequestHandler):
    """The proxy side: CONNECT to the allowed target opens a TLS tunnel; everything else gets 403."""
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_CONNECT(self):
        cfg = self.server.cfg
        client = find_client(self.client_address[:2], self.server.server_address[:2])
        if not allowed_target(self.path):
            write(cfg["out"], record("refused", client, target=self.path, method="CONNECT"))
            self.send_response(403)
            self.send_header("content-length", "0")
            self.end_headers()
            self.close_connection = True
            return
        write(cfg["out"], record("connect", client, target=self.path, method="CONNECT"))
        self.send_response(200, "Connection Established")
        self.end_headers()
        self.wfile.flush()
        self.close_connection = True
        try:
            tls = cfg["ctx"].wrap_socket(self.connection, server_side=True)
        except (ssl.SSLError, OSError) as e:
            write(cfg["out"], record("tls_error", client, target=self.path, method=type(e).__name__))
            return
        tls.settimeout(300)
        try:
            Inner(tls, self.client_address, Conn(cfg, client, self.path))
        except (OSError, ssl.SSLError):
            pass
        finally:
            try:
                tls.close()
            except OSError:
                pass

    def refuse(self):
        write(self.server.cfg["out"], record("refused", find_client(self.client_address[:2], self.server.server_address[:2]),
                                             target=self.path, method=self.command))
        self.send_response(403)
        self.send_header("content-length", "0")
        self.end_headers()

    do_GET = do_POST = do_HEAD = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = refuse


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cert", required=True)
    ap.add_argument("--key", required=True)
    ap.add_argument("--upstream", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--port-file", required=True)
    a = ap.parse_args()
    dummy = os.environ.get("RECORDER_DUMMY")
    if not dummy:
        sys.exit("RECORDER_DUMMY not set")
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(a.cert, a.key)
    ctx.set_alpn_protocols(["http/1.1"])
    srv = ThreadingHTTPServer(("127.0.0.1", 0), Outer)
    srv.daemon_threads = True
    srv.cfg = {"ctx": ctx, "upstream": a.upstream, "dummy": dummy, "out": a.out}
    with open(a.port_file + ".tmp", "w") as f:
        f.write(str(srv.server_address[1]))
    os.rename(a.port_file + ".tmp", a.port_file)
    srv.serve_forever()


if __name__ == "__main__":
    main()
