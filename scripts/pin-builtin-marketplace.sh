#!/bin/sh
# Pins the built-in act101 marketplace (spec §15.9, §15.7).
#   <tag>    clone the tag, write the trimmed fixture, the pin and the compiled-in digests (network).
#   --check  offline, no git: marketplace.toml, PIN.toml, the fixture tree and the approved data agree.
# The fixture tree has one writer, write_fixture (crates/zygo-plugins/src/builtin.rs), run through
# the pin_digests example.
set -eu
LC_ALL=C

usage() {
  echo "usage: sh scripts/pin-builtin-marketplace.sh [--root <dir>] [--source <url|path>] (--check | <tag>)" >&2
  exit 2
}
die() {
  echo "pin-builtin-marketplace: $1" >&2
  exit 1
}
# A tag or ref is safe to write into TOML and to pass to git when it matches this rule.
RULE='^[A-Za-z0-9._+-]+$'
safe_ref() {
  case $1 in
    '' | *[!A-Za-z0-9._+-]*) return 1 ;;
  esac
}

SELF=$(CDPATH= cd -P -- "$(dirname -- "$0")/.." && pwd)
ROOT=
SOURCE=
root_set=
source_set=
CHECK=
TAG=
while [ $# -gt 0 ]; do
  case $1 in
    --root)
      [ -z "$root_set" ] && [ $# -ge 2 ] && [ -n "$2" ] || usage
      ROOT=$2; root_set=1; shift 2 ;;
    --source)
      [ -z "$source_set" ] && [ $# -ge 2 ] && [ -n "$2" ] || usage
      SOURCE=$2; source_set=1; shift 2 ;;
    --check)
      [ -z "$CHECK" ] && [ -z "$TAG" ] || usage
      CHECK=1; shift ;;
    -*) usage ;;
    *)
      [ -z "$CHECK" ] && [ -z "$TAG" ] && [ -n "$1" ] || usage
      TAG=$1; shift ;;
  esac
done
[ -n "$CHECK" ] || [ -n "$TAG" ] || usage
[ -z "$CHECK" ] || [ -z "$source_set" ] || usage
[ -n "$ROOT" ] || ROOT=$SELF
if [ -n "$TAG" ] && ! safe_ref "$TAG"; then
  echo "pin-builtin-marketplace: tag must match $RULE" >&2
  exit 2
fi

MT=$ROOT/crates/zygo-config/data/marketplace.toml
AP=$ROOT/crates/zygo-plugins/data/act101-approved.toml
FX=$ROOT/fixtures/act101-marketplace
[ -f "$MT" ] || die "$MT is missing"

# The quoted value of `KEY = "..."` in file $2.
val() { sed -n "s/^$1 = \"\(.*\)\"\$/\1/p" "$2" 2>/dev/null || true; }
M_SOURCE=$(val source "$MT")
M_REF=$(val ref "$MT")
M_COMMIT=$(val commit "$MT")
DEFAULT_ON=$(sed -n 's/^default_on = \[\(.*\)\]$/\1/p' "$MT" | tr ',' '\n' | sed -n 's/^ *"\(.*\)" *$/\1/p')

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

# pin_digests with its stdout to file $1; its stderr passes through; its `error: ` line is the reason.
tool() {
  out=$1
  shift
  if "${CARGO:-cargo}" run -q --offline --manifest-path "$SELF/Cargo.toml" -p zygo-plugins --example pin_digests -- "$@" \
    >"$out" 2>"$T/err"; then
    cat "$T/err" >&2
    return 0
  fi
  cat "$T/err" >&2
  reason=$(sed -n 's/^error: //p' "$T/err" | head -n 1)
  die "${reason:-pin_digests failed}"
}

if [ -n "$CHECK" ]; then
  [ -n "$M_REF" ] || die "marketplace.toml has no ref"
  safe_ref "$M_REF" || die "marketplace.toml ref must match $RULE"
  printf '%s\n' "$M_COMMIT" | grep -Eqx '[0-9a-f]{40}' || die "marketplace.toml has no 40-hex commit"
  [ "$(val ref "$FX/PIN.toml")" = "$M_REF" ] || die "PIN.toml ref differs from marketplace.toml"
  [ "$(val commit "$FX/PIN.toml")" = "$M_COMMIT" ] || die "PIN.toml commit differs from marketplace.toml"
  # shellcheck disable=SC2086 # DEFAULT_ON is a list of plugin names
  tool "$T/approved" render "$FX/tree" "$M_REF" "$M_COMMIT" $DEFAULT_ON
  cmp -s "$T/approved" "$AP" || die "approved data differs from the fixture"
  exit 0
fi

if [ -n "$SOURCE" ]; then
  URL=$SOURCE
else
  case $M_SOURCE in
    github:*/*) URL=https://github.com/${M_SOURCE#github:} ;;
    *) die "source is not github:<owner>/<repo>" ;;
  esac
fi
git -c advice.detachedHead=false clone --quiet --branch "$TAG" -- "$URL" "$T/m" || die "clone of $TAG failed"
COMMIT=$(git -C "$T/m" rev-parse --verify --quiet "refs/tags/$TAG^{commit}") || die "$TAG is not a tag"
# shellcheck disable=SC2086
tool /dev/null fixture "$T/m" "$T/tree" $DEFAULT_ON
# shellcheck disable=SC2086
tool "$T/approved" render "$T/tree" "$TAG" "$COMMIT" $DEFAULT_ON

rm -rf "$FX"
mkdir -p "$FX"
mv "$T/tree" "$FX/tree"
printf '# Written by scripts/pin-builtin-marketplace.sh. Do not edit.\nref = "%s"\ncommit = "%s"\n' "$TAG" "$COMMIT" >"$FX/PIN.toml"
awk -v r="$TAG" -v c="$COMMIT" '
  /^ref = / || /^commit = / { next }
  { print }
  /^source = / { print "ref = \"" r "\""; print "commit = \"" c "\"" }
' "$MT" >"$MT.tmp"
mv "$MT.tmp" "$MT"
mv "$T/approved" "$AP"
echo "pinned act101 at $TAG $COMMIT"
echo "check: fixtures/act-lite/languages.json act_lite must equal the pinned act-lite version (spec 10.5)"
echo "run: sh scripts/pin-builtin-marketplace.sh --check"
