# zygo: workspace definition — design

**Status:** proposed 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track D (roadmap items P3-11 and P3-12, `docs/roadmap/phase-3.md`). P3-13 has its own spec.
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`, constraints `Cn`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, constraints `W-Cn`, decisions `AUn`, premises `PWn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`, constraints `M-Cn`). `R§n` is a section of this document. The governing spec of the build is `docs/specs/2026-11-24-decision-records-design.md`.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's P3-11 and P3-12 design notes, their two "Resolved with the operator (2026-11-23)" lists, and the plan of work of 2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767`.

---

## 1. Intent

Phase 2 reads loadouts from user config only (W-C5, AU10). Each person who works on a repository writes the same loadout and the same model profiles by hand, on each machine.

The operator wants a repository to define its workspace in files: the config, the model profiles and the members **(operator, 2026-11-23)**. Each machine and each person then starts the same workspace, after the operator approves the files on that machine. A repository file is untrusted until that approval: it can come from a clone, a fetch, or an edit by an agent.

## 2. Goals

- **R-G1** A repository defines its members (name, agent kind, job, profile) and its model profiles in files under `.zygo/`. `zygo up` starts that workspace on each machine where the operator approved the files.
- **R-G2** A clone, a fetch, a push or an agent edit cannot start an agent, select a model or reach a credential through these files without a new approval on that machine.
- **R-G3** The members can run on different Claude Code config directories, taken from a pool that the user configures, so that they can run on different agent accounts.
- **R-G4** `zygo config show`, `zygo doctor`, `zygo profile list` and `zygo loadout list` show what comes from the repository files and whether they are approved.

## 3. Non-goals

- A change to `.zygo.toml`: it keeps its four tables and its rules (§6.1 layer 4, §13.5) **(operator)**. One key is filtered out of its `[limits]` (R§9.4).
- Repository content other than the allow lists of R§6.2 and R§6.3: credentials, providers, endpoints, redaction, update settings, prompts, custom roles, plugins, feature keys, agent arguments, shell panes **(operator; author for prompts, custom roles, plugins and features, R§8)**.
- A verb that removes an approval, and approvals shared between machines. An approval is local to one machine (P3-11 note "Trust gate").
- Slots or scheduling of agents. Only the config-directory pool returns from W§3 (R§14).
- A pool for OpenCode (R§9.5).
- Member identity across machines (P3-3) and session run time (P3-13).
- A change to a running workspace when the files change. `zygo up --reset` applies the loadout again (AU10).

## 4. Constraints

- **R-C1** C1-C10, W-C1 to W-C9 (W-C5 as amended in R§14) and M-C1 to M-C6 (M-C6 as amended in R§14) apply.
- **R-C2** zygo applies nothing from `.zygo/` on a machine until the operator approved its digest on that machine. The approval is the security boundary. The default-branch rule is a review boundary only; §13.5 holds **(operator)**.
- **R-C3** The repository files can set only the keys of R§6.2 and R§6.3. Any other key, at any level, makes the whole definition invalid. A repository file never sets a credential, a provider, an endpoint or base URL, a redaction setting, an update setting, `config_dir`, `args`, `yolo`, `passthrough`, `server` or a shell pane **(operator)**.
- **R-C4** zygo reads the files from the git objects of one commit of the default branch. It never reads them from the working tree, the index or the current branch, and it never writes a file from them.
- **R-C5** A launch never prompts (§15.7). Only `zygo workspace approve` asks; `--yes` approves in scripts **(operator)**.
- **R-C6** zygo does not read, copy or log the files of a pool directory. It sets the config directory of the child only. The §8.4 preflight still reads the `settings.json` of that directory, as for any config directory **(operator)**.
- **R-C7** Each repository text that zygo prints passes `zygo_core::esc` (`crates/zygo-core/src/text.rs:4-16`: control characters and bidi controls escaped). A TOML error is printed only as its message and position, never with the quoted source line (the rule of `crates/zygo-config/src/repo_file.rs:28-37`).
- **R-C8** No hook reads `.zygo/`. The read runs only in the entry points of R§10.
- **R-C9** Tests never run a real `claude` or `opencode` binary and never use a real agent config directory: stub agents, temp directories and temp git repositories only. No test asserts a wall-clock time.

## 5. Terms

| Term | Meaning |
|---|---|
| Workspace files | `.zygo/workspace.toml` and the files `.zygo/profiles/<name>.toml`. |
| Definition | The workspace files of one read commit, parsed. It can hold a workspace file, profile files, or both. |
| Read commit | The commit of the default branch that one read resolves first. Each later git call of that read names this commit id. |
| Digest | SHA-256 of the workspace files of one read (R§6.5). |
| Repository loadout | The loadout named `repository` that zygo makes from `.zygo/workspace.toml` (R§9.1). |
| Member | The W§5 member: the agent slot of one pane, with the optional name of M§6. |
| Job | `orchestrator` or `worker`. An orchestrator gives work to the other members and collects their results through workspace mail (M§1). A worker is an elephant: a lead session (§5) that holds context and delegates to `zygo-<role>` subagents. Each `zygo claude` member is already a lead session, so the two differ in their job only, not in how zygo starts them **(operator)**. |
| Pool | A directory in user config whose subdirectories are Claude Code config directories, for example `~/.claude-interns/<name>` **(operator)**. |
| Pool member | A `claude-code` loadout pane with `pool = true`. |

The word "team" keeps its one meaning, the team config of serve mode (§6.1 layer 2) **(operator)**.

## 6. The files (P3-11)

### 6.1 Location and read

**(operator)** A `.zygo/` directory at the repository root holds `.zygo/workspace.toml` and one file per profile under `.zygo/profiles/`. It is read from the default branch, as `.zygo.toml` is (§13.5), so a work branch cannot change the workspace it runs in.

**(author)** One read is these steps, under one 2 s budget (as `GIT_BOUND`, `crates/zygo-config/src/repo_file.rs:40`, `:244-246`), each git call made with `git_command` (`repo_file.rs:80-99`: the caller's `GIT_*` repository variables and every `GIT_CONFIG*` variable removed, `LC_ALL=C`):

1. Find the git common dir and compute the workspace id from it (`crates/zygo-workspace/src/identity.rs:17`, W§5). The approval is keyed by this id.
2. Resolve the default branch as §13.5 does (`repo_file.rs:247-251`: the target of `refs/remotes/origin/HEAD`, else `main`), then resolve it to one commit id, the read commit. Each later call names the commit id, never a branch name. Reason: a branch that moves during the read cannot mix two commits in one definition.
3. List the tree entries of the read commit at `.zygo`, `.zygo/workspace.toml` and `.zygo/profiles`, with mode, type, object id and size.
4. Check the entries before any content is read:
   - `.zygo` absent: the definition is **absent**. `.zygo` that is not a tree (a link, mode `120000`; a submodule, mode `160000`; a file): **invalid**, `.zygo is not a directory`.
   - `.zygo/workspace.toml`: absent, or a regular file (mode `100644` or `100755`). Any other mode: invalid.
   - `.zygo/profiles`: absent, or a tree. Each entry in it whose name ends in `.toml` must be a regular file whose name matches `^[a-z0-9-]{1,32}\.toml$`, with a stem other than `none` (§11.3). Else invalid. Entries whose names do not end in `.toml`, and entries of `.zygo/` other than these two, are not read and are not in the digest; `zygo workspace approve` lists them as ignored.
   - Sizes, from the listing: `.zygo/workspace.toml` at most 64 KiB; each profile file at most 16 KiB; at most 32 profile files. Above a limit: invalid, with the path and the limit.
   - No workspace file and no profile file: absent.
5. Read each file by the object id of its listing entry. The byte count must equal the listed size, and the bytes must be UTF-8. Else invalid.

Results: **absent**; **unread** (a git call failed or passed the budget: counts as absent, with one stderr line, as the §13.5 note of 2026-11-19); **invalid** (the first violation, R§6.4); **found** (the ref, the read commit, the files, the digest, the parsed definition).

Hostile input:

- **Path traversal.** zygo builds no file path from a tree entry name, and the profile name pattern holds no `/`, `.` prefix or `..`. Nothing from the files is written to disk.
- **Links.** The working tree is never read (R-C4), so a link in a checkout has no effect. A link inside the git tree is refused by its mode in step 4.
- **Oversized files.** The sizes are checked from the listing, before any content read.
- **A branch that moves.** Step 2 pins the commit.
- **A ref name that starts with `-`.** Each call names the commit id or a full ref name (`refs/…`), never a short name, so no name is read as an option.

### 6.2 `.zygo/workspace.toml`

**(operator)** The file is a loadout (W§11), not a new format. The table is `[members]`. Each member has a name (M§6), a `kind`, a profile and one new key, `job`. `kind` keeps its W§11 meaning, the agent kind. Not allowed: `config_dir`, `args`, `yolo`, `passthrough`, `server` and a shell pane. **(author)** The allow list:

```toml
version = 1                  # required; the integer 1
layout = "main-vertical"     # optional; even-horizontal | even-vertical | main-horizontal | main-vertical | tiled

[members.orchestrator]       # the key is the member name, M§6: ^[a-z][a-z0-9-]{0,31}$; not `all`, not `cli`
kind = "claude-code"         # optional; claude-code (default) | opencode
job = "orchestrator"         # optional; orchestrator | worker (default)
profile = "rust"             # optional; a profile name, §11.3 name rule
pool = true                  # optional; default false; claude-code only (R§9.5)
cwd = "branch:feature-x"     # optional; branch:<name>, or a relative path (R§9.1)
```

| Key | Type | Rule |
|---|---|---|
| `version` | integer | Required, `1`. |
| `layout` | string | One of the five tmux preset names. A tmux layout string is not allowed: the five names are enough to arrange the members, and the rule keeps repository text out of the tmux command line (W-C8). Default: R§9.1. |
| `members` | table of tables | At most 32 members. At most one member with `job = "orchestrator"`. Absent or empty: the default set (R§9.2). |
| `members.<name>.kind` | string | `claude-code` or `opencode`. `shell` is refused **(operator)**. |
| `members.<name>.job` | string | `orchestrator` or `worker`. |
| `members.<name>.profile` | string | 1-32 of `[a-z0-9-]`, not `none`. A name that no source defines at launch gives the existing line and a launch without it (§11.3, R§8). |
| `members.<name>.pool` | boolean | `true` only with `kind = "claude-code"` (R§9.5). |
| `members.<name>.cwd` | string | `branch:<name>`, with 1-255 bytes after the prefix and no control character; or a relative path: parts split by `/`, each part 1-255 bytes, not `.` or `..`, without `\` or a control character, at most 1024 bytes in all. An absolute path and a path that starts with `~` are refused. |

Why a table keyed by name and not an array: TOML refuses a key given twice, so two members cannot have one name, and the order is the order of the names, not the order in the file.

### 6.3 `.zygo/profiles/<name>.toml`

**(author)** One file holds one profile. Its name is the file stem. The allow list:

```toml
overrides = { review = "gpt-6-sol", code = "claude-sonnet-5-5" }   # role → catalog model id
disabled = ["design"]                                               # bundle roles hidden from the lead
```

| Key | Rule |
|---|---|
| `overrides` | At most 16 entries. Each key 1-32 of `[a-z0-9-]`; each value 1-128 bytes, no control character. |
| `disabled` | At most 16 entries, each 1-32 of `[a-z0-9-]`. |

At least one of the two is not empty (§11.3: a profile with no part set is invalid). That a key is a bundle role and that a model is in the catalog and servable are checked at launch, by the §11.3 validation of the proxy that will serve, because the catalog differs per machine and in serve mode (R§8).

### 6.4 Validation

**(author)** Each file is parsed into a typed shape that refuses any unknown key at every level. The first violation wins, in the order: the workspace file, then the profile files in byte order of name. A violation is reported as `<path>: <key path>: <reason>`, with the key path escaped (R-C7). Examples: `.zygo/workspace.toml: members.a.args: not allowed in a repository file`; `.zygo/profiles/rust.toml: prompt: not allowed in a repository file`; `.zygo/workspace.toml: TOML parse error at line 3, column 1: <message>`. A key from the never-allowed set of R-C3 gets the same "not allowed" reason as any unknown key, so no second list is kept.

One violation in one file makes the whole definition invalid. A partial definition is never approved and never applied. Reason: the digest covers all files (R§6.5), so the operator approves one whole thing.

### 6.5 Digest

**(author)** The digest is SHA-256 over, for each file in the order of R§6.4: the path bytes (`.zygo/workspace.toml`, `.zygo/profiles/<name>.toml`), one `0x00` byte, the byte count in decimal ASCII, one `0x00` byte, the file bytes. It is written as 64 lower-case hex characters, and as its first 12 characters in one-line messages.

The bytes come from git objects, so the line endings of a checkout do not change the digest (unlike §15.7, no CRLF rule is needed).

## 7. Approval

### 7.1 `zygo workspace approve [--yes] [--digest <hex>]`

**(operator)** The approval is a named verb. It shows the files and asks. `--yes` approves in scripts. The approval is for a content digest and is stored as plugin approvals are (§15.7). A new digest needs a new approval. **(author)** The steps:

1. Runs on each platform, native Windows included: it starts nothing and needs no tmux. The OP5 check of `zygo workspace` (`crates/zygo/src/cmd/workspace.rs:12`) is not applied to this subverb, so repository profiles work where workspaces do not.
2. Inside an agent session (`ZYGO_SESSION_DIR` set): exit 1, `zygo: approval is not available inside an agent session; run it in your own shell: zygo workspace approve` (the rule of `crates/zygo/src/cmd/plugins.rs:133-136`). The slash form `/zygo:workspace approve` returns that command and approves nothing.
3. Find the checkout from the current folder; not a repository: exit 2. Do one read (R§6.1). Absent: exit 1, `zygo: this repository has no workspace files at <ref>`. Unread: exit 1 with the reason. Invalid: exit 1 with the violation.
4. Already approved: print `workspace files already approved (<digest>)`, exit 0.
5. With `--digest <hex>` (64 hex characters) that is not the digest of step 3: exit 1, `zygo: the workspace files changed: their digest is <digest>, not <hex>; nothing approved`. Use: an operator reads the files, then approves in a script exactly what was read.
6. Show: the folder, the ref, the read commit and the full digest; each file, with its path and size, line by line, each line through `zygo_core::esc`; the ignored entries; then a summary: each member (name, kind, job, profile, pool, cwd) with the defaults filled in, the number of agents the files start, and each profile with its overrides and disabled roles. Without `--yes`, the listing and the question `approve these workspace files? [y/N]` go to stderr, and one line is read from stdin (`y` or `yes`, any case). With `--yes`, the listing goes to stdout and no question is asked.
7. Store the digest of step 3 (R§7.2). The files are not read again: what is stored is the digest of what was shown. Print `approved <digest>`, then `to start it: zygo up (a workspace that exists already: zygo down, then zygo up --reset)`.

An answer other than yes: print `not approved; run zygo workspace approve`, exit 0, as `zygo plugins approve` does (`plugins.rs:145`).

Known limit, as in the §15.7 note of N30: step 2 guards against a mistake, not a hostile agent. An agent that can run shell commands can remove `ZYGO_SESSION_DIR` and run `zygo workspace approve --yes`.

### 7.2 Store

**(author)** In the plugin approvals file, `approvals.toml` (`Paths.approvals_file`, `crates/zygo-core/src/paths.rs:50`), as a new array of tables:

```toml
[[workspace]]
id = "3f9a1c0d5e7b2a46"   # the workspace id, 16 hex (W§5)
digest = "…"              # 64 hex
```

The write uses the protocol of `approve_in` (`crates/zygo-plugins/src/approve.rs:38-77`): an exclusive lock on `<file>.lock`, the entry appended as text so every byte already in the file stays, a temp file, `fsync`, rename. An invalid approvals file approves nothing, and the verb exits 1 and leaves the file as it is (`approve.rs:54`). The plugin reader's `File` shape has no rule against unknown tables (`approve.rs:16-19`), so a zygo without this feature still reads its plugin approvals from the same file.

Why keyed by the workspace id and not by the digest alone: a hostile repository could copy a definition that the operator approved for another repository and start its agents in itself. Why one file: §15.7 stores approvals there, and the note asks for the same store.

### 7.3 Check at each use

**(author)** Each use (R§10) does its own read (R§6.1), computes the digest of the bytes it read, looks for `(workspace id, digest)` in the store, and applies the parsed bytes of that same read. It never checks one read and applies another. So a change between the approval and the use, or during the use, gives another digest, and nothing is applied: no race between show and approve, and none between check and apply.

A git object is the same on each machine, but each machine has its own approvals file and fetches at its own time. Two machines whose default branches point at different commits can have different digests; each approves what it has.

## 8. Profiles from the repository (P3-11)

**(operator)** A repository profile is one more profile source. The §6.1 layer order does not change. A user profile is used before a repository profile of the same name. A repository profile names roles and models from the user's catalog only; it cannot define a provider.

**(author)** The treatment in §11:

- **§11.1 shape.** The repository source holds `overrides` and `disabled` only (R§6.3). The operator's first pick, "roles and models only", stays. Not allowed: `prompt` and custom roles with prompts, because a repository already has a text channel to the agent (its `CLAUDE.md`), so a prompt key adds risk and no use; `plugins`, because a selection delivers plugin code; `features`, because §15.4 keys turn off the result store, `redaction.content` or work units.
- **Order of sources for one name.** User, then team preset, then repository. A repository profile whose name a user profile or a team preset already has is not used, and `zygo profile list` marks it `(repository, shadowed)`. It is never merged key by key with another profile. Reason: §11.2 already lets the more personal source shadow by name, and the team presets of serve mode are curated; a repository profile only fills names that no config defines.
- **Not a layer.** The approved repository profiles are added to `config.profile` after the merge of the layers, by a new step (`Merged::with_definition`). `Merged::apply_profile` (`crates/zygo-config/src/layers.rs:669-683`) is not changed: a repository profile has no `features`, so it adds no key to layer 5. `load` (`layers.rs:424-532`) does not read `.zygo/`: the hooks call it (`crates/zygo-work/src/hooks.rs:53`, `crates/zygo-mcp/src/tool.rs:60`), and R-C8 keeps their git calls as they are.
- **§11.2 selection.** By `--profile <name>`, by `[user] default_profile` (`zygo profile use <name>` can name a repository profile), or by a member's `profile` (R§6.2). The repository cannot set a default profile.
- **§11.3 validation.** The same rules, by the proxy that will serve: each override key is a bundle role; each model is in the catalog and servable on the agent's dialect; each disabled role is a bundle role, listed once, and not `main_role`. In serve mode the server's catalog decides (§6.1). An invalid profile: the existing line, and the launch goes on without the profile (§11.3; `crates/zygo-launch/src/launcher.rs:331-332`).
- **When the launcher reads.** Only when the requested profile name is in neither the user nor the team config (`launcher.rs:326-328`), so a launch with no repository profile makes no extra git call. Lines, each once, in place of `profile <n> not found; launching without it`:
  - not approved: `zygo: profile <n> is in this repository's workspace files, which are not approved on this machine (digest <12>); launching without it; see zygo workspace approve`;
  - invalid: `zygo: profile <n>: this repository's workspace files are invalid (<why>); launching without it`.
- **§11.4 effects.** No change. The usage row records the profile name.

## 9. Members (P3-12)

### 9.1 From `[members]` to a loadout

**(author)** The `[members]` table is the source form. A W§11 loadout is the resolved form, and `zygo up` sees only the resolved form. zygo makes the repository loadout as one window:

- No window name (automatic rename; no repository text goes to `-n`, W-C8, AU55).
- Layout: `layout` when set; else `main-vertical` when there is an orchestrator, else `tiled`.
- Panes: the orchestrator first, then the other members in byte order of name. Each pane is one `[[loadout.repository.window.pane]]` entry with `name`, `kind`, `job`, `profile`, `pool` and `cwd` from the member, and every other W§11 field unset, so it takes the default of `zygo claude` or `zygo opencode` (AU45).

The W§11 pane gets three keys, in user loadouts too: `name` (M§6; added by P3-1, which this item depends on), `job`, and `pool`. `zygo config check` (`crates/zygo-config/src/check.rs`) adds: `job` is `orchestrator` or `worker`; at most one orchestrator per loadout; `pool` only with `kind = "claude-code"`; `pool` and `config_dir` not both set.

`cwd` resolves as W§11 does (`crates/zygo-workspace/src/loadout.rs:116-128`): `branch:<name>` to the worktree that has the branch checked out, else the main worktree with one warning; a relative path under the main worktree. R§6.2 refuses an absolute path, `~` and `..`, so a repository file cannot name a folder outside the repository by its text. A link inside the checkout can still lead out; that is accepted, because the checkout is the user's own and an agent can change its folder anyway.

### 9.2 The default set

**(operator)** When no member is defined: one orchestrator and four elephant workers. **(author)** This applies when `.zygo/workspace.toml` exists and its `[members]` is absent or empty. Without `.zygo/workspace.toml`, the repository defines no loadout and `zygo up` behaves as today. Reason: a repository with profile files only must not start five agents.

The default set: `orchestrator` (`job = "orchestrator"`) and `worker-1` to `worker-4` (`job = "worker"`), each `kind = "claude-code"`, `pool = true`, no profile, no `cwd`. With no pool configured, `pool = true` has no effect (R§9.5).

### 9.3 The job

**(operator)** The job key is `job = "orchestrator" | "worker"`. **(author)** Its effects:

1. The pane order and the default layout (R§9.1).
2. The member record keeps it: `member.launch.job` (W§7.2), set by `up` from the loadout pane (AU45), kept at each resume; absent for a pane with no job and for a hand start (AU9).
3. The session text. With `[features] mail` on (M§8), item 8 of M§8a lists each member with its job, `Members: orchestrator (orchestrator), worker-1 (worker), …`, and adds one fixed sentence for the member itself:
   - orchestrator: `Your job: orchestrator. Give work to the workers with mail_send, collect their results with mail_read, and check each result against its brief.`
   - worker: `Your job: worker. Take your work from <orchestrator> by mail, do it through your zygo-<role> subagents, and send the result to <orchestrator> with mail_send.` With no orchestrator in the workspace, `<orchestrator>` is `the member that gave you the work`.

   The text holds names and tool names only (M-C2, C7). With mail off there is no job sentence, because it names mail tools.

Why a fixed sentence and not a repository prompt: it makes `job` mean something to the agent without any repository text in the session text (R§8).

### 9.4 The member maximum

**(operator)** `[limits]` in user config sets a maximum member count that a repository file cannot exceed. **(author)** The rule:

- `[limits] max_members`, an integer 1-32, default 8. Set by the built-in, team and user layers.
- `.zygo.toml` cannot set it. The repository-file load removes `limits.max_members` and prints `.zygo.toml: limits.max_members ignored; only user or team config may set it`, as it does for `guards.min_level` (`layers.rs:505-509`). Why not the "lower value" rule of the turn cap: a repository cannot raise the maximum by construction, a lower value from it has no use, and one rule needs fewer tests.
- `zygo up` counts the agent panes of the repository loadout. Above `max_members`: exit 1 before any write, `zygo: this repository's workspace files start <n> agents; [limits] max_members is <m>; nothing started`.
- It applies to the repository loadout only. A user loadout is the user's own.
- R§6.2 also caps a file at 32 members, so the parse is bounded whatever the user config says.

### 9.5 The pool

**(operator)** The pool path is user config, not repository content: it is a path on one machine and it leads to credentials. The repository file can only say that a member takes its config directory from the pool. Assignment is round-robin over the pool in name order, stable per member name across restarts. With fewer directories than members, directories are shared and `zygo doctor` reports it. With no pool, each member uses the user's default config directory. zygo does not read the pool's credentials.

**(author)** User config (W§11 `[workspace]`, user config only, AU22):

```toml
[workspace]
pool = "~/.claude-interns"   # optional; `~` expanded as --config-dir does; must be absolute after that
pool_passthrough = false     # optional; true: each pool member also starts with --passthrough (§12)
```

- **Directories.** Each entry of the pool whose name does not start with `.` and that is a directory (links followed: the pool is the user's own), in byte order of name. zygo reads names and types only; it opens no file in them (R-C6).
- **Assignment.** At a fresh build only (W§8.1 step 7, a loadout plan). The pool members, in byte order of name (panes with no name after them, in pane order), get the directories in turn: member `i` gets directory `i mod k`. The path becomes the pane's `config_dir`, so the existing path sets `CLAUDE_CONFIG_DIR` (`crates/zygo/src/cmd/claude.rs:37-39`, `:66-90`) and the launcher callback saves it in `member.launch.config_dir` and `member.config_dir` (AU92).
- **Stable across restarts.** A resume never assigns again: `ws run` sets the saved `member.config_dir` (AU61), because a resume with another config directory finds no conversation (PW16, failed sub-check 16.2 on 2.1.330). An `up --reset` with the same members and the same directories gives the same assignment.
- **Why `pool_passthrough`.** An account is used only in passthrough: outside it, Claude Code sends the session token through zygo's key helper (§8.5, §12.3). Without this key, a pool would separate history and settings but not accounts, and R-G3 would fail. The key is user config, because passthrough sends the login of the directory to the vendor (§12.1) **(author)**. Passthrough is local mode only (§12.1); in serve mode a pool member's launch is refused by the existing rule.
- **No pool.** Pool members start with no `config_dir`, so they use the inherited `CLAUDE_CONFIG_DIR` or the default directory **(operator)**.
- **A pool that does not work.** `pool` set but relative after the `~` expansion (also an error of `zygo config check`), not a directory, not readable, or with no subdirectory: `zygo up` exits 1 before any write, `zygo: [workspace] pool <path>: <reason>; nothing started`. Reason: a silent fall back would move each member to the default account.
- **Shared directories.** With `k` directories and `n > k` pool members, `zygo doctor` prints one line per shared directory, `pool: <dir> is shared by <name>, <name>`.
- **OpenCode.** No pool: R§6.2 and `config check` refuse `pool` on an `opencode` pane. Reason: OpenCode sends its model requests to the proxy with the session token (§8.7), so its own login is never used, and zygo already gives each session its own `OPENCODE_CONFIG_DIR` (§8.7). A pool would only move OpenCode's data folders, which hold its sessions, and it would break resume (WA-1).
- **Launch (§8.4, §8.5).** The pool sets `CLAUDE_CONFIG_DIR` and nothing else. The per-session settings file is passed with `--settings`, the highest scope after managed settings, so zygo's keys win over the pool directory's own `settings.json` (§8.5). `--plugin-dir`, `--mcp-config` and `--agents` are per-session flags and do not depend on the config directory. The preflight reads `$CLAUDE_CONFIG_DIR/settings.json` (`crates/zygo-launch/src/claude_code.rs:565-569`), so a pool directory whose settings set a credential key or a provider switch stops that member's launch with exit 2 (§8.4), and the pane follows the existing failed-launch path (W§8.2). Plugins that the user installed in a pool directory load as they do from the default directory; the doctor check for a plugin that would load twice reads the default directory only (`crates/zygo/src/cmd/doctor.rs:266-292`) and is not run on pool directories (R-C6).
- **Claude Code version.** No new minimum. A pool directory is a `CLAUDE_CONFIG_DIR`. zygo's minimum is 2.1.328 (`crates/zygo-config/data/agents.toml:2-3`), on which the user settings file is `$CLAUDE_CONFIG_DIR/settings.json` (§8.4 note of N32) and a resume needs the same directory (PW16, 2.1.330). R§19 PR1 and PR2 verify the pool on the pinned version.

## 10. Entry points

**(author)** Each entry point that uses the definition does one read and one check (R§7.3). No other one reads `.zygo/`.

| Entry point | Behavior |
|---|---|
| `zygo up` (W§8.1 step 7, a loadout plan: `--reset`, no state, or no pane) | The loadout: `--loadout <l>`; else `[workspace] default_loadout` when the user config file sets it; else `repository` when the default branch has `.zygo/workspace.toml`; else `solo`. A `repository` plan is made after step 3, when the checkout is known, and before step 8. Chosen by the default and not approved, invalid or unread: plan `solo` and one line, `zygo: this repository's workspace files (<ref>, digest <12>) are not approved on this machine; nothing from them was applied; see zygo workspace approve` (or `… are invalid (<why>) …`). Chosen by `--loadout repository`: the same cases exit 1 with the same line and start nothing; absent: exit 1, `zygo: this repository has no workspace files at <ref>`. Then R§9.4 and R§9.5, both before any write. A user `[loadout.repository]` is the user's loadout; the repository one is then not used (user first, as for profiles). |
| `zygo up` (a restore), `zygo up --all` | Reads nothing (AU10: a later `up` restores the snapshot). |
| Repair of a dead build (AU67) whose `loadout` is `repository` | Reads nothing. AU67 works from `panes[]` and the member records that step 8 wrote from the checked read; it does not resolve the loadout again. The state keeps no copy of the files. |
| `zygo ws run` (start or resume of one pane) | Reads nothing. The saved launch holds the profile name, the job and the config directory. The profile name resolves at launch as any `--profile` (R§8): an approved repository profile is used; else the R§8 line. |
| `zygo claude`, `zygo opencode` | R§8: a read only when the requested profile is not in user or team config. |
| `zygo config show [--layer repository]` | In a repository: the approved repository profiles and `loadout.repository`, each key with the source `repository` and its file, `<key> = <value>  # repository .zygo/profiles/rust.toml` (§6.1, `layers.rs:752-782`). `--layer repository` shows them with the `.zygo.toml` keys. Not approved or invalid: nothing from the files, one stderr line. |
| `zygo config check` | Validates the definition by R§6.4, approved or not. A violation is one warning line; the exit status does not change, as for an invalid `.zygo.toml` (§13.5). |
| `zygo profile list`, `zygo profile show <n>` | Approved repository profiles with the source `(repository)`, or `(repository, shadowed)`. Not approved: not listed, one line. |
| `zygo loadout list`, `zygo loadout show repository` | `repository (repository)`, `(repository, not approved)`, `(repository, invalid)` or `(repository, shadowed)`. `show` prints the loadout only when approved. |
| `zygo doctor` | `workspace files: <ref> <commit 12> digest <12>: approved`, or `not approved`, `invalid (<why>)`, `none`, `unread (<why>)`; `pool: <path>: <k> directories` or `pool: <path>: <reason>`; the R§9.5 shared lines for the loadout `up` would choose; `workspace files: <n> agents; [limits] max_members is <m>` when above. |
| `zygo workspace approve` | R§7.1. |
| Hooks, the MCP server, the status line | Read nothing (R-C8). |

Two sessions at once: two `zygo workspace approve` runs take the approvals lock in turn, and an entry already present is not added twice. An `up` that runs during an approval uses its own read (R§7.3). Two `up` runs for one workspace are serialized by `op.lock` (W§8.1 step 4).

## 11. Failure handling

| Case | Behavior |
|---|---|
| A git call fails or passes the 2 s budget | Unread: counts as absent, one stderr line naming the call. `approve` exits 1. |
| The default branch gets a new commit that changes `.zygo/` | New digest: not approved until `zygo workspace approve`. A running workspace keeps running; a restore reads nothing; a repository profile at the next launch gives the R§8 line. |
| An agent edits `.zygo/` in a worktree or on a work branch | No effect (R-C4). |
| An agent moves the default branch locally (`git update-ref`) or pushes to it | New digest; nothing applied without an approval (R-C2). |
| Approvals file invalid | Nothing is approved; `approve` exits 1 and does not change the file. |
| Approval write fails | Exit 1; the temp file is removed; nothing is stored. |
| The pool directory is gone at a resume | The existing `--config-dir` rule (`claude.rs:80-82`): the launch fails and the pane opens a shell with the W§8.2 line. |
| A pool directory's settings set a credential key | That member's launch exits 2 (§8.4); the other members start. |
| `up` crashes during a repository build | AU67 repair from the saved records; no new read (R§10 row "Repair"). |

## 12. Owners of state

| State | Owner (only writer) | Readers |
|---|---|---|
| `.zygo/` files | The repository's authors, through git. zygo never writes them. | R§10 entry points. |
| `[[workspace]]` entries of `approvals.toml` | `zygo workspace approve`. | R§7.3 checks. |
| `[workspace] pool`, `pool_passthrough`, `[limits] max_members` | The user, in user config (`max_members` also team config). | `zygo up`, `zygo doctor`. |
| The pool assignment | `zygo up` at a fresh build, saved through `member.launch.config_dir` and `member.config_dir` by the launcher callback (AU92). | `ws run` at resume (AU61). |
| `member.launch.job` | `zygo up` (from the loadout pane), the launcher callback (AU92). | The session-start hook (R§9.3), `ws run`. |

## 13. Configuration

User config, the additions of this spec:

```toml
[workspace]
pool = "~/.claude-interns"
pool_passthrough = false

[limits]
max_members = 8

[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
name = "elena"          # M§6
job = "worker"          # R§9.3
pool = true             # R§9.5; not with config_dir
```

## 14. Base-spec changes

| § | Change |
|---|---|
| W-C5 | New text **(operator)**: "A cloned repository cannot start an agent through a workspace until the operator approves its workspace files on this machine (R§7). Loadouts come from user config, and from `.zygo/workspace.toml` after that approval. A repository loadout never starts a shell pane and never sets agent arguments (R-C3)." |
| W AU10 | Loadouts live in user config, `[loadout.<name>]`, and, after an approval, in `.zygo/workspace.toml` as the loadout `repository` (R§9.1). `.zygo.toml` still cannot set `[loadout]` or `[workspace]`. A later `up` restores the last snapshot; `up --reset` applies the loadout again. |
| W AU22 | Unchanged for layers: only the built-in and user layers set `[loadout]` and `[workspace]`, and the team layer drops them. The repository loadout is not a layer; it is added after an approval (R§9.1). The profile layer still merges only `features`; a repository profile has none (R§8). |
| W§14 row "Loadouts in `.zygo.toml`" | Still rejected for `.zygo.toml`, with the reason: `.zygo.toml` is applied with no approval (§13.5). A repository loadout comes from `.zygo/workspace.toml`, behind the approval of R§7 **(operator)**. |
| W§3, W§14 row "Messages, wake, … pools, slots …" | The config-directory pool returns (R§9.5) **(operator)**. Slots stay a non-goal. |
| W§11 | Pane keys `job` and `pool` (and `name`, M§6); `[workspace] pool`, `pool_passthrough`; the `config check` rules of R§9.1. |
| W§7.2 | `member.launch.job`. |
| W§8.1 step 7 | The loadout choice of R§10; the R§9.4 and R§9.5 checks before step 8. |
| W§12 | Row `zygo workspace approve [--yes] [--digest <hex>]`: R§7.1. Not subject to OP5. |
| §6.1 layer 3 | `[workspace] pool`, `pool_passthrough`; `[limits] max_members`. |
| §6.1 layer 4, §13.5 | `.zygo.toml` cannot set `limits.max_members` (R§9.4). Its four tables do not change. |
| §6.1 | The layer order does not change **(operator)**. `zygo config show` names `.zygo/` files as the source of the keys they add (R§10). |
| §11.1 | Profiles come from user config, team config, and, after an approval, the repository's `.zygo/profiles/` with `overrides` and `disabled` only (R§8) **(operator: the source; author: the parts)**. |
| §11.2 | Order of sources for one name: user, team preset, repository (R§8). |
| §11.3 | A repository profile gets the same rules; the shape rules of R§6.3 apply at read. |
| §15.7 | `approvals.toml` also holds `[[workspace]]` entries (R§7.2). |
| §17 | Row `zygo workspace approve`. |
| M-C6 | New text: "A cloned repository cannot send mail. Member names come from user config, or from approved repository files (R§7)." |
| M§8a item 8 | The member list shows each job; one job sentence (R§9.3). |

## 15. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| Allow all but secrets (a deny list) | A key added later is allowed by default; a provider endpoint could send each request to a host that the repository author controls **(operator)**. |
| Read the files from the working tree | An agent or a work branch could change the workspace it runs in **(operator)**. |
| Ask at `zygo up` (trust on first use) | A launch never prompts (§15.7) **(operator)**. |
| A new config layer for the repository files | §6.1 holds **(operator)**; a profile source and a loadout source need no layer. |
| Approval by digest alone | A hostile repository could copy a definition approved for another repository. |
| Approval per file | A partial approval gives a definition that no one saw as a whole. |
| Profiles with prompts, custom roles, plugins or features | R§8: no use that `CLAUDE.md` does not give, plugin code, or a weaker guard. |
| Key-by-key merge of a repository profile with a user or team profile of the same name | A mix that no source defined. |
| `[[members]]` as an array | Two members could have one name, and the pane order would depend on the file order. |
| The "lower value" rule for `max_members` from `.zygo.toml` | No use for a lower value from the repository; one rule is simpler (R§9.4). |
| Pool assignment by a hash of the name | Two members can get one directory while others are free. |
| A pool for OpenCode through its data folders | Moves its sessions and breaks resume; its login is never used (R§9.5). |
| Fall back to the default config directory when the pool fails | Moves members to the default account with no notice. |
| Read `.zygo/` in `load` | Each hook would make more git calls (R-C8). |
| A raw tmux layout string or a window name from the repository | Repository text in tmux arguments (W-C8); the five presets are enough. |

## 16. Files expected to change

- `crates/zygo-config/src/repo_def.rs` (new): the read (R§6.1), the typed shapes and validation (R§6.2-6.4), the digest (R§6.5), the approval store read and write (R§7.2), `Merged::with_definition` (R§8, R§9.1).
- `crates/zygo-config/src/schema.rs`: `LoadoutPane` (`:722-741`) gets `name`, `job`, `pool`; `WorkspaceConfig` (`:685-698`) gets `pool`, `pool_passthrough`; `LimitsConfig` (`:599-602`) gets `max_members`.
- `crates/zygo-config/src/layers.rs`: the `.zygo.toml` filter drops `limits.max_members` (next to `:505-509`); `show` (`:752-782`) prints the file of a repository-definition key.
- `crates/zygo-config/src/check.rs`: the R§9.1 checks.
- `crates/zygo-workspace/src/loadout.rs`: `Source::Repository` (`:17-22`), the pool assignment, `job` and `pool` in `PlanPane` (`:35-46`).
- `crates/zygo-workspace/src/up.rs`: the step 7 choice (next to `:713`), the R§9.4 and R§9.5 checks.
- `crates/zygo-workspace/src/state.rs`: `Launch.job` (`:148-170`).
- `crates/zygo/src/cmd/workspace.rs`: the `approve` subverb.
- `crates/zygo/src/cmd/doctor.rs`, `config.rs`, `profile.rs`, `loadout.rs`: the R§10 lines.
- `crates/zygo-launch/src/launcher.rs`: the R§8 read and lines (next to `:326-343`).
- The session-start text of M§8a, item 8 (the hook file that P3-1 adds).
- The user docs that describe `.zygo.toml`, profiles and loadouts.

## 17. Left to the builder

- The exact git commands of R§6.1 steps 1-5, within its rules (one commit id, no short names, sizes before content, one budget).
- Type and function names, and the module split, within R§16.
- The text of lines this spec does not fix, within R-C7.
- The place of the new `zygo doctor` lines in its output.

## 18. Acceptance

Each check is a test in temp directories with temp git repositories (a bare "origin" and a clone, so `refs/remotes/origin/HEAD` exists), stub agents and a stub tmux where the existing workspace tests use one. No real agent, no real agent config directory, no wall-clock assertion.

1. **Default branch only.** With `.zygo/` committed on the default branch: an edit in the working tree, a commit on another branch, and an edit in a linked worktree each leave the digest the same. A new commit on the default branch that changes a file changes the digest.
2. **Tree entries.** `.zygo` as a link, as a submodule and as a file; `workspace.toml` as a link; `profiles/x.toml` as a link and as a tree: each read is invalid with the R§6.1 reason, and the fake `git` log shows no content read.
3. **Sizes.** `workspace.toml` of 64 KiB + 1 byte, a profile of 16 KiB + 1 byte, and 33 profile files: each read is invalid before any content read (fake `git` log).
4. **Allow list.** Each of `members.a.args`, `members.a.config_dir`, `members.a.passthrough`, `members.a.yolo`, `members.a.server`, `members.a.compact`, `kind = "shell"`, a top-level `[[model]]`, `[redaction]`, `[update]`, `[workspace]`, an unknown top-level key, `version = 2`, two orchestrators, 33 members, `pool = true` on `opencode`, `cwd = "/tmp"`, `cwd = "../x"`, `cwd = "~/x"`, a profile `prompt`, `roles`, `plugins`, `features`, an empty profile, and a profile file named `None.toml` or `none.toml` makes the definition invalid with the R§6.4 path, `approve` exits 1, and `up` applies nothing.
5. **Approve.** Answer `y`: one `[[workspace]]` entry with the id and digest. Answer `n` or end of input: none, exit 0, `not approved` line. `--yes`: an entry, listing on stdout. `--digest` of another value: none, exit 1. `ZYGO_SESSION_DIR` set: none, exit 1. Slash form: the command text, none. A second approve: `already approved`, the file unchanged.
6. **Race.** Approve digest D1; commit a change on the default branch; `up` plans `solo` and prints the not-approved line. A unit test with a counting reader shows that `up` and the launcher read the definition once per use, and that the applied plan comes from the read whose digest was checked.
7. **Escaping.** A `workspace.toml` comment with `ESC [ 2 J`, a carriage return and U+202E: the approve listing holds none of these characters raw.
8. **Approval key.** The same `.zygo/` content committed in a second repository is not approved there.
9. **Profiles.** After approval, `zygo claude --profile rust` (stub agent) sends the overrides of `.zygo/profiles/rust.toml` in the session request. A user profile `rust` is used in its place, and `zygo profile list` shows `rust (repository, shadowed)`. Not approved: the R§8 line, no profile. An override to a model not in the catalog: the §11.3 line. A launch without `--profile` makes no `ls-tree` call (fake `git` log).
10. **Default set.** An approved `workspace.toml` with `version = 1` only: the plan has one window, layout `main-vertical`, five `claude-code` panes `orchestrator`, `worker-1` to `worker-4`, in that order, each `pool = true`. Profile files without `workspace.toml`: `up` plans `solo` and prints no line.
11. **Choice.** `--loadout solo` and a user `default_loadout` each win over an approved repository loadout. `--loadout repository` with no `.zygo/`: exit 1, no state file written.
12. **Maximum.** `[limits] max_members = 4` and the default set: exit 1, no state file written. `.zygo.toml` with `[limits] max_members = 30`: the ignored line, and the effective value stays the user value.
13. **Pool.** A pool with directories `a` and `b` and the default set: the member records hold `orchestrator → a`, `worker-1 → b`, `worker-2 → a`, `worker-3 → b`, `worker-4 → a`; `zygo doctor` prints the shared lines for `a` and `b`. After a directory `0` is added, a restore keeps each saved directory. A pool path that is missing, a file, or holds no directory: exit 1, no state file written. No pool: no `CLAUDE_CONFIG_DIR` from zygo in the stub agent's environment. `pool_passthrough = true`: the pool members' `LaunchOpts` have `passthrough`. A pool directory with mode-`000` files does not change the plan or the output of `up`.
14. **Preflight.** A pool directory whose `settings.json` sets `ANTHROPIC_BASE_URL`: that member's stub launch exits 2 naming the file; the other members start.
15. **Session text.** With mail on, a worker's session text holds `Your job: worker.` and the orchestrator's name; with mail off, no job sentence.
16. **Hooks.** The trim, prompt and session-start hooks in a repository with `.zygo/` start no `git` child that lists `.zygo` (fake `git` log).
17. **Shared approvals file.** With `[[workspace]]` entries in `approvals.toml`, `is_approved_in` still finds each plugin approval, and `zygo plugins approve` keeps the workspace entries byte for byte.
18. **Config surfaces.** `zygo config show` prints `# repository .zygo/profiles/rust.toml` for the profile keys; not approved: no such line and one stderr line. `zygo config check` with an invalid definition prints the warning and keeps its exit status.
19. **Windows.** On native Windows, `zygo workspace approve` runs, and an approved repository profile applies to `zygo claude --profile`.

## 19. Premises the first build verifies

- **PR1** (live check by the operator, recorded in `docs/live-checks.md`; never in CI, never by an agent: owner login risk) On the pinned Claude Code, two `zygo claude --passthrough` launches with two pool directories, each logged in to another account, send each directory's own login. If it fails: `pool_passthrough` is removed and the docs say a pool separates settings and history only.
- **PR2** (live check by the operator) With `CLAUDE_CONFIG_DIR` set to a pool directory, zygo's `--settings` hooks, the status line and `--plugin-dir` act as with the default directory.
- **PR3** (CI) `toml` 1.2.1 (`Cargo.toml:66`) ends a parse of 64 KiB of nested arrays and nested inline tables with an error or a value, and no stack overflow.
