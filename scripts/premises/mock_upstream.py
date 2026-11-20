#!/usr/bin/env python3
"""Mock Anthropic upstream for the N2 premise experiments (Python 3 stdlib only).

Usage: mock_upstream.py --dir <record dir> --rules <rules.json> [--port-file <file>]

- Listens on 127.0.0.1 (a free port, written to --port-file).
- Accepts only the dummy credential from MOCK_KEY (x-api-key or "Bearer <key>").
  Any other credential: HTTP 401, an ABORT file in the record dir, and the mock stops. A request
  with no credential at all is recorded and answered 401 without aborting.
- A compaction request (x-claude-code-compaction) that no rule matches gets a realistic
  <analysis>/<summary> reply instead of the one-word default.
- Records each request (method, path, headers, JSON body) as <dir>/<nnnn>.json, with
  authorization and x-api-key values written as "[omitted]".
- Replies with the SSE file of the first rule that matches and has uses left, else a
  plain short end_turn reply. SSE files may use {{MODEL}} and {{MSGID}}.

Rule (JSON list, first match wins):
  {"name": "...", "sse": "<file>", "times": <n, default unlimited>,
   "header": {"<name>": "<regex>"}, "no_header": ["<name>"],
   "model": "<regex>", "last": "<regex on the last non-system message as JSON>",
   "body": "<regex on the whole body as JSON>", "path": "<regex>"}
"""
import argparse
import json
import os
import re
import sys
import threading
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


def default_json(model):
    return {"id": "msg_mock_default", "type": "message", "role": "assistant", "model": model,
            "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn",
            "stop_sequence": None, "usage": {"input_tokens": 10, "output_tokens": 1}}


def matches(rule, path, headers, body):
    if "path" in rule and not re.search(rule["path"], path):
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
        sse = None
        with LOCK:
            STATE["n"] += 1
            idx = STATE["n"]
            if self.command == "POST" and re.search(r"/v1/messages(\?|$)", path):
                for i, rule in enumerate(cfg["rules"]):
                    left = rule.get("times")
                    used = STATE["uses"].get(i, 0)
                    if left is not None and used >= left:
                        continue
                    if matches(rule, path, headers, body):
                        STATE["uses"][i] = used + 1
                        rule_name = rule.get("name", "rule%d" % i)
                        with open(rule["sse"]) as f:
                            sse = f.read()
                        break
                if sse is None and "x-claude-code-compaction" in headers:
                    rule_name = "default-summary"
        rec = {"seq": idx, "method": self.command, "path": path, "headers": rec_headers,
               "credential_headers": sorted(k for k in ("authorization", "x-api-key") if k in headers),
               "body": body, "reply": rule_name or "default"}
        with open(os.path.join(cfg["dir"], "%04d.json" % idx), "w") as f:
            json.dump(rec, f, indent=2)
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
        if rule_name == "default-summary":
            text = default_sse(model, SUMMARY)
        else:
            text = sse if sse is not None else default_sse(model)
        text = text.replace("{{MODEL}}", model).replace("{{MSGID}}", "msg_mock_%04d" % idx)
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
        for r in rules:
            if not os.path.isabs(r["sse"]):
                r["sse"] = os.path.join(base, r["sse"])
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
