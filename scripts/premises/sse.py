#!/usr/bin/env python3
"""Author a synthetic Anthropic SSE reply for the mock (N2 experiments).

sse.py text "<text>" [options]            -> one text block, stop_reason end_turn
sse.py tool <name> <id> '<input json>' [options] -> one tool_use block, stop_reason tool_use
Options:
  --usage '<json>'   usage of the message_delta event (default {"output_tokens": 5})
  --iterations       also write the usage as the last entry of usage.iterations
  --start-usage '<json>'  usage of message_start (default input 10, output 1)
The model is written as {{MODEL}} and the id as {{MSGID}}; the mock fills them.
"""
import argparse
import json

ap = argparse.ArgumentParser()
ap.add_argument("kind", choices=["text", "tool"])
ap.add_argument("args", nargs="+")
ap.add_argument("--usage", default='{"output_tokens": 5}')
ap.add_argument("--start-usage", default='{"input_tokens": 10, "output_tokens": 1}')
ap.add_argument("--iterations", action="store_true")
a = ap.parse_args()

usage = json.loads(a.usage)
if a.iterations:
    it = {"type": "message"}
    it.update({k: v for k, v in usage.items() if k != "iterations"})
    usage["iterations"] = [it]
events = [("message_start", {"type": "message_start", "message": {
    "id": "{{MSGID}}", "type": "message", "role": "assistant", "model": "{{MODEL}}",
    "content": [], "stop_reason": None, "stop_sequence": None,
    "usage": json.loads(a.start_usage)}})]
if a.kind == "text":
    events += [
        ("content_block_start", {"type": "content_block_start", "index": 0,
                                 "content_block": {"type": "text", "text": ""}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0,
                                 "delta": {"type": "text_delta", "text": a.args[0]}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0})]
    stop = "end_turn"
else:
    name, tid, inp = a.args[0], a.args[1], a.args[2]
    json.loads(inp)
    events += [
        ("content_block_start", {"type": "content_block_start", "index": 0,
                                 "content_block": {"type": "tool_use", "id": tid, "name": name, "input": {}}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0,
                                 "delta": {"type": "input_json_delta", "partial_json": inp}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0})]
    stop = "tool_use"
events += [
    ("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None},
                       "usage": usage}),
    ("message_stop", {"type": "message_stop"})]
print("".join("event: %s\ndata: %s\n\n" % (e, json.dumps(d)) for e, d in events), end="")
