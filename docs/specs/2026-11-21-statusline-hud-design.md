# zygo status line HUD — design

**Status:** approved by the operator 2026-11-21
**Date:** 2026-11-21
**Base spec:** `docs/specs/2026-11-18-zygo-design.md`.
**Related:** `docs/specs/2026-11-20-workspaces-design.md` (phase 2, workspaces).

**Citation rule.** A bare `§n` is a section of the base spec. `WS§n` is a section of the workspaces design (that document calls its own sections `W§n`; `WS§n` here and `W§n` there are the same section). Its decision ids are cited as `WS AUn` and its constraint ids by their own names (`W-C4`). `H§n` is a section of this document. This document changes the base spec only where H§9 says so.

Each decision has a mark. **(operator)**: the operator made the decision. **(author)**: the author of this document made the decision; the operator can change it before approval. Code references are `file:line` at commit `48f9583` (the later commit `20cfbbe` changes documents only).

---

## 1. Intent

The zygo status line today is one plain text line (`crates/zygo-hooks/src/statusline.rs:121-141`):

```
▌▌▌ zygo ◇ branch:main ◇ - ◇ ctx -/777k ◇ compact 777k ◇ cache - ◇ trimmed 0
```

(live capture: `fixtures/claude-code/live/p-n28-1-pane.txt:17`; the capture has no stripe because it is older than the stripe.)

The line has four problems:

1. `trimmed` shows bytes. The user thinks in tokens.
2. The second ctx number is §10.2.1 `effective` (`SessionState::status` in `crates/zygo-proxy/src/session.rs`). `effective_for` (`crates/zygo-proxy/src/window.rs`) returns `None`, and the line prints `-`, in two cases: the model has no `context_window`; or there is no soft threshold (no numeric override and no `soft_context_window`, or the override is `off`) and no safety window (the safety window exists only when the agent's assumed window is larger than the model window). This is the usual case: the built-in catalog sets no `soft_context_window` (`crates/zygo-config/data/catalog.toml`), so with no override and no safety window the user sees `-` and not the model window.
3. The work item prints as `source:id` text (`launcher.rs:230`). `branch:main` is noisy.
4. The line shows no model, no token totals, no spend, no git state and no worktree.

This document replaces the line with a fixed HUD (heads-up display) in two styles: `compact` (one row) and `verbose` (two rows). The HUD shows what the session costs and how full its context is, with color and icons.

## 2. Goals

- **H-G1** One look at the HUD tells the user: the work item and worktree, the served model, the profile, the context use against the model window and the compaction threshold, the session tokens, the session spend, the cache share and the trimmed tokens.
- **H-G2** Spend is a list-price estimate that stays correct when a role routes to a provider other than Anthropic. zygo computes it from its own usage and catalog list prices, not from Claude Code's numbers.
- **H-G3** The HUD never breaks the agent (C6) and finishes within the 3 s deadline.
- **H-G4** The HUD reads well with and without a Nerd Font, with and without color, on Linux, macOS and Windows.
- **H-G5** One config key and one session verb select the style.

## 3. Non-goals

- **User-customizable segments.** No segment list, no format string, no per-segment colors in config **(operator, OP4)**. The only exceptions are the icon set (OP6, H§6.5) and the style (OP7, H§8).
- **Budgets, limits, alerts.** Spend is display only. zygo never refuses, warns or throttles on spend. This keeps §3 ("Quotas, budgets, cost tiers, credits, boosters, spend limits") a non-goal **(operator, OP5)**.
- **Claude Code's `cost` field.** The HUD never shows the `cost` value from the status line stdin. Claude Code prices every request as an Anthropic model, which is wrong when a role routes to another provider **(operator, OP5)**.
- **Per-person or per-team spend reports.** The HUD shows one session only.
- **OpenCode.** zygo sets no OpenCode status surface. The conformance row meets A-9 ("Status", §8.1: "on the agent's status surface, else `zygo status`") through the CLI (`docs/conformance.md:11`, A-9 cell `full https://opencode.ai/docs/cli/`). That OpenCode 1.18.16 has no command status line is an assumption (PH13). So no zygo surface shows a HUD in an OpenCode session; the HUD is Claude Code only, as today. The `zygo statusline` verb works the same in an OpenCode session as in a Claude Code session **(operator, 2026-11-21)**: the session form `zygo statusline <style>` stores the style in `overrides.json` and gives the same reply, `--save` also writes user config, and the form with no argument prints the current style and its source. The verb does not read `ZYGO_AGENT` (H§8).
- **A change to the usage store schema (§14).** The HUD adds no column.

## 4. Constraints

- **H-C1** The base constraints C1-C10 (§4) apply.
- **H-C2** The whole hook run has a 3 s deadline (`statusline.rs:15-16`, §17).
- **H-C3** C6: on any internal error the hook prints nothing that changes a decision, writes one line to stderr and exits 0. `guarded` does this today (`crates/zygo-core/src/hook.rs:76-92`).
- **H-C4** C7 and the §17.1 note (N28): the status line reaches only the user, so it may show the window and the compaction threshold. `zygo status` and `zygo help` run in the agent's shell must still hide the window number (`crates/zygo/src/cmd/status.rs:51-53`). The HUD changes nothing in that rule. Live check P-N28-1 verified that the status line text is in no upstream request (`docs/live-checks.md`, Results row P-N28-1). Residual risk (author, accepted): the agent can run `zygo hook statusline` in its own shell (it has `ZYGO_SESSION_DIR`) or read the cache files in the session directory, and so learn the window and the threshold. C7 forbids zygo to tell the agent; it does not stop an agent that looks. To keep the exposure small, the cache file holds only the numbers that rendering needs (H§7.7), not the named window fields of the status reply. Residual limit: those numbers still include the printed window, the `⇣` value and the bar denominator, so an agent that reads the cache can still infer the compaction point.
- **H-C5** Serve mode: provider names and prices never leave the server (§7.2 "Team config"; §14 N16 note; `crates/zygo-proxy/src/routes.rs:160-162`, `:187`).
- **H-C6** Every string from outside zygo's own code (work item, profile, model, effort, branch, worktree name, serve host, stdin values) goes through `zygo_core::esc` (`crates/zygo-core/src/text.rs:4-18`) before it is printed. The HUD prints no team label and no error text; a failure shows only a fixed marker (`offline`, `suspended`, `ended`).

## 5. Glossary

| Term | Meaning |
|---|---|
| HUD | The status line that this document defines. |
| Segment | One item of the HUD, for example `ctx 196k/1m ▰▱▱▱▱`. |
| Style | `compact` (one row) or `verbose` (two rows). |
| Stripe | The three half-block mark `▌▌▌` (U+258C; red, yellow, cyan) that starts each HUD row (`statusline.rs:53-57`). |
| Logo | `𝘇𝘆𝗴𝗼` (Mathematical Sans-Serif Bold small z, y, g, o) after the stripe; in verbose row 2 its reflection `zʎƃo`. |
| Model window | `facts.context_window` of the catalog model (`crates/zygo-config/data/catalog.toml:8`). |
| Threshold | The status field `threshold` (H§7.2): the soft or override compaction point, clamped by the safety window. `⇣` shows it (AU28). |
| Effective window | §10.2.1 `effective`, the status field `effective_window` (`SessionState::status` in `crates/zygo-proxy/src/session.rs`, AU29): the threshold, else the safety window. The bar and the verbose percent use it as their denominator (AU17). |
| Spend | Session tokens × catalog list price, summed by the proxy (H§7.3). |
| Notional spend | Spend that is a comparison only, shown with `≈`: on a subscription login (passthrough, §12), where the user pays no per-token price; and in a team session, where the team server prices the tokens with the public list prices of the built-in catalog compiled into its binary, not with the team's own prices (AU14). |
| Linked worktree | A git worktree that is not the main worktree: its git dir differs from the common git dir. |
| Estimated tokens | `ceil(bytes ÷ 4)`, the same rule as the proxy's `count_tokens` estimate (both estimates in `run`, `crates/zygo-proxy/src/forward.rs`). |

## 6. Decided approach

### 6.1 Decisions

| Id | Decision | Mark |
|---|---|---|
| OP1 | The trimmed segment shows estimated tokens (bytes ÷ 4), not bytes. One shared constant serves the HUD and the proxy estimate. | operator |
| OP2 | ctx shows `used/window`. With no threshold, the second number is the model window (`ctx 196k/1m`). With a threshold, the HUD also shows it (`⇣600k`). The HUD never prints `-` for the window when the model window is known. | operator |
| OP3 | The work item has no `branch:` text label. A branch shows as `<branch icon> main` in bold blue. Other sources keep a short marker: `#123` for GitHub, `◆ program/node` for a work graph, the bead id for Beads. | operator |
| OP4 | The HUD is fixed. No segment config. Exceptions: OP6 and OP7. | operator |
| OP5 | The HUD shows session tokens in and out and session spend = usage × catalog price per route, computed by the proxy, not Claude Code's stdin `cost`. Display only. On a subscription login the spend is notional and shows as `≈$1.84` (the `≈` dim). | operator |
| OP6 | Nerd Font icons by default; a plain Unicode fallback. | operator |
| OP7 | Two styles, chosen by config: `[statusline] style = "compact" \| "verbose"`. | operator |
| OP8 | The segment table and the mockups of H§6.2 and H§6.3 are approved ("perfect"). This document adds the stripe (OP10) and the worktree segment (OP9) to them. | operator |
| OP9 | A worktree segment, shown only when the session cwd is in a linked worktree. It shows only the final directory name of that worktree's top-level path, never the full path. Color: deep amber, 256-color 172 (OP13). | operator |
| OP10 | In verbose, the stripe leads row 1 and row 2 at the same column, so that it reads as one continuous stripe. | operator |
| OP11 | Logo `𝘇𝘆𝗴𝗼` (U+1D607 U+1D606 U+1D5F4 U+1D5FC, Mathematical Sans-Serif Bold small letters) in bold bright white (SGR `1;97`) in both styles, right after the stripe. Verbose row 2 shows its reflection `zʎƃo` (U+007A U+028E U+0183 U+006F) in dim white (SGR `2;37`) under the logo, at the same column. | operator |
| OP12 | The stripe is three half blocks `▌` (U+258C) in red, yellow, cyan (SGR `31`, `33`, `36`), as the current code (`statusline.rs:56`). It leads the compact row and both verbose rows at the same column. | operator |
| OP13 | The reference mockups of H§6.3 (the two `printf` commands) are approved. The HUD's text, spacing and SGR codes match them exactly. Consequences: worktree 256-color 172 (16-color fallback 33); ahead `↑N` cyan `36`; model and effort in one magenta span (`opus-4.7 · high`); profile as icon and name in both styles; the window number of `ctx` dim. | operator |
| OP14 | (2026-11-21) Drop the git dirty count `●N`. The OP13 verbose reference is amended: its `\e[33m●3\e[0m ` run is removed; `↑1` stays cyan. Git runs only `rev-parse` and `rev-list` (H§7.5). | operator |
| AU1 | Default style `compact`. `/zygo:statusline compact\|verbose` and `zygo statusline compact\|verbose [--save]` switch it (O14, §17.1 one rule). The session choice is kept in `overrides.json` like other hook-side session settings. A `statusline` value in `overrides.json` other than `compact` or `verbose` is ignored; the hook then uses `view.json`, then the default. `--save` outside a session writes user config, which affects sessions launched after it only (a running session reads its style from `view.json` and `overrides.json`). | author |
| AU2 | Icon set. `ZYGO_NERD_FONT=1` forces the Nerd set; it overrides the `TERM` and Windows rules below. `ZYGO_NERD_FONT=0` forces the plain set. Any other value, or unset, selects automatically: plain when `TERM` is `linux` or `dumb`, or on Windows (the PH8 and PH10 fallback, applied by default 2026-11-22: `WT_SESSION` and `TERM_PROGRAM` do not select the Nerd set on Windows); else Nerd. `NO_COLOR` removes color only; it does not select plain icons (https://no-color.org defines color only). `zygo doctor` prints an icon test line. `ZYGO_NERD_FONT` is the operator-approved exception to OP4. | author |
| AU3 | Width: the HUD fits each row into an assumed width (H§6.7) and drops low-priority segments first in a fixed order. | author |
| AU4 | Refresh and cache (H§7.7). One cache file holds the status part, the git part, the fetch `outcome`, `retry_at` and the last status latency. One lease file, taken with `create_new`, lets one run at a time fetch and write; the holder writes the whole cache file. A part is fetched again when its `at` is 30 s old or more and `retry_at` has passed. With a status part 90 s old or less, the status fetch waits `clamp(1.5 × last latency, 1 s, deadline)`; otherwise it gets the full deadline. The hook prints, then exits; a result that is not in by the print is lost. The trimmed count is not cached: one indexed query per call (`Store::bytes_trimmed`, H§7.4). The stdin values, the session view, `overrides.json` and the workspace phase are read fresh on each call. The cache holds data, not a rendered line. | author |
| AU5 | Spend is computed in the proxy, per request, by one price function (`Prices::cost`, H§7.3) that also writes the usage row's `estimated_usd`. The function applies a long-context price tier when the catalog row has one. The session keeps running sums. The status reply carries them (H§7.2). | author |
| AU6 | Git: one bounded worker in the session cwd, in parallel with the status fetch, after the stdin parse (AU24). It runs two commands that run no repository-configured program: `git rev-parse` (worktree, branch) and `git rev-list --left-right --count` (ahead/behind against the upstream) (H§7.5). Every HUD git call has `GIT_NO_LAZY_FETCH=1` (a partial clone could otherwise fetch missing commits through `remote.<n>.uploadpack`), `GIT_OPTIONAL_LOCKS=0` and `GIT_TERMINAL_PROMPT=0`. `rev-parse` has its own 150 ms budget, so the worktree and the branch show when `rev-list` is slow; the worker has 400 ms in all. A timeout counts as a failed try. Both styles use the worktree result; only verbose shows the ahead/behind marks. | author |
| AU7 | ctx bar: compact 5 cells; verbose 8 cells plus a percent. The bar and the percent need both `used` and a known model window: with `used` unknown the segment is `ctx -/1m`; with the window unknown it is `ctx 196k/-`; in both cases there is no bar and no percent. A numeric `⇣<n>` also needs the known window (it shows only when below it); `⇣off` does not (AU28). | author |
| AU8 | Row start: stripe, one space, logo (OP11, OP12). With `NO_COLOR` the stripe is omitted on every row (as the 2026-11-20 §17 note says today); the logo and its reflection stay, with no SGR. Plain icon set: the logo is plain `zygo` in SGR `1;97`, and verbose row 2 has four spaces in place of the reflection, so that the next segment keeps its column; `ʎ` and `ƃ` are not in the plain glyph set (H§6.5 rule). With the plain set and color on, the stripe stays (`▌` is in every console font that zygo targets). If PH9 fails, the logo is plain `zygo` in every icon set; the reflection stays. | author |
| AU9 | Fixed segment order (H§6.2) and fixed gaps, as OP13 shows. **Compact:** one space after the stripe; one space inside a group; two spaces between groups. Groups: [logo, workspace phase, work item, worktree], [model, profile], [ctx, bar, threshold], [tokens], [spend], [cache], [trimmed], [stale/offline/suspended/ended]. **Verbose:** one space after the stripe; two spaces after the logo or the reflection; three spaces between groups; one space inside a group. Row 1 groups: [workspace phase], [work item, worktree, ahead/behind], [model · effort], [profile], [mode]. Row 2 groups: [ctx, bar, percent, threshold], [tokens; two spaces between `in` and `↓`], [spend], [cache], [trimmed], [stale/offline/suspended/ended]. No `◇`. | author |
| AU10 | The model segment shows the served model from the proxy, not the stdin `model`. In a zygo session Claude Code's model is a role name (`ANTHROPIC_MODEL` = main role, `crates/zygo-launch/src/claude_code.rs:141`; settings `model`, `:291`). The proxy reports the catalog id of the model that served the last main-loop request (H§7.2). The display name drops a leading `claude-`. When the status reply has no `model` (no fetch yet, offline with no cache, or an old server), the model segment is dropped; the hook never shows the stdin `model`, which is a role name. The OP13 mockup lost the model glyph in transit; the author keeps `` U+F4BC (H§6.5). The branch glyph was lost too; the author keeps `` U+E0A0. | author |
| AU11 | Work item source. The value comes from `ZYGO_WORK` (set by the launcher, `launcher.rs:517`); the status `work` field is the fallback. An empty `ZYGO_WORK` counts as absent (the launcher writes an empty value when there is no item, `launcher.rs:517` `unwrap_or_default`). When the source is `branch`, the HUD shows the live branch `branch.head` from the git worker in place of the launch-time id, so a checkout shows at the next refresh; with no git result, or with no branch from H§7.5 call 1, it shows the launch-time id. Call 1 gives no branch for every fourth line not under `refs/heads/`: a detached head (`rev-parse --symbolic-full-name HEAD` prints `HEAD`), a HEAD on `refs/remotes/…` or `refs/tags/…`, an empty line, or any other line. With SIGCHLD ignored by the hook's parent, the exit status of git is lost, so an unborn HEAD also gives the worktree with no branch (H§7.5) and the launch-time id. With no work item at all, the HUD shows the live branch in plain blue `34` (not bold), with the same branch icon as the `branch` source (Nerd `` U+E0A0; the plain set has no branch glyph, H§6.5, so the name stands alone). Other sources show the launch-time id. Limits: `/zygo:work` and `zygo work` change the repository setting only (§17.1 scope "repository; session text at next launch"); they write neither `ZYGO_WORK` nor `view.json` (`crates/zygo/src/cmd/work.rs:99-104`), so the HUD shows the new item only after a relaunch. With the `branch` source, the live branch can differ from the unit that the session's usage is attributed to (the launch-time id); this is accepted. The marker rule per source is in H§6.2. | author |
| AU12 | The profile comes from `view.json` (`SessionView.profile`, `crates/zygo-core/src/session_view.rs:14`); the status `profile` field is the fallback. | author |
| AU13 | Mode (verbose only), first match wins: `passthrough` when the status data has `passthrough = true`; `serve <host>` (host of `ZYGO_URL`, whatever it is, a loopback tunnel included) when the status data has `team_session = true`; `local` when both are `false`. A **team session** is a session created with a team token on a team server (`SessionState.team_label` is set, `crates/zygo-proxy/src/session.rs`); the status reply carries only the flag `team_session`, never the label. The mode segment is omitted when the status data lacks `passthrough` or `team_session` (an old server) or when there is no status data (offline, no cache). In a team session `ZYGO_URL` is the server URL (`launcher.rs:236-262` uses the local proxy URL only on the local branch; `:342`, `:515`). | author |
| AU14 | Team session spend (lead decision): the team server prices each request of a team session with `Prices::cost` and the built-in catalog compiled into its own binary (public list prices; the first provider of the served model's row that has a price; exact per-request tier test), sums it, and sends `spend_usd` with `spend_notional = true`. The team's configured prices stay on the server, as its rows' `estimated_usd` (H-C5; §7.2 unchanged). The HUD shows `≈`. The operator checks this call (it resolves the former Q1). | author |
| AU15 | Spend display. `spend_usd` present → spend; absent (an old server) → no spend segment. Counts: `requests_total` (requests whose usage has at least one token) and `requests_priced`. For a local proxy, a request is priced when its route had a price (or notional prices in passthrough); for a team session, when the built-in catalog of the server binary prices the served model. Forms: `requests_total = 0` → `$0.00`; `requests_priced = 0` → `$?`; `requests_priced < requests_total` → `$X+`; else `$X`. **Notional prefix.** The hook reads `spend_notional` (absent → `false`); when `true`, `≈` goes in front of whichever form applies: `≈$1.84`, `≈$1.84+`, `≈$?`, `≈<$0.01`, `≈$0.00`, `≈$123`. The `≈` is one run in style `2`; the rest of the form is in style `32` (`$?` too). | author |
| AU16 | Tokens in = input + cache read + cache write of every request of the session (subagents included); tokens out = output tokens. This differs from `zygo usage`: its `tokens:` line (`crates/zygo/src/cmd/usage.rs:100`) prints input, output, cache read and cache write as four numbers, and its `in` is input only. HUD `↑` = the sum of `zygo usage` in + cache read + cache write. HUD spend and `zygo usage` spend also differ: passthrough rows hold `estimated_usd = 0` (§14), and a team session's rows reach the local store only at session end and with `estimated_usd = 0` (§14 N16 note), while the HUD shows the live notional value. | author |
| AU17 | The bar and the verbose percent measure `used ÷ D`, where D = `effective_window` (H§7.2; it includes the safety window) when it is below the model window, else the model window. The bar shows pressure toward compaction, which is the event the user cares about. D can differ from the window number printed after `/`: with model window 1m and threshold 600k, used 300k prints `ctx 300k/1m` with a half bar and `50%`, not `30%`. This is by design; `⇣600k` (AU28) shows the user the denominator. With a safety-only D, no `⇣` shows, and the bar still uses D. | author |
| AU18 | Bar fill: `cells = round_half_up(fraction × N)`, at least 1 when used > 0, at most N. At 20 % this gives 1.0 → 1 of 5 (compact) and 1.6 → 2 of 8 (verbose), as both OP13 mockups show. | author |
| AU19 | Truncation: each text segment has a maximum width in terminal cells (H§6.6). Text longer than that keeps its first cells and ends in `…` (U+2026). Escape first, then truncate on a character boundary. | author |
| AU20 | Failure display (H§6.8): local segments (stripe, logo, phase, work item, worktree, git, trimmed, profile from `view.json`) always render from their own sources; proxy-derived segments (model, profile fallback from status, mode, ctx with its numbers and percent, bar, threshold, tokens, spend, cache) use cached status data of any age. Stale (status data older than 90 s): every run of every proxy-derived segment gets the style `2` alone (dim, default foreground), whatever its normal style (bar `32`/`33`/`31`, red cache `31`, green `serve` `32`, plain `ctx 196k/` and percent all become `2`); then `stale` (style `2`) is added. Local segments keep their styles. Exact bytes: H§6.4 example 3. With no status data the HUD shows `offline` (red); a suspended team session shows `suspended` (yellow; the server resumes it on the next heartbeat, `liveness` and `Sessions::heartbeat` in `session.rs`). A session that is gone, the reply `401` with code `zygo_session_unknown` or `zygo_token_invalid` (`crates/zygo-proxy/src/errors.rs:22-27`), shows `ended` (red, style `31`). The cache stores the fetch `outcome` (`ok`, `offline`, `suspended`, `ended`) and `retry_at`; the marker is rendered from `outcome`. With `ended` the cached data is ignored (every proxy-derived segment is dropped) and `retry_at` = now + 300 s. | author |
| AU21 | Worktree segment: shown in both styles, right after the work item; Nerd icon `󰙅` U+F0645 (nf-md-file_tree, as the OP13 mockup), plain icon `⌂` U+2302; color 256-color 172 with a 16-color fallback of yellow `33` (OP13); name at most 24 cells (AU19). In a bare repository with worktrees, every worktree is linked by git's rule and shows; the "main worktree" of WS AU77 is a workspace naming rule and does not apply here. | author |
| AU22 | Withdrawn: the git dirty mark `●N` is dropped (OP14). | author |
| AU23 | 256 colors are used when `COLORTERM` is `truecolor` or `24bit`, or `TERM` contains `256color`, or `WT_SESSION` is set. Else the 16-color fallback of H§6.4 applies. | author |
| AU24 | Stdin: a worker parses the JSON value as soon as it is complete, or at 64 KiB, end of file or 200 ms, whichever comes first. A detached thread reads and discards the rest; the hook never waits for it (PH11). A value that is not a JSON object counts as empty input. Unknown keys are ignored; a missing or mistyped key drops only the segment that needs it. | author |
| AU25 | Workspace phase (phase 2): shown in both styles right after the logo (segment order: stripe, logo, phase, work item, …), only when the session is a workspace member and `phase` is `building`, `draining`, `stopping` or `down`; `up` and any other value hide it. Read fresh on each call, never cached. Never dropped for width. In compact it is in the first group (one space after the logo); in verbose row 1 it is its own group (two spaces after the logo, three before the work item). The file is `paths.workspaces_dir()/<ws_id>/workspace.json` (WS§7.1; `workspaces_dir` is a `Paths` method added by WS node C1, not a field). The id comes from `view.workspace.member.ws_id` (`SessionView.workspace: Option<ViewWorkspace>`, WS C1). The hook parses only `version` and `phase` with its own small serde struct; it does not depend on `zygo-workspace`. The hook checks `ws_id` against `^[0-9a-f]{16}$` (WS AU84) before it builds the path, and hides the segment when the id fails, the file is missing or does not parse, or `version` is not 1 (WS AU29: a newer version is not read). | author |
| AU26 | `zygo status` switches its trimmed text to tokens: `savings: cache 92%, trimmed 23k tok`, and adds `spend: $3.41` (AU15 forms, with `≈` when notional; no line outside a session). `zygo usage` keeps the exact `bytes trimmed: N` line and adds `tokens trimmed (estimate): M`. | author |
| AU27 | The trimmed segment uses `✂` U+2702 in the Nerd set (the approved mockup) and the word `trim` in the plain set (U+2702 is not in the console fonts of H§6.5). | author |
| AU28 | The `compact <n>` / `compact off` segment of §10.2.4 is removed. The proxy sends `threshold` only when a numeric override or a soft window is set; its value is that number clamped by the safety window (`min`), as trigger 1 uses it (`soft_threshold` in `crates/zygo-proxy/src/window.rs`). Example: soft 190k and safety 180k → `threshold` 180k → `⇣180k` (180k is below a 200k model window). A safety window alone gives no `threshold` field, so no `⇣`; the bar still uses `effective_window` (AU17). `⇣<n>` shows when `threshold` is present and below the model window. An override of `off` (status `compact = off`) shows `⇣off` whenever that value is known, also with an unknown model window. A numeric `threshold` with an unknown model window shows no `⇣`. | author |
| AU29 | The proxy's status `effective_window`, `threshold` and `context_window` follow the last main-loop request. At request time, after resolution, the proxy looks up the catalog row of `resolved.model` in that request's serving config (`ctx.cfg`) and keeps in `Pending` what the status needs of it: `soft_context_window`, `facts.context_window` and the request's agent window, computed with the rule of the window stage, `if one_m(resolved.one_m, &headers) { ONE_M_WINDOW } else { agent_window(row) }` (`crates/zygo-proxy/src/window.rs`, `agent_window_for`). `Pending::drop` stores these values with the served model in the session under the existing `used_context` condition (route `/anthropic/v1/messages`, 2xx, class `main` or absent, prompt > 0), also when the stream ends before `message_delta`. `status()` computes `context_window`, `threshold` and `effective_window` from the stored values and the current override each time; it does not read the session's config snapshot for them, so after a serve-mode reload they stay those of the row that served the request. So a non-streamed main-loop request stores them too (the condition does not test streaming), and an OpenCode request on the Anthropic route stores its model with `agent_window(row)` (OpenCode sends no `[1m]` name and no `context-1m-` beta); requests on other routes, non-2xx replies, subagent and compaction requests store nothing, and the status keeps the previous values. A served model with no row in the serving config (an unmatched passthrough name) gives `model` and no windows. Before the first main-loop request, also after a serve-mode reload, it uses the main role's model and row from the session's config snapshot and `agent_window(row)` (today's rule, which ignores `[1m]`). In passthrough it sends no `model` (and no windows) until a request is served, because the main role does not apply. After `/model` picks another role, the HUD shows that role's window. | author |
| AU30 | Windows: the HUD has the same output on Windows. Claude Code runs the status line through Git Bash there (`docs/live-checks.md`, Results row W-SHELL). Nerd icons follow AU2. ANSI color is on unless `NO_COLOR` is set and non-empty. | author |
| AU31 | Withdrawn: client-side pricing of per-model token sums; replaced by AU14. | author |
| AU32 | Price sanity. `config check` rejects a price that is negative, NaN or infinite, and a `long_context.above` of 0. `Prices::cost` counts a non-finite result as not priced. A value above $99999 shows `>$99999`. | author |
| AU33 | Cache-write TTL. Today the tee reads only `cache_creation_input_tokens` (`crates/zygo-proxy/src/tee.rs:60`, `:71`), and the catalog `cache_write` is the 1-hour rate (`catalog.toml:27`). Anthropic usage also carries `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` (seen in `fixtures/claude-code/tool-output/Agent.json:42-44`). The tee also reads them when present; `Prices` gains an optional `cache_write_5m` rate; `Prices::cost` prices 5-minute writes at `cache_write_5m` (else `cache_write`), 1-hour writes at `cache_write`, and writes with no split at `cache_write`. Each Anthropic row of the built-in catalog gets `cache_write_5m` = 1.25 × its `input` rate (H§7.3). Without the split, 5-minute writes are overpriced at the 1-hour rate; this is accepted for an estimate. The usage row keeps one `cache_write_tokens` total (no schema change). | author |

### 6.2 Segments

Order is fixed (AU9). "C" = shown in compact. "V" = shown in verbose (row).

| # | Segment | Source | Color | C | V |
|---|---|---|---|---|---|
| 0 | Stripe `▌▌▌` | constant (OP12) | `31`, `33`, `36` | yes | rows 1 and 2 |
| 1 | Logo `𝘇𝘆𝗴𝗼`; reflection `zʎƃo` | constant (OP11) | `1;97`; reflection `2;37` | logo | logo row 1, reflection row 2 |
| 2 | Workspace phase `ws draining` (phase 2) | `workspace.json` (WS§7.2) | H§6.4 | yes, when not `up` | row 1 |
| 3 | Work item | `ZYGO_WORK`, else status `work`; else live branch (AU11) | bold blue `1;34` | yes | row 1 |
| 4 | Worktree `<icon> name` | git worker (AU6, AU21) | `38;5;172` | yes, when linked | row 1 |
| 5 | Git ahead `↑N`, behind `↓N` | git worker (AU6) | `36` | no | row 1 |
| 6 | Model; effort | status `model` (AU10); stdin `effort` | `35` (one span) | model only | row 1, `opus-4.7 · high` |
| 7 | Profile `<icon> name` | `view.json`, else status (AU12) | `36` | yes | row 1 |
| 8 | Mode | `ZYGO_URL`, status `passthrough` (AU13) | `2`; `serve` green | no | row 1 |
| 9 | ctx `used/window` + bar | status `used_context`, `context_window` (OP2); no bar and no percent when either is unknown (AU7) | used default, window `2`; bar H§6.4 | 5 cells | row 2, 8 cells + percent |
| 10 | Threshold `⇣N` | status `threshold`, `compact` (AU28) | `2` | yes, when shown (AU28) | row 2 |
| 11 | Tokens | status `tokens_in`, `tokens_out` | `2` | `↑1.2m ↓48k` | row 2, `↑ 1.2m in  ↓ 48k out` |
| 12 | Spend `$X` / `≈$X` | status `spend_usd`, `spend_notional`, `requests_total`, `requests_priced` (AU15) | `32`; `≈` `2` | yes | row 2 |
| 13 | Cache | status `cache_read_share` | `2`; `31` below 50 % | `󰆼 92%` | row 2, `cache 92%` |
| 14 | Trimmed | usage store, bytes ÷ 4 (OP1), session id from `view.json` (H§7.4) | `2` | `✂ 23k` | row 2, `trimmed 23k tok` |
| 15 | `stale` / `offline` / `suspended` / `ended` | cache `outcome` and status age (AU20) | `2` / `31` / `33` / `31` | yes, when it applies | row 2 (row 2 always exists: it holds the reflection) |

Work item markers (OP3). The hook splits `ZYGO_WORK` at the first `:` into source and id (source names hold no `:`, `crates/zygo-work/src/sources.rs:34-37`). An empty value counts as absent (AU11).

| Source | Id form (where it is built) | Rule | Example | Nerd | Plain |
|---|---|---|---|---|---|
| `branch` | branch name (`sources.rs:306-316`) | the id as it is | `branch:main` | `<U+E0A0> main` | `main` |
| `github` | `owner/name#N` (`sources.rs:267`, `:280`) | the text from the last `#` to the end | `github:o/n#123` | `<U+F408> #123` | `#123` |
| `work-graph` | `program/node` (`sources.rs:156`) | `◆` + the id | `work-graph:hud/N3` | `◆ hud/N3` | `♦ hud/N3` |
| `beads` | `prefix-hash[.n]` (`sources.rs:220`) | the id as it is | `beads:zy-a1b2` | `<U+F41B> zy-a1b2` | `○ zy-a1b2` |
| any other source, a `github` id with no `#`, or a value with no `:` | — | the whole value | `x:y` | `x:y` | `x:y` |

Every form is escaped (H-C6), then truncated (AU19).

### 6.3 Mockups

**Reference mockups (OP13, approved; the verbose one as amended by OP14: no `●3` run).** Run each command in a terminal with a Nerd Font. The HUD's output equals these bytes for the same data (acceptance 3). The OP13 text the author received had lost the branch and model glyphs; the author put back U+E0A0 and U+F4BC (AU10).

Verbose:

```sh
printf '\e[31m▌\e[33m▌\e[36m▌\e[0m \e[1;97m𝘇𝘆𝗴𝗼\e[0m  \e[1;34m main\e[0m \e[38;5;172m󰙅 hud-work\e[0m \e[36m↑1\e[0m   \e[35m opus-4.7 · high\e[0m   \e[36m󰘦 fast\e[0m   \e[2mlocal\e[0m\n\e[31m▌\e[33m▌\e[36m▌\e[0m \e[2;37mzʎƃo\e[0m  ctx 196k/\e[2m1m\e[0m \e[32m▰▰\e[2m▱▱▱▱▱▱\e[0m 20%%   \e[2m↑ 1.2m in  ↓ 48k out\e[0m   \e[32m$3.41\e[0m   \e[2mcache 92%%   trimmed 23k tok\e[0m\n'
```

Compact:

```sh
printf '\e[31m▌\e[33m▌\e[36m▌\e[0m \e[1;97m𝘇𝘆𝗴𝗼\e[0m \e[1;34m main\e[0m \e[38;5;172m󰙅 hud-work\e[0m  \e[35m opus-4.7\e[0m \e[36m󰘦 fast\e[0m  ctx 196k/\e[2m1m\e[0m \e[32m▰\e[2m▱▱▱▱\e[0m  \e[2m↑1.2m ↓48k\e[0m  \e[32m$3.41\e[0m  \e[2m󰆼 92%%  ✂ 23k\e[0m\n'
```

The same mockups as plain text (the stripe is colored in a real terminal; a viewer without a Nerd Font shows boxes for some icons; H§6.5 lists each code point). Model names follow AU10 (`opus-4.7` is an example name).

**Compact, Nerd, linked worktree (OP13):**

```
▌▌▌ 𝘇𝘆𝗴𝗼  main 󰙅 hud-work   opus-4.7 󰘦 fast  ctx 196k/1m ▰▱▱▱▱  ↑1.2m ↓48k  $3.41  󰆼 92%  ✂ 23k
```

**Verbose, Nerd (OP13; the stripe is one continuous column, the reflection is under the logo):**

```
▌▌▌ 𝘇𝘆𝗴𝗼   main 󰙅 hud-work ↑1    opus-4.7 · high   󰘦 fast   local
▌▌▌ zʎƃo  ctx 196k/1m ▰▰▱▱▱▱▱▱ 20%   ↑ 1.2m in  ↓ 48k out   $3.41   cache 92%   trimmed 23k tok
```

**Compact, Nerd, threshold set, main worktree:**

```
▌▌▌ 𝘇𝘆𝗴𝗼  main   opus-4.7 󰘦 fast  ctx 196k/1m ▰▰▱▱▱ ⇣600k  ↑1.2m ↓48k  $3.41  󰆼 92%  ✂ 23k
```

**Compact, plain icons, passthrough:**

```
▌▌▌ zygo main ⌂ hud-work  opus-4.7 ≡ fast  ctx 196k/1m ■□□□□  ↑1.2m ↓48k  ≈$1.84  cache 92%  trim 23k
```

**Verbose, plain icons (four spaces in place of the reflection, AU8):**

```
▌▌▌ zygo  main ⌂ hud-work ↑1   opus-4.7 · high   ≡ fast   local
▌▌▌       ctx 196k/1m ■■□□□□□□ 20%   ↑ 1.2m in  ↓ 48k out   $3.41   cache 92%   trimmed 23k tok
```

**Verbose, workspace draining, serve mode (client-side notional spend, AU14):**

```
▌▌▌ 𝘇𝘆𝗴𝗼  ws draining    main    opus-4.7 · high   󰘦 fast   serve zygo.example.org
▌▌▌ zʎƃo  ctx 512k/1m ▰▰▰▰▰▰▰▱ 85% ⇣600k   ↑ 9.8m in  ↓ 210k out   ≈$21.07   cache 88%   trimmed 1.1m tok
```

**Compact, `NO_COLOR` set:** the same text with no stripe and no escape bytes.

```
𝘇𝘆𝗴𝗼  main 󰙅 hud-work   opus-4.7 󰘦 fast  ctx 196k/1m ▰▱▱▱▱  ↑1.2m ↓48k  $3.41  󰆼 92%  ✂ 23k
```

### 6.4 Colors

SGR codes. **Emission algorithm (author).** It reproduces both OP13 mockups byte for byte (checked by hand against each escape of the two `printf` commands; acceptance 3, 4).

1. **Style.** A style is a pair (intensity, foreground). Intensity is `normal`, `bold` (`1`) or `dim` (`2`). Foreground is `default`, a 16-color code (`31`-`37`, `97`) or a 256-color code (`38;5;n`). Two styles are identical when both parts are equal. The empty style is (normal, default).
2. **Runs.** The renderer builds each row as a list of runs (style, text): each segment gives one or more runs (the stripe gives three; the ctx segment gives `ctx 196k/` empty, `1m` (dim, default), a gap, the filled cells, the empty cells, then for verbose a gap and the percent with the empty style; a model segment gives one run of icon, name, ` · ` and effort). An icon and the space after it are in the run of their text. Separators between segments, and spaces inside a segment that have no style of their own, are **gap** runs: text of spaces only, empty style.
3. **Gap absorption.** A gap run whose previous and next non-gap runs have identical styles takes that style. Every other gap keeps the empty style.
4. **Merge.** Consecutive runs with identical styles become one run.
5. **Bar empty cells.** The empty cells have the style (dim, fg of the filled cells) when filled cells exist, else (dim, default). This is what `\e[32m▰▰\e[2m▱▱▱▱▱▱` means: the `2` adds dim to the green.
6. **Transition.** The emitter keeps the current style, first the empty style. For each run, from current C to next N:
   - N = C: emit nothing.
   - Nothing must be turned off (C intensity is `normal` or equals N intensity, and C foreground is `default` or N foreground is not `default`): emit one escape with the parts that change, intensity code first, then foreground code, joined by `;` (`\e[1;97m`, `\e[33m`, `\e[2m`).
   - Else (intensity must go back to normal, or to another non-normal value, or the foreground must go back to default): emit `\e[0m`, then, when N is not empty, one escape with all non-default parts of N.
   Then emit the run's text.
7. **Row end.** When the current style is not empty, emit `\e[0m`. Then `\n` after row 1 of verbose; no newline after the last row (as today, `statusline.rs:50` prints the text as it is).
8. **No color.** With `NO_COLOR` set and non-empty, the emitter emits no escape at all, and it leaves out the three stripe runs and the gap run that follows them, so each row starts with the logo (or, in verbose row 2, the reflection or its four spaces) (AU8; the `NO_COLOR` mockup in H§6.3).
9. **16 colors.** Before emission, `38;5;172` becomes `33` when AU23 selects 16 colors.

Examples (Nerd set, color, 256 colors):

1. Compact, threshold 600k, used 196k, model window 1m (bar fraction 0.33 → 2 of 5 cells; AU17):
```sh
printf '\e[31m▌\e[33m▌\e[36m▌\e[0m \e[1;97m𝘇𝘆𝗴𝗼\e[0m \e[1;34m main\e[0m  \e[35m opus-4.7\e[0m \e[36m󰘦 fast\e[0m  ctx 196k/\e[2m1m\e[0m \e[32m▰▰\e[2m▱▱▱\e[0m \e[2m⇣600k  ↑1.2m ↓48k\e[0m  \e[32m$3.41\e[0m  \e[2m󰆼 92%%  ✂ 23k\e[0m'
```
   The `⇣600k` run and the tokens run are both (dim, default), so the two-space gap between them is absorbed and they share one escape.
2. Compact, OP13 data: the OP13 compact `printf` without its final `\n`.
3. Compact, stale (OP13 data, status data 100 s old; the worktree and the profile from `view.json` are local and keep their styles; the trimmed run is already `2` and merges with the dim proxy runs and `stale`):
```sh
printf '\e[31m▌\e[33m▌\e[36m▌\e[0m \e[1;97m𝘇𝘆𝗴𝗼\e[0m \e[1;34m main\e[0m \e[38;5;172m󰙅 hud-work\e[0m  \e[2m opus-4.7\e[0m \e[36m󰘦 fast\e[0m  \e[2mctx 196k/1m ▰▱▱▱▱  ↑1.2m ↓48k  $3.41  󰆼 92%%  ✂ 23k  stale\e[0m'
```

| Use | SGR (256-color) | SGR (16-color fallback) |
|---|---|---|
| Stripe bars | `31`, `33`, `36` (`statusline.rs:56`) | same |
| Logo `𝘇𝘆𝗴𝗼` (plain set: `zygo`) | `1;97` (bold bright white) | same |
| Reflection `zʎƃo` (verbose row 2) | `2;37` (dim white) | same |
| Work item, branch | `1;34` (bold blue) | same |
| Live branch with no work item (AU11) | `34` | same |
| Git ahead/behind | `36` | same |
| Worktree, icon and name (OP9, OP13) | `38;5;172` (deep amber, #d78700) | `33` |
| Model and effort, one span (`opus-4.7 · high`) | `35` | same |
| Profile, icon and name | `36` | same |
| Mode `local`, `passthrough` | `2` | same |
| Mode `serve <host>` | `32` | same |
| ctx `used/` and the verbose percent | none (terminal default) | same |
| ctx window number (`1m`) | `2` | same |
| Bar filled, fraction < 60 % | `32` | same |
| Bar filled, 60 % ≤ fraction < 85 % | `33` | same |
| Bar filled, fraction ≥ 85 % | `31` | same |
| Bar empty cells | `2` | same |
| Threshold `⇣N` | `2` | same |
| Tokens | `2` | same |
| Spend | `32` | same |
| `≈` of notional spend | `2` | same |
| Cache, share ≥ 50 % | `2` | same |
| Cache, share < 50 % | `31` | same |
| Trimmed | `2` | same |
| `ws building` | `33` | same |
| `ws draining` | `31` | same |
| `ws stopping` | `1;31` | same |
| `ws down` | `2` | same |
| `stale` | `2` | same |
| `offline` | `31` | same |
| `suspended` | `33` | same |
| `ended` | `31` | same |

Worktree color (operator, OP13): 172 (#d78700). The author's earlier 136 is withdrawn.

With `NO_COLOR` set and non-empty, the HUD prints no escape byte at all and omits the stripe and the space after it (AU8, step 8).

### 6.5 Icons

| Use | Nerd set | Code point | Plain set | Code point |
|---|---|---|---|---|
| Stripe | `▌` | U+258C | `▌` | U+258C |
| Logo | `𝘇𝘆𝗴𝗼` | U+1D607 U+1D606 U+1D5F4 U+1D5FC | `zygo` | ASCII |
| Reflection (verbose row 2) | `zʎƃo` | U+007A U+028E U+0183 U+006F | four spaces | — |
| Branch | `` (Powerline branch) | U+E0A0 | none | — |
| GitHub issue | `` (nf-oct-mark_github) | U+F408 | none (`#123` is the marker) | — |
| Work graph | `◆` | U+25C6 | `♦` | U+2666 |
| Bead | `` (nf-oct-issue_opened) | U+F41B | `○` | U+25CB |
| Worktree | `󰙅` (nf-md-file_tree) | U+F0645 | `⌂` | U+2302 |
| Ahead / behind; tokens in / out | `↑` `↓` | U+2191, U+2193 | `↑` `↓` | U+2191, U+2193 |
| Model | `` (nf-oct-cpu) | U+F4BC | none | — |
| Profile | `󰘦` (the approved mockup's glyph) | U+F0626 | `≡` | U+2261 |
| Bar filled / empty | `▰` `▱` | U+25B0, U+25B1 | `■` `□` | U+25A0, U+25A1 |
| Threshold | `⇣` | U+21E3 | `▼` | U+25BC |
| Notional | `≈` | U+2248 | `≈` | U+2248 |
| Cache | `󰆼` (nf-md-database) | U+F01BC | word `cache` | — |
| Trimmed | `✂` | U+2702 | word `trim` | — |
| Truncation | `…` | U+2026 | `…` | U+2026 |

Rule for the plain set (author): each plain glyph is in WGL4, the character set of the Windows console fonts (Consolas, Lucida Console) and of common Linux console fonts. Verbose uses the words `cache`, `trimmed … tok`, `in` and `out` in both sets, as the OP13 mockup shows; it has no `profile` or `effort` word.

### 6.6 Formats and limits

- Token counts and trimmed tokens use `short` (`statusline.rs:143-154`): `999`, `412k`, `1m`, `1.2m`.
- Trimmed tokens = `ceil(trimmed_bytes ÷ BYTES_PER_TOKEN)`, with `BYTES_PER_TOKEN = 4`, one constant in `zygo-core` (`zygo_core::tokens`) that both estimates in `run` (`forward.rs`) also use (OP1). 92,160 bytes → 23,040 → `23k`.
- Spend: round to whole cents first (half up), then choose the form from the rounded value: 0 cents with spend > 0 → `<$0.01`; below $100.00 → two decimals (`$3.41`); $100.00 and above → whole dollars, half up (`$123`). Example: $99.996 → $100.00 → `$100`. Suffix `+` or the form `$?` per AU15. Notional: prefix `≈`.
- Percent (verbose ctx, cache): `round(x × 100)` then `%`, as `pct` (`statusline.rs:156-158`).
- Maximum widths (AU19), in cells: work item 32, worktree name 24, model 24, profile 16, effort 8, serve host 32, live branch 32.

### 6.7 Width

A hook cannot see the terminal width. The HUD first finds the terminal width T:

1. Unix: the size of the controlling terminal, `TIOCGWINSZ` on `/dev/tty` (no write; skipped when `/dev/tty` cannot be opened or the call fails). Inside tmux this is the pane's size, so tmux needs no step of its own. Under Claude Code 2.1.330 this source does not answer (PH4);
2. `COLUMNS` from the environment. Claude Code 2.1.330 sets it to the terminal width for the status line command (PH4);
3. else no source answers.

A value outside 20 to 1000 from any source is skipped. Each source is a premise (PH4).

Claude Code draws the status line 2 cells from the left edge of the terminal and cuts it with `…` 2 cells before the right edge (PH4, P-HUD-1 2026-11-22). The HUD fits each row to the width W of that status area: W = T − 4 when a source answers (the indent 2 and the right gap 2 are named constants in the code); W = 120 when no source answers (120 is an assumed status-area width, so nothing is subtracted). The session log names the source and T, not W (P-HUD-1).

Width counting uses Unicode East Asian width (the `unicode-width` crate). The logo counts 4 cells (PH9). Each Nerd private-use glyph counts as 2 cells, which is safe for both "Mono" and proportional Nerd Font variants. Escape bytes count 0.

When a row is wider than W, the HUD removes whole segments, first to last in this order, until the row fits:

- **Compact:** trimmed → cache → tokens → threshold `⇣` → spend → profile → model → bar cells (the numbers stay) → `stale` → worktree → ctx numbers → work item.
- **Verbose row 1:** mode → profile → effort → ahead/behind → model → worktree → work item. When row 1 drops a segment, row 2 does not change: the logo and the reflection keep their column.
- **Verbose row 2:** trimmed → cache → tokens → spend → percent → threshold → bar cells → `stale` → ctx numbers.

The stripe, the logo and its reflection, the workspace phase and `offline`/`suspended`/`ended` are never removed. A row that still does not fit is printed as it is; Claude Code then cuts or wraps it (premise PH3).

### 6.8 When data is missing

One outcome per condition. The conditions are independent; each row changes only the segments it names.

| Condition | Outcome |
|---|---|
| No `ZYGO_SESSION_DIR` | The hook prints nothing (as today, `statusline.rs:30`). |
| Status fetch fails or passes the deadline; cached status data 90 s old or less | Proxy-derived segments (AU20) from the cache, normal colors. |
| Status fetch fails or passes the deadline; cached status data older than 90 s | Proxy-derived segments from the cache in SGR `2`, and `stale`. |
| Status fetch fails; no cached status data; the error is a transport error, a timeout or another HTTP error | No proxy-derived segment; `offline`. |
| Status reply `401` `zygo_session_unknown` or `zygo_token_invalid` (`errors.rs:22-27`) | `outcome = ended`: no proxy-derived segment (the cached data is ignored); `ended` (SGR `31`); `retry_at` = now + 300 s (AU20). |
| Status fetch fails with `zygo_session_suspended` (`crates/zygo-proxy/src/errors.rs:28-30`) | Proxy-derived segments from cached status data when it exists (dim when older than 90 s); the marker is `suspended` (SGR `33`), never `offline` or `stale`. |
| Status data has no `model` | No model segment (AU10). |
| Status data has no `context_window` (the model is not in the catalog, or its row has no window) | `ctx <used>/-`, no bar, no percent, no numeric `⇣<n>`; `⇣off` still shows when `compact = off` (AU28). |
| Status data has no `used_context` | `ctx -/<window>`, no bar, no percent. |
| Status data lacks a new field (old server) | The segment that needs that field is dropped (all new fields are `Option`, H§7.2). |
| Git missing, not a repository, or git timeout | No ahead/behind, no worktree, no live branch. |
| Usage store missing or busy, or `view.json` unreadable | No trimmed segment (`statusline.rs:160-164` gives `None`). |
| `ZYGO_WORK` empty or unset and no status `work` and no live branch | No work item segment. |
| A new launch (also a workspace resume, which is a new zygo session, WS§8.2) | Tokens, spend, cache and trimmed start at 0 for this launch; the HUD shows this launch only. Until the first main-loop request, `used_context` is unknown: `ctx -/<window>` (AU7). |

## 7. Data sources

```
   Claude Code ──stdin JSON (≤64 KiB, fresh)──┐
                                              │
   environment: ZYGO_SESSION_DIR, ZYGO_WORK,  │
   ZYGO_URL, NO_COLOR, ZYGO_NERD_FONT, TERM,  │
   COLORTERM, WT_SESSION, COLUMNS ────────────┤
   /dev/tty (TIOCGWINSZ) ─────────────────────┤
                                              ▼
   session dir: view.json, overrides.json ─▶ zygo hook statusline ──▶ stdout: 1 or 2 rows
   (fresh)                                    │  ▲
                                              │  │ statusline.json (status + git, 30 s)
            ┌─────────────────────────────────┼──┴──────────────────┐
            ▼ worker 1 (≤ fetch cap)          ▼ worker 2 (≤ 400 ms)  ▼ fresh read (phase 2)
   GET /v1/zygo/sessions/{id}/status    1 git rev-parse        paths.workspaces_dir()/<id>/
   (local proxy or team server)         2 git rev-list --count        workspace.json `phase`
                                        (in the stdin cwd;            (WS§7.1, WS§7.2)
                                         GIT_NO_LAZY_FETCH=1)
   worker 3 (≤ deadline, 1 s cap): usage.db read-only, one indexed SUM of trimmed bytes for SessionView.session (no cache)
   lease: statusline.lock (create_new); cache: statusline.json (one file)
```

### 7.1 Stdin

Claude Code sends a JSON object to the status line command. The top-level keys seen on 2.1.328 are `context_window`, `cost`, `cwd`, `effort`, `exceeds_200k_tokens`, `fast_mode`, `model`, `output_style`, `session_id`, `thinking`, `transcript_path`, `version`, `workspace` (`fixtures/claude-code/q7/q7-claude.json:191-204`). No fixture holds a full payload, so the nested shapes are a premise (PH5).

The HUD reads only:

| Key | Use |
|---|---|
| `cwd` (string), else `workspace.current_dir` (string) | The git worker's directory. Else the hook's own cwd. |
| `effort` (string, or an object with a string level; PH5) | Verbose effort. |

The HUD never reads `cost` (OP5), never shows the stdin `model` (a role name, AU10), and never uses the stdin `context_window` as the model window. It never uses Claude Code's context numbers for `used`: the §10.2.2 rewrite raises the usage that Claude Code sees, so its numbers are wrong after a trigger.

Today the dispatcher reads stdin with no bound (`crates/zygo/src/cmd/hook.rs:47-49`), and `HookInput::parse` fails on input that is not a JSON object (`crates/zygo-core/src/hook.rs:21-28`), so bad stdin gives no line at all. AU24 changes this for the status line only: its table entry stops the shared read (`reads_stdin: false`, as `token` at `hook.rs:15`), and the hook keeps at most 64 KiB itself, on a worker under the deadline, and discards the rest (AU24).

### 7.2 Proxy status reply

`GET /v1/zygo/sessions/{id}/status` (`crates/zygo-proxy/src/routes.rs:231-235`) returns `SessionStatus` (struct `SessionStatus` in `crates/zygo-proxy/src/client.rs`). The struct has no `deny_unknown_fields`, so new fields are compatible in both directions. Every new field is an `Option` with `#[serde(default)]`, so a reply from an old server parses and each missing field is `None` (acceptance 17).

| Field | Type | New | Meaning |
|---|---|---|---|
| `session`, `profile`, `work`, `used_context`, `compact`, `cache_read_share`, `expires_at`, `usage_note` | — | no | As today. |
| `effective_window` | `Option<u64>` | changed | §10.2.1 `effective` for `model` (AU29). |
| `model` | `Option<String>` | yes | Catalog id of the model of the last main-loop 2xx Messages request with a prompt above 0, stored with the row values of AU29 when the request's usage row is written (`Pending::drop`), under the same condition as `used_context`, also when the stream ends before `message_delta`. Before the first one, the main role's model from the session's config snapshot; in passthrough, absent from the reply until a request is served (AU29). An unmatched passthrough name is the requested name after the secret pass, with `[1m]` removed (`requested_name`, `passthrough_names` in `crates/zygo-proxy/src/forward.rs`), cut to at most 128 bytes on a character boundary; when that name is empty (the secret pass blocked or failed), `model` is absent. |
| `context_window` | `Option<u64>` | yes | `facts.context_window` of `model`'s catalog row, taken from the serving config when the request is served and stored (AU29); before the first main-loop request, from the main role's row in the session's config snapshot. |
| `threshold` | `Option<u64>` | yes | Present only when a numeric override or the row's `soft_context_window` exists; value = that number, clamped by the safety window (`min`), for `model` (`soft_threshold`, `crates/zygo-proxy/src/window.rs`, made public), from the row values stored at request time from the serving config (AU29; before the first main-loop request, the main role's row in the session's config snapshot). Absent with a safety window alone, with no soft window and no override, and with the override `off` (AU28). |
| `tokens_in` | `Option<u64>` | yes | AU16. |
| `tokens_out` | `Option<u64>` | yes | AU16. |
| `spend_usd` | `Option<f64>` | yes | Session spend (H§7.3). From a local proxy: catalog prices (notional prices in passthrough), 0.0 with no request. From a team server: built-in list prices (AU14). Absent from an old server (AU15). |
| `spend_notional` | `Option<bool>` | yes | `true` for a passthrough session and for every team session. |
| `requests_total` | `Option<u64>` | yes | Requests whose usage has at least one token (AU15). |
| `requests_priced` | `Option<u64>` | yes | Of those, the requests that were priced (AU15: route price or notional price locally; built-in catalog price on a team server). |
| `passthrough` | `Option<bool>` | yes | `SessionState.passthrough` (`session.rs`). |
| `team_session` | `Option<bool>` | yes | `SessionState.team_label.is_some()` (`session.rs`); the label itself is not sent (AU13). |
| `requests_in_flight`, `last_request_end` | — | yes, by WS AU91 | Planned by the workspaces design (WS AU91, approved by WS OP13). The HUD does not use them; WS X1 adds them after the HUD's fields (H§17 note 1). |

### 7.3 Spend

Today the proxy already reads output tokens (`crates/zygo-proxy/src/tee.rs:25-30`) and computes a cost per request for the usage row (`Pending::drop` in `forward.rs`). The session sum keeps only input, cache read and cache write (`PromptUsage` and `SessionState::add_usage` in `session.rs`). The changes:

1. **Price function** (AU5) in `zygo-config`: `Prices::cost(&usage) -> Option<f64>`. `prompt = input + cache_read + cache_write` (§10.2.1). When the table has `long_context` and `prompt > long_context.above`, all rates come from `long_context`; else the base rates. Cache writes follow AU33. Result = Σ tokens × rate ÷ 10⁶; a non-finite result is `None` (AU32).
2. **Catalog tier.** `Prices` (`crates/zygo-config/src/schema.rs:216-223`) gains `long_context: Option<LongContextPrices>` with `above` (tokens) and the four rates. The Haiku row's comment (`catalog.toml:111`, "prices are for prompts up to 100,000 tokens; above that: 0.50 / 2.50 / 0.05 / 1.00") becomes data: `long_context = { above = 100000, input = 0.50, output = 2.50, cache_read = 0.05, cache_write = 1.00 }`. The builder checks the provider's page and records `source` and `verified_on` (§19a). Each Anthropic row also gets `cache_write_5m` (AU33).
3. **Row and session.** `Pending::drop` (`forward.rs`) calls `Prices::cost` once. It writes the row's `estimated_usd` as today (now with the tier rule) and adds to the session: tokens in, tokens out, spend, `requests_total`, `requests_priced`.
4. **Passthrough.** The passthrough stage clears the price (`crates/zygo-proxy/src/passthrough.rs:226`), so the row cost is 0 (§14 keeps this). The stage finds the catalog match by the rule of `passthrough.rs:181-184` and `:200-206`: the first catalog model, in config order, with a provider on the credential type's dialect (`t.dialect`) whose `upstream_model` is the requested name without `[1m]` (a disabled provider counts). It keeps the price of the first provider of that row on `t.dialect` that has a price as `notional_prices`. The session spend uses it; the row does not. No match, or no priced provider: the request is not priced.
5. **Team session.** The team server computes the row's `estimated_usd` with its configured prices, as today, and keeps it in its own database. For the session spend it calls `Prices::cost` a second time with the price table from the built-in catalog of its binary (`zygo_config` built-in data; the first provider of the served model's row that has a price; the served model's catalog id, so the per-request tier test is exact). Its status reply carries that sum with `spend_notional = true` (AU14). No configured price leaves the server.

### 7.4 Usage store

The trimmed count is the sum of `stored_bytes − view_bytes` over the session's `trims` rows (today inside `summary`, `crates/zygo-usage/src/query.rs:179-183`, whose `?1 IS NULL OR session = ?1` form may not use the index), written by the trim hook (`crates/zygo-hooks/src/trim.rs:118-129`). A new `Store::bytes_trimmed(session)` runs `SELECT COALESCE(SUM(MAX(stored_bytes - view_bytes, 0)), 0) FROM trims WHERE session = ?1`, which uses the `trims_session` index (`crates/zygo-usage/src/schema.rs:63`), with the store opened read-only. Today the session id comes from the status reply (`statusline.rs:60-63`), so the count needs a working proxy. The HUD takes the id from `SessionView.session` (`crates/zygo-core/src/session_view.rs:8`) and runs the query on worker 3, apart from the status fetch, bounded by the same deadline and by a 1 s cap; a query that does not finish gives no trimmed segment. `stale` never dims it. The HUD converts the count to tokens (OP1).

### 7.5 Git

The worker uses the pinned git runner of `zygo-work`: `git_run` (`crates/zygo-work/src/repo.rs:99-110`, on `run_bounded_err`, `:52-94`, which kills the child at the deadline) sets `-c core.fsmonitor=false`, `GIT_OPTIONAL_LOCKS=0` and `GIT_TERMINAL_PROMPT=0`; `git_command` (`crates/zygo-config/src/repo_file.rs:79-91`) removes the repository-selecting `GIT_*` and every `GIT_CONFIG*` variable and sets `LC_ALL=C`. `git_run` and `find_checkout` (`repo.rs:119`) are both `pub(crate)`, so `zygo-work` gains one new public function for the HUD, built on `git_run`, that keeps stdout as bytes and adds `GIT_NO_LAZY_FETCH=1`. The HUD adds no fsmonitor override of its own: `rev-parse` and `rev-list` do not run the fsmonitor hook (evidence below), and `git_run` already passes `core.fsmonitor=false`. `zygo-hooks` already depends on `zygo-work`.

1. `git rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir --symbolic-full-name HEAD` (150 ms). Linked worktree ⇔ canonical git dir ≠ canonical common dir. This is the same answer as `Checkout.top != Checkout.main_root` (`repo.rs:128-133`) without the `worktree list` call: the main worktree's git dir is the common dir; a linked worktree's git dir is `<common>/worktrees/<id>`; in a bare repository every worktree is linked by both rules. The output is parsed as bytes and must have exactly four lines; a path that holds a newline gives more, and then the worktree and branch are dropped. Name = the final component of `--show-toplevel`, decoded lossily (invalid UTF-8 → U+FFFD), then escaped (H-C6). The fourth line gives the live branch (AU11): `refs/heads/<name>` on a branch, also when a tag has the same name (`--abbrev-ref` would print `heads/<name>` then); the branch is `<name>`. `HEAD` means detached; any other line gives no branch. A non-zero exit gives no git result (an unborn HEAD exits 128). Exception: when the hook's parent ignores SIGCHLD, the kernel reaps git and the exit status is lost, so output that passes the checks counts; an unborn HEAD then shows the worktree with no branch.
2. `git rev-list --left-right --count HEAD...@{upstream} --` (the rest of the 400 ms): `<ahead>\t<behind>`. The `--` ends the revisions, so a file in the cwd named like the range cannot make the argument ambiguous. No upstream, or any error → no ahead/behind marks.

Neither command reads the index or the working tree, so no filter, attribute, submodule or index hook applies. Neither has a fixed output size beyond a few lines; the reader keeps at most 4 KiB.

Evidence (author, git 2.55.0, 2026-11-21, scratch directory `.../scratchpad/op14`): a clone with `core.fsmonitor` set to a script that writes a marker, `core.hooksPath` holding `post-index-change`, `reference-transaction` and `post-checkout` scripts that write markers, and `filter.m.clean` (writes a marker) named in `.gitattributes` for a stat-dirty file. Calls 1 and 2 (and `rev-parse --abbrev-ref --symbolic-full-name @{upstream}`), with `GIT_NO_LAZY_FETCH=1` and `GIT_OPTIONAL_LOCKS=0`, printed `main`, `origin/main`, `0 0` and created no marker. In the partial clone of `.../scratchpad/g2` (`remote.origin.uploadpack` set to a marker script), `GIT_NO_LAZY_FETCH=1` stopped a lazy fetch that otherwise ran the script. (These were the calls before the HUD-2 review: call 1 with `--abbrev-ref HEAD`, call 2 without `--`.)

Evidence for the current calls (HUD-2, git 2.55.0, 2026-11-21): in a plain clone with an upstream, call 1 (`--symbolic-full-name HEAD`) printed the three paths and `refs/heads/main`, and call 2 (with `--`) printed `0	0`. Both exited 0. The no-marker check above was not run again for these calls: the HUD-2 session's worktree guard refuses every git call that configures a program (`-c core.fsmonitor=…`, repository config writes). That check stays open for the current calls until H§14 #25 (HUD-9) or the operator runs it.

Budgets: 400 ms and not 200 ms (author): process start of git on Windows is slow, and the budget runs in parallel with the status fetch, so it adds nothing to the 3 s bound.

### 7.6 Workspace phase (phase 2)

The workspaces build records membership in the session view: `SessionView.workspace: Option<ViewWorkspace>` with `member: Member { ws_id, pane }` (WS AU58; WS node C1). The HUD reads `view.workspace.member.ws_id` from `view.json`, checks it (WS AU84), then reads `paths.workspaces_dir()/<ws_id>/workspace.json` (WS§7.1, WS§7.2; `workspaces_dir` is a `Paths` method, WS C1) without a lock, as the workspace hooks do (WS§9.2; the file is replaced by rename, W-C4). It deserializes only `version` and `phase` with its own small serde struct (unknown fields ignored), so `zygo-hooks` gains no dependency on `zygo-workspace`. A missing marker, a bad id, a missing file, a parse error, a `version` other than 1, or a phase outside `building`, `draining`, `stopping`, `down` hides the segment (AU25). This part ships with or after the workspaces build (H§17).

### 7.7 Cache and refresh

Claude Code runs the command on its own events and every 30 s (`refreshInterval: 30`, `crates/zygo-launch/src/claude_code.rs:268`).

- **One cache file.** `statusline.json` changes from `{at, line}` (`statusline.rs:20-24`) to `{v: 2, at, outcome, retry_at, latency_ms, status: {at, …}, git: {at, cwd, …}}`. A file without `v: 2` counts as no cache. `outcome` is `ok`, `offline`, `suspended` or `ended` (AU20); `retry_at` is the earliest next status fetch (now + 30 s after `offline` or `suspended`, now + 300 s after `ended`); `latency_ms` is the last successful status fetch time.
- **Numbers only (H-C4).** `status` holds only what segments render: model display name, profile, used, printed window, bar denominator, `⇣` value or `off`, tokens in and out, spend form inputs (value, notional flag, total and priced counts), cache share, mode flags. `git` holds branch, ahead, behind, linked flag, worktree name.
- **Lease.** A run that finds a part due creates `statusline.lock` with `OpenOptions::create_new`, holding `{tried_at, retry_after}` (`retry_after` = 5 s, longer than the 3 s run). If the file exists, the run does not fetch; it renders from the cache. A lock older than its `retry_after` is left by a dead run: it is removed and taken again. The holder fetches the due parts, writes the whole cache file atomically (a unique temp file, then rename, as `statusline.rs:112-119`), removes `statusline-*.tmp` and `statusline.json.*.tmp` files older than 60 s, and removes the lock.
- **Due.** The status part is due when `now - status.at ≥ 30 s` (or it is missing or in the future) and `now ≥ retry_at`. The git part is due when `now - git.at ≥ 30 s`, or its `cwd` differs from the stdin cwd. A failed or timed-out git try (or a git worker panic) writes the git part with `at = now`, its `cwd` and no data, so it is not due again for 30 s and renders no git segment.
- **cwd change (M3).** Git data whose `cwd` differs from the stdin cwd is never rendered. A cwd change ignores the lease for git: when another run holds the lock, this run runs the git worker anyway, renders its result, and does not write the cache.
- **Fetch cap.** With a status part 90 s old or less, the status fetch waits `clamp(1.5 × latency_ms, 1 s, deadline)`; with none or an older one, it gets the full deadline. The 90 s `stale` rule uses `status.at`.
- The hook prints, then exits. A result that is not in by the print is lost; the next run fetches again when the part is due. There is no write after the print and no early stdout close.
- **Worker panics (C6).** The status line hook installs a silent process-wide panic hook at its start (`guarded` restores the previous hook after its call, `crates/zygo-core/src/hook.rs:77-80`, which is too late for a worker still running), and each worker body runs in `catch_unwind`. A worker panic counts as a failed try of its part and writes at most one stderr line, `zygo hook: statusline <part> failed`.
- The trimmed count is read on every call (AU4), never cached.
- Rendering runs on every call from the cached data plus the fresh inputs (stdin, view, overrides, environment, phase).

## 8. Style selection

| Layer | Where | Notes |
|---|---|---|
| Default | `compact` (AU1) | |
| User config | `[statusline] style = "compact" \| "verbose"` | User config only. The repository file may not set it (§6.1 layer 4 ignores the table with one stderr line). The team config does not carry it. |
| Launch | The launcher writes the merged value into `view.json` as `statusline_style`. | The hook loads no config (`statusline.rs:26`). |
| Session | `overrides.json` field `statusline` | Read on every hook call, as other hook-side settings (§17.1 last bullet). |

The verb:

- `zygo statusline compact|verbose [--save]` and `/zygo:statusline compact|verbose [--save]`. Scope: session, `--save` (`Scope::SessionSave`, as `results` at `crates/zygo-core/src/verbs.rs:24`).
- Replies follow the §17.1 N49 note: `statusline verbose for this session`, `…; saved to user config`, and outside a session without `--save`, exit 2.
- With no argument, the verb prints the current style and its source, and exits 0, in any session (OpenCode included) and outside one.
- In an OpenCode session the verb works the same as in a Claude Code session **(operator, 2026-11-21)**: the session form `zygo statusline <style>` stores the style in `overrides.json` and gives the same reply, `--save` also writes `[statusline] style` to user config, and the form with no argument prints the current style and its source. The verb does not read `ZYGO_AGENT` (`crates/zygo/src/cmd/statusline.rs:5`). No zygo surface shows the HUD in an OpenCode session (H§3); a saved style applies to a later launch.
- The verb writes `overrides.json` only through the shared session writer `set_session` (`crates/zygo/src/cmd/verbs.rs:171`, which loads, applies a `Setting` and saves at `:198-200`), with a new `Setting::Statusline`. So the workspaces mirror of WS AU105 (each code path that changes `overrides.json` also writes the member record) covers it with no extra code.
- `VERB_SPECS` (`crates/zygo-core/src/verbs.rs:20-32`) and `VERBS` (`crates/zygo/src/cmd/verbs.rs:35-49`) gain one entry each. The embedded plugin writes `commands/statusline.md` from `VERB_SPECS` with no extra code (`crates/zygo-plugins/src/embedded.rs:26-33`). The slash path is the shared one (`crates/zygo/src/cmd/mod.rs:358-371`).
- A workspace member keeps the session style across `down` and `up`, because `member.overrides` saves `overrides.json` (WS§7.2).

## 9. Base-spec changes

| Base section | Change |
|---|---|
| §17, `zygo hook statusline` paragraph | Replace the line format with "The status line is the HUD of `docs/specs/2026-11-21-statusline-hud-design.md`." Keep the N38 note (P-N28-1). The `trimmed` bytes text and the `◇` format end. |
| §17, command table | Add `zygo statusline [compact\|verbose] [--save]` → H§8. |
| §17.1, verb table | Add `statusline [compact\|verbose]` · "Status line style" · session, `--save`. |
| §10.2.4, last bullet | "The status line shows `compact 600k` / `compact off`" becomes "The status line shows the soft or override threshold as `⇣600k`, and `⇣off` for an override of `off`; a safety-only window shows no `⇣` (H§6.1 AU28)." |
| §10.2.2, N15 note, last sentence | `effective_window` is computed for the model of the last main-loop request, else the main role's model (AU29). |
| §7.2 session API, status | The reply gains `model`, `context_window`, `threshold`, `tokens_in`, `tokens_out`, `spend_usd`, `spend_notional`, `requests_total`, `requests_priced`, `passthrough`, `team_session` (H§7.2), first (HUD-5, HUD-6); WS X1 adds the two WS AU91 fields later. A team server sends `spend_usd` at built-in list prices with `spend_notional = true` (AU14). The §7.2 rule "prices … never leave the server" does not change: no configured price leaves the server. |
| §6.3 catalog | `price_usd_per_mtok` may hold `long_context = { above, input, output, cache_read, cache_write }` (H§7.3). |
| §6.3 catalog, cache writes | `price_usd_per_mtok` may hold `cache_write_5m` (AU33). |
| §14 `requests.estimated_usd` | List price × tokens with the long-context tier when the row has one; still 0 for passthrough. |
| §6.1 / config | New table `[statusline]` with `style`; user config only. |
| §10.7, `zygo status` | Trimmed text in tokens, and a `spend:` line (AU26). |
| §14 usage recording | The tee also reads `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` for the cost (AU33); the row keeps one cache-write total. |
| §17, `zygo doctor` | Prints one icon test line (AU2). |
| §20 | Add the checks of H§14. |
| `docs/live-checks.md:77` (W-SHELL part 2) | "shows `zygo ◇`" becomes "shows the stripe, the logo (`𝘇𝘆𝗴𝗼` or `zygo`) and `ctx`". |

## 10. Alternatives rejected

- **Token estimate by word count.** It needs the text or a word count per trim. The usage store has bytes only (`trims`: `stored_bytes`, `view_bytes`, §14), and a new column breaks the "no schema change" non-goal. Bytes ÷ 4 is already the proxy's estimate.
- **Claude Code's stdin `cost`.** Claude Code prices each request as the Anthropic model it thinks it uses. zygo routes roles to other providers, and in a zygo session Claude Code's model name is a role name (AU10). The number is wrong (OP5).
- **Claude Code's stdin context numbers for `used`.** The §10.2.2 rewrite raises the usage that Claude Code reads, so Claude Code's own context number jumps to the full window after a trigger.
- **Spend from the local usage store in the hook.** In serve mode the request rows reach the local store only at session end (§14); in passthrough the rows hold 0. The proxy has the correct sums live.
- **Serve mode: the server sends the session spend at the team's configured prices.** Still rejected: a user can divide it by the tokens and get the team's blended price, against §7.2. AU14 sends a spend at the public built-in list prices instead, which reveals nothing that the binary does not already hold.
- **Serve mode: per-model token sums priced on the client** (an earlier draft). It needed a second price path, a bucket protocol for the long-context tier, and version skew between client and server catalogs; AU14 gives the same `≈` value with one price function.
- **Serve mode: no spend at all.** The user then sees no cost in the mode that a team uses most.
- **User-customizable segments or a format string.** OP4. A fixed HUD has one golden test per style and icon set, and no config to validate.
- **starship or another external prompt tool.** It adds a runtime dependency (against C1, one binary), cannot read the proxy session, and its git module scans the tree with no zygo bound.
- **The git dirty count `●N`** (dropped by OP14). `git status` runs programs that the repository configures. Three critic passes found three vectors (clean, smudge and process filters from several attributes sources; the fsmonitor hook; lazy fetch through `remote.<n>.uploadpack` in a partial clone), each closed only by a git-version-dependent workaround. For a segment shown only in verbose, the cost exceeds the value.
- **Plain text only.** It does not meet OP6. The plain set stays as the fallback.
- **Powerline arrow separators with background colors.** They need a patched font even in the plain set, and background colors fail on one of light or dark themes.
- **Caching the rendered line (today).** Stdin values change between calls (model, effort, cwd), and the style can change mid-session, so the cache must hold data.
- **The team token label as the serve-mode label.** The host of `ZYGO_URL` tells the user the same thing and adds no new value to the status reply.

## 11. Files expected to change

| File | Change |
|---|---|
| `crates/zygo-hooks/src/statusline.rs` | Rewrite: data cache v2, bounded stdin, workers, render for two styles × two icon sets × color modes, width fit. The builder may split it into a `statusline/` module. |
| `crates/zygo/src/cmd/hook.rs` | Table entry `statusline` → `reads_stdin: false` (`:16`). |
| `crates/zygo-core/src/lib.rs` (or a new `tokens.rs`) | `BYTES_PER_TOKEN = 4`. |
| `crates/zygo-core/src/overrides.rs` | Field `statusline: Option<String>` (`:10-20`). |
| `crates/zygo-core/src/session_view.rs` | Field `statusline_style` with a serde default. |
| `crates/zygo-core/src/verbs.rs` | `VerbSpec` for `statusline`. |
| `crates/zygo/src/cmd/verbs.rs` | `VERBS` entry; `Setting::Statusline`. |
| `crates/zygo/src/cmd/statusline.rs` (new) | The verb. |
| `crates/zygo/src/cmd/mod.rs` | CLI subcommand `statusline`. |
| `crates/zygo/src/cmd/status.rs` | Trimmed in tokens (`:65-66`, tests `:117`, `:147`). |
| `crates/zygo/src/cmd/usage.rs` | Add `tokens trimmed (estimate)` (`:102`). |
| `crates/zygo/src/cmd/doctor.rs` | Icon test line. |
| `crates/zygo-proxy/src/session.rs` | `Live` sums: output, spend (built-in list prices on a team server), `requests_total`, `requests_priced`; stored last main-loop model, agent window, effective window and threshold (AU29); `status()` new fields. |
| `crates/zygo-proxy/src/window.rs` | `soft_threshold` public, for the status `threshold` (`:76-83`). |
| `crates/zygo-proxy/src/client.rs` | `SessionStatus` new fields with `#[serde(default)]`. |
| `crates/zygo-proxy/src/forward.rs` | `Pending::drop` uses the price function and feeds the session sums; `notional_prices`; `:350` and `:446` use `BYTES_PER_TOKEN`. |
| `crates/zygo-proxy/src/window.rs` (call site `:133`) and `forward.rs` | Pass the request's agent window to the main-loop store (AU29). |
| `crates/zygo-proxy/src/tee.rs` | Read `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` when present (AU33). |
| `crates/zygo-proxy/src/passthrough.rs` | Keep the matched row's price as `notional_prices` (`:226`). |
| `crates/zygo-proxy/src/routes.rs` | Status reply new fields. |
| `crates/zygo-config/src/schema.rs` | `Prices.long_context`, `Prices::cost`, `cache_write_5m`; `StatuslineConfig`; `Config.statusline`. |
| `crates/zygo-config/data/catalog.toml` | Haiku `long_context` data (`:110-115`); `cache_write_5m` on each Anthropic row: fable 10.0, opus 5.0, sonnet 2.5, haiku 0.125 (`long_context` 0.625) (1.25 × input, AU33). The price tables have no `source` field of their own; the row's `facts.source` and `verified_on` (`catalog.toml:13-14`) cover the row, so the builder checks the price page and updates `verified_on` (§19a). |
| `crates/zygo-config` `config check` | Validate `style`, prices and `long_context` (AU32). The repository-file filter needs no change: it keeps only `KEPT` tables and reports the rest (`crates/zygo-config/src/repo_file.rs:219-229`); a test covers `[statusline]`. |
| `crates/zygo-usage/src/query.rs` | `Store::bytes_trimmed(session)` (H§7.4). |
| `crates/zygo-launch/src/launcher.rs` | Write `statusline_style` into the view. |
| `crates/zygo-work/src/repo.rs` | Public bounded function for the HUD's two git calls. |
| `crates/zygo-plugins/embedded/skills/zygo-cli/SKILL.md` | Mention `zygo statusline`. |
| `Cargo.toml` (workspace), `crates/zygo-hooks/Cargo.toml` | `unicode-width`. |
| `docs/live-checks.md` | W-SHELL part 2 text; new check P-HUD-1. |
| `docs/specs/2026-11-18-zygo-design.md` | Dated notes per H§9. |
| `crates/zygo-hooks/tests/` | Golden files and tests of H§14. |

## 12. Premises to verify

Each premise is checked on the pinned Claude Code version before the work that depends on it. A failed premise applies its fallback and gets a dated note here.

| Id | Premise | Evidence so far | Fallback |
|---|---|---|---|
| PH1 | Claude Code renders SGR colors (16-color and `38;5;n`) in the status line. | The stripe ships (§17 2026-11-20 note). No capture with escapes: `p-n28-1-pane.txt` is a plain `capture-pane`. *(Note 2026-11-22, LIV-1, P-HUD-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux. capture-pane -e on tmux 3.7c: stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 as 38;5;n. `docs/live-checks.md` row P-HUD-1/PH1.)* *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. capture-pane -e on tmux 3.7c of the H§6.3 verbose printf bytes: stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 as 38;5;n. Claude Code rewrites `\e[1;97m` as `\e[1m\e[97m` and puts `\e[38;5;246m` after each reset, so HUD text with no colour of its own shows in grey 246: `fixtures/claude-code/hud/prb-1-verbose.txt` lines 1 and 2. `docs/live-checks.md` row PRB-1/PH1.)* | No 256-color: AU23 always picks the 16-color fallback. No color at all: the HUD prints no escapes and no stripe, as with `NO_COLOR` (no new variable, OP4). |
| PH2 | Claude Code shows a two-row status line output as two rows. | None. *(Note 2026-11-22, LIV-1, P-HUD-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux. verbose shows two rows: the logo row, then the reflection row, on tmux 3.7c at 200 columns. `docs/live-checks.md` row P-HUD-1/PH2.)* *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. the two rows of the H§6.3 verbose printf show as two rows on tmux 3.7c at 200 columns. `docs/live-checks.md` row PRB-1/PH2.)* | Verbose joins its rows into one row (stripe once, three spaces between the parts). |
| PH3 | How Claude Code shows a row wider than its area: cut, or wrapped. | None. *(Note 2026-11-22, LIV-1, P-HUD-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux. cut: at 70 columns the row is whole, 65 of 65 cells shown; at 19 columns, with the width source default 120, a row of 87 cells is cut. `docs/live-checks.md` row P-HUD-1/PH3.)* *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. cut: a row of 302 cells in a 200-column pane shows 196 cells, ruler 193, ends in …: true; the next line does not go on with the row. `docs/live-checks.md` row PRB-1/PH3.)* | Cut: no change. Wrapped: lower the default W to 100. |
| PH4 | The width sources reach the hook: (a) `/dev/tty` opens and `TIOCGWINSZ` gives the terminal (or tmux pane) width, so the hook has a controlling terminal; (b) `COLUMNS` is exported. Also: Claude Code's status area is the full terminal width. | None. `COLUMNS` is a shell variable that shells often do not export. *(Note 2026-11-22, LIV-1, P-HUD-1: failed on Claude Code 2.1.330, tmux 3.7c, Linux. a: width source in tmux at 200 columns: COLUMNS 200; on a pseudo-terminal of 100 columns: COLUMNS 100; b: COLUMNS in the hook: tmux 200, pseudo-terminal 100; c: status area = pane width - 4 at 19 columns, indent 2, right gap 2; row at 70 columns: whole. `docs/live-checks.md` row P-HUD-1/PH4. FIX-5 builds the fallback: H§6.7 fits each row to T − 4.)* *(Note 2026-11-22, PRB-1: failed on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. a: /dev/tty in the command: tmux 200 columns: ENXIO, pseudo-terminal 100 columns: ENXIO; b: COLUMNS: tmux 200, pseudo-terminal 100; c: status area = pane width - 4 at 200 columns, indent 2, right gap 2. The fallback of this row, each row fitted to T − 4 in H§6.7, is built by FIX-5, which is done; PRB-1 names no new node. `docs/live-checks.md` row PRB-1/PH4.)* | A source that fails is skipped; with none, W = 120, which is acceptable (the full compact row of the OP13 data is about 100 cells). If the status area is narrower than the terminal, subtract the difference found by P-HUD-1. |
| PH5 | Shape of stdin `effort` (string, or an object with a string level). | Top-level keys only (`q7-claude.json:191-204`). *(Note 2026-11-22, LIV-1, P-HUD-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux. stdin effort is {"level":"medium"}, an object; stdin keys: session_id transcript_path cwd scratchpad_dir prompt_id effort session_name model workspace version output_style cost context_window exceeds_200k_tokens prompt_cache fast_mode thinking rate_limits. `docs/live-checks.md` row P-HUD-1/PH5.)* *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. stdin effort is of type object, {"level":"medium"}, in every logged run of the tmux and pseudo-terminal sessions; stdin keys of the first run: session_id transcript_path cwd scratchpad_dir effort model workspace version output_style cost context_window exceeds_200k_tokens fast_mode thinking. `docs/live-checks.md` row PRB-1/PH5.)* | Another shape drops the effort part of the model segment. |
| PH6 | Claude Code counts a Nerd private-use glyph as one cell when it lays out the status line. | None. *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. Claude Code counts U+F0645 at cell width 1: the row with 10 glyphs shows 10 fewer ruler cells than the ASCII row, 193 against 183. `docs/live-checks.md` row PRB-1/PH6.)* | No change: H§6.7 already counts 2 cells, so a row is never wider than computed. |
| PH7 | `✂` U+2702 shows in text presentation (1 cell), not as an emoji. | None. *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. cell widths only: Claude Code counts U+2702 at cell width 1: the row with 10 glyphs shows 10 fewer ruler cells than the ASCII row, 193 against 183; tmux 3.7c cell width 1; U+FE0F after it in the compact row: false; text or emoji presentation in a terminal font: not checked. Only cell widths were measured; the text or emoji presentation in a terminal font is not checked. `docs/live-checks.md` row PRB-1/PH7.)* | The Nerd set uses U+F0190 (nf-md-content_cut). |
| PH8 | On Windows, `WT_SESSION` reaches the status line command through Git Bash. | Git Bash runs it (`docs/live-checks.md`, Results row W-SHELL). *(Note 2026-11-22, LIV-2 plan: not run: no Windows machine available to the executor. Fallback applied by default: on Windows the plain set unless `ZYGO_NERD_FONT=1`; AU2 changed, LIV-2 builds it.)* | Windows uses the plain set unless `ZYGO_NERD_FONT=1`. |
| PH9 | Claude Code and the common terminals measure each Mathematical Sans-Serif Bold letter (U+1D5F4-U+1D607, outside the BMP) as one cell, and the user's font or its fallback has the glyphs. Some terminals measure these letters wrongly or show boxes. | None. *(Note 2026-11-22, LIV-1, P-HUD-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux. logo and reflection start at column 6 of the pane on tmux 3.7c; the glyphs of the font are not checked. `docs/live-checks.md` row P-HUD-1/PH9.)* *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. cell widths only: Claude Code counts each logo letter at cell width 1: the row with 20 letters shows 20 fewer ruler cells than the ASCII row, 193 against 173; tmux 3.7c cell width 1; logo and reflection start at column 6 and 6; glyph shapes in a terminal font: not checked. Only cell widths and columns were measured; the glyph shapes of a terminal font are not checked. `docs/live-checks.md` row PRB-1/PH9.)* | The logo becomes plain `zygo` in SGR `1;97` in every icon set (AU8); the reflection stays. |
| PH10 | Windows Terminal, VS Code and other modern Windows terminals set `TERM_PROGRAM` (or `WT_SESSION`), and the value reaches the hook through Git Bash. | None. *(Note 2026-11-22, LIV-2 plan: not run: no Windows machine available to the executor. Fallback applied by default, the same rule as PH8: on Windows every terminal gets the plain set unless `ZYGO_NERD_FONT=1`.)* | Windows uses the plain set unless `ZYGO_NERD_FONT=1`, whatever `WT_SESSION` and `TERM_PROGRAM` hold (AU2). |
| PH11 | Claude Code tolerates a status line command that exits before it reads all of stdin (no error, no lost line). | None. Real payloads are a few KiB, so the detached discard normally reaches end of file first. *(Note 2026-11-22, PRB-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. a command that closes stdin unread and exits at once: each of 3 or more early runs that ended 2.5 s before the last capture showed its line, no run was lost to a stdout error or a missing end, no error text, and the status line went on after the phase; EPIPE likely not exercised: each logged stdin payload is smaller than a 64 KiB pipe buffer, so Claude Code's write likely ended before the command closed stdin. The early command closes fd 0 before it does anything else, but the largest stdin payload logged in this run was 1181 bytes, so Claude Code's write most likely ended in the pipe buffer first; the EPIPE path of Claude Code was likely not exercised. `docs/live-checks.md` row PRB-1/PH11.)* | The hook waits for the discard thread until end of file or the deadline before it exits. |
| PH12 | Observation only: how Claude Code treats a status run that is still going when the next refresh starts, and whether it kills slow runs. | None. *(Note 2026-11-22, LIV-1, P-HUD-1: verified on Claude Code 2.1.330, tmux 3.7c, Linux. observation, refreshInterval 2 s: the first slow run lived 2.16 s of its 3 s bound; the next run started 2.23 s after its start: Claude Code ends the run early; 22 runs in 45 s. `docs/live-checks.md` row P-HUD-1/PH12.)* *(Note 2026-11-22, PRB-1: observed on Claude Code 2.1.330, tmux 3.7c, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. refreshInterval 2 s, a command that sleeps 5 s: Claude Code ends a running command when the next refresh comes; signals seen: TERM; concurrent commands at most: 1; slow line shown: false. `docs/live-checks.md` row PRB-1/PH12.)* | The design does not depend on it: the hook prints within its deadline and exits (AU4); the lease stops runs from piling up fetches. P-HUD-1 records the behavior. |
| PH13 | OpenCode 1.18.16 has no command status line that zygo could set. | `docs/conformance.md:11` meets A-9 through the CLI. *(Note 2026-11-22, PRB-1: verified on OpenCode 1.19.6, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh`, not the zygo HUD. OpenCode 1.19.6: --help and debug --help exit 0 and name no status line; debug config --pure with each of 3 top-level keys drops it: statusLine dropped, statusline dropped, status_line dropped; control: username kept, zygoProbeUnknownKey dropped; nested keys not tested. Method: `opencode --help` and `opencode debug --help`, each with its exit status and output size checked, and `opencode debug config --pure` with `OPENCODE_CONFIG_CONTENT` holding one top-level key at a time, in 3 spellings, `statusLine`, `statusline` and `status_line`, each set to a command object, under a temporary HOME. Nested keys were not tested, for example `tui.statusLine`, `tui.statusline` and `experimental.statusLine`. `docs/live-checks.md` row PRB-1/PH13.)* | If one exists, a later design adds it; v1 keeps `zygo status`. |

**Probe first (author).** Before the build, a probe node installs a temporary `statusLine` command that prints the bytes of the two OP13 `printf` commands and logs its stdin and environment. It checks PH1 (colors), PH2 (two rows), PH4 (`/dev/tty`, `COLUMNS`, status area width), PH5 (`effort` shape), PH9 (logo width), PH11 (early stdin close) and PH12 (overlapping runs) on the pinned Claude Code version, and records each result as a dated note here. A fallback node is planned only for a premise that fails. The full live check P-HUD-1 runs at the end, on the built HUD.

Live check **P-HUD-1** (new, `docs/live-checks.md`): run `zygo claude` in tmux with each style; `capture-pane -e -p` shows the SGR codes (PH1) and two rows for verbose (PH2); a 70-column pane shows the cut or wrap (PH3); the hook logs which width source answered (`/dev/tty`, `COLUMNS`) inside and outside tmux, and the status area width (PH4); with a test seam that makes the status fetch take 5 s and `refreshInterval` 2, the log records whether Claude Code cancels, kills or waits for a run (PH12); the reflection on row 2 starts at the same column as the logo on row 1 (PH9); the hook writes its stdin and environment to a test file under the `test-hooks` feature (PH4, PH5); no recorded upstream request holds HUD text (C7, as P-N28-1).

*(Note 2026-11-22, LIV-1: P-HUD-1 ran with `scripts/premises/live-hud.sh` on Claude Code 2.1.330, tmux 3.7c, Linux. Parts PH1 to PH5, PH9 and PH12 ran through `zygo claude --passthrough` on the stored login. The C7 part ran on the mock upstream, as P-N28-1, because a passthrough request cannot be recorded: verified. HUD shown, verbose with --compact 777k; the logo, the reflection, 777k and 777000 are in none of 2 recorded requests, and 2 hold the tracer prompt; mock upstream. In a 70-column pane the HUD fits its row to the pane by H§6.7 step 1, so no row is wider than the pane there; the script also makes the pane 19 columns wide, below the 20-column limit of H§6.7, so the hook uses another width source and prints a row that is wider than the area, for PH3 and for the status area width of PH4. The "outside tmux" part of PH4 ran on a pseudo-terminal of 100 columns. PH9 compares columns in the tmux grid; the glyphs of a font are not checked. Rows: `docs/live-checks.md` P-HUD-1/PH1 to P-HUD-1/C7.)*

*(Note 2026-11-22, PRB-1: the probe of "Probe first" ran after the build, with `scripts/premises/live-hud-probe.sh` on Claude Code 2.1.330, tmux 3.7c, Linux, and OpenCode 1.19.6. It ran Claude Code directly with a `--settings` file that holds only the probe `statusLine` command at refreshInterval 2, with no prompt, in tmux at 200 columns and on a pseudo-terminal of 100 columns; the command printed the two OP13 `printf` outputs, then ruler rows that measure Claude Code's cell count of U+F0645, U+2702 and the logo letters, then an early exit and a slow run. Beside the ids of this paragraph it also checked PH3, PH6, PH7 and PH13, as the PRB-1 contract asks. PH7 and PH9 compare cell widths in Claude Code's cut and in tmux only; glyph shapes and the presentation of a terminal font are not checked. Round 2 of PRB-1, after a diff review, put each verdict gate under an offline test and ran the probe once more; the words did not change. Results: a dated PRB-1 note in each row; `docs/live-checks.md` rows PRB-1/PH1 to PRB-1/PH13.)*

## 13. Left to the builder

- Module layout of the hook; names of private types.
- How `zygo-work` exposes the git calls (one function or two).
- The exact text of the doctor icon line, and error texts beyond those quoted.
- The fixture values of the golden tests.
- The `long_context` source URL and `verified_on` date for the Haiku row.

## 14. Acceptance

Each item is a test or a command whose output decides it. "Fixture inputs" = one fixed status reply, stdin JSON, git result, trimmed count and environment, kept in the test directory.

**Rendering**

1. Golden tests: compact and verbose × Nerd and plain × color and `NO_COLOR` = 8 golden files; each render of the fixture inputs equals its file byte for byte.
2. Verbose with color: row 1 and row 2 both start with the same bytes `\x1b[31m▌\x1b[33m▌\x1b[36m▌\x1b[0m `. Compact starts with the same bytes. Then row 1 and the compact row hold `\x1b[1;97m𝘇𝘆𝗴𝗼\x1b[0m`, and verbose row 2 holds `\x1b[2;37mzʎƃo\x1b[0m`; logo and reflection start at the same column. Plain set: `\x1b[1;97mzygo\x1b[0m` on row 1 and four spaces on row 2. With `NO_COLOR=1`: no row holds `\x1b` or `▌`.
3. The fixture data of the OP13 mockups (work `branch:main`, linked worktree `hud-work`, ahead 1, `claude-opus-4.7` (a test catalog row), effort `high`, profile `fast`, local, used 196,000 of 1,000,000, tokens 1,200,000 in and 48,000 out, $3.41, cache 0.92, 92,160 trimmed bytes) renders to the same screen as the two `printf` commands of H§6.3 (verbose as amended by OP14): both outputs are parsed by a small VT parser in the test into a grid of (cell, character, style) where style = (intensity, foreground), and the grids are equal (S1, author: the comparison keeps the operator's visual intent of OP13 and lets the emitter change escapes that draw the same screen; listed for operator check; the emitter still uses H§6.4). The comparison drops the final `\n` of each `printf` (H§6.4 step 7: no newline after the last row).
4. Emission examples 1 and 3 of H§6.4 are golden tests: the threshold case and the stale case render to the same grid as their `printf` output (as item 3). A unit test of the emitter covers each transition kind of H§6.4 step 6 (no change; add a part; reset then set) and gap absorption on both sides.
5. Work item markers: `branch:main` → branch icon + `main`, SGR `1;34`, no `branch:` text; `github:o/n#123` → `#123`; `github:a#b#7` → `#7`; `github:o/n` → `github:o/n`; `work-graph:hud/N3` → `◆ hud/N3` (plain `♦`); `beads:zy-a1b2` → `zy-a1b2` with its icon; `x:y` → `x:y`; `nocolon` → `nocolon`. `ZYGO_WORK=""` with no status `work` and git head `feat` → `<U+E0A0> feat` with SGR `34` (plain set: `feat`); `ZYGO_WORK=""`, no status `work`, no git → no work segment. `ZYGO_WORK=branch:main` with git head `feat` → `feat` (live branch); with git head `(detached)` or no git result → `main`. After `zygo work ENG-9` in the session, the HUD still shows the launch-time item.
6. Worktree: cwd in the main worktree → no worktree segment; cwd in a linked worktree at `<tmp>/a/b/feat-hud` → `feat-hud` and no `/` in the segment; a bare repository with two worktrees → each shows its own name; a worktree directory named `x\x1b[31my` → printed as `x\u{1b}[31my`; a 40-character name → 23 characters and `…`. Color `38;5;172` with `COLORTERM=truecolor`, `33` with `TERM=xterm` and no `COLORTERM`.
7. Git marks (verbose): ahead 1 → `↑1` in SGR `36`; ahead 0 and behind 2 → `↓2`; no upstream → no marks; the worktree comes before the marks; compact shows no git marks; no `●` appears in any output (OP14).

**ctx and threshold**

8. Model window 1,000,000, used 196,000: no `threshold` → `ctx 196k/1m`, no `⇣`; `threshold` 600,000 → `⇣600k`; override `off` → `⇣off`; `threshold` 1,000,000 (not below the window) → no `⇣`. Safety only: model window 200,000, agent window 1,000,000, no soft window, no override (so `effective_window` 180,000 and `threshold` None), used 90,000 → no `⇣`, bar fraction 0.5. Model window unknown → `ctx 196k/-` and no bar, no percent, no `⇣`. `used_context` unknown → `ctx -/1m` and no bar. Window known → the line never holds `/-`. Soft 190,000 with safety 180,000 (model window 200,000) → `threshold` 180,000 → `⇣180k`. Window unknown with `compact = off` → `ctx 196k/- ⇣off`; window unknown with `threshold` 600,000 → no `⇣`. `[1m]`: role `design` on a 200k Claude model called as `design[1m]` (or with an `anthropic-beta` token `context-1m-…`), soft window none, used 150,000 → after that main-loop request the status has `context_window` 200,000 and `effective_window` 180,000 (the safety window for agent window 1,000,000), not none; the bar fraction is 150/180. AU29: after a main-loop request served by another role's model (window 400,000), the status `model` and `context_window` are that model's. Passthrough before the first request → no `model` in the reply. S3: after `zygo compact 150k` with no new request, the next status has `threshold` 150,000 (computed in `status()` from the stored model and agent window). A non-streamed main-loop request stores model and agent window like a streamed one; an OpenCode request on `/anthropic/v1/messages` stores its model with `agent_window(row)`; a request on another route leaves the stored values unchanged.
9. Bar: fractions 0.59, 0.60, 0.84, 0.85 → SGR `32`, `33`, `33`, `31`. Compact 20 % → 1 of 5 cells; verbose 20 % → 2 of 8 and `20%`; used 1 token → 1 cell; used > threshold → all cells. With a threshold of 600k and used 300k, the fraction is 0.5 (AU17). Bar fill matches OP13: 20 % → `▰▱▱▱▱` (compact) and `▰▰▱▱▱▱▱▱ 20%` (verbose).

**Spend and tokens**

10. Spend format: 3.414 → `$3.41`; 99.994 → `$99.99`; 99.996 → `$100`; 0.004 → `<$0.01`; 0.005 → `$0.01`; 0 with `requests_total` 1 and `requests_priced` 1 → `$0.00`.
11. `Prices::cost`: Haiku, prompt 100,000 tokens → base rates; prompt 100,001 → all four rates from `long_context`; known values in, known USD out (to 1e-9).
12. AU32: `config check` rejects `input = -1`, `output = nan`, `cache_read = inf` and `above = 0`; a cost that overflows to infinity counts as not priced; spend 123456.0 → `>$99999`. AU33: a usage with 1,000 5-minute and 1,000 1-hour cache-write tokens on a table with `cache_write_5m` prices them at the two rates; without the split all 2,000 use `cache_write`.
13. Local mode, two routed requests on two priced providers: status `spend_usd` = the sum of the two rows' `estimated_usd`; `requests_total = 2`, `requests_priced = 2`; `tokens_in` = Σ(input + cache read + cache write); `tokens_out` = Σ output. HUD `$X.XX` in SGR `32`.
14. Passthrough: rows have `estimated_usd = 0`; status `spend_notional = true` and `spend_usd` = the notional cost; HUD `≈$1.84` with `≈` in SGR `2`. With `spend_notional = true`: one unpriced request of two → `≈$1.84+`; spend 0.004 → `≈<$0.01`; no priced request → `≈$?`. With `spend_notional` absent → no `≈` (`$1.84`).
15. Team session (test server whose configured prices differ from the built-in catalog): status `team_session = true`, `spend_notional = true`, and `spend_usd` = Σ `Prices::cost` with the built-in prices (a request with a prompt above `long_context.above` priced at long rates); no field holds a configured price or cost; the server's row `estimated_usd` uses its configured prices. The HUD shows `≈$X` with `≈` in SGR `2`. A served model missing from the built-in catalog makes `requests_priced < requests_total` → `≈$X+`. The same session through a loopback tunnel (`ZYGO_URL=https://127.0.0.1:…`) shows mode `serve 127.0.0.1`. A local proxy reply has `team_session = false`.
16. Local mode, a provider with no price: `requests_total = 2`, `requests_priced = 1` → `$X.XX+`; `requests_total = 1`, `requests_priced = 0` → `$?`; `requests_total = 0` → `$0.00`. A request with zero tokens (a refusal) counts in neither.
17. An old status reply with none of the new fields parses (every new field `None`); the HUD has no model, mode, tokens, spend or `⇣` segment (`spend_usd`, `passthrough` and `team_session` are absent), keeps ctx as `ctx <used>/-`, and logs no error.

**Trimmed**

18. 92,160 trimmed bytes → compact `✂ 23k`, plain `trim 23k`, verbose `trimmed 23k tok`. `zygo status` prints `trimmed 23k tok`. `zygo usage` prints `bytes trimmed: 92160` and `tokens trimmed (estimate): 23040`. `zygo status` in a session prints `spend: $3.41`. A trimmed row inserted between two calls 1 s apart shows at the second call (no trimmed cache). `Store::bytes_trimmed` gives the same value as `summary` for the session; `EXPLAIN QUERY PLAN` names `trims_session`.
19. `rg 'div_ceil\(4\)' crates/zygo-proxy/src` finds nothing; `forward.rs` uses `BYTES_PER_TOKEN`.

**Width**

20. Fixture `width-all`: one status fixture with status data 100 s old (so `stale` shows), a `threshold` of 600k below a 1m window, a linked worktree, and every droppable segment present (work item, worktree, model, profile, ctx with bar, `⇣`, tokens, spend, cache, trimmed); its full compact row is 115 cells. The fit width W is `COLUMNS` − 4 (H§6.7). `COLUMNS=200` → all segments; `COLUMNS=119` → all segments; `COLUMNS=118` → no trimmed; then each smaller `COLUMNS` removes segments in the H§6.7 order, one test step per segment (the row before a step is whole at `COLUMNS` = its cells + 4 and loses the segment at one less); `COLUMNS=20` → exactly `▌▌▌ 𝘇𝘆𝗴𝗼 <branch icon> main` (stripe, logo, work item; 16 cells). With phase `draining` added, the phase stays at every `COLUMNS`. The stripe and the logo are present at every `COLUMNS`. Width sources: with a test seam for each, `/dev/tty` 90 wins over `COLUMNS` 110; without `/dev/tty`, `COLUMNS`; none → 120; any source giving 5 or 5000 is skipped; `COLUMNS=abc` → skipped. Fit width: `/dev/tty` 90 → 86; `COLUMNS` 110 → 106; `COLUMNS` 20 → 16; no source → 120.
21. Verbose rows fit separately with their own orders (H§6.7); `stale` and the row 2 ctx numbers drop in their listed places; the reflection keeps its column at every `COLUMNS`.

**Input, deadline, failure**

22. Stdin cases: empty; `[]`; `"x"`; invalid JSON; 10 MiB of JSON; a 1 MiB string in `model.id`; `effort` holding `\x1b]0;x\x07` and U+202E; a valid object followed by 10 MiB of garbage; a writer that sends half an object and then stalls. Each run prints a HUD, parses at most 64 KiB, finishes within 3 s, and prints every stdin string it uses escaped; the stalled writer is parsed (as empty) at 200 ms. The stdin `model.id` never appears in the output.
23. Deadline: with the `test-hooks` seam blocking the proxy fetch (`statusline.rs:65-76`) and a second seam blocking the git worker, the hook ends within 3 s; the git seam past 400 ms gives no git segments while the proxy segments show.
24. Backoff and lease: a status endpoint that accepts and never answers; five calls 1 s apart with no cache: only the first waits (to the deadline); it writes `outcome = offline` and `retry_at`; the next four make no request and print at once. With a status part 60 s old and `latency_ms` 200, a fetch waits at most 1 s; with `latency_ms` 2,000, at most 3 s (clamped to the deadline); with a part 100 s old, the full deadline. Two runs started together with an expired cache make one status request between them (the second finds `statusline.lock`). A `statusline.lock` 10 s old is removed and taken. After the print the process exits at once. A leftover `statusline.json.123.tmp` 2 minutes old is gone after the next write. A forced panic in the git worker: the HUD prints without git segments, stderr has one line `zygo hook: statusline git failed`, exit 0.
25. Git safety: a repository with `core.fsmonitor`, `core.hooksPath` (`post-index-change`, `reference-transaction`, `post-checkout`), `filter.m.clean` named in `.gitattributes`, `.git/info/attributes` and a global `core.attributesFile`, all writing marker files, every file stat-dirty, and a partial clone whose `remote.origin.uploadpack` writes a marker: after a render no marker exists, and the worktree, branch and ahead/behind show. A fake `git rev-list` that sleeps 1 s → worktree and branch still show (from call 1); the next call within 30 s does not run git (the git part is not due). A cwd change while another run holds the lock: the HUD shows the new cwd's branch, never the old one; the cache keeps the old git part. A git worker that panics → no git segments, one stderr line.
26. Git output: a worktree directory whose name holds invalid UTF-8 → U+FFFD in the name; a directory name that holds a newline → no worktree segment; a renamed file counts once.
27. Cache: two calls 10 s apart with different stdin `effort` → one fetch, two different outputs. A call 31 s later → a second fetch. An old `{at, line}` cache file → a fetch.
28. Proxy unreachable, no cache → work item, worktree, git, trimmed (from `SessionView.session`) and `offline` in SGR `31`; no mode segment. Cached status data 100 s old and the fetch failing → model, ctx, tokens, spend and cache in SGR `2` and `stale`, while work item, worktree, git marks and trimmed keep their colors. Suspended serve session → `suspended`. Status reply `401 zygo_session_unknown` (or `zygo_token_invalid`) with a cache present → the cache has `outcome = ended`; that call and five more calls within 5 minutes each print the local segments and `ended` in SGR `31`, and no model, mode, ctx, tokens, spend or cache segment; none of the five makes a request.
29. Mode: `passthrough = true` → `passthrough`; `team_session = true` → `serve <host of ZYGO_URL>` (also for a loopback host); both `false` → `local`; either field absent → no mode segment.
30. C6: a forced panic in render → empty stdout, exit 0, one stderr line.

**Icons and style**

31. `ZYGO_NERD_FONT=0` → plain set. `ZYGO_NERD_FONT=1` with `TERM=linux` → Nerd set; Windows build with no `WT_SESSION` and `ZYGO_NERD_FONT=1` → Nerd set. Unset: `TERM=linux` → plain; `TERM=dumb` → plain; Windows build with no `WT_SESSION` and no `TERM_PROGRAM` → plain; Windows build with `WT_SESSION` or `TERM_PROGRAM` set → plain (PH8, PH10 fallback). `ZYGO_NERD_FONT=yes` → automatic rules. `NO_COLOR=1` alone → Nerd icons, no escapes.
32. `zygo doctor` prints one line that holds the Nerd glyphs of H§6.5 and names the active set.
33. `/zygo:statusline verbose` in a session → reply `statusline verbose for this session`; `overrides.json` has `"statusline":"verbose"`; the next render has two rows. `--save` also writes `[statusline] style = "verbose"` to user config. `zygo statusline wide` → exit 2. Outside a session without `--save` → exit 2 with the §17.1 text. `zygo help` lists `statusline`. The embedded plugin has `commands/statusline.md`. An `overrides.json` with `"statusline":"wide"` → ignored; the style comes from `view.json`. `zygo statusline verbose --save` outside a session changes user config; a running compact session stays compact. With `ZYGO_AGENT=opencode` in a session, `zygo statusline verbose` gives the same reply `statusline verbose for this session`, the same `overrides.json` change and exit 0; `zygo statusline verbose --save` also writes user config and exits 0.
34. A `.zygo.toml` with `[statusline] style = "verbose"` → ignored with one stderr line by the existing filter (`repo_file.rs:219-229`); the session renders compact.
35. User config `style = "verbose"` → the launcher writes `statusline_style = "verbose"` into `view.json`; the HUD renders verbose.

**Workspace (phase 2, with the workspaces build)**

36. A member session whose `workspace.json` has `phase = "draining"` → `ws draining` right after the logo (compact: `▌▌▌ <logo> ws draining <branch icon> main`; verbose: `<logo>  ws draining   <branch icon> main`) in SGR `31`, in both styles, at every `COLUMNS`. `phase = "up"` → no phase segment. A non-member session, a missing file or bad JSON → no phase segment and no error. The file is read from `paths.workspaces_dir()`; a marker `ws_id` of `../x` or `ABC` → no phase segment and no file read outside that directory; `version = 2` → no phase segment. `phase = "up"` or `"other"` → no phase segment; `"building"`, `"stopping"`, `"down"` → shown in SGR `33`, `1;31`, `2`.

**Live**

37. P-HUD-1 passes on the pinned Claude Code version, or its fallbacks are applied and noted.
38. W-SHELL part 2 (Windows) passes with the new text. Not run by the executor (2026-11-22: no Windows machine available to it): `docs/live-checks.md:77` gets the new text, part 1 keeps its 2026-11-20 pass (the shell that runs `statusLine` did not change), and the W-SHELL row records part 2 as "not run: no Windows machine"; the stop report and the PR say so. W-SHELL has no fallback cell: a later Windows run that fails part 2 re-opens HUD-8 (rows, logo) or HUD-9 (hook entry) as a new node.

## 15. Brief corrections

Places where the repository contradicts the brief:

1. **Output tokens and per-request cost exist today.** The tee reads `output` (`tee.rs:25-30`); `Pending::drop` writes it and computes `estimated_usd` per request (`Pending::drop` in `forward.rs`). Only the session sum lacks output and cost (`PromptUsage`, `SessionState::add_usage` in `session.rs`).
2. **Serve mode cannot send spend as briefed.** The brief's AU5 says the team server computes it the same way. Prices never leave the server (§7.2, §14 N16 note, `routes.rs:160-162`, `:187`). Resolved by AU14 (lead decision): the team server prices with the public built-in list prices and marks the value notional.
3. **No price tier exists in the schema.** The >100k text is a comment on the Haiku row (`catalog.toml:111`, threshold 100,000), and `Prices` has four flat fields (`schema.rs:216-223`). H§7.3 adds `long_context`.
4. **Passthrough rows cost 0 by design** (`passthrough.rs:184`, `:226`; §14). Notional spend needs the matched row's price kept apart from the row.
5. **Stdin `model` is a role name** in a zygo session (`claude_code.rs:141`, `:291`), so a model segment from stdin would show `lead`. AU10 takes the model from the proxy.
6. **Only two bytes ÷ 4 sites.** Two lines in `run` (`forward.rs`: the local `count_tokens` estimate and `let estimate`) divide; `string_bytes` is the helper.
7. **Bad stdin already breaks the line.** `run` ignores its input (`statusline.rs:29`), but the dispatcher reads stdin with no bound (`hook.rs:49`) and `HookInput::parse` fails on a non-object (`crates/zygo-core/src/hook.rs:21-28`), so the line is empty.
8. **The cache holds the rendered line** (`statusline.rs:20-24`, `:86-119`). Fresh stdin per call needs a data cache (AU4).
9. **Mode is verbose only**, so it cannot be in the compact drop order of the brief's AU3. H§6.7 puts it first in verbose row 1.
10. **The live fixture has no ANSI.** `p-n28-1-pane.txt:17` is a plain capture and older than the stripe; color and two-row output are unverified (PH1, PH2).
11. **`find_checkout` is `pub(crate)`** (`repo.rs:119`). The HUD needs a public function in `zygo-work`.
12. **`effective_window` is also `None` when the model has no `context_window`** (`effective_for` in `window.rs`), not only with no soft window, an override or a safety window.
13. **The first compact mockup's bar** (2 of 5 cells at 20 %) matched no rounding rule; the OP13 mockups (1 of 5, 2 of 8) match AU18.
14. **`NO_COLOR` is about color only.** The brief's AU2 made it select plain icons; AU2 here does not.
15. **W-SHELL expects `zygo ◇`** (`docs/live-checks.md:77`); the new rows have no `◇`, so the check text changes.
16. **`zygo usage` already shows an estimated cost** (`usage.rs:101`). It gains the tier rule through the shared price function.
17. **Stdin key evidence.** The key list is at `q7-claude.json:191-204`, as the brief says; it lists top-level keys only.
18. **Citation style.** The workspaces design calls its own sections `W§n`; this document writes `WS§n` as the brief asks.
19. **The status call refreshes the session's idle time** (`routes.rs:233`, `accept`). The HUD keeps this behavior; it is not new.

## 16. Not resolved

Nothing. The former S4 (dirty count) is decided by OP14. The former Q1 (serve-mode spend) is resolved by AU14; the operator checks that author's call.


## 17. Plan notes

Author's calls for the planner. WS node ids and HUD node ids refer to `docs/work/phase2/graph.md`.

1. **Branch and order.** The HUD and the workspaces build are one work graph on `feat/workspaces` (`docs/work/phase2/graph.md`), and the HUD nodes come first. The one HUD change to `SessionStatus` is made by HUD-5 and HUD-6 on today's code: HUD-5 adds `#[derive(Default)]` and ends each test literal it breaks in `..Default::default()`; WS X1 then adds its two fields (`requests_in_flight`, `last_request_end`) after the HUD's, changing only `status()` in `session.rs`. HUD-7 adds `statusline_style` with `impl Default` for `SessionView` and ends each view literal in `..Default::default()`; WS C1 then adds only its `workspace` field. The HUD writes its own dated §7.2 note in the base spec (DOC-1); WS D1 writes the W§15 rows after it.
2. **Operator checks.** Plan as written. AU14 (team-server spend at built-in list prices) and S1 (grid comparison in acceptance 3 and 4) are listed for the operator to check at approval. Team-server spend is its own node.
3. **Late nodes.** Two nodes come last: the workspace phase segment (H§7.6; depends on WS C1 and WS W2) and the `zygo up` check of the auto-update design (`docs/specs/2026-11-21-auto-update-design.md`; depends on WS W7). The phase segment reads `view.workspace.member.ws_id` and parses only `version` and `phase` (AU25).
4. **Probe node first** (H§12): it checks PH1, PH2, PH4, PH5, PH9, PH11 and PH12 before the build; P-HUD-1 runs at the end; fallback nodes only for a premise that fails.
5. **AU29 tests.** The agent window is computed at request time and kept in `Pending` (AU29). Checked: the existing assertions in `client_compact_and_status` (`crates/zygo-proxy/tests/session_api.rs`; no request; main role), `status_used_and_effective` (`crates/zygo-proxy/tests/window.rs`; main-loop requests served by the main role's model, no `[1m]`) and `slash_off_disables_and_on_clears` (`crates/zygo/tests/compact_cmd.rs`; no request) keep their expected values, because before the first main-loop request, and when the main role's model serves with no `[1m]`, AU29 gives today's numbers. Those tests gain assertions for `model` and `context_window`; new tests cover `[1m]`, another role's model, a non-streamed request, and an OpenCode request (acceptance 8).
6. **Config node.** One small config-schema node adds `[statusline]` (this design) and `[update]` (auto-update design) together: user config only; both tables are dropped from the team layer, as WS W5 does for `[workspace]`; the repository-file filter already drops them (`crates/zygo-config/src/repo_file.rs:219-229`, `KEPT`). Shared files that the plan must order: `crates/zygo/src/cmd/mod.rs` (CLI subcommands of this design, the auto-update design and the workspaces graph) and base spec §17 (command table and status line paragraph).

## Change log

| Date | Section | Decided by | Reason |
|---|---|---|---|
| 2026-11-21 | H§17 intro and note 1, H§7.2 (row `requests_in_flight`) | author's call (lead decision: one merged graph, HUD first) | The HUD and workspaces graphs are merged with the HUD nodes first, so the HUD makes its `SessionStatus` and `SessionView` changes before WS X1 and C1. |
| 2026-11-21 | H§9 (row §7.2 session API, status) | author's call | The merged graph lands the HUD fields first and WS X1 adds its two fields later, so the row no longer says "in the same change". |
| 2026-11-21 | H§7.5 (calls 1 and 2), AU11 (detached-head note) | author's call (HUD-2 review) | rev-list gets "--" (a file named like the range made it ambiguous); branch read with --symbolic-full-name (a tag named like the branch made --abbrev-ref print heads/<name>). |
| 2026-11-21 | AU11 (no-branch cases), H§7.5 (call 1 SIGCHLD exception; evidence for the current calls) | author's call (HUD-2 review, round 3) | AU11 lists every no-branch case of call 1 (detached, refs/remotes/…, refs/tags/…, empty, any line outside refs/heads/), each with the launch-time-id fallback; with SIGCHLD ignored the exit status is lost, so an unborn HEAD shows the worktree with no branch (kept, not told apart from detached). |
| 2026-11-21 | H§7.2 model row | author's call (HUD-5 review) | matches AU29: stored under the used_context condition |
| 2026-11-21 | AU29, H§7.2 rows context_window and threshold | author's call (HUD-5 review) | status() uses the row values stored at request time from the serving config, so a serve-mode reload does not change them; before the first main-loop request, also after a reload, the main role's row from the session snapshot |
| 2026-11-21 | H§7.2 model row | author's call (HUD-5 review) | the stored model name is at most 128 bytes, cut on a character boundary, so a passthrough client cannot put a long string into every status reply; an empty name stores no model |
| 2026-11-21 | H§1, H§5, AU13, AU20, AU28, H§6.6, H§7.2, H§7.3, H§15 items 1, 6, 12, H§17 note 5 | author's call (HUD-5 review, round 2) | line citations of `session.rs`, `forward.rs`, `window.rs`, `client.rs` and the AU29 test files that HUD-5 moved now name the function, type or test |
| 2026-11-22 | H§12 (rows PH1, PH2, PH3, PH4, PH5, PH9, PH12; note below the P-HUD-1 paragraph) | LIV-1 (live check) | P-HUD-1 ran on Claude Code 2.1.330, tmux 3.7c, Linux; each result is a dated note. |
| 2026-11-22 | H§6.7, H§12 row PH4 (note), H§14 #20 | H§12 PH4 fallback (FIX-5); author's call for: the source order is kept, no source keeps W = 120, the log keeps T | P-HUD-1 on Claude Code 2.1.330: /dev/tty does not answer the hook, COLUMNS is the terminal width, and the status area starts 2 cells in and ends 2 cells before the right edge; the HUD fits its rows to T − 4. |
| 2026-11-22 | AU2, H§12 rows PH8 and PH10 (notes), H§14 #31, #38 | author's call, PH8/PH10/W-SHELL: no Windows machine available to the executor; fallback applied by default | PH8 and PH10 are not run; their fallback applies: on Windows the plain icon set unless `ZYGO_NERD_FONT=1` (node LIV-2 builds it). W-SHELL part 2 is not run; part 1 keeps its 2026-11-20 pass; reported as "not run: no Windows machine". |
| 2026-11-22 | H§12 (rows PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13; note below the LIV-1 note) | PRB-1 (live probe) | The probe ran on Claude Code 2.1.330, tmux 3.7c, and OpenCode 1.19.6; each result is a dated note. |
| 2026-11-22 | H§12 (PRB-1 notes of rows PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13; the PRB-1 paragraph) | PRB-1 round 2 (diff review) | The notes hold no bracket; they come from the round 2 run; PH1 records the SGR rewrite, PH4 names FIX-5, PH11 the EPIPE reach, PH13 its method. |
| 2026-11-22 | H-C4, AU30, H§12 row PH8 | LIV-2 (citation fix) | The citations of `docs/live-checks.md` named lines 93 and 104, which moved when rows were added; they now name the Results rows P-N28-1 and W-SHELL. |
| 2026-11-22 | H§12 row PH10 (fallback cell); H§12 LIV-1 notes of rows PH1, PH2, PH3, PH4, PH5, PH9 and the LIV-1 note below the P-HUD-1 paragraph | LIV-2 round 2 (diff review) | The PH10 fallback cell states the rule that LIV-2 built: on Windows the plain set unless `ZYGO_NERD_FONT=1`. The LIV-1 notes hold no parenthesis inside the note body; the words keep their meaning. |
| 2026-11-22 | H§3 (OpenCode bullet), H§8, H§14 #33 | operator, 2026-11-21 ("same as Claude Code"); built by HUD-11 | The `statusline` verb works the same in an OpenCode session as in a Claude Code session and does not read `ZYGO_AGENT`; the text that the session form exits 2 there is replaced. zygo still sets no OpenCode status surface. |
