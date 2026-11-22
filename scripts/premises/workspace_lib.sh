# Record helpers of live-workspace.sh (node P2), also sourced by workspace_test.sh.
# Needs: REC (record folder), SOCK and N2_SCRATCH (text that clean replaces; may be empty),
# scrub <file> (lib.sh) and result <id> <word> <text> (live.sh), REPO_DIR.

# clean <text>: one line; the private socket and the scratch folder named by placeholders
clean() {
  _c=$(printf '%s' "$1" | tr '\n' ' ')
  [ -z "${SOCK:-}" ] || _c=$(printf '%s' "$_c" | sed "s#$SOCK#SOCKET#g")
  [ -z "${N2_SCRATCH:-}" ] || _c=$(printf '%s' "$_c" | sed "s#$N2_SCRATCH#SCRATCH#g")
  printf '%s' "$_c"
}
# sub <PW id> <sub id> <0 = pass, other = fail> <detail>; note <PW id> <sub id> <detail> (not part of the verdict)
sub() {
  if [ "$3" -eq 0 ] 2>/dev/null; then printf '%s ok %s\n' "$2" "$(clean "$4")" >>"$REC/$1.txt"
  else printf '%s FAIL %s\n' "$2" "$(clean "$4")" >>"$REC/$1.txt"; printf ' %s' "$2" >>"$REC/$1.bad"; fi
}
note() { printf '%s note %s\n' "$2" "$(clean "$3")" >>"$REC/$1.txt"; }
# rest <reason> <PW id>:<sub id>...: the sub-checks that a failed step did not reach
rest() { _r=$1; shift; for _x in "$@"; do sub "${_x%%:*}" "${_x#*:}" 1 "not reached: $_r"; done; }

# pw_ticks: polls per second of a wait bound. 10, or PW_TICKS when PW_TEST=1 and PW_TICKS is a positive
# integer (tests only); any other PW_TICKS is ignored.
pw_ticks() {
  if [ "${PW_TEST:-}" = 1 ]; then case "${PW_TICKS:-}" in "" | 0* | *[!0-9]*) ;; *) echo "$PW_TICKS"; return ;; esac; fi
  echo 10
}
# jv <json text> <jq path> <default>: one value of an analysis output, or the default
jv() { _v=$(printf '%s' "$1" | jq -r "$2 // empty" 2>/dev/null); printf '%s\n' "${_v:-$3}"; }

# rec_2_3 <output of workspace_analyze.py pw2-sub>: the 2.3 line; prints the hook file to publish or none
rec_2_3() {
  _n=$(jv "$1" .inside 0); _nb=$(jv "$1" .without_agent_id 0)
  _t=1; [ "$_n" -ge 1 ] && [ "$_nb" -eq 0 ] && _t=0
  sub PW2 2.3 $_t "SessionStart inputs inside the subagent (between its SubagentStart and SubagentStop): $_n, source $(jv "$1" .sources none); without an agent_id key: $_nb"
  jv "$1" .first none
}
# rec_7_345 <output of workspace_analyze.py pw7>: the 7.3, 7.4 and 7.5 lines
rec_7_345() {
  _tu=$(jv "$1" .turns 0); _op=$(jv "$1" .open 0)
  _t=1; [ "$_tu" -ge 6 ] && [ "$_op" -eq 0 ] && _t=0
  sub PW7 7.3 $_t "main-loop turns (UserPromptSubmit inputs): $_tu; turns with no Stop or StopFailure before the next prompt: $_op (prompts: $(jv "$1" '.open_prompts | join(", ")' none))"
  _di=$(jv "$1" .disp 0); _db=$(jv "$1" .disp_bad 0)
  _t=1; [ "$_di" -ge 2 ] && [ "$_db" -eq 0 ] && _t=0
  sub PW7 7.4 $_t "dispatched subagents (one foreground, one background): ids seen $_di; ids without both SubagentStart and SubagentStop: $_db ($(jv "$1" .disp_kinds none))"
  _ii=$(jv "$1" .internal 0); _ib=$(jv "$1" .internal_bad 0)
  _t=1; [ "$_ii" -ge 1 ] && [ "$_ib" -eq 0 ] && _t=0
  sub PW7 7.5 $_t "internal subagents (empty agent_type): ids seen $_ii, of which $(jv "$1" .internal_compaction 0) sent SubagentStop between PreCompact and PostCompact (compaction); ids without both SubagentStart and SubagentStop: $_ib ($(jv "$1" .internal_kinds none))"
}

# finish <PW id> <agent version text> <fixture file> <first line> <expected sub ids> [<path to print>]:
# a sub-check of the list with no ok or FAIL line becomes FAIL "not reached: no result line"; then the
# record goes through scrub to the fixture file and one LIVE line is printed.
finish() {
  _id=$1; LIVE_VER=$2; _fx=$3; _head=$4; _want=$5; _show=${6:-$3}
  touch "$REC/$_id.txt"
  for _s in $_want; do
    grep -Eq "^$(printf '%s' "$_s" | sed 's/[.]/[.]/g') (ok|FAIL) " "$REC/$_id.txt" || sub "$_id" "$_s" 1 "not reached: no result line"
  done
  { echo "$_head"; cat "$REC/$_id.txt"; } >"$REC/$_id.out"
  scrub "$REC/$_id.out" >"$_fx"
  _n=$(grep -Ec '^[^ ]+ (ok|FAIL) ' "$REC/$_id.txt"); _b=$(cat "$REC/$_id.bad" 2>/dev/null); _nb=$(set -- $_b; echo $#)
  _rel=${_show#"$REPO_DIR"/}
  if [ "$_n" -gt 0 ] && [ -z "$_b" ]; then result "$_id" verified "$_n of $_n sub-checks passed; $_rel"
  else result "$_id" failed "$((_n - _nb)) of $_n sub-checks passed; failed:${_b:- none ran}; first: $(sed -n 's/^[^ ]* FAIL //p' "$REC/$_id.txt" | sed -n 1p); $_rel"; fi
}
