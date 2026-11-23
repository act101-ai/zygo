# zygo

zygo is one native binary that wraps Claude Code and OpenCode: it routes each agent role to a chosen model through a local proxy, saves tokens, redacts secrets and profanity from requests, tracks work units, and delivers plugins from approved marketplaces, with act-lite built in.

Version 0.1.0.

---

## Agentic Coding like you've never experienced before

<img width="1072" height="93" alt="image" src="https://github.com/user-attachments/assets/6365701c-305f-418b-a85d-6f00fb35b464" />

---

## Install

macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/act101-ai/zygo/main/scripts/install.sh | sh
```

Windows (PowerShell; see [docs/install.md](docs/install.md#windows)):

```powershell
irm https://raw.githubusercontent.com/act101-ai/zygo/main/scripts/install.ps1 | iex
```

No administrator rights are needed. Options, environment variables and `zygo update`: [docs/install.md](docs/install.md).

Passthrough (`zygo claude --passthrough`) runs Claude Code on your claude.ai login, in local mode only. The vendor's consumer terms restrict routing subscription credentials through third-party software; your own account carries that risk. See [docs/passthrough.md](docs/passthrough.md).

Build: `cargo build --release`

License: MIT
