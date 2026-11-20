#!/bin/sh
# Installs zygo from a release: no sudo, no profile edit.
# Usage: install.sh [--version <v>] [--dry-run]
# Environment: ZYGO_INSTALL_DIR (default $HOME/.local/bin), ZYGO_BASE_URL (mirror), ZYGO_TARGET (target triple).
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

    if [ "$dry" -eq 1 ]; then
        echo "target: $target"
        echo "would download $url_dir/$name"
        echo "would download $url_dir/SHA256SUMS"
        echo "would verify checksum"
        echo "would install $dest"
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
    trap 'rm -f "$tmp" "$sums"' EXIT
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

    case ":$PATH:" in
        *":$dir:"*) ;;
        *) echo "add $dir to PATH" ;;
    esac
}

main "$@"
