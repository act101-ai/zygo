#!/usr/bin/env python3
"""drive_stream.py <out.jsonl> <messages file, one per line> -- <command ...>

Runs the command (Claude Code with --input-format stream-json --output-format stream-json),
sends each user message as one stream-json line, waits for the "result" line before the
next, then closes stdin. Writes every stdout line to <out.jsonl>. Timeout 120 s per turn.
"""
import json
import subprocess
import sys
import threading

out = sys.argv[1]
i = sys.argv.index("--")
msgs = [l.rstrip("\n") for l in open(sys.argv[2]) if l.strip()]
cmd = sys.argv[i + 1:]
p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
results = threading.Semaphore(0)


def reader():
    with open(out, "w") as f:
        for line in p.stdout:
            f.write(line)
            f.flush()
            try:
                if json.loads(line).get("type") == "result":
                    results.release()
            except ValueError:
                pass


t = threading.Thread(target=reader, daemon=True)
t.start()
for m in msgs:
    p.stdin.write(json.dumps({"type": "user", "message": {"role": "user", "content": m}}) + "\n")
    p.stdin.flush()
    if not results.acquire(timeout=120):
        print("drive_stream: no result for %r" % m, file=sys.stderr)
        p.kill()
        sys.exit(1)
p.stdin.close()
p.wait(timeout=60)
t.join(timeout=5)
sys.exit(p.returncode)
