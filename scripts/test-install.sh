#!/bin/sh
# Tests for scripts/install.sh against a file:// release fixture. No network, no sudo.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/install.sh"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT INT TERM

TRIPLE=x86_64-unknown-linux-musl
CASE=""
OUT=""
RC=0

fail() {
    echo "test-install: FAIL $CASE: $1" >&2
    echo "$OUT" >&2
    exit 1
}

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

# mkfix <dir> <bytes>: a release directory holding the binary and a correct SHA256SUMS.
mkfix() {
    mkdir -p "$1"
    printf '%s' "$2" >"$1/zygo-$TRIPLE"
    printf '%s  zygo-%s\n' "$(digest "$1/zygo-$TRIPLE")" "$TRIPLE" >"$1/SHA256SUMS"
}

fixtures() {
    rm -rf "$T/rel"
    mkfix "$T/rel/latest/download" fixture-binary
    mkfix "$T/rel/download/v1.2.3" tagged-binary
}

mkshims() {
    mkdir -p "$T/shim"
    cat >"$T/shim/uname" <<'EOS'
#!/bin/sh
case "$1" in
    -s) echo "$FAKE_OS" ;;
    -m) echo "$FAKE_ARCH" ;;
    *) echo "$FAKE_OS" ;;
esac
EOS
    cat >"$T/shim/sudo" <<EOS
#!/bin/sh
echo called >"$T/sudo-called"
exit 1
EOS
    chmod +x "$T/shim/uname" "$T/shim/sudo"
}

# inst [args]: runs install.sh with the fixture base, a private HOME and the shims first on PATH.
inst() {
    RC=0
    OUT=$(
        HOME="$T/home"
        ZYGO_BASE_URL="file://$T/rel"
        PATH="$T/shim:$PATH"
        export HOME ZYGO_BASE_URL PATH
        if [ -z "${NO_DIR:-}" ]; then
            ZYGO_INSTALL_DIR="${DIR:-$T/bin}"
            export ZYGO_INSTALL_DIR
        fi
        sh "$SCRIPT" "$@" 2>&1
    ) || RC=$?
}

expect_ok() { [ "$RC" -eq 0 ] || fail "exit $RC, want 0"; }
expect_rc() { [ "$RC" -eq "$1" ] || fail "exit $RC, want $1"; }
expect_out() { case "$OUT" in *"$1"*) ;; *) fail "output lacks: $1" ;; esac; }
expect_no_temp() {
    for f in "${1:-$T/bin}"/.zygo-install.*; do
        [ ! -e "$f" ] || fail "left over $f"
    done
}

mkshims
fixtures

CASE=dry_run_maps_uname_pairs_to_unix_targets
for pair in "Darwin arm64 aarch64-apple-darwin" "Darwin x86_64 x86_64-apple-darwin" \
    "Linux aarch64 aarch64-unknown-linux-musl" "Linux arm64 aarch64-unknown-linux-musl" \
    "Linux x86_64 x86_64-unknown-linux-musl"; do
    set -- $pair
    FAKE_OS=$1 FAKE_ARCH=$2 inst --dry-run
    expect_ok
    expect_out "target: $3"
    expect_out "would download file://$T/rel/latest/download/zygo-$3"
    expect_out "would download file://$T/rel/latest/download/SHA256SUMS"
    expect_out "would verify checksum"
    expect_out "would install $T/bin/zygo"
    [ ! -e "$T/bin" ] || fail "dry run created $T/bin"
done

CASE=unsupported_platform_is_refused
FAKE_OS=FreeBSD FAKE_ARCH=amd64 inst --dry-run
[ "$RC" -ne 0 ] || fail "exit 0"
expect_out "unsupported platform"

CASE=install_writes_a_verified_executable
ZYGO_TARGET=$TRIPLE inst
expect_ok
cmp -s "$T/rel/latest/download/zygo-$TRIPLE" "$T/bin/zygo" || fail "content differs"
[ -x "$T/bin/zygo" ] || fail "not executable"
expect_out "installed zygo"
[ ! -e "$T/sudo-called" ] || fail "sudo was called"
expect_no_temp

CASE=second_run_is_idempotent
ino1=$(ls -i "$T/bin/zygo" | awk '{print $1}')
ZYGO_TARGET=$TRIPLE inst
expect_ok
expect_out "already installed"
ino2=$(ls -i "$T/bin/zygo" | awk '{print $1}')
[ "$ino1" = "$ino2" ] || fail "file was rewritten"
cmp -s "$T/rel/latest/download/zygo-$TRIPLE" "$T/bin/zygo" || fail "content differs"

CASE=checksum_mismatch_is_refused
rm -rf "$T/bin"
printf '%s  zygo-%s\n' "$(printf 'x' | { if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi; } | awk '{print $1}')" "$TRIPLE" >"$T/rel/latest/download/SHA256SUMS"
ZYGO_TARGET=$TRIPLE inst
[ "$RC" -ne 0 ] || fail "exit 0"
expect_out "checksum mismatch"
[ ! -e "$T/bin/zygo" ] || fail "binary was installed"
expect_no_temp

CASE=bad_checksum_keeps_the_installed_file
fixtures
rm -rf "$T/bin"
ZYGO_TARGET=$TRIPLE inst
expect_ok
mkfix "$T/rel/latest/download" changed-binary
printf '%s  zygo-%s\n' "0000000000000000000000000000000000000000000000000000000000000000" "$TRIPLE" >"$T/rel/latest/download/SHA256SUMS"
ZYGO_TARGET=$TRIPLE inst
[ "$RC" -ne 0 ] || fail "exit 0"
[ "$(cat "$T/bin/zygo")" = fixture-binary ] || fail "installed file changed"
expect_no_temp

CASE=version_flag_uses_the_tagged_path
fixtures
rm -rf "$T/bin"
ZYGO_TARGET=$TRIPLE inst --version v1.2.3
expect_ok
[ "$(cat "$T/bin/zygo")" = tagged-binary ] || fail "wrong bytes"

CASE=default_dir_is_home_local_bin
rm -rf "$T/home"
NO_DIR=1 ZYGO_TARGET=$TRIPLE inst
expect_ok
[ -f "$T/home/.local/bin/zygo" ] || fail "no file in HOME/.local/bin"
expect_out "add $T/home/.local/bin to PATH"
NO_DIR= # a prefix assignment before a function call can outlive the call in POSIX sh

CASE=missing_sha256_tool_exits_1
mkdir -p "$T/nohash"
for tool in uname curl awk cmp chmod mv rm mkdir cat dirname; do
    p=$(command -v "$tool") || fail "host lacks $tool"
    ln -sf "$p" "$T/nohash/$tool"
done
RC=0
OUT=$(
    HOME="$T/home" ZYGO_BASE_URL="file://$T/rel" ZYGO_INSTALL_DIR="$T/bin9" ZYGO_TARGET=$TRIPLE PATH="$T/nohash" \
        /bin/sh "$SCRIPT" 2>&1
) || RC=$?
expect_rc 1
expect_out "no sha256 tool"

CASE=install_dir_with_a_space
DIR="$T/a b" ZYGO_TARGET=$TRIPLE inst
expect_ok
cmp -s "$T/rel/latest/download/zygo-$TRIPLE" "$T/a b/zygo" || fail "content differs"
expect_no_temp "$T/a b"

CASE=bad_version_is_a_usage_error
rm -rf "$T/bin"
for v in abc 1.2; do
    ZYGO_TARGET=$TRIPLE inst --version "$v"
    expect_rc 2
    [ ! -e "$T/bin" ] || fail "created the install dir for $v"
done

CASE=missing_checksum_line_is_refused
rm -rf "$T/bin"
printf '%s  other-file\n' "$(digest "$T/rel/latest/download/zygo-$TRIPLE")" >"$T/rel/latest/download/SHA256SUMS"
ZYGO_TARGET=$TRIPLE inst
expect_rc 1
expect_out "no checksum for"
[ ! -e "$T/bin/zygo" ] || fail "binary was installed"

CASE=cut_download_runs_nothing
rm -rf "$T/bin"
cut=$(grep -n 'mkdir -p "$dir"' "$SCRIPT" | head -n 1 | cut -d: -f1)
head -n "$cut" "$SCRIPT" >"$T/cut.sh"
RC=0
OUT=$(
    HOME="$T/home" ZYGO_BASE_URL="file://$T/rel" ZYGO_INSTALL_DIR="$T/bin" ZYGO_TARGET=$TRIPLE PATH="$T/shim:$PATH"
    export HOME ZYGO_BASE_URL ZYGO_INSTALL_DIR ZYGO_TARGET PATH
    sh <"$T/cut.sh" 2>&1
) || RC=$?
[ "$RC" -ne 0 ] || fail "a cut script exited 0"
printf '%s' "$OUT" | grep -qi 'syntax error' || fail "a cut script did not stop at the parse"
[ ! -e "$T/bin" ] || fail "a cut script created the install dir"

echo "test-install: all passed"
