# Agent conformance (node N39)

One row per v1 agent, one cell per capability A-1 to A-21 (spec `docs/specs/2026-11-18-zygo-design.md` §8.1). Cell form: `full`, `partial (what is missing)` or `none (why)`, then the documentation URL of the feature. Checked by `scripts/check-conformance.sh`. The workspace capabilities WA-1 and WA-2 (`docs/specs/2026-11-20-workspaces-design.md` AU28) have their own table in the same cell form.

Claude Code: 2.1.328
OpenCode: 1.18.16

| Agent | A-1 | A-2 | A-3 | A-4 | A-5 | A-6 | A-7 | A-8 | A-9 | A-10 | A-11 | A-12 | A-13 | A-14 | A-15 | A-16 | A-17 | A-18 | A-19 | A-20 | A-21 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Claude Code | partial (PowerShell needs & before a quoted path) https://code.claude.com/docs/en/llm-gateway | full https://code.claude.com/docs/en/env-vars | full https://code.claude.com/docs/en/model-config | full https://code.claude.com/docs/en/sub-agents | full https://code.claude.com/docs/en/cli-reference | full https://code.claude.com/docs/en/mcp | full https://code.claude.com/docs/en/hooks | full https://code.claude.com/docs/en/hooks | partial (PowerShell needs & before a quoted path) https://code.claude.com/docs/en/statusline | full https://code.claude.com/docs/en/env-vars | full https://code.claude.com/docs/en/plugins | full https://code.claude.com/docs/en/settings | full https://code.claude.com/docs/en/cli-reference | full https://code.claude.com/docs/en/sub-agents | full https://code.claude.com/docs/en/hooks | full https://code.claude.com/docs/en/context-window | full https://code.claude.com/docs/en/mcp | full https://code.claude.com/docs/en/hooks | full https://code.claude.com/docs/en/hooks | full https://code.claude.com/docs/en/sub-agents | full https://code.claude.com/docs/en/cli-reference |
| OpenCode | full https://opencode.ai/docs/providers/ | full https://opencode.ai/docs/config/ | full https://opencode.ai/docs/models/ | full https://opencode.ai/docs/agents/ | full https://opencode.ai/docs/config/ | full https://opencode.ai/docs/mcp-servers/ | full https://opencode.ai/docs/plugins/ | full https://opencode.ai/docs/plugins/ | full https://opencode.ai/docs/cli/ | full https://opencode.ai/docs/cli/ | full https://opencode.ai/docs/skills/ | full https://opencode.ai/docs/models/ | full https://opencode.ai/docs/cli/ | full https://opencode.ai/docs/agents/ | full https://opencode.ai/docs/permissions/ | partial (no proxy signal and the window is a launch-time setting) https://opencode.ai/docs/config/ | none (the SSE rewrite is not armed for OpenCode in v1) https://opencode.ai/docs/mcp-servers/ | full https://opencode.ai/docs/plugins/ | partial (no subagent-stop event and no agent id verified; claims and scope guards cannot run) https://opencode.ai/docs/plugins/ | full https://opencode.ai/docs/permissions/ | full https://opencode.ai/docs/cli/ |

## Workspace capabilities

WA-1 Resume: pre-assign or capture the agent session id; resume by id. WA-2 Turn signals: turn start and end, subagent start and end, dispatch block (workspaces spec AU28, W§9.1). Evidence ids PW1-PW16 are in `docs/premises.md`; captures are under `fixtures/claude-code/workspace/` and `fixtures/opencode/workspace/`.

| Agent | WA-1 | WA-2 |
|---|---|---|
| Claude Code | partial (a resume with another CLAUDE_CONFIG_DIR finds no conversation, PW16 16.2; the AU61 fallback reuses the saved one) https://code.claude.com/docs/en/cli-reference | partial (a turn ended by Esc sends no Stop or StopFailure, and internal subagents send SubagentStop without SubagentStart; AU26 fallback) https://code.claude.com/docs/en/hooks |
| OpenCode | partial (a TUI switch to an existing session gives the plugin no session.updated event, so up resumes the earlier top session, PW4 4.9 and AU27 known limit 1; the resume is verified with opencode run -s, PW4 4.2, and in the TUI with --session=<id>, PW4 4.8; evidence on OpenCode 1.19.6, W§17 PW4 note of 2026-11-23; the pin 1.18.16 is a minimum) https://opencode.ai/docs/cli/ | partial (no subagent start or stop event; a typed TUI prompt gives a session.status busy event of the main session before its first request, PW4 4.7, but that busy status is not in a fixture; evidence on OpenCode 1.19.6, W§17 PW4 note of 2026-11-23; the pin 1.18.16 is a minimum) https://opencode.ai/docs/plugins/ |

- Claude Code WA-1: `--session-id <uuid>` pre-assign and `--resume <id>` keep one id (PW1, `fixtures/claude-code/workspace/pw1.txt`; zygo passes the one-token forms `--session-id=<uuid>` and `--resume=<id>`, the `=` form that the pinned help shows for options of the same parser); `SessionStart` gives `session_id` and fires again after `/clear` (PW2); resume after an upgrade (PW3); another `CLAUDE_CONFIG_DIR` does not find the conversation (PW16 16.2), so AU61 saves and reuses the config folder. WA-2: `UserPromptSubmit`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, and the `PreToolUse` deny on `Agent` (PW5, `fixtures/claude-code/workspace/PreToolUse-Agent-denied.json`); partial by PW7 7.2, 7.3 and 7.5 (`fixtures/claude-code/workspace/pw7.txt`).
- OpenCode, both cells: the evidence is from OpenCode 1.19.6 (PW4); the pin 1.18.16 above is a minimum, not the version of this evidence; E2 re-checked PW4 on OpenCode 1.19.6 (notes 4.7-4.9 in W§17 PW4, 2026-11-23). WA-1: the plugin sees the `sessionID` on its first `session.created` event, and `opencode run -s <id>` resumes it (PW4 4.1, 4.2, `fixtures/opencode/workspace/pw4.txt`); a TUI started with `--session=<id>` resumes that session and the plugin's first session id is `<id>` (PW4 4.8); a TUI switch to an existing session (`/sessions`, Enter) gives the plugin no event with that session id (PW4 4.9), so AU27 known limit 1 stays. WA-2: the first `session.status` busy of the main session after an idle starts a turn, and a busy of another top session makes it the main session (status types `idle`, `retry`, `busy` in the OpenCode 1.19.6 binary; not in a fixture); the first `session.idle` or `session.status` idle after a turn ends it (PW4 4.3, `fixtures/opencode/workspace/plugin-event-types.txt`); `tool.execute.before` sees `task` before the subagent runs, and a throw there stops it (PW4 4.4, 4.5 note, `fixtures/opencode/workspace/tool.execute.before-task.json`); no subagent event (A-19).

## Evidence

Evidence ids (P1-P6b, Q1-Q7) are in `docs/premises.md`; captures are under `fixtures/claude-code/` and `fixtures/opencode/`.

Claude Code (all full except A-1 and A-9, partial):
- A-1 Q7 `apiKeyHelper` (7 mock-accepted requests). A-2 Q7 `ANTHROPIC_CUSTOM_HEADERS` gives `x-zygo-session` and `x-zygo-repo` in requests; Q5. A-3 P2, Q3 (`modelPicker`, `availableModels`). A-4 Q7 aliases, `CLAUDE_CODE_SUBAGENT_MODEL`, `--agents`; P3. A-5 and A-6 `--settings`, `--agents`, `--mcp-config`, `--plugin-dir` in `help.txt`; Q7. A-7 Q4 `updatedToolOutput` for Bash, Read, Grep, Glob, WebFetch. A-8 `hooks/SessionStart-main.json`, `hooks/SessionStart-sub.json`.
- A-9 `statusLine` in `settings-schema.json`. A-10 Q7 `env` row. A-11 Q7 `--plugin-dir` loads plugin `zygo-probe`. A-12 Q7 `model`, `availableModels`. A-13 `--version` in `help.txt`. A-14 Q7 role prompt text in subagent system. A-15 `hooks/PreToolUse-Edit-main.json` and the Write and NotebookEdit files. A-16 P1. A-17 P1 `tool_use` case; tool name `mcp__<server>__<tool>`. A-18 `hooks/PreCompact-main.json`. A-19 P3, Q1. A-20 Q7 `maxTurns=2`; `disallowedTools` as an `--agents` key is observed live (N38): `fixtures/claude-code/live/a20-tools.json` holds the `zygo-review` entry with `disallowedTools` and that subagent's tool list without `Edit`, `Write`, `NotebookEdit`. A-21 Q7 and the spec A-21 note: Q7 `failed` means two managed keys cannot be observed without administrator rights; the stub launch test and the schema premise test are both buildable.

OpenCode:
- A-1 P4 `options.baseURL`, `apiKey`, `headers`. A-2 P4 inline header value in captured requests. A-3 P4 model entries and `limit.context`. A-4 P5 `agent-list.txt`. A-5 P4, P5, Q2 (`OPENCODE_CONFIG_CONTENT`, `OPENCODE_CONFIG_DIR`; docs do not name them). A-6 P5 `mcp-list.txt`, tool `zygo_probe_tool`. A-7 Q2 `tool.execute.after`, assigning `output.output` (docs do not name the hook). A-8 Q2 `experimental.chat.system.transform` (docs do not name the hook). A-9 the spec A-9 fallback `zygo status`; no OpenCode status-line premise. A-10 launcher-built environment; P4 runs under `env -i`.
- A-11 P5 `debug-skill.txt`, plugin `zygo.js` loads. A-12 P4 inline `model`, `agent.build.model`. A-13 `--version` in `fixtures/opencode/help.txt`. A-14 P5 subagent `prompt` named in the request. A-15 Q2 `permission.edit: deny` on `build` removes edit and write tools. A-16 Q2 compaction on usage over `limit.context`; missing: no proxy signal, window fixed at launch. A-17 Q2 `zygo_compact` is visible as `<server>_<tool>`; spec §10.2.5. A-18 Q2 `experimental.session.compacting`, text pushed to `output.context` appears in the compaction request (docs do not name the hook); differs from the spec v1 target (none). A-20 Q2 `permission.edit: deny` and `steps`; differs from the spec v1 target (partial). A-21 Q7 every OpenCode key is in `config-schema.json`.
- A-19 partial: the plugin hooks seen are `tool.execute.before` and `tool.execute.after` only (`fixtures/opencode/plugin-hooks/`); §13.3 needs a `SubagentStop`-like event and an agent id, and §13.2 says OpenCode carries no agent id. Matches the spec note of 2026-11-18 (N39 planning) on §8.1.

A-21 cross-platform tests (node N34), both agents: `crates/zygo-launch/tests/stub_launch.rs` launches a stub agent (`tests/stub_agent/stub.rs`) through `Launcher::run` on the Linux, macOS and Windows CI jobs and checks the environment rows (A-1, A-2, A-4, A-10, A-13), the flags, the generated settings or inline config (A-3, A-5, A-9, A-12 rows, A-15, A-18 and A-19 registrations, A-20 tool restriction), the MCP entry (A-6), the plugin directory (A-11), and the session token: it is only in the `token` file, and in Claude Code passthrough also in the 0600 `settings.json` and `ANTHROPIC_CUSTOM_HEADERS`; never in an argument, stderr, the session log or another file. `crates/zygo-launch/tests/premise.rs` finds every flag, setting key, env row and agent key zygo writes in the pinned `--help`, schema, binary-string and capture files (Claude Code 2.1.328, OpenCode 1.18.16) and the `<plugin>:<skill>` form of §20 #40 in `fixtures/claude-code/q7/a1-init.json`.

Windows shell for the generated commands (Claude Code A-1 `apiKeyHelper`, A-9 `statusLine`): Claude Code runs a hook command in Git Bash on Windows, or in PowerShell when Git Bash is not installed (https://code.claude.com/docs/en/hooks); its pages name no shell for `apiKeyHelper` and `statusLine`. The CI test runs every command in Git Bash. zygo writes the path in double quotes on Windows; PowerShell needs `& "<path>" args`, and the double-quoted form is not run in PowerShell.

Documentation caveat (OpenCode): the live OpenCode docs describe a newer plugin API than the pinned 1.18.16 and do not name `experimental.chat.system.transform`, `experimental.session.compacting`, `OPENCODE_CONFIG_CONTENT` or `OPENCODE_CONFIG_DIR`. The cells for A-5, A-7, A-8 and A-18 rest on the fixtures and the binary strings (`fixtures/opencode/binary-names.txt`); the URL is the page of the feature.

## Capabilities


- A-1: Transport
- A-2: Attribution
- A-3: Model list
- A-4: Subagent roles
- A-5: Per-session config
- A-6: Local services
- A-7: Result trimming
- A-8: Session context
- A-9: Status
- A-10: Secrets hygiene
- A-11: Skills and plugins
- A-12: Model profile
- A-13: Version
- A-14: Role prompts
- A-15: Lead guard
- A-16: Compaction window
- A-17: Model-triggered compaction
- A-18: Compaction instructions
- A-19: Work units and guards
- A-20: Role tool restriction
- A-21: Tests
- WA-1: Resume
- WA-2: Turn signals
