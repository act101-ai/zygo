#!/usr/bin/env python3
"""Mock Anthropic and OpenAI Responses upstream for the premise experiments (Python 3 stdlib only).

Usage: mock_upstream.py --dir <record dir> --rules <rules.json> [--port-file <file>]

- Listens on 127.0.0.1 (a free port, written to --port-file).
- Accepts only the dummy credential from MOCK_KEY (x-api-key or "Bearer <key>").
  Any other credential: HTTP 401, an ABORT file in the record dir, and the mock stops. A request
  with no credential at all is recorded and answered 401 without aborting.
- A compaction request (x-claude-code-compaction) that no rule matches gets a realistic
  <analysis>/<summary> reply instead of the one-word default.
- Records each request (method, path, headers, JSON body) as <dir>/<nnnn>.json, with
  authorization and x-api-key values written as "[omitted]".
- Rules are tried for POST .../v1/messages, POST .../v1/responses and GET .../v1/models (query
  string allowed). A rule with no "path" matches only POST .../v1/messages. The first rule that
  matches and has uses left replies with its "sse" or "json" file and its "status"; "times" gives
  a rule sequence (the n-th matching request gets the n-th rule's reply).
- No rule matched: /v1/messages gets a plain short end_turn reply; /v1/responses gets a short
  Responses SSE stream (response.created, output_text.delta "ok", output_item.done,
  response.completed); /v1/models gets 404. SSE and JSON files may use {{MODEL}} and {{MSGID}}.
- "usage" replaces response.usage in the response.completed event of an SSE reply.
- CONNECT <host>:<port> (an HTTPS_PROXY tunnel) is refused (403, connection closed, no tunnel) and
  recorded as the line "<host>:<port> refused" in <dir>/connect.txt. An absolute-URI request
  (an HTTP_PROXY request such as "GET http://host/x") is refused the same way and recorded as
  "<host>:<port> <METHOD> refused" (port 80 or 443 when the URI names none). Neither gets a
  <nnnn>.json record or a credential check. An absolute-URI request to the mock's own address
  (127.0.0.1, localhost or [::1] with the mock's port) is served as an origin request to its path.

Rule (JSON list, first match wins):
  {"name": "...", "sse": "<file>" | "json": "<file>", "status": <int, default 200>,
   "usage": {...}, "times": <n, default unlimited>,
   "header": {"<name>": "<regex>"}, "no_header": ["<name>"],
   "model": "<regex>", "last": "<regex on the last non-system message as JSON>",
   "body": "<regex on the whole body as JSON>", "path": "<regex>"}
Relative "sse" and "json" paths resolve against the rules file folder.
"""
import argparse
import json
import os
import re
import sys
import threading
from urllib.parse import urlsplit
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOCK = threading.Lock()
STATE = {"n": 0, "uses": {}}


SUMMARY = ("<analysis>The conversation so far: the user sent tracer turns; the assistant answered and ran "
           "one command.</analysis>\n<summary>1. Primary request: answer the tracer turns.\n"
           "2. Work done: one command ran and printed its tracer output.\n"
           "3. Pending: none.\n4. Next step: wait for the next user message.</summary>")


def default_sse(model, text="ok"):
    return "".join(
        "event: %s\ndata: %s\n\n" % (e, json.dumps(d))
        for e, d in [
            ("message_start", {"type": "message_start", "message": {
                "id": "msg_mock_default", "type": "message", "role": "assistant",
                "model": model, "content": [], "stop_reason": None, "stop_sequence": None,
                "usage": {"input_tokens": 10, "output_tokens": 1}}}),
            ("content_block_start", {"type": "content_block_start", "index": 0,
                                     "content_block": {"type": "text", "text": ""}}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0,
                                     "delta": {"type": "text_delta", "text": text}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            ("message_delta", {"type": "message_delta",
                               "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                               "usage": {"output_tokens": 1}}),
            ("message_stop", {"type": "message_stop"}),
        ])


def default_responses_sse(model: str) -> str:
    rid = "resp_mock_default"
    item = {"type": "message", "id": "msg_mock_default", "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": "ok", "annotations": []}]}
    return "".join(
        "event: %s\ndata: %s\n\n" % (e, json.dumps(d))
        for e, d in [
            ("response.created", {"type": "response.created", "response": {
                "id": rid, "object": "response", "model": model, "status": "in_progress", "output": []}}),
            ("response.output_text.delta", {"type": "response.output_text.delta", "item_id": item["id"],
                                            "output_index": 0, "content_index": 0, "delta": "ok"}),
            ("response.output_item.done", {"type": "response.output_item.done", "output_index": 0,
                                           "item": item}),
            ("response.completed", {"type": "response.completed", "response": {
                "id": rid, "object": "response", "model": model, "status": "completed", "output": [item],
                "usage": {"input_tokens": 10, "output_tokens": 1, "total_tokens": 11}}}),
        ])


def set_usage(sse: str, usage: dict) -> str:
    """Replace response.usage in the data JSON of the response.completed event only."""
    out = []
    for block in sse.split("\n\n"):
        lines = block.split("\n")
        if "event: response.completed" in lines:
            for i, line in enumerate(lines):
                if line.startswith("data: "):
                    d = json.loads(line[6:])
                    d.setdefault("response", {})["usage"] = usage
                    lines[i] = "data: " + json.dumps(d)
        out.append("\n".join(lines))
    return "\n\n".join(out)


def endpoint(method, path):
    """The route a rule may answer: "messages", "responses", "models", or None."""
    if method == "POST" and re.search(r"/v1/messages(\?|$)", path):
        return "messages"
    if method == "POST" and re.search(r"/v1/responses(\?|$)", path):
        return "responses"
    if method == "GET" and re.search(r"/v1/models(\?|$)", path):
        return "models"
    return None


def default_json(model):
    return {"id": "msg_mock_default", "type": "message", "role": "assistant", "model": model,
            "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn",
            "stop_sequence": None, "usage": {"input_tokens": 10, "output_tokens": 1}}


def matches(rule, route, path, headers, body):
    if "path" not in rule:
        if route != "messages":
            return False
    elif not re.search(rule["path"], path):
        return False
    for name, rx in rule.get("header", {}).items():
        v = headers.get(name.lower())
        if v is None or not re.search(rx, v):
            return False
    for name in rule.get("no_header", []):
        if name.lower() in headers:
            return False
    if not isinstance(body, dict):
        return not any(k in rule for k in ("model", "last", "body"))
    if "model" in rule and not re.search(rule["model"], str(body.get("model", ""))):
        return False
    if "last" in rule:
        msgs = [m for m in body.get("messages") or [] if not (isinstance(m, dict) and m.get("role") == "system")] or [None]
        if not re.search(rule["last"], json.dumps(msgs[-1])):
            return False
    if "body" in rule and not re.search(rule["body"], json.dumps(body)):
        return False
    return True


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def _send(self, code, ctype, data):
        self.send_response(code)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _refuse(self, line):
        """Refuse a proxy request (CONNECT or absolute URI): record the line, 403, close."""
        with LOCK:
            with open(os.path.join(self.server.cfg["dir"], "connect.txt"), "a") as f:
                f.write(line + "\n")
        self.close_connection = True
        data = b'{"error":{"message":"mock: proxy request refused"}}'
        self.send_response(403)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def do_CONNECT(self):
        self._refuse("%s refused" % self.path)

    def _absolute_uri(self):
        """An absolute-URI request line (a plain-HTTP proxy request): refuse it, return True.
        One to the mock's own address (127.0.0.1, localhost or [::1] and its port) becomes an
        origin request to that path (False)."""
        if not re.match(r"(?i)https?://", self.path):
            return False
        u = urlsplit(self.path)
        host = u.hostname or ""
        try:
            port = u.port
        except ValueError:
            port = None
        if port is None:
            port = 443 if u.scheme.lower() == "https" else 80
        if host.lower() in ("127.0.0.1", "localhost", "::1") and port == self.server.server_address[1]:
            self.path = (u.path or "/") + ("?" + u.query if u.query else "")
            return False
        if ":" in host:
            host = "[%s]" % host
        self._refuse("%s:%d %s refused" % (host, port, self.command))
        return True

    def _record_only(self, headers, raw):
        with LOCK:
            STATE["n"] += 1
            idx = STATE["n"]
        rec = {"seq": idx, "method": self.command, "path": self.path, "headers": headers,
               "credential_headers": [], "body": raw.decode("utf-8", "replace") or None,
               "reply": "401-no-credential"}
        with open(os.path.join(self.server.cfg["dir"], "%04d.json" % idx), "w") as f:
            json.dump(rec, f, indent=2)

    def handle_any(self):
        if self._absolute_uri():
            return
        cfg = self.server.cfg
        n = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(n) if n else b""
        headers = {k.lower(): v for k, v in self.headers.items()}
        key = cfg["key"]
        got_key = headers.get("x-api-key")
        auth = headers.get("authorization")
        if got_key is None and auth is None:
            # No credential at all (not a leaked login): record it, answer 401, go on.
            self._record_only(headers, raw)
            self._send(401, "application/json", b'{"type":"error","error":{"type":"authentication_error","message":"mock: no credential"}}')
            return
        ok = (got_key == key and auth is None) or (auth == "Bearer " + key and got_key is None) \
            or (got_key == key and auth == "Bearer " + key)
        if not ok:
            with open(os.path.join(cfg["dir"], "ABORT"), "w") as f:
                f.write("request %s %s carried a credential other than the dummy\n"
                        % (self.command, self.path))
            self._send(401, "application/json", b'{"type":"error","error":{"type":"authentication_error","message":"mock: not the dummy key"}}')
            threading.Thread(target=self.server.shutdown, daemon=True).start()
            return
        try:
            body = json.loads(raw) if raw else None
        except ValueError:
            body = raw.decode("utf-8", "replace")
        rec_headers = {k: ("[omitted]" if k in ("authorization", "x-api-key") else v)
                       for k, v in headers.items()}
        path = self.path
        model = body.get("model", "claude-mock") if isinstance(body, dict) else "claude-mock"
        rule_name = None
        rule = None
        route = endpoint(self.command, path)
        with LOCK:
            STATE["n"] += 1
            idx = STATE["n"]
            if route is not None:
                for i, r in enumerate(cfg["rules"]):
                    left = r.get("times")
                    used = STATE["uses"].get(i, 0)
                    if left is not None and used >= left:
                        continue
                    if matches(r, route, path, headers, body):
                        STATE["uses"][i] = used + 1
                        rule_name = r.get("name", "rule%d" % i)
                        rule = r
                        break
                if rule is None and route == "messages" and "x-claude-code-compaction" in headers:
                    rule_name = "default-summary"
        rec = {"seq": idx, "method": self.command, "path": path, "headers": rec_headers,
               "credential_headers": sorted(k for k in ("authorization", "x-api-key") if k in headers),
               "body": body, "reply": rule_name or "default"}
        with open(os.path.join(cfg["dir"], "%04d.json" % idx), "w") as f:
            json.dump(rec, f, indent=2)
        msgid = "msg_mock_%04d" % idx
        if rule is not None:
            with open(rule.get("sse") or rule["json"]) as f:
                text = f.read().replace("{{MODEL}}", model).replace("{{MSGID}}", msgid)
            status = int(rule.get("status", 200))
            if "sse" not in rule:
                self._send(status, "application/json", text.encode())
                return
            if "usage" in rule:
                text = set_usage(text, rule["usage"])
            if route == "messages" and not (isinstance(body, dict) and body.get("stream")):
                self._send(200, "application/json", json.dumps(default_json(model)).encode())
                return
            self._send(status, "text/event-stream", text.encode())
            return
        if route == "responses":
            self._send(200, "text/event-stream", default_responses_sse(model).encode())
            return
        if self.command != "POST":
            self._send(404, "application/json", b'{"type":"error","error":{"type":"not_found_error","message":"mock"}}')
            return
        if "count_tokens" in path:
            self._send(200, "application/json", b'{"input_tokens": 10}')
            return
        if not re.search(r"/v1/messages(\?|$)", path):
            self._send(404, "application/json", b'{"type":"error","error":{"type":"not_found_error","message":"mock"}}')
            return
        stream = isinstance(body, dict) and body.get("stream")
        if not stream:
            self._send(200, "application/json", json.dumps(default_json(model)).encode())
            return
        text = default_sse(model, SUMMARY if rule_name == "default-summary" else "ok")
        text = text.replace("{{MODEL}}", model).replace("{{MSGID}}", msgid)
        self._send(200, "text/event-stream", text.encode())

    do_POST = handle_any
    do_GET = handle_any
    do_HEAD = handle_any
    do_PUT = handle_any
    do_DELETE = handle_any


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--rules")
    ap.add_argument("--port-file")
    a = ap.parse_args()
    key = os.environ.get("MOCK_KEY")
    if not key:
        sys.exit("MOCK_KEY not set")
    rules = []
    if a.rules:
        with open(a.rules) as f:
            rules = json.load(f)
        base = os.path.dirname(os.path.abspath(a.rules))
        for i, r in enumerate(rules):
            if "sse" not in r and "json" not in r:
                sys.exit("mock: rule %d needs sse or json" % i)
            for k in ("sse", "json"):
                if k in r and not os.path.isabs(r[k]):
                    r[k] = os.path.join(base, r[k])
    os.makedirs(a.dir, exist_ok=True)
    srv = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    srv.cfg = {"dir": a.dir, "rules": rules, "key": key}
    if a.port_file:
        with open(a.port_file + ".tmp", "w") as f:
            f.write(str(srv.server_address[1]))
        os.rename(a.port_file + ".tmp", a.port_file)
    srv.serve_forever()


if __name__ == "__main__":
    main()
