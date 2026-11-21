#!/bin/sh
# Installs zygo from a release: no sudo, no profile edit.
# Usage: install.sh [--version <v>] [--dry-run]
# Environment: ZYGO_INSTALL_DIR (default $HOME/.local/bin), ZYGO_BASE_URL (mirror), ZYGO_TARGET (target triple),
# ZYGO_ACT_LITE_INSTALLER (URL of act-lite's install.sh).
# Code navigation: act101's `act` found -> plugin act101:act101; else `act-lite` found -> act101:act-lite;
# else act-lite is installed by its own installer and act101:act-lite is used.
REPO="act101-ai/zygo"
set -eu

# Everything runs inside main, so a download cut short defines a function and runs nothing.
main() {
    ver=""
    dry=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || { echo "install: --version needs a value" >&2; exit 2; }
                ver=$2
                shift 2
                ;;
            --dry-run)
                dry=1
                shift
                ;;
            *)
                echo "install: unknown argument $1" >&2
                exit 2
                ;;
        esac
    done

    if [ -n "$ver" ]; then
        if ! printf '%s' "$ver" | grep -Eq '^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'; then
            echo "install: bad version $ver" >&2
            exit 2
        fi
        ver=${ver#v}
    fi

    if [ -n "${ZYGO_TARGET:-}" ]; then
        target=$ZYGO_TARGET
    else
        os=$(uname -s)
        arch=$(uname -m)
        case "$os/$arch" in
            Darwin/arm64 | Darwin/aarch64) target=aarch64-apple-darwin ;;
            Darwin/x86_64) target=x86_64-apple-darwin ;;
            Linux/aarch64 | Linux/arm64) target=aarch64-unknown-linux-musl ;;
            Linux/x86_64) target=x86_64-unknown-linux-musl ;;
            *)
                echo "install: unsupported platform $os/$arch" >&2
                exit 1
                ;;
        esac
    fi

    name="zygo-$target"
    base=${ZYGO_BASE_URL:-https://github.com/$REPO/releases}
    if [ -n "$ver" ]; then
        url_dir="$base/download/v$ver"
        label=$ver
    else
        url_dir="$base/latest/download"
        label=latest
    fi
    dir=${ZYGO_INSTALL_DIR:-$HOME/.local/bin}
    dest="$dir/zygo"
    act_lite_installer=${ZYGO_ACT_LITE_INSTALLER:-https://raw.githubusercontent.com/act101-ai/act-lite/main/install.sh}

    # act101's act prints `act <version>`; nektos/act (GitHub Actions runner) prints `act version <version>`.
    nav=none
    if command -v act >/dev/null 2>&1 && act --version </dev/null 2>/dev/null | head -n 1 | grep -Eq '^act [0-9]'; then
        nav=act
    elif command -v act-lite >/dev/null 2>&1; then
        nav=act-lite
    fi

    if [ "$dry" -eq 1 ]; then
        echo "target: $target"
        echo "would download $url_dir/$name"
        echo "would download $url_dir/SHA256SUMS"
        echo "would verify checksum"
        echo "would install $dest"
        case $nav in
            act) echo "found act: would enable act101:act101 and disable act101:act-lite" ;;
            act-lite) echo "found act-lite: would enable act101:act-lite and disable act101:act101" ;;
            none)
                echo "would install act-lite with $act_lite_installer"
                echo "would enable act101:act-lite and disable act101:act101"
                ;;
        esac
        exit 0
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        sha() { sha256sum "$1" | awk '{print tolower($1)}'; }
    elif command -v shasum >/dev/null 2>&1; then
        sha() { shasum -a 256 "$1" | awk '{print tolower($1)}'; }
    else
        echo "install: no sha256 tool (need sha256sum or shasum)" >&2
        exit 1
    fi

    if command -v curl >/dev/null 2>&1; then
        get() { curl -fsSL -o "$2" "$1"; }
    elif command -v wget >/dev/null 2>&1; then
        get() { wget -qO "$2" "$1"; }
    else
        echo "install: need curl or wget" >&2
        exit 1
    fi

    mkdir -p "$dir"
    tmp="$dir/.zygo-install.$$"
    sums="$tmp.sums"
    alsh="$tmp.act-lite.sh"
    trap 'rm -f "$tmp" "$sums" "$alsh"' EXIT
    trap 'exit 1' INT TERM

    get "$url_dir/SHA256SUMS" "$sums" || { echo "install: cannot download $url_dir/SHA256SUMS" >&2; exit 1; }
    expected=$(awk -v n="$name" '{f=$2; sub(/^\*/,"",f); if (f==n) {print tolower($1); exit}}' "$sums")
    if [ -z "$expected" ]; then
        echo "install: no checksum for $name" >&2
        exit 1
    fi
    get "$url_dir/$name" "$tmp" || { echo "install: cannot download $url_dir/$name" >&2; exit 1; }
    actual=$(sha "$tmp")
    if [ "$actual" != "$expected" ]; then
        echo "install: checksum mismatch for $name" >&2
        exit 1
    fi

    if [ -f "$dest" ] && cmp -s "$tmp" "$dest"; then
        echo "zygo $label already installed"
    else
        chmod 755 "$tmp"
        mv -f "$tmp" "$dest"
        echo "installed zygo $label to $dest"
    fi

    # The plugin selection is written with the zygo just installed (spec §15.2); stdin is closed so a
    # `curl | sh` script is not read by a child.
    plugin() {
        "$dest" plugins "$@" </dev/null >/dev/null ||
            { echo "install: zygo plugins $* failed; run it again when the cause is fixed" >&2; exit 1; }
    }
    if [ "$nav" = none ]; then
        get "$act_lite_installer" "$alsh" || { echo "install: cannot download $act_lite_installer" >&2; exit 1; }
        sh "$alsh" </dev/null || { echo "install: act-lite install failed; see https://github.com/act101-ai/act-lite#install" >&2; exit 1; }
        nav=act-lite
    fi
    if [ "$nav" = act ]; then
        plugin enable act101:act101 --yes
        plugin disable act101:act-lite
        echo "found act: using plugin act101:act101"
    else
        plugin enable act101:act-lite --yes
        plugin disable act101:act101
        echo "using plugin act101:act-lite"
    fi

    case ":$PATH:" in
        *":$dir:"*) ;;
        *) echo "add $dir to PATH" ;;
    esac
}

main "$@"
