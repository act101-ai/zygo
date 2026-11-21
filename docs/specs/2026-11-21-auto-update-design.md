# zygo: auto-update on start — design

**Status:** approved by the operator 2026-11-21
**Base:** docs/specs/2026-11-18-zygo-design.md (§n). Size: small change.

## Intent

The operator wants this pattern:

1. zygo checks for an update on start.
2. If an update exists, zygo asks.
3. On yes, zygo downloads and installs the update.
4. The user relaunches.

`zygo update` (crates/zygo/src/cmd/update.rs) already does these steps:

- It finds the latest release from the VERSION file (version rule :10; fetch and parse :50-56).
- It verifies the SHA-256 checksum (:80-90).
- It replaces the binary atomically (:164-198). On Windows it swaps through `.exe.old` (:225-238).

Nothing checks at start. Nothing asks. Nothing remembers a check.

## Conflicts with the base spec (operator's call: amend)

- §3 says: "Telemetry that leaves the machine. Nothing phones home." The check sends a GET for the public VERSION file to GitHub on each check. GitHub sees the IP address and the time.
- §15.7 says: "A launch never prompts".

Both rules get an explicit exception. See "Base-spec changes".

## Design (operator-approved)

### Where

The check runs in `zygo claude`, `zygo opencode` and `zygo up`, before the launch. The dispatch is at crates/zygo/src/cmd/mod.rs:395-396.

### When the check is skipped

- stdin or stderr is not a TTY.
- `-p` or `--print` is given.
- The command is `zygo serve`.
- The command runs inside a zygo session (`ZYGO_SESSION` is set).
- The command runs in a workspace pane (`ZYGO_WORKSPACE` is set): the workspace's `zygo up` already asked. `ws run` runs the check before it cleans its environment, so it sees the session's `ZYGO_WORKSPACE` and skips too.
- `zygo up` asks one time for the whole workspace.

### Check

- The check runs at most one time in 24 h.
- The total time limit is 2 s.
- The result is cached in `~/.cache/zygo/update.json`. On Windows, use the `cache_dir` equivalent from crates/zygo-core/src/paths.rs.
- A network failure is silent. The launch continues.

### Prompt

```
zygo <new> is available (you have <old>). Update now? [y/N/s]
```

- **y:** Run the existing `zygo update` path (checksum, atomic replace). Print `updated to <new>; relaunch zygo`. Exit 0. Do not start the agent. Reason: hooks run the absolute binary path (crates/zygo-launch/src/claude_code.rs:38), so a running session would mix versions.
- **N (default):** The launch continues. Ask again after 24 h.
- **s:** Skip this version. Do not ask again until a newer release exists.

### Other sessions

If other zygo sessions are live, the prompt adds this line:

```
N zygo sessions are running; they keep the old version until you restart them (zygo down / zygo up)
```

On Windows, the update is deferred while other sessions are live. zygo prints that line and continues the launch. Reason: the locked `.exe.old` makes a second update fail (update.rs:225-238).

### Opt-out

- `[update] check = false` in the user config, or
- `ZYGO_NO_UPDATE_CHECK=1`.

### Proxy

No change. The check ignores the system proxy, as `zygo update` does (N36, update.rs:144).

## Base-spec changes

- **§3:** Add an exception: "the update check reads the release's public VERSION file and sends nothing else; opt-out `[update] check = false` / `ZYGO_NO_UPDATE_CHECK=1`."
- **§15.7:** Change to: "a launch never prompts, except the update prompt (auto-update design)."
- **§17:** Add the check text to the `zygo update` row.

## Files expected to change

- crates/zygo/src/cmd/update.rs — split check and install, so that the start check can use them again.
- crates/zygo/src/cmd/mod.rs (or claude.rs / opencode.rs / up) — call the start check before the launch.
- crates/zygo-config/src/schema.rs — add `[update] check`.
- crates/zygo/tests/update.rs — the acceptance tests.
- docs/specs/2026-11-18-zygo-design.md — §3, §15.7, §17.

## Acceptance (fixture server, as in crates/zygo/tests/update.rs)

1. A newer version on a TTY prompts.
2. y installs, prints the relaunch line, exits 0, and does not start the agent.
3. N launches.
4. s writes the skip. The same version does not prompt again. A newer version prompts.
5. These cases give no prompt and no request: non-TTY, `-p`, `ZYGO_SESSION`, `ZYGO_WORKSPACE`, `[update] check = false`, `ZYGO_NO_UPDATE_CHECK=1`, and a check less than 24 h old.
6. An unreachable server: the launch starts with no delay of more than 2 s.
7. Other live sessions: the extra line shows. On Windows the install is deferred.

## Left to the builder

- The TTY detection crate or API.
- The prompt helper (`confirm` exists at crates/zygo/src/cmd/plugins.rs:101).
- The exact fields of update.json.

## Citation corrections

All cited line numbers were verified against the repo on 2026-11-21 (branch feat/workspaces). No line number needed a fix. Notes:

- update.rs:10 is `VERSION_RULE`, the version-string rule. The VERSION file URL and its parse are at :50-56.
- update.rs:164-198 covers `temp_path`, `replace_binary` and the non-Windows `swap_in`.
- mod.rs:395-396 dispatches `zygo claude` and `zygo opencode` only. `zygo up` does not exist in the code yet. It is planned in docs/work/phase2/nodes/W7.md (its `zygo up` check is node M5 of that graph). The start check for `zygo up` attaches where W7 adds its dispatch.

## Change log

| Date | Section | Decided by | Reason |
|---|---|---|---|
| 2026-11-21 | Citation corrections (last bullet) | author's call (lead decision: one merged graph) | The workspaces graph moved to `docs/work/phase2/`; the pointer names the new path and node M5. |
| 2026-11-21 | When the check is skipped | author's call | The intent "workspace panes never ask" was false for a member started by hand in a pane: `up` and `ws run` remove `ZYGO_SESSION` from the pane environment (W§18 #17), but the pane keeps `ZYGO_WORKSPACE` (AU93). |
| 2026-11-21 | Acceptance #5 | author's call | Matches the `ZYGO_WORKSPACE` skip added to "When the check is skipped". |
| 2026-11-21 | When the check is skipped (`ZYGO_WORKSPACE` bullet) | author's call | `ws run` now passes through the start check, so a test can prove that the `ZYGO_WORKSPACE` skip is what keeps panes from asking. |
