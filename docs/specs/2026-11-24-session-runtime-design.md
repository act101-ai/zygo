# zygo: session run time — design

**Status:** approved by the operator 2026-11-24
**Date:** 2026-11-24
**Item:** P3-13 (`docs/roadmap/phase-3.md`, "P3-13 design notes", including "Resolved with the operator (2026-11-23)")
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, decisions `W AUn`, constraints `W-Cn`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`, decisions `H AUn`/`H OPn`, constraints `H-Cn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`). `T§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the P3-13 design notes and their resolutions). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767` (branch `phase-3`).

---

## 1. Intent

A workspace member runs for days. The HUD and `zygo status` do not show how long it has run. This item adds one figure, the **run time**: the wall-clock time since the member's first start. A session outside a workspace shows the time since its own start **(operator)**.

## 2. Description

### 2.1 What the figure is

- **Member** (a session with the membership marker, `view.workspace`, `crates/zygo-core/src/session_view.rs:34-37`): now minus `member.first_started_at`, a new field of the member record (T§3.2). The record persists across `zygo down`, `zygo up`, a snapshot and a resume (W§5 "Member"), so the run time continues across them. It is not the process start **(operator)**.
- **Time while down counts.** The figure is wall-clock time since the first start. A member that ran 1 day, was down 2 days, and then resumed shows `3d00h`. **(author**, from the operator's "wall-clock time since the member's first start"**)**.
- **Session outside a workspace** (W-C6): now minus the session's own start **(operator)**. The source is the zygo session id. The proxy makes the id with `ulid()` when it creates the session (`crates/zygo-proxy/src/session.rs:528`; `ulid()` is `ulid::Ulid::generate()`, `crates/zygo-core/src/ids.rs:2-3`). The launcher writes it into `view.json` as `session` (`crates/zygo-launch/src/launcher.rs:584-585`). A ULID holds its creation time in milliseconds (`Ulid::timestamp_ms`, ulid 3.0.0). The start is `timestamp_ms ÷ 1000`, floor. **(author)**. In serve mode the team server, which runs the same proxy session code, makes the id, and the launcher takes it from the grant (`launcher.rs:404`, `:414`). So the start is on the server clock (T§2.6).
- **Active time** (time with a request in flight) is not shown **(operator)**. The Claude Code stdin `cost` key is never read (H§3) **(operator)**.

### 2.2 Start-time rule

One function gives the start, for the HUD and for `zygo status` **(author)**. Input: `view.json` of the session, and for a member the `workspace.json` read that the HUD already does for the phase (H§7.6). A value of 0 counts as unknown everywhere: the start write puts 0 when the clock is before 1970 (`crates/zygo/src/cmd/member.rs:197`, `map_or(0, …)`).

1. No `view.workspace`: the session's own start (T§2.1). A `session` that is not a ULID gives no start.
2. `view.workspace` is set. The reader checks `ws_id` (`^[0-9a-f]{16}$`, W AU84) and reads `workspace.json` as H AU25 does (regular file, at most 64 KiB, `version` 1; `crates/zygo-hooks/src/statusline/phase.rs:57-77`). A failure of any of these gives **no start**: the segment and the status line are hidden, as the phase segment and the M§9a segments are on a read error. A wrong value is worse than no value.
3. The record is `members[<view.workspace.member.pane>]`. When there is no record, or the record does not belong to this zygo session (neither `zygo_session` nor `pending.zygo_session` equals `view.session`; the rule of `record_of`, `crates/zygo-hooks/src/workspace.rs:480-482`), use the session's own start (rule 1). This is the case of a launch that took the pane while another record holds it.
4. `first_started_at` set: that value.
5. Else, `pending` names this zygo session: `pending.started_at`. This covers the time from the start write to the commit, and an old record (T§4).
6. Else: the session's own start.

### 2.3 Format

**(operator)** Two units at most, no seconds: `47m`, `5h12m`, `2d04h`, `12d07h`. **(author)** the exact rule:

- `d` = now − start, in whole seconds. When start > now (clock change, server clock ahead), `d` = 0.
- `d` < 3600: `<m>m`, `m` = floor(d ÷ 60), no padding. `0m` to `59m`.
- 3600 ≤ `d` < 86400: `<h>h<mm>m`, `h` no padding, minutes two digits. `1h00m` to `23h59m`.
- `d` ≥ 86400: `<D>d<hh>h`, `D` no padding, hours two digits. `1d00h`, `2d04h`, `12d07h`, `100d00h`. Every `u64` has a form; no cap.
- Floor at each unit. No rounding up, so the figure never shows a time that has not passed.

**Fixed width within each range (operator).** The second unit always has two digits, so the width does not change while the first number keeps its digit count. The width changes only at 10m, 1h, 10h, 1d, 10d and 100d. The segment is the last segment of its row (T§2.4), so these changes do not move the segments before it. Padding the first number with spaces was rejected: the operator's examples (`5h12m`, `2d04h`) have no padding.

### 2.4 HUD segment (Claude Code, both styles)

**(operator)** both HUD styles show it (H§8). **(author)** the row, the group and the place in the removal order (the open point of the P3-13 notes):

| Property | Value |
|---|---|
| Text | Nerd set: `󰅐 <form>` (U+F0150, nf-md-clock_outline). Plain set: `run <form>`. The same text in compact and verbose. |
| Compact place | Its own group, after the trimmed group and before the `stale`/`offline`/`suspended`/`ended` group. |
| Verbose place | Row 1, its own group, after the mode group (last on row 1). Row 1 is the shorter row (H§6.3 mockups), so it has the free width. |
| Color | `2` (dim), as tokens and trimmed. |
| Kind (H AU20) | Local: it renders from its own sources with or without status data, and `stale` does not change it. It is not proxy-derived. |
| Shown when | T§2.2 gives a start. |
| Read | Fresh on each call, never cached (as H AU25). `now` is the HUD clock, `seams.now` (`ZYGO_TEST_NOW` under `test-hooks`) else the system clock (`crates/zygo-hooks/src/statusline/data.rs:584`). |
| Width (H§6.7) | Removed for width. Compact: third, after trimmed and cache. Verbose row 1: second, after mode. |
| Width counting | The Nerd glyph counts 2 cells (H§6.7 rule for private-use glyphs). |

Reasons for the place **(author)**:

- Last in the row: the width changes of T§2.3 move no other segment. Only the marker after it in compact can move, and the marker shows only on a fault.
- Not in the first group: the first group holds segments that are never removed (phase, and the M§9a member name and mail). The run time is less urgent than the work item and the context.
- Removal order: the run time changes slowly and the user reads it seldom. The figures of H-G1 (context, tokens, spend) stay longer. Trimmed and cache go first, as they do today, because they are the least used in a narrow pane.

### 2.5 `zygo status` (OpenCode and scripts)

**(operator)** one line in `zygo status` (A-9, H§3). **(author)** the line is `runtime: <form>`, the T§2.3 form. It is printed inside a session only (`ctx.session_dir` is set, `crates/zygo/src/cmd/status.rs:37-39`), right after the `session:` line, also after `session: unreachable (…)`: the start is local and needs no proxy. With no start (T§2.2), there is no line. Outside a session there is no line. The line holds no window number, so the C7 rule (`hide_window`, `status.rs:25-27`) does not change it. `/zygo:status` prints the same text (§17.1 one rule).

### 2.6 Cost and limits

- **Cost (operator):** one read of a stored timestamp, no new process, no change to H-C2 (3 s). For a member, the start comes from the `workspace.json` read that the phase segment already does (H§7.6, `phase.rs:57-77`): one read serves both. For a session outside a workspace, there is no file read: the start is decoded from `view.session`, which the HUD has already loaded (`data.rs:585`).
- **Refresh:** Claude Code runs the status line every 30 s (`REFRESH_INTERVAL`, `crates/zygo-launch/src/claude_code.rs:423`), so a minute figure is at most 30 s late.
- **Team session clock (author, accepted):** outside a workspace the start is on the team server's clock. A skew shifts the figure by the skew; a server ahead of the local clock gives `0m` until local time passes the start. A member uses the local clock (`member.rs:197`).

## 3. Base-spec changes

### 3.1 HUD spec (H), as M§9a did for its two segments

This section amends the HUD spec. HUD OP4 (no segment list, no format string) does not change: the run time is one more fixed segment. The groups below include the M§9a amendment.

| Base section | Change |
|---|---|
| H§6.2 segment table | Add a row after row 14 (Trimmed): `Run time` · source `workspace.json` `members.<pane>.first_started_at`, else `pending.started_at`, else the ULID time of `view.session` (T§2.2) · color `2` · C: yes, `󰅐 5h12m` · V: row 1, `󰅐 5h12m`. |
| H AU9 compact groups | `[logo, workspace phase, member name, mail, work item, worktree], [model, profile], [ctx, bar, threshold], [tokens], [spend], [cache], [trimmed], [run time], [stale/offline/suspended/ended]`. |
| H AU9 verbose row 1 groups | `[workspace phase], [member name, mail], [work item, worktree, ahead/behind], [model · effort], [profile], [mode], [run time]`. Row 2 does not change. |
| H AU20 | Add the run time to the list of local segments. |
| H§6.4 color table | Add `Run time` · `2` · same. |
| H§6.5 icon table | Add `Run time` · `󰅐` (nf-md-clock_outline) · U+F0150 · word `run` · —. |
| H§6.6 | Add a bullet: run time uses the T§2.3 form. It has no maximum width and no truncation. |
| H§6.7 compact order | trimmed → cache → **run time** → tokens → threshold `⇣` → spend → profile → model → bar cells → `stale` → worktree → ctx numbers → work item. |
| H§6.7 verbose row 1 order | mode → **run time** → profile → effort → ahead/behind → model → worktree → work item. |
| H§6.8 | Add two rows. "Member, and `workspace.json` cannot be read (bad id, missing, not a regular file, above 64 KiB, does not parse, `version` not 1)": no run time segment. "Not a member, and `view.session` is not a ULID": no run time segment. |
| H§7.6 | The reader also takes, for the member's pane key, `first_started_at`, `zygo_session` and `pending.{zygo_session, started_at}` with its own small serde struct (unknown fields ignored). Still no dependency on `zygo-workspace` from the status line module. |
| H§6.3, H§6.4 examples | The reference bytes stay those of a HUD without the run time. With a run time of `5h12m`, OP13 compact gets `  󰅐 5h12m` after `✂ 23k` and before the last `\e[0m` (dim gap absorbed, H§6.4 step 3); OP13 verbose row 1 gets `   󰅐 5h12m` after `local` and before its `\e[0m`; example 3 (stale) gets `  󰅐 5h12m` before `  stale` in the same dim run. |

### 3.2 Workspaces spec (W)

| Base section | Change |
|---|---|
| W§7.2 field table | Add after `member.pending`: `member.first_started_at` · "Unix time (seconds) of the first committed start of this member record: the `pending.started_at` of the first commit (W AU92). Absent until then, and in a record of an older zygo (T§4). Never changed after it is set; a new record (a fresh loadout member, `up --reset`, a hand start that replaces the record, `zygo ws forget-pane`) starts without it." |
| W AU92 | Add: "Each commit of a pending start (the member `SessionStart`, the OpenCode `session` call, the wrapper's exit commit) sets `first_started_at = pending.started_at` when `first_started_at` is absent or 0 and `pending.started_at` is not 0. The start write itself does not set it: a start that never spawns (`spawned = false`) leaves no trace." |
| W AU29 | No change: `version` stays 1. The field is optional with a serde default, and an older zygo keeps it through the flattened unknown fields (W AU78; `crates/zygo-workspace/src/state.rs:128-129`). |

### 3.3 Base spec (§)

| Base section | Change |
|---|---|
| §17, `zygo status` row | Inside a session, add "the `runtime:` line (session run time; P3-13)". |
| §8.1 A-9 row | No change. The run time is one more line in the same surfaces. |

## 4. State migration

A member record written by a zygo before this field has no `first_started_at`. It reads with the field absent (serde default). Behavior:

- **Running member, record already committed** (no `pending` for this session): T§2.2 rule 6, the session's own start. The HUD shows the time since this launch until the next start.
- **Next start (resume after `zygo up`, or a hand start that keeps the record):** the start write puts `pending.started_at`; the HUD shows it (rule 5); the commit then sets `first_started_at` to the same value (T§3.2), so the figure does not jump at the commit.
- **Result:** for an old record, the run time counts from the first start under the new zygo, not from the true first start. No stored value holds the true first start: `snapshot_at` and `phase_since` are workspace times, and the last `zygo_session` ULID is the last start. This is accepted **(author)**.
- **Downgrade:** an older zygo keeps the field through W AU78 and never sets or changes it.

## 5. Flows

Each flow, as a builder walks it. "Start write" is `record_start_fields` (`crates/zygo/src/cmd/member.rs:430-438`). "Commit" is the hook commit (`crates/zygo-hooks/src/workspace.rs:401-418`) or the exit commit (`member.rs:481-489`).

| Flow | What happens | HUD / status |
|---|---|---|
| Fresh member (`zygo up` from a loadout) | `fresh_member` makes a new record (`crates/zygo-workspace/src/up.rs:661-663`), no field. The start write puts `pending` (`started_at` at `member.rs:197`). The first main-loop `SessionStart` commits and sets `first_started_at`. | `0m` from the start write on (rule 5, then rule 4, same value). |
| `zygo down`, then `zygo up` | `down` keeps the record. `up` restore resumes a `running`/`resumable` member with an agent session id (`up.rs:792-794`). The start write puts a new `pending`; the commit keeps the set field. | Time since the first start, down time included. |
| Snapshot | A snapshot writes windows and panes. Member records pass through the typed `Store::update` and keep the field. | No change. |
| Crash (tmux server killed, reboot) | `workspace.json` is on disk. `zygo up --all` restores as above. | As `down`/`up`. |
| Crash between start write and commit, first start | `pending` stays; no field. The next start replaces `pending`; its commit sets the field. | The run time then counts from that later start. Accepted: the first start was not confirmed. |
| Commit without the lock (W AU60 known limit) | The hook commit fails; the wrapper commits at agent exit and sets the field from the same `pending.started_at`. | Rule 5 until then; same value. |
| Hand start, `zygo claude` in a member pane (W§8.4) | With the saved conversation, the record is kept (`member.rs:803-812`) and the field stays. Else the record is removed and made again (`:805-808`): a new conversation starts at `0m`. | As stated. |
| `up --reset`, `zygo ws forget-pane` | New record or no record. | Starts again at `0m`. |
| `/clear` or compaction in Claude Code | Same zygo session; a later `SessionStart` commits nothing (W AU60). | No change. |
| Old record without the field | T§4. | Session's own start, then the start under the new zygo. |
| Session outside a workspace (W-C6) | No marker in `view.json`. | ULID time of `view.session`. |
| Team session (serve) outside a workspace | The id is made on the server. | T§2.6. |
| OpenCode member | The plugin `session` call commits (`workspace.rs:308-310`). No HUD (H§3). | `zygo status` prints `runtime: …`. |
| Small terminal width | H§6.7 removes segments in the T§3.1 order. | Compact: the run time goes third. Verbose: second on row 1; row 2 does not change. |
| `workspace.json` unreadable for a member | T§2.2 rule 2. | No segment, no status line. |

## 6. Files expected to change

| File | Change |
|---|---|
| `crates/zygo-workspace/src/state.rs` | `MemberRecord.first_started_at: Option<u64>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`; `MemberRecord::new` sets `None` (`:295-314`). |
| `crates/zygo-hooks/src/workspace.rs` | The commit in `record_session` (`:401-418`) sets the field (W AU92 addition). |
| `crates/zygo/src/cmd/member.rs` | The exit commit in `record_exit` (`:481-489`) sets the field. |
| `crates/zygo-core/src/ids.rs` | `ulid_secs(&str) -> Option<u64>`: the ULID time in whole seconds; `None` for a string that is not a ULID. |
| `crates/zygo-hooks/src/statusline/phase.rs` | One read of `workspace.json` gives the phase and the member's start fields; the T§2.2 rule as one public function used by the HUD and by `zygo status`. |
| `crates/zygo-hooks/src/statusline/data.rs` | `HudData` gets the run time in seconds, from `now` (`:584`) and the T§2.2 start. |
| `crates/zygo-hooks/src/statusline/render.rs` | `Seg::RunTime`; `COMPACT` (`:428-437`), `ROW_1` (`:439-440`), `COMPACT_DROPS`, `ROW_1_DROPS` (`:451-465`); `Glyphs` gets the clock glyph (`:154-170`); the public T§2.3 format function. |
| `crates/zygo/src/cmd/status.rs` | The `runtime:` line (T§2.5). `run` takes `now: u64`; `verb` passes the system clock. |

## 7. Acceptance

Unit tests, each with an injected `now`. No test reads the wall clock or waits (project rule: no timing tests in CI).

1. **Format** (the T§2.3 function, input seconds): `0`→`0m`, `59`→`0m`, `60`→`1m`, `599`→`9m`, `600`→`10m`, `2820`→`47m`, `3599`→`59m`, `3600`→`1h00m`, `18720`→`5h12m`, `35999`→`9h59m`, `36000`→`10h00m`, `86399`→`23h59m`, `86400`→`1d00h`, `187200`→`2d04h`, `863999`→`9d23h`, `864000`→`10d00h`, `1062000`→`12d07h`, `8639999`→`99d23h`, `8640000`→`100d00h`; `u64::MAX` gives a form and does not panic.
2. **Start rule** (T§2.2, with `view` and `workspace.json` fixtures): `first_started_at` used when set and the record names this session; `pending.started_at` when the field is absent and `pending` names this session; own ULID start for a record of another session, for no record, and for a committed old record; `first_started_at = 0` counts as absent; no start for a bad `ws_id`, a missing file, a file above 64 KiB, a parse error and `version` 2; no start outside a workspace for a `session` that is not a ULID.
3. **Clock behind start:** start > now gives `0m`.
4. **Commit:** a hook commit and an exit commit of a record with no field set it to `pending.started_at`; a second commit does not change it; a commit with `pending.started_at = 0` leaves it absent; a `NotStarted` exit does not set it.
5. **Migration:** a `workspace.json` member without the field parses, and a write keeps every other field unchanged; a record with the field read and written by the state code keeps it.
6. **HUD bytes:** the T§3.1 compact, verbose and stale examples, byte for byte, with `now` = start + 18720 (`ZYGO_TEST_NOW`, `test-hooks`); the plain set shows `run 5h12m`; `stale` does not change the run time run.
7. **Width:** a compact row that fits only without trimmed, cache and run time keeps tokens; one cell more of width keeps the run time. A verbose row 1 that fits only without mode and run time keeps profile.
8. **`zygo status`:** with a session dir and `now` = start + 187200, the output has `runtime: 2d04h` right after the `session:` line, also when the proxy is unreachable; outside a session there is no `runtime:` line; the existing `inside_a_session` and `outside_a_session` tests (`status.rs:103-139`) pass.

## 8. Alternatives rejected

- **Set `first_started_at` at the start write.** A launch that exits before the spawn would set it.
- **Session start from the session directory or `view.json` time.** Member hooks read, change and save `view.json` (`session_view.rs:38-40`), so its mtime moves; directory birth time is not on every platform. A new `view.json` field would add a write where the ULID already holds the time.
- **Hide the run time for an old record.** Every member that ran before the upgrade would show nothing until its next start.
- **Place in the first group.** That group is never removed for width, and the run time is less urgent than the work item.
- **Take the figure from the proxy status reply.** It needs a running proxy and resets at each launch; the status line would show nothing when the proxy is offline.

What would change these calls: if the operator wants the run time never removed for width, it moves to the first group; if team server clock skew shows in use, the outside-a-workspace start moves to a launcher-written field.

## 9. Brief corrections

- The P3-13 notes cite `member.last_run_at` as "the time of the member's last committed start" (W§7.2, W AU92). No such field exists: there is no match for `last_run_at` in `crates/` or `docs/specs/` at `97d8767`. W§7.2 and the code hold only `member.pending.started_at` (`crates/zygo-workspace/src/state.rs:142`). The operator's conclusion does not change: no stored field is the first start, so this item adds `first_started_at`.
- The M§9a segments (member name, mail) are not in the code yet (`Member` holds `ws_id` and `pane` only, `session_view.rs:128-134`; `COMPACT` has no such segment, `render.rs:428-437`). The groups in T§3.1 include them so that the two amendments agree in whichever order they are built.
