#!/usr/bin/env python3
"""mcp_list.py [--call <tool>] <command> [args...]: start a stdio MCP server, initialize, print
tools/list JSON (names and description starts), and with --call the result of that tool (no arguments)."""
import json
import subprocess
import sys
import threading

args = sys.argv[1:]
call = None
if args[:1] == ["--call"]:
    call, args = args[1], args[2:]
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)


def send(m):
    p.stdin.write(json.dumps(m) + "\n")
    p.stdin.flush()


def recv(want_id):
    for line in p.stdout:
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == want_id:
            return m
    return None


timer = threading.Timer(30, p.kill)
timer.start()
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "n2-probe", "version": "0"}}})
init = recv(1)
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
tools = recv(2)
called = None
if call:
    send({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": call, "arguments": {}}})
    called = recv(3)
timer.cancel()
p.kill()
print(json.dumps({"serverInfo": (init or {}).get("result", {}).get("serverInfo"),
                  "tools": [{"name": t["name"], "description": t.get("description", "")[:200]}
                            for t in ((tools or {}).get("result") or {}).get("tools", [])],
                  **({"call": {"tool": call, "result": (called or {}).get("result")}} if call else {})}, indent=1))
