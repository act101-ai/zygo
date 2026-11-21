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

# mkfix <dir> <marker>: a release directory holding a stub zygo and a correct SHA256SUMS. The stub logs its
# arguments to $ZYGO_STUB_LOG and fails when ZYGO_STUB_FAIL is set; line 2 is the marker comment.
mkfix() {
    mkdir -p "$1"
    printf '#!/bin/sh\n# %s\necho "zygo $*" >>"$ZYGO_STUB_LOG"\n[ -z "${ZYGO_STUB_FAIL:-}" ]\n' "$2" >"$1/zygo-$TRIPLE"
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
    # The host tools install.sh uses; no host act or act-lite is reachable.
    mkdir -p "$T/tools" "$T/acts" "$T/al"
    for tool in sh grep head awk curl mkdir rm cmp chmod mv cat sha256sum shasum; do
        p=$(command -v "$tool") && ln -sf "$p" "$T/tools/$tool"
    done
    cat >"$T/al/install.sh" <<'EOS'
echo "act-lite-installer" >>"$ZYGO_STUB_LOG"
[ -z "${AL_FAIL:-}" ]
EOS
}

# acts <names>: puts stubs of act101's act, act-lite or nektos/act (named nektos) on PATH, and nothing else.
acts() {
    rm -f "$T/acts/"*
    for a in "$@"; do
        case $a in
            act) printf '#!/bin/sh\necho "act 2.9.2"\n' >"$T/acts/act" ;;
            nektos) printf '#!/bin/sh\necho "act version 0.2.61"\n' >"$T/acts/act" ;;
            act-lite) printf '#!/bin/sh\necho "act-lite 1.2.7"\n' >"$T/acts/act-lite" ;;
        esac
    done
    chmod +x "$T/acts/"* 2>/dev/null || true
}

marker() { sed -n 2p "$1"; }
log() { cat "$T/log" 2>/dev/null || true; }

# inst [args]: runs install.sh with the fixture base, a private HOME and the shims first on PATH.
inst() {
    RC=0
    rm -f "$T/log"
    OUT=$(
        HOME="$T/home"
        ZYGO_BASE_URL="file://$T/rel"
        ZYGO_ACT_LITE_INSTALLER="file://$T/al/install.sh"
        ZYGO_STUB_LOG="$T/log"
        PATH="$T/shim:$T/acts:$T/tools"
        export HOME ZYGO_BASE_URL ZYGO_ACT_LITE_INSTALLER ZYGO_STUB_LOG PATH
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
[ "$(marker "$T/bin/zygo")" = "# fixture-binary" ] || fail "installed file changed"
expect_no_temp

CASE=version_flag_uses_the_tagged_path
fixtures
rm -rf "$T/bin"
ZYGO_TARGET=$TRIPLE inst --version v1.2.3
expect_ok
[ "$(marker "$T/bin/zygo")" = "# tagged-binary" ] || fail "wrong bytes"

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

CASE=no_navigation_tool_installs_act_lite
DIR= # see NO_DIR above
fixtures
rm -rf "$T/bin"
acts
ZYGO_TARGET=$TRIPLE inst
expect_ok
expect_out "using plugin act101:act-lite"
[ "$(log)" = "act-lite-installer
zygo plugins enable act101:act-lite --yes
zygo plugins disable act101:act101" ] || fail "log: $(log)"
expect_no_temp

CASE=act_lite_found_is_used_and_not_reinstalled
acts act-lite
ZYGO_TARGET=$TRIPLE inst
expect_ok
expect_out "using plugin act101:act-lite"
[ "$(log)" = "zygo plugins enable act101:act-lite --yes
zygo plugins disable act101:act101" ] || fail "log: $(log)"

CASE=act_found_selects_the_act101_plugin
acts act act-lite
ZYGO_TARGET=$TRIPLE inst
expect_ok
expect_out "found act: using plugin act101:act101"
[ "$(log)" = "zygo plugins enable act101:act101 --yes
zygo plugins disable act101:act-lite" ] || fail "log: $(log)"

CASE=nektos_act_is_not_act101
acts nektos
ZYGO_TARGET=$TRIPLE inst
expect_ok
expect_out "using plugin act101:act-lite"
case "$(log)" in act-lite-installer*) ;; *) fail "act-lite not installed: $(log)" ;; esac

CASE=dry_run_reports_the_navigation_choice
rm -rf "$T/bin"
for pair in "act:found act: would enable act101:act101 and disable act101:act-lite" \
    "act-lite:found act-lite: would enable act101:act-lite and disable act101:act101" \
    ":would install act-lite with file://$T/al/install.sh"; do
    acts ${pair%%:*}
    ZYGO_TARGET=$TRIPLE inst --dry-run
    expect_ok
    expect_out "${pair#*:}"
    [ -z "$(log)" ] || fail "dry run ran: $(log)"
done
[ ! -e "$T/bin" ] || fail "dry run created $T/bin"

CASE=plugin_failure_exits_1
acts act
ZYGO_STUB_FAIL=1 ZYGO_TARGET=$TRIPLE inst
expect_rc 1
expect_out "zygo plugins enable act101:act101 --yes failed"
[ -x "$T/bin/zygo" ] || fail "zygo was not installed"
ZYGO_STUB_FAIL=

CASE=act_lite_install_failure_exits_1
acts
AL_FAIL=1 ZYGO_TARGET=$TRIPLE inst
expect_rc 1
expect_out "act-lite install failed"
case "$(log)" in *plugins*) fail "plugins ran after a failed act-lite install" ;; esac
AL_FAIL=
expect_no_temp

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
