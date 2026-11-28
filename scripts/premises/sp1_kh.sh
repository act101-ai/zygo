# The known_hosts steps of live-sp1.sh (node FX-38), sourced; tested by sp1_watch_test.py with temporary folders.
# Needs PREMISES_DIR (sp1_watch.py) and python3. <priv> is the run's private folder: kh443 (the run's line),
# known_hosts.start (the saved copy) and kh.state (the run state: the resolved target, the saved copy's size and
# hash) are in it. kh-add writes kh.state before it changes the file, so the restore depends on that file only,
# not on a shell variable that a signal can stop the shell from setting.

# sp1_kh_add <known_hosts> <priv>: KHR = the kh-add result (added, present, failed: <reason>)
sp1_kh_add() {
  KHR=$(python3 -I "$PREMISES_DIR/sp1_watch.py" kh-add --file "$1" --line "$2/kh443" --save "$2/known_hosts.start" \
    --state "$2/kh.state")
}

# sp1_kh_restore <priv>: when <priv>/kh.state exists, kh-remove on the target it names; KH_R = the result, KH_MISS =
# lines missing against the saved copy, KH_MORE = lines added by other processes ("?" when not known). Returns 0
# when there is no run state or the file is restored (the run's line or fragment removed, or not there, and no line
# missing); kh.state is then deleted. Else 1, and kh.state stays.
sp1_kh_restore() {
  KH_R="" KH_MISS="" KH_MORE=""
  [ -f "$1/kh.state" ] || return 0
  _kho=$(python3 -I "$PREMISES_DIR/sp1_watch.py" kh-remove --state "$1/kh.state" --line "$1/kh443" \
    --start "$1/known_hosts.start" 2>/dev/null)
  IFS="$(printf '\t')" read -r KH_R KH_MISS KH_MORE <<EOF
$_kho
EOF
  KH_R=${KH_R:-not checked (kh-remove gave no result)} KH_MISS=${KH_MISS:-?} KH_MORE=${KH_MORE:-?}
  case "$KH_R" in
    removed | "removed a partial append "* | "the added line is not in the file")
      [ "$KH_MISS" = 0 ] && { rm -f "$1/kh.state"; return 0; } ;;
  esac
  return 1
}
