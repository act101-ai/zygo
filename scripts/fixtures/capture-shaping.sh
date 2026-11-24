#!/bin/sh
# Captures one shaping fixture case (O§8.1 "Capture"):
#   scripts/fixtures/capture-shaping.sh <shaper> <case> -- <command...>
# Runs the command in the current directory with stdin from /dev/null and writes stdout, stderr,
# an empty must_keep and meta.json to ${ZYGO_FIXTURES_DIR:-<repo>/fixtures/shaping}/<shaper>/<case>/.
# Scrubs, in this order: the current directory -> /repo, $HOME -> /home/user, the user name -> user,
# the host name -> host. It never writes view. It refuses an existing case folder (exit 1) and
# bad arguments (exit 2); it exits 0 whatever the command's exit, which goes to meta.exit.
# Pass a pipeline as: sh -c '...'. Edit event, agent, tool and source by hand when wrong.
set -u

usage() { echo "usage: capture-shaping.sh <shaper> <case> -- <command...>" >&2; exit 2; }
fail() { echo "capture-shaping: $*" >&2; exit 1; }

[ $# -ge 4 ] && [ "$3" = "--" ] || usage
shaper=$1 name=$2
for n in "$shaper" "$name"; do
    # Explicit letters: a [a-z] range can match capitals under some locales.
    case $n in '' | *[!abcdefghijklmnopqrstuvwxyz0123456789-]*) usage ;; esac
done
shift 3

repo=$(cd "$(dirname "$0")/../.." && pwd -P) || exit 1
out=${ZYGO_FIXTURES_DIR:-$repo/fixtures/shaping}/$shaper/$name
[ -e "$out" ] && fail "$out exists"
mkdir -p "$out" || fail "cannot create $out"

"$@" </dev/null >"$out/stdout" 2>"$out/stderr"
code=$?
: >"$out/must_keep"

tool=$("$1" --version </dev/null 2>/dev/null | head -n 1)
[ -n "$tool" ] || tool=$(basename "$1")
command=$*
CAP_COMMAND=$command CAP_EXIT=$code CAP_TOOL=$tool CAP_DATE=$(date -u +%Y-%m-%d) \
CAP_SOURCE="$command, run by capture-shaping.sh in $PWD" \
perl -MJSON::PP -e 'print JSON::PP->new->canonical->pretty->encode({
    command => $ENV{CAP_COMMAND}, exit => 0 + $ENV{CAP_EXIT}, event => "PostToolUse",
    agent => "claude-code", tool => $ENV{CAP_TOOL}, captured_on => $ENV{CAP_DATE},
    source => $ENV{CAP_SOURCE} })' >"$out/meta.json" || fail "cannot write meta.json"

# A user or host name shorter than 4 characters, or localhost, is too common to replace.
name_ok() {
    if [ ${#2} -lt 4 ] || [ "$2" = localhost ]; then
        echo "capture-shaping: $1 '$2' not scrubbed (shorter than 4 characters or localhost)" >&2
        return 1
    fi
}
S5=${USER:-$(id -un)}
S6=$(hostname)
S7=${S6%%.*}
name_ok "user name" "$S5" || S5=
name_ok "host name" "$S6" || S6=
name_ok "host name" "$S7" || S7=

S1=$(pwd -P) S2=$PWD S3=$(cd "$HOME" && pwd -P) S4=$HOME S5=$S5 S6=$S6 S7=$S7 \
perl -0777 -pi -e 'for my $r (["S1","/repo"],["S2","/repo"],["S3","/home/user"],["S4","/home/user"],["S5","user"],["S6","host"],["S7","host"]) {
    my $v = $ENV{$r->[0]}; next unless defined $v && length $v; s/\Q$v\E/$r->[1]/g }' \
    "$out/stdout" "$out/stderr" "$out/meta.json" || fail "scrub failed"

echo "$out"
