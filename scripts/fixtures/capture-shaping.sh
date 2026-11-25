#!/bin/sh
# Captures one shaping fixture case (O§8.1 "Capture"):
#   ZYGO_CAPTURE_TOOL='<program> <version>' scripts/fixtures/capture-shaping.sh <shaper> <case> -- <command...>
# ZYGO_CAPTURE_TOOL is meta.tool, e.g. "cargo 1.83.0"; the script never runs the program a second time.
# Runs the command in the current directory with stdin from /dev/null and writes stdout, stderr,
# an empty must_keep and meta.json into a temp dir, scrubs them there, and then moves the folder to
# ${ZYGO_FIXTURES_DIR:-<repo>/fixtures/shaping}/<shaper>/<case>/ (a relative dir is taken from the
# current directory). On any failure that folder does not exist.
# Scrubs, in this order: the current directory -> /repo, $HOME -> /home/user (each only at a path
# boundary), the user name -> user, the host name -> host (each only as a whole word). It never
# writes view. It refuses an existing case folder (exit 1) and bad arguments or no
# ZYGO_CAPTURE_TOOL (exit 2); it exits 0 whatever the command's exit, which goes to meta.exit.
# meta.command is the <s> of `sh|bash|zsh -c <s>`, else the words shell-quoted.
# Pass a pipeline as: sh -c '...'. Edit event, agent and source by hand when wrong.
set -u

usage() { echo "usage: ZYGO_CAPTURE_TOOL='<program> <version>' capture-shaping.sh <shaper> <case> -- <command...>" >&2; exit 2; }
fail() { echo "capture-shaping: $*" >&2; exit 1; }

[ $# -ge 4 ] && [ "$3" = "--" ] || usage
shaper=$1 name=$2
for n in "$shaper" "$name"; do
    # Explicit letters: a [a-z] range can match capitals under some locales.
    case $n in '' | *[!abcdefghijklmnopqrstuvwxyz0123456789-]*) usage ;; esac
done
shift 3
tool=${ZYGO_CAPTURE_TOOL:-}
[ -n "$tool" ] || usage
[ -n "${HOME:-}" ] || fail "HOME is not set"
home_real=$(cd "$HOME" && pwd -P) || fail "cannot enter HOME $HOME"

repo=$(cd "$(dirname "$0")/../.." && pwd -P) || exit 1
base=${ZYGO_FIXTURES_DIR:-$repo/fixtures/shaping}
case $base in /*) ;; *) base=$PWD/$base ;; esac
out=$base/$shaper/$name
[ -e "$out" ] && fail "$out exists"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/capture-shaping.XXXXXX") || fail "cannot create a temp dir"
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
w=$tmp/case
mkdir "$w" || fail "cannot create $w"

"$@" </dev/null >"$w/stdout" 2>"$w/stderr"
code=$?
: >"$w/must_keep" || fail "cannot write must_keep"

# chr(39) is a single quote: the perl text has none, so it can sit in single quotes here.
CAP_EXIT=$code CAP_TOOL=$tool CAP_DATE=$(date -u +%Y-%m-%d) CAP_DIR=$PWD \
perl -MJSON::PP -e 'my $q = chr 39; my $c;
    if (@ARGV == 3 && $ARGV[1] eq "-c" && $ARGV[0] =~ /\A(?:sh|bash|zsh)\z/) { $c = $ARGV[2] }
    else { $c = join " ", map { /\A[A-Za-z0-9_.\/=:\@%+-]+\z/ ? $_ : do { (my $s = $_) =~ s/$q/$q\\$q$q/g; "$q$s$q" } } @ARGV }
    print JSON::PP->new->canonical->pretty->encode({
    command => $c, exit => 0 + $ENV{CAP_EXIT}, event => "PostToolUse",
    agent => "claude-code", tool => $ENV{CAP_TOOL}, captured_on => $ENV{CAP_DATE},
    source => "$c, run by capture-shaping.sh in $ENV{CAP_DIR}" })' -- "$@" >"$w/meta.json" || fail "cannot write meta.json"

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

# Paths (S1-S4) match only before "/", the end, or a byte not in [A-Za-z0-9._-]; "/" is skipped.
# Names (S5-S7) match only with no [A-Za-z0-9_] on either side.
S1=$(pwd -P) S2=$PWD S3=$home_real S4=$HOME S5=$S5 S6=$S6 S7=$S7 \
perl -0777 -pi -e 'for my $r (["S1","/repo",1],["S2","/repo",1],["S3","/home/user",1],["S4","/home/user",1],["S5","user",0],["S6","host",0],["S7","host",0]) {
    my $v = $ENV{$r->[0]}; next unless defined $v && length $v && $v ne "/";
    if ($r->[2]) { s/\Q$v\E(?=\/|\z|[^A-Za-z0-9._-])/$r->[1]/g }
    else { s/(?<![A-Za-z0-9_])\Q$v\E(?![A-Za-z0-9_])/$r->[1]/g } }' \
    "$w/stdout" "$w/stderr" "$w/meta.json" || fail "scrub failed"

mkdir -p "$base/$shaper" || fail "cannot create $base/$shaper"
mv "$w" "$out" || fail "cannot move the case to $out"
echo "$out"
