# FIX-31 — The AU30 restore filter keeps the value of an unlisted flag

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` AU30 (saved launch arguments: zygo removes positional prompt text; "the rest is stored as typed"; the builder classifies flags from the pinned `--help`) and W§7.2 (`member.launch.args`). Created 2026-11-23 at the FIX-24 merge (controller decision; FIX-24 diff review round 2 "Found, not fixed", `docs/work/phase2/nodes/FIX-24.md:232`). Problem: `args::filter` (`crates/zygo-workspace/src/args.rs:351`, also `filter_from` at `:358`) keeps an unlisted flag but removes the next token as a positional. In `walk` (`args.rs:256`), a long flag that is not in the table is kept alone and the walk goes to the next token (`args.rs:282-285`, `out.kept.push(token.clone()); continue;`; an unlisted letter of a short cluster is kept the same way at `:304-306`); the next token, when it does not start with `-`, then goes to the positional branch (`args.rs:270-273`) and is removed. So `zygo opencode -- --zzunknown X` is saved as `["--zzunknown"]` and `ws run` restores it without `X`. FIX-24 added `args::opencode_subcommand` (`args.rs:415-440`), which models the OpenCode 1.19.6 parser: the global booleans of `OPENCODE_GLOBAL_BOOLEANS` (`args.rs:407`; `-h`, `--help`, `-v`, `--version`, `--print-logs`, `--pure`, `fixtures/opencode/help.txt:34-38`) take no value (a next token `true` or `false` only), a `--no-<x>` flag takes no value, a token with `=` takes no next token, and every other unlisted flag takes the next token unless that token starts with `-` (`args.rs:426-437`). The evidence is the OpenCode 1.19.6 matrix in `docs/work/phase2/nodes/FIX-24.md:247-261` (FIX-24 diff review round 3, finding 2). Claude Code needs the same check: the executor finds the value rule of an unlisted flag of Claude Code from the pinned `fixtures/claude-code/help.txt` and the installed Claude Code 2.1.331 (`~/.local/share/claude/versions/2.1.331`), records the evidence in this node, and uses that rule. Change: `filter` (and `filter_from`, which share `walk`) uses for OpenCode the same value rule as `opencode_subcommand`, and for Claude Code the rule that matches the evidence, so an unlisted flag keeps its value on restore (both tokens kept as typed). The positional removal of AU30 stays for true positionals. The AU30 text and its change-log row say the unlisted-flag value rule. Edges: FIX-24 (it adds `opencode_subcommand` and changes `args.rs` and `tests/args.rs`); FIX-30 (E1, E2 and FIX-30 also change `docs/specs/2026-11-20-workspaces-design.md`; FIX-30 runs after E2, and E2 after E1, so this one edge orders FIX-31 after all three); D1 depends on FIX-31.
**Files:** `crates/zygo-workspace/src/args.rs`, `crates/zygo-workspace/tests/args.rs`, `docs/specs/2026-11-20-workspaces-design.md`, `fixtures/claude-code/options-2.1.331.txt` (new), `fixtures/claude-code/scan/mkfixture.py` (new), `fixtures/claude-code/scan/rootopts2.py` (new), `crates/zygo/src/cmd/member.rs` (test module only), `crates/zygo/tests/workspace_member.rs`, `crates/zygo-launch/tests/claude_code.rs` (the three new fixture files were written at plan time; the last three files were added by the plan, drift PD5 and finding minor 2)
**Consumes:** `opencode_subcommand` (FIX-24)
**Produces:** —
**Acceptance:** `cargo test -p zygo-workspace --locked` → all pass, including these tests in `crates/zygo-workspace/tests/args.rs`:
- OpenCode `filter` of `["--zzunknown", "X"]` keeps both tokens.
- Claude Code `filter` of `["--zzunknown", "X"]` gives `kept == ["--zzunknown"]` and removes `X`.
- Claude Code `filter` of `["--max-turns", "5", "--model", "opus"]` keeps all four tokens.
- OpenCode `["--print-logs", "msg"]` keeps `--print-logs` and removes `msg`.
- OpenCode `["--no-x", "msg"]` keeps `--no-x` and removes `msg`.
- OpenCode `["run", "-ip", "SECRET"]` keeps nothing.

## Plan

Planned 2026-11-23 on `worktree-zygo-phase-2` at `e49d5c5` (FIX-24 merged). The plan was revised three times the same day, by coordinator decisions (each an author's call):
- Revision 1: the 52 hidden Claude Code options with a value are in this node (D4).
- Revision 2: the one-action booleans are in this node (D7), and the plan has no "Found, not fixed" item (§5.4).
- Revision 3: a fresh plan review gave 4 blocking and 11 minor findings. Each was verified against the code or the binary ("Review findings" below) and is applied here.

No agent session was started and no network request was made. The evidence is a static read of the installed binaries with Python scripts run as `python3 -I`, each with the binary path as an argument:
- `fixtures/claude-code/scan/mkfixture.py` writes `fixtures/claude-code/options-2.1.331.txt`.
- `fixtures/claude-code/scan/rootopts2.py` lists a commander chain with its descriptions.
- Scratchpad scripts did the OpenCode scans (`ocopts.py`, and the inline object-form and boolean scans).

OpenCode was not run; the FIX-24 matrix is reused.

### Review findings (verified 2026-11-23)

Each finding was checked against the live code or binary. All 15 are confirmed, so no finding row is kept unchanged.

- **B1 confirmed: short clusters.** In the live walk, an unlisted letter keeps the rest of the cluster and ends it (`args.rs:304-306`: `kept_letters.push_str(&body[at..]); break;`). The first plan's step 5f then tested the whole token against the boolean set.
  - `-ip SECRET`: `i` is unlisted, so `p` (now a removed row) is never looked up, and `-ip` is not a boolean, so `SECRET` was kept.
  - `-ci "fix it"`: `c` is removed, `i` is unlisted, and `-ci` is not a boolean, so `fix it` was kept.
  - `-fi msg`: the same. `-f` is now a removed row (B2), so the cluster is removed.

  Both parsers give the value to the last letter. Commander takes the rest of the token for a value letter (`if(y?.required||y?.optional){this.#m(y,[k.slice(C?A+1:A)])…}`, `if(y&&!C){this.#m(y,[]),w=`-${k.slice(A)}`…}`, Claude Code @348940686). yargs-parser gives the next token only to the last letter. The new step 5f is applied.
- **B2 confirmed: OpenCode `mcp add` options.** `option("env",{describe:"environment variable for a local MCP server (KEY=VALUE)",type:"string",array:!0})` is at @125861521, and `option("header",{describe:"HTTP header for a remote MCP server (KEY=VALUE)",type:"string",array:!0})` at @125861627. The full scan of the OpenCode builders is in P5 and the D3 table.
- **B3 confirmed: `--forward-home-settings` and `--workload`.**
  - `--workload`: "Workload tag for billing-header attribution (cc_workload). Process-scoped; set by SDK daemon callers that spawn subprocesses for cron work. (only works with --print)" (@349008754).
  - `--forward-home-settings`: "… true = yes for this launch, standing in for the machine's stored choice (not saved). Requires --cloud or --environment." (@349019796).
  - The reviewer's offsets (349008743, 349019785) point at the `.addOption(` before each one, 11 bytes earlier. This plan uses the fixture convention: the offset of `new Ns(` or `.option(`.
- **B4 confirmed: booleans.** The reviewer named 10 booleans; each meets rule (a) or (b). The widened scan found 6 more. See D7 and its table.
- **Minor 1 confirmed: T8 expected red.** T8 now asserts the exact boolean set first, so that assertion fails first.
- **Minor 2 confirmed: `--remote`.** `--cloud` is `flag(&["--cloud"], Arity::Optional, Class::Session)` (`args.rs:56`), so `--remote` gets the same row. The zygo-launch check that each session flag is in the pinned help is at `crates/zygo-launch/tests/claude_code.rs:1733-1736`. W§8.3 lists the session flags at `docs/specs/2026-11-20-workspaces-design.md:394`.
- **Minor 3 confirmed: `run --title`.** "title for the session (uses truncated prompt if no value provided)" (@110096307) is user text, so it is `Remove`.
- **Minor 4 accepted.** A1 and D2 now say: the table is complete for 2.1.331, and on a newer binary an option that is not in the table follows the unlisted rule.
- **Minor 5 done.** `mkfixture.py` finds the chain by its two anchors (`.name("claude").description(` and `us("run_main_options_built")`), with no hard-coded offsets. It writes every header line, and takes the binary path as an argument (the header names only its base name). The fixture is its exact output (139 options). Both scripts are now in `fixtures/claude-code/scan/`, and neither holds a home path.
- **Minor 6 confirmed.** At @348943705 the text is `g.length>0&&!this.#_)this.#P(g[0]);…`. The first plan cited @348943129, the start of the `this.args=w.concat(g)` statement before it. E2 now uses 348943705.
- **Minor 7 accepted.** `--dangerously-load-development-channels` is `Remove`: "For local channel development only. Shows a confirmation dialog at startup." (@349016747).
- **Minor 8 done.** Step 5d gives the exact old and new doc block (`args.rs:409-414`).
- **Minor 9 done.** Step 9 runs the three packages that the node changes.
- **Minor 10 done.** Step 7 gives each mutation as an exact old text and new text, with the test that must go red.
- **Minor 11 done.**
  - Bodies are replaced by signatures and rules, except where a mutation needs the exact text or where the tests do not decide the algorithm (the cluster walk).
  - The table rows stay, because they are spec data.
  - T8 shares the parser of `claude_options` through `claude_spec`.
  - The Acceptance line names each agent's rule.

### Premise check (against `e49d5c5`)

- P1. Holds. Lines in `crates/zygo-workspace/src/args.rs`:
  - `walk` is at `:256`;
  - the positional branch is `:270-273`;
  - the unlisted long-flag branch is `:282-285`;
  - the cluster branch is `:298-333`;
  - the common outcome loop is `:334-342`;
  - `filter` `:351`, `filter_from` `:358`, `removable` `:365`, `removed_labels` `:374`;
  - `OPENCODE_GLOBAL_BOOLEANS` `:407`;
  - the `opencode_subcommand` doc block `:409-414`, its body `:415-440`, its value rule `:426-437`;
  - `CLAUDE_CODE` `:49-102` (`--cloud` `:56`, `--teleport` `:57`, `-p`/`--print` `:58`, `--bare` `:101`), `OPENCODE` `:105-121`;
  - `settle` `:200` (`Arity::None => {}` at `:213`).
- P2. Callers (search over `crates/`):
  - `filter`: only `record_start` (`crates/zygo/src/cmd/member.rs:397`).
  - `filter_from`: only `fresh_member` (`crates/zygo-workspace/src/up.rs:664`).
  - `removed_labels`: only `zygo config check` (`crates/zygo/src/cmd/config.rs:104`).
  - `removable`: tests only.
  - The same walk serves:
    - `has_session_arg` (`crates/zygo-launch/src/opencode.rs:489`, `crates/zygo-launch/src/claude_code.rs:696`, `crates/zygo/src/cmd/ws.rs:206`, and the pre-assign decision `crates/zygo/src/cmd/member.rs:65`);
    - `occurrences` (`claude_code.rs:90`, `:95`, `crates/zygo/src/cmd/loadout.rs:253`, `member.rs:677`);
    - `flags` (`claude_code.rs:101`);
    - `is_flag` (`loadout.rs:277`);
    - the Claude Code arm of `events_off` (`args.rs:391`).
  - `opencode_subcommand`: `events_off` (`args.rs:396`) and the launcher (`crates/zygo-launch/src/launcher.rs:717`).
  - `session_flags`: tests only (`tests/args.rs:379`, `crates/zygo-launch/tests/claude_code.rs:1732-1737`, `crates/zygo-launch/tests/opencode.rs:1236`).
  - No source uses `--tmux`, `--workload`, `--forward-home-settings` or `"--remote"`; only comments and a help-text check do (`claude_code.rs:704`, `tests/claude_code.rs:1743-1745`).
- P3. Holds. The FIX-24 matrix covers each case of the OpenCode value rule, so OpenCode was not run.
- P4. The parse context. `opencode_subcommand` models the parse before a subcommand. `ws run` restores the default command, which yargs parses again with its builder: `runCommand` → `applyBuilderUpdateUsageAndParse` → `t.builder`, then `runYargsParserAndExecuteCommands(null,…)` (`~/.opencode/bin/opencode` @125750413). So the two callers pass different boolean sets to one function (D1).
- P5. OpenCode scan. Every yargs `.option("x",{…})` of the binary was read (scratchpad `ocopts.py`), and the object-form declarations too. Each option with a value whose value can hold a secret, a URL, user text or content, or a path (in doubt), becomes a removed row (D3 table). These stay unlisted and keep their value by the rule, because they are plain settings:
  - `run` `--command` (@110095453), `--format` (@110096072), `--variant` (@110096987);
  - `login` `--provider` (@125816749), `--method` (@125816867);
  - `agent create` `--mode` (@125821540);
  - `upgrade` `--method` (@125825019);
  - `debug files`/`search` `--glob`, `--limit` (@125838991, @125839064, @125839539, @125839602);
  - `debug agent` `--tool` (@125841592);
  - `stats` `--days`, `--tools`, `--models`, `--project` (@125844144-125844439);
  - `session list` `--max-count`, `--format` (@125888312, @125888401);
  - `models` `--format` (@125889668).

  The object form has only `port`, `hostname`, `mdns-domain` and `cors` (@125833507-125833878), which are already table rows.
- P6. OpenCode booleans. The `[boolean]` options of every command (@110095539-125894356) form `OPENCODE_COMMAND_BOOLEANS` (D1). A boolean that the filter did not know would take the next token, which can be a `run` message or other user text. `-f` (`plugin --force`) is not in the set, because the `-f`/`--file` row of the table decides `-f` first.
- P7. Claude Code: see the evidence below. The table becomes complete for 2.1.331 (D2, D4, D7).
- PD5. Files drift: three test cases outside the first Files line assert the saved `["--attach"]` of `run --attach <url>`:
  - `crates/zygo/src/cmd/member.rs:1460-1462`;
  - `crates/zygo/tests/workspace_member.rs:486-507`;
  - `workspace_member.rs:589-590`.

### Claude Code evidence (installed 2.1.331)

- E1. Root parse (@348940686, `parseOptions`):
  - A registered option takes its declared value: `<x>` takes the next token whatever it is; `[x]` takes it unless it starts with `-`; `<x...>` takes tokens up to the first that starts with `-`.
  - In a short cluster, a value letter takes the rest of the token. A boolean letter goes on with the next letter.
  - An unregistered token that starts with `-` goes, with the later non-option tokens, to the `unknown` list.
- E2. `if(g.length>0&&!this.#_)this.#P(g[0])` (@348943705) prints `error: unknown option '<flag>'` and exits 1, before the `preAction` hooks and the action. So no session starts.
- E3. `#_` is set only by `allowUnknownOption()` (@348940102). Its callers are `remote-control` (@350109352) and `plugin test` (@369263625), not the root.
- E4. The root registers 139 options (`fixtures/claude-code/options-2.1.331.txt`, written by `mkfixture.py`; chain 348994202-349021969). No later code adds a root option (`rootCommand.option(`, `rootCommand.addOption(`: 0 matches).
- E5. Of the 139:
  - the options with a value that the table does not hold are 52, all hidden (D4 table);
  - every option with a value in the pinned help is already in the table;
  - the booleans that must be removed are in the D7 table.
- Rule (D2). The table is complete for Claude Code 2.1.331 (from the fixture), so a flag that the table does not hold takes no value:
  - an unregistered flag stops the start (E2), and removing its next token keeps a possible prompt word out of `workspace.json` (W-C7);
  - a registered boolean has no value.

  On a newer binary, an option that is not in the table follows the same rule: it is kept as typed and takes no value, so a new option with a value is saved without its value. The brief's KEEP fall-back does not apply, because the rule is shown. KEEP would save prompt text.

### Decisions (author's calls; D3-D7 tables by coordinator decision)

- D1. One value function: `opencode_takes_value(token, after, booleans)`, private.
  - Its rule:
    1. A token with `=` or a `--no-` prefix takes no value.
    2. The name is the token for a long flag, and `-<last letter>` for a short token (B1: the value goes to the last letter).
    3. A name in `booleans` takes the next token only when it is `true` or `false`.
    4. Any other name takes the next token unless it starts with `-`.
  - `opencode_subcommand` passes `OPENCODE_GLOBAL_BOOLEANS`. This changes it only for a cluster whose last letter is `h` or `v` (`-xh X attach` now gives the subcommand `X`), which matches yargs.
  - The walk passes `OPENCODE_COMMAND_BOOLEANS` through `unlisted_takes_value`, for an unlisted long flag or an unlisted last letter of a cluster.
  - T4 asserts that the two agree, apart from the boolean set.
- D2. Claude Code: an unlisted flag takes no value. The table is complete for 2.1.331, and the rule for a newer binary is stated in the evidence section.
- D3. OpenCode removed rows: the D3 table. `-f`/`--file`, `--env` and `--header` are arrays in yargs, so their rows are `Arity::Variadic`, and every value up to the next flag is removed. `-p` also names `login --provider`, so `providers login -p x` removes `x` (a plain value lost, which is safe).
- D4. Hidden Claude Code options with a value: the D4 table, with 13 `Keep`, 38 `Remove` and 1 `Session`.
  - `Remove`: the value can hold a secret, content, a prompt, a path to user content or a URL, or it is data of one launch.
  - `Keep`: the value is a plain setting.
  - In doubt: `Remove`.
  - `--remote` is `Session`, as `--cloud` is (finding minor 2).
- D5. OpenCode plain-setting values (P5 list) are kept as typed.
- D6. Coverage (T8) reads the fixture.
  - It asserts the count of 139, then the exact set of boolean table rows.
  - Each option with a value must be a table row and must filter by its arity and class.
  - A boolean that is not in the table filters as kept, with no value.
  - A boolean that is in the table is removed. The one exception is `--bare`, a `Keep` row that exists so that `flags` and `occurrences` report it (`args.rs:100-101`).
- D7. A Claude Code boolean gets a row with `Arity::None` and `Class::Remove` when any of these is true:
  - (a) it is data of one launch, or a host, SDK, server or parent process sets it;
  - (b) a restore that resumes would do its action again (a turn, setup hooks, a query, a new tmux session), or it does one action and exits;
  - (c) it belongs to a removed value flag;
  - (d) it is not sure.

  Method: every boolean of the fixture, visible and hidden, was read with its description (`rootopts2.py … full`) and, where the description was not enough, the code it reaches. Result: 19 new rows (D7 table); `-c`/`--continue`, `--fork-session` (session rows) and `-p`/`--print` (a removed row) already exist; `--bare` stays `Keep`.

  These booleans stay unlisted (kept, no value), because they are user settings:
  - `-d2e`/`--debug-to-stderr`, `--verbose`, `--safe-mode`;
  - `--include-hook-events`, `--include-partial-messages`, `--forward-subagent-text`, `--replay-user-messages`, `--no-session-persistence` (output settings of `--print`, which is removed);
  - `--dangerously-skip-permissions`, `--allow-dangerously-skip-permissions` (AU70 prints them);
  - `--restricted`, `--exclude-dynamic-system-prompt-sections`, `--include-unsupported-models`, `--ide`, `--strict-mcp-config`, `--disable-slash-commands`, `--chrome`, `--no-chrome`, `--enable-auto-mode`, `--brief`, `--ax-screen-reader`.

  `-h`/`--help` and `-v`/`--version` are left out: a launch with them never saves a session. `Arity::None` exists (`args.rs:11-20`), so the enum does not change. With it, `settle` takes no value token (`args.rs:213`), and `unlisted_takes_value` is never called for a table row, so the next token stays its own argument.

#### D3 table: OpenCode 1.19.6 removed rows (all `Class::Remove`)

| Flag | Arity | Reason | Binary offset |
|---|---|---|---|
| `-p`, `--password` | Required | secret: basic-auth password (`run`, `attach`) | 110096527, 125876635 |
| `-u`, `--username` | Required | credential: basic-auth user name (`run`, `attach`) | 110096644, 125876752 |
| `--attach` | Required | URL of a server; can hold user info (`run`) | 110096417 |
| `--token` | Required | secret: GitHub token (`github run`) | 125868442 |
| `--url` | Required | URL; can hold user info (`mcp add`) | 125861452 |
| `--env` | Variadic | secret: `KEY=VALUE` environment of an MCP server (`mcp add`, array) | 125861521 |
| `--header` | Variadic | secret: `KEY=VALUE` HTTP header, often a token (`mcp add`, array) | 125861627 |
| `-f`, `--file` | Variadic | path to user content attached to the message (`run`, array) | 110096215 |
| `--title` | Required | user text: session title, else the truncated prompt (`run`) | 110096307 |
| `--dir` | Required | path; on attach, a remote path of one launch (`run`, `attach`; in doubt) | 110096775, 125876295 |
| `--description` | Required | user text: what the agent should do (`agent create`) | 125821466 |
| `--path` | Required | path for the agent file (`agent create`; in doubt) | 125821382 |
| `--query` | Required | user text: a file query (`debug files`) | 125838923 |
| `--params` | Required | content: tool params as JSON (`debug agent`) | 125841656 |
| `--event` | Required | GitHub mock event of one run (`github run`; in doubt) | 125868360 |
| `--cwd` | Required | path: working directory (`acp`; in doubt) | 125883888 |

#### D4 table: hidden Claude Code 2.1.331 options with a value

Offsets are those of the fixture (`new Ns(` or `.option(`).

| Flag | Arity | Class | Reason | Binary offset |
|---|---|---|---|---|
| `--thinking <mode>` | Required | Keep | plain setting: `enabled`, `adaptive` or `disabled` | 348999032 |
| `--thinking-display <display>` | Required | Keep | plain setting: a choice | 348999185 |
| `--max-thinking-tokens <tokens>` | Required | Keep | plain setting: a number | 348999306 |
| `--max-turns <turns>` | Required | Keep | plain setting: a number | 348999499 |
| `--task-budget <tokens>` | Required | Keep | plain setting: a positive integer | 348999859 |
| `--permission-prompt-tool <tool>` | Required | Keep | plain setting: an MCP tool name | 349001899 |
| `--append-subagent-system-prompt <prompt>` | Required | Remove | prompt text | 349003578 |
| `--append-subagent-system-prompt-file <file>` | Required | Remove | path to user content (a prompt file) | 349003850 |
| `--plan-mode-instructions <instructions>` | Required | Remove | prompt text | 349004063 |
| `--inherit-permission-mode <mode>` | Required | Remove | set by a parent session for one launch; can carry `bypassPermissions` past the AU70 printout | 349004783 |
| `--watch-artifact <artifact>` | Required | Remove | id or URL; a URL can hold a token | 349005331 |
| `--watch-artifact-no-autoreact <artifact>` | Required | Remove | same as `--watch-artifact` | 349005486 |
| `--prefill <text>` | Required | Remove | prompt text | 349005559 |
| `--deep-link-repo <slug>` | Required | Remove | data of one deep-link launch | 349005776 |
| `--deep-link-last-fetch <ms>` | Required | Remove | data of one deep-link launch | 349005903 |
| `--prefill-b64 <b64>` | Required | Remove | encoded prompt text | 349006104 |
| `--deep-link-cwd-b64 <b64>` | Required | Remove | encoded path of one deep-link launch | 349006288 |
| `--resume-session-at <message id>` | Required | Remove | belongs to one truncating resume | 349006737 |
| `--resume-drops-turn <message id>` | Required | Remove | belongs to one truncating resume | 349007005 |
| `--rewind-files <user-message-id>` | Required | Remove | one action: restores files and exits | 349007639 |
| `--fast-mode <on\|off>` | Required | Keep | plain setting | 349008285 |
| `--workload <tag>` | Required | Remove | process-scoped billing tag set by SDK daemon callers for cron work; only with `--print` (B3) | 349008754 |
| `--client-data-url <url>` | Required | Remove | URL of a signed document; the help says to keep it out of the process list | 349009065 |
| `--managed-settings <json>` | Required | Remove | content: policy settings JSON | 349009606 |
| `--project-config-root <dir>` | Required | Remove | path to user content, set by a host | 349010263 |
| `--plugin-dir-no-mcp <path>` | Required | Remove | path to user content; its caller owns the MCP connections | 349011880 |
| `--advisor <model>` | Required | Keep | plain setting: a model name | 349015159 |
| `--proactivity <level>` | Required | Keep | plain setting: a choice | 349015614 |
| `--messaging-socket-path <path>` | Required | Remove | socket of one running server | 349016117 |
| `--channels <servers...>` | Variadic | Keep | plain setting: MCP server names | 349016572 |
| `--dangerously-load-development-channels <servers...>` | Variadic | Remove | development only; shows a confirmation dialog at startup (minor 7) | 349016747 |
| `--agent-id <id>` | Required | Remove | teammate identity of one launch | 349016964 |
| `--agent-name <name>` | Required | Remove | teammate identity of one launch | 349017034 |
| `--team-name <name>` | Required | Remove | teammate identity of one launch | 349017112 |
| `--agent-color <color>` | Required | Remove | teammate identity of one launch | 349017203 |
| `--parent-session-id <id>` | Required | Remove | id of another session | 349017376 |
| `--lead-verify-key <time.key>` | Required | Remove | key material | 349017481 |
| `--teammate-mode <mode>` | Required | Keep | plain setting: a choice | 349017635 |
| `--agent-type <type>` | Required | Remove | teammate identity of one launch | 349017805 |
| `--sdk-url <url>` | Required | Remove | URL of an SDK endpoint; can hold a token or user info | 349017897 |
| `--runner-declares <names>` | Required | Remove | set by a runner host for one launch | 349018055 |
| `--session-began-as-copy <value>` | Required | Remove | sent by the server for one session | 349018819 |
| `--initialize-fields <names>` | Required | Remove | SDK only; needs `--await-initialize` | 349019215 |
| `--forward-home-settings <true\|false>` | Required | Remove | data of one launch ("not saved"); requires `--cloud` or `--environment` (B3) | 349019796 |
| `--remote [description\|session_id\|url]` | Optional | Session | hidden alias of `--cloud` ("Deprecated alias for --cloud"), which is a session row (minor 2) | 349020222 |
| `--attach-serve <session_id>` | Required | Remove | session id; started by the desktop app, not for interactive use | 349020325 |
| `--pool <pool_id>` | Required | Keep | alias of `--environment`, which the table keeps | 349020640 |
| `--correlation-id <id>` | Required | Remove | opaque id of one work order | 349020728 |
| `--metadata <key=value>` | Required | Remove | free labels; can hold any text | 349020884 |
| `--ref <ref>` | Required | Remove | needs `--cloud` or `--environment`; in doubt | 349021104 |
| `--on-branch <branch>` | Required | Remove | needs `--cloud` or `--environment`, as `--ref` | 349021269 |
| `--rc [name]` | Optional | Keep | alias of `--remote-control`, which the table keeps | 349021749 |

#### D7 table: Claude Code 2.1.331 boolean rows (all `Arity::None`, `Class::Remove`)

| Flag | Rule | Evidence (description or code) | Binary offset |
|---|---|---|---|
| `--init-only` | (b) | "Run Setup and SessionStart:startup hooks, then exit"; the start is non-interactive (`nonInteractive:o.nonInteractive\|\|Boolean(e.initOnly)`, @348427686) | 348996561 |
| `--desktop` | (b) | "Open in the Claude Desktop app instead of the terminal"; the action runs `if(R.desktop){… launchDesktopFromCli …; return …}` (@349012601) | 349010683 |
| `--bg`, `--background` | (b) | "Start the session in the background and return immediately"; the action hands the launch to the background handler (`if(e.heldBgLaunchArgs!==void 0){…}`, @349012931) | 349015771 |
| `--init` | (b) | "Run Setup hooks with init trigger, then continue": a restore would run the setup hooks again | 348996471 |
| `--maintenance` | (b) | "Run Setup hooks with maintenance trigger, then continue": same | 348996659 |
| `--session-mirror` | (a) | "Emit transcript_mirror frames on stdout (SDK-internal; set by ProcessTransport …)" | 348997684 |
| `--await-claim` | (a) | "Start as a pre-warmed spare for an SDK host … (SDK-internal …)" | 348997847 |
| `--await-initialize` | (a) | "Read the initialize control request from stdin … Pass it only from the process that writes that request …" | 348998359 |
| `--enable-auth-status` | (a) | "Enable auth status messages in SDK mode" | 349000676 |
| `--deep-link-origin` | (a), (c) | "Signal that this session was launched from a deep link"; its value flags are removed (D4) | 349005668 |
| `--reply-on-resume` | (a), (b) | "When resuming, immediately query … (set by /background mid-turn …)" | 349007429 |
| `--allow-other-models` | (c) | "With --client-data-url: …"; `--client-data-url` is removed (D4) | 349009368 |
| `--managed-settings-in-initialize` | (a), (c) | "… (SDK use only; requires --await-initialize)" | 349009738 |
| `--host-auth-is-org-sign-in` | (a) | "The ANTHROPIC_AUTH_TOKEN this host manages is … (desktop hosts only)" | 349010003 |
| `--ignore-persisted-trust` | (a) | `new Ns(ela,"Honor no persisted workspace trust record in this process (set by an IDE host …)")`, `var ela="--ignore-persisted-trust"` (@327986592) | 349011539 |
| `--tmux` | (b), (d) | "Create a tmux session for the worktree (requires --worktree)"; the action runs `execHeldTmuxLaunch` (@349013731): a restore in a zygo pane would start another tmux session | 349014983 |
| `--agents-view-if-allowed` | (a), (d) | no description; when the fleet gate allows it, the action opens `agentsCommandHandler` in place of a session (@349013731) | 349015712 |
| `--plan-mode-required` | (a) | "Require plan mode before implementation", in the teammate block that a team lead passes (`--agent-id` … `--agent-type`) | 349017279 |
| `--hold-repository-while-parked` | (a) | "Sent by a host on a spare that names a --project-config-root …" | 349018227 |

### Interfaces

Consumes (live signatures):
- `opencode_subcommand` — `pub fn opencode_subcommand(args: &[String]) -> Option<(usize, &str)>` (`args.rs:415`, FIX-24). The body and doc change; the signature stays; the behaviour changes only for a cluster that ends in `h` or `v` (D1).
- `OPENCODE_GLOBAL_BOOLEANS` — `const OPENCODE_GLOBAL_BOOLEANS: &[&str] = &["-h", "--help", "-v", "--version", "--print-logs", "--pure"];` (`args.rs:407`, private, unchanged).
- `is_flag` — `pub fn is_flag(agent: Agent, token: &str) -> bool` (`args.rs:444`, used by T8).

Produces: — (no public item is added or changed). Private items added in `args.rs`:
- `OPENCODE_COMMAND_BOOLEANS` — `const OPENCODE_COMMAND_BOOLEANS: &[&str]`
- `opencode_takes_value` — `fn opencode_takes_value(token: &str, after: Option<&str>, booleans: &[&str]) -> bool`
- `unlisted_takes_value` — `fn unlisted_takes_value(agent: Agent, token: &str, after: Option<&str>) -> bool`

### Implementer rules

- Every cargo command sets `CARGO_TARGET_DIR=/home/kagemartin/Work/zygo/.claude/worktrees/zygo-phase-2/.worktrees/phase2/FIX-31-target`. Never build under `/tmp`.
- Run only the scoped commands of each step. No pipe and no filter (`head`, `tail`, `grep`, `sed`, `awk`, `cut`, `tee`, `rg`) on any output.
- Do not run Claude Code or OpenCode. No network request.
- `fixtures/claude-code/options-2.1.331.txt` is the exact output of `python3 -I fixtures/claude-code/scan/mkfixture.py <binary> <output>`. Nobody edits it by hand; T8 checks its count.
- The tmux tests of `workspace_member.rs` use the `Harness` of `crates/zygo/tests/common/workspace.rs` (private sockets). Without tmux 3.2 they print a skip reason and pass.
- Operator rules: no TTL, expiry, limit or clean-up that ends user work. No Claude attribution. Commit trailers: `Zygo-Session: <session>` and `Zygo-Work: FIX-31`, each on its own line at the end.
- Before step 1, if FIX-30 or E1/E2 merged after `e49d5c5`, read the AU30 row, the W§7.2 `member.launch` row and W§8.3 again. Each edit of step 8 anchors on the quoted text. If a quoted text is gone, stop and report.

### Steps

- [ ] 1. Premise re-check, no edit. Confirm P1 and P2 (`act101` `references` on `filter`, `filter_from`, `removed_labels`, `removable`, `opencode_subcommand`, `is_flag`, `session_flags`, `has_session_arg`) and PD5. If a new caller exists, stop and report.
- [ ] 2. Tests first, `crates/zygo-workspace/tests/args.rs`.
  - **Imports and helpers:**
    - Add `is_flag` to the `use zygo_workspace::args::{…}` line.
    - Split the row parser of `claude_options` (`:162-179`) into `fn claude_spec(spec: &str) -> (Vec<String>, Arity)`. It takes the names (the words that start with `-`, a `,` at the end cut) and the arity (first other word: none → `None`, `[` → `Optional`, ends with `...>` → `Variadic`, else `assert!(m.starts_with('<'), "{spec}")` and `Required`).
    - `claude_options` becomes `… .filter(|l| l.starts_with("  -")).map(|l| claude_spec(l[2..].split("  ").next().unwrap())).collect()`.
    - Add `fn binary_options() -> Vec<(Vec<String>, Arity)>`: `fixture("claude-code/options-2.1.331.txt").lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| claude_spec(l.split('\t').nth(2).unwrap())).collect()`.
    - Add the consts:
      - `CC_HIDDEN_REMOVED: &[&str]`: the 38 `Remove` flags of the D4 table and `--remote`, by long name;
      - `CC_PATH: &[&str] = &["--system-prompt-file", "--append-system-prompt-file"]`;
      - `CC_BOOL_REMOVED: &[&str]`: the 19 flag names of the D7 table, with both `--bg` and `--background` (20 names).
  - Add the tests below after `opencode_subcommand_follows_the_opencode_1_19_6_parser`. Each assertion message names its case.
  - T1 `an_unlisted_opencode_flag_keeps_its_value` (`// FIX-31, AU30: an OpenCode flag that the table does not hold takes the next token unless it starts with -`):
    - `oc(&["--zzunknown", "X"]) == Filtered { kept: s(&["--zzunknown", "X"]), dropped_flags: vec![] }` and `removable(Agent::OpenCode, &s(&["--zzunknown", "X"])) == s(&[])`;
    - `oc(&["--zzunknown", "X", "proj"]).kept == s(&["--zzunknown", "X"])` and `removable(..) == s(&["proj"])`;
    - `oc(&["--zzunknown", "--model", "m"]).kept == s(&["--zzunknown", "--model", "m"])`;
    - `oc(&["--zzunknown"]).kept == s(&["--zzunknown"])`;
    - `oc(&["--zzunknown=X", "proj"]).kept == s(&["--zzunknown=X"])`;
    - `oc(&["-x", "X"]).kept == s(&["-x", "X"])`, `oc(&["-xy", "X"]).kept == s(&["-xy", "X"])`, `oc(&["-r", "abc"]).kept == s(&["-r", "abc"])`;
    - `oc(&["-x=X", "proj"]).kept == s(&["-x=X"])`;
    - `oc(&["--zzunknown", "X", "--", "Y"]).kept == s(&["--zzunknown", "X"])` and `removable(..) == s(&["--", "Y"])`;
    - `oc(&["--zzunknown", "--", "Y"]).kept == s(&["--zzunknown"])`;
    - `removed_labels(Agent::OpenCode, &s(&["--zzunknown", "X", "proj"]), Path::new("")) == s(&["<positional>"])`;
    - `filter_from(Agent::OpenCode, &s(&["--zzunknown", "X"]), &std::env::current_dir().unwrap()) == oc(&["--zzunknown", "X"])`.
  - T2 `an_opencode_boolean_takes_only_true_or_false` (`// FIX-31, AU30, W-C7: a boolean of an OpenCode command keeps no value; a run message is never saved`):
    - For each `n` of `["-h", "--help", "-v", "--version", "--print-logs", "--pure", "--mdns", "--auto", "--mini", "--replay", "--share", "--thinking", "-i", "--interactive", "--yolo", "--dangerously-skip-permissions", "--demo", "--major", "--keep-config", "--keep-data", "--dry-run", "--force", "--verbose", "--refresh", "--sanitize", "-g", "--global"]`: `oc(&[n, "msg"]).kept == s(&[n])`, `removable(Agent::OpenCode, &s(&[n, "msg"])) == s(&["msg"])`, `oc(&[n, "true"]).kept == s(&[n, "true"])`, `oc(&[n, "false"]).kept == s(&[n, "false"])`.
    - `oc(&["--print-logs", "msg"]).kept == s(&["--print-logs"])`.
    - `oc(&["--no-x", "msg"]).kept == s(&["--no-x"])` and `removable(..) == s(&["msg"])`; `oc(&["--no-x", "true"]).kept == s(&["--no-x"])`.
    - `oc(&["--print-logs", "true", "msg"]).kept == s(&["--print-logs", "true"])`.
    - `oc(&["run", "--thinking", "fix it"]).kept == s(&["--thinking"])` and `removable(..) == s(&["run", "fix it"])`.
    - `oc(&["run", "-i", "fix it"]).kept == s(&["-i"])`.
    - Clusters (B1):
      - `oc(&["run", "-ci", "fix it"]) == Filtered::default()` and `removable(..) == s(&["run", "-ci", "fix it"])`;
      - `oc(&["run", "-fi", "msg"]) == Filtered::default()` and `removable(..) == s(&["run", "-fi", "msg"])`;
      - `oc(&["-ih", "msg"]).kept == s(&["-ih"])` and `removable(..) == s(&["msg"])` (a cluster of booleans only);
      - `oc(&["-xi", "X"]).kept == s(&["-xi"])` and `removable(..) == s(&["X"])` (the last letter is a boolean);
      - `oc(&["-ix", "X"]).kept == s(&["-ix", "X"])` (the last letter is not a boolean).
  - T3 `opencode_secret_options_are_removed_with_their_value` (`// FIX-31 D3, AU30, W-C7: no secret, URL user info or user text of an OpenCode subcommand is saved`):
    - For each `a` of these, `oc(a) == Filtered::default()` and `removable(Agent::OpenCode, &s(a)) == s(a)`:
      - `["-p", "pw"]`, `["--password", "pw"]`, `["--password=pw"]`, `["-ppw"]`, `["--password", "-pw"]`;
      - `["-u", "me"]`, `["--username", "me"]`;
      - `["--attach", "https://u:p@h"]`, `["--attach=https://u:p@h"]`, `["--token", "ghp_x"]`, `["--url", "https://u:p@h"]`;
      - `["--env", "A=1", "B=2"]`, `["--env", "A=1", "--env", "B=2"]`, `["--env=A=1"]`;
      - `["--header", "Authorization: Bearer x"]`, `["--header", "A: 1", "--header", "B: 2"]`, `["--header=A: 1"]`;
      - `["-f", "a.txt", "b.txt"]`, `["--title", "fix it"]`, `["--dir", "/x"]`, `["--description", "do it"]`, `["--path", "p"]`, `["--query", "q"]`, `["--params", "{}"]`, `["--event", "e"]`, `["--cwd", "/x"]`.
    - `oc(&["attach", "http://h", "-p", "SECRET", "--mini"]).kept == s(&["--mini"])`.
    - `oc(&["run", "--attach", "https://u:p@h", "msg"]) == Filtered::default()`.
    - Clusters (B1): `oc(&["run", "-ip", "SECRET"]) == Filtered::default()` and `removable(..) == s(&["run", "-ip", "SECRET"])`; `oc(&["-pi", "SECRET"]) == Filtered::default()` and `removable(..) == s(&["-pi", "SECRET"])`.
    - `removed_labels(Agent::OpenCode, &s(&["-p", "SECRET"]), Path::new("")) == s(&["-p"])`; `removed_labels(Agent::OpenCode, &s(&["--attach", "https://u:p@h"]), Path::new("")) == s(&["--attach"])`; `removed_labels(Agent::OpenCode, &s(&["-ip", "SECRET"]), Path::new("")) == s(&["-p"])`.
  - T4 `filter_and_opencode_subcommand_share_one_value_rule` (`// FIX-31 D1: one value function; only the boolean set differs, by the parse context`):
    - For each `f` of `["--zzunknown", "-x", "-xy", "-xh", "--zzunknown=V", "-x=V", "--no-zz", "--no-replay", "--print-logs", "--pure", "-h", "--help", "-v", "--version"]` and each `x` of `["X", "true", "false"]`: `removable(Agent::OpenCode, &s(&[f, x])).is_empty() == (opencode_subcommand(&s(&[f, x, "attach"])) == Some((2, "attach")))`, message `"{f} {x}"`.
    - For each `f` of `["--mini", "--mdns", "--auto", "--thinking"]`: `removable(Agent::OpenCode, &s(&[f, "X"])) == s(&["X"])` and `opencode_subcommand(&s(&[f, "X", "attach"])) == Some((2, "attach"))`.
  - T5 `an_unlisted_claude_code_flag_takes_no_value` (`// FIX-31 D2, AU30: the table is complete for 2.1.331; Claude Code stops at a flag it does not register`):
    - `cc(&["--zzunknown", "X"]) == Filtered { kept: s(&["--zzunknown"]), dropped_flags: vec![] }` and `removable(Agent::ClaudeCode, &s(&["--zzunknown", "X"])) == s(&["X"])`;
    - `cc(&["--zzunknown", "true"]).kept == s(&["--zzunknown"])`;
    - `cc(&["--verbose", "fix it"]).kept == s(&["--verbose"])` and `removable(..) == s(&["fix it"])`;
    - `cc(&["-x", "X"]).kept == s(&["-x"])`.
    - Clusters (B1):
      - `cc(&["-pd", "hooks"]) == Filtered::default()`, `removable(..) == s(&["-pd", "hooks"])` and `removed_labels(Agent::ClaudeCode, &s(&["-pd", "hooks"]), Path::new("")) == s(&["-p", "-d"])`;
      - `cc(&["-cx", "X"]) == Filtered::default()` and `removable(..) == s(&["-cx", "X"])`;
      - `cc(&["-xd", "hooks"]).kept == s(&["-xd", "hooks"])` (the last letter `-d` takes its optional value);
      - `cc(&["-xyz", "X"]).kept == s(&["-xyz"])` and `removable(..) == s(&["X"])`.
  - T6 `a_hidden_claude_code_setting_keeps_its_value` (`// FIX-31 D4, AU30`):
    - `cc(&["--max-turns", "5", "--model", "opus"]) == Filtered { kept: s(&["--max-turns", "5", "--model", "opus"]), dropped_flags: vec![] }` and `removable(Agent::ClaudeCode, &s(&["--max-turns", "5", "--model", "opus"])) == s(&[])`;
    - `cc(&["--max-turns", "5", "fix it"]).kept == s(&["--max-turns", "5"])` and `removable(..) == s(&["fix it"])`;
    - `cc(&["--max-turns=5"]).kept == s(&["--max-turns=5"])`;
    - `cc(&["--channels", "a", "b", "--model", "m"]).kept == s(&["--channels", "a", "b", "--model", "m"])`;
    - `cc(&["--rc", "name", "fix it"]).kept == s(&["--rc", "name"])` and `cc(&["--rc", "--model", "m"]).kept == s(&["--rc", "--model", "m"])`.
  - T7 `a_hidden_claude_code_secret_option_drops_its_value` (`// FIX-31 D4, AU30, W-C7`):
    - For each `a` of `[["--prefill", "fix it"], ["--prefill-b64", "Zm9v"], ["--lead-verify-key", "1.k"], ["--sdk-url", "wss://u:p@h"], ["--sdk-url=wss://u:p@h"], ["--managed-settings", "{\"x\":1}"], ["--rewind-files", "m1"], ["--workload", "cron"], ["--forward-home-settings", "true"], ["--dangerously-load-development-channels", "a", "b"], ["--remote", "fix it"]]`: `cc(a) == Filtered::default()` and `removable(Agent::ClaudeCode, &s(a)) == s(a)`.
    - `cc(&["--prefill", "fix it", "--model", "m"]).kept == s(&["--model", "m"])`.
    - `removed_labels(Agent::ClaudeCode, &s(&["--lead-verify-key", "K"]), Path::new("")) == s(&["--lead-verify-key"])`.
    - `has_session_arg(Agent::ClaudeCode, &s(&["--remote", "X"]))` (as `--cloud`, minor 2).
  - T8 `every_option_of_the_claude_code_binary_is_classified` (`// FIX-31 D4, D6, D7, AU30: every root option of Claude Code 2.1.331 filters by its arity and class`). In this order:
    1. `binary_options().len() == 139`.
    2. The exact set: the names of the `Arity::None` options of `binary_options()` for which `is_flag(Agent::ClaudeCode, n)` is true, sorted, equal `["--agents-view-if-allowed", "--allow-other-models", "--await-claim", "--await-initialize", "--background", "--bare", "--bg", "--continue", "--deep-link-origin", "--desktop", "--enable-auth-status", "--fork-session", "--hold-repository-while-parked", "--host-auth-is-org-sign-in", "--ignore-persisted-trust", "--init", "--init-only", "--maintenance", "--managed-settings-in-initialize", "--plan-mode-required", "--print", "--reply-on-resume", "--session-mirror", "--tmux", "-c", "-p"]`.
    3. For each option with `arity != Arity::None` and each name `n`:
       - `assert!(is_flag(Agent::ClaudeCode, n), "{n} has no table row")`;
       - unless `CC_CONTENT` or `CC_PATH` holds `n`: `probe(Agent::ClaudeCode, n, arity, CC_REMOVED.contains(&n.as_str()) || CC_HIDDEN_REMOVED.contains(&n.as_str()))`.
    4. For each `Arity::None` option and each name `n`:
       - if `is_flag(Agent::ClaudeCode, n) && n != "--bare"`: `cc(&[n, "zz", "--model", "m"]).kept == s(&["--model", "m"])`, message `"{n}: a boolean in the table is removed"`;
       - else: `cc(&[n, "zz"]).kept == s(&[n])`, message `"{n}: a boolean not in the table is kept, with no value"`.
    5. For each `n` of `CC_PATH`: `cc(&[n, "x.md"]).kept == s(&[n, &abs("x.md")])`.
  - T9 `a_removed_claude_code_boolean_keeps_the_next_token_apart` (`// FIX-31 D7, AU30: a removed boolean takes no value; the next token is its own argument`). For each `n` of `CC_BOOL_REMOVED`:
    - `cc(&[n, "--model", "opus"]) == Filtered { kept: s(&["--model", "opus"]), dropped_flags: vec![] }`;
    - `removable(Agent::ClaudeCode, &s(&[n, "--model", "opus"])) == s(&[n])`;
    - `cc(&[n, "--verbose"]).kept == s(&["--verbose"])`;
    - `removable(Agent::ClaudeCode, &s(&[n, "fix it"])) == s(&[n, "fix it"])`;
    - `cc(&[&format!("{n}=x")]).kept == s(&[])`;
    - `removed_labels(Agent::ClaudeCode, &s(&[n, "--model", "m"]), Path::new("")) == s(&[n])`.
  - Run `cargo test -p zygo-workspace --locked --test args`. Expected red:
    - T1 at its first assertion (left kept `["--zzunknown"]`);
    - T2 at `-h true` (left `["-h"]`);
    - T3 at `["-p", "pw"]` (left kept `["-p"]`);
    - T4 at `--zzunknown X`;
    - T5 at `-pd hooks` (left kept `["-d", "hooks"]`);
    - T6 at its first assertion (left kept `["--max-turns", "--model", "opus"]`);
    - T7 at `["--prefill", "fix it"]` (left kept `["--prefill"]`);
    - T8 at the exact set (left `["--bare", "--continue", "--fork-session", "--print", "-c", "-p"]`);
    - T9 at `--agents-view-if-allowed` or the first name of `CC_BOOL_REMOVED` (left kept `[n, "--model", "opus"]`).
    - Every other test passes.
- [ ] 3. Tests first, the existing tests of `crates/zygo-workspace/tests/args.rs` that hold the old result:
  - `claude_keeps_the_rest_as_typed`:
    - remove `&["--tmux=classic"], ` from the `kept` list;
    - replace `// short clusters: a removed letter leaves the kept ones` and the four assertions after it with `// short clusters (FIX-31 B1): a removed letter removes the whole cluster and its value` and `assert_eq!(cc(&["-pd"]).kept, s(&[]));`, `assert_eq!(removable(Agent::ClaudeCode, &s(&["-pd"])), s(&["-pd"]));`, `assert_eq!(cc(&["-pdhooks"]).kept, s(&[]));`, `assert_eq!(cc(&["-pn", "my name"]).kept, s(&[]));`, `assert_eq!(cc(&["--tmux=classic"]).kept, s(&[]));`.
  - `opencode_rules`:
    - the comment before `oc(&["-r", "abc"])` becomes `// the Claude Code table does not apply: -r is an unknown flag here, and OpenCode gives it the next token (FIX-31)`;
    - `assert_eq!(oc(&["-r", "abc"]).kept, s(&["-r"]));` becomes `assert_eq!(oc(&["-r", "abc"]).kept, s(&["-r", "abc"]));`;
    - `assert_eq!(oc(&["--mcp-config", INLINE]), Filtered { kept: s(&["--mcp-config"]), dropped_flags: vec![] });` becomes `assert_eq!(oc(&["--mcp-config", INLINE]), Filtered { kept: s(&["--mcp-config", INLINE]), dropped_flags: vec![] }); // not an OpenCode flag: its value is kept as typed, as --mcp-config=<json> is`.
  - `table_letters`: the body becomes `let known = |c: &char| !occurrences(agent, &s(&[&format!("-{c}")])).is_empty(); ('0'..='9').chain('A'..='Z').chain('a'..='z').filter(known).collect()`, and the doc comment becomes `/// The ASCII letters that the table of agent knows as a short flag, sorted: the walk reports -<letter> as a table flag.`
  - `every_short_flag_in_the_pinned_help_is_classified`:
    - `assert_eq!(table_letters(Agent::OpenCode), ['c', 'm', 's']);` becomes `assert_eq!(table_letters(Agent::OpenCode), ['c', 'f', 'm', 'p', 's', 'u']);`;
    - the loop becomes `for (help, agent, sub) in [(claude, Agent::ClaudeCode, &[][..]), (opencode, Agent::OpenCode, &['f', 'p', 'u'][..])] { let mut want: Vec<char> = table_letters(agent).into_iter().filter(|c| !sub.contains(c)).collect(); want.extend(['h', 'v']); want.sort_unstable(); assert_eq!(help, want, "{agent:?}"); }`, with the comment `// the help set is the table set plus the exempt letters; -f, -p and -u are options of run and attach (FIX-31 D3), not of the default help`;
    - the exempt-letter comment and its six assertions become `// FIX-31 B1: every letter of a cluster is looked up, so a cluster that holds a session or print letter is removed whole, also after -h or -v.` and `assert_eq!(cc(&["-vp"]), Filtered::default());`, `assert_eq!(cc(&["-hc"]), Filtered::default());`, `assert_eq!(removable(Agent::ClaudeCode, &s(&["-vp", "-hc"])), s(&["-vp", "-hc"]));`, `assert_eq!(oc(&["-vc"]), Filtered::default());`, `assert_eq!(oc(&["-hc"]), Filtered::default());`, `assert_eq!(removable(Agent::OpenCode, &s(&["-vc", "-hc"])), s(&["-vc", "-hc"]));`.
  - `claude_filter_matches_pinned_help`: `probe(Agent::ClaudeCode, n, *arity, CC_REMOVED.contains(&n.as_str()));` becomes `probe(Agent::ClaudeCode, n, *arity, CC_REMOVED.contains(&n.as_str()) || CC_BOOL_REMOVED.contains(&n.as_str()));`, with the comment `// --bg, --background, --desktop and --tmux are removed booleans of the pinned help (FIX-31 D7)`.
  - `session_flags_come_from_the_table`: the Claude Code list becomes `["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport", "--remote"]`.
  - Run `cargo test -p zygo-workspace --locked --test args`. Expected: these five are also red, each at its first changed assertion.
- [ ] 4. Tests first, outside the package:
  - `crates/zygo/src/cmd/member.rs`, `record_start_sets_events_off_from_the_arguments_of_this_start`: `(OpenCode, &["run", "--attach", "http://h"], true, &["--attach"])` becomes `(OpenCode, &["run", "--attach", "http://h"], true, &[])`.
  - `crates/zygo/tests/workspace_member.rs`, `a_saved_attach_flag_without_run_is_not_events_off`:
    - the doc comment becomes `/// FIX-24 P12, AU27 known limit 8; diff review round 2, finding 5: a member that an older zygo saved from run --attach <url> keeps only ["--attach"]; its ws run start (the real record_start path) runs the TUI, computes events_off = false over a stale true, saves no --attach (FIX-31 D3), and down counts the quiet member as idle.`;
    - the assertion becomes `assert_eq!((m.launch.args.clone(), m.launch.events_off), (Vec::<String>::new(), false));`.
  - Same file, `a_hand_start_whose_agent_sends_no_member_events_is_saved_with_events_off`: the case `(Agent::OpenCode, &["opencode", "--", "run", "--attach", "http://127.0.0.1:1"], &["--attach"])` becomes `(…, &[])`.
  - Same file, add after `au30_an_own_session_argument_is_not_preassigned_or_saved`: `a_remote_argument_is_not_preassigned` (`// FIX-31 minor 2, W§8.3: --remote is a session argument, as --cloud`). Steps:
    - `Harness::new()`, `workspace_session(&h, Some(KEY2), Some(WS_ID))`, `up_state(&h, &[], &mut |_| {})`;
    - one pane `split(&h, "=zygo-app:", "cat")`, a stub `h.stub(Agent::ClaudeCode, "r0")`, and a hand start `hand(&h, &pane, &["claude", "--", "--remote", "X"], &pairs(&env))` through `Guarded::spawn(..).output("hand start")`.
    - Assert exit `Some(0)`.
    - With `a = argv(&out)`: `a.iter().any(|t| t == "--remote") && a.iter().any(|t| t == "X")`, and `!a.iter().any(|t| t.starts_with("--session-id="))`, message `"zygo adds no --session-id: {a:?}"`.
    - `member(&h, &pane_key(&h, &pane)).unwrap().launch.args == Vec::<String>::new()`.
  - `crates/zygo-launch/tests/claude_code.rs`, `session_args_follow_w4_and_the_pinned_help`:
    - add the helper `fn binary_option_names() -> BTreeSet<String>`, which reads `../../fixtures/claude-code/options-2.1.331.txt` (`CARGO_MANIFEST_DIR`), skips `#` lines, and takes the words that start with `-` of the third tab field, a `,` at the end cut;
    - the list becomes `["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport", "--remote"]`;
    - `assert!(help.contains(f), "{f} is not in the pinned --help");` becomes `assert!(help.contains(f) || binary.contains(f), "{f} is in neither the pinned --help nor the 2.1.331 option list");`, with `let binary = binary_option_names();` before the loop.
  - Run:
    - `cargo test -p zygo --locked --lib cmd::member` → red at the `record_start` test (left `["--attach"]`);
    - `cargo test -p zygo --locked --test workspace_member -- a_saved_attach_flag_without_run_is_not_events_off a_hand_start_whose_agent_sends_no_member_events_is_saved_with_events_off a_remote_argument_is_not_preassigned` → all three red with tmux (left `["--attach"]`; the third at the `--session-id=` assertion); without tmux, record the skip line;
    - `cargo test -p zygo-launch --locked --test claude_code session_args_follow_w4_and_the_pinned_help` → red at the list.
- [ ] 5. Code, `crates/zygo-workspace/src/args.rs`:
  - a. After `OPENCODE_GLOBAL_BOOLEANS` (`:407`), add `const OPENCODE_COMMAND_BOOLEANS: &[&str] = &["-h", "--help", "-v", "--version", "--print-logs", "--pure", "--mdns", "--auto", "--mini", "--replay", "--share", "--thinking", "-i", "--interactive", "--yolo", "--dangerously-skip-permissions", "--demo", "--major", "--keep-config", "--keep-data", "--dry-run", "--force", "--verbose", "--refresh", "--sanitize", "-g", "--global"];`. Its doc: `/// The boolean options of every OpenCode 1.19.6 command that the OpenCode table does not hold (the [boolean] rows of fixtures/opencode/help.txt and the boolean options of each subcommand builder in the binary). In the restored default command these take no value; a boolean of another command, which the default command does not know, loses only a value that it ignores (AU30, FIX-31). --no-<x> is not listed (opencode_takes_value gives it no value), and -f is not (the -f/--file row decides it).`
  - b. Add `fn opencode_takes_value(token: &str, after: Option<&str>, booleans: &[&str]) -> bool` with the rule of D1 and the doc `/// Whether the OpenCode 1.19.6 flag token takes after, the next token, as its value (FIX-24 matrix; FIX-31): = or a --no- prefix: never; the name is the token for a long flag and -<last letter> for a short token, because the value goes to the last letter; a name in booleans takes only true or false; any other name takes the next token unless it starts with -. The one value rule of opencode_subcommand and of the AU30 walk.`
  - c. Add `fn unlisted_takes_value(agent: Agent, token: &str, after: Option<&str>) -> bool`. Its body is the two arms `Agent::ClaudeCode => false,` and `Agent::OpenCode => opencode_takes_value(token, after, OPENCODE_COMMAND_BOOLEANS),` (exact, for M3). Its doc: `/// Whether a flag (or the last letter of a cluster, as -<letter>) that the table of agent does not hold takes after as its value (AU30, FIX-31). Claude Code: never; the table holds every option with a value of 2.1.331, and Claude Code stops at a flag that it does not register. OpenCode: opencode_takes_value with OPENCODE_COMMAND_BOOLEANS.`
  - d. `opencode_subcommand`:
    - replace the block `if token.contains('=') || token.starts_with("--no-") {` … `if takes {\n            next += 1;\n        }` (`:426-437`) with `if opencode_takes_value(token, args.get(next).map(String::as_str), OPENCODE_GLOBAL_BOOLEANS) {\n            next += 1;\n        }`;
    - replace the doc block `:409-414`. Old:
      ```
      /// The OpenCode subcommand in `args` and its index, as OpenCode 1.19.6 reads the tokens before its subcommand (W§7.2;
      /// FIX-24 diff review round 3, findings 2 and 5): the first token that does not start with `-` (or is `-`) and is not
      /// the value of a flag. A flag that holds `=` takes no next token. A `--no-<x>` flag takes no next token. A global
      /// boolean of [`OPENCODE_GLOBAL_BOOLEANS`] takes the next token only when it is `true` or `false`. Every other flag,
      /// long or short, listed in the table or not, takes the next token unless it starts with `-`. `--` ends the flags and
      /// there is no subcommand after it. `events_off` and the launcher (session arguments after `run`) use this one walk.
      ```
      New:
      ```
      /// The OpenCode subcommand in `args` and its index, as OpenCode 1.19.6 reads the tokens before its subcommand (W§7.2;
      /// FIX-24 diff review round 3, findings 2 and 5): the first token that does not start with `-` (or is `-`) and is not
      /// the value of a flag. Whether a flag (long or short, listed in the table or not) takes the next token is
      /// [`opencode_takes_value`] with the global booleans of [`OPENCODE_GLOBAL_BOOLEANS`], the value rule that AU30's
      /// filter also uses (FIX-31). `--` ends the flags and there is no subcommand after it. `events_off` and the launcher
      /// (session arguments after `run`) use this one walk.
      ```
  - e. `walk`, the unlisted long-flag branch (`:282-285`) becomes:
    ```
    let Some(flag) = lookup(table, name) else {
        out.kept.push(token.clone());
        if unlisted_takes_value(agent, token, args.get(next).map(String::as_str)) {
            out.kept.push(args[next].clone());
            next += 1;
        }
        continue;
    };
    ```
  - f. `walk`, the cluster branch: replace `:299-332` (from `// `-x` or a cluster `-xyz`: each letter is a flag until one takes a value or is not in the table` to the `if letter_removed { out.removed.push(token.clone()); }` block) with this body (B1; kept, because the tests do not decide it alone):
    ```
    // `-x` or a cluster `-xyz` (FIX-31 B1): every letter is looked up. A table letter that takes a value takes the
    // rest of the token, or, as the last letter, the next tokens by its arity, and ends the cluster; an unlisted
    // letter followed by `=` keeps the rest as typed and ends it. The next token belongs to the last letter only: an
    // unlisted last letter takes it by the rule of that letter. When any table letter is not kept, the whole cluster
    // and its value are removed.
    let body = &token[1..];
    let mut shape = String::new();
    let mut letters: Vec<char> = vec![];
    let mut last_unlisted: Option<char> = None;
    for (at, letter) in body.char_indices() {
        let rest = &body[at + letter.len_utf8()..];
        let Some(flag) = lookup(table, &format!("-{letter}")) else {
            if rest.starts_with('=') {
                shape.push_str(&body[at..]);
                last_unlisted = None;
                break;
            }
            shape.push(letter);
            last_unlisted = Some(letter);
            continue;
        };
        last_unlisted = None;
        let attached = (flag.arity != Arity::None && !rest.is_empty()).then_some(rest);
        let start = next;
        let outcome = settle(flag, attached, args, &mut next, base);
        out.occurrences.push(occurrence(flag, attached, &args[start..next], token_at..next));
        if outcome.flag_kept {
            shape.push(letter);
            shape.push_str(outcome.attached.as_deref().unwrap_or(""));
        }
        letters.push(letter);
        outcomes.push((flag, outcome));
        if flag.arity != Arity::None {
            break;
        }
    }
    let mut taken: Vec<String> = vec![];
    if let Some(letter) = last_unlisted.filter(|_| !body.contains('=')) {
        if unlisted_takes_value(agent, &format!("-{letter}"), args.get(next).map(String::as_str)) {
            taken.push(args[next].clone());
            next += 1;
        }
    }
    let all_kept = outcomes.iter().all(|(_, o)| o.flag_kept);
    for (letter, (_, outcome)) in letters.iter().zip(outcomes.iter_mut()) {
        if !all_kept {
            let kept = std::mem::take(&mut outcome.kept);
            outcome.removed.extend(kept);
        }
        if !all_kept || !outcome.removed.is_empty() {
            out.labels.push(format!("-{letter}"));
        }
    }
    if all_kept {
        out.kept.push(format!("-{shape}"));
        out.kept.extend(taken);
    } else {
        out.removed.push(token.clone());
        out.removed.extend(taken);
    }
    ```
    The common outcome loop (`:334-342`) stays. It appends the values of the table letters after the token (`taken` and those values are never both non-empty: `taken` exists only when the last letter is unlisted). It also sets `session` and `dropped_flags`.
  - g. `OPENCODE`: after `flag(&["--prompt"], Arity::Required, Class::Remove),`, add the comment `// removed: the options of the subcommands whose value can hold a secret, a URL, user text or a path (W-C7, FIX-31 D3)` and the 16 rows of the D3 table, each `flag(&[<names>], Arity::<arity>, Class::Remove), // <reason>`. For example `flag(&["-p", "--password"], Arity::Required, Class::Remove), // secret: basic-auth password`, `flag(&["--env"], Arity::Variadic, Class::Remove), // secret: KEY=VALUE environment of an MCP server`, `flag(&["-f", "--file"], Arity::Variadic, Class::Remove), // path to user content attached to the message`.
  - h. Doc comments:
    - `:48` becomes `/// fixtures/claude-code/help.txt, and every other root option of Claude Code 2.1.331 that takes a value or must be removed (fixtures/claude-code/options-2.1.331.txt, FIX-31). The table is complete for 2.1.331; a flag that is not here is kept as typed and takes no value (unlisted_takes_value).`
    - `:104` becomes `/// fixtures/opencode/help.txt, and the subcommand options whose value can hold a secret, a URL, user text or a path. A flag that is not here is kept as typed, with the value that unlisted_takes_value gives it.`
    - In the `filter` doc (`:347-350`), `All other tokens are kept as typed.` becomes `All other tokens are kept as typed; a flag that the table does not hold keeps the next token as its value when [`unlisted_takes_value`] says so, and a short cluster gives the next token to its last letter only (FIX-31).`
  - i. `CLAUDE_CODE`:
    - Directly after `flag(&["--teleport"], Arity::Optional, Class::Session),` (`:57`), add `flag(&["--remote"], Arity::Optional, Class::Session), // hidden alias of --cloud (FIX-31 minor 2)`.
    - Before the `--bare` row (`:101`), add the other 51 rows of the D4 table in table order, in two groups:
      - the comment `// hidden in 2.1.331 (fixtures/claude-code/options-2.1.331.txt), kept with the value: a plain setting (FIX-31 D4)` and the 13 `Keep` rows;
      - the comment `// hidden in 2.1.331, removed with the value: a secret, content, a prompt, a path to user content, a URL, or data of one launch (FIX-31 D4)` and the 38 `Remove` rows.
    - Each row is `flag(&["<long name>"], Arity::<arity>, Class::<class>), // <reason>`, for example `flag(&["--max-turns"], Arity::Required, Class::Keep), // plain setting: a number` and `flag(&["--prefill"], Arity::Required, Class::Remove), // prompt text`.
  - j. `CLAUDE_CODE`: directly after `flag(&["-p", "--print"], Arity::None, Class::Remove),` (`:58`), add the comment `// removed booleans (FIX-31 D7): one action and exit, an action that a restore would repeat, data of one launch, or the companion of a removed value flag` and the 19 rows of the D7 table, each `flag(&[<names>], Arity::None, Class::Remove), // <rule and short reason>`. For example `flag(&["--init-only"], Arity::None, Class::Remove), // (b) runs the Setup and SessionStart hooks, then exits` and `flag(&["--bg", "--background"], Arity::None, Class::Remove), // (b) starts the session in the background and returns`.
  - Run `cargo test -p zygo-workspace --locked --test args` → all pass.
- [ ] 6. Run the callers' tests:
  - `cargo test -p zygo --locked --lib cmd::member` → all pass;
  - `cargo test -p zygo --locked --test workspace_member` → all pass;
  - `cargo test -p zygo --locked --test loadout_secrets` → all pass;
  - `cargo test -p zygo-launch --locked --test opencode --test stub_launch --test claude_code` → all pass.
- [ ] 7. Mutation proofs. A scratchpad script makes one exact edit in `crates/zygo-workspace/src/args.rs`, runs `cargo test -p zygo-workspace --locked --test args`, and restores the file. Each run must exit 101 with a test panic and no compile error.

  | # | Old text | New text | Must go red |
  |---|---|---|---|
  | M1 | `        if unlisted_takes_value(agent, token, args.get(next).map(String::as_str)) {` (the long-flag branch, 5e) | `        if false {` | T1 |
  | M2 | `        if unlisted_takes_value(agent, &format!("-{letter}"), args.get(next).map(String::as_str)) {` (5f) | `        if false && unlisted_takes_value(agent, &format!("-{letter}"), args.get(next).map(String::as_str)) {` | T1 (`-xy X`) |
  | M3 | `Agent::ClaudeCode => false,` (in `unlisted_takes_value`) | `Agent::ClaudeCode => opencode_takes_value(token, after, OPENCODE_COMMAND_BOOLEANS),` | T5 |
  | M4 | `        if opencode_takes_value(token, args.get(next).map(String::as_str), OPENCODE_GLOBAL_BOOLEANS) {\n            next += 1;\n        }` (in `opencode_subcommand`) | the old rule without `--no-`: `        if token.contains('=') {\n            continue;\n        }\n        let after = args.get(next).map(String::as_str);\n        let takes = if OPENCODE_GLOBAL_BOOLEANS.contains(&token) {\n            matches!(after, Some("true" \| "false"))\n        } else {\n            after.is_some_and(\|t\| !t.starts_with('-'))\n        };\n        if takes {\n            next += 1;\n        }` | T4 (`--no-zz true`) and `opencode_subcommand_follows_the_opencode_1_19_6_parser` |
  | M5 | `"--share", "--thinking", "-i",` (in `OPENCODE_COMMAND_BOOLEANS`) | `"--share", "-i",` | T2 |
  | M6 | `flag(&["-p", "--password"], Arity::Required, Class::Remove), // secret: basic-auth password` | (line removed) | T3 |
  | M7 | `flag(&["--max-turns"], Arity::Required, Class::Keep), // plain setting: a number` | (line removed) | T6, T8 |
  | M8 | `flag(&["--prefill"], Arity::Required, Class::Remove), // prompt text` | `flag(&["--prefill"], Arity::Required, Class::Keep), // prompt text` | T7, T8 |
  | M9 | `flag(&["--init-only"], Arity::None, Class::Remove), // (b) runs the Setup and SessionStart hooks, then exits` | `flag(&["--init-only"], Arity::None, Class::Keep), // (b) runs the Setup and SessionStart hooks, then exits` | T9, T8 |
  | M10 | `    if all_kept {` (5f) | `    if true {` | T3 (`-ip SECRET`), T5 (`-pd hooks`) |
  | M11 | `flag(&["--remote"], Arity::Optional, Class::Session), // hidden alias of --cloud (FIX-31 minor 2)` | `flag(&["--remote"], Arity::Optional, Class::Remove), // hidden alias of --cloud (FIX-31 minor 2)` | T7 (`has_session_arg`), `session_flags_come_from_the_table` |

  (In M4, `\|` is a table escape for `|`.) Record each result in the node.
- [ ] 8. Spec, `docs/specs/2026-11-20-workspaces-design.md` (final text under "Spec text"): AU30 edits A1, A2, A4 and A5; W§7.2 edit B1; W§8.3 edit D1; W§16 edit C1; four change-log rows at the end of the table.
- [ ] 9. Package floor for the three packages that the node changes (`zygo-workspace`, `zygo`, `zygo-launch`; the controller runs the full workspace floor at merge):
  - `cargo test -p zygo-workspace -p zygo -p zygo-launch --locked --no-fail-fast` → all pass;
  - `cargo clippy -p zygo-workspace -p zygo -p zygo-launch --all-targets --locked -- -D warnings` → clean.

  Record the counts.
- [ ] 10. Write `### Results` in this node: the red runs of steps 2-4 with their messages, M1-M11, the counts of step 9, and any drift. Commit with the trailers of the implementer rules.

### Spec text

A1 (AU30, `:171`). Replace `The rest is stored as typed.` with:

> The rest is stored as typed. A flag that is not in the agent's table (`crates/zygo-workspace/src/args.rs`) is kept as typed, and the value rule of its agent decides whether the next token is its value (kept as typed with it) or a positional (removed). In a short cluster (`-abc`) every letter is looked up, the next token belongs to the last letter only, and a cluster that holds a removed letter is removed whole, with its value. Claude Code: the table is complete for Claude Code 2.1.331, from its option list (`fixtures/claude-code/options-2.1.331.txt`), so a flag that is not in it takes no value: a registered one is a boolean (for example `--verbose`), and one that Claude Code does not register stops the start with `error: unknown option '<flag>'` and exit 1 before any session (`allowUnknownOption` is set only on its `remote-control` and `plugin test` subcommands), so zygo removes the next token and no prompt word reaches the state (W-C7). On a newer Claude Code, an option that is not in the table follows the same rule: it is kept as typed and takes no value, so a new option with a value is saved without its value. OpenCode: the value rule of the OpenCode 1.19.6 parser, the one function that `opencode_subcommand` also uses (W§7.2 `member.launch.events_off`): a flag that holds `=` takes no next token; a `--no-<x>` flag takes no next token; a boolean option of an OpenCode command (the `[boolean]` rows of `fixtures/opencode/help.txt` and the boolean options of each subcommand) takes the next token only when it is `true` or `false`; every other flag takes the next token unless that token starts with `-`. So `zygo opencode -- --zzunknown X` saves `--zzunknown X`, and `zygo opencode -- run --thinking "fix it"` saves `--thinking` only. The default command, which `ws run` restores, parses the tokens again with its own options, so its booleans take no value there, although the parse of the tokens before a subcommand gives them one (W§7.2). The OpenCode subcommand options whose value can hold a secret, a URL, user text or a path (`-p`/`--password`, `-u`/`--username`, `--attach`, `--token`, `--url`, `--env`, `--header`, `-f`/`--file`, `--title`, `--dir`, `--description`, `--path`, `--query`, `--params`, `--event`, `--cwd`) are in the table and are removed with their value (W-C7).

A2 (AU30). Replace `(an `--attach` saved from `run --attach <url>` is then ignored, AU27 known limit 8)` with `(`--attach` and its URL are removed, AU27 known limit 8)`.

A4 (AU30). Replace `The builder classifies flags from the pinned `--help` (`fixtures/claude-code/help.txt`, `fixtures/opencode/help.txt`).` with:

> The builder classifies flags from the pinned `--help` (`fixtures/claude-code/help.txt`, `fixtures/opencode/help.txt`), and, for Claude Code, from the option list of the 2.1.331 binary (`fixtures/claude-code/options-2.1.331.txt`, 139 options, written by `fixtures/claude-code/scan/mkfixture.py`). A hidden option with a value is kept with its value when the value is a plain setting (for example `--max-turns`, `--thinking`, `--channels`). It is removed with its value when the value can hold a secret, content, a prompt, a path to user content or a URL, or is data of one launch that a host, SDK or server sets (for example `--prefill`, `--lead-verify-key`, `--sdk-url`, `--workload`, `--forward-home-settings`). In doubt it is removed. A boolean is removed when it does one action and exits or a restore would do its action again (`--init-only`, `--init`, `--maintenance`, `--desktop`, `--bg`, `--tmux`, as `-p`/`--print`), when it is data of one launch set by a host, SDK, server or parent process (for example `--await-claim`, `--session-mirror`, `--reply-on-resume`), or when it belongs to a removed value flag (`--allow-other-models`, `--deep-link-origin`); the token after it stays its own argument. `--bare` is kept. The full lists with each class are in `docs/work/phase2/nodes/FIX-31.md` (D3, D4 and D7 tables).

A5 (AU30, the session list). Replace `--from-pr`, `--cloud`; OpenCode` with `--from-pr`, `--cloud`, `--remote` (the hidden alias of `--cloud`); OpenCode`, and `--cloud [value]`, and any other` with `--cloud [value]`, `--remote [value]`, and any other`.

B1 (W§7.2, `member.launch` row, `:293`). Replace `AU30's filter does not use this rule.` with:

> AU30's filter uses the same value function (`opencode_takes_value`) for an OpenCode flag that its table does not hold, with the boolean options of every OpenCode command in place of the global booleans, because `ws run` restores the default command, which parses the tokens again with its own options (AU30). In a short cluster the function reads the last letter (`-xh` is read as `-h`).

D1 (W§8.3, `:394`). Replace `--from-pr`, `--cloud`, `--teleport` (the session rows` with `--from-pr`, `--cloud`, `--teleport`, `--remote` (the session rows`.

C1 (W§16, row `crates/zygo-workspace/src/args.rs`, `:652`). Replace `(W§7.2 `member.launch.events_off`). |` at the end of the row with:

> (W§7.2 `member.launch.events_off`); the value rule of an OpenCode flag that the table does not hold, one function for `filter` and `opencode_subcommand`; the short-cluster rule; the Claude Code table complete for 2.1.331 (hidden options with a value, removed booleans, `--remote`), checked against `fixtures/claude-code/options-2.1.331.txt`, which `fixtures/claude-code/scan/mkfixture.py` writes from the binary (AU30). |

Change-log rows (append at the end of the table):

> | 2026-11-23 | AU30 (unlisted-flag value rule, short clusters, OpenCode subcommand options, Claude Code option list, removed booleans, `--remote`, `--attach` known limit) | author's call (FIX-31 plan); coordinator decisions 2026-11-23 (hidden options and booleans in FIX-31; plan review B1-B4, minor 2-4, 7) | The filter kept an unlisted flag and removed its value as a positional, so `zygo opencode -- --zzunknown X` lost `X`, and `claude --max-turns 5 --model opus` was restored with `opus` as the prompt. OpenCode now uses the value function of `opencode_subcommand`, with the booleans of every OpenCode command; a short cluster gives the next token to its last letter, and a cluster with a removed letter is removed whole. Claude Code: the table is complete for 2.1.331 (52 hidden options with a value: 13 kept, 38 removed, `--remote` a session flag as `--cloud`; 19 booleans removed); a flag outside it keeps no value. Kept values would save a password, a token, a header, a URL with user info or user text, so the OpenCode subcommand options that hold them are removed with their value (W-C7). |
> | 2026-11-23 | W§7.2 (`member.launch.events_off`) | author's call (FIX-31 plan) | `opencode_subcommand` and AU30's filter share one value function, so the two rules cannot drift; only the boolean set differs, by the parse context; a short cluster is read by its last letter. |
> | 2026-11-23 | W§8.3 | author's call (FIX-31 plan review, minor 2) | `--remote`, the hidden alias of `--cloud` in Claude Code 2.1.331, is a session argument as `--cloud` is, so zygo pre-assigns no session id beside it. |
> | 2026-11-23 | W§16 (row `args.rs`) | author's call (FIX-31 plan) | The row names the shared value rule, the cluster rule, the complete Claude Code table and the option-list fixture with its scan script. |

### Self-review (once, after revision 3)

- **Spec coverage.** Each sentence of A1, A2, A4, A5, B1, C1 and D1 has a test:
  - unlisted value kept: T1;
  - `=`, `--no-`, `true`/`false`: T1, T2;
  - clusters: T2, T3 (OpenCode) and T5 (Claude Code), with M2 and M10;
  - a `run` message is removed: T2;
  - default-command booleans take no value: T4 and the existing probe;
  - OpenCode subcommand secret options removed, with repeated and `=` forms: T3;
  - Claude Code unlisted flag takes no value: T5;
  - hidden settings keep their value: T6;
  - hidden secret options drop their value: T7;
  - every binary option classified, with the exact boolean set: T8;
  - removed booleans take no value: T9;
  - `--remote` is a session flag (A5, D1): T7, `session_flags_come_from_the_table`, the zygo-launch list test and `a_remote_argument_is_not_preassigned`;
  - true positionals and `--` still removed: T1, T9, `opencode_rules`;
  - one value function: T4, M4.

  Each acceptance case is in T1, T2, T3, T5 or T6.
- **Step scan.**
  - Each step has its file, its exact text or signature, and a run with an expected red or green.
  - The table rows are given fully by the D3, D4 and D7 tables, and the cluster body is written out.
  - Each mutation has exact old and new text.
  - There is no placeholder except `<session>` in the commit trailer, and no "Found, not fixed" item.
- **Interface match.**
  - No public signature changes.
  - The private items are listed with their signatures.
  - `opencode_subcommand` changes only for a cluster that ends in `h` or `v`. Its existing matrix test rows are unchanged, because none of them is such a cluster.
  - `session_flags` gains `--remote`. Its two list tests are updated (step 3, step 4).
  - `has_session_arg` becomes true for `--remote`. That is the intended pre-assign change, tested in `workspace_member.rs`.
  - The new rows give `occurrences`, `flags` and `is_flag` more names, which `loadout show` then shows as flags after a secret-named flag (W§12 rule (b)).
  - No source uses a flag whose class changes (P2).
  - A `Required` row takes a next token that starts with `-`, as the existing removed rows do. An `Arity::None` row takes none.
- **Proportion.**
  - `args.rs`: about 90 changed lines of code, of which the cluster branch is about 45, plus 88 table rows (52 + 19 + 1 for Claude Code, 16 for OpenCode).
  - `tests/args.rs`: nine new tests and seven changed ones.
  - `crates/zygo`: three changed test cases and one new test.
  - `crates/zygo-launch`: one changed test.
  - Two scan scripts and one fixture (written).
  - Spec text.

  Bodies are signatures except where a mutation needs the exact text (`unlisted_takes_value`'s two arms, the 5e branch) and the cluster branch. The step count stays 10.
