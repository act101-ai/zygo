#!/bin/sh
# PO1 and PO2 (node OC-1), OpenCode 1.18.16.
# PO1: a provider's options.apiKey given as {file:<absolute path>} in OPENCODE_CONFIG_CONTENT is read
#      from that file; the inline config text holds no key.
# PO2: enabled_providers = ["zygo"] drops the catalog, environment-key, stored-login and config
#      providers: only zygo/ models list and no request goes to another provider.
# Isolation: env -i, a scratch HOME and XDG_*, the mock upstream (only the dummy key is accepted),
# HTTP(S)_PROXY at a closed local port so that no request reaches a real host.
# OC1_STUB=1: a stub opencode; no version check, no result checks, no fixture; isolation checks only.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/opencode/oc1"
new_run OC1
PROXY=http://127.0.0.1:9

iso_env() { # the isolated environment for OpenCode
  echo env -i PATH="$TOOL_PATH" TERM=xterm-256color HOME="$H" XDG_CONFIG_HOME="$H/.config" \
    XDG_DATA_HOME="$H/.local/share" XDG_CACHE_HOME="$H/.cache" XDG_STATE_HOME="$H/.local/state" \
    OPENCODE_DISABLE_AUTOUPDATE=1 OPENAI_API_KEY="$DUMMY_KEY" \
    HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY" http_proxy="$PROXY" https_proxy="$PROXY" \
    NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost
}
if [ -z "${OC1_STUB:-}" ]; then
  v=$($(iso_env) "$OPENCODE_BIN" --version </dev/null 2>&1 || true)
  [ "$v" = 1.18.16 ] || { echo "PO: opencode --version is '$v', not 1.18.16"; exit 1; }
fi

# The mock: a text reply to the PO1 run's main request (it has tools); every other request gets the default reply.
mkdir -p "$RUN/sse"
echo '{"trigger": "tracer-po1", "extra": {"body": "\"tools\""}, "steps": [{"text": "po1 done"}]}' >"$RUN/steps.json"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" po1 "$RUN/rules.json" "$RUN/steps.json"
start_mock "$RUN/rules.json"

# The key file (a space in its path on purpose), mode 600 before the key is written.
SD="$RUN/sess dir"
mkdir -p "$SD"
(umask 077 && printf '%s' "$DUMMY_KEY" >"$SD/token")
chmod 600 "$SD/token"
# A user config with a vendor provider at the mock's /vendor path, and a stored openai login.
mkdir -p "$H/.config/opencode" "$H/.local/share/opencode" "$H/.local/state"
cat >"$H/.config/opencode/opencode.json" <<J
{"\$schema": "https://opencode.ai/config.json",
 "provider": {"anthropic": {"options": {"baseURL": "$MOCK_URL/vendor/v1", "apiKey": "$DUMMY_KEY"},
   "models": {"probe-model": {"name": "probe-model"}}}}}
J
(umask 077 && printf '{"openai": {"type": "api", "key": "%s"}}\n' "$DUMMY_KEY" >"$H/.local/share/opencode/auth.json")
chmod 600 "$H/.local/share/opencode/auth.json"
KEYREF="{file:$SD/token}" # the key field: a reference to the key file, not the key
inline() { # [<more keys>]: the inline config
  cat <<J | python3 -I -c 'import json,sys; print(json.dumps(json.loads(sys.stdin.read())))'
{"\$schema": "https://opencode.ai/config.json", "model": "zygo/lead"$1,
 "provider": {"zygo": {"npm": "@ai-sdk/anthropic",
   "options": {"baseURL": "$MOCK_URL/v1", "apiKey": "$KEYREF"},
   "models": {"lead": {"limit": {"context": 200000, "output": 4000}}}}}}
J
}
ON=$(inline ', "enabled_providers": ["zygo"]')
OFF=$(inline '')
echo "$ON" >"$RUN/inline-on.json"
echo "$OFF" >"$RUN/inline-off.json"

oc() { # <ON|OFF> <args>: OpenCode isolated, in the repo, with that inline config
  eval "cfg=\$$1"; shift
  (cd "$R" && $(iso_env) OPENCODE_CONFIG_CONTENT="$cfg" "$OPENCODE_BIN" "$@" </dev/null)
  check_abort
}
oc_bounded() { # <seconds> <out> <ON|OFF> <args>: oc with a time bound (a stuck run is stopped)
  s=$1 o=$2; shift 2
  oc "$@" >"$o" 2>&1 &
  p=$!; i=0
  while kill -0 $p 2>/dev/null && [ $i -lt $s ]; do sleep 1; i=$((i + 1)); done
  pkill -P $p 2>/dev/null || true; kill $p 2>/dev/null || true; wait $p 2>/dev/null || true
  check_abort
}
oc ON models >"$RUN/models-on.txt" 2>"$RUN/models-on.err" || true
oc OFF models >"$RUN/models-off.txt" 2>"$RUN/models-off.err" || true
mark() { ls "$RUN/rec" | grep -c '^[0-9].*\.json$' || true; }
m0=$(mark); oc_bounded 60 "$RUN/run-po1.out" ON run -m zygo/lead "tracer-po1"
m1=$(mark); oc_bounded 60 "$RUN/run-po2-on.out" ON run -m anthropic/probe-model "tracer-po2"
m2=$(mark); oc_bounded 60 "$RUN/run-po2-off.out" OFF run -m anthropic/probe-model "tracer-po2"
m3=$(mark)
stop_mock
echo "$m0 $m1 $m2 $m3" >"$RUN/marks.txt"

fails=0
if [ -n "${OC1_STUB:-}" ]; then
  # Isolation: each HOME and XDG_* value the stub saw is under the scratch dir; no inline text holds the key.
  E="$N2_SCRATCH/stub-env.txt"
  [ -s "$E" ] || { echo "isolation: fail: the stub wrote no environment ($E)"; exit 1; }
  bad=$(grep -E '^(HOME|XDG_[A-Z_]+)=' "$E" | while IFS= read -r l; do
    case "${l#*=}" in ("$N2_SCRATCH"/*) ;; (*) echo "$l" ;; esac; done)
  if [ -z "$bad" ]; then echo "isolation: pass: HOME and XDG_* under the scratch dir ($(grep -cE '^(HOME|XDG_[A-Z_]+)=' "$E") values)"
  else echo "isolation: fail: $bad"; fails=1; fi
  for p in HTTP_PROXY HTTPS_PROXY; do
    if grep -qx "$p=$PROXY" "$E"; then echo "isolation: pass: $p=$PROXY"; else echo "isolation: fail: $p"; fails=1; fi
  done
  for c in "$ON" "$OFF"; do
    case "$c" in *"$DUMMY_KEY"*) echo "isolation: fail: an inline config holds the key"; fails=1 ;;
      *) echo "isolation: pass: the inline config holds no key" ;; esac
  done
  exit $fails
fi

python3 -I - "$RUN" "$DUMMY_KEY" >"$RUN/result.json" <<'PY'
import json, os, sys
run, key = sys.argv[1:3]
m0, m1, m2, m3 = map(int, open(run + "/marks.txt").read().split())
recs = [json.load(open(os.path.join(run, "rec", f))) for f in sorted(os.listdir(run + "/rec")) if f[0].isdigit()]
def lines(f): return [l.strip() for l in open(os.path.join(run, f)).read().splitlines() if l.strip()]
def view(r): return {"seq": r["seq"], "method": r["method"], "path": r["path"],
                     "credential_headers": r["credential_headers"], "reply": r["reply"]}
on_inline = open(run + "/inline-on.json").read()
po1_recs = [r for r in recs if m0 < r["seq"] <= m1]
po1_hit = [r for r in po1_recs if r["method"] == "POST" and r["path"].split("?")[0] == "/v1/messages"
           and r["credential_headers"] == ["x-api-key"] and r["reply"] != "401-no-credential"]
on, off = lines("models-on.txt"), lines("models-off.txt")
on_vendor = [r for r in recs if r["seq"] <= m2 and r["path"].startswith("/vendor/")]
off_vendor = [r for r in recs if m2 < r["seq"] <= m3 and r["path"].startswith("/vendor/")]
po1 = {"inline config holds the key": key in on_inline,
       "inline apiKey": json.loads(on_inline)["provider"]["zygo"]["options"]["apiKey"],
       "requests of the zygo/lead run": [view(r) for r in po1_recs],
       "a POST /v1/messages with x-api-key accepted (the mock aborts on any key but the dummy)": bool(po1_hit),
       "mock ABORT file": os.path.exists(run + "/rec/ABORT"),
       "run output has the scripted reply text": "po1 done" in open(run + "/run-po1.out").read()}
po1["pass"] = (not po1["inline config holds the key"] and po1[
    "a POST /v1/messages with x-api-key accepted (the mock aborts on any key but the dummy)"]
    and not po1["mock ABORT file"] and po1["run output has the scripted reply text"])
po2 = {"ON: models": on, "ON: every model is zygo/": bool(on) and all(l.startswith("zygo/") for l in on),
       "ON: requests to /vendor/": [view(r) for r in on_vendor],
       "ON: anthropic/probe-model run output (first 300 chars)": open(run + "/run-po2-on.out").read()[:300],
       "OFF: lists anthropic/probe-model": "anthropic/probe-model" in off,
       "OFF: lists an openai/ model": any(l.startswith("openai/") for l in off),
       "OFF: model count": len(off),
       "OFF: requests to /vendor/": [view(r) for r in off_vendor]}
po2["pass"] = (po2["ON: every model is zygo/"] and not on_vendor and po2["OFF: lists anthropic/probe-model"]
               and po2["OFF: lists an openai/ model"] and bool(off_vendor))
print(json.dumps({"PO1": po1, "PO2": po2}, indent=1))
PY
rm -rf "$FX"; mkdir -p "$FX"
scrub "$RUN/result.json" | sed "s/$DUMMY_KEY/[omitted]/g" >"$FX/result.json"
for f in models-on.txt models-off.txt inline-on.json run-po1.out run-po2-on.out; do
  scrub "$RUN/$f" | sed "s/$DUMMY_KEY/[omitted]/g" >"$FX/$f"
done
for f in "$RUN"/rec/[0-9]*.json; do
  s=$(expr "$(basename "$f" .json)" + 0)
  if [ "$s" -gt "$(cut -d' ' -f1 "$RUN/marks.txt")" ] && [ "$s" -le "$(cut -d' ' -f2 "$RUN/marks.txt")" ] &&
    jq -e '.method == "POST" and .reply == "po1-00"' "$f" >/dev/null; then
    scrub "$f" | sed "s/$DUMMY_KEY/[omitted]/g" >"$FX/po1-request.json"; break
  fi
done
for id in PO1 PO2; do
  if jq -e ".$id.pass" "$RUN/result.json" >/dev/null; then echo "$id: pass"
  else echo "$id: fail $(jq -c ".$id" "$FX/result.json" | cut -c1-400)"; fails=1; fi
done
exit $fails
