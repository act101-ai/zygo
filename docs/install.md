# Install zygo

zygo is one binary. It needs no administrator rights: the installers write to a directory in your home and never run `sudo`.

## macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/act101-ai/zygo/main/scripts/install.sh | sh
```

Pass flags after `sh -s --`:

- `--version <v>`: install that release (for example `v0.1.0`) instead of the latest.
- `--dry-run`: print the target, the two URLs, the install path and the code-navigation choice; download and write nothing.

Environment variables:

- `ZYGO_INSTALL_DIR`: install directory (default `$HOME/.local/bin`).
- `ZYGO_BASE_URL`: use a mirror of the release downloads (default `https://github.com/act101-ai/zygo/releases`).
- `ZYGO_TARGET`: force a target triple instead of detecting it.
- `ZYGO_ACT_LITE_INSTALLER`: URL of act-lite's installer (default `https://raw.githubusercontent.com/act101-ai/act-lite/main/install.sh`, or `install.ps1` on Windows).

The script does not edit a shell profile. If the install directory is not on `PATH`, it prints `add <dir> to PATH`.

## Windows

Tested on Windows 11 x64 (2026-11-20): the shell, the agent shims and Claude Code. OpenCode on Windows is not live-tested yet. Report what you find.

```powershell
irm https://raw.githubusercontent.com/act101-ai/zygo/main/scripts/install.ps1 | iex
```

zygo goes to `%LOCALAPPDATA%\zygo\bin\zygo.exe`. To pass parameters (`-Version v0.1.0`, `-DryRun`), save the script and run it: `pwsh install.ps1 -Version v0.1.0`. The same three environment variables apply. No registry value or `PATH` entry is changed.

## Code navigation

After zygo is installed, the installers choose the plugin that gives agents code navigation:

| Found on `PATH` | Result |
|---|---|
| act101's `act` (its `act --version` prints `act <version>`) | `act101:act101` is enabled and approved; `act101:act-lite` is disabled |
| `act-lite` | `act101:act-lite` is enabled; `act101:act101` is disabled |
| neither | act-lite's own installer runs (its defaults and `ACT_LITE_*` variables apply), then as for `act-lite` |

nektos/act, the GitHub Actions runner, is also named `act`. Its `act --version` prints `act version <version>`, so it does not count as act101's `act`. The choice is written with `zygo plugins enable` and `zygo plugins disable`. To change it later, run those commands yourself. If a step fails, the installer names the command and exits 1. The zygo binary stays installed.

On Windows, act-lite's installer adds its own directory to the user `PATH`.

## Platforms

| Platform | Release asset |
|---|---|
| macOS, Apple silicon | `zygo-aarch64-apple-darwin` |
| macOS, Intel | `zygo-x86_64-apple-darwin` |
| Linux, ARM64 (static) | `zygo-aarch64-unknown-linux-musl` |
| Linux, x86-64 (static) | `zygo-x86_64-unknown-linux-musl` |
| Windows, x86-64 | `zygo-x86_64-pc-windows-msvc.exe` |
| Windows, ARM64 | `zygo-aarch64-pc-windows-msvc.exe` |

## Checksums

Each release holds a `SHA256SUMS` file (`<hex>  <asset>` per line) and a `VERSION` file. The installers and `zygo update` download `SHA256SUMS` and the asset, compare the SHA-256 digest, and write nothing when they differ.

## Update

```sh
zygo update              # install the latest release if it is newer
zygo update --check      # print whether an update exists; install nothing
zygo update --version v0.1.0
```

`zygo update` downloads both files from the tagged release, verifies the checksum, then replaces the running binary. On macOS and Linux the replace is one atomic rename. On Windows a running `.exe` cannot be renamed over, so the binary is renamed to `zygo.exe.old`, the new file is renamed in, and `.old` is deleted when possible; if the second rename fails, `.old` is renamed back.
