# zygo: output shaping and saving accounting — design

**Status:** approved by the operator 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, output track (roadmap items P3-5, P3-6 and P3-7, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`; C6, §10.1, §10.7, §14). Also cited: `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`). `O§n` is a section of this document. The build follows `docs/specs/2026-11-24-decision-records-design.md`; this spec does not repeat it.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's "Intent" output-track paragraphs, the P3-5 to P3-7 item rows, the "P3-5 design notes", the "P3-6 design notes" and the "Plan of work (operator, 2026-11-24)"). **(lead)**: the lead of this design work ruled it after an independent audit (2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767` (branch `phase-3`).

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
- **O-G3** `zygo usage` reports "tokens kept out of context" (one time) and "estimated spend avoided" (carried), both net of read-backs, with a breakdown by tool and command and the view to read-back ratio.
- **O-G4** A view never hides a line that says why something failed, is never larger than the text it replaces, and a shaper fault never costs the agent its output.
- **O-G5** Each shaper has fixtures from real command output and a minimum saving that a CI test checks.

## 3. Non-goals

- A rewrite of the command before it runs (`PreToolUse`) **(operator)**.
- User-defined filter files **(operator)**; also a scan of past transcripts for missed savings (roadmap "Not in phase 3").
- Shaping of `Read`, `Grep`, `Glob` and `WebFetch` output. They keep the size view of §10.1 (O§7.1).
- A number for the later compaction that a smaller context causes (O§6.8) **(operator)**.
- A change to the status line (O§6.7).
- Runners other than the three of O§7.6 in this phase.

## 4. Constraints

- **O-C1** C6 applies to each new branch: a shaper fault, a store fault or a usage-store fault never changes the agent's output for the worse. A fault gives the size view of §10.1, or no change.
- **O-C2** §14 applies: the usage store holds no tool input and no tool output. The command is stored only as a name from the closed list of O§7.2.
- **O-C3** The raw output is the one exception to C3 that §10.1 already allows: it lives only in the session's results directory, mode `0600`.
- **O-C4** A view is used only when it is smaller than the raw text by at least `MIN_SAVING_BYTES` (O§7.8). Else the hook changes nothing.
- **O-C5** Price multipliers come from the catalog price rows (§6.3), never from constants in code **(operator)**.
- **O-C6** CI tests are deterministic. No CI test measures wall-clock time or has a deadline. The latency budget is a measurement run on demand, outside CI (O§8.5) **(operator, project rule)**.
- **O-C7** Tests never run a real `claude` or `opencode` binary. Hook tests feed hook-input JSON to the hook; launcher tests use the stub agents.
- **O-C8** Each new stored field is additive and readable by older and newer binaries without a schema version change (O§6.5).
- **O-C9** **(lead)** A shaped view hides only lines that a shaper positively recognizes as noise. Each line that a shaper does not recognize is kept. No view drops the text that says why something failed. When a shaper is unsure, it keeps the line or gives "no change", so the size view applies.
- **O-C10** The trim hook never makes a Bash call wait more than `HOOK_DB_BOUND` (100 ms) on a busy usage database (O§7.11).

## 5. Terms

- **Context.** One conversation that the model sees: the main loop of a zygo session, or one subagent. Key: `(session, agent_id)`, with `agent_id` null for the main loop.
- **Event.** Something that changes the context size: a **trim** (a shaped or size view replaced the raw output) or a **read-back** (a reply of `result_view`, `result_search` or `result_tree`).
- **Carrying request.** A request of the same context, at or after the event, before the context is cut (O§6.3).
- **Segment.** A part of a command between `;`, `&&`, `||` and newline, after the shell-word split of O§7.2. A newline after a `|`, with only blanks, comments and newlines between them, does not end the segment: the pipeline goes on to the next line.
- **Shaper.** One step of the pipeline of O§7.3. **Runner view.** A shaper for one test runner.
- **Blank line.** A line that is empty or holds only ASCII space (`0x20`) and tab (`0x09`).
- **Raw text.** The text the hook measures and stores (O§7.2).

## 6. Saving accounting (P3-5)

### 6.1 The open point: is `agent_id` filled?

**(author, from the code)** Yes, for Claude Code. No change is needed.

- A request row takes `agent_id` from the `x-claude-code-agent-id` header (`crates/zygo-proxy/src/forward.rs:291`).
- A trim row takes it from the hook input (`trim.rs:121`, read at `crates/zygo-core/src/hook.rs:36`).
- Premise P3 is verified on Claude Code 2.1.328 (`docs/premises.md:27`): each request and each hook input of a subagent has the same id; no main-loop request has the header and no main-loop hook input has `agent_id`. Q5 (`docs/premises.md:36`) adds that a subagent's compaction request has the subagent's id.
- OpenCode has no agent id (§13.6). O§6.3 handles it.

### 6.2 The two figures

**(operator)** Two figures, kept apart:

1. **Tokens kept out of context** (one time). `net_bytes = Σ max(stored_bytes − view_bytes, 0) over trims − Σ bytes over read-backs`. Tokens = `sign(net_bytes) × ceil(|net_bytes| ÷ BYTES_PER_TOKEN)` (`crates/zygo-core/src/tokens.rs:4`, value 4). It is negative only when the agent read back more than the views kept out: a real loss, which is shown.
2. **Estimated spend avoided** (carried). USD, always with the `≈` mark, computed by O§6.3. Multiplied tokens are never shown as a context-size figure.

**(author)** Names: "trimmed" always means the gross figure (`stored − view`, no read-backs): the current `bytes trimmed` and `tokens trimmed (estimate)` lines of `zygo usage` and the status line's trimmed segment keep that meaning. The net figure is always called "kept out of context".

### 6.3 The carry formula

**(operator)** for the factor; **(author)** for the rules that make it computable from the stored rows.

For one event `e` with `T_e = bytes ÷ BYTES_PER_TOKEN` tokens (a real number; for a trim, `bytes = max(stored_bytes − view_bytes, 0)`; for a read-back, the reply bytes) at time `ts_e`, in context `C`:

1. **The context's requests `Q_C`.** The `requests` rows of the session with a 2xx `status`, ordered by `ts`, then `rowid`:
   - main loop (`agent_id` null): `agent_id IS NULL` and `request_class` is null, `main` or `compaction`. `auxiliary` rows (title, suggestions, WebFetch's second call; Q5) do not carry the context and are left out.
   - subagent: `agent_id = C.agent_id`, any class.
   - Prompt of a row = `input_tokens + cache_read_tokens + cache_write_tokens` (as `crates/zygo-proxy/src/window.rs:89-91`).
2. **The carrying requests `R_e`.** Start at the first row of `Q_C` with `ts ≥ ts_e` and take rows in order.
   - **Cut.** A row whose prompt is less than half the prompt of the row of `Q_C` just before it ends `R_e` before that row. The test applies to the first row too, against the last row of `Q_C` before the event. So when the context was cut between the event and its first request, `R_e` is empty and the event adds 0. The cut catches `/clear`, a compaction by any means, and a context edit.
   - **Compaction.** A row with `compaction = 1` is taken, and `R_e` ends after it (that request still sends the old text one time).
3. **Rate of each carrying request**, in USD per token, from the price row of the request's `model` in the catalog, with the tier of the request's prompt (the rule of `Prices::cost`, `crates/zygo-config/src/schema.rs:280-289`). The write rate is `cache_write_5m` (absent: `cache_write`).
   - A row with `cache_read_tokens = 0` and `cache_write_tokens = 0` (no caching): `input`.
   - The first row of `R_e`: the write rate. The event's text is new in the prompt, so this request writes it.
   - Each later row: the write rate when the row **follows a cache expiry**, else `cache_read`. A row follows a cache expiry when its `cache_read_tokens` is less than the `cache_read_tokens` of the row before it in `Q_C`. A context that only grows reads at least the prefix that the previous row read, so growth never looks like an expiry. **(lead)**
4. `spend_e = T_e × Σ rates of R_e`. A row whose model has no price row adds 0 and is counted in `unpriced` (O§6.6).
5. **Estimated spend avoided** = `Σ spend_e over trims − Σ spend_e over read-backs`.

This is the operator's `S × (w + r × N)`, with `w` and `r` taken per request from the catalog, and with `w` in place of `r` for each request that follows a cache expiry **(operator)**.

Rules **(author)**, each with the failure it prevents or the error it accepts:

- **The 5-minute write rate.** The `requests` row keeps one cache-write total, not the 5-minute/1-hour split (§14, HUD AU33). The lower rate under-states the figure rather than over-states it.
- **The half-prompt cut.** No stored row marks a `/clear` or zygo's own context cut. Without the cut, a trim before a `/clear` is carried for the rest of the session.
- **Same second.** Timestamps are Unix seconds. Let `A` be the request whose reply called the tool, and `B` the next request. `A` starts before the tool runs, and its stream takes time, so `A.ts < ts_e` in almost all cases. When `A.ts = ts_e`, `R_e` starts at `A`: `A` is priced at the write rate although it did not carry the text, and `B` is priced as a later row. The over-count is at most `T_e` × the write rate of one request per event.
- **OpenCode.** Its requests and trims have no agent id, so its subagent requests mix with the main loop. For a session whose `requests.agent` is `opencode`, `R_e` is its first row only, priced with that row's model. The figure is then the one-time write. When that row is from a subagent with another model, the error is `T_e` × the difference of the two models' write rates. This rule never carries a subagent's trim on the main loop's requests.
- **Prices.** `zygo usage` takes the catalog of the merged user config (built-in rows included). This is the catalog that priced `estimated_usd` at write time. A team session's rows carry `estimated_usd = 0` and no provider (§14 note N16); the catalog row of the model still prices them. Passthrough rows (`estimated_usd = 0`) are priced the same way: the figure is notional, as the `≈` says.

Worked example (test catalog row: `input` 4, `cache_read` 0.4, `cache_write_5m` 5, `cache_write` 8 USD per Mtok). A trim keeps 40,000 bytes = 10,000 tokens out of the main loop. Then: request 1 writes (5); requests 2-5 read, each with more cache-read tokens than the one before (4 × 0.4); request 6 has fewer cache-read tokens than request 5 (an expiry) and writes (5); request 7 is the compaction and reads (0.4). Sum of rates = 12 USD per Mtok. `spend_e = 10,000 × 12 ÷ 10^6 = $0.12`. A read-back of 4,000 bytes (1,000 tokens) after request 5 is carried by request 6 (its first row: the write rate, 5) and request 7 (a later row that reads: 0.4): `1,000 × 5.4 ÷ 10^6 = $0.0054`. Net: `≈$0.1146`. Tokens kept out: `ceil((40,000 − 4,000) ÷ 4) = 9,000`.

### 6.4 Read-backs

**(author)** The MCP server records each successful reply of `result_view`, `result_search` and `result_tree` (`crates/zygo-mcp/src/tools/results.rs:44-51`, `:66-70`, `:85-88`) as one `readbacks` row.

- Why the MCP server and not a `PostToolUse` hook on the MCP tools: the server serves Claude Code and OpenCode the same way, and no premise about MCP hook input shapes is needed.
- **Key.** The result's meta file gains `session` (O§6.5 item 1): the session whose hook stored it. The row's `session` is that session, so a read-back always counts in the figures of the session that got the trim. Thus a workspace member resumed in a new session (results directory shared, §10.1 "Workspace members") that reads a result of its old session does not make the new session's figure negative. A result with no `session` in its meta (stored by an older binary) takes the reading session.
- **Context of the carry.** The read-back's text goes into the context that called the tool. The row also has `read_in`: the reading session. The carry context is `(read_in, trims.agent_id)` when `read_in` equals the trim's session; else `(read_in, null)`, because only a main loop is resumed into a new session. The trim is found by `result_id`. A read-back of a result with no trim row counts in the main loop of `read_in`.
- A read-back from a context other than the trim's in the same session (a lead passes an id to a subagent) is counted in the trim's context. This is accepted: the id is in the marker of the trim's context, and a pass of an id to another context is rare.
- An error reply (bad id, bad range, bad pattern) writes no row.
- The server opens the usage store with the bounded open of O§7.11, at its first read-back, and keeps it. A busy or broken store: no row, and the reply is unchanged (O-C1); the next read-back tries to open again.

### 6.5 Store changes

**(author)** All changes are additive (O-C8). `SCHEMA_VERSION` (`crates/zygo-usage/src/store.rs:62`) does not change, so `open_read_only` (`store.rs:119-132`) still opens the database for each binary.

1. **Result meta.** `Meta` (`crates/zygo-results/src/store.rs:11-18`) gains `session: Option<String>` with `#[serde(default)]`. `Store::put` takes it.
2. **New table**, created by the `CREATE TABLE IF NOT EXISTS` schema text (as `upstream_errors`, `schema.rs:67-77`):
   ```sql
   CREATE TABLE IF NOT EXISTS readbacks (
       ts INTEGER NOT NULL,
       session TEXT NOT NULL,   -- the trim's session (O§6.4 key)
       read_in TEXT NOT NULL,   -- the reading session (carry context)
       result_id TEXT NOT NULL,
       tool TEXT NOT NULL,      -- result_view | result_search | result_tree
       bytes INTEGER NOT NULL
   );
   CREATE INDEX IF NOT EXISTS readbacks_session ON readbacks(session);
   CREATE INDEX IF NOT EXISTS readbacks_ts ON readbacks(ts);
   ```
3. **Three nullable columns on `trims`:** `result_id TEXT`, `command TEXT`, `shaper TEXT`. They are added by the full open `Store::open` only, one time per process: it reads `PRAGMA table_xinfo(trims)` and runs `ALTER TABLE trims ADD COLUMN` for each missing column. A "duplicate column name" error (a concurrent process added it first) is success. Processes that use the full open run one time per launch or command: the launcher (which already opens the store at launch to prune it, §14) and the CLI. The hook and the MCP server's read-back write use the bounded open (O§7.11), which never adds a column.
4. **Writes.** `TrimRow` gets `result_id`, `command` and `shaper` as `Option<String>`, with `#[serde(default)]`. `insert_trim` (`store.rs:232-245`) writes ten columns. When the insert fails with "no such column" (a database that no new full open has touched yet: a session launched by an older binary, whose hooks now run the new one), it inserts the seven old columns. An older binary's insert names its own seven columns, so the new columns are null in its rows.
5. **Reads.** A reader checks `PRAGMA table_info(trims)` and `sqlite_master` first. A missing column reads as null; a missing `readbacks` table reads as no rows. So a new `zygo usage` works on a database that only an older binary has opened.
6. `prune` (`store.rs:272-284`) also deletes `readbacks` rows older than the cutoff. `replace_requests` (`zygo usage --sync`) does not touch `trims` or `readbacks`.

### 6.6 `zygo usage`

**(author)** The current lines stay as they are (H§14 item 18 checks them). `bytes trimmed` now includes shaped views below the threshold. After `tokens trimmed (estimate)` (`usage.rs:104`), new lines:

```
bytes read back: 4000
tokens kept out of context (estimate): 9000
estimated spend avoided: ≈$0.1146
views: 14, read-backs: 3, view to read-back ratio: 4.67
by tool and command:
  Bash cargo test libtest: 6 views, 2 read-backs, 28500 bytes kept out, ≈$0.0888
  Bash git diff diff: 3 views, 0 read-backs, 9000 bytes kept out, ≈$0.0270
  Read - size: 5 views, 1 read-back, -1500 bytes kept out, ≈-$0.0012
```

- `views` = trim rows; `read-backs` = read-back rows; the view to read-back ratio = views ÷ read-backs, two decimals, `-` when there is no read-back. A low ratio shows that views hide text the agent needs.
- A breakdown key is `tool`, `command` (`-` when null), `shaper` (`-` when null). A read-back takes the key of its trim (by `result_id`), else `- - -`. Rows are sorted by bytes kept out, largest first.
- A negative amount prints the sign before `$`: `≈-$0.0012`.
- When some carrying requests had no price row: `estimated spend avoided: ≈$0.1146 (12 requests had no price)`.
- When the config does not load: `estimated spend avoided: unknown (config does not load)`, and the other lines print.
- The filter (`--session`, `--since`) selects events by their `session` (O§6.4 key for read-backs) and their own `ts`. The carrying requests of a selected event are read even when they are outside the filter: the figure belongs to the event.
- Every name passes `esc` as today (`usage.rs:112-116`).

### 6.7 Status line and `zygo status`

**(author)** No change. The status line's trimmed segment (H§6.2 row 14), `zygo status` (`crates/zygo/src/cmd/status.rs:67-69`) and `zygo_hooks::statusline::trimmed` (`crates/zygo-hooks/src/statusline/mod.rs:55-58`) keep the gross trimmed figure, which now includes shaped views. "Trimmed" means one thing everywhere (O§6.2).

The net figures are not in the status line:

- The spend figure needs a join over the session's requests on each status call; the H§7.7 deadline is for one indexed read per source.
- The status line already shows the session spend (H§6.2). A second dollar figure next to it reads as a part of it.
- The HUD has a fixed segment list (HUD OP4), and a second token figure needs a new segment.

### 6.8 Not counted

**(operator)** A smaller context reaches the soft window (§10.2) later, so it compacts less often. Each compaction avoided saves a summary request and a cache write of the new context. zygo does not put a number on this. `zygo usage` does not print it. This spec states it here only.

## 7. Output shaping (P3-6)

### 7.1 Where shaping runs

**(operator)** In the `PostToolUse` trim hook, on the finished output, with the command from the hook input. No command is rewritten.

**(author)** Shaping runs for the Bash tool only, on both agents. `Read`, `Grep`, `Glob` and `WebFetch` keep the size view of §10.1. Failure prevented: an agent copies an `old_string` for `Edit` from a view, and the view changed the file text.

For the same reason, a Bash call is not shaped when its command may print a file **(lead)**. That is when the O§7.2 parse finds a file printer in any stage of any segment, at any depth:

- file printers: `cat`, `head`, `tail`, `sed`, `awk`, `nl`, `bat`, `less`, `more`, `grep`, `rg`, `git show`, `git cat-file`; and **(operator)** `egrep`, `fgrep`, `zcat`, `gawk`, `batcat`, `tac`, `git grep`, `git blame`, and `$(<f)`: a `$( … )` or backtick substitution whose text, after blanks and newlines, starts with one `<` or with `0<` (not `<<` or `<(`), which prints the file;
- places looked into: each segment and each pipeline stage; the string argument of `bash -c`, `sh -c`, `zsh -c` (parsed in turn); **(operator)** the words after `eval` and one leading `--`, joined by single spaces (parsed in turn, as for `bash -c`); `( … )` subshells, `$( … )` and backticks; the command words after `xargs` and after `find … -exec` / `-execdir`;
- a parse that is unsure (an unclosed quote, unbalanced brackets, a nesting deeper than 4, a command over 4 KiB, or **(operator)** a `bash`, `sh` or `zsh` stage after a `|` with no `-c` option, alone or in a cluster such as `-lc`, which runs a script that it reads from the pipe) counts as a file printer. The `-c` option is looked for among the leading options only: up to `--` or the first word that does not start with `-` or `+`; `-o`, `+o`, `-O` and `+O` take the next word.

A stage made only of redirects, with an input redirect, prints that file in zsh (NULLCMD), so it is a file printer.

Such a call keeps the §10.1 size view, with stdout only (as today). A pipeline such as `cargo test 2>&1 | tail -n 80` is thus not shaped: it is the cost of the rule, accepted because a missed file printer changes text the agent may edit.

### 7.2 Input

**(author)**

- **Command.** `tool_input.command` when it is a string; else no command. Only its first 4 KiB is read. A shell-word split (single quotes, double quotes, backslash; it never fails) cuts it into segments, and each segment into pipeline stages at `|`. In a stage, leading `NAME=value` words and the wrappers `env`, `time`, `sudo`, `nice`, `command`, `timeout`, `stdbuf`, `builtin` are skipped **(operator: `timeout`, `stdbuf`; author: `builtin`)**. A wrapper is skipped with its options and the argument of each option that takes one (`timeout -s KILL`, `stdbuf -o L`); after the options of `timeout`, its duration word is skipped too. A program word is reduced to its base name.
- **Command name (stored).** From a closed list: `cargo`, `go`, `pytest`, `python`, `python3`, `uv`, `npm`, `pnpm`, `yarn`, `npx`, `node`, `make`, `just`, `git`, `gh`, `docker`, `kubectl`, `terraform`, `ls`, `find`, `tree`, `du`, `ps`, `cat`, `head`, `tail`, `sed`, `awk`, `grep`, `rg`, `jq`, `curl`. For `git`, `cargo`, `go`, `npm`, `pnpm`, `yarn`, `docker`, `kubectl` and `gh`, the next word is added when it is in that program's built-in subcommand list (for example `git diff`, `cargo test`); the builder writes these lists. The name is taken from the first segment that names a runner (O§7.6); else from the first segment whose first program is in the list; else `other`. So `cargo test || true` records `cargo test`, and `cd crates/x && git diff` records `git diff`. Nothing else of the command is stored (O-C2).
- **Raw text, shaping on, Claude Code.** `stdout`; when `stderr` is not empty, then a newline if `stdout` is not empty and does not end in one, the line `[stderr]`, and `stderr`.
- **Raw text, shaping on, OpenCode.** The plugin's `output.output` (`crates/zygo-core/src/hook_opencode.rs:42-49`), which holds both streams (premise PO3).
- **Raw text, shaping off** (`[results] shape = false`, a file printer, or a tool other than Bash): exactly as at `97d8767`: Bash `stdout` only, `stderr` is not read and not changed (`trim.rs:42-47`, `:74`).
- Only `isImage = false` output is read, as today (`trim.rs:43-45`).

### 7.3 The pipeline

**(author)** For a Bash call with `[results] shape = true` (O§9), a raw text of at most `SHAPE_MAX_BYTES` (4 MiB), and no file printer (O§7.1):

1. **Clean** (O§7.4): color codes and progress.
2. **Dedup** (O§7.5): runs of equal lines.
3. **Runner view** (O§7.6), when a segment names the runner and the text has that runner's summary.
4. **Kind view** (O§7.7), only when the text after steps 1-3 is still over the threshold.
5. **Size view** (§10.1 head and tail), when the text after step 4 is still over the threshold.

Each step takes text and gives text or "no change". No step adds content: it removes characters or lines that it recognizes (O-C9), or adds a count or a fixed note. Then O§7.8 decides if the view is used.

**Text that the model may reuse** **(lead)**: when a segment runs `git diff`, `git show`, `git log` with `-p` or `--patch`, `git format-patch --stdout` or `diff`, or when the sniffed kind of the raw text (`kind.rs:51`) is `diff`, `json` or `jsonl`, steps 1 and 2 only remove ANSI escape sequences: no `\r` change and no dedup. Failure prevented: a patch or a JSON value that the agent copies or applies differs from the real one.

Above `SHAPE_MAX_BYTES`, only step 5 runs, on the raw text. Failure prevented: a hook that runs for seconds on a huge output; the OpenCode plugin hook has a 10 s timeout (§8.7). The bound is in bytes, not time, so the result does not depend on machine speed.

Build order **(operator)**: steps 1-2, then step 4, then step 3. The work graph orders the nodes so.

### 7.4 Clean

- Remove ANSI escape sequences: CSI (`ESC [`, parameter bytes `0x30-0x3F`, intermediate bytes `0x20-0x2F`, one final byte `0x40-0x7E`), the 8-bit CSI `U+009B` with the same tail, OSC (`ESC ]` to `BEL` or `ESC \`; an OSC of more than 4 KiB with no terminator: remove only `ESC ]`), a charset designation (`ESC (` or `ESC )` plus one character, 3 bytes in all), and any other `ESC` plus its next character. A lone `ESC` is removed alone: at the end of the text, and before another `ESC`, which then starts its own sequence. OSC 8 hyperlinks keep their visible text.
- Unless O§7.3 "text that the model may reuse" applies: `\r\n` becomes `\n`; in each line, a `\r` overwrites, so only the text after the last `\r` is kept. This removes progress bars and spinners.
- Clean removes no line.

### 7.5 Dedup

- A run of 3 or more equal consecutive lines that are not blank becomes the first line plus ` [repeated <n> times]`, where `<n>` is the run length in decimal.
- When any line of the text already ends in the form ` [repeated <digits> times]`, dedup gives "no change" for the whole text.
- With these two rules dedup is lossless: replacing each line that ends in ` [repeated <n> times]` by `<n>` copies of the line without that suffix gives the input again (O§16 item 6).
- Only consecutive lines. Failure prevented: a dedup over the whole text changes the order of output, and the model reads a wrong sequence of events.

### 7.6 Runner views

**(author)** The first runners: `cargo test` (libtest output), `pytest` and `go test`. Others (`cargo nextest`, `jest`, `vitest`, `mocha`, `rspec`) wait until these three are measured (O§13).

Each runner view is a blacklist **(lead)**: it hides only the noise forms listed in its row, and keeps every other line, in order. Thus `--nocapture` output, `note:` lines, `error: test failed, to rerun pass …`, signal and crash text (`process didn't exit successfully`, `SIGSEGV`, `panicked at`), compile errors and warnings all stay. A hidden form must match the whole line.

A runner view is tried when a segment of the command names the runner, and it is used only when the text has the runner's summary. The command picks the parser; the output confirms it. A wrapper segment `make test`, `just test`, `npm test`, `pnpm test` or `yarn test` tries the three runners in the table's order.

| Runner | Command forms | Summary that confirms | Hidden (whole-line forms only) | Count line for what was hidden |
|---|---|---|---|---|
| libtest | `cargo test`, `cargo +<toolchain> test` | at least one line `^test result: (ok\|FAILED)\. \d+ passed; \d+ failed;` | `^test \S+ \.\.\. (ok\|ignored)$`, also `^test \S+ \.\.\. ignored, .*$`; cargo progress `^\s+(Compiling\|Checking\|Fresh\|Downloaded\|Downloading\|Locking\|Updating\|Blocking) \S.*$` | `[zygo: <p> passed and <i> ignored test lines hidden]` at the place of the first hidden test line of each test binary; `[zygo: <n> build progress lines hidden]` at the place of the first one |
| pytest | `pytest`, `py.test`, `python -m pytest`, `python3 -m pytest`, `uv run pytest`, `poetry run pytest` | a last line `^=+ .*\b(passed\|failed\|error\|errors\|skipped)\b.* in [\d.]+s.* =+$` | progress lines `^\S+\.py [.sxX]+\s*(\[\s*\d+%\])?$` (no `F`, no `E`); `-v` lines `^\S+::\S+ (PASSED\|SKIPPED\|XFAIL\|XPASS)\b.*$` | `[zygo: <n> passed or skipped progress lines hidden]` at the place of the first one |
| go test | `go test` | at least one line `^(ok\|FAIL\|\?)\s+\S+` | see the go test rules below; `^\?\s+\S+\s+\[no test files\]$` | `[zygo: <n> lines of passed tests hidden]`, `[zygo: <n> packages with no test files hidden]` |

**Failure section** of each runner: the lines that a view keeps whole for each failed test.

- libtest: the line `test <name> ... FAILED`, the `---- <name> stdout ----` block to the next `---- ` header or `failures:` line, and the `failures:` name list. None of these match a hidden form.
- pytest: the `FAILURES` and `ERRORS` sections and the `short test summary info` section, to the next `=`-line header. None of these match a hidden form.
- go test: every line from the test's `=== RUN <name>` to its `--- FAIL: <name>` line and the indented lines after it, including all output lines that the rules below give to that test.

**go test rules.** With `-v`, output of parallel tests is interleaved. The view tracks the current test:

1. `=== RUN <name>`, `=== CONT <name>`, `=== NAME <name>` and `=== PAUSE <name>` make `<name>` the current test. Each later line that is not a `===` or `---` line belongs to the current test.
2. `^\s*--- (PASS|FAIL|SKIP): <name> \(.*\)$` is the result line of `<name>`. The indented lines after a result line, to the next line with less or equal indentation, belong to `<name>`.
3. After the whole text is read, the lines of each test whose result is `PASS` or `SKIP` (its `===` lines, its result line and the lines that belong to it) are hidden. The lines of a test whose result is `FAIL`, or that has no result line (a panic, a timeout, a crash), are kept. A line that belongs to no test is kept.
4. Package lines (`ok`, `FAIL`, `PASS`, `panic:`, `exit status`) are kept.

**Pass and fail.** A run that passed keeps its summary lines, so the view is short. A run that failed keeps each failure section in full. This is the "failures-only" view **(operator)**. A parse that does not find the summary gives "no change". The count lines name only what was hidden.

### 7.7 Kind views

**(author)** Only for a text that is still over the threshold after steps 1-3. Each replaces the head-and-tail view, which can cut through the part that matters. Each stays within `VIEW_CAP_BYTES` (`crates/zygo-results/src/view.rs:6`, 8 KiB). When a kind view cannot fit its first unit, it gives "no change" and step 5 runs.

- **diff** (`kind.rs:72-74`): first a list of the changed files, one line each, `<path> +<added> -<removed>`. Then whole file sections, in order, while they fit; the first section that does not fit ends the walk. Then the marker `[zygo: <n> file sections not shown]`, where `<n>` is the count of file sections not shown; the marker is written also when `<n>` is 0. Text before the first file section (for example a commit header) is not shown. Failure prevented: a head-and-tail cut hides which files changed.
- **json**: the key outline of `result_tree` (`kind.rs:89-91`, depth 3 with array lengths), cut to half the cap; then the first lines of the text to fill the cap. A parse error (including serde_json's nesting limit) gives "no change".
- **log**: the first 20 lines; then each line that holds one of the whole words `ERROR`, `WARN`, `WARNING`, `FATAL` or `CRITICAL` anywhere in the line (case-sensitive), or that starts with `panic` or `Traceback`, each as `<line number>: <line>`, while they fit; then the last 20 lines. `<line number>` counts lines in the text after steps 1-3, not in the stored raw text. A log with no such line between the first and the last 20 lines still gives the first and the last 20 lines; the use rule (O§7.8) decides whether to use the view. Failure prevented: the error in the middle of a long log is cut out.
- `jsonl`, `tree`, `text`: no kind view.

A kind view and the size view cut lines that are not noise. They run only over the threshold, where §10.1 already cuts, and the marker names the stored raw text. This is the "fall back to the size view" of O-C9.

### 7.8 The view and the marker

**(author)**

- When steps 1-4 changed the text and step 5 did not run, the view is the **shaped marker** on the first line, then the shaped text:
  `[zygo shaped this output: <steps>. The full output (<n> lines, <bytes> bytes) is saved as result <id>; read it with result_view, result_search or result_tree.]`
  `<steps>` is a comma list from a fixed set: `color codes removed`, `progress lines removed`, `repeated lines counted`, `passed tests hidden`, `build progress hidden`, `diff files listed`, `json outlined`, `log errors kept`. It holds no output text.
- When step 5 ran, the view is the §10.1 view of the shaped text with the §10.1 trim marker (`view.rs:15-22`).
- `<n>`, `<bytes>` and `<id>` are always those of the stored raw text.
- **Use rule.** The view is used only when `raw_bytes − view_bytes ≥ MIN_SAVING_BYTES` (256), with `raw_bytes` and `view_bytes` as the row records them (O§7.9). Else the hook changes nothing, stores nothing and writes no row. Failure prevented: a store write, a row and a marker for a saving of a few bytes. The check is on the final view, marker included, so the §10.1 rule "no view larger than the text" (`trim.rs:111-113`) holds.
- **Output, `PostToolUse`.** Claude Code: `{"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedToolOutput": <tool_response with stdout = view, stderr = "">}}`; every other field of `tool_response` is copied (`trim.rs:66-82`, `:143-146`). Premise PO2 checks that `stderr = ""` removes the stderr. OpenCode: the view goes into `stdout`, which the plugin copies into `output.output` (`crates/zygo-launch/assets/opencode-plugin.js:179-182`).
- **When PO2 fails** (Claude Code shows the original `stderr` although the hook set it to `""`): the hook leaves `stderr` unchanged in the output, and the view is built from `stdout` only. The stored raw text is still the O§7.2 composition (stderr stored). The row counts only bytes that left the context: `stored_bytes` = `stdout` bytes, `view_bytes` = view bytes. The use rule uses these numbers. Failure prevented: the model sees the stderr part twice, and the figure counts stderr bytes that stayed in the context.

### 7.9 Store and row

**(author)** For each used view, the hook stores the raw text with `Store::put` (`crates/zygo-results/src/store.rs:57-81`, with the session of O§6.5 item 1; the sniffed kind is of the raw text), then writes one `trims` row: `stored_bytes` and `view_bytes` as O§7.8 says (raw text bytes and view bytes, except in the PO2-fail branch), `result_id` = the stored id, `command` = the O§7.2 name (null for a tool other than Bash), `shaper` = the last step that changed the text: `libtest`, `pytest`, `gotest`, `diff`, `json`, `log`, `dedup`, `clean`, or `size` (a §10.1 view with no other step, and every trim of another tool). The work-log line (`trim.rs:131-141`) is written as for a trim today.

A shaped view below the threshold is stored too **(operator)**. The results directory thus holds more files. The sweep of §10.1 removes them with the session.

### 7.10 Fail open

**(operator)** A shaper that cannot parse its input gives "no change", and the pipeline goes on.

**(author)** The hook runs steps 1-4 under `catch_unwind`. A panic in them gives the raw text to step 5 (the size view when over the threshold, else no change). Failure prevented: a shaper bug turns off the size trim of a large output too. The outer C6 wrapper (`crates/zygo-core/src/hook.rs:76-80`) stays for every other fault. A `Store::put` error gives no change (`trim.rs:115`, as today).

### 7.11 Hook cost

**(author)** Today each Bash, Read, Grep, Glob and WebFetch call starts one `zygo hook trim` process (`crates/zygo-launch/src/claude_code.rs:476`; OpenCode: each tool, `opencode-plugin.js:179`). It parses the input and, for a hooked tool, reads the session view and `overrides.json` (`trim.rs:100-102`). Only an output over 16 KiB costs more: one result file and one usage-store write.

Shaping adds, for each Bash call with shaping on: the shaping steps (CPU, bounded by `SHAPE_MAX_BYTES`, measured in O§8.5); and, for each used view, which is now also below 16 KiB: one `Store::put` (two files and a rename) and one usage-store write.

Bound **(lead)**: the hook opens the usage store with a new bounded open, `Store::open_bounded(path, HOOK_DB_BOUND)`:

- busy timeout `HOOK_DB_BOUND` (100 ms) for each statement, and no retry loop (the full open waits up to 5 s, `store.rs:79-111`);
- no `PRAGMA table_info`, no `ALTER` (O§6.5 item 3);
- the insert of O§6.5 item 4.

When the store is busy or broken, the hook writes no row, keeps the view, and writes one fixed stderr line. The figures then under-count that view. The bound is a parameter, so a test passes 0 (O§16 item 15). The MCP server's read-back write (O§6.4) uses the same open.

### 7.12 OpenCode

The plugin passes `tool.execute.after` to `zygo hook trim` (`opencode-plugin.js:174-182`). `normalize` makes a `PostToolUse` input with `tool_input` from `input.args` (so `command` is there) and `tool_response.stdout` for `bash` (`hook_opencode.rs:42-49`). No change is needed there. The normalized input has no exit code; no shaper needs one, because each runner view reads pass or fail from the summary. A failed OpenCode command still reaches the hook as `tool.execute.after`, so the failures-only view works on OpenCode in each case.

### 7.13 A failed Bash call on Claude Code

**(author)** On Claude Code a Bash call that exits with a code that is not 0 fires `PostToolUseFailure`, not `PostToolUse`. The recorded fixture shows this for a command that prints nothing: `exit 3` gives `"hook_event_name": "PostToolUseFailure"` and `"error": "Exit code 3"` (`fixtures/claude-code/live/p-n24-1-exit.json:10-16`). No fixture shows where the output of a failing command that prints text arrives. Today `trim` is not registered on that event (the `PostToolUseFailure` entries are `work` and `workspace` only, `crates/zygo-launch/src/claude_code.rs:486`, `:494`, `:501-503`), and `trim_in` returns at `trim.rs:86`. So a failing test run on Claude Code is not shaped today.

Premise PO1 (O§17) decides what the build does. Its capture is recorded as `fixtures/claude-code/hooks/PostToolUseFailure-Bash-output-main.json`, from a command that prints to both streams and exits 3.

**If PO1 holds** (the `error` text holds the command's output, and a `PostToolUseFailure` hook output replaces the text that the model sees):

1. Registration: the launcher adds `{"matcher": "Bash", "hooks": [{"type": "command", "command": "<zygo> hook trim"}]}` to `PostToolUseFailure`, always (also when work units are off and for a non-member), before the existing entries.
2. Input: `trim_in` accepts `hook_event_name = "PostToolUseFailure"` with `tool_name = "Bash"`. The raw text is the `error` string when it is a string; else no change.
3. The first line of `error` that matches `^Exit code \d+$` stays the first line of the view, before the shaped marker. It is never hidden, cut or counted.
4. The pipeline of O§7.3-7.10 runs on the rest of the text. A file printer: the §10.1 size view applies over the threshold, with the exit-code line first. `[results] shape = false`: no change, as at `97d8767` (O§9).
5. Output: `{"hookSpecificOutput": {"hookEventName": "PostToolUseFailure", "<field>": <view>}}`, where `<field>` and its shape (a string, or an object like `tool_response`) are as the PO1 capture shows. The builder records the field in `docs/premises.md` and in §10.1.

**If PO1 fails**: no registration on `PostToolUseFailure` for `trim`; `trim_in` ignores that event; a failed Bash call on Claude Code passes unchanged, as today. A passing run is still shaped. The build records the limit in `docs/premises.md` and in §10.1. The `PreToolUse` rewrite that would avoid it is rejected **(operator)**.

## 8. Shaping tests (P3-7)

### 8.1 Fixtures

**(author)**

- **Place.** `fixtures/shaping/<shaper>/<case>/`, with `meta.json`, `stdout`, `stderr`, `must_keep` and `view`. `meta.json`: `{ "command": …, "exit": <int>, "event": "PostToolUse" | "PostToolUseFailure", "agent": "claude-code" | "opencode", "tool": "<program> <version>", "captured_on": "<date>", "source": "<what was run, where>" }`. `view` is the expected view, with the id written as 26 `0` characters.
- **`must_keep`** **(lead)**: the lines of the raw text, one per line, that each view of this case must hold: for each failing test, its name line and each line of its assertion or panic text; for a log, each `ERROR` line; for a diff, each `diff --git` line; when the output is over the threshold, only those of the file sections that the diff view (O§7.7) shows whole under the cap. The person who captures the case writes it by reading the raw output. A case whose `meta.json` has a failing test (exit not 0, or a runner summary with failures) and an empty `must_keep` fails the CI test.
- **Byte-exact.** `.gitattributes` gets `fixtures/shaping/** -text`. The current rule `* text=auto eol=lf` would change `\r\n` in a capture.
- **Real output.** Each case is the output of a real run, not typed by hand. The required cases:

| Shaper | Cases |
|---|---|
| clean | `cargo test --color always -- --color always` (passing; the part after `--` colors libtest stdout); `git clone --progress --no-local` stderr with `\r` progress (a clone from a local path prints no progress without `--no-local`); `git diff --color=always` with a CRLF file (must stay byte-equal except the color codes) |
| dedup | a real build log with a repeated warning; an output that already has a line ending in ` [repeated 4 times]` (no change) |
| libtest | `cargo test` of a scratch crate, all passing (≥ 100 tests); the same crate with 2 failing tests, one with `--nocapture` output; a test that crashes with a signal; a compile error |
| pytest | a scratch project: all passing; 2 failing with `-v`; a collection error |
| go test | a scratch module: passing with `-v`; failing with `-v` with `t.Parallel()` subtests, so that `=== PAUSE`, `=== CONT` and `=== NAME` occur and a failing test's `t.Log` output is interleaved with a passing one's; a panic |
| diff | `git diff` of this repository over 16 KiB, ≥ 5 files |
| json | `cargo metadata --format-version 1` (over 16 KiB) |
| log | a log over 16 KiB with `ERROR` lines in its middle |
| failure | the PO1 capture (O§7.13), both a short and a failing `cargo test` run on Claude Code |
| none | each Bash fixture of `fixtures/claude-code/hooks/` (small outputs that must not change) |

- **Capture.** `scripts/fixtures/capture-shaping.sh <shaper> <case> -- <command…>` runs the command in the current directory, writes `stdout`, `stderr`, an empty `must_keep` and `meta.json`, and then scrubs: it replaces `$HOME` with `/home/user`, the user name with `user`, the host name with `host`, and the scratch directory with `/repo`. It never writes `view`. The builder writes `view` with `ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping` and reviews it in the diff. **Bless refuses** to write a `view` that does not hold each `must_keep` line or that fails any other O§8.2-8.3 check; the test then fails and names the missing lines.
- **Secret check.** A CI test runs the built-in secret patterns of §16.1 over every file under `fixtures/shaping/` and fails on any finding (O§16 item 16).
- **Size.** Each case is at most 256 KiB; the folder is at most 4 MiB.

### 8.2 Minimum saving per shaper

**(author)** A CI test checks, on each case of the shaper, `1 − view_bytes ÷ raw_bytes` against the floor. The floors are below what these shapers give on such output, so a regression trips them and normal variance does not.

| Shaper | Floor | Plus |
|---|---|---|
| clean | 5 % | no `ESC` byte in the view; no `\r` unless O§7.3 "text that the model may reuse" applies |
| dedup | 50 % on the repeated-warning case | O§7.5 expansion gives the cleaned text |
| libtest, passing | 80 % | each `test result:` line is in the view |
| libtest, pytest, go test, failing | 30 % | each `must_keep` line is in the view |
| diff, json, log | view ≤ 8 KiB | each `must_keep` line is in the view (checked with `contains`: the log view writes a kept line inside a `<n>: <line>` line) |
| none | 0 (no change) | the hook output is empty |

A case whose raw text has no runner summary line (O§7.6), such as the libtest compile error, is checked against the `none` row: the hook makes no change, so the model gets each `must_keep` line in the raw text.

### 8.3 The never-worse check

**(author)** A CI test, over every case and over a fixed list of built inputs (empty output; one line; 3 equal blank lines; a lone `ESC`; an OSC with no end; a 5 MiB output; a `test result:` line with no tests; a command that is not a string; a command with an unclosed quote; a 10,000-deep JSON array; a 400,000-character line):

1. The hook does not panic, and its exit code is 0.
2. The view is empty (no change) or `raw_bytes − view_bytes ≥ 256` with the row's numbers.
3. When there is a view, its marker names a result id, and `Store::get` of that id gives the raw text byte for byte.
4. Each `must_keep` line is in the view, when there is a view (O-C9). The check uses `contains`, because the log view writes a kept line inside a `<n>: <line>` line.
5. Each line that a runner view hides matches one of the hidden forms of its runner's row (O§7.6), or belongs to a passed go test by the go test rules. The test checks this line by line against the raw text.
6. Above 4 MiB, the view equals the §10.1 size view of the raw text.

### 8.4 Determinism

Each CI test above feeds hook-input JSON to `trim_in` in the test process, with a temporary session directory and usage database. No test runs a real `claude` or `opencode`. No CI test reads a clock for a pass or fail decision. An injected shaper that panics (a `#[cfg(test)]` hook) checks O§7.10. The busy store is a write lock held by a second connection in the test, with `HOOK_DB_BOUND` passed as 0.

### 8.5 The latency budget

**(author)** The budget, for a release build on the operator's development machine:

- the shaping steps (O§7.3 steps 1-4) on a 64 KiB input: p95 ≤ 5 ms;
- the same on a 4 MiB input: p95 ≤ 100 ms;
- a whole `zygo hook trim` process on the largest fixture, from start to exit: p95 ≤ 50 ms more than the same process with `[results] shape = false`;
- the same process while another connection holds the usage-store write lock: p95 ≤ `HOOK_DB_BOUND` + 50 ms.

It is measured by a test marked `#[ignore = "timing: shaping latency budget, run on demand"]` in `crates/zygo-hooks/tests/shaping_latency.rs`, run with `cargo test -p zygo-hooks --release --test shaping_latency -- --ignored`. It runs each case 200 times, prints p50 and p95, and fails over the budget. CI runs `cargo test --workspace` (`.github/workflows/ci.yml:45`), which does not run ignored tests. The pull request of the shaping nodes carries the printed numbers. This follows the convention of commit `cb7d13c`.

## 9. Configuration

User config, `[results]` (`ResultsConfig`, `crates/zygo-config/src/schema.rs`; copied to the session view, `crates/zygo-core/src/session_view.rs:20-29`):

```toml
[results]
shape = true    # O§7: the shaping pipeline for Bash. false = the behavior at 97d8767.
```

**(author)** With `shape = false` the hook output and the stored file are byte-equal to those at `97d8767`: Bash `stdout` only, `stderr` neither read nor changed (O§7.2), the §10.1 size view over the threshold, nothing below it. The row has the same seven values as at `97d8767`, plus `result_id`, `command` and `shaper = "size"`. Failure prevented: a shaper that harms work can be turned off without losing the size trim and without a downgrade. `[features] results = false` still turns off both. `SHAPE_MAX_BYTES`, `MIN_SAVING_BYTES`, `HOOK_DB_BOUND` and the runner forms are constants, not config: each one more key is one more state to test.

## 10. Failure handling

| Case | Behavior |
|---|---|
| Command missing or not a string | No runner view; no file-printer found; command name `other`; steps 1, 2, 4, 5 run. |
| Command parse unsure | Counts as a file printer: size view, stdout only (O§7.1). |
| Shaper cannot parse | That step gives "no change"; the next step runs. |
| Shaper panics | Steps 1-4 are dropped; step 5 on the raw text (O§7.10). |
| Raw text over 4 MiB | Step 5 only (O§7.3). |
| View saves < 256 bytes | No change, no store, no row. |
| `Store::put` fails | No change; one stderr line (C6). |
| Usage store busy past `HOOK_DB_BOUND`, or broken (hook) | The view is returned; no row; one stderr line; the figures under-count (O§7.11). |
| Usage store busy or broken (MCP read-back) | The reply is returned; no row; one stderr line. |
| Database without the new columns (hook insert) | The seven-column insert (O§6.5 item 4). |
| Database without the new columns or table (reader) | Null columns, no read-backs (O§6.5 item 5). |
| Two full opens add a column at once | The second gets "duplicate column name": success. |
| No request rows for an event (serve mode before `--sync`, or the session ended) | The event adds 0 to the spend figure; its tokens still count. |
| The context was cut before the event's first request | The event adds 0 to the spend figure (O§6.3 cut). |
| A model with no price row | Its rows add 0; `zygo usage` names the count (O§6.6). |
| Config does not load in `zygo usage` | The spend line says `unknown`; the other lines print. |
| PO2 fails | Shape and count `stdout` only (O§7.8). |
| PO1 fails | Failed Bash calls on Claude Code pass unchanged (O§7.13). |

**Flows walked.**

- *Normal, main loop and subagent, Claude Code:* one hook process per call, shapes, stores, writes the row with the input's `agent_id`; `zygo usage` carries it over that context's requests only.
- *Concurrent subagents:* each tool call runs its own hook process, writes its own result file (a new ULID) and its own row (WAL). No two processes write the same file or row. A long lock by one writer makes the others skip their rows after 100 ms; their views are not affected.
- *Failure part way:* store written, row not written (busy): the view stands, the figure under-counts. Row written, view not delivered (the agent ended): the figure over-counts that one event, and its carry is 0 when no request follows.
- *OpenCode:* the plugin path of O§7.12; OpenCode's own hook timeout (10 s, §8.7) is far above the hook's bounds.
- *Workspace member resume:* the results directory is shared; a read-back counts in the trim's session and is carried in the new session's main loop (O§6.4).
- *Serve mode:* `trims` and `readbacks` are local; `requests` arrive at session end or by `--sync`; before that the spend adds 0.
- *Other entry points:* `zygo usage --sync` replaces `requests` only; `zygo ws forget` deletes a results directory, so later read-backs of it fail and write no row; the sweep deletes ended sessions' results with the same effect.

## 11. Owners of state

| State | Owner (only writer) | Readers |
|---|---|---|
| Raw text of a view and its meta (with `session`) | the trim hook, `results/<id>` and `<id>.json` | `result_*` MCP tools |
| `trims` row (with `result_id`, `command`, `shaper`) | the trim hook | `zygo usage`, status line, `zygo status` |
| `trims` columns `result_id`, `command`, `shaper` (their existence) | `Store::open` (full open) | readers, the hook's insert |
| `readbacks` row | the MCP server's result tools | `zygo usage` |
| `requests` row | the proxy (as today) | `zygo usage` (carry) |
| Prices and multipliers | the catalog (§6.3) | `zygo usage` |
| The two figures, the ratio, the breakdown | none: computed at each read | — |
| Shaper rules, runner forms, command list | the code | the trim hook |
| `must_keep` of a fixture | the person who captured it | the CI tests, bless |
| `[results] shape` | user config, copied to the session view at launch | the trim hook |

## 12. Base-spec changes

**(author)** When this spec is approved, each change is written into the section as final text, with a change-log line.

1. §10.1, "Threshold": shaping of Bash output (O§7) below and above the threshold; the raw text of Bash includes `stderr` when shaping is on (O§7.2); the failed-call branch that PO1 decides (O§7.13). "View": the shaped marker (O§7.8). "Kind": the kind views (O§7.7). "Measure": the new `trims` columns. "Stored form": `session` in the meta. Config: `[results] shape`. "On/off is `[features] results` (§15.4), the only switch" becomes "… the switch for the store; `[results] shape` turns off shaping alone".
2. §10.7: the new `zygo usage` lines (O§6.6). "Trimmed" keeps its gross meaning.
3. §14: the `readbacks` table; the `trims` columns `result_id`, `command` (a name from a closed list, O§7.2), `shaper`; additive columns without a version change, added by the full open (O§6.5); the bounded open of the hook (O§7.11).
4. `docs/premises.md`: rows PO1-PO4 (O§17).

The HUD spec does not change (O§6.7).

## 13. Alternatives rejected

- **A `PreToolUse` rewrite to a wrapper binary** (as rtk): changes what permission rules see and makes output unfit for scripts **(operator)**.
- **User-defined filter files**: need a trust gate; decide after the built-in shapers are measured **(operator)**.
- **Runner views as allow-lists** (keep the listed failure forms, hide the rest): any form the list does not know (a crash line, `--nocapture` output) is lost **(lead)**.
- **Read-backs measured by a `PostToolUse` hook on `mcp__zygo__result_*`**: gives the agent id directly, but needs a premise on MCP hook input shapes for each agent, and OpenCode MCP tool names are not in a fixture.
- **A read-back keyed by the reading session**: a resumed member's read-back of an old result would make the new session's figure negative (O§6.4).
- **A `SCHEMA_VERSION` bump with the new columns**: `open_read_only` refuses each other version (`store.rs:126-130`), so the status line of an older binary in a running session loses its trimmed segment during an update.
- **`PRAGMA table_info` and `ALTER` in each hook process**: costs each Bash call a schema read and can take a write lock; the full open does it one time per launcher or CLI process.
- **The command text, or its first word, in the usage store**: §14 forbids tool inputs; a program name can be a path. A closed list holds no user content.
- **Shaping `Read`, `Grep`, `Glob`, `WebFetch`**: a changed file text breaks `Edit` (O§7.1).
- **A file-printer check on the last segment only**: `cat f && true` and `sh -c 'cat f'` would be shaped **(lead)**.
- **Dedup of non-consecutive lines**, and **dedup of log lines equal after their timestamps**: the first changes the order of events; the second is not lossless.
- **Expiry as "cache write > cache read"**: fires on a small context that grows, which is not an expiry **(lead)**.
- **A time budget inside the shaping steps** (as the content pass of C4): the result would depend on machine load. A byte bound gives the same result each time. The one time bound, `HOOK_DB_BOUND`, decides only if a row is written, never the view.
- **Carry over the whole session, with no cut**: a `/clear` or a compaction removes the text, so the figure would grow without a limit.
- **The 1-hour write rate**: the row has no split; the higher rate over-states the saving.
- **A net figure or a spend figure in the status line**: O§6.7.
- **Jest, vitest, nextest views now**: their output changes with the reporter; measure the first three first.
- **A change to the session text's result-store sentence** (§8.6 item 7): the shaped marker names the tools to call, so the sentence is still true.

## 14. Files expected to change

| File | Change |
|---|---|
| `crates/zygo-results/src/shape/` (new: `mod.rs`, `clean.rs`, `dedup.rs`, `command.rs`, `runner.rs`, `kind_view.rs`) | O§7.1-7.7, pure functions over text |
| `crates/zygo-results/src/view.rs` | a size view over any text with the raw text's marker numbers; the shaped marker (O§7.8) |
| `crates/zygo-results/src/store.rs` | `session` in `Meta`; `Store::put` takes it |
| `crates/zygo-hooks/src/trim.rs` | raw text with `stderr` when shaping is on; command; the pipeline; use rule; `catch_unwind`; new row fields; bounded store open; the `PostToolUseFailure` branch when PO1 holds |
| `crates/zygo-usage/src/schema.rs` | `readbacks`, its indexes, `ReadbackRow`, new `TrimRow` fields |
| `crates/zygo-usage/src/store.rs` | additive columns in `open`; `open_bounded`; `insert_trim` with the seven-column fallback; `insert_readback`; `prune` |
| `crates/zygo-usage/src/query.rs` | the carry computation (with a rate lookup passed in, because `zygo-usage` does not depend on `zygo-config`); the breakdown; reads that accept a database without the new parts |
| `crates/zygo-mcp/src/tools/results.rs` | a read-back row per successful reply |
| `crates/zygo/src/cmd/usage.rs` | O§6.6 lines; the rate lookup from the merged config's catalog |
| `crates/zygo-config/src/schema.rs`, `crates/zygo-core/src/session_view.rs`, the launcher's view writer | `[results] shape` |
| `crates/zygo-launch/src/claude_code.rs` | when PO1 holds: `trim` on `PostToolUseFailure` with matcher `Bash` |
| `crates/zygo-hooks/tests/shaping.rs`, `crates/zygo-hooks/tests/shaping_latency.rs` (new) | O§8 |
| `fixtures/shaping/**`, `fixtures/claude-code/hooks/PostToolUseFailure-Bash-output-main.json`, `scripts/fixtures/capture-shaping.sh` (new), `.gitattributes` | O§8.1, O§7.13 |
| `docs/specs/2026-11-18-zygo-design.md`, `docs/premises.md` | O§12 |

## 15. Left to the builder

- The subcommand lists of O§7.2 (each a closed list of the program's own subcommands).
- The exact regular expressions of O§7.4 and O§7.6, within the forms given there. A form that the fixtures show to be wrong is fixed only toward hiding less (O-C9), and the fix is recorded as a decision record.
- The struct and function names, and the module split inside `shape/`.
- Which real projects give the fixture captures, within O§8.1.
- The wording of the count lines, within the forms of O§7.6.

## 16. Acceptance

Each item is a test or a command whose output decides it. Items 1-16 are CI tests; item 17 runs on demand.

1. Each fixture of `fixtures/shaping/` through `trim_in` gives its `view` file byte for byte (with the id masked), each `must_keep` line is in it, and its saving is at or above the O§8.2 floor. `ZYGO_BLESS=1` with a shaper changed to hide a `must_keep` line writes no `view` and fails naming the line.
2. Each fixture's stored result, read by `Store::get` with the id from the view, equals the fixture's raw text byte for byte.
3. The never-worse check of O§8.3 passes for every case and built input.
4. go test: in the parallel failing fixture, each line of the failing test's `t.Log` output is in the view, also the lines that follow an `=== CONT` or `=== NAME` of that test; the lines of the passing tests are not.
5. Each Bash input of `fixtures/claude-code/hooks/` and `fixtures/opencode/plugin-hooks/tool.execute.after-bash.json` gives no hook output and writes no row.
6. Dedup, then the O§7.5 expansion, gives the input again for each built input; an input with a line ending in ` [repeated 4 times]` gives no change; 3 equal blank lines give no change.
7. No shaping, only the §10.1 rule on stdout, for each of: `cat f.txt`; `cat f.txt && true`; `true; head f.txt`; `sh -c 'sed -n 1,9p f'`; `bash -c "cat f"`; `(cat f)`; `echo $(cat f)`; `ls | xargs cat`; `find . -name x -exec cat {} \;`; `cargo test 2>&1 | tail -n 80`; a command with an unclosed quote. Each with a colored 20 KiB output.
8. `cargo test || true` with libtest output gives a libtest view and the row's `command` is `cargo test`; `cd x && git diff` records `git diff`.
9. `git diff` output with a CRLF line and 3 equal `+` lines, and a JSON output with 3 equal lines: the view differs from the raw text only by removed color codes.
10. A Bash input with `stdout` 10 KiB of libtest output and `stderr` 6 KiB of cargo progress gives one view in `stdout`, `stderr` = `""`, a stored raw text with the `[stderr]` line, and a row whose `stored_bytes` counts both streams. With the PO2-fail branch selected: `stderr` is unchanged in the output, the view is built from `stdout`, and the row's `stored_bytes` and `view_bytes` count `stdout` only.
11. With `[results] shape = false`, every fixture gives a hook output and a stored file byte-equal to those of the code at `97d8767`, and `stderr` is never read.
12. An injected panic in a shaper on a 100 KiB input gives the §10.1 view of the raw text; on a 2 KiB input, no output.
13. Carry, on a usage database built in the test:
    - the O§6.3 worked example gives `≈$0.1146` and `9000` tokens;
    - a small context whose cache-write tokens are above its cache-read tokens on each row, and whose cache-read tokens never fall, prices each later row at `cache_read`;
    - an event whose first request has a prompt under half of the request before the event adds 0;
    - a subagent's trim is carried only by its own rows; an `auxiliary` row carries nothing; an OpenCode trim counts its first row only;
    - a read-back in session S2 of a result stored in S1 counts in S1's figures (`--session S1`), adds nothing to S2's (`--session S2` shows no negative figure), and is carried by S2's main-loop rows;
    - the same rows with a catalog row whose rates are doubled give twice the dollar figure (O-C5).
14. Read-back: `result_view` on a stored id writes one `readbacks` row with the reply's byte count, the id, the trim's session and the reading session; a bad id writes no row; with the store's write lock held by another connection and the bound 0, the reply is unchanged and no row is written.
15. Store: with the write lock held by another connection and `HOOK_DB_BOUND` passed as 0, the trim hook returns its view and writes no row. On a database made by the code at `97d8767`, the hook's insert writes the seven columns; a full open then adds the three columns; a full open whose column check is injected to report a column as missing gets "duplicate column name" and succeeds. `zygo usage` on the `97d8767` database exits 0 and prints `-` keys and `bytes read back: 0`. On a fixture database it prints the O§6.6 lines exactly.
16. A CI test scans `fixtures/shaping/` with the §16.1 patterns and finds nothing.
17. On demand: `cargo test -p zygo-hooks --release --test shaping_latency -- --ignored` prints p50 and p95 for each case and passes the O§8.5 budget.

When PO1 holds, also:

18. The launcher's settings for a non-member with work units off have `PostToolUseFailure: [{"matcher": "Bash", … "hook trim"}]`. `PostToolUseFailure-Bash-output-main.json` with its output repeated to 20 KiB gives a view whose first line is `Exit code 3`, in the field and shape that PO1 recorded, with `hookEventName` `PostToolUseFailure`. The same input with a silent command (`p-n24-1-exit.json`) gives no output.

When PO1 fails, instead:

18. The launcher's settings have no `trim` entry under `PostToolUseFailure`, and `trim_in` on `PostToolUseFailure-Bash-output-main.json` gives no output and writes no row.

## 17. Premises the first build verifies

Each is checked with a live Claude Code or OpenCode session by the premise scripts (`scripts/premises/`), not in CI, and recorded in `docs/premises.md` with its capture.

- **PO1** Claude Code 2.1.328: run `printf 'out-line\n'; printf 'err-line\n' >&2; exit 3` through the Bash tool, and capture the `PostToolUseFailure` hook input as `fixtures/claude-code/hooks/PostToolUseFailure-Bash-output-main.json`. Check (a) which field holds `out-line` and `err-line`, and in which order; (b) which hook output field, if any, replaces the text that the model sees after a failed call: try `updatedToolOutput` as a string and as a Bash-shaped object, and record the request body that follows. Result decides O§7.13.
- **PO2** Claude Code 2.1.328: a Bash `updatedToolOutput` with `stderr` set to `""` removes the original stderr from what the model sees (Q4, `docs/premises.md:35`, verified `stdout` only). Result decides O§7.8.
- **PO3** OpenCode 1.18.16 (the version of Q7, `docs/premises.md:38`): `tool.execute.after` for `bash` gives stdout and stderr together in `output.output`.
- **PO4** On both agents, the text that the model sees for a `result_view` reply is the reply text plus at most a fixed wrapper, so the reply's byte count is the read-back size within that wrapper.

## Change log

| Date | Section | Decided by | Reason |
|---|---|---|---|
| 2026-11-24 | O§6.5 item 3 | author's call (OS-2 review round 1) | the column check reads `PRAGMA table_xinfo` so a generated column counts as present |
| 2026-11-24 | O§8.1 | author's call | The `clean` row's commands did not give or record the required bytes; the row now names `-- --color always` (colors libtest stdout), `--no-local` (a local clone prints no `\r` progress) and `git diff --color=always` (the recorded command) (found in phase3 OS-17). |
| 2026-11-24 | O§8.2, O§8.3 | author's call | A case with no runner summary (libtest compile error, O§8.1) gets no change by O§7.6, so it cannot meet the 30 % failing floor or hold must_keep lines in a view; it is checked against the none row (found in phase3 OS-19). |
| 2026-11-24 | O§8.1 | operator | The diff view keeps only the whole sections that fit, so a diff case over 16 KiB cannot hold every `diff --git` line; a diff case's `must_keep` holds each `diff --git` line, and over the threshold only those of the sections the diff view shows whole; a diff under the threshold gets no diff view (O§7.7), so all its `diff --git` lines stay (found in phase3 OS-18). |
| 2026-11-24 | O§7.1, O§7.2 | operator | The parse missed commands that print a file: the wrappers `timeout` and `stdbuf`, the printers `egrep`, `fgrep`, `zcat`, `gawk`, `batcat`, `tac`, `git grep`, `git blame` and `$(<f)`, an `eval` string, and a shell that runs a script from a pipe; the lists and the unsure parse now hold them (found in phase3 OS-11 review). |
| 2026-11-24 | O§7.1, glossary | author's call | review of OS-40: newline after a pipe, backtick and 0< substitutions, eval --, builtin, -c only among leading options (the arguments of `-o`, `+o`, `-O`, `+O` skipped), redirect-only stage in zsh |
| 2026-11-25 | O§6.6 | author's call | the sample block's key amounts did not sum to its total, and its Read key kept out more bytes than were read back; replaced with figures a store can produce (OS-8). |
| 2026-11-25 | O§7.4 | author's call (pending operator ratification under O§15 item 2) | OS-12 review F4/F5: charset designations and ESC ESC left residue |
| 2026-11-25 | O§7.7, O§8.2, O§8.3 item 4 | author's call (lead) | OS-29 plan open items: diff marker text, diff preamble not shown, log level words and line numbers, log with no level line, `must_keep` checked with `contains`; `VIEW_CAP_BYTES` cite moved to `view.rs:6` |
