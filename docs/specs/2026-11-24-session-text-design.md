# zygo: session text for each subagent and after a compaction — design

**Status:** proposed 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track E (roadmap items P3-14 and P3-15, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`; constraint `C6`). Also cited: `docs/specs/2026-11-20-workspaces-design.md` (`W§n`). The build is governed by `docs/specs/2026-11-24-decision-records-design.md`. `T§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's "Plan of work", the P3-14 and P3-15 design notes). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767`. Claude Code documentation references are to https://code.claude.com/docs/en/hooks ("Hooks reference"), https://code.claude.com/docs/en/sub-agents ("Subagents") and https://code.claude.com/docs/en/env-vars ("Environment variables"), read 2026-11-24.

---

## 1. Intent

zygo changes what an agent sees: the trim hook puts a marker in place of large tool output, the secret pass puts a redaction marker (§8.6 item 7) in place of a secret, and the guards refuse some tool calls. The session text (§8.6) explains these changes. Two gaps exist (roadmap, "The gaps today", verified 2026-11-23):

1. **P3-14.** Only the `zygo-<role>` subagents get the mechanics text (§8.6 item 7). The built-in, plugin and user subagents get the markers and the guards, but not the sentences that explain them.
2. **P3-15.** No premise, fixture or test records which zygo text is in the context after a compaction.

This spec closes both gaps. It adds one hook entry and one plugin branch, and it adds five premises with a live check that the operator runs by hand.

## 2. Goals

- **T-G1** Each subagent on Claude Code and OpenCode that runs on zygo's proxy gets the mechanics text one time at its start, with no change to the definition of an agent that zygo did not write.
- **T-G2** For each agent (Claude Code main loop, Claude Code subagent, OpenCode request), a premise, a recorded fixture and a test state which zygo text is in the context after a compaction.
- **T-G3** If a compaction removes the text, zygo gives it again. If the text is then in the context two times, this spec states the cost, or zygo stops the second copy.
- **T-G4** Each piece of text has one owner for each receiver (T§5).

## 3. Non-goals

- A change to the words of the text (§8.6 items 1 to 8 keep their words).
- Work units for subagents that are not `zygo-<role>` agents (T§8.3).
- A block or a redirect of the lead away from other agent types (T§8.2).
- The full session text for a subagent. A subagent gets the mechanics text only, as a `zygo-<role>` agent does today.
- OpenCode subagents that run on the main role's model (a user agent with no `model`, or `general` when `main_role = "code"`). They get the full session text today (`crates/zygo-launch/assets/opencode-plugin.js:185-190`); this spec does not change that.
- The `--bare` mode and `disableAllHooks: true`. They stop every hook (§8.5), so they stop this text as they stop the main session text.

## 4. Constraints

- **T-C1** C6 applies to each new hook branch: on an internal error the hook prints nothing that changes a decision, writes one line to stderr, and exits 0.
- **T-C2** zygo does not change an agent definition that it did not write: a user `--agents` entry is merged as given, a plugin agent keeps its own prompt (roadmap P3-14 note, verified), and a built-in agent keeps its built-in prompt.
- **T-C3** No CI test runs a real `claude` or `opencode` binary. CI uses the stub agents and the recorded fixtures. No CI test depends on wall-clock time.
- **T-C4** Nothing in the added text names a context window, a size or a price (C7, §8.6).
- **T-C5** The added text is the existing mechanics text (`crates/zygo-hooks/src/session_text.rs:17-30`), with the session's `results` and `work_units` overrides applied, as `crates/zygo-hooks/src/session_start.rs:33-39` applies them to the session text.

## 5. Owners of the text

One owner for each piece of text, for each receiver. "Pending" is the behavior while the premise in the last column has no result row in `docs/premises.md` (T§12).

| Text | Receiver | Owner (channel) | Premise |
|---|---|---|---|
| Session text, §8.6 items 1-8 | Claude Code main loop | `zygo hook session-start` on `SessionStart` (unchanged) | PT1 for the `compact` source |
| Mechanics text | `zygo-<role>` subagent, Claude Code | `--agents` prompt, launch time (unchanged, `crates/zygo-launch/src/claude_code.rs:397,406`) | none |
| Mechanics text | every other Claude Code subagent | `zygo hook session-start` on `SubagentStart` (new, T§6) | PT0; PT2 after its own compaction |
| Session text | OpenCode request on `zygo/<main_role>` | plugin system-prompt transform (unchanged) | PT3 |
| Mechanics text | OpenCode request on `zygo/<other role>` | plugin system-prompt transform (new, T§7) | PT3, PT4 |
| Mechanics text | OpenCode `zygo-<role>` agent prompt | `crates/zygo-launch/src/opencode.rs:139-143` while PT4 is not verified; then none | PT4 |
| Compaction instructions (§10.6) | the compaction request | `zygo hook pre-compact` (unchanged) | none |

## 6. Subagents on Claude Code (P3-14)

### 6.1 Today

- The `--agents` JSON gives each `zygo-<role>` agent the role prompt, one space, and the mechanics text, built at launch (`crates/zygo-launch/src/claude_code.rs:395-420`; `mechanics(&ctx.view)` at `:397`, the prompt at `:406`). User `--agents` keys are merged after, and a user key with the same name wins (`:418`).
- `zygo hook session-start` returns nothing for an input with `agent_id` (`crates/zygo-hooks/src/session_start.rs:24-27` and `:67-69`).
- zygo registers `SubagentStart` only for a workspace member, as `workspace --event subagent-start` (`crates/zygo-launch/src/claude_code.rs:497`). That branch prints nothing (`crates/zygo-hooks/src/workspace.rs:315-330`).
- The trim hook, the guard hook and the secret pass act on each subagent: the trim hook keys its work-log line by `agent_id` and does not skip a subagent (`crates/zygo-hooks/src/trim.rs:132-140`); the guard hook runs for every `PreToolUse` (`crates/zygo-work/src/guards.rs:41-51`); the secret pass acts on each request body (`crates/zygo-proxy/src/redact_stage.rs:98-110`).

### 6.2 Facts from the Claude Code documentation

- "SubagentStart hooks can't block subagent creation, but they can inject context into the subagent." `additionalContext` is a "String added to the subagent's context at the start of its conversation, before its first prompt." (Hooks reference, SubagentStart.)
- `SubagentStart` runs "when Claude spawns a subagent with the Agent tool, when Claude resumes a subagent, and each time an in-process agent team teammate handles a new message." "When the hook runs again for the same subagent, Claude Code injects the returned context only when the subagent's context doesn't already hold the copy from an earlier run. After auto-compaction discards that copy, Claude Code injects the next run's context again." (same section).
- The matcher and the `agent_type` input are the agent name: `general-purpose`, `Explore`, `Plan` for built-in agents, the custom name for a user agent, and the plugin-scoped name such as `my-plugin:reviewer` for a plugin agent (same section). The recorded fixture `fixtures/claude-code/hooks/SubagentStart-sub.json` holds `agent_type: "claude"`, the built-in catch-all agent (Subagents, "Other" table).
- "For the built-in `web-fetch` subagent, Claude Code runs `SubagentStart` hooks from managed settings only." (same section).
- A non-fork subagent starts with "a fresh, isolated context window". A fork "inherits the parent conversation instead of starting fresh" (Subagents).
- The recorded fixture `fixtures/claude-code/hooks/SessionStart-sub.json` shows that Claude Code sends a `SessionStart` with `agent_id`, `agent_type` and `source: "compact"` inside a subagent after the subagent's compaction.

### 6.3 Design

1. **Registration (author).** Every launch registers `SubagentStart` (no matcher) → `<zygo> hook session-start`. A workspace member keeps its `workspace --event subagent-start` entry; the `SubagentStart` list is `session-start` first, then `workspace --event subagent-start`. The entry has no `--event` flag: the hook reads `hook_event_name` from its input.
2. **The subagent branch.** In `zygo hook session-start`, the two early returns for an input with `agent_id` (`crates/zygo-hooks/src/session_start.rs:25-27`, `:67-69`) are replaced by one function. For an input with `agent_id`:
   - When `agent_type` is a string that starts with `zygo-`: nothing. This is the rule of `is_zygo` (`crates/zygo-work/src/hooks.rs:159-161`). The agent has the text in its system prompt (T§5).
   - Else, when `hook_event_name` is `SubagentStart`: print `{"hookSpecificOutput":{"hookEventName":"SubagentStart","additionalContext":<mechanics>}}`.
   - Else, when `hook_event_name` is `SessionStart` and `source` is `compact`: by the PT2 row of T§9.2.
   - Else: nothing.
   `<mechanics>` is `mechanics(v)` of the session view `v` with the `results` and `work_units` overrides applied (T-C5). A missing `agent_type`, or one that is not a string, counts as not `zygo-`. The member part of `run_member` never runs for an input with `agent_id`, as today.
3. **No other change to the `--agents` prompt (author).** A `zygo-<role>` agent keeps the mechanics text in its prompt, built at launch. See T§8.4 for the overrides.

### 6.4 Known limits (author)

- **User agent with a `zygo-` name.** A user `--agents` key that replaces a `zygo-<role>` agent (§8.5) gets no mechanics text from zygo, because its name starts with `zygo-`. The same name rule already makes such an agent a zygo role for work units (`crates/zygo-work/src/hooks.rs:208-212`). Consequence: that one agent can see a marker with no explanation. The user wrote its prompt.
- **`web-fetch`.** Claude Code runs no settings `SubagentStart` hook for it (T§6.2). It gets no mechanics text. Its tool output still goes through the trim hook.
- **Fork.** A fork inherits the parent's context, which holds the session text. If Claude Code runs `SubagentStart` for a fork, the fork holds the mechanics text two times (T§11).

## 7. Subagents on OpenCode (P3-14)

### 7.1 Today

- The built-in agents get a model and an edit permission only (`crates/zygo-launch/src/opencode.rs:130-138`, map at `:18`: `build` → `lead`, `plan` → `plan`, `general` → `code`, `explore` → `read`). Each `zygo-<role>` agent gets the role prompt and the mechanics text (`:139-148`). Converted plugin agents are merged after (`:149`).
- The plugin's system-prompt transform calls `zygo hook session-start` only when the request's model is `zygo/<ZYGO_MAIN_ROLE>`, and pushes the output into `output.system` (`crates/zygo-launch/assets/opencode-plugin.js:185-190`). The transform input has the model and no agent id (§8.7 note; `fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json`).

### 7.2 Design: the mechanism without an agent id (author)

The request's model identifies the receiver. A request on `zygo/<main_role>` is the main agent; a request on another `zygo/<role>` is a subagent, a primary agent that is not the main one (`plan`), or the title agent (`small_model`, §8.7 note).

1. The transform runs for each request whose `input.model.providerID` is `zygo`:
   - model id equal to `ZYGO_MAIN_ROLE`: `run("session-start", {input, output})`, as today;
   - else (another role, or `ZYGO_MAIN_ROLE` not set): `run("session-start", {input, output}, ["--event", "subagent"])`.
   A non-empty output is pushed into `output.system`, as today. A request on another provider gets nothing.
2. `zygo hook session-start --agent opencode --event subagent` prints the mechanics text (T-C5) as plain text and returns before any member part: no resume note, no view write.
3. **Duplicate in `zygo-<role>` prompts.** While PT4 is not verified, the `zygo-<role>` prompts keep the mechanics text (`crates/zygo-launch/src/opencode.rs:143`), and such an agent gets the text two times (T§11). When PT4 is verified, the prompt is the role prompt only, and the transform is the one owner. When PT4 fails, the `--event subagent` branch of the plugin and of the hook is removed with its tests, and the `zygo-<role>` prompts keep the text.

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

The mechanics text in a Claude Code `zygo-<role>` prompt stays a launch-time value, as the role prompt is (roadmap P3-15 note "Launch-time values"; §17.1 "next launch" scope for values that the agent reads only at start). The text that the hooks give (T§6.3, T§7.2) reads the overrides at each run. The difference has no effect on a decision:

- `results off` after launch: the store sentence stays in the prompt, but no trim marker appears, so the sentence has no trigger.
- `units off` after launch: the trailer sentence stays in the prompt; a `Zygo-Session` trailer on a commit is harmless.
- `units on` after launch, when the launch had units off: the work hooks were not registered (`crates/zygo-launch/src/claude_code.rs:482-488`), so no unit exists for the agent, and no trailer sentence is needed.

Moving the `zygo-<role>` text to the hook was rejected (T§15, item 1).

### 8.5 §8.6 item 7 and the code (author)

Both change. The intent of item 7 holds: each subagent gets the mechanics text. The code changes to meet it (T§6, T§7). The item's words change to name the channel for each receiver (T§13), because "appended to each subagent prompt" is not possible for an agent that zygo does not define (T-C2).

## 9. Text after a compaction (P3-15)

### 9.1 Claude Code main loop

Today: zygo registers `SessionStart` with no matcher (`crates/zygo-launch/src/claude_code.rs:510-513`), and the hook does not read `source` (`crates/zygo-hooks/src/session_start.rs:23-30`). So the hook gives the session text for each main-loop `SessionStart`, of any source. The Hooks reference documents the `compact` source ("`compact` | Auto or manual compaction"). It does not say whether the text from the start stays in the context through a compaction.

PT1 has two parts. **H**: the hook log shows a main-loop `SessionStart` (no `agent_id`) with `source: "compact"` after an automatic compaction, and its copy is in the next context. **R**: the first context after that compaction still holds the startup copy (T§12.2, "Reading the result").

| H | R | Behavior | Code change |
|---|---|---|---|
| — (pending) | — | The hook gives the text on every main-loop `SessionStart`. | none (today) |
| yes | no | The same. One copy. | none |
| yes | yes | The hook gives nothing on a main-loop `SessionStart` with `source: "compact"`. One copy. | `session_start.rs` reads `source` |
| no | yes | The text stays. One copy. | none |
| no | no | Fallback F1 (below). | F1 |

**F1.** `zygo hook pre-compact`, for an input with no `agent_id`, writes the empty file `$ZYGO_SESSION_DIR/session-text-due` (`0600`). The main-loop `UserPromptSubmit` hook `zygo hook prompt`, for a prompt that is not a `/zygo:` verb, prints the session text as `{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":<text>}}` when the file exists, then removes the file. Limit: after an automatic compaction in the middle of a turn, the rest of that turn runs without the text.

Resume, clear and fork are not changed. On `clear` the history is gone, so the text is there one time. On `resume` and `fork` the hook gives the text again; this spec does not check whether Claude Code also replays the earlier copy (T§11 states the cost).

### 9.2 Claude Code subagent

A `zygo-<role>` agent has the text in its system prompt, and a compaction keeps the system prompt, so nothing changes for it. For another subagent, the `SubagentStart` copy is in its messages, and the documentation says that auto-compaction discards it (T§6.2).

PT2: after the subagent's own automatic compaction, which zygo copy is in its context. **S**: the `SubagentStart` copy is there (Claude Code ran `SubagentStart` again, or kept the copy). **C**: the copy from the subagent `SessionStart` with `source: "compact"` is there.

| S | C | Behavior for a subagent `SessionStart` with `source: "compact"`, type not `zygo-` |
|---|---|---|
| — (pending) | — | Print `{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":<mechanics>}}`. |
| yes | any | Nothing. `SubagentStart` covers it. |
| no | yes | Print the mechanics text, as pending. |
| no | no | Nothing. Known limit: such a subagent loses the text at its own compaction. |

### 9.3 OpenCode

The transform runs per model request and writes into `output.system`, which is not in the message history. So a compaction cannot remove that text if OpenCode calls the transform on the first request after a compaction (PT3).

| PT3 | Behavior |
|---|---|
| verified, or pending | No change. |
| failed | Fallback F3: the plugin's `experimental.session.compacting` handler also calls `run("session-start", {input, output}, ["--event", "subagent"])` and pushes `Keep these rules word for word in the summary: ` followed by the output into `output.context`. The compacting input has a session id only, so the mechanics text, which fits every session, is the text used. |

## 10. Flows

| Flow | Text in context after this spec |
|---|---|
| Claude Code main loop, `startup` | Session text (`SessionStart`). |
| Claude Code main loop, `resume`, `fork` | Session text (`SessionStart`); a replayed earlier copy is possible (T§11). |
| Claude Code main loop, `clear` | Session text, one copy. |
| Claude Code main loop, compaction | T§9.1 table. |
| `zygo-<role>` subagent (start, resume, compaction) | Role prompt and mechanics text in the system prompt; the hook gives nothing. |
| Built-in `Explore`, `Plan`, `general-purpose`, `claude`, `statusline-setup`, `claude-code-guide` | Mechanics text from `SubagentStart`; after its compaction, T§9.2. |
| Plugin agent (`<plugin>:<name>`) | Same as the built-in agents. |
| User `--agents` entry, `.claude/agents/`, `~/.claude/agents/` | Same, unless its name starts with `zygo-` (T§6.4). |
| Nested subagent (a subagent's own Agent call) | Same rule by its type: `SubagentStart` runs for each spawn. |
| Agent-team teammate | `SubagentStart` per message; Claude Code adds the text only when its copy is not there (T§6.2). |
| Fork | Parent context (session text), and the mechanics text if `SubagentStart` runs for it. |
| `web-fetch` | No zygo text (T§6.4). |
| OpenCode main agent (`zygo/<main_role>`) | Session text, each request. |
| OpenCode `zygo-<role>` agent | Mechanics text from the transform (if PT4 holds); while PT4 is not verified, also in its prompt. |
| OpenCode `general`, `explore`, `plan` (when not main), user and plugin agents on a `zygo/` model | Mechanics text from the transform (if PT4 holds). |
| OpenCode title request (`small_model`) | Mechanics text, if the transform runs for it. |
| OpenCode agent on another provider | No zygo text; its requests do not pass the proxy, so no redaction marker. |

## 11. Cost in tokens

Sizes are bytes of the text; tokens are bytes ÷ 4, the rule of §10.7.

- Mechanics text, all parts on: 721 bytes, about 180 tokens. With `results` and `work_units` off and no navigation: 317 bytes, about 79 tokens.
- Session text of the default bundle (lead prompt, legend of six roles, alias and capabilities sentences, mechanics text; no user prompt, profile prompt or work item): 2,087 bytes, about 520 tokens.

| Case | Added input tokens |
|---|---|
| Claude Code subagent of another type | about 180 per dispatch, plus about 180 after each of its compactions in the "pending" and "no/yes" rows of T§9.2. |
| Claude Code `zygo-<role>` agent | 0. |
| Claude Code fork, if `SubagentStart` runs for it | about 180, a second copy, per fork. |
| Claude Code main loop, PT1 row "yes/yes" | Today about 520 per compaction (two copies); after the change 0. |
| Claude Code main loop, `resume` or `fork` with a replayed copy | about 520 until the next compaction or `clear`; not changed by this spec. |
| OpenCode request on another `zygo/` role | about 180 on each request. The text is the same on each request, so after the first request it is part of the cached prefix (§10.4). |
| OpenCode `zygo-<role>` request while PT4 is not verified | about 180 on each request (second copy, cached). |
| OpenCode title request | about 180 per title request, if the transform runs for it. |
| F1 | about 520 one time per compaction; no second copy. |
| F3 | about 180 in the compaction request; the summary keeps the rules. |

Hook run time: one more `zygo hook` process per Claude Code subagent dispatch, and per OpenCode subagent request. The main agent already pays this per request on OpenCode (§8.7).

## 12. Premises and the live check

### 12.1 Premises

| Id | Premise | Evidence so far | If verified | If failed |
|---|---|---|---|---|
| PT0 | `SubagentStart` `additionalContext` reaches the subagent's context (Claude Code). | Documented (T§6.2). | T§6.3 as written. | The `SubagentStart` entry and branch are removed; no subagent of another type gets the text on Claude Code; the limit is recorded in §8.6. |
| PT1 | After an automatic compaction, Claude Code sends a main-loop `SessionStart` with `source: "compact"` whose copy reaches the context (H), and the startup copy is or is not still in the context (R). | `compact` source documented; no main-loop fixture. | T§9.1 row for the measured H and R. | T§9.1 row for the measured H and R. |
| PT2 | After a subagent's automatic compaction, which of the `SubagentStart` copy (S) and the subagent `SessionStart` `compact` copy (C) is in its context. | `fixtures/claude-code/hooks/SessionStart-sub.json` (the event exists). | T§9.2 row for the measured S and C. | T§9.2 row for the measured S and C. |
| PT3 | OpenCode calls `experimental.chat.system.transform` on the first model request after a compaction. | The transform runs per request (`fixtures/opencode/plugin-hooks/experimental.chat.system.transform.json`); not observed after a compaction. | No change. | F3 (T§9.3). |
| PT4 | OpenCode calls the transform for a subagent request (the `task` tool), with the subagent's model in `input.model`. | Not observed. | `zygo-<role>` prompts lose the mechanics text (T§7.2 item 3). | The `--event subagent` branch is removed (T§7.2 item 3). |

The build uses the "pending" behavior of each table until `docs/premises.md` holds the row. A later row changes only the lines that its table names.

### 12.2 Live check P3-15 (operator, by hand)

Run on your own machine. It uses your normal Claude Code and OpenCode logins. It runs no zygo and no CI job; from this repository it uses only `scripts/premises/scrub.py`. It does not change your config files: the hooks are in a `--settings` file, the OpenCode plugin is in a scratch config folder, and Claude Code runs with no MCP servers. Spend: about 250,000 input tokens on Haiku in Claude Code, and three short turns in OpenCode.

**Claude Code (PT0, PT1, PT2)**

1. Make a scratch folder with ten files of about 22,000 tokens each, the probe hook and its settings file:

```sh
S=$(mktemp -d) && cd "$S" && git init -q
for f in a b c d e f g h i j; do yes 'the quick brown fox jumps over the lazy dog 0123456789' | head -n 1400 > $f.txt; done
cat > probe.py <<'EOF'
import json, sys
d = json.load(sys.stdin)
open(sys.argv[1], "a").write(json.dumps(d) + "\n")
ev = d.get("hook_event_name", "")
if ev in ("SessionStart", "SubagentStart"):
    who = "sub" if "agent_id" in d else "main"
    tag = "CANARY-%s-%s-%s" % (ev, d.get("source", "na"), who)
    print(json.dumps({"hookSpecificOutput": {"hookEventName": ev, "additionalContext": tag}}))
EOF
H="python3 -I $S/probe.py $S/hooks.jsonl"
E='[{"hooks":[{"type":"command","command":"'"$H"'"}]}]'
printf '{"hooks":{"SessionStart":%s,"SubagentStart":%s,"PreCompact":%s,"PostCompact":%s}}\n' "$E" "$E" "$E" "$E" > settings.json
echo "$S"
```

2. Start Claude Code with automatic compaction at 70,000 tokens:

```sh
CLAUDE_CODE_AUTO_COMPACT_WINDOW=100000 CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=70 claude --model haiku --strict-mcp-config --settings "$S/settings.json"
```

3. Type `/context`. If the used tokens are above 40,000, type `/exit` and start again with `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=85`.
4. Type: `Use the general-purpose subagent with this task: list every line of your context that starts with CANARY-, word for word, one per line, or NONE. Show me its answer word for word.` (PT0.)
5. Type: `Read a.txt, b.txt, c.txt, d.txt and e.txt in full with the Read tool, one after another, then reply DONE.` Wait for the turn to end. Run `grep -c '"trigger": "auto"' "$S/hooks.jsonl"` in another terminal: it must be 2 or more (a `PreCompact` and a `PostCompact` line). If it is 0, read one more file the same way.
6. Type: `List every line of your context that starts with CANARY-, word for word, one per line, or NONE.` (PT1.)
7. Type: `Use the general-purpose subagent with this task: read f.txt, g.txt, h.txt, i.txt and j.txt in full with the Read tool, one after another; then list every line of your context that starts with CANARY-, word for word, one per line, or NONE, and return only that list. Show me its answer word for word.` (PT2.)
8. Type `/exit`. Copy the replies of steps 4, 6 and 7, each under a line `== step N`, into the file `$S/replies.txt`.
9. Save the main transcript lines after the last compaction boundary:

```sh
T=$(head -n 1 "$S/hooks.jsonl" | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["transcript_path"])')
awk '/compact_boundary/{n=NR} {l[NR]=$0} END{for(i=n+1;i<=NR;i++) print l[i]}' "$T" | grep -o 'CANARY-[A-Za-z]*-[a-z]*-[a-z]*' > "$S/transcript-after.txt"
cat "$S/transcript-after.txt"
```

**OpenCode (PT3, PT4)**

10. Pick two different model ids of one OpenCode provider, `MAIN` and `SUB` (for example a Sonnet id and a Haiku id). Make the probe plugin and start OpenCode in the scratch folder:

```sh
mkdir -p "$S/oc/plugins" && cat > "$S/oc/plugins/probe.js" <<'EOF'
import { appendFileSync } from "node:fs";
const put = (o) => { try { appendFileSync(process.env.P315_LOG, JSON.stringify({ t: Date.now(), ...o }) + "\n"); } catch {} };
export const P315 = async () => ({
  "experimental.chat.system.transform": async (input) => put({ hook: "transform", model: input?.model?.id, keys: Object.keys(input ?? {}).sort() }),
  "experimental.session.compacting": async (input) => put({ hook: "compacting", keys: Object.keys(input ?? {}).sort() }),
});
EOF
cd "$S" && P315_LOG="$S/oc.jsonl" OPENCODE_CONFIG_DIR="$S/oc" OPENCODE_CONFIG_CONTENT='{"agent":{"general":{"model":"<provider>/<SUB>"}}}' opencode -m <provider>/<MAIN>
```

11. Type: `Use the general subagent to read a.txt and tell me its first line.` Then type `/compact`. Then type `Reply OK.` Then exit OpenCode.

**Capture**

12. From the checkout of this repository:

```sh
python3 -I scripts/premises/scrub.py "$S" "$S/hooks.jsonl" > fixtures/claude-code/live/p3-15-hooks.jsonl
python3 -I scripts/premises/scrub.py "$S" "$S/replies.txt" > fixtures/claude-code/live/p3-15-replies.txt
cp "$S/transcript-after.txt" fixtures/claude-code/live/p3-15-transcript-after.txt
python3 -I scripts/premises/scrub.py "$S" "$S/oc.jsonl" > fixtures/opencode/live/p3-15-transform.jsonl
claude --version; opencode --version
```

Send the four files and the two version lines to the lead. Then `rm -rf "$S"`.

**Reading the result.** The lead writes the rows into `docs/premises.md` and a `P3-15` section with this procedure into `docs/live-checks.md`.

- PT0 verified: the step 4 reply holds `CANARY-SubagentStart-na-sub`.
- PT1 H "yes": `p3-15-hooks.jsonl` holds a `SessionStart` line with `"source": "compact"` and no `agent_id` after the first main `PreCompact` line, and the step 6 reply holds `CANARY-SessionStart-compact-main`. Else "no".
- PT1 R "yes": both the step 6 reply and `p3-15-transcript-after.txt` hold `CANARY-SessionStart-startup-main`. Else "no". If `transcript-after.txt` is empty because the transcript has no `compact_boundary` line, R is read from the step 6 reply alone, and the row says so.
- PT2: decided only when `p3-15-hooks.jsonl` holds a `PreCompact` line with an `agent_id`; else PT2 stays pending. S "yes": the step 7 reply holds `CANARY-SubagentStart-na-sub`. C "yes": it holds `CANARY-SessionStart-compact-sub`.
- PT3 verified: `p3-15-transform.jsonl` holds a `transform` line with model `MAIN` after the `compacting` line.
- PT4 verified: it holds a `transform` line with model `SUB`.
- The `keys` lists are recorded in the rows; they change no behavior in this spec.

## 13. Base-spec changes

- **§8.5, hooks list:** add "`SubagentStart` → `zygo hook session-start` (every launch; for a member, before `workspace --event subagent-start`)".
- **§8.6 item 7**, the parenthesis becomes: "(also given to each subagent: in the `--agents` prompt of each Claude Code `zygo-<role>` agent, built at launch; by `zygo hook session-start` on `SubagentStart` to every other Claude Code subagent whose type does not start with `zygo-`; on OpenCode by the system-prompt transform on each request whose model is a `zygo/` role other than the main role, T§7)".
- **§8.6, new last paragraph:** "After a compaction: `docs/specs/2026-11-24-session-text-design.md` T§9 and premises PT1 to PT3."
- **§8.7 note (N33):** "so subagents and the title agent do not get it" becomes "so subagents and the title agent get the mechanics text only (T§7)".
- After the PT4 row: §8.7 says whether the OpenCode `zygo-<role>` prompt holds the mechanics text.

## 14. Failure handling

- The session view cannot be loaded: as `run_in` today (`crates/zygo-hooks/src/session_start.rs:28`); the error goes to the C6 wrapper (`crates/zygo-core/src/hook.rs:75-76`), which prints nothing and writes one stderr line. The subagent starts with no zygo text.
- The OpenCode hook call fails or passes its 10 s bound: the plugin's `run` gives `""` (`crates/zygo-launch/assets/opencode-plugin.js:12-15, 27-28`), and nothing is pushed.
- F1: a failed write or remove of `session-text-due` is a C6 internal error of that hook; a file left behind gives the text one more time at the next prompt.

## 15. Alternatives rejected

1. **The hook as the one owner for `zygo-<role>` agents too** (remove the text from the Claude Code `--agents` prompt). The hook text would follow the overrides, but a `zygo-<role>` agent would then lose the text at its own compaction unless PT2 holds. The gain is small (T§8.4). Today's system-prompt copy survives a compaction with no premise.
2. **Append the text to user `--agents` entries and to plugin agent prompts at launch.** It breaks T-C2, and it cannot reach the built-in agents or the agents in `.claude/agents/`, which Claude Code reads itself.
3. **Replace a built-in agent with a `--agents` entry of the same name** (for example `Explore`). That replaces Claude Code's built-in prompt and tool rules (Subagents: "A user or project subagent named `Explore` overrides the built-in").
4. **`PreToolUse` on the agent tool, with `updatedInput` that adds the text to the task prompt.** It changes the brief that the lead wrote, and the copy is lost at the subagent's compaction as the `SubagentStart` copy is. `SubagentStart` is the documented channel, and Claude Code removes its duplicates (T§6.2).
5. **Set `prompt` on the OpenCode built-in agents.** It replaces their built-in prompt.
6. **Give the session text on `PostCompact`.** `PostCompact` has no decision control and no context channel (Hooks reference, PostCompact).
7. **Read `source` and skip `compact` now, before PT1.** If the startup copy is lost, the main loop then runs with no session text. Today's behavior can only cost a second copy.
8. **A deny or notify on other agent types.** T§8.2.
9. **A live check through zygo or the mock harness.** The mock answers with authored replies, so it cannot show what the real model sees after a real compaction; a run through zygo would test zygo's own text, which is the subject of the change. The probe hook gives one canary for each event and source.

## 16. Files expected to change

| File | Change |
|---|---|
| `crates/zygo-hooks/src/session_start.rs` | The subagent branch (T§6.3 item 2); the `--event subagent` branch (T§7.2 item 2); the `source` rule if PT1 is "yes/yes". |
| `crates/zygo-launch/src/claude_code.rs` | `SubagentStart` entry in `hooks` (T§6.3 item 1). |
| `crates/zygo-launch/assets/opencode-plugin.js` | Transform branch (T§7.2 item 1); F3 only if PT3 fails. |
| `crates/zygo-launch/src/opencode.rs` | Only when PT4 is verified: the `zygo-<role>` prompt is the role prompt only. |
| `crates/zygo-hooks/src/pre_compact.rs`, `crates/zygo-hooks/src/prompt.rs` | Only for F1. |
| `crates/zygo-hooks/tests/hooks_basic.rs` (or a new test file), `crates/zygo-launch/tests/claude_code.rs`, `crates/zygo-launch/tests/opencode.rs` | Tests of T§17. |
| `fixtures/claude-code/live/p3-15-*`, `fixtures/opencode/live/p3-15-transform.jsonl` | From the operator (T§12.2). |
| `fixtures/claude-code/hooks/SessionStart-compact-main.json` | The main `SessionStart` `compact` line of `p3-15-hooks.jsonl`, when that line exists. |
| `docs/premises.md`, `docs/live-checks.md` | PT0 to PT4 rows; the P3-15 section with T§12.2. |
| `docs/specs/2026-11-18-zygo-design.md` | T§13. |

## 17. Acceptance

Each check is decided by a test with the stub agents and recorded fixtures, or by a command.

1. The settings of a plain launch hold `SubagentStart` = one entry, command `<zygo> hook session-start`, no matcher. A member launch holds two entries in the order `session-start`, `workspace --event subagent-start`.
2. `session-start` with `fixtures/claude-code/hooks/SubagentStart-sub.json` (type `claude`), a view with results, navigation and units on, and no overrides prints exactly `{"hookSpecificOutput":{"hookEventName":"SubagentStart","additionalContext":M}}`, where M is `mechanics` of that view (721 bytes).
3. The same input with `agent_type` set to `Explore`, `Plan`, `general-purpose`, `act101:code-explorer`, the number `1`, or with no `agent_type`, prints the same. With `agent_type` `zygo-code` it prints nothing.
4. With `overrides.json` `{"results": false, "work_units": false}` and navigation off, M is the redaction sentence, one space, the attribution sentence.
5. `session-start` with `fixtures/claude-code/hooks/SessionStart-sub.json` prints the T§9.2 row that `docs/premises.md` names for PT2 (pending: the mechanics text with `hookEventName` `SessionStart`). With `agent_type` `zygo-read`, or with `source` `startup` or `resume`, it prints nothing.
6. A main-loop `SessionStart` input with `source: "compact"` (`SessionStart-compact-main.json` when it exists, else `SessionStart-main.json` with `source` changed, named as a constructed input in the test) gives the T§9.1 row that `docs/premises.md` names for PT1 (pending: the same output as `source: "startup"`).
7. The Claude Code `--agents` value of a default launch still has each `zygo-<role>` prompt equal to the role prompt, one space, and M.
8. The generated plugin, run under `node` with a stub `zygo` that logs its arguments (the method of `crates/zygo-launch/tests/opencode.rs:703-707`, whose `code/zygo` and `read/zygo` cases change): with `ZYGO_MAIN_ROLE=lead`, a transform call with model `{providerID: "zygo", id: "read"}` calls `hook session-start --agent opencode --event subagent` and pushes the stub's output into `output.system`; with `id: "lead"` it calls `hook session-start --agent opencode` with no `--event`; with `providerID: "anthropic"` it calls nothing.
9. `zygo hook session-start --agent opencode --event subagent` in a member session view that holds resume note inputs prints M as plain text, and `view.json` is byte-equal before and after.
10. `session-start` with a `SubagentStart` input and a `view.json` that does not parse exits 0, prints nothing on stdout and one line on stderr.
11. `scripts/check-premises.sh` passes with the PT0 to PT4 rows and their evidence files.
12. Only when the matching premise result is recorded: F1 (a main `PreCompact` writes `session-text-due`; the next prompt that is not a verb prints the session text as `UserPromptSubmit` `additionalContext`, and the file is gone; a `/zygo:` verb prompt leaves the file); F3 (the compacting handler pushes the rule line and M); PT4 verified (the OpenCode `zygo-<role>` prompt equals the role prompt); PT0 failed (no `SubagentStart` entry).
13. No test added by this work runs a real `claude` or `opencode` binary or reads a wall clock (review of the diff).

## 18. Left to the builder

- The function names and the place of the subagent branch inside `session_start.rs`.
- The test file layout.
- The exact words of the §8.6 and §8.7 edits, inside the meaning of T§13.
