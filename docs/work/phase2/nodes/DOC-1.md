# DOC-1 — Base-spec notes for the HUD (H§9) and auto-update

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§9 (every row except `docs/live-checks.md`; the status-reply change is recorded in §7.2), H§17 note 1; `docs/specs/2026-11-21-auto-update-design.md` "Base-spec changes". The W§15 rows, its §7.2 row included, stay with workspaces D1, which runs later. Records only the `zygo claude`/`zygo opencode` start check in the `zygo update` §17 row; the `zygo up` check text is D1's (after M5)
**Files:** `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `for t in statusline-hud-design spend_notional long_context cache_write_5m '\[statusline\]' 'icon test' 'update prompt' ZYGO_NO_UPDATE_CHECK 'zygo statusline' '⇣off' 'H§6.1 AU28' 'last main-loop request' 'long-context tier' 'start check'; do grep -q "$t" docs/specs/2026-11-18-zygo-design.md || echo "missing $t"; done` → prints nothing, and each H§9 row and each auto-update "Base-spec changes" row has its dated edit in the section it names

## Plan

All edits are in `docs/specs/2026-11-18-zygo-design.md` (the base spec). Each step rewrites its section in place as final text (no appended amendment note) and marks the new text with `*(2026-11-22, HUD design)*` or `*(2026-11-22, auto-update design)*`, as the base spec marks amended text with *(A1)*. Step 18 adds the one `## Change log` row that names every section. One commit per step.

Sources checked for the texts below: H§6.1 (AU2, AU14, AU26, AU28, AU29, AU33), H§6.7 (fit width after FIX-5: W = T − 4), H§7.2, H§7.3, H§8, H§14; `crates/zygo-config/src/schema.rs:246-256` (`LongContextPrices` holds an optional `cache_write_5m`, the graph Log 2026-11-21 finding); `crates/zygo/src/cmd/update.rs:286-368` (`start_check`: skip rules, prompt, sessions line `{others} zygo sessions are running; …`, install failure `zygo update: <msg>`); `crates/zygo/src/cmd/statusline.rs:5` (the verb is the same in a Claude Code and an OpenCode session: the operator decision of the graph Log 2026-11-21, built by HUD-11). The `docs/live-checks.md:77` row of H§9 is not in this node (Spec). No H§9 row cites the fit width; step 14 names it with the current H§6.7 text.

### H§9 rows

- [x] 1. H§9 row "§17, `zygo hook statusline` paragraph" → base §17, the paragraph that starts `` `zygo hook statusline` output: ``. Replace the whole paragraph with this text (the N28 note and the 2026-11-20 note end; the N38 note stays word for word):

  ```text
  `zygo hook statusline`: the status line is the HUD of `docs/specs/2026-11-21-statusline-hud-design.md`. It reads `GET /v1/zygo/sessions/{id}/status` (cached 30 s in the session directory) and the session's trimmed bytes from the local usage store, opened read-only; the whole run has a 3 s deadline. *(2026-11-22, HUD design)* *(Note 2026-11-19, N38, P-N28-1: on Claude Code 2.1.328 the status line is shown (`zygo ◇ … ◇ compact 777k`, `fixtures/claude-code/live/p-n28-1-pane.txt`) and its text is in no request sent upstream (C7 holds).)*
  ```

- [x] 2. H§9 row "§17, command table" → base §17 command table. Insert this row after the `zygo usage` row:

  ```text
  | `zygo statusline [compact\|verbose] [--save]` | Status line style (HUD design H§8): a session change; `--save` also writes `[statusline] style` to user config; with no argument, prints the style and its source. *(2026-11-22, HUD design)* |
  ```

- [x] 3. H§9 row "§17.1, verb table" → base §17.1 verb table. Insert this row after the `units on\|off` row:

  ```text
  | `statusline [compact\|verbose]` | Status line style (§17, HUD design H§8) *(2026-11-22, HUD design)* | session, `--save` |
  ```

- [x] 4. H§9 row "§10.2.4, last bullet" → base §10.2.4. Replace the bullet `- The status line shows `compact 600k` / `compact off` while an override is set.` with:

  ```text
  - The status line shows the soft or override threshold as `⇣600k`, and `⇣off` for an override of `off`; a safety-only window shows no `⇣` (HUD design H§6.1 AU28). *(2026-11-22, HUD design)*
  ```

- [x] 5. H§9 row "§10.2.2, N15 note, last sentence" → base §10.2.2, the note `*(Note 2026-11-19, N15 planning: …)*`. Replace its last sentence `The status `effective_window` is §10.2.1 `effective` for the main role's model, safety window included.` with:

  ```text
  The status `effective_window` is §10.2.1 `effective`, safety window included, computed for the model of the last main-loop request, else the main role's model (HUD design AU29, 2026-11-22).
  ```

- [x] 6. H§9 row "§7.2 session API, status" (and H§17 note 1: the HUD's own dated §7.2 text) → base §7.2. Insert this bullet before the bullet `- **Credentials per route.**`:

  ```text
  - **Status.** `GET /v1/zygo/sessions/{id}/status` (session token) replies with the session's `session`, `profile`, `work`, `used_context`, `effective_window` (§10.2.2), `compact`, `cache_read_share`, `expires_at` and `usage_note`, and with the HUD fields of `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2: `model`, `context_window`, `threshold`, `tokens_in`, `tokens_out`, `spend_usd`, `spend_notional`, `requests_total`, `requests_priced`, `passthrough`, `team_session`. Each HUD field is optional, so a reply from an older server parses. A local proxy prices `spend_usd` with the catalog prices of the route that served each request (notional prices in passthrough, with `spend_notional = true`); a team server prices it with the built-in list prices compiled into its binary and sends `spend_notional = true` (HUD design AU14). The "Team config" rule above holds: no configured price leaves the server. *(2026-11-22, HUD design)*
  ```

- [x] 7. H§9 row "§6.3 catalog" (`long_context`) → base §6.3. In the TOML example, replace the comment `# for the local cost estimate only (§14)` with `# for the local cost estimate (§14) and the session spend (§7.2)`. Insert this bullet after the bullet that starts `- The built-in catalog ships rows`:

  ```text
  - `price_usd_per_mtok` may hold `long_context = { above, input, output, cache_read, cache_write, cache_write_5m }` (`cache_write_5m` optional): when a request's `prompt` (§10.2.1) is above `above` tokens, all its rates come from `long_context`, else the base rates (HUD design H§7.3). `zygo config check` refuses `above = 0` and a rate that is negative or not finite. *(2026-11-22, HUD design)*
  ```

- [x] 8. H§9 row "§6.3 catalog, cache writes" → base §6.3. Insert this bullet after the bullet of step 7:

  ```text
  - `price_usd_per_mtok` may hold `cache_write_5m`, the rate of 5-minute cache writes; `cache_write` is the 1-hour rate and also prices cache writes whose usage has no 5-minute/1-hour split. Each Anthropic row of the built-in catalog sets `cache_write_5m` = 1.25 × its `input` rate (HUD design AU33). *(2026-11-22, HUD design)*
  ```

- [x] 9. H§9 row "§14 `requests.estimated_usd`" → base §14, the `requests` bullet. Replace `estimated_usd (list price × tokens; 0 for passthrough)` with:

  ```text
  estimated_usd (list price × tokens, with the long-context tier of §6.3 when the price row has one; 0 for passthrough) *(2026-11-22, HUD design)*
  ```

- [x] 10. H§9 row "§6.1 / config" → base §6.1. Insert this paragraph after the paragraph that starts `In serve mode the server's catalog and provider set are authoritative`:

  ```text
  `[statusline] style = "compact" | "verbose"` (default `compact`) selects the status line style (§17; HUD design H§8). The table is read from user config only: the repository file ignores it (layer 4), the team config does not carry it, and `zygo statusline` changes it for a session (layer 7). *(2026-11-22, HUD design)*
  ```

- [x] 11. H§9 row "§10.7, `zygo status`" → base §10.7. Replace the last sentence `The status line shows the session's cache-read share and trimmed bytes.` with:

  ```text
  `zygo usage` also prints `tokens trimmed (estimate): <n>` (bytes ÷ 4, rounded up) after the exact `bytes trimmed: <n>` line. `zygo status` in a session prints its savings in tokens (`savings: cache 92%, trimmed 23k tok`) and a `spend: $3.41` line with the session spend of §7.2 (`≈` when notional); outside a session it prints no spend line. The status line shows the session's cache-read share and trimmed tokens. *(2026-11-22, HUD design)*
  ```

- [x] 12. H§9 row "§14 usage recording" → base §14, the tee bullet. After its sentence `Usage is written after the stream ends or the client disconnects.` insert:

  ```text
  The tee also reads `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` when present, for the cost: 5-minute writes at `cache_write_5m` (else `cache_write`), 1-hour writes and writes with no split at `cache_write` (§6.3); the row keeps one cache-write total (HUD design AU33). *(2026-11-22, HUD design)*
  ```

- [x] 13. H§9 row "§17, `zygo doctor`" → base §17 command table, the `zygo doctor` row. Replace its "Does" cell with:

  ```text
  Agent binaries and versions, secret store, keys present per provider, server reachable, plugin store, missing plugin commands, plugins also installed in the agent's own config, repo policy, and one icon test line: the Nerd glyphs of the status line and the active icon set (HUD design AU2). *(2026-11-22, HUD design)*
  ```

- [x] 14. H§9 row "§20" → base §20. Insert this group after item 21 and before `**Profiles**`:

  ```text
  **Status line** *(2026-11-22, HUD design)*
  21a. The status line HUD passes the acceptance items #1 to #38 of `docs/specs/2026-11-21-statusline-hud-design.md` H§14: rendering (#1-#7); ctx and threshold (#8-#9); spend and tokens (#10-#17); trimmed tokens (#18-#19); width, with the fit width W = `COLUMNS` − 4 (#20-#21, H§6.7); input, deadline and failure (#22-#30); icons and style (#31-#35; except the OpenCode clause of #33: in an OpenCode session the `statusline` verb behaves as in a Claude Code session, operator decision); the workspace phase, with the workspaces build (#36); the live checks P-HUD-1 (#37) and W-SHELL part 2 (#38: not run, no Windows machine available).
  ```

### Auto-update "Base-spec changes" rows

- [x] 15. Row "§3" → base §3, the last bullet. Replace `- Telemetry that leaves the machine. Nothing phones home.` with:

  ```text
  - Telemetry that leaves the machine. Nothing phones home, with one exception: the update check reads the release's public VERSION file and sends nothing else; opt-out `[update] check = false` / `ZYGO_NO_UPDATE_CHECK=1` (§17 `zygo update`; auto-update design `docs/specs/2026-11-21-auto-update-design.md`). *(2026-11-22, auto-update design)*
  ```

- [x] 16. Row "§15.7" → base §15.7, the approval bullet. Replace `A launch never prompts; `--yes` approves in scripts.` with:

  ```text
  A launch never prompts, except the update prompt (§17 `zygo update`, auto-update design) *(2026-11-22, auto-update design)*; `--yes` approves in scripts.
  ```

- [x] 17. Row "§17" → base §17 command table, the `zygo update` row. Keep its "Does" text and the N36 note; append after the N36 note:

  ```text
  The start check: `zygo claude` and `zygo opencode` run the update check before the launch, at most once in 24 h: one GET for the release's public VERSION file, 2 s limit, the system proxy not used, the time and result kept in `~/.cache/zygo/update.json` (Windows: `update.json` in the zygo cache directory); a failure is silent and the launch continues. A newer release that was not skipped is asked about: `zygo <new> is available (you have <old>). Update now? [y/N/s]`. `y` installs it by the path above, prints `updated to <new>; relaunch zygo` and exits 0 without starting the agent (a failed install prints `zygo update: <reason>` and exits with its code); `N`, the default, and any other answer launch and ask again after 24 h; `s` records the skip and asks again only for a newer release. When other zygo sessions are live, the prompt comes after the line `<n> zygo sessions are running; they keep the old version until you restart them (zygo down / zygo up)`; on Windows the update is then deferred and the launch continues. No check and no prompt when stdin or stderr is not a terminal, with `-p` or `--print`, inside a zygo session (`ZYGO_SESSION` set), in a workspace pane (`ZYGO_WORKSPACE` set), or with the opt-out of §3 (`ZYGO_NO_UPDATE_CHECK` with any non-empty value). `zygo serve` does not check. *(2026-11-22, auto-update design)*
  ```

### Change log and verification

- [ ] 18. Base `## Change log`: append this row:

  ```text
  | 2026-11-22 | §3, §6.1, §6.3, §7.2, §10.2.2 (N15 note), §10.2.4, §10.7, §14, §15.7, §17 (status line paragraph, `zygo statusline`, `zygo doctor`, `zygo update` rows), §17.1, §20 (21a) | operator (statusline HUD design H§9 and auto-update design "Base-spec changes", both approved 2026-11-21); texts by DOC-1 | The two approved designs change these sections. Each section is rewritten in place, and its new text is marked *(2026-11-22, HUD design)* or *(2026-11-22, auto-update design)*. The `zygo up` start check and the W§15 rows are workspaces D1's. |
  ```

- [ ] 19. Verification: run the Acceptance command alone from the node checkout root; it prints nothing. Then run `grep -n "2026-11-22" docs/specs/2026-11-18-zygo-design.md` and confirm one dated edit per step 1-17 in the section the step names: 13 marks `(2026-11-22, HUD design)` for steps 1-4 and 6-14, the `AU29, 2026-11-22` sentence for step 5, 3 marks `(2026-11-22, auto-update design)` for steps 15-17, and the change-log row of step 18.
