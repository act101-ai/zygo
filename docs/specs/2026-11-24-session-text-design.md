# zygo: session text for each subagent and after a compaction — design

**Status:** approved by the operator 2026-11-24 (audit round 1 closed 2026-11-24)
**Date:** 2026-11-24
**Release:** phase 3, track E (roadmap items P3-14 and P3-15, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`; constraint `C6`). Also cited: `docs/specs/2026-11-20-workspaces-design.md` (`W§n`). The build is governed by `docs/specs/2026-11-24-decision-records-design.md`. `T§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's "Plan of work", the P3-14 and P3-15 design notes). **(lead)**: the lead of this design work ruled it after an independent audit (2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767`. Claude Code documentation references, read 2026-11-24:

- **[Hooks]** https://code.claude.com/docs/en/hooks
- **[Subagents]** https://code.claude.com/docs/en/sub-agents
- **[Context]** https://code.claude.com/docs/en/context-window, section "What survives compaction"
- **[Env]** https://code.claude.com/docs/en/env-vars

---

## 1. Intent

zygo changes what an agent sees: the trim hook puts a marker in place of large tool output, the secret pass puts a redaction marker (§8.6 item 7) in place of a secret, and the guards refuse some tool calls. The session text (§8.6) explains these changes. Two gaps exist (roadmap, "The gaps today", verified 2026-11-23):

1. **P3-14.** Only the `zygo-<role>` subagents get the mechanics text (§8.6 item 7). The built-in, plugin and user subagents get the markers and the guards, but not the sentences that explain them.
2. **P3-15.** No premise, fixture or test records which zygo text is in the context after a compaction.

This spec closes both gaps. The Claude Code behavior after a compaction comes from the documentation (T§9). A short live check, run by hand by the operator, covers only what the documentation does not answer (T§12).

## 2. Goals

- **T-G1** Each subagent on Claude Code and OpenCode that runs on zygo's proxy gets the mechanics text at its start, with no change to the definition of an agent that zygo did not write.
- **T-G2** For each agent (Claude Code main loop, Claude Code subagent, OpenCode request), a documented fact or a premise with a recorded fixture, and a test, state which zygo text is in the context after a compaction.
- **T-G3** If a compaction removes the text, zygo gives it again (roadmap P3-15 note). If the text is then in the context two times, this spec states the cost.
- **T-G4** Each piece of text has one owner for each receiver (T§5).

## 3. Non-goals

- A change to the words of the text (§8.6 items 1 to 8 keep their words).
- Work units for subagents that are not `zygo-<role>` agents (T§8.3).
- A block or a redirect of the lead away from other agent types (T§8.2).
- The full session text for a subagent. A subagent gets the mechanics text only, as a `zygo-<role>` agent does today.
- OpenCode subagents that run on the main role's model (a user agent with no `model`, or `general` when `main_role = "code"`). The roadmap records that the transform does not give zygo text to subagents today. If OpenCode calls the transform for a subagent request (ST4), such a subagent gets the full session text, because the plugin selects by model (`crates/zygo-launch/assets/opencode-plugin.js:185-190`). This spec does not change that.
- The `--bare` mode and `disableAllHooks: true`. They stop every hook (§8.5), so they stop this text as they stop the main session text.

## 4. Constraints

- **T-C1** C6 applies to each new hook branch: on an internal error the hook prints nothing that changes a decision, writes one line to stderr, and exits 0.
- **T-C2** zygo does not change an agent definition that it did not write: a user `--agents` entry is merged as given, a plugin agent keeps its own prompt (roadmap P3-14 note, verified), and a built-in agent keeps its built-in prompt.
- **T-C3** No CI test runs a real `claude` or `opencode` binary. CI uses the stub agents and the recorded fixtures. No CI test depends on wall-clock time.
- **T-C4** Nothing in the added text names a context window, a size or a price (C7, §8.6).
- **T-C5** The added text is the existing mechanics text (`crates/zygo-hooks/src/session_text.rs:17-30`), with the session's `results` and `work_units` overrides applied, as `crates/zygo-hooks/src/session_start.rs:33-39` applies them to the session text.
- **T-C6 (lead)** Where the Claude Code documentation answers a question, the design follows the documentation. A live result that contradicts the documentation is reported to the lead. It does not change the code by itself.

## 5. Owners of the text

One owner for each piece of text, for each receiver.

| Text | Receiver | Owner (channel) | Basis |
|---|---|---|---|
| Session text, §8.6 items 1-8 | Claude Code main loop, at each `SessionStart` (all sources, `compact` included) | `zygo hook session-start` (unchanged) | [Context] |
| Mechanics text | `zygo-<role>` subagent, Claude Code | `--agents` prompt, launch time (unchanged, `crates/zygo-launch/src/claude_code.rs:397,406`) | system prompt survives compaction, [Context] |
| Mechanics text | every other Claude Code subagent, at its start | `zygo hook session-start` on `SubagentStart` (new, T§6) | [Hooks]; ST1, ST2 |
| Mechanics text | the same subagent, after its own compaction | `zygo hook trim` on `PostToolUse`, once per compaction (new, T§9.2) | [Hooks] |
| Session text | OpenCode request on `zygo/<main_role>` | plugin system-prompt transform (unchanged) | ST3 |
| Mechanics text | OpenCode request on `zygo/<other role>` | plugin system-prompt transform (new, T§7) | ST3, ST4 |
| Mechanics text | OpenCode `zygo-<role>` agent prompt | `crates/zygo-launch/src/opencode.rs:139-143` until ST4 is recorded as verified; then none | ST4 |
| Compaction instructions (§10.6) | the compaction request | `zygo hook pre-compact` (unchanged text) | none |

## 6. Subagents on Claude Code (P3-14)

### 6.1 Today

- The `--agents` JSON gives each `zygo-<role>` agent the role prompt, one space, and the mechanics text, built at launch (`crates/zygo-launch/src/claude_code.rs:395-420`; `mechanics(&ctx.view)` at `:397`, the prompt at `:406`). User `--agents` keys are merged after, and a user key with the same name wins (`:418`).
- `zygo hook session-start` returns nothing for an input with `agent_id` (`crates/zygo-hooks/src/session_start.rs:24-27` and `:67-69`).
- zygo registers `SubagentStart` only for a workspace member, as `workspace --event subagent-start` (`crates/zygo-launch/src/claude_code.rs:497`). That branch prints nothing (`crates/zygo-hooks/src/workspace.rs:315-330`).
- The trim hook, the guard hook and the secret pass act on each subagent: the trim hook keys its work-log line by `agent_id` and does not skip a subagent (`crates/zygo-hooks/src/trim.rs:132-140`); the guard hook runs for every `PreToolUse` (`crates/zygo-work/src/guards.rs:41-51`); the secret pass acts on each request body (`crates/zygo-proxy/src/redact_stage.rs:98-110`).

### 6.2 Facts from the documentation and the fixtures

- "SubagentStart hooks can't block subagent creation, but they can inject context into the subagent." `additionalContext` is a "String added to the subagent's context at the start of its conversation, before its first prompt." ([Hooks], SubagentStart.)
- `SubagentStart` runs "when Claude spawns a subagent with the Agent tool, when Claude resumes a subagent, and each time an in-process agent team teammate handles a new message." "When the hook runs again for the same subagent, Claude Code injects the returned context only when the subagent's context doesn't already hold the copy from an earlier run. After auto-compaction discards that copy, Claude Code injects the next run's context again." ([Hooks], SubagentStart.)
- The matcher and the `agent_type` input are the agent name: `general-purpose`, `Explore`, `Plan` for built-in agents, the custom name for a user agent, and the plugin-scoped name such as `my-plugin:reviewer` for a plugin agent ([Hooks], SubagentStart). The fixture `fixtures/claude-code/hooks/SubagentStart-sub.json` holds `agent_type: "claude"`, the built-in catch-all agent ([Subagents], "Other" table).
- "For the built-in `web-fetch` subagent, Claude Code runs `SubagentStart` hooks from managed settings only." ([Hooks], SubagentStart.)
- A non-fork subagent starts with "a fresh, isolated context window". A fork "inherits the parent conversation instead of starting fresh" ([Subagents]).
- Tool events in a subagent carry `agent_id` and `agent_type`: "When a subagent calls a tool, tool events such as `PreToolUse` and `PostToolUse` fire the same configured hooks as in the main conversation, and the input carries the `agent_id` and `agent_type` common input fields that identify the subagent." ([Hooks]). Recorded: `fixtures/claude-code/hooks/PostToolUse-Bash-sub.json`.
- `PostToolUse` `additionalContext` is a "String added to Claude's context alongside the tool result." ([Hooks], PostToolUse decision control.)
- A subagent's compaction sends `PreCompact` with `agent_id`, `agent_type` and `trigger: "auto"`: recorded in `fixtures/claude-code/hooks/PreCompact-sub.json` (Claude Code 2.1.328, `docs/premises.md`). "Subagents support automatic compaction." ([Subagents].)

### 6.3 Design

1. **Registration (author).** Every launch registers `SubagentStart` (no matcher) → `<zygo> hook session-start`. A workspace member keeps its `workspace --event subagent-start` entry; the `SubagentStart` list is `session-start` first, then `workspace --event subagent-start`. The entry has no `--event` flag: the hook reads `hook_event_name` from its input.
2. **The subagent branch.** In `zygo hook session-start`, the two early returns for an input with `agent_id` (`crates/zygo-hooks/src/session_start.rs:25-27`, `:67-69`) are replaced by one function. For an input with `agent_id`:
   - When `agent_type` is a string that starts with `zygo-`: nothing. This is the rule of `is_zygo` (`crates/zygo-work/src/hooks.rs:159-161`). The agent has the text in its system prompt (T§5).
   - Else, when `hook_event_name` is `SubagentStart`: print `{"hookSpecificOutput":{"hookEventName":"SubagentStart","additionalContext":<mechanics>}}`.
   - Else (for example a subagent `SessionStart` with `source: "compact"`, `fixtures/claude-code/hooks/SessionStart-sub.json`): nothing, as today. T§9.2 owns the copy after a subagent's compaction.
   `<mechanics>` is `mechanics(v)` of the session view `v` with the `results` and `work_units` overrides applied (T-C5). A missing `agent_type`, or one that is not a string, counts as not `zygo-`. The member part of `run_member` never runs for an input with `agent_id`, as today.
3. **No other change to the `--agents` prompt (author).** A `zygo-<role>` agent keeps the mechanics text in its prompt, built at launch. See T§8.4 for the overrides.

### 6.4 Known limits (author)

- **User agent with a `zygo-` name.** A user `--agents` key that replaces a `zygo-<role>` agent (§8.5) gets no mechanics text from zygo, because its name starts with `zygo-`. The same name rule already makes such an agent a zygo role for work units (`crates/zygo-work/src/hooks.rs:208-212`). Consequence: that one agent can see a marker with no explanation. The user wrote its prompt.
- **`web-fetch`.** Claude Code runs no settings `SubagentStart` hook for it (T§6.2). It gets no mechanics text. Its tool output still goes through the trim hook.
- **Fork.** A fork inherits the parent's context, which holds the session text. If Claude Code runs `SubagentStart` for a fork, the fork holds the mechanics text two times (T§11).

## 7. Subagents on OpenCode (P3-14)

### 7.1 Today

- The built-in agents get a model and an edit permission only (`crates/zygo-launch/src/opencode.rs:130-138`, map at `:18`: `build` → `lead`, `plan` → `plan`, `general` → `code`, `explore` → `read`). Each `zygo-<role>` agent gets the role prompt and the mechanics text (`:139-148`). Converted plugin agents are merged after (`:149`).
- The plugin's system-prompt transform calls `zygo hook session-start` only when the request's model is `zygo/<ZYGO_MAIN_ROLE>`, and pushes the output into `output.system` (`crates/zygo-launch/assets/opencode-plugin.js:185-190`). The transform input has the model and no agent id (§8.7 note; `fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json`). The OpenCode documentation does not name this hook (the reason of the OpenCode A-8.1 grade, capability A-8, in `docs/conformance/opencode.toml`).

### 7.2 Design: the mechanism without an agent id (author)

The request's model identifies the receiver. A request on `zygo/<main_role>` is the main agent; a request on another `zygo/<role>` is a subagent, a primary agent that is not the main one (`plan`), or the title agent (`small_model`, §8.7 note).

1. The transform runs for each request whose `input.model.providerID` is `zygo`:
   - model id equal to `ZYGO_MAIN_ROLE`: `run("session-start", {input, output})`, as today;
   - else (another role, or `ZYGO_MAIN_ROLE` not set): `run("session-start", {input, output}, ["--event", "subagent"])`.
   A non-empty output is pushed into `output.system`, as today. A request on another provider gets nothing.
2. `zygo hook session-start --agent opencode --event subagent` prints the mechanics text (T-C5) as plain text and returns before any member part: no resume note, no view write.
3. **Duplicate in `zygo-<role>` prompts.** Until ST4 is recorded, the `zygo-<role>` prompts keep the mechanics text (`crates/zygo-launch/src/opencode.rs:143`), and such an agent gets the text two times if the transform runs for it (T§11). ST4 verified: the prompt becomes the role prompt only, and the transform is the one owner. ST4 failed: the `--event subagent` branch of the plugin and of the hook is removed with its tests, and the `zygo-<role>` prompts keep the text.

## 8. The open points of P3-14, settled

### 8.1 OpenCode without an agent id (author)

T§7.2: the request's model, which the transform receives, selects the text.

### 8.2 Steering the lead away from other agent types (author)

No steering is added. No `PreToolUse` entry on the agent tool is registered for this purpose. Reasons:

- After T§6 and T§7, a subagent of another type gets the same mechanics text as a `zygo-<role>` agent, so the gap that the review found is closed.
- Plugin agents (for example `act101:code-explorer`) and user agents are features that the user enabled. A deny would remove them.
- The legend, the alias sentence and the capabilities sentence (§8.6 items 4 to 6) already point the lead at the `zygo-<role>` agents. `CLAUDE_CODE_SUBAGENT_MODEL = code` (§8.5) already routes a subagent with no model to a role.
- Cost to reverse is low: one `PreToolUse` `Agent` entry with a notify text.

The workspace-member deny of the agent tool (W§9.2) is a different rule and does not change.

### 8.3 Work units for other subagents (author)

No. A `work_begin` from a subagent whose type does not start with `zygo-` stays ignored (`crates/zygo-work/src/hooks.rs:208-212`), and its `SubagentStop` folds nothing (`:514-519`, `is_zygo` at `:517`). Reasons: a unit has a role, a check and a verdict by a `zygo-review` agent (§13); a plugin or built-in agent has none of these. The agent's tool calls still pass the guards. Its commits carry the `Zygo-Session` trailer through the mechanics text, and no `Zygo-Work` trailer, because its brief names no unit.

### 8.4 Launch-time `--agents` prompt and later overrides (author)

The mechanics text in a Claude Code `zygo-<role>` prompt stays a launch-time value, as the role prompt is (roadmap P3-15 note "Launch-time values"; §17.1 "next launch" scope for values that the agent reads only at start). The text that the hooks give (T§6.3, T§7.2, T§9.2) reads the overrides at each run. The difference has no effect on a decision:

- `results off` after launch: the store sentence stays in the prompt, but no trim marker appears, so the sentence has no trigger.
- `units off` after launch: the trailer sentence stays in the prompt; a `Zygo-Session` trailer on a commit is harmless.
- `units on` after launch, when the launch had units off: the work hooks were not registered (`crates/zygo-launch/src/claude_code.rs:482-488`), so no unit exists for the agent, and no trailer sentence is needed.

Moving the `zygo-<role>` text to the hook was rejected (T§15, item 1).

### 8.5 §8.6 item 7 and the code (author)

Both change. The intent of item 7 holds: each subagent gets the mechanics text. The code changes to meet it (T§6, T§7, T§9.2). The item's words change to name the channel for each receiver (T§13), because "appended to each subagent prompt" is not possible for an agent that zygo does not define (T-C2).

## 9. Text after a compaction (P3-15)

### 9.1 Claude Code main loop (documented)

Today: zygo registers `SessionStart` with no matcher (`crates/zygo-launch/src/claude_code.rs:510-513`), and the hook does not read `source` (`crates/zygo-hooks/src/session_start.rs:23-30`). So the hook gives the session text for each main-loop `SessionStart`, of any source.

The documentation answers both unknowns of the roadmap note ([Context], "What survives compaction"):

- "Context that hooks added earlier | Summarized with the rest of the conversation". So the startup copy is not kept word for word.
- "SessionStart hooks that match the `compact` source | Claude Code runs them and adds their output to the compacted context". The `compact` source fires on "Auto or manual compaction" ([Hooks], SessionStart matcher table).

**Design (lead):** no code change. After each compaction the main loop holds one copy of the session text, from the `SessionStart` with `source: "compact"`. A test pins this (T§17, check 6).

Resume, clear and fork are not changed. On `clear` the history is gone, so the text is there one time. On `resume` and `fork` the hook gives the text again ([Hooks]: "`SessionStart` hooks run again on resume … so they can refresh their context"); Claude Code "replays the saved text" of mid-session events on resume ([Hooks], Add context for Claude), so an earlier copy can also be there (T§11).

### 9.2 Claude Code subagent

A `zygo-<role>` agent has the text in its system prompt. The system prompt "still applies" after a compaction ([Context]). Nothing changes for it.

For another subagent, the documentation says that its auto-compaction discards the `SubagentStart` copy, and that `SubagentStart` injects it again only at "the next run" ([Hooks], SubagentStart), that is at a resume of that subagent. So between the compaction and a resume the text is lost.

**Design (lead):** the hook gives it again, through a per-agent flag.

1. `zygo hook pre-compact`, for an input with `agent_id` whose `agent_type` does not start with `zygo-`, creates the empty file `$ZYGO_SESSION_DIR/reinject/<agent_id>` (directory `0700`, file `0600`). The `agent_id` must pass the agent-name rule of the work log (`crates/zygo-work/src/log.rs:141-146`: 1 to 64 characters of ASCII letters, digits, `_` and `-`); another value creates no file. The hook's output, the §10.6 text, does not change.
2. `zygo hook trim` (`PostToolUse`, matcher `Bash|Read|Grep|Glob|WebFetch`, `crates/zygo-launch/src/claude_code.rs:476`), for an input with `agent_id` whose flag file exists: removes the file, then adds `"additionalContext": <mechanics>` to its `hookSpecificOutput` (`hookEventName` `PostToolUse`). When the trim itself has nothing to print (`crates/zygo-hooks/src/trim.rs:20-25`, `:99-112`), the output is `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":<mechanics>}}`. The flag check runs before the trim's own early returns. The text is given only when the remove succeeds, so two hook runs cannot both give it.
3. Main-loop inputs (no `agent_id`) never read or write a flag; the main loop is covered by T§9.1.
4. A subagent `SessionStart` with `source: "compact"` gets nothing (T§6.3 item 2), so the flag is the one owner after a subagent's compaction.

Limits (author): the copy arrives with the result of the subagent's next `Bash`, `Read`, `Grep`, `Glob` or `WebFetch` call. A subagent that calls only other tools after its compaction does not get it. Those other tools give no trim marker. A flag whose subagent ends before such a call stays in the session folder until the session sweep (§10.1); an `agent_id` is not reused, so it never fires.

Reasons for this channel (author): `PostToolUse` is a documented context channel in a subagent (T§6.2), and the trim entry already runs on these tools, so no hook process is added per tool call.

### 9.3 OpenCode

The transform runs per model request and writes into `output.system`, which is not in the message history. So a compaction cannot remove that text if OpenCode calls the transform on the first request after a compaction (ST3). The OpenCode documentation does not answer this (the reason of the OpenCode A-8.1 grade, capability A-8, in `docs/conformance/opencode.toml`).

| ST3 | Behavior |
|---|---|
| no row yet, or verified | No change. |
| failed | Fallback F3: the plugin's `experimental.session.compacting` handler also calls `run("session-start", {input, output}, ["--event", "subagent"])` and pushes `Keep these rules word for word in the summary: ` followed by the output into `output.context`. The compacting input has a session id only, so the mechanics text, which fits every session, is the text used. |

## 10. Flows

| Flow | Text in context after this spec |
|---|---|
| Claude Code main loop, `startup` | Session text (`SessionStart`). |
| Claude Code main loop, `resume`, `fork` | Session text (`SessionStart`); a replayed earlier copy is possible (T§11). |
| Claude Code main loop, `clear` | Session text, one copy. |
| Claude Code main loop, compaction | Session text, one copy (T§9.1). |
| `zygo-<role>` subagent (start, resume, compaction) | Role prompt and mechanics text in the system prompt; no hook gives text. |
| Built-in `Explore`, `Plan`, `general-purpose`, `claude`, `statusline-setup`, `claude-code-guide` | Mechanics text from `SubagentStart`; after its compaction, from the next trim-matched tool result (T§9.2). |
| Plugin agent (`<plugin>:<name>`) | Same as the built-in agents. |
| User `--agents` entry, `.claude/agents/`, `~/.claude/agents/` | Same, unless its name starts with `zygo-` (T§6.4). |
| Nested subagent (a subagent's own Agent call) | Same rule by its type: `SubagentStart` runs for each spawn. |
| Resumed subagent | `SubagentStart` runs again; Claude Code adds the text only when its `SubagentStart` copy is not there (T§6.2). |
| Agent-team teammate | `SubagentStart` per message, with the same rule. |
| Fork | Parent context (session text), and the mechanics text if `SubagentStart` runs for it. |
| `web-fetch` | No zygo text (T§6.4). |
| OpenCode main agent (`zygo/<main_role>`) | Session text, each request. |
| OpenCode `zygo-<role>` agent | Mechanics text in its prompt; also from the transform if ST4 holds; after ST4 is verified, from the transform only. |
| OpenCode `general`, `explore`, `plan` (when not main), user and plugin agents on a `zygo/` model | Mechanics text from the transform, if ST4 holds. |
| OpenCode title request (`small_model`) | Mechanics text, if the transform runs for it. |
| OpenCode agent on another provider | No zygo text; its requests do not pass the proxy, so no redaction marker. |

## 11. Cost in tokens

Sizes are bytes of the text; tokens are bytes ÷ 4, the rule of §10.7.

- Mechanics text, all parts on: 721 bytes, about 180 tokens. With `results` and `work_units` off and no navigation: 317 bytes, about 79 tokens.
- Session text of the default bundle (lead prompt, legend of six roles, alias and capabilities sentences, mechanics text; no user prompt, profile prompt or work item): 2,087 bytes, about 520 tokens.

| Case | Added input tokens |
|---|---|
| Claude Code subagent of another type, at its start | about 180 per dispatch. |
| The same subagent, after each of its compactions | about 180 one time (T§9.2). |
| A resumed subagent that has the T§9.2 copy | about 180 more: `SubagentStart` does not see the `PostToolUse` copy as its own, so Claude Code adds its copy too. |
| Claude Code `zygo-<role>` agent | 0. |
| Claude Code fork, if `SubagentStart` runs for it | about 180, a second copy, per fork. |
| Claude Code main loop, compaction | 0 (one copy, as today). |
| Claude Code main loop, `resume` or `fork` with a replayed copy | about 520 until the next compaction or `clear`; not changed by this spec. |
| OpenCode request on another `zygo/` role | about 180 on each request. The text is the same on each request, so after the first request it is part of the cached prefix (§10.4). |
| OpenCode `zygo-<role>` request before ST4 is recorded, if the transform runs for it | about 180 on each request (second copy, cached). |
| OpenCode title request | about 180 per title request, if the transform runs for it. |
| F3 (only if ST3 fails) | about 180 in the compaction request; the summary keeps the rules. |

Hook run time: one more `zygo hook` process per Claude Code subagent dispatch and per OpenCode subagent request. The T§9.2 flag adds one file check to each trim run in a subagent. The main agent already pays one process per request on OpenCode (§8.7).

## 12. Premises and the live check

### 12.1 What is documented and what is checked

Documented, no premise row (T-C6): the main-loop text after a compaction (T§9.1); the loss of the `SubagentStart` copy at a subagent's auto-compaction (T§9.2); `PostToolUse` `additionalContext` and the `agent_id` of tool events in a subagent (T§6.2). The `PreCompact` input of a subagent is recorded (`fixtures/claude-code/hooks/PreCompact-sub.json`).

Checked by hand (T§12.2), one result per id:

| Id | Premise | If verified | If failed |
|---|---|---|---|
| ST1 | A `SubagentStart` `additionalContext` reaches the context of a built-in subagent (`general-purpose`). | T§6.3 as written. | Documented behavior is contradicted: the lead reports it to the operator; no code changes by itself (T-C6). |
| ST2 | The same for a plugin subagent (type `<plugin>:<name>`). | T§6.3 as written. | As ST1. |
| ST3 | OpenCode calls `experimental.chat.system.transform` on the first main-model request after a compaction. | No change. | F3 (T§9.3). |
| ST4 | OpenCode calls the transform for a subagent request (the `task` tool), with the subagent's model in `input.model`. | `zygo-<role>` prompts lose the mechanics text (T§7.2 item 3). | The `--event subagent` branch is removed (T§7.2 item 3). |

**How a result enters the repository (author).** The lead, after the operator sends the files of T§12.2:

1. Commits the fixtures, after `N2_FORBIDDEN_NAMES='<operator user name> <other names>' sh scripts/check-premises.sh` passes on them.
2. Adds one row per id to the `docs/premises.md` "Results" table, in its existing columns: `| STn | <premise> | Claude Code or OpenCode | <version> | <date> | verified or failed | <evidence paths> |`.
3. For a failed id, adds a dated note to T§12.1 of this spec: `*(Note YYYY-MM-DD, P3-15, STn: <result>)*`.
4. In the same commit, changes `scripts/check-premises.sh`: a new list `SIDS="ST1 ST2 ST3 ST4"` and `SSPEC=docs/specs/2026-11-24-session-text-design.md`; the row loop runs over `$IDS $WIDS $SIDS` (today `$IDS $WIDS`, `:58`); the failed-note `case` gets an `ST*)` arm before the `P*)` arm (`:106`), which looks for `*(Note YYYY-MM-DD, P3-15, STn: …)*` in `$SSPEC`. The row checks for date, agent version, `verified|failed` (`:90-91`) and nontrivial evidence apply unchanged.
5. In the same commit, makes the code change that the result's row names (T§7.2 item 3, T§9.3), with its tests.

Tests do not read `docs/premises.md`. A test reads the recorded fixtures and checks the code as built for the recorded result. Before the rows exist, the code is built for "no row yet" (ST3: no change; ST4: prompts keep the text and the transform branch exists), and the script has no `SIDS`.

### 12.2 Live check P3-15 (operator, by hand)

Run on your own machine with your normal Claude Code and OpenCode logins. It runs no zygo and no CI job; from this repository it uses only `scripts/premises/scrub.py`. Claude Code runs once in print mode on Haiku with no compaction: about 5 model requests, about 80,000 input tokens in all (each request sends the whole context again; most of it is a cache read). OpenCode runs three short turns and one compaction on the models you pick.

What it leaves on your machine, and how to remove it (step 8): Claude Code writes its transcripts to `~/.claude/projects/<folder name>/`, and it can add an entry for the scratch folder to `~/.claude.json`; OpenCode keeps the scratch session in its own data folder. It does not load your user settings or MCP servers (`--setting-sources project,local`, `--strict-mcp-config`), and it does not change your config files.

**Claude Code (ST1, ST2)**

1. Make a scratch folder with the probe hook, its settings file and a one-agent plugin:

```sh
S=$(mktemp -d -t p315) && cd "$S" && git init -q
cat > probe.py <<'EOF'
import json, sys
d = json.load(sys.stdin)
open(sys.argv[1], "a").write(json.dumps(d) + "\n")
ev = d.get("hook_event_name", "")
if ev in ("SessionStart", "SubagentStart"):
    who = "sub" if "agent_id" in d else "main"
    tag = "CANARY-%s-%s-%s" % (ev, d.get("source", "na"), who)
    print(json.dumps({"hookSpecificOutput": {"hookEventName": ev, "additionalContext": "\n" + tag}}))
EOF
H="python3 -I $S/probe.py $S/hooks.jsonl"
E='[{"hooks":[{"type":"command","command":"'"$H"'"}]}]'
printf '{"hooks":{"SessionStart":%s,"SubagentStart":%s}}\n' "$E" "$E" > settings.json
mkdir -p plug/p315/.claude-plugin plug/p315/agents
printf '{"name":"p315","version":"0.0.1"}\n' > plug/p315/.claude-plugin/plugin.json
printf -- '---\nname: probe\ndescription: Replies DONE.\n---\nReply with the word DONE and nothing else.\n' > plug/p315/agents/probe.md
echo "$S"
```

2. Run Claude Code once. It waits for each subagent (no background subagents, no fork mode) and exits after its reply:

```sh
cd "$S" && CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1 CLAUDE_CODE_FORK_SUBAGENT=0 claude -p --model haiku --setting-sources project,local --strict-mcp-config --settings "$S/settings.json" --plugin-dir "$S/plug/p315" "Call the general-purpose subagent with the task 'Reply with the word DONE.' and wait for its result. Then call the p315:probe subagent with the task 'Reply with the word DONE.' and wait for its result. Then reply with both results."
```

3. Read the evidence from the subagent transcripts (`~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl`, [Subagents]). Only entries that are not `assistant` entries count, so a model's own words are never evidence:

```sh
python3 -I - "$S/hooks.jsonl" > "$S/evidence.txt" <<'EOF'
import json, os, sys
hooks = [json.loads(l) for l in open(sys.argv[1])]
main = next(h for h in hooks if h.get("hook_event_name") == "SessionStart" and "agent_id" not in h)
base = main["transcript_path"][: -len(".jsonl")]
for h in hooks:
    if h.get("hook_event_name") != "SubagentStart":
        continue
    p = os.path.join(base, "subagents", "agent-%s.jsonl" % h["agent_id"])
    kinds = []
    if os.path.exists(p):
        for line in open(p):
            e = json.loads(line)
            if e.get("type") != "assistant" and "CANARY-SubagentStart" in line:
                kinds.append(str(e.get("type")))
    print(h.get("agent_type"), "transcript" if os.path.exists(p) else "no-transcript", "canary:" + (",".join(kinds) or "none"))
EOF
cat "$S/evidence.txt"
```

**OpenCode (ST3, ST4)**

4. Pick two different model ids of one OpenCode provider, `MAIN` and `SUB` (for example a Sonnet id and a Haiku id). `small_model` is set to `MAIN`, so a title request cannot look like a subagent request. Make the probe plugin and start OpenCode in the scratch folder:

```sh
mkdir -p "$S/oc/plugins" && cat > "$S/oc/plugins/probe.js" <<'EOF'
import { appendFileSync } from "node:fs";
const put = (o) => { try { appendFileSync(process.env.P315_LOG, JSON.stringify({ t: Date.now(), ...o }) + "\n"); } catch {} };
export const P315 = async () => ({
  "experimental.chat.system.transform": async (input) => put({ hook: "transform", model: input?.model?.id, session: input?.sessionID ?? null, keys: Object.keys(input ?? {}).sort() }),
  "experimental.session.compacting": async (input) => put({ hook: "compacting", session: input?.sessionID ?? null, keys: Object.keys(input ?? {}).sort() }),
});
EOF
cd "$S" && P315_LOG="$S/oc.jsonl" OPENCODE_CONFIG_DIR="$S/oc" OPENCODE_CONFIG_CONTENT='{"small_model":"<provider>/<MAIN>","agent":{"general":{"model":"<provider>/<SUB>"}}}' opencode -m <provider>/<MAIN>
```

5. Type: `Use the general subagent to read probe.py and tell me its first line.` Wait until its answer is shown. Type `/compact` and wait until the summary is shown. Type `Reply OK.` and wait for the reply. Exit OpenCode.

**Capture**

6. From the checkout of this repository:

```sh
python3 -I scripts/premises/scrub.py "$S" "$S/hooks.jsonl" > fixtures/claude-code/live/st-hooks.jsonl
cp "$S/evidence.txt" fixtures/claude-code/live/st-evidence.txt
python3 -I scripts/premises/scrub.py "$S" "$S/oc.jsonl" > fixtures/opencode/live/st-transform.jsonl
claude --version; opencode --version
```

7. Send the three files and the two version lines to the lead.

8. Remove what the check left. Close every Claude Code session first, because `~/.claude.json` is shared:

```sh
rm -rf ~/.claude/projects/*p315*
cp ~/.claude.json ~/.claude.json.bak-p315
python3 -I -c 'import json,os; p=os.path.expanduser("~/.claude.json"); d=json.load(open(p)); r=[k for k in list(d.get("projects",{})) if "p315" in k]; [d["projects"].pop(k) for k in r]; json.dump(d,open(p,"w"),indent=2); print("removed", r)'
rm -rf "$S"
```

The OpenCode session stays in OpenCode's data folder; delete it from OpenCode's session list if you want it gone.

**Reading the result** (the lead, T§12.1 "How a result enters the repository"):

- ST1 verified: the `general-purpose` line of `st-evidence.txt` says `transcript` and `canary:` with at least one entry. ST1 failed: it says `transcript` and `canary:none`. `no-transcript`: not decided; no row.
- ST2: the same for the `p315:probe` line.
- ST3 verified: `st-transform.jsonl` holds a `transform` line with model `MAIN` after the `compacting` line, with the same `session` when both lines have one. Failed: no such line, while the log shows the `Reply OK.` turn ran (a `transform` line before the `compacting` line exists).
- ST4 verified: it holds a `transform` line with model `SUB`. Failed: none, while the subagent's answer was shown in step 5.
- The `keys` lists go into the rows as information; they change no behavior in this spec.

## 13. Base-spec changes

- **§8.5, hooks list:** add "`SubagentStart` → `zygo hook session-start` (every launch; for a member, before `workspace --event subagent-start`)".
- **§8.6 item 7**, the parenthesis becomes: "(also given to each subagent: in the `--agents` prompt of each Claude Code `zygo-<role>` agent, built at launch; by `zygo hook session-start` on `SubagentStart` to every other Claude Code subagent whose type does not start with `zygo-`, and again after that subagent's compaction (T§9.2); on OpenCode by the system-prompt transform on each request whose model is a `zygo/` role other than the main role, T§7)".
- **§8.6, new last paragraph:** "After a compaction: the main loop gets the text again from `SessionStart` with `source: compact`; see `docs/specs/2026-11-24-session-text-design.md` T§9."
- **§8.7 note (N33):** "so subagents and the title agent do not get it" becomes "so subagents and the title agent get the mechanics text only (T§7)".
- **§10.6:** add "For a subagent (an input with `agent_id`) the hook also sets the flag of T§9.2."
- After the ST4 row: §8.7 says whether the OpenCode `zygo-<role>` prompt holds the mechanics text.

## 14. Failure handling

- The session view cannot be loaded: as `run_in` today (`crates/zygo-hooks/src/session_start.rs:28`); the error goes to the C6 wrapper (`crates/zygo-core/src/hook.rs:75-76`), which prints nothing and writes one stderr line. The subagent starts with no zygo text.
- The flag cannot be created (T§9.2 item 1): a C6 internal error of `pre-compact`, reported as one stderr line after the §10.6 text is printed; the §10.6 text is still given. The flag cannot be removed (item 2): no text is given, one stderr line, and the trim output is unchanged.
- The OpenCode hook call fails or passes its 10 s bound: the plugin's `run` gives `""` (`crates/zygo-launch/assets/opencode-plugin.js:12-15, 27-28`), and nothing is pushed.

## 15. Alternatives rejected

1. **The hook as the one owner for `zygo-<role>` agents too** (remove the text from the Claude Code `--agents` prompt). The hook text would follow the overrides, but the `zygo-<role>` text would then depend on T§9.2, which covers only the trim-matched tools. The gain is small (T§8.4). The system-prompt copy survives a compaction ([Context]).
2. **Append the text to user `--agents` entries and to plugin agent prompts at launch.** It breaks T-C2, and it cannot reach the built-in agents or the agents in `.claude/agents/`, which Claude Code reads itself.
3. **Replace a built-in agent with a `--agents` entry of the same name** (for example `Explore`). That replaces Claude Code's built-in prompt and tool rules ([Subagents]: "A user or project subagent named `Explore` overrides the built-in").
4. **`PreToolUse` on the agent tool, with `updatedInput` that adds the text to the task prompt.** It changes the brief that the lead wrote, and the copy is lost at the subagent's compaction. `SubagentStart` is the documented channel, and Claude Code removes its duplicates (T§6.2).
5. **Set `prompt` on the OpenCode built-in agents.** It replaces their built-in prompt.
6. **Give the text on `PostCompact`.** `PostCompact` has no decision control and no context channel ([Hooks], PostCompact).
7. **Answer the subagent `SessionStart` with `source: "compact"`.** The documentation does not say that its output reaches the subagent; the documented `compact` row is for the session ([Context]). The T§9.2 channel is documented for subagents.
8. **A new `PostToolUse` `*` entry for the T§9.2 copy.** It adds one hook process to every tool call of every session. The trim entry already runs on the tools that make trim markers.
9. **A deny or notify on other agent types.** T§8.2.
10. **A live check of the main loop and subagent compaction.** The documentation answers both (T§9.1, T§9.2). A run that needs a compaction costs several hundred thousand tokens on the operator's login.

## 16. Files expected to change

| File | Change |
|---|---|
| `crates/zygo-hooks/src/session_start.rs` | The subagent branch (T§6.3 item 2); the `--event subagent` branch (T§7.2 item 2). |
| `crates/zygo-hooks/src/pre_compact.rs` | The flag for a subagent (T§9.2 item 1); it then reads its input. |
| `crates/zygo-hooks/src/trim.rs` | The flag check and the `additionalContext` (T§9.2 item 2). |
| `crates/zygo-core/src/session_dir.rs` | The `reinject` directory path, next to `results()` and `work()`. |
| `crates/zygo-launch/src/claude_code.rs` | `SubagentStart` entry in `hooks` (T§6.3 item 1). |
| `crates/zygo-launch/assets/opencode-plugin.js` | Transform branch (T§7.2 item 1); F3 only if ST3 fails. |
| `crates/zygo-launch/src/opencode.rs` | Only when ST4 is recorded as verified: the `zygo-<role>` prompt is the role prompt only. |
| `crates/zygo-hooks/tests/hooks_basic.rs` (or a new test file), `crates/zygo-launch/tests/claude_code.rs`, `crates/zygo-launch/tests/opencode.rs` | Tests of T§17. |
| `fixtures/claude-code/live/st-hooks.jsonl`, `fixtures/claude-code/live/st-evidence.txt`, `fixtures/opencode/live/st-transform.jsonl` | From the operator (T§12.2). |
| `fixtures/claude-code/hooks/SubagentStart-plugin-sub.json` | The `p315:probe` `SubagentStart` line of `st-hooks.jsonl`. |
| `docs/premises.md`, `docs/live-checks.md` | ST1 to ST4 rows; a `P3-15` section with T§12.2. |
| `scripts/check-premises.sh` | `SIDS`, `SSPEC` and the `ST*)` arm (T§12.1 item 4), in the commit that adds the rows. |
| `docs/specs/2026-11-18-zygo-design.md` | T§13. |

## 17. Acceptance

Each check is decided by a test with the stub agents and recorded fixtures, or by a command.

1. The settings of a plain launch hold `SubagentStart` = one entry, command `<zygo> hook session-start`, no matcher. A member launch holds two entries in the order `session-start`, `workspace --event subagent-start`.
2. `session-start` with `fixtures/claude-code/hooks/SubagentStart-sub.json` (type `claude`), a view with results, navigation and units on, and no overrides prints exactly `{"hookSpecificOutput":{"hookEventName":"SubagentStart","additionalContext":M}}`, where M is `mechanics` of that view (721 bytes).
3. The same input with `agent_type` set to `Explore`, `Plan`, `general-purpose`, `p315:probe`, the number `1`, or with no `agent_type`, prints the same. With `agent_type` `zygo-code` it prints nothing.
4. With `overrides.json` `{"results": false, "work_units": false}` and navigation off, M is the redaction sentence, one space, the attribution sentence.
5. `session-start` with `fixtures/claude-code/hooks/SessionStart-sub.json` (`source: "compact"`) prints nothing.
6. A main-loop `SessionStart` input with `source: "compact"` (built in the test from `SessionStart-main.json` with `source` changed, named as a constructed input) prints the same output as `source: "startup"`.
7. `pre-compact` with `fixtures/claude-code/hooks/PreCompact-sub.json` prints the §10.6 text and creates `reinject/a1a80dd100f197242` (`0600`); with `PreCompact-main.json`, with `agent_type` `zygo-read`, or with `agent_id` `../x`, it creates no file.
8. After check 7, `trim` with `fixtures/claude-code/hooks/PostToolUse-Bash-sub.json` (output under the threshold) prints `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":M}}` and the file is gone; a second run prints nothing. With an output over the threshold, the one JSON object holds both `updatedToolOutput` and `additionalContext`. `PostToolUse-Bash-main.json` never reads a flag.
9. The Claude Code `--agents` value of a default launch still has each `zygo-<role>` prompt equal to the role prompt, one space, and M.
10. The generated plugin, run under `node` with a stub `zygo` that logs its arguments (the method of `crates/zygo-launch/tests/opencode.rs:703-707`, whose `code/zygo` and `read/zygo` cases change): with `ZYGO_MAIN_ROLE=lead`, a transform call with model `{providerID: "zygo", id: "read"}` calls `hook session-start --agent opencode --event subagent` and pushes the stub's output into `output.system`; with `id: "lead"` it calls `hook session-start --agent opencode` with no `--event`; with `providerID: "anthropic"` it calls nothing.
11. `zygo hook session-start --agent opencode --event subagent` in a member session view that holds resume note inputs prints M as plain text, and `view.json` is byte-equal before and after.
12. `session-start` with a `SubagentStart` input and a `view.json` that does not parse exits 0, prints nothing on stdout and one line on stderr.
13. After the ST rows are added: `sh scripts/check-premises.sh` exits 0; with one ST row's result changed to `pending`, it exits 1 with `STn: result 'pending' is not verified or failed`; with an ST row `failed` and no `*(Note …, P3-15, STn: …)*` in this spec, it exits 1.
14. Only with the matching row: F3 (the compacting handler pushes the rule line and M) when ST3 failed; the OpenCode `zygo-<role>` prompt equals the role prompt when ST4 is verified; the `--event subagent` branch and its tests are gone when ST4 failed.
15. No test added by this work runs a real `claude` or `opencode` binary or reads a wall clock (review of the diff).

## 18. Left to the builder

- The function names, the place of the subagent branch inside `session_start.rs`, and how `trim.rs` merges the field into its output.
- Whether the agent-name rule is shared from `zygo-work` or repeated in `zygo-hooks`, with the same character set and length.
- The test file layout.
- The exact words of the §8.6, §8.7 and §10.6 edits, inside the meaning of T§13.

## Change log

- 2026-11-25: §7.1 and §9.3 — the OpenCode documentation caveat points to the reason of the OpenCode A-8.1 grade (capability A-8) in `docs/conformance/opencode.toml`, not to `docs/conformance.md:42`, which is generated and keeps no hand note (pointer correction, node CF-4); author's call.
