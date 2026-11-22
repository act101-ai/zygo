---
name: zygo-cli
description: How to run the zygo commands that report and change this session. Use it when the user asks about the context window, compaction, savings, work items, marketplaces or plugins.
---

# zygo CLI

Never state or guess the context window. Run `zygo status` and report what it prints.

Run these in the shell. Each prints a short result; report it as it is.

- `zygo status` shows the session: agent, context window, compaction, guards and plugins. Run it before you say anything about limits.
- `zygo compact <tokens|on|off>` sets the compaction window for this session. Run it only when the user asks.
- `zygo usage` shows this session's savings. Add `--session` for the current session only, or `--since <time>` for a period.
- `zygo statusline [compact|verbose]` shows the status line style, or sets it for this session: `compact` is one row, `verbose` is two rows. Add `--save` to also write it to user config. Run it only when the user asks.
- `zygo work [<id>|clear]` shows, sets or clears the current work item of the repository.
- `zygo marketplace [list|add|update|remove ...]` manages plugin marketplaces. `add` and `update` can ask the user to approve plugin code; leave that to the user.
- `zygo plugins [list|enable|disable|approve <marketplace>:<plugin>]` lists plugins or changes which are selected. `zygo plugins approve` asks the user to approve code. Never run it with `--yes` for the user. Changes apply at the next launch.

If a command fails, show its message to the user.
