# ver_ge <have> <min> (node PRB-2): 0 when both are dotted numbers (digits and single dots) and
# have >= min, else 1. Fields compare as integers of any length (leading zeros dropped, a missing
# field is 0). A field longer than 32 digits fails closed (1) before any compare, so the digit loops
# stay short. Sourcing this file has no side effect; ver_ge sets only _vg_* variables.
ver_ge() {
  for _vg_x in "$1" "$2"; do
    case "$_vg_x" in '' | .* | *. | *..* | *[!0-9.]*) return 1 ;; esac
    case "$_vg_x" in
      *[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]*) return 1 ;;
    esac
  done
  _vg_a=$1. _vg_b=$2.
  while [ -n "$_vg_a$_vg_b" ]; do
    _vg_p=${_vg_a%%.*} _vg_q=${_vg_b%%.*}
    _vg_a=${_vg_a#*.} _vg_b=${_vg_b#*.}
    while [ "${_vg_p#0}" != "$_vg_p" ]; do _vg_p=${_vg_p#0}; done
    while [ "${_vg_q#0}" != "$_vg_q" ]; do _vg_q=${_vg_q#0}; done
    [ ${#_vg_p} -gt ${#_vg_q} ] && return 0
    [ ${#_vg_p} -lt ${#_vg_q} ] && return 1
    while [ -n "$_vg_p" ]; do
      _vg_c=${_vg_p%"${_vg_p#?}"} _vg_d=${_vg_q%"${_vg_q#?}"}
      [ "$_vg_c" -gt "$_vg_d" ] && return 0
      [ "$_vg_c" -lt "$_vg_d" ] && return 1
      _vg_p=${_vg_p#?} _vg_q=${_vg_q#?}
    done
  done
  return 0
}
