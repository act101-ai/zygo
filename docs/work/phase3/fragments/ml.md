# Fragment ML — workspace mail, local store on one machine (P3-1)

Spec: `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`). The assembler adds the DR track's first node to Depends of each ML node whose Depends is `—` (ML-1, ML-2, ML-16).

## 1. Rows

| ID | Node | Depends | TODO | Capability | — |
|----|------|---------|------|------------|---|
| ML-1 | Mail config keys, the loadout pane `name` and the member-name grammar | — | TODO | implementation | — |
| ML-2 | Maildir store: message file, atomic send, read by rename, limits, sweep | — | TODO | judgment | — |
| ML-3 | Member name in the plan, the member record and `panes[]` | ML-1 | TODO | implementation | — |
| ML-4 | Address resolution and the send operation | ML-2, ML-3 | TODO | implementation | — |
| ML-5 | Multiplexer operations for the wake: literal keys and Enter | ML-3 | TODO | implementation | — |
| ML-6 | Wake of an idle member after a send | ML-4, ML-5, ML-17 | TODO | judgment | — |
| ML-7 | Member name in the session view marker, hand start `--name` | ML-3 | TODO | implementation | — |
| ML-8 | `zygo ws name <key> <name>` | ML-5, ML-7 | TODO | mechanical | — |
| ML-9 | Session text item 8 (workspace mail) at main-loop SessionStart | ML-4, ML-7 | TODO | implementation | — |
| ML-10 | Delivery by hooks: Stop block, prompt line, SessionStart line, OpenCode per-request line | ML-9, ML-17 | TODO | judgment | — |
| ML-11 | MCP tools `mail_send`, `mail_read`, `mail_list` | ML-6, ML-7 | TODO | implementation | — |
| ML-12 | CLI `zygo mail send\|read\|ls`, slash `/zygo:mail`, mail sweep at command start | ML-6, ML-7 | TODO | implementation | — |
| ML-13 | `zygo mail watch` | ML-12 | TODO | implementation | — |
| ML-14 | Unread counts in `zygo workspace`, `zygo ls` and `up`; `forget-pane` removes the maildir | ML-2, ML-8 | TODO | mechanical | — |
| ML-15 | HUD segments: member name and unread mail | ML-7 | TODO | implementation | — |
| ML-16 | Live-check script for PM1-PM3 and acceptance #12 | — | TODO | implementation | — |
| ML-17 | Live premises PM1-PM3 recorded (operator run) | ML-16 | TODO | lookup | — |
| ML-18 | End-to-end mail on tmux with stub agents | ML-10, ML-11, ML-13, ML-14 | TODO | implementation | — |
| ML-19 | Live acceptance #12: a member mails by name with no prompt about mail (operator run) | ML-18 | TODO | lookup | — |

Edges added only for a shared file: ML-3 → ML-5 (`crates/zygo-workspace/tests/snapshot.rs`), ML-5 → ML-8 (`crates/zygo-workspace/tests/admin.rs`).

## 2. Files per node

- ML-1: `crates/zygo-config/src/{schema,features,layers,check}.rs`, `crates/zygo-core/src/ids.rs`, `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-core/tests/core_basics.rs`
- ML-2: `crates/zygo-workspace/src/lib.rs`, `crates/zygo-workspace/src/mail/{mod,store}.rs` (new), `crates/zygo-workspace/tests/mail_store.rs` (new)
- ML-3: `crates/zygo-workspace/src/{loadout,state,up,snapshot}.rs`, `crates/zygo-workspace/tests/{loadout,state,snapshot}.rs`
- ML-4: `crates/zygo-workspace/src/mail/{mod,names}.rs`, `crates/zygo-workspace/tests/mail_names.rs` (new)
- ML-5: `crates/zygo-workspace/src/mux/{mod,tmux}.rs`, `crates/zygo-workspace/tests/{mux,snapshot,admin}.rs`
- ML-6: `crates/zygo-workspace/src/mail/{mod,wake}.rs`, `crates/zygo-workspace/tests/mail_wake.rs` (new)
- ML-7: `crates/zygo-core/src/session_view.rs`, `crates/zygo/src/cmd/{mod,claude,opencode,member,ws}.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo/tests/workspace_member.rs`
- ML-8: `crates/zygo/src/cmd/ws.rs`, `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/tests/admin.rs`
- ML-9: `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-hooks/src/{lib,mail,workspace}.rs`, `crates/zygo-hooks/tests/session_text.rs`
- ML-10: `crates/zygo-hooks/src/{mail,workspace}.rs`, `crates/zygo-hooks/tests/workspace.rs`
- ML-11: `crates/zygo-mcp/src/tools/{mail,mod}.rs`, `crates/zygo-mcp/tests/mail_tools.rs` (new)
- ML-12: `crates/zygo/src/cmd/{mail,mod,verbs}.rs`, `crates/zygo-core/src/verbs.rs`, `crates/zygo/tests/{mail_cli,slash_parity}.rs`
- ML-13: `crates/zygo/src/cmd/mail.rs`, `crates/zygo/tests/mail_cli.rs`
- ML-14: `crates/zygo-workspace/src/{admin,up}.rs`, `crates/zygo-workspace/tests/{admin,up}.rs`
- ML-15: `crates/zygo-hooks/src/statusline/{mod,mail,data,render}.rs`, `crates/zygo-hooks/tests/{statusline_mail,statusline_render}.rs`
- ML-16: `scripts/premises/live-mail.sh` (new), `scripts/premises/live-selftest.sh`
- ML-17: `fixtures/claude-code/mail/`, `fixtures/opencode/mail/`, `docs/premises.md`
- ML-18: `crates/zygo/tests/workspace_mail.rs` (new), `crates/zygo/tests/common/e2e.rs`
- ML-19: `fixtures/claude-code/mail/m12.txt`, `docs/live-checks.md`

## 3. Cross-track needs

Produced by ML for other tracks:

- **Member `name` key on loadout panes** — `LoadoutPane.name: Option<String>` and its `config check` rules (ML-1), `PlanPane.name` (ML-3), `MemberRecord.name` and `Pane.name` (ML-3), `zygo_core::ids::valid_member_name` and `RESERVED_MEMBER_NAMES` (ML-1). Consumer: workspace definition, `docs/specs/2026-11-24-workspace-definition-design.md` R§9.1 ("The W§11 pane `name` key is owned by P3-1"), R§6.2 member key grammar. The repository loadout of R§9.1 fills `name` in each pane; its `job`/`pool` checks go into the same `loadout_violations` of `crates/zygo-config/src/check.rs:152`, so that track's check node needs a path from ML-1.
- **Mail session text (M§8a item 8)** — `zygo_hooks::mail::session_item(ws, own_key)` in `crates/zygo-hooks/src/mail.rs` (ML-9), the hook file the workspace-definition spec names ("the hook file that P3-1 adds", R§16 file list). Consumer: R§9.3 item 3 (job in brackets, the job sentence). Its node depends on ML-9 and edits `crates/zygo-hooks/src/mail.rs`; it also needs ML-10 when it edits that file (both ML nodes touch it).
- **M§9a HUD segments** — `Seg` member name and mail, the COMPACT and VERBOSE groups in `crates/zygo-hooks/src/statusline/render.rs` (`COMPACT` at `:428`, verbose `ROW_1` at `:439` today), `HudData.member_name`, `HudData.unread`, `zygo_core::session_view::Member.name` (ML-7, ML-15). Consumer: session runtime, `docs/specs/2026-11-24-session-runtime-design.md` T§3.1 (its groups include the M§9a segments) and T§9 ("not in the code yet"). Both tracks edit `render.rs`, `data.rs` and `session_view.rs`: the assembler must add an edge between ML-15 and the T-track render node (either order; T§9 says the groups agree in either order).
- **Mail store API used by mail sync (P3-2)** — "the mail module" of `docs/specs/2026-11-24-mail-sync-design.md` S§17 is `crates/zygo-workspace/src/mail/` (ML-2, ML-4, ML-6): `Message`, `mail_dir`, `store::{deliver, read, list, unread, sweep, remove_pane}`, `names::{members, resolve, sender}`, `send`, `wake::after_send`; MCP tools `crates/zygo-mcp/src/tools/mail.rs` (ML-11) and the CLI `crates/zygo/src/cmd/mail.rs` (ML-12, ML-13); `[mail]` in `USER_ONLY` (ML-1; S§11 says "add `mail` there if the P3-1 build did not": it does). Note for the sync planner: AU103 limits `zygo-workspace` to `zygo-core`, `zygo-config`, `zygo-work`, `zygo-proxy`; the sync parts that need `zygo-redact` or git (S§8.6, S§8.3) cannot go into `zygo-workspace::mail` without changing AU103.
- **Shared files with other tracks** (edges for the assembler): `crates/zygo-core/src/session_view.rs` (ML-7, ML-9; T track `first_started_at`), `crates/zygo-launch/src/launcher.rs` (ML-9; T track and R track `LaunchOpts` pin), `crates/zygo/src/cmd/member.rs` (ML-7; T track), `crates/zygo-hooks/src/workspace.rs` (ML-9, ML-10; session-text track P3-14/15 and T track), `crates/zygo-core/src/ids.rs` (ML-1; T track), `crates/zygo-workspace/src/state.rs`, `loadout.rs`, `up.rs` (ML-3, ML-14; R track `Launch.job`, `PlanPane` pin), `crates/zygo-config/src/{schema,check,layers}.rs` (ML-1; R track and S track), `crates/zygo/src/cmd/ws.rs` (ML-7, ML-8; R track `launch_of`).

Consumed by ML from other tracks: the DR track's first node (decision-record format), added by the assembler. Nothing else.

## 4. Global constraints

- W-C1 to W-C9 apply to all mail code; C6: a mail error never breaks the agent (each new hook branch exits 0) — M§4 M-C1
- Mail is content: only under `mail/`; never in `workspace.json`, `activity.json`, a log or a usage row; the proxy stores no body — M§4 M-C2
- Every mail write: write in `tmp/`, `fsync`, rename into `new/` — M§4 M-C3, M§5
- `mail/` folders `0700`, files `0600`; only the owning OS user reads or writes — M§4 M-C4 (AU2)
- No mail text reaches a tmux argument; the wake text is fixed: `zygo: you have mail; read it with mail_read` — M§4 M-C5, M§7.3
- Member names come from user config only (loadout, `--name`, `zygo ws name`); a cloned repository cannot send mail or name a member — M§4 M-C6
- Message file version 1, ULID names, never edited after the rename into `new/`; unknown fields kept — M§5 (AU78)
- Limits: subject at most 200 bytes, body at most 1 MiB, `[mail] max_unread = 500`; `[mail] keep = "30d"`; `[mail] wake_gap = "30s"`; `tmp/` files older than 1 h deleted — M§5, M§10, M§11
- Name grammar `^[a-z][a-z0-9-]{0,31}$`, unique in the loadout; `all` and `cli` reserved; checked at each read (AU84) — M§6
- `[mail]` is user config only (team layer and `.zygo.toml` drop it) — M§11 (AU10, AU22)
- `[features] mail`: on in a workspace member, off otherwise — M§8
- No daemon; delivery by hooks and the sender's process — M§2 M-G6, W-G6
- HUD segments: shown only for a member, read fresh, never cached, hidden on any read error, no dependency of the hook on `zygo-workspace`, never dropped for width; pane key validated (`^k[a-z2-7]{8}$`) before the path; message files never opened — M§9a (H AU25)
- Native Windows: every mail verb exits 2 with `workspaces need tmux; run zygo in WSL`; the code builds — M§3 (OP5)
- tmux-dependent tests: run on tmux 3.2 or later, else print the skip reason; `cfg(unix)` on Windows; marked `#[ignore = "timing: tmux e2e: …"]` so CI does not run them — `docs/work/phase2/graph.md` Global constraints; commit `96c1cbc`
- No wall-clock test in CI: time rules (wake gap, watch timeout, sweep age) are tested with an injected clock — operator rule (commits `cb7d13c`, `96c1cbc`)
- CI never runs a real agent; live checks use stub agents in self-tests and run real agents only by the operator — `docs/live-checks.md` "Method"

## 5. Verification floor additions

- `cargo test --workspace --locked --no-fail-fast` on ubuntu-latest, macos-latest, windows-latest covers the CI part of ML-1 to ML-15 — `.github/workflows/ci.yml` job `test`
- Local, unix with tmux 3.2 or later: `cargo test -p zygo-workspace --test mux --test mail_wake --test admin --test up -- --include-ignored` and `cargo test -p zygo --test workspace_mail --test workspace_member -- --include-ignored` — M§13 #1, #2, #5, #6, #7
- Live (operator, recorded): PM1, PM2, PM3 (ML-17), M12 (ML-19) — M§13 #12, M§14
- `sh scripts/premises/live-selftest.sh` stays green with the `live-mail.sh` cases (stub agents only) — ML-16

## 6. Review focus

1. Two readers and a sender on one maildir at once — each message read once, no partial file in `new/` — ML-2 — M§5 (Read, Send), M-C3
2. Wake safety — no wake to a pane whose `@zygo-pane` or session `@zygo-ws` differs; no text but `WAKE_TEXT` reaches `send-keys`; the gap holds under the lock across two senders — ML-6, ML-5 — M§7.3, M-C5
3. Stop loop — block only with `stop_hook_active = false`; `busy` kept while blocking and cleared otherwise — ML-10 — M§7.1 (AU43)
4. C6 on every new hook branch — unreadable `mail/` gives exit 0, no decision change — ML-10 — M-C1, M§10
5. No content outside `mail/` — no subject or body in state, logs, usage, errors.log, HUD; HUD opens no message — ML-4, ML-15, ML-18 — M-C2, M§9a
6. Name trust — names only from user config and CLI; AU84 check at each read; reserved names refused — ML-1, ML-3, ML-7, ML-8 — M-C6, M§6
7. Atomic state edits — name copy in `up`, snapshot keeps `Pane.name`, `ws name` under `lock` — ML-3, ML-8 — W§7.3, AU78
8. Feature gate — tools listed only for a member with `features.mail` on; item 8 only then — ML-9, ML-11 — M§8, M§8a

## 7. Author's calls

- The store lives in `crates/zygo-workspace/src/mail/`, not a new crate: it needs only `zygo-core` and `zygo-config`, and AU103 holds.
- `parse_duration` gains the unit `d`, because `[mail] keep = "30d"` (M§11) and today it takes `s`, `m`, `h` only (`crates/zygo-config/src/schema.rs:661-674`).
- "The mail lock" (M§7.3 step 3, not defined in M§5) is an fs4 lock on `mail/<key>/lock` with the 1 s hook bound (W§7.3).
- The mail sweep runs where the results sweep runs: at the start of each command that is not `hook`, `mcp` or `ws` (`crates/zygo/src/cmd/mod.rs:547-552`, `zygo_results::sweep` at `:551`).
- M§7.2 and M§9 both add a SessionStart line when `new/` is not empty; one line only, the M§7.2 text, after the resume note.
- OpenCode has no blocking Stop (M§7.3 last paragraph): the delivery line rides on its per-request session text (`workspace::session_start` returns `Some(output)` per request, `crates/zygo-hooks/src/workspace.rs:643`), so a busy OpenCode member sees it on its next model request; PM3 checks that it reaches the model.
- `[features] mail` defaults to `true`; tools and item 8 also need a membership marker, which gives "off outside a member".
- `MemberRecord.name` is the source; `Pane.name` is the copy M§6 asks for.
- Hand start `--name` refuses a name another member holds (M§6 "unique"), with one line, before the launch.
- M§13 #5 (2 s) and #7 (1 s) are wall-clock: in CI they are checked by rule with an injected clock (ML-6, ML-13); the times are checked only in the ignored tmux e2e (ML-18).
- The wake runs in the sender's process (MCP server or CLI) after the send returns its ids; a wake failure never fails the send.
- Live checks split into a script node (ML-16, stub self-test) and operator-run nodes (ML-17, ML-19), as phase 2 P2.
