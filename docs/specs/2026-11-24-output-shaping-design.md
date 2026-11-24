# zygo: output shaping and saving accounting — design

**Status:** proposed 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, output track (roadmap items P3-5, P3-6 and P3-7, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`; C6, §10.1, §10.7, §14). Also cited: `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`, decisions `OPn`/`AUn` of that spec). `O§n` is a section of this document. The build follows `docs/specs/2026-11-24-decision-records-design.md`; this spec does not repeat it.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's "Intent" output-track paragraphs, the P3-5 to P3-7 item rows, the "P3-5 design notes", the "P3-6 design notes" and the "Plan of work (operator, 2026-11-24)"). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767` (branch `phase-3`).

---

## 1. Intent

The trim hook decreases the tokens that tool output puts in the context. Today it acts on size only, and the figure that it reports is not the real saving.

The gap today, from the code:

1. The hook acts only on `PostToolUse` (`crates/zygo-hooks/src/trim.rs:86`) and only above the threshold (`trim.rs:103-106`, default 16384 bytes). Bash output below it (git, test runners, linters) goes to the model in full.
2. For Bash it reads `stdout` only (`trim.rs:42-47`). It does not read the command (`tool_input.command`), and it does not measure or store `stderr`, although the hook input has both (`fixtures/claude-code/hooks/PostToolUse-Bash-main.json:12-21`).
3. Kind sniffing (`crates/zygo-results/src/kind.rs:51-83`) is used by `result_tree` only (`kind.rs:89-97`). The view is always the first 60 and the last 20 lines (`crates/zygo-results/src/view.rs:24-43`).
4. The saving figure is `SUM(stored_bytes − view_bytes)` (`crates/zygo-usage/src/query.rs:183-187`), printed ÷ 4 (`crates/zygo/src/cmd/usage.rs:104`). It counts a trimmed byte one time. The model pays for that byte again on each later request of the same context. It also does not subtract the replies of `result_view`, `result_search` and `result_tree`, which put text back into the context.
5. The `trims` table has `tool` and `kind` (`crates/zygo-usage/src/schema.rs:35-43`); `zygo usage` prints neither.
6. No benchmark and no measured hook latency exist.

The direction **(operator)**: zygo shapes output in the `PostToolUse` trim hook, after the tool ran, with the command from the hook input. It does not rewrite commands. It keeps the raw output in the result store for each shaped view. It reports two saving figures, net of read-backs, counted per context.

## 2. Goals

- **O-G1** Bash output below the threshold gets shorter when it holds noise: color codes, progress lines, repeated lines, passed tests.
- **O-G2** The full raw output is always recoverable by the result id in the view.
- **O-G3** `zygo usage` reports "tokens kept out of context" (one time) and "estimated spend avoided" (carried), both net of read-backs, with a breakdown by tool and command and a read-back count per view.
- **O-G4** A shaped view is never larger than the text it replaces, and a shaper fault never costs the agent its output.
- **O-G5** Each shaper has fixtures from real command output and a minimum saving that a CI test checks.

## 3. Non-goals

- A rewrite of the command before it runs (`PreToolUse`) **(operator)**.
- User-defined filter files **(operator)**; also a scan of past transcripts for missed savings (roadmap "Not in phase 3").
- Shaping of `Read`, `Grep`, `Glob` and `WebFetch` output. They keep the size view of §10.1 (O§7.1).
- A number for the later compaction that a smaller context causes (O§6.8) **(operator)**.
- A spend figure in the status line (O§6.7).
- Runners other than the three of O§7.6 in this phase.

## 4. Constraints

- **O-C1** C6 applies to each new branch: a shaper fault, a store fault or a usage-store fault never changes the agent's output for the worse. A fault gives the size view of §10.1, or no change.
- **O-C2** §14 applies: the usage store holds no tool input and no tool output. The command is stored only as a name from the closed list of O§7.2.
- **O-C3** The raw output is the one exception to C3 that §10.1 already allows: it lives only in the session's results directory, mode `0600`.
- **O-C4** A view is always strictly smaller than the raw text it replaces, by at least `MIN_SAVING_BYTES` (O§7.8). Else the hook changes nothing.
- **O-C5** Price multipliers come from the catalog price rows (§6.3), never from constants in code **(operator)**.
- **O-C6** CI tests are deterministic. No CI test measures wall-clock time or has a deadline. The latency budget is a measurement run on demand, outside CI (O§8.5) **(operator, project rule)**.
- **O-C7** Tests never run a real `claude` or `opencode` binary. Hook tests feed hook-input JSON to the hook; launcher tests use the stub agents.
- **O-C8** Each new stored field is additive and readable by older and newer binaries without a schema version change (O§6.5).

## 5. Terms

- **Context.** One conversation that the model sees: the main loop of a zygo session, or one subagent. Key: `(session, agent_id)`, with `agent_id` null for the main loop.
- **Event.** Something that changes the context size: a **trim** (a shaped or size view replaced the raw output) or a **read-back** (a reply of `result_view`, `result_search` or `result_tree`).
- **Carrying request.** A request of the same context, at or after the event, before the context is cut (O§6.3).
- **Shaper.** One step of the pipeline of O§7.3. **Runner view.** A shaper for one test runner.
- **Raw text.** For Bash: the composition of O§7.2 of `stdout` and `stderr`. For other tools: the one text field that §10.1 measures.

## 6. Saving accounting (P3-5)

### 6.1 The open point: is `agent_id` filled?

**(author, from the code)** Yes, for Claude Code. No change is needed.

- A request row takes `agent_id` from the `x-claude-code-agent-id` header (`crates/zygo-proxy/src/forward.rs:291`).
- A trim row takes it from the hook input (`trim.rs:121`, read at `crates/zygo-core/src/hook.rs:36`).
- Premise P3 is verified on Claude Code 2.1.328 (`docs/premises.md:27`): each request and each hook input of a subagent has the same id; no main-loop request has the header and no main-loop hook input has `agent_id`. Q5 (`docs/premises.md:36`) adds that a subagent's compaction request has the subagent's id.
- OpenCode has no agent id (§13.6). O§6.3 handles it.

### 6.2 The two figures

**(operator)** Two figures, kept apart:

1. **Tokens kept out of context** (one time). `net_bytes = Σ max(stored_bytes − view_bytes, 0) over trims − Σ bytes over read-backs`. Tokens = `sign(net_bytes) × ceil(|net_bytes| ÷ BYTES_PER_TOKEN)` (`crates/zygo-core/src/tokens.rs:4`, value 4). This figure can be negative. It is the only figure that is shown as a token count.
2. **Estimated spend avoided** (carried). USD, always with the `≈` mark, computed by O§6.3. Multiplied tokens are never shown as a context-size figure.

### 6.3 The carry formula

**(operator)** for the factor; **(author)** for the rules that make it computable from the stored rows.

For one event `e` with `T_e = bytes ÷ BYTES_PER_TOKEN` tokens (a real number; for a trim, `bytes = max(stored_bytes − view_bytes, 0)`; for a read-back, the reply bytes) at time `ts_e`, in context `C`:

1. **The context's requests `Q_C`.** The `requests` rows of the session with a 2xx `status`, ordered by `ts`, then `rowid`:
   - main loop (`agent_id` null): `agent_id IS NULL` and `request_class` is null, `main` or `compaction`. `auxiliary` rows (title, suggestions, WebFetch's second call; Q5) do not carry the context and are left out.
   - subagent: `agent_id = C.agent_id`, any class.
2. **The carrying requests `R_e`.** From the first row of `Q_C` with `ts ≥ ts_e`, take rows in order. Stop before a row whose prompt is less than half the prompt of the row before it (a `/clear`, a compaction by any means, or a context edit cut the context). Stop after a row with `compaction = 1` (that request still sends the old text one time). Prompt = `input_tokens + cache_read_tokens + cache_write_tokens` (as `crates/zygo-proxy/src/window.rs:89-91`).
3. **Rate of each carrying request**, in USD per token, from the price row of the request's `model` in the catalog, with the tier of the request's prompt (the rule of `Prices::cost`, `crates/zygo-config/src/schema.rs:280-289`):
   - the first row of `R_e`: the write rate `cache_write_5m` (absent: `cache_write`). The event's text is new in the prompt, so the request writes it. A row with `cache_read_tokens = 0` and `cache_write_tokens = 0` (no caching) takes `input`.
   - each later row: `input` when it has no cache tokens; the write rate when `cache_write_tokens > cache_read_tokens` (the cache expired and the prefix was written again); else `cache_read`.
4. `spend_e = T_e × Σ rates of R_e`. A row whose model has no price row adds 0 and is counted in `unpriced` (O§6.6).
5. **Estimated spend avoided** = `Σ spend_e over trims − Σ spend_e over read-backs`.

This is the operator's `S × (w + r × N)` with `w` and `r` taken per request from the catalog, and with `w` in place of `r` for each request after a cache expiry.

Rules **(author)**, each with the failure it prevents:

- **The 5-minute write rate.** The `requests` row keeps one cache-write total, not the 5-minute/1-hour split (§14, HUD AU33). The lower rate under-states the figure rather than over-states it.
- **The half-prompt cut.** No stored row marks a `/clear` or zygo's own context cut. Without the cut, a trim before a `/clear` is carried for the rest of the session.
- **`ts ≥ ts_e`.** Timestamps are Unix seconds. The request that called the tool starts before the tool runs, and its stream takes time, so its `ts` is almost always below `ts_e`. When it is in the same second, the figure counts one extra request of that context. The error is at most one request per event.
- **OpenCode.** Its requests and trims have no agent id, so its subagent requests mix with the main loop. For a session whose `requests.agent` is `opencode`, `R_e` is its first row only. The figure is then the one-time write. This under-states it; it never carries a subagent's trim on the main loop's requests.
- **Prices.** `zygo usage` takes the catalog of the merged user config (built-in rows included). This is the catalog that priced `estimated_usd` at write time. A team session's rows carry `estimated_usd = 0` and no provider (§14 note N16); the catalog row of the model still prices them. Passthrough rows (`estimated_usd = 0`) are priced the same way: the figure is notional, as the `≈` says.

Worked example (test catalog row: `input` 4, `cache_read` 0.4, `cache_write_5m` 5, `cache_write` 8 USD per Mtok). A trim keeps 40,000 bytes = 10,000 tokens out of the main loop. Then: request 1 writes (5); requests 2-5 read (4 × 0.4); request 6 follows a cache expiry and writes (5); request 7 is the compaction and reads (0.4). Sum of rates = 12 USD per Mtok. `spend_e = 10,000 × 12 ÷ 10^6 = $0.12`. A read-back of 4,000 bytes (1,000 tokens) after request 5 is carried by request 6 (its first row: the write rate, 5) and request 7 (a later row that reads: 0.4): `1,000 × 5.4 ÷ 10^6 = $0.0054`. Net: `≈$0.1146`. Tokens kept out: `ceil((40,000 − 4,000) ÷ 4) = 9,000`.

### 6.4 Read-backs

**(author)** The MCP server records each successful reply of `result_view`, `result_search` and `result_tree` (`crates/zygo-mcp/src/tools/results.rs:44-51`, `:66-70`, `:85-88`) as one `readbacks` row.

- Who writes: the MCP tool, after it builds the reply. The MCP request has no agent id, so the row has none. Its context comes from the trim of the same `result_id` (O§6.5): `(readbacks.session, trims.agent_id)`. A read-back of a result with no trim row (its row was not written, or it was pruned) counts in the main loop of its session.
- Why the MCP server and not a `PostToolUse` hook on the MCP tools: the server serves Claude Code and OpenCode the same way, and no premise about MCP hook input shapes is needed.
- A workspace member resumed in a new session reads results of the old session (§10.1, "Workspace members"). The row has the new session, which is the session whose context holds the reply.
- A read-back from a context other than the trim's (a lead passes an id to a subagent) is counted in the trim's context. This is accepted: the id is in the marker of the trim's context, and a pass of an id to another context is rare.
- The server opens the usage store once, at its first read-back, and keeps it (the store's open can wait up to 5 s on a busy database, `crates/zygo-usage/src/store.rs:80-111`). A write fault: the reply is returned unchanged, and the server writes one fixed stderr line (O-C1).
- An error reply (bad id, bad range, bad pattern) writes no row: it is short and is not a read-back of stored text.

### 6.5 Store changes

**(author)** All changes are additive (O-C8). `SCHEMA_VERSION` (`store.rs:62`) does not change, so `open_read_only` (`store.rs:119-132`) still opens the database for each binary.

1. New table, created by the `CREATE TABLE IF NOT EXISTS` schema text (as `upstream_errors`, `schema.rs:67-77`):
   ```sql
   CREATE TABLE IF NOT EXISTS readbacks (
       ts INTEGER NOT NULL,
       session TEXT NOT NULL,
       result_id TEXT NOT NULL,
       tool TEXT NOT NULL,      -- result_view | result_search | result_tree
       bytes INTEGER NOT NULL
   );
   CREATE INDEX IF NOT EXISTS readbacks_session ON readbacks(session);
   CREATE INDEX IF NOT EXISTS readbacks_ts ON readbacks(ts);
   ```
2. Three nullable columns on `trims`: `result_id TEXT`, `command TEXT`, `shaper TEXT`. `Store::open` adds each column that `PRAGMA table_info(trims)` does not list, with `ALTER TABLE trims ADD COLUMN`. A "duplicate column name" error from a concurrent open is success. `open_read_only` adds nothing.
3. `TrimRow` gets `result_id`, `command` and `shaper` as `Option<String>`, with `#[serde(default)]`. `insert_trim` (`store.rs:232-245`) writes them. An older binary's insert names its own seven columns, so the new columns are null in its rows.
4. A reader checks `PRAGMA table_info(trims)` and `sqlite_master` first. A missing column reads as null; a missing `readbacks` table reads as no rows. So a new `zygo usage` on a database that only an older binary has opened works.
5. `prune` (`store.rs:272-284`) also deletes `readbacks` rows older than the cutoff. `replace_requests` (`zygo usage --sync`) does not touch `trims` or `readbacks`.
6. `Store::bytes_read_back(session)`: `SELECT COALESCE(SUM(bytes), 0) FROM readbacks WHERE session = ?1`, on the `readbacks_session` index (as `bytes_trimmed`, `query.rs:209-212`).

### 6.6 `zygo usage`

**(author)** The current lines stay as they are (H§14 item 18 checks them). `bytes trimmed` now includes shaped views below the threshold. After `tokens trimmed (estimate)` (`usage.rs:104`), new lines:

```
bytes read back: 4000
tokens kept out of context (estimate): 9000
estimated spend avoided: ≈$0.1146
views: 14, read-backs: 3 (0.21 per view)
by tool and command:
  Bash cargo libtest: 6 views, 2 read-backs, 31200 bytes kept out, ≈$0.0900
  Bash git diff: 3 views, 0 read-backs, 9100 bytes kept out, ≈$0.0210
  Read - size: 5 views, 1 read-back, -4300 bytes kept out, ≈-$0.0040
```

- `views` = trim rows; `read-backs` = read-back rows; the ratio is read-backs ÷ views, two decimals, `-` when views is 0. A high ratio shows that a view hides text the agent needs.
- A breakdown key is `tool`, `command` (`-` when null), `shaper` (`-` when null). A read-back takes the key of its trim (by `result_id`), else `- - -`. Rows are sorted by bytes kept out, largest first.
- A negative amount prints the sign before `$`: `≈-$0.0040`.
- When some carrying requests had no price row: `estimated spend avoided: ≈$0.1146 (12 requests had no price)`.
- When the config does not load: `estimated spend avoided: unknown (config does not load)`, and the other lines print.
- The filter (`--session`, `--since`) selects events by their own `ts` and session. The carrying requests of a selected event are read even when they are after a `--since` boundary of a later day: the figure belongs to the event.
- Every name passes `esc` as today (`usage.rs:112-116`).

### 6.7 Status line and `zygo status`

**(author)** The status line keeps its trimmed segment (H§6.2 row 14) and its form (`✂ 23k`, `trim 23k`, `trimmed 23k tok`). Its number becomes the net one-time figure: `ceil(max(bytes_trimmed − bytes_read_back, 0) ÷ 4)`. The hook runs two indexed sums (`crates/zygo-hooks/src/statusline/data.rs:550-552` plus `bytes_read_back`), both in the H§7.7 deadline. `zygo status` (`crates/zygo/src/cmd/status.rs:67-69`) and `zygo_hooks::statusline::trimmed` (`crates/zygo-hooks/src/statusline/mod.rs:55-58`) use the same net number.

The spend figure is not in the status line:

- It needs a join over the session's requests on each status call. The H§7.7 deadline is for one indexed read per source.
- The status line already shows the session spend (H§6.2). A second dollar figure next to it reads as a part of it.
- The HUD has a fixed segment list (HUD OP4).

### 6.8 Not counted

**(operator)** A smaller context reaches the soft window (§10.2) later, so it compacts less often. Each compaction avoided saves a summary request and a cache write of the new context. zygo does not put a number on this. `zygo usage` does not print it. This spec states it here only.

## 7. Output shaping (P3-6)

### 7.1 Where shaping runs

**(operator)** In the `PostToolUse` trim hook, on the finished output, with the command from the hook input. No command is rewritten.

**(author)** Shaping runs for the Bash tool only, on both agents. `Read`, `Grep`, `Glob` and `WebFetch` keep the size view of §10.1. Failure prevented: an agent copies an `old_string` for `Edit` from a view, and the view changed the file text.

For the same reason a Bash call is not shaped when the first program of its last segment (O§7.2) is a file printer: `cat`, `head`, `tail`, `sed`, `awk`, `nl`, `bat`, `less`, `more`, `grep`, `rg`, `git show`, `git cat-file`. It keeps the size view. A file printer later in a pipeline (`cargo test 2>&1 | tail -n 80`) reads another program's output, so that call is shaped.

### 7.2 Input

**(author)**

- **Command.** `tool_input.command` when it is a string; else no command. Only its first 4 KiB is read. A shell-word split (single quotes, double quotes, backslash; an unclosed quote takes the rest as one word; it never fails) cuts it into segments at `&&`, `||`, `;` and newline, and each segment into pipeline stages at `|`. In a stage, leading `NAME=value` words and the wrappers `env`, `time`, `sudo`, `nice`, `command` are skipped. A program word is reduced to its base name.
- **Command name (stored).** The base name of the first program of the last segment, when it is in this closed list: `cargo`, `go`, `pytest`, `python`, `python3`, `uv`, `npm`, `pnpm`, `yarn`, `npx`, `node`, `make`, `just`, `git`, `gh`, `docker`, `kubectl`, `terraform`, `ls`, `find`, `tree`, `du`, `ps`, `cat`, `head`, `tail`, `sed`, `awk`, `grep`, `rg`, `jq`, `curl`. For `git`, `cargo`, `go`, `npm`, `pnpm`, `yarn`, `docker`, `kubectl` and `gh`, the next word is added when it is in that program's built-in subcommand list (for example `git diff`, `cargo test`); the builder writes these lists. Else `other`. Nothing else of the command is stored (O-C2).
- **Raw text (Bash, Claude Code).** `stdout`; when `stderr` is not empty, then a newline if `stdout` does not end in one and is not empty, the line `[stderr]`, and `stderr`. The size test, the stored raw and `stored_bytes` all use this text. Thus stderr is measured and stored in each case, shaped or not.
- **Raw text (Bash, OpenCode).** The plugin's `output.output` (`crates/zygo-core/src/hook_opencode.rs:42-49`), which holds both streams (premise PO3).
- Only `isImage = false` output is read, as today (`trim.rs:43-45`).

### 7.3 The pipeline

**(author)** For a Bash call with `[results] shape = true` (O§9), a raw text of at most `SHAPE_MAX_BYTES` (4 MiB), and no file printer (O§7.1):

1. **Clean** (O§7.4): color codes and progress.
2. **Dedup** (O§7.5): runs of repeated lines.
3. **Runner view** (O§7.6), when the command names a runner and the text has that runner's summary.
4. **Kind view** (O§7.7), only when the text after steps 1-3 is still over the threshold.
5. **Size view** (§10.1 head and tail), when the text after step 4 is still over the threshold.

Each step takes text and gives text or "no change". No step adds content: it removes lines or characters, or adds a count or a fixed note. Then O§7.8 decides if the view is used.

Above `SHAPE_MAX_BYTES`, only step 5 runs, as today. Failure prevented: a hook that runs for seconds on a huge output; the OpenCode plugin hook has a 10 s timeout (§8.7). The bound is in bytes, not time, so the result does not depend on machine speed.

Build order **(operator)**: steps 1-2, then step 4, then step 3. The work graph orders the nodes so.

### 7.4 Clean

- Remove ANSI escape sequences: CSI (`ESC [`, parameter bytes `0x30-0x3F`, intermediate bytes `0x20-0x2F`, one final byte `0x40-0x7E`), the 8-bit CSI `U+009B` with the same tail, OSC (`ESC ]` to `BEL` or `ESC \`; an OSC of more than 4 KiB with no terminator: remove only `ESC ]`), and any other `ESC` plus its next character. A lone `ESC` at the end is removed. OSC 8 hyperlinks keep their visible text.
- `\r\n` becomes `\n`. In each line, a `\r` overwrites: only the text after the last `\r` is kept. This removes progress bars and spinners.
- Lines that hold only a progress form after this are removed: cargo `Building [====>   ] 12/80`, and percent-only lines (`^\s*\d{1,3}%\s*$`).

### 7.5 Dedup

- A run of 3 or more equal consecutive lines becomes the first line plus ` [repeated <n> times]`, where `<n>` is the run length. Blank lines are not counted as repeats.
- When the sniffed kind of the text (`kind.rs:51`) is `log`, two lines are equal when they are equal after their leading timestamp (the `LOG_TIME` form, `kind.rs:35`) is removed. The kept line keeps its own timestamp.
- Dedup is lossless up to the timestamps: expanding each count gives the cleaned text again (O§16 item 5).
- Only consecutive lines. Failure prevented: a dedup over the whole text changes the order of output, and the model reads a wrong sequence of events.

### 7.6 Runner views

**(author)** The first runners: `cargo test` (libtest output), `pytest` and `go test`. Others (`cargo nextest`, `jest`, `vitest`, `mocha`, `rspec`) wait until these three are measured (O§13).

A runner view is tried when a segment of the command names the runner, and it is used only when the text has the runner's summary. The command picks the parser; the output confirms it.

| Runner | Command forms | Summary that confirms | Kept | Hidden (with a count line) |
|---|---|---|---|---|
| libtest | `cargo test`, `cargo +<toolchain> test`; a wrapper `make test`, `just test`, `npm test`, `pnpm test`, `yarn test` tries all three runners in this table's order | at least one line `^test result: (ok\|FAILED)\. \d+ passed; \d+ failed;` | `running <n> tests`; `test <name> ... FAILED`; each `---- <name> stdout ----` block to the next `----` header or `failures:` line; the `failures:` name list; each `test result:` line; each cargo `Running <target>` and `Doc-tests <crate>` line; each `error` and `warning` line and the block that follows it to the next blank line | `test <name> ... ok` and `... ignored` lines (one line per test binary: `[zygo: <p> passed and <i> ignored test lines hidden]`); cargo progress lines `Compiling`, `Checking`, `Fresh`, `Downloaded`, `Downloading`, `Locking`, `Updating`, `Blocking`, `Finished` (one line: `[zygo: <n> build progress lines hidden]`) |
| pytest | `pytest`, `py.test`, `python -m pytest`, `python3 -m pytest`, `uv run pytest`, `poetry run pytest`; wrappers as above | a last summary line `^=+ .*\b(passed\|failed\|error\|errors\|skipped)\b.* in [\d.]+s.* =+$` | every line except the hidden ones; thus the `FAILURES`, `ERRORS` and `short test summary info` sections stay whole | progress lines `^\S+\.py [.sxXfFE]+\s*(\[\s*\d+%\])?$` that hold no `F` and no `E`; `-v` lines `^\S+::\S+ (PASSED\|SKIPPED\|XFAIL\|XPASS)\b` (one line: `[zygo: <n> passed or skipped progress lines hidden]`) |
| go test | `go test` | at least one line `^(ok\|FAIL\|\?)\s+\S+` | `--- FAIL:` lines and the indented lines under them; `FAIL` and `ok` package lines; `panic:` and the lines after it | `=== RUN`, `=== PAUSE`, `=== CONT`, `--- PASS:`, `--- SKIP:` lines and their indented lines; `?   <pkg> [no test files]` lines (one count line) |

- A run that passed keeps its summary lines, so the view is short. A run that failed keeps each failure in full. This is the "failures-only" view **(operator)**.
- A parse that does not find the summary returns "no change" (O§7.10). The view never claims a result that is not in the text: the count lines name only what was hidden.
- On Claude Code a test run that fails exits with a code that is not 0. Claude Code then sends `PostToolUseFailure`, not `PostToolUse` (`fixtures/claude-code/live/p-n24-1-exit.json:10-16`: `exit 3` gives `"error": "Exit code 3"`). O§7.12 handles this.

### 7.7 Kind views

**(author)** Only for a text that is still over the threshold after steps 1-3. Each replaces the head-and-tail view, which can cut through the part that matters. Each stays within `VIEW_CAP_BYTES` (`view.rs:5`, 8 KiB). When a kind view cannot fit its first unit, it returns "no change" and step 5 runs.

- **diff** (`kind.rs:72-74`): first a list of the changed files, one line each, `<path> +<added> -<removed>`. Then whole file sections, in order, while they fit. Then the marker, which names how many file sections are not shown. Failure prevented: a head-and-tail cut hides which files changed.
- **json**: the key outline of `result_tree` (`kind.rs:89-91`, depth 3 with array lengths), cut to half the cap; then the first lines of the text to fill the cap. A parse error (including serde_json's nesting limit) returns "no change".
- **log**: the first 20 lines; then each line whose level is `ERROR`, `WARN`, `WARNING`, `FATAL` or `CRITICAL`, or that starts with `panic` or `Traceback`, each as `<line number>: <line>`, while they fit; then the last 20 lines. Failure prevented: the error in the middle of a long log is cut out.
- `jsonl`, `tree`, `text`: no kind view.

### 7.8 The view and the marker

**(author)**

- When steps 1-4 changed the text and step 5 did not run, the view is the **shaped marker** on the first line, then the shaped text:
  `[zygo shaped this output: <steps>. The full output (<n> lines, <bytes> bytes) is saved as result <id>; read it with result_view, result_search or result_tree.]`
  `<steps>` is a comma list from a fixed set: `color codes removed`, `progress lines removed`, `repeated lines counted`, `passed tests hidden`, `build progress hidden`, `diff files listed`, `json outlined`, `log errors kept`. It holds no output text. The first line is used so that the agent knows before it reads the counts.
- When step 5 ran, the view is the §10.1 view of the shaped text with the §10.1 trim marker (`view.rs:15-22`). The numbers and the id in that marker are those of the raw text.
- `<n>`, `<bytes>` and `<id>` are always those of the stored raw text.
- **Use rule.** The view is used only when `raw_bytes − view_bytes ≥ MIN_SAVING_BYTES` (256). Else the hook changes nothing, stores nothing and writes no row. Failure prevented: a store write, a row and a marker for a saving of a few bytes, and a view that is no shorter than the text (O-C4). The check is on the final view, marker included, so the §10.1 rule "no view larger than the text" (`trim.rs:111-113`) holds.
- **Output.** Claude Code: the view goes into `stdout`, and `stderr` becomes `""` (premise PO2). Every other field of `tool_response` is copied (`trim.rs:66-82`). OpenCode: the view goes into `stdout`, which the plugin copies into `output.output` (`crates/zygo-launch/assets/opencode-plugin.js:179-182`).
- When PO2 fails (Claude Code shows the original `stderr` although the hook set it to `""`), a Bash call with a non-empty `stderr` is shaped on `stdout` only: the raw text is still stored and measured whole, the view is the shaped `stdout`, and the use rule compares `stdout` bytes. Failure prevented: the model sees the stderr part twice.

### 7.9 Store and row

**(author)** For each used view, the hook stores the raw text with `Store::put` (`crates/zygo-results/src/store.rs:57-81`; the sniffed kind is of the raw text), then writes one `trims` row: `stored_bytes` = raw bytes, `view_bytes` = view bytes, `result_id` = the stored id, `command` = the O§7.2 name (null for a tool other than Bash), `shaper` = the last step that changed the text: `libtest`, `pytest`, `gotest`, `diff`, `json`, `log`, `dedup`, `clean`, or `size` (a §10.1 view with no other step, and every non-Bash trim). The work-log line (`trim.rs:131-141`) is written as for a trim today.

A shaped view below the threshold is stored too **(operator)**. The results directory thus holds more files. The sweep of §10.1 removes them with the session.

### 7.10 Fail open

**(operator)** A shaper that cannot parse its input gives "no change", and the pipeline goes on.

**(author)** The hook runs steps 1-4 under `catch_unwind`. A panic in them gives the raw text to step 5 (the size view when over the threshold, else no change). Failure prevented: a shaper bug turns off the size trim of a large output too. The outer C6 wrapper (`crates/zygo-core/src/hook.rs:76-80`) stays for every other fault. A `Store::put` error gives no change (`trim.rs:115`, as today). A usage-store error keeps the view (`trim.rs:127-130`, as today).

### 7.11 OpenCode

The plugin passes `tool.execute.after` to `zygo hook trim` (`opencode-plugin.js:174-182`). `normalize` makes a `PostToolUse` input with `tool_input` from `input.args` (so `command` is there) and `tool_response.stdout` for `bash` (`hook_opencode.rs:42-49`). No change is needed there. The OpenCode output has no exit code in the normalized input; no shaper needs one, because each runner view reads pass or fail from the summary. A failed OpenCode command still reaches the hook, so the failures-only view works on OpenCode in each case.

### 7.12 A failed Bash call on Claude Code

**(author)** Claude Code sends `PostToolUseFailure` for a Bash call that exits with a code that is not 0, with the output in `error`. Today `trim` is not registered on that event (`crates/zygo-launch/src/claude_code.rs:476`; the `PostToolUseFailure` entries are `work` and `workspace` only, `:486`, `:494`, `:501-503`), and `trim_in` returns at `trim.rs:86`.

Premise PO1 decides what the build does:

- **PO1 holds** (a `PostToolUseFailure` hook output can replace the `error` text that the model sees, and `error` holds the command's output): the launcher registers `trim` on `PostToolUseFailure` with the matcher `Bash`, also when work units are off. The hook takes the raw text from `error` and returns the view in the field that PO1 names. All of O§7.3-7.10 apply.
- **PO1 fails**: a failed Bash call on Claude Code passes unchanged, as today. A passing run is still shaped. The build records this limit in `docs/premises.md` and in §10.1. The `PreToolUse` command rewrite that would avoid it is rejected **(operator)**.

## 8. Shaping tests (P3-7)

### 8.1 Fixtures

**(author)**

- **Place.** `fixtures/shaping/<shaper>/<case>/`, with `meta.json`, `stdout`, `stderr` and `view` (the expected view, with the id written as 26 `0` characters). `meta.json`: `{ "command": …, "exit": <int>, "agent": "claude-code" | "opencode", "tool": "<program> <version>", "captured_on": "<date>", "source": "<what was run, where>" }`.
- **Byte-exact.** `.gitattributes` gets `fixtures/shaping/** -text`. The current rule `* text=auto eol=lf` would change `\r\n` in a capture and break the clean tests.
- **Real output.** Each case is the output of a real run, not typed by hand. The required cases:

| Shaper | Cases |
|---|---|
| clean | `cargo test --color always` (passing); `git clone --progress` stderr with `\r` progress; a cargo build with the progress bar |
| dedup | a real log with repeated lines (for example a repeated warning from a build); a timestamped log with repeats |
| libtest | `cargo test` of a scratch crate, all passing (≥ 100 tests); the same crate with 2 failing tests, captured through OpenCode or with `|| true` so that it exits 0; a compile error |
| pytest | a scratch project: all passing; 2 failing with `-v`; a collection error |
| go test | a scratch module: passing with `-v`; failing with `-v`; a panic |
| diff | `git diff` of this repository over 16 KiB, ≥ 5 files |
| json | `cargo metadata --format-version 1` (over 16 KiB) |
| log | a log over 16 KiB with `ERROR` lines in its middle |
| none | each Bash fixture of `fixtures/claude-code/hooks/` (small outputs that must not change) |

- **Capture.** `scripts/fixtures/capture-shaping.sh <shaper> <case> -- <command…>` runs the command in the current directory, writes `stdout`, `stderr` and `meta.json`, and then scrubs: it replaces `$HOME` with `/home/user`, the user name with `user`, the host name with `host`, and the scratch directory with `/repo`. It never writes `view`. The builder writes `view` once with `ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping` and reviews it in the diff.
- **Secret check.** A CI test runs the built-in secret patterns of §16.1 over every file under `fixtures/shaping/` and fails on any finding. Plus the scrub of the capture script and review in the pull request.
- **Size.** Each case is at most 256 KiB; the folder is at most 4 MiB.

### 8.2 Minimum saving per shaper

**(author)** A CI test checks, on each case of the shaper, `1 − view_bytes ÷ raw_bytes` against the floor. The floors are set below what these shapers give on such output, so a regression trips them and normal variance does not.

| Shaper | Floor | Plus |
|---|---|---|
| clean | 5 % | no `ESC` byte and no `\r` in the view |
| dedup | 50 % on the repeated-line cases | expanding the counts gives the cleaned text (O§7.5) |
| libtest, passing | 80 % | each `test result:` line is in the view |
| libtest, failing | 40 % | each failed test's name and its whole `---- … stdout ----` block are in the view |
| pytest, go test | as libtest | each failed test's name and its failure section are in the view |
| diff, json, log | view ≤ 8 KiB | diff: each changed path is in the file list; log: each `ERROR` line that fits is in the view |
| none | 0 (no change) | the hook output is empty |

### 8.3 The never-worse check

**(author)** A CI test, over every case and over a fixed list of built inputs (empty output; one line; 3 equal blank lines; a lone `ESC`; an OSC with no end; a 5 MiB output; a `test result:` line with no tests; a command that is not a string; a 10,000-deep JSON array; a 400,000-character line):

1. The hook does not panic, and its exit code is 0.
2. The view is empty (no change) or `raw_bytes − view_bytes ≥ 256`.
3. When there is a view, its marker names a result id, and `Store::get` of that id gives the raw text byte for byte.
4. A runner view holds each line of the raw text that matches the runner's failure forms (O§7.6).
5. Above 4 MiB, the view equals the §10.1 size view of the raw text.

### 8.4 Determinism

Each CI test above feeds hook-input JSON to `trim_in` in the test process, with a temporary session directory and usage database. No test runs a real `claude` or `opencode`. No CI test reads a clock for a pass or fail decision. An injected shaper that panics (a `#[cfg(test)]` hook) checks O§7.10.

### 8.5 The latency budget

**(author)** The budget, for a release build on the operator's development machine:

- the shaping steps (O§7.3 steps 1-4) on a 64 KiB input: p95 ≤ 5 ms;
- the same on a 4 MiB input: p95 ≤ 100 ms;
- a whole `zygo hook trim` process on the largest fixture, from start to exit: p95 ≤ 50 ms more than the same process with `[results] shape = false`.

It is measured by a test marked `#[ignore = "timing: shaping latency budget, run on demand"]` in `crates/zygo-hooks/tests/shaping_latency.rs`, run with `cargo test -p zygo-hooks --release --test shaping_latency -- --ignored`. It runs each case 200 times, prints p50 and p95, and fails over the budget. CI runs `cargo test --workspace` (`.github/workflows/ci.yml:45`), which does not run ignored tests. The pull request of the shaping nodes carries the printed numbers. This follows the convention of commit `cb7d13c`.

## 9. Configuration

User config, `[results]` (`ResultsConfig`, `crates/zygo-config/src/schema.rs`; copied to the session view, `crates/zygo-core/src/session_view.rs:20-29`):

```toml
[results]
shape = true    # O§7: the shaping pipeline for Bash. false = size view only (§10.1).
```

**(author)** Failure prevented: a shaper that harms work can be turned off without losing the size trim and without a downgrade. `[features] results = false` still turns off both. `SHAPE_MAX_BYTES`, `MIN_SAVING_BYTES` and the runner forms are constants, not config: each one more key is one more state to test.

## 10. Failure handling

| Case | Behavior |
|---|---|
| Command missing or not a string | No runner view and no file-printer check; command name `other`; steps 1, 2, 4, 5 run. |
| Shaper cannot parse | That step gives "no change"; the next step runs. |
| Shaper panics | Steps 1-4 are dropped; step 5 on the raw text (O§7.10). |
| Raw text over 4 MiB | Step 5 only (O§7.3). |
| View saves < 256 bytes | No change, no store, no row. |
| `Store::put` fails | No change; one stderr line (C6). |
| Usage store busy or broken (hook) | The view is returned; one stderr line (`trim.rs:127-130`). |
| Usage store busy or broken (MCP read-back) | The reply is returned; one stderr line. |
| Database without the new columns or table (an older binary made it) | Writer: `Store::open` adds them. Reader: null columns, no read-backs (O§6.5). |
| Two processes add a column at once | The second gets "duplicate column name": success. |
| No request rows for an event (serve mode before `--sync`, or the session ended) | The event adds 0 to the spend figure; its tokens still count. |
| A model with no price row | Its rows add 0; `zygo usage` names the count (O§6.6). |
| Config does not load in `zygo usage` | The spend line says `unknown`; the other lines print. |
| PO2 fails | Shape `stdout` only when `stderr` is not empty (O§7.8). |
| PO1 fails | Failed Bash calls on Claude Code pass unchanged (O§7.12). |

Concurrency: subagents run their tool calls in parallel, each in its own hook process. Each process writes its own result file (a new ULID) and its own row (WAL, `busy_timeout` 5 s, `store.rs:79-85`). No two processes write the same file or row. The MCP server is one process per agent session; it writes `readbacks` rows only.

## 11. Owners of state

| State | Owner (only writer) | Readers |
|---|---|---|
| Raw text of a view | the trim hook, `results/<id>` | `result_*` MCP tools |
| `trims` row (with `result_id`, `command`, `shaper`) | the trim hook | `zygo usage`, status line, `zygo status` |
| `readbacks` row | the MCP server's result tools | the same |
| `requests` row | the proxy (as today) | `zygo usage` (carry) |
| Prices and multipliers | the catalog (§6.3) | `zygo usage` |
| The two figures, the ratio, the breakdown | none: computed at each read | — |
| Shaper rules, runner forms, command list | the code | the trim hook |
| `[results] shape` | user config, copied to the session view at launch | the trim hook |

## 12. Base-spec changes

**(author)** When this spec is approved, each change is written into the section as final text, with a change-log line.

1. §10.1, "Threshold": shaping of Bash output (O§7) below and above the threshold; raw text of Bash includes `stderr` (O§7.2). "View": the shaped marker (O§7.8). "Kind": the kind views (O§7.7). "Measure": the new `trims` columns. Config: `[results] shape`. "On/off is `[features] results` (§15.4), the only switch" becomes "… the switch for the store; `[results] shape` turns off shaping alone".
2. §10.7: the new `zygo usage` lines (O§6.6) and the net trimmed figure of `zygo status` (O§6.7).
3. §14: the `readbacks` table; the `trims` columns `result_id`, `command` (a name from a closed list, O§7.2), `shaper`; additive columns without a version change (O§6.5).
4. H§6.2 row 14 and H§7.4: the trimmed figure is net of read-backs; two indexed sums.
5. `docs/premises.md`: rows PO1-PO4 (O§17).

## 13. Alternatives rejected

- **A `PreToolUse` rewrite to a wrapper binary** (as rtk): changes what permission rules see and makes output unfit for scripts **(operator)**.
- **User-defined filter files**: need a trust gate; decide after the built-in shapers are measured **(operator)**.
- **Read-backs measured by a `PostToolUse` hook on `mcp__zygo__result_*`**: gives the agent id directly, but needs a premise on MCP hook input shapes for each agent, and OpenCode MCP tool names are not in a fixture. The MCP server sees each reply on both agents.
- **A `SCHEMA_VERSION` bump with the new columns**: `open_read_only` refuses each other version (`store.rs:126-130`), so the status line of an older binary in a running session loses its trimmed segment during an update. Additive columns have no such effect.
- **The command text, or its first word, in the usage store**: §14 forbids tool inputs; a program name can be a path. A closed list holds no user content.
- **Shaping `Read`, `Grep`, `Glob`, `WebFetch`**: a changed file text breaks `Edit` (O§7.1).
- **Dedup of non-consecutive lines**: changes the order of events.
- **A time budget inside the hook** (as the content pass of C4): the result would depend on machine load. A byte bound gives the same result each time.
- **Carry over the whole session, with no cut**: a `/clear` or a compaction removes the text, so the figure would grow without a limit.
- **The 1-hour write rate**: the row has no split; the higher rate over-states the saving.
- **A spend figure in the status line**: O§6.7.
- **Jest, vitest, nextest views now**: their output changes with the reporter; measure the first three first.
- **A change to the session text's result-store sentence** (§8.6 item 7): the shaped marker names the tools to call, so the sentence is still true. A change would also touch the P3-14/P3-15 track.

## 14. Files expected to change

| File | Change |
|---|---|
| `crates/zygo-results/src/shape/` (new: `mod.rs`, `clean.rs`, `dedup.rs`, `command.rs`, `runner.rs`, `kind_view.rs`) | O§7.2-7.7, pure functions over text |
| `crates/zygo-results/src/view.rs` | a size view over any text with the raw text's marker numbers; the shaped marker (O§7.8) |
| `crates/zygo-hooks/src/trim.rs` | raw text with `stderr`; command; the pipeline; use rule; `catch_unwind`; new row fields; `PostToolUseFailure` input when PO1 holds |
| `crates/zygo-usage/src/schema.rs` | `readbacks`, its indexes, `ReadbackRow`, new `TrimRow` fields |
| `crates/zygo-usage/src/store.rs` | additive columns in `open`; `insert_trim` columns; `insert_readback`; `prune` |
| `crates/zygo-usage/src/query.rs` | `bytes_read_back`; the carry computation (with a rate lookup passed in, because `zygo-usage` does not depend on `zygo-config`); the breakdown; reads that accept a database without the new parts |
| `crates/zygo-mcp/src/tools/results.rs` | a read-back row per successful reply |
| `crates/zygo/src/cmd/usage.rs` | O§6.6 lines; the rate lookup from the merged config's catalog |
| `crates/zygo/src/cmd/status.rs`, `crates/zygo-hooks/src/statusline/mod.rs`, `crates/zygo-hooks/src/statusline/data.rs` | the net figure (O§6.7) |
| `crates/zygo-config/src/schema.rs`, `crates/zygo-core/src/session_view.rs`, the launcher's view writer | `[results] shape` |
| `crates/zygo-launch/src/claude_code.rs` | when PO1 holds: `trim` on `PostToolUseFailure` with matcher `Bash` |
| `crates/zygo-hooks/tests/shaping.rs`, `crates/zygo-hooks/tests/shaping_latency.rs` (new) | O§8 |
| `fixtures/shaping/**`, `scripts/fixtures/capture-shaping.sh` (new), `.gitattributes` | O§8.1 |
| `docs/specs/2026-11-18-zygo-design.md`, `docs/specs/2026-11-21-statusline-hud-design.md`, `docs/premises.md` | O§12 |

## 15. Left to the builder

- The subcommand lists of O§7.2 (each a closed list of the program's own subcommands).
- The exact regular expressions of O§7.4 and O§7.6, within the forms given there. A form that the fixtures show to be wrong is fixed, and the fix is recorded as a decision record.
- The struct and function names, and the module split inside `shape/`.
- Which real projects give the fixture captures, within O§8.1.
- The wording of the count lines, within the forms of O§7.6.

## 16. Acceptance

Each item is a test or a command whose output decides it. Items 1-14 are CI tests; item 15 runs on demand.

1. Each fixture of `fixtures/shaping/` through `trim_in` gives its `view` file byte for byte (with the id masked), and its saving is at or above the O§8.2 floor.
2. Each fixture's stored result, read by `Store::get` with the id from the view, equals the fixture's raw text (O§7.2 composition) byte for byte.
3. The never-worse check of O§8.3 passes for every case and built input.
4. Each Bash input of `fixtures/claude-code/hooks/` and `fixtures/opencode/plugin-hooks/tool.execute.after-bash.json` gives no hook output and writes no row.
5. Dedup of each built input, then expansion of the counts, gives the cleaned input.
6. `cat f.txt` with a colored 20 KiB file gives the §10.1 size view, not a shaped view; `cargo test 2>&1 | tail -n 80` with libtest output gives a libtest view.
7. A Bash input with `stdout` 10 KiB of libtest output and `stderr` 6 KiB of cargo progress gives one view in `stdout`, `stderr` = `""`, and a stored raw text with the `[stderr]` line; the row's `stored_bytes` counts both streams.
8. An injected panic in a shaper on a 100 KiB input gives the §10.1 view of the raw text; on a 2 KiB input, no output.
9. With `[results] shape = false`, every fixture gives the output that the code at `97d8767` gives.
10. The trim row of `PostToolUse-Bash-sub.json` with a shaped output has `agent_id = "a1a80dd100f197242"`, `command = "other"` for `echo`, and a `result_id` that names a stored file.
11. Carry: a usage database built in the test with the O§6.3 worked example (plus an `auxiliary` row, a subagent row, a `/clear` prompt drop and an OpenCode session) gives `≈$0.1146` and `9000` tokens for the main loop; the subagent's trim is carried only by its own rows; the OpenCode trim counts its first row only. The same rows with a catalog row whose rates are doubled give twice the dollar figure (O-C5).
12. Read-back: `result_view` on a stored id writes one `readbacks` row with the reply's byte count and the id; a bad id writes no row; with the usage database path unwritable, the reply is unchanged.
13. `zygo usage` on a fixture database prints the O§6.6 lines exactly; on a database made by the code at `97d8767` (no new columns, no `readbacks`), it exits 0 and prints `-` keys and `bytes read back: 0`. Two threads that open a database of the old form at once both succeed, and `trims` has each new column once.
14. 92,160 trimmed bytes and 8,192 read-back bytes (net 83,968 bytes, 20,992 tokens) give `✂ 20k` in the status line and `trimmed 20k tok` in `zygo status`; with more read-back bytes than trimmed bytes, `✂ 0`. A CI test scans `fixtures/shaping/` with the §16.1 patterns and finds nothing.
15. On demand: `cargo test -p zygo-hooks --release --test shaping_latency -- --ignored` prints p50 and p95 for each case and passes the O§8.5 budget.

## 17. Premises the first build verifies

Each is checked with a live Claude Code or OpenCode session by the premise scripts (`scripts/premises/`), not in CI, and recorded in `docs/premises.md`.

- **PO1** Claude Code 2.1.328: for a Bash call that exits with a code that is not 0, the `PostToolUseFailure` input's `error` holds the command's stdout and stderr, and a hook output field replaces the `error` text that the model sees. The capture names the field. Result decides O§7.12.
- **PO2** Claude Code 2.1.328: a Bash `updatedToolOutput` with `stderr` set to `""` removes the original stderr from what the model sees (Q4, `docs/premises.md:35`, verified `stdout` only). Result decides O§7.8.
- **PO3** OpenCode 1.19.6: `tool.execute.after` for `bash` gives stdout and stderr together in `output.output`.
- **PO4** On both agents, the text that the model sees for a `result_view` reply is the reply text plus at most a fixed wrapper, so the reply's byte count is the read-back size within that wrapper.
