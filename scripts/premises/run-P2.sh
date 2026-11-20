#!/bin/sh
# P2 and Q3: modelPicker rows with behavesAs. Compares the request for --model lead (row with
# behavesAs, row without) with the Claude id sent directly, and the window /context shows.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
mkdir -p "$FX/requests" "$FX/p2"
new_run P2
start_mock
for target in claude-opus-5-5 claude-haiku-4-5; do
  printf '{"modelPicker":{"options":[{"model":"lead","label":"Lead","description":"lead role","behavesAs":"%s"}]}}\n' "$target" >"$RUN/s-with-$target.json"
done
echo '{"modelPicker":{"options":[{"model":"lead","label":"Lead","description":"lead role"}]}}' >"$RUN/s-without.json"
# Q3: the row form of spec §8.5 (id instead of model).
echo '{"modelPicker":{"options":[{"id":"lead","label":"Lead","description":"lead role","behavesAs":"claude-haiku-4-5"}]}}' >"$RUN/s-id-row.json"
echo '{"modelPicker":[{"model":"lead","label":"Lead","behavesAs":"claude-haiku-4-5"}]}' >"$RUN/s-array.json"

ctx() { # <name> <claude args>
  n=$1; shift
  run_claude -p "/context" "$@" </dev/null >"$RUN/context-$n.txt" 2>&1 || true
  sed -n '1,6p' "$RUN/context-$n.txt" >"$RUN/context-$n.head"
  scrub "$RUN/context-$n.head" >"$FX/p2/context-$n.txt"
}
req() { # <name> <claude args>: one main request with tracer text; keep it as a fixture
  n=$1; shift
  before=$(ls "$RUN/rec" | grep -c '^[0-9]' || true)
  run_claude -p "tracer-p2-$n" "$@" </dev/null >"$RUN/out-$n.txt" 2>&1 || true
  f=$(grep -l "tracer-p2-$n" "$RUN"/rec/[0-9]*.json | tail -n 1)
  scrub "$f" >"$FX/requests/p2-$n.json"
}
for target in claude-opus-5-5 claude-haiku-4-5; do
  ctx "lead-behavesas-$target" --model lead --settings "$RUN/s-with-$target.json"
  ctx "direct-$target" --model "$target"
  req "lead-behavesas-$target" --model lead --settings "$RUN/s-with-$target.json"
  req "direct-$target" --model "$target"
done
ctx lead-no-behavesas --model lead --settings "$RUN/s-without.json"
req lead-no-behavesas --model lead --settings "$RUN/s-without.json"
ctx lead-id-row --model lead --settings "$RUN/s-id-row.json"
ctx lead-array --model lead --settings "$RUN/s-array.json"
# Interactive check of the validation message for the id-row form (print mode ignores invalid settings silently).
stop_mock
python3 -I - "$FX/requests" >"$RUN/p2-compare.json" <<'PY'
import json, os, sys
d = sys.argv[1]
out = {}
for f in sorted(os.listdir(d)):
    if not f.startswith("p2-"):
        continue
    r = json.load(open(os.path.join(d, f)))
    b = r["body"]
    out[f] = {"model": b.get("model"), "max_tokens": b.get("max_tokens"), "thinking": b.get("thinking"),
              "output_config": b.get("output_config"), "context_management": b.get("context_management"),
              "anthropic-beta": r["headers"].get("anthropic-beta")}
print(json.dumps(out, indent=1))
PY
scrub "$RUN/p2-compare.json" >"$FX/p2/compare.json"
echo "P2 done"
