#!/bin/sh
# PO1 and PO2 (node OC-1), OpenCode 1.18.16.
# PO1: a provider's options.apiKey given as {file:<absolute path>} in OPENCODE_CONFIG_CONTENT is read
#      from that file; the inline config text holds no key; after all runs no file under the run folder
#      (the scratch HOME and TMPDIR are in it) holds the session key, except the token file itself.
#      The session key is made at run time, differs from the vendor dummy key, and is written only to
#      the token file; the zygo mock accepts only it, so an accepted x-api-key came from the {file:} read.
# PO2: with enabled_providers = ["zygo"] only zygo/ models list and no request goes to the vendor mock,
#      while the control (no enabled_providers) lists catalog, env-key/stored-login and config providers.
#      The user config.json and the project opencode.json both disable zygo and one more provider. The
#      script plants the inline disabled_providers that zygo computes for them (crates/zygo-launch/src/
#      opencode.rs user_disabled: the union of the lists without "zygo", config.json first), which replaces
#      the earlier lists (arrays replace on merge): zygo/lead lists and answers. Two controls, each with
#      "zygo" in only one of the two files (the other sets no disabled_providers) and no inline
#      disabled_providers, list no model, so each file is read. A third control (zygo in the user list, a
#      project list without it) lists zygo/lead: a later list replaces an earlier one.
# Isolation: env -i, a scratch HOME, XDG_* and TMPDIR, two mocks (zygo: only the session key; vendor: only
# the dummy key; any other key aborts), HTTP(S)_PROXY at a closed local port so that no request reaches
# a real host. /tmp/opencode (OpenCode's temp folder without TMPDIR) is recorded before and after.
# OC1_STUB=1: a stub opencode; no version check, no result checks, no fixture; isolation checks only.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/opencode/oc1"
new_run OC1
PROXY=http://127.0.0.1:9
TMPD="$RUN/tmp"
mkdir -p "$TMPD"
SESSION_KEY="oc1-session-$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')"

iso_env() { # the isolated environment for OpenCode
  echo env -i PATH="$TOOL_PATH" TERM=xterm-256color HOME="$H" XDG_CONFIG_HOME="$H/.config" \
    XDG_DATA_HOME="$H/.local/share" XDG_CACHE_HOME="$H/.cache" XDG_STATE_HOME="$H/.local/state" \
    TMPDIR="$TMPD" OPENCODE_DISABLE_AUTOUPDATE=1 OPENAI_API_KEY="$DUMMY_KEY" \
    HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY" http_proxy="$PROXY" https_proxy="$PROXY" \
    NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost
}
if [ -z "${OC1_STUB:-}" ]; then
  v=$($(iso_env) "$OPENCODE_BIN" --version </dev/null 2>&1 || true)
  [ "$v" = 1.18.16 ] || { echo "PO: opencode --version is '$v', not 1.18.16"; exit 1; }
fi

# /tmp/opencode before any run: absent, or its mtime; a marker for files changed after it.
tmp_state() { if [ -e /tmp/opencode ]; then echo "present, mtime $(stat -c %Y /tmp/opencode 2>/dev/null || stat -f %m /tmp/opencode)"; else echo absent; fi; }
tmp_state >"$RUN/tmp-opencode-before.txt"
touch "$RUN/start-mark"

# The mocks. zygo (rec/): a text reply to the PO1 run's main request (it has tools), every other request the
# default reply, only the session key. vendor (rec-vendor/): the user config's provider, only the dummy key.
mkdir -p "$RUN/sse" "$RUN/rec-vendor"
echo '{"trigger": "tracer-po1", "extra": {"body": "\"tools\""}, "steps": [{"text": "po1 done"}]}' >"$RUN/steps.json"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" po1 "$RUN/rules.json" "$RUN/steps.json"
mock() { # <rec dir> <key> <port file> <log> [rules]: start a mock; prints its pid
  MOCK_KEY="$2" python3 -I "$PREMISES_DIR/mock_upstream.py" --dir "$1" ${5:+--rules "$5"} --port-file "$3" >"$4" 2>&1 &
  echo $!
}
wait_port() { i=0; while [ ! -s "$1" ]; do i=$((i + 1)); [ $i -lt 100 ] || { echo "mock did not start"; exit 1; }; sleep 0.1; done; }
ZPID=$(mock "$RUN/rec" "$SESSION_KEY" "$RUN/port" "$RUN/mock.log" "$RUN/rules.json")
VPID=$(mock "$RUN/rec-vendor" "$DUMMY_KEY" "$RUN/port-vendor" "$RUN/mock-vendor.log")
wait_port "$RUN/port"; wait_port "$RUN/port-vendor"
MOCK_URL="http://127.0.0.1:$(cat "$RUN/port")"
VENDOR_URL="http://127.0.0.1:$(cat "$RUN/port-vendor")"
aborted() { for d in rec rec-vendor; do [ ! -e "$RUN/$d/ABORT" ] || { echo "ABORT ($d): $(cat "$RUN/$d/ABORT")" >&2; exit 3; }; done; }

# The key file (a space in its path on purpose), mode 600 before the key is written. It is the only file
# this script writes the session key to.
SD="$RUN/sess dir"
mkdir -p "$SD"
(umask 077 && printf '%s' "$SESSION_KEY" >"$SD/token")
chmod 600 "$SD/token"
# A user config with a vendor provider at the vendor mock, a stored openai login, and disabled_providers
# in the user config.json and in the project config (dis_files).
mkdir -p "$H/.config/opencode" "$H/.local/share/opencode" "$H/.local/state"
cat >"$H/.config/opencode/opencode.json" <<J
{"\$schema": "https://opencode.ai/config.json",
 "provider": {"anthropic": {"options": {"baseURL": "$VENDOR_URL/vendor/v1", "apiKey": "$DUMMY_KEY"},
   "models": {"probe-model": {"name": "probe-model"}}}}}
J
dis_files() { # <user config.json list> <project opencode.json list>; "-": the file sets no disabled_providers
  if [ "$1" = - ]; then echo '{}'; else echo "{\"disabled_providers\": $1}"; fi >"$H/.config/opencode/config.json"
  if [ "$2" = - ]; then echo '{"$schema": "https://opencode.ai/config.json"}'
  else echo "{\"\$schema\": \"https://opencode.ai/config.json\", \"disabled_providers\": $2}"; fi >"$R/opencode.json"
}
USER_DIS='["amazon-bedrock", "zygo"]' PROJ_DIS='["zygo", "google"]'
ZDIS='["amazon-bedrock", "google"]' # what zygo computes for USER_DIS and PROJ_DIS
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
ON=$(inline ', "enabled_providers": ["zygo"], "disabled_providers": '"$ZDIS")
OFF=$(inline ', "disabled_providers": '"$ZDIS")
NODIS=$(inline ', "enabled_providers": ["zygo"]')
echo "$ON" >"$RUN/inline-on.json"
echo "$OFF" >"$RUN/inline-off.json"

oc() { # <ON|OFF|NODIS> <args>: OpenCode isolated, in the repo, with that inline config
  eval "cfg=\$$1"; shift
  (cd "$R" && $(iso_env) OPENCODE_CONFIG_CONTENT="$cfg" "$OPENCODE_BIN" "$@" </dev/null)
  aborted
}
oc_bounded() { # <seconds> <out> <ON|OFF> <args>: oc with a time bound (a stuck run is stopped)
  s=$1 o=$2; shift 2
  oc "$@" >"$o" 2>&1 &
  p=$!; i=0
  while kill -0 $p 2>/dev/null && [ $i -lt $s ]; do sleep 1; i=$((i + 1)); done
  pkill -P $p 2>/dev/null || true; kill $p 2>/dev/null || true; wait $p 2>/dev/null || true
  aborted
}
dis_files '["amazon-bedrock", "zygo"]' -
oc NODIS models >"$RUN/models-nodis-user.txt" 2>"$RUN/models-nodis-user.err" || true
dis_files - '["zygo", "google"]'
oc NODIS models >"$RUN/models-nodis-project.txt" 2>"$RUN/models-nodis-project.err" || true
dis_files '["amazon-bedrock", "zygo"]' '["google"]'
oc NODIS models >"$RUN/models-nodis-replace.txt" 2>"$RUN/models-nodis-replace.err" || true
dis_files "$USER_DIS" "$PROJ_DIS"
oc ON models >"$RUN/models-on.txt" 2>"$RUN/models-on.err" || true
oc OFF models >"$RUN/models-off.txt" 2>"$RUN/models-off.err" || true
mark() { ls "$RUN/$1" | grep -c '^[0-9].*\.json$' || true; }
marks() { echo "$(mark rec) $(mark rec-vendor)"; }
m0=$(marks); oc_bounded 60 "$RUN/run-po1.out" ON run -m zygo/lead "tracer-po1"
m1=$(marks); oc_bounded 60 "$RUN/run-po2-on.out" ON run -m anthropic/probe-model "tracer-po2"
m2=$(marks); oc_bounded 60 "$RUN/run-po2-off.out" OFF run -m anthropic/probe-model "tracer-po2"
m3=$(marks)
for p in $ZPID $VPID; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
aborted
printf '%s\n' "$m0" "$m1" "$m2" "$m3" >"$RUN/marks.txt"
tmp_state >"$RUN/tmp-opencode-after.txt"
if [ -e /tmp/opencode ]; then find /tmp/opencode -newer "$RUN/start-mark" 2>/dev/null | wc -l | tr -d ' ' >"$RUN/tmp-opencode-changed.txt"
else echo 0 >"$RUN/tmp-opencode-changed.txt"; fi

fails=0
if [ -n "${OC1_STUB:-}" ]; then
  # Isolation: each HOME, XDG_* and TMPDIR value the stub saw is under the scratch dir; no inline text holds a key.
  E="$N2_SCRATCH/stub-env.txt"
  [ -s "$E" ] || { echo "isolation: fail: the stub wrote no environment ($E)"; exit 1; }
  bad=$(grep -E '^(HOME|XDG_[A-Z_]+|TMPDIR)=' "$E" | while IFS= read -r l; do
    case "${l#*=}" in ("$N2_SCRATCH"/*) ;; (*) echo "$l" ;; esac; done)
  if [ -z "$bad" ]; then echo "isolation: pass: HOME, XDG_* and TMPDIR under the scratch dir ($(grep -cE '^(HOME|XDG_[A-Z_]+|TMPDIR)=' "$E") values)"
  else echo "isolation: fail: $bad"; fails=1; fi
  if grep -qx "TMPDIR=$TMPD" "$E"; then echo "isolation: pass: TMPDIR=<run>/tmp"; else echo "isolation: fail: TMPDIR"; fails=1; fi
  for p in HTTP_PROXY HTTPS_PROXY; do
    if grep -qx "$p=$PROXY" "$E"; then echo "isolation: pass: $p=$PROXY"; else echo "isolation: fail: $p"; fails=1; fi
  done
  for c in "$ON" "$OFF" "$NODIS"; do
    case "$c" in *"$DUMMY_KEY"* | *"$SESSION_KEY"*) echo "isolation: fail: an inline config holds a key"; fails=1 ;;
      *) echo "isolation: pass: the inline config holds no key" ;; esac
  done
  exit $fails
fi

# A-1.2 at run time: every file under the run folder (scratch HOME and TMPDIR included) that holds the session
# key. No exclusion: the token file is expected in the list and removed from it only in the check.
grep -rlF -- "$SESSION_KEY" "$RUN" "$TMPD" 2>/dev/null | sed "s|^$RUN/||" | sort -u >"$RUN/key-files.txt" || true

USER_DIS="$USER_DIS" PROJ_DIS="$PROJ_DIS" python3 -I - "$RUN" "$SESSION_KEY" "$DUMMY_KEY" >"$RUN/result.json" <<'PY'
import json, os, sys
run, skey, dkey = sys.argv[1:4]
marks = [tuple(map(int, l.split())) for l in open(run + "/marks.txt").read().splitlines()]
def recs(d): return [json.load(open(os.path.join(run, d, f))) for f in sorted(os.listdir(os.path.join(run, d))) if f[0].isdigit()]
zrecs, vrecs = recs("rec"), recs("rec-vendor")
def between(rs, i, a, b): return [r for r in rs if marks[a][i] < r["seq"] <= marks[b][i]]
def lines(f): return [l.strip() for l in open(os.path.join(run, f)).read().splitlines() if l.strip()]
def text(f): return open(os.path.join(run, f)).read().strip()
def view(r): return {"seq": r["seq"], "method": r["method"], "path": r["path"],
                     "credential_headers": r["credential_headers"], "reply": r["reply"]}
on_inline = open(run + "/inline-on.json").read()
po1_recs = between(zrecs, 0, 0, 1)
po1_hit = [r for r in po1_recs if r["method"] == "POST" and r["path"].split("?")[0] == "/v1/messages"
           and r["credential_headers"] == ["x-api-key"] and r["reply"] != "401-no-credential"]
on, off = lines("models-on.txt"), lines("models-off.txt")
nodis_user, nodis_project = lines("models-nodis-user.txt"), lines("models-nodis-project.txt")
nodis_replace = lines("models-nodis-replace.txt")
on_vendor = [r for r in vrecs if r["seq"] <= marks[2][1]]
off_vendor = between(vrecs, 1, 2, 3)
key_files = lines("key-files.txt")
po1 = {"session key differs from the vendor dummy key": skey != dkey and dkey not in skey,
       "inline config holds a key": skey in on_inline or dkey in on_inline,
       "inline apiKey": json.loads(on_inline)["provider"]["zygo"]["options"]["apiKey"],
       "requests of the zygo/lead run (zygo mock: only the session key, any other key aborts)": [view(r) for r in po1_recs],
       "a POST /v1/messages with x-api-key accepted by the zygo mock": bool(po1_hit),
       "mock ABORT file (zygo, vendor)": [os.path.exists(run + "/rec/ABORT"), os.path.exists(run + "/rec-vendor/ABORT")],
       "run output has the scripted reply text": "po1 done" in open(run + "/run-po1.out").read(),
       "files holding the session key after all runs (search of the run folder and TMPDIR, no exclusion)": key_files,
       "TMPDIR (run folder tmp/) entries": sorted(os.listdir(run + "/tmp")),
       "/tmp/opencode before": text("tmp-opencode-before.txt"),
       "/tmp/opencode after": text("tmp-opencode-after.txt"),
       "/tmp/opencode entries changed during the run": int(text("tmp-opencode-changed.txt"))}
po1["of them, files other than the token file"] = [f for f in key_files if f != "sess dir/token"]
po1["pass"] = (po1["session key differs from the vendor dummy key"] and not po1["inline config holds a key"]
    and po1["a POST /v1/messages with x-api-key accepted by the zygo mock"]
    and not any(po1["mock ABORT file (zygo, vendor)"]) and po1["run output has the scripted reply text"]
    and key_files == ["sess dir/token"]
    and po1["/tmp/opencode before"] == po1["/tmp/opencode after"]
    and po1["/tmp/opencode entries changed during the run"] == 0)
po2 = {"user config.json disabled_providers": json.loads(os.environ["USER_DIS"]),
       "project opencode.json disabled_providers": json.loads(os.environ["PROJ_DIS"]),
       "inline disabled_providers (the value zygo computes for these files, planted by the script)": json.loads(on_inline)["disabled_providers"],
       "ON: models": on, "ON: every model is zygo/": bool(on) and all(l.startswith("zygo/") for l in on),
       "ON: zygo/lead answered (the PO1 run, same ON config)": po1["run output has the scripted reply text"],
       "ON: requests to the vendor mock": [view(r) for r in on_vendor],
       "ON: anthropic/probe-model run output (first 300 chars)": open(run + "/run-po2-on.out").read()[:300],
       "control: user config.json [\"amazon-bedrock\",\"zygo\"], project sets none, no inline disabled_providers: models": nodis_user,
       "control: user config.json sets none, project [\"zygo\",\"google\"], no inline disabled_providers: models": nodis_project,
       "control: user config.json [\"amazon-bedrock\",\"zygo\"], project [\"google\"], no inline disabled_providers: models": nodis_replace,
       "OFF: lists anthropic/probe-model": "anthropic/probe-model" in off,
       "OFF: lists an openai/ model": any(l.startswith("openai/") for l in off),
       "OFF: model count": len(off),
       "OFF: requests to the vendor mock": [view(r) for r in off_vendor]}
po2["pass"] = (po2["ON: every model is zygo/"] and "zygo/lead" in on and po2["ON: zygo/lead answered (the PO1 run, same ON config)"]
               and not on_vendor and not nodis_user and not nodis_project and "zygo/lead" in nodis_replace
               and po2["OFF: lists anthropic/probe-model"] and po2["OFF: lists an openai/ model"] and bool(off_vendor))
print(json.dumps({"PO1": po1, "PO2": po2}, indent=1))
PY
omit() { scrub "$1" | sed -e "s/$DUMMY_KEY/[omitted]/g" -e "s/$SESSION_KEY/[session key omitted]/g"; }
rm -rf "$FX"; mkdir -p "$FX"
omit "$RUN/result.json" >"$FX/result.json"
for f in models-on.txt models-off.txt inline-on.json run-po1.out run-po2-on.out; do
  omit "$RUN/$f" >"$FX/$f"
done
first=$(sed -n 1p "$RUN/marks.txt" | cut -d' ' -f1) last=$(sed -n 2p "$RUN/marks.txt" | cut -d' ' -f1)
for f in "$RUN"/rec/[0-9]*.json; do
  s=$(expr "$(basename "$f" .json)" + 0)
  if [ "$s" -gt "$first" ] && [ "$s" -le "$last" ] && jq -e '.method == "POST" and .reply == "po1-00"' "$f" >/dev/null; then
    omit "$f" >"$FX/po1-request.json"; break
  fi
done
for id in PO1 PO2; do
  if jq -e ".$id.pass" "$RUN/result.json" >/dev/null; then echo "$id: pass"
  else echo "$id: fail $(jq -c ".$id" "$FX/result.json" | cut -c1-600)"; fails=1; fi
done
jq -c '.PO1 | {"key files": .["files holding the session key after all runs (search of the run folder and TMPDIR, no exclusion)"], "tmp before": .["/tmp/opencode before"], "tmp after": .["/tmp/opencode after"], "tmp changed": .["/tmp/opencode entries changed during the run"]}' "$FX/result.json"
exit $fails
