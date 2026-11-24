# Fragment WD — workspace definition as repository files (P3-11, P3-12)

Spec: `docs/specs/2026-11-24-workspace-definition-design.md` (R§n). Node contracts: `docs/work/phase3/nodes/WD-*.md`. First nodes of this track: WD-1 and WD-5 (the assembler adds the DR first node to each).

## 1. Rows

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| WD-1 | Git read of `.zygo/` from the default branch and at a pinned commit | — | TODO | judgment | — |
| WD-2 | Typed shapes, allow-list validation and digest of the workspace files; PR3 | WD-1 | TODO | judgment | — |
| WD-3 | Workspace approvals in `approvals.toml` | WD-2 | TODO | judgment | — |
| WD-4 | Checked use: one read, one digest, one store lookup per use; `Pin` | WD-3 | TODO | judgment | — |
| WD-5 | Config keys: pane `job`/`pool`, `[workspace] pool`, `[limits] max_members`, `.zygo.toml` filter, `config check` rules | — | TODO | implementation | — |
| WD-6 | The repository loadout from `[members]`, with the default set | WD-4, WD-5 | TODO | implementation | — |
| WD-7 | Approved definition added to the merged config with source `workspace-files` | WD-6 | TODO | judgment | — |
| WD-8 | Member record fields `launch.job` and `launch.definition` | WD-4 | TODO | mechanical | — |
| WD-9 | Pool directory listing, assignment and shared lines | WD-5 | TODO | judgment | — |
| WD-10 | Plan panes carry `job`, `pool` and the pin; `Source::Repository` | WD-6 | TODO | implementation | — |
| WD-11 | `zygo up`: loadout choice, checked read, pin, member maximum | WD-7, WD-8, WD-10 | TODO | judgment | — |
| WD-12 | `zygo up`: pool check, assignment and passthrough at a fresh build | WD-9, WD-11 | TODO | judgment | — |
| WD-13 | Launcher: repository profile by a default-branch read (not pinned) | WD-7 | TODO | judgment | — |
| WD-14 | Pinned launches: resume and `ws run` resolve the profile from the pin | WD-12, WD-13 | TODO | judgment | — |
| WD-15 | `zygo workspace approve [--yes] [--digest <hex>]` | WD-14 | TODO | judgment | — |
| WD-16 | `config show/check`, `profile list/show`, `loadout list/show` show the repository files | WD-15 | TODO | implementation | — |
| WD-17 | `zygo doctor` lines: workspace files, pool, member maximum | WD-7, WD-9 | TODO | implementation | — |
| WD-18 | Session text: each member's job and the job sentence | WD-11 | TODO | implementation | — |
| WD-19 | Hooks, MCP server and status line read nothing from `.zygo/` | WD-14 | TODO | implementation | — |
| WD-20 | User docs: workspace files, approval, members, pool, passthrough | WD-12, WD-15, WD-16, WD-17 | TODO | mechanical | — |
| WD-21 | PR1 live check (operator): two pool members send two logins | WD-14 | TODO | lookup | — |
| WD-22 | PR2 live check (operator): per-session settings act the same on a pool directory | WD-14 | TODO | lookup | — |

Edges for file order only (no consumed name): WD-6 → WD-4 (`repo_def/mod.rs`); WD-15 → WD-14 (`docs/specs/2026-11-18-zygo-design.md`, `docs/specs/2026-11-20-workspaces-design.md`); WD-16 → WD-15 (`docs/specs/2026-11-18-zygo-design.md`).

## 2. Files per node

| Node | Files |
|---|---|
| WD-1 | `crates/zygo-config/src/repo_def/mod.rs` (new), `crates/zygo-config/src/repo_def/read.rs` (new), `crates/zygo-config/src/lib.rs`, `crates/zygo-config/tests/repo_def_read.rs` (new) |
| WD-2 | `crates/zygo-config/src/repo_def/mod.rs`, `…/repo_def/shape.rs` (new), `…/repo_def/digest.rs` (new), `crates/zygo-config/tests/repo_def_shape.rs` (new) |
| WD-3 | `crates/zygo-config/src/repo_def/mod.rs`, `…/repo_def/store.rs` (new), `crates/zygo-config/tests/repo_def_store.rs` (new), `crates/zygo-plugins/tests/approve_shared.rs` (new) |
| WD-4 | `crates/zygo-config/src/repo_def/mod.rs`, `…/repo_def/check.rs` (new), `crates/zygo-config/tests/repo_def_check.rs` (new) |
| WD-5 | `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/tests/definition_keys.rs` (new), `docs/specs/2026-11-20-workspaces-design.md`, `docs/specs/2026-11-18-zygo-design.md` |
| WD-6 | `crates/zygo-config/src/repo_def/mod.rs`, `…/repo_def/loadout.rs` (new), `crates/zygo-config/tests/repo_def_loadout.rs` (new) |
| WD-7 | `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/repo_def/mod.rs`, `…/repo_def/apply.rs` (new), `crates/zygo-config/tests/repo_def_apply.rs` (new) |
| WD-8 | `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs` |
| WD-9 | `crates/zygo-workspace/src/pool.rs` (new), `crates/zygo-workspace/src/lib.rs`, `crates/zygo-workspace/tests/pool.rs` (new) |
| WD-10 | `crates/zygo-workspace/src/loadout.rs`, `crates/zygo-workspace/tests/loadout.rs` |
| WD-11 | `crates/zygo-workspace/src/up.rs`, `crates/zygo/src/cmd/up.rs`, `crates/zygo-workspace/tests/up_definition.rs` (new), `docs/specs/2026-11-20-workspaces-design.md`, `docs/specs/2026-11-22-workspace-mail-design.md` |
| WD-12 | `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/tests/up_definition.rs`, `docs/specs/2026-11-20-workspaces-design.md` |
| WD-13 | `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/tests/repo_profile.rs` (new), `docs/specs/2026-11-18-zygo-design.md` |
| WD-14 | `crates/zygo-launch/src/launcher.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/claude.rs`, `crates/zygo/src/cmd/opencode.rs`, `crates/zygo/src/cmd/ws.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/tests/workspace_definition.rs` (new), `docs/specs/2026-11-18-zygo-design.md` |
| WD-15 | `crates/zygo/src/cmd/workspace.rs`, `crates/zygo-core/src/verbs.rs`, `crates/zygo/tests/workspace_approve.rs` (new), `docs/specs/2026-11-20-workspaces-design.md`, `docs/specs/2026-11-18-zygo-design.md` |
| WD-16 | `crates/zygo/src/cmd/config.rs`, `crates/zygo/src/cmd/profile.rs`, `crates/zygo/src/cmd/loadout.rs`, `crates/zygo/tests/definition_surfaces.rs` (new), `docs/specs/2026-11-18-zygo-design.md` |
| WD-17 | `crates/zygo/src/cmd/doctor.rs`, `crates/zygo/tests/definition_doctor.rs` (new) |
| WD-18 | `crates/zygo-hooks/src/session_text.rs`, `crates/zygo-hooks/tests/session_text.rs`, `docs/specs/2026-11-22-workspace-mail-design.md` |
| WD-19 | `crates/zygo/tests/definition_hooks.rs` (new) |
| WD-20 | `README.md`, `docs/passthrough.md` |
| WD-21 | — (the operator writes the record in `docs/live-checks.md`) |
| WD-22 | — (the operator writes the record in `docs/live-checks.md`) |

Base-spec amendments (R§14) by node: WD-5 W§11, §6.1 layer 3, §6.1 layer 4 and §13.5; WD-11 W-C5, W AU10, W AU22, W§14 "Loadouts in `.zygo.toml`", W§8.1 step 7, W§7.2, M-C6; WD-12 W§3, W§14 "Messages, wake, … pools, slots …"; WD-13 §11.1, §11.3; WD-14 §11.2, §12; WD-15 W§12, §15.7, §17; WD-16 §6.1 (`config show` source); WD-18 M§8a item 8.

## 3. Cross-track needs

| Need | From | Used by | Why |
|---|---|---|---|
| `LoadoutPane.name` (W§11 pane key `name`, `^[a-z][a-z0-9-]{0,31}$`, `all`/`cli` reserved) in `crates/zygo-config/src/schema.rs` | (ML: M§6) | WD-5, WD-6 | R§9.1: this spec uses the pane `name`, P3-1 owns it (lead ruling 7). WD-5 also edits `schema.rs`: the ML node that adds `name` must precede WD-5 (shared file). |
| `PlanPane.name` and the name copied into `panes[]` and the member record by `up` in `crates/zygo-workspace/src/loadout.rs`, `up.rs` | (ML: M§6) | WD-10, WD-11, WD-12 | Pool order is byte order of name (R§9.5); passthrough and shared lines name members. Shared files `loadout.rs`, `up.rs`: the ML node(s) must precede WD-10 and WD-11. |
| M§8a item 8 session text (member list, `[features] mail` gate) in the session-start text, expected `crates/zygo-hooks/src/session_text.rs` | (ML: M§8a, M§8) | WD-18 | R§9.3 effect 3 adds the job to item 8. If ML puts item 8 in another file, the assembler replaces WD-18's `session_text.rs` with that file. |
| ML's amendment of M-C6 or M§8a, if any, in `docs/specs/2026-11-22-workspace-mail-design.md` | (ML) | WD-11, WD-18 | Shared spec file: any ML node that edits it needs a path to WD-11 and WD-18. |
| A hand-start `--name` on `zygo claude`/`zygo opencode` (M§6) in `crates/zygo/src/cmd/claude.rs`, `opencode.rs`, `mod.rs`, `member.rs` | (ML: M§6) | WD-14 | Shared files with WD-14: the ML node must have a path to WD-14. |
| Any other track that edits `crates/zygo-launch/src/launcher.rs`, `crates/zygo/src/cmd/ws.rs`, `crates/zygo/src/cmd/doctor.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-workspace/src/state.rs` or `docs/specs/2026-11-18-zygo-design.md` | (P3-13 session runtime; others) | WD-5, WD-7, WD-8, WD-13, WD-14, WD-17 | Shared files: the assembler orders them. |
| DR first node | (DR) | WD-1, WD-5 | The governing spec of the build (R header). |

## 4. Global constraints

- R-C1 C1-C10, W-C1 to W-C9 (W-C5 as amended) and M-C1 to M-C6 (M-C6 as amended) apply — R§4 R-C1, R§14
- Nothing from `.zygo/` applies on a machine before the operator approved its digest for that workspace id on that machine; the approval is the security boundary — R§4 R-C2, R§7.2, R§7.3
- Allow list only: `version` (= 1), `layout` (5 tmux preset names), `members.<name>.{kind, job, profile, pool, cwd}`; profiles `overrides` (≤ 16, keys 1-32 `[a-z0-9-]`, values 1-128 printable ASCII) and `disabled` (≤ 16); any other key at any level makes the whole definition invalid — R§4 R-C3, R§6.2, R§6.3, R§6.4
- Read from git objects of one commit only; never the working tree or index; nothing is written from them — R§4 R-C4, R§6.1
- One 2 s budget for the git calls of one read (`GIT_BOUND`, `crates/zygo-config/src/repo_file.rs:40`); every call through `git_command` (`repo_file.rs:80-99`); full ref names and commit ids, never short names — R§6.1
- Limits: `.zygo/workspace.toml` ≤ 64 KiB; each profile ≤ 16 KiB; ≤ 32 profile files; ≤ 32 members; ≤ 1 orchestrator; profile file names `^[a-z0-9-]{1,32}\.toml$`, stem not `none`; member names `^[a-z][a-z0-9-]{0,31}$`, not `all`, not `cli`; `cwd` printable ASCII (`0x20`-`0x7e`), parts 1-255 bytes, ≤ 1024 bytes, no `.`, `..`, `\`, absolute or `~` — R§6.1 step 4, R§6.2
- Digest: SHA-256 over path, `0x00`, decimal byte count, `0x00`, bytes, per file in R§6.4 order; 64 lower-case hex, 12 in one-line messages — R§6.5
- Store: `[[workspace]] id = <16 hex> digest = <64 hex>` in `Paths.approvals_file` (`crates/zygo-core/src/paths.rs:50`), written with the `approve_in` protocol (`crates/zygo-plugins/src/approve.rs:38-77`); zygo never removes an entry — R§7.2
- A launch never prompts; only `zygo workspace approve` asks; `--yes` for scripts — R§4 R-C5
- zygo never reads, copies or logs files in a pool directory (names and types only) — R§4 R-C6, R§9.5
- Repository text on one line passes `zygo_core::esc` (`crates/zygo-core/src/text.rs:4-16`); the approve listing escapes every non-printable-ASCII character as `\u{<hex>}`; TOML errors print message and position only (`crates/zygo-config/src/repo_file.rs:28-37`) — R§4 R-C7, R§7.1 step 6
- No hook, the MCP server or the status line reads `.zygo/`; `load` (`crates/zygo-config/src/layers.rs:424`) does not read it — R§4 R-C8, R§8
- `[limits] max_members` 1-32, default 8, user and team layers only; `.zygo.toml` value dropped with a line — R§9.4
- A pinned member keeps its pin, pool directory and passthrough for life; a new digest applies at the next fresh `up` only — R§7.4, R§9.5
- Tests: no real `claude`/`opencode`, no real agent config directory, temp git repos with a bare origin, stub agents and stub tmux, no wall-clock assertion — R§4 R-C9, R§18 preamble; owner memory "Stub agents in harness tests", "No timing tests in CI"

## 5. Verification floor additions

- `cargo test -p zygo-config --test repo_def_read --test repo_def_shape --test repo_def_store --test repo_def_check --test repo_def_loadout --test repo_def_apply --test definition_keys` on ubuntu, macOS and Windows — R§18 items 1-4, 8, 10, 12, 21; R§19 PR3
- `cargo test -p zygo --test workspace_approve --test definition_surfaces --test definition_doctor` and `cargo test -p zygo-launch --test repo_profile` on windows-latest (not `cfg(unix)`) — R§18 item 24, R§7.1 step 1
- Live checks PR1 and PR2, operator-run, recorded in `docs/live-checks.md`; never in CI, never by an agent — R§19 PR1, PR2

## 6. Review focus

1. Approval bypass: a use that checks one read and applies another, or applies a partial definition — WD-4 (R§7.3, R§6.4), WD-11 (R§10 `up` row)
2. Short ref names or option injection in git calls; `origin/HEAD` outside `refs/remotes/origin/` — WD-1 (R§6.1 step 2, "Difference")
3. Content read before the size and mode checks; link and submodule modes — WD-1 (R§6.1 step 4)
4. A key outside the allow list accepted at any depth (`deny_unknown_fields` on every level, inline tables included); `cwd` look-alike characters — WD-2 (R§6.2-6.4, R-C3)
5. Approval keyed by digest alone, or a plugin approvals file broken by `[[workspace]]` entries — WD-3 (R§7.2, R§18 item 21)
6. A resume that reads the default branch again, or starts without its pinned profile — WD-14 (R§7.4)
7. Pool fallback to the default config directory; a pool file opened; a resume that reassigns — WD-9, WD-12 (R§9.5, R-C6)
8. Terminal sequences or bidi controls reaching the approve listing or a one-line message raw — WD-15 (R§7.1 step 6, R-C7)
9. A user profile or user `[loadout.repository]` shadowed by the repository; `--layer repository` showing repository-file keys — WD-7 (R§8, R§10 `config show`)
10. A hook or `load` that makes a new git call — WD-19, WD-7 (R-C8)
11. Writes before the R§9.4/R§9.5 refusals (state file, tmux session) — WD-11, WD-12 (R§10 `up` row)

## 7. Author's calls

1. Module split: `crates/zygo-config/src/repo_def/` with `read`, `shape`, `digest`, `store`, `check`, `loadout`, `apply` in place of one `repo_def.rs` (R§17 allows the split); one responsibility per node.
2. R§6.1 step 1 (workspace id) is done by the caller, not inside `repo_def`: `zygo-config` cannot depend on `zygo-workspace` (`crates/zygo-workspace/Cargo.toml` depends on `zygo-config`), so `workspace_id` (`crates/zygo-workspace/src/identity.rs:17`) and checkout discovery (`find_checkout`, `crates/zygo-workspace/src/up.rs:332`; `zygo_work::repo::checkout`, `crates/zygo-work/src/repo.rs:213`) stay where they are and the id is passed to `check_default`/`check_pinned`. The step-1 git call is then outside the 2 s budget of steps 2-5 but under the checkout's own bound.
3. A `GitRunner` seam in place of a fake `git` on `PATH` for the "no content read" and "one read per use" checks (R§18 items 2, 3, 6, 9, 23): it runs on Windows, where a script named `git` is not found by `Command::new("git")`. The `PATH` fake stays only in WD-19 part 2, `cfg(unix)`, with a portable source scan as part 1.
4. Pin type `repo_def::Pin` lives in `zygo-config` so `zygo-workspace` state, the plan and the launcher share one serde shape.
5. The hand-start pin travels back through `LaunchOpts.definition_out` (an `Arc<Mutex<Option<Pin>>>`), not a new `SessionCallback` parameter, so the existing callback sites (`crates/zygo-launch/tests/stub_launch.rs`, `crates/zygo/src/cmd/member.rs:102`) keep their type.
6. `UpOpts.default_loadout_set` carries "the user config file sets `[workspace] default_loadout`" (R§10), because `UpOpts.config` (`crates/zygo-workspace/src/up.rs:65-78`) is a `Config` with no layer sources.
7. Base-spec amendments are placed so that every node that edits one spec file has a dependency path to the others: §15.7 ships with the approve verb (WD-15), not the store (WD-3); W§7.2 with `up` (WD-11), not the record type (WD-8); §12 with the pinned/passthrough launch (WD-14), not WD-12.
8. Workspace-verb tests (`up_definition.rs`, `workspace_definition.rs`) are `#![cfg(unix)]` as the existing `crates/zygo-workspace/tests/up.rs` and `crates/zygo/tests/workspace_run.rs` are (OP5); every other WD test file runs on all three CI systems.
9. PR3 is in WD-2 (the parse it guards), not a separate node.
