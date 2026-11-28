# zygo: workspace definition — design

**Status:** approved by the operator 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track D (roadmap items P3-11 and P3-12, `docs/roadmap/phase-3.md`). P3-13 has its own spec. P3-12 needs the member `name` and the session-text item of workspace mail (P3-1, M§6, M§8a) and is built after it.
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`, constraints `Cn`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, constraints `W-Cn`, decisions `AUn`, premises `PWn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`, constraints `M-Cn`). `R§n` is a section of this document. The governing spec of the build is `docs/specs/2026-11-24-decision-records-design.md`.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's P3-11 and P3-12 design notes, their two "Resolved with the operator (2026-11-23)" lists, the plan of work of 2026-11-24, and the pool ruling of 2026-11-24 in R§9.5). **(lead)**: the lead of this design work ruled it after an independent audit of the first draft (2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767`.

---

## 1. Intent

Phase 2 reads loadouts from user config only (W-C5, AU10). Each person who works on a repository writes the same loadout and the same model profiles by hand, on each machine.

The operator wants a repository to define its workspace in files: the config, the model profiles and the members **(operator, 2026-11-23)**. Each machine and each person then starts the same workspace, after the operator approves the files on that machine. A repository file is untrusted until that approval: it can come from a clone, a fetch, or an edit by an agent.

## 2. Goals

- **R-G1** A repository defines its members (name, agent kind, job, profile) and its model profiles in files under `.zygo/`. `zygo up` starts that workspace on each machine where the operator approved the files.
- **R-G2** A clone, a fetch, a push or an agent edit cannot start an agent, select a model or reach a credential through these files without an approval on that machine.
- **R-G3** Members can run in passthrough on different agent accounts: each takes a Claude Code config directory from a pool that the user configures.
- **R-G4** `zygo config show`, `zygo doctor`, `zygo profile list` and `zygo loadout list` show what comes from the repository files and whether they are approved.

## 3. Non-goals

- A change to `.zygo.toml`: it keeps its four tables and its read (§6.1 layer 4, §13.5) **(operator)**. One key is filtered out of its `[limits]` (R§9.4).
- Repository content other than the allow lists of R§6.2 and R§6.3: credentials, providers, endpoints, redaction, update settings, prompts, custom roles, plugins, feature keys, agent arguments, shell panes **(operator; author for prompts, custom roles, plugins and features, R§8)**.
- A verb that removes an approval, and approvals shared between machines **(author)**.
- Slots or scheduling of agents. Only the config-directory pool returns from W§3 (R§14).
- A pool for OpenCode (R§9.5).
- Member identity across machines (P3-3) and session run time (P3-13).
- A change to a running or resumed member when the files change. A new digest applies at the next fresh `zygo up` after its approval (R§7.4) **(lead)**.

## 4. Constraints

- **R-C1** C1-C10, W-C1 to W-C9 (W-C5 as amended in R§14) and M-C1 to M-C6 (M-C6 as amended in R§14) apply.
- **R-C2** zygo applies nothing from `.zygo/` on a machine until the operator approved its digest on that machine. The approval is the security boundary. The default-branch rule is a review boundary only; §13.5 holds **(operator)**.
- **R-C3** The repository files can set only the keys of R§6.2 and R§6.3. Any other key, at any level, makes the whole definition invalid. A repository file never sets a credential, a provider, an endpoint or base URL, a redaction setting, an update setting, `config_dir`, `args`, `yolo`, `passthrough`, `server` or a shell pane **(operator)**.
- **R-C4** zygo reads the files from the git objects of one commit. It never reads them from the working tree or the index, and it never writes a file from them.
- **R-C5** A launch never prompts (§15.7). Only `zygo workspace approve` asks; `--yes` approves in scripts **(operator)**.
- **R-C6** zygo does not read, copy or log the files of a pool directory. It sets the config directory of the child only. The §8.4 preflight still reads the `settings.json` of that directory, as for any config directory **(operator)**.
- **R-C7** Each repository text that zygo prints on one line passes `zygo_core::esc` (`crates/zygo-core/src/text.rs:4-16`: control characters and bidi controls escaped). The approve listing escapes more (R§7.1 step 6). A TOML error is printed only as its message and position, never with the quoted source line (the rule of `crates/zygo-config/src/repo_file.rs:28-37`).
- **R-C8** No hook reads `.zygo/`. The read runs only in the entry points of R§10.
- **R-C9** Tests never run a real `claude` or `opencode` binary and never use a real agent config directory: stub agents, temp directories and temp git repositories only. No test asserts a wall-clock time.

## 5. Terms

| Term | Meaning |
|---|---|
| Workspace files | `.zygo/workspace.toml` and the files `.zygo/profiles/<name>.toml`. |
| Definition | The workspace files of one read commit, parsed. It can hold a workspace file, profile files, or both. |
| Read commit | The commit id that one read uses for each of its git calls (R§6.1). |
| Digest | SHA-256 of the workspace files of one read (R§6.5). |
| Pin | A read commit and its digest, saved in a member record (R§7.4). |
| Repository loadout | The loadout named `repository` that zygo makes from `.zygo/workspace.toml` (R§9.1). |
| Member | The W§5 member: the agent slot of one pane, with the optional name of M§6. |
| Job | `orchestrator` or `worker`. An orchestrator gives work to the other members and collects their results through workspace mail (M§1). A worker is an elephant: a lead session (§5) that holds context and delegates to `zygo-<role>` subagents. Each `zygo claude` member is already a lead session, so the two differ in their job only, not in how zygo starts them **(operator)**. |
| Pool | A directory in user config whose subdirectories are Claude Code config directories, for example `~/.claude-interns/<name>` **(operator)**. |
| Pool member | A `claude-code` loadout pane with `pool = true`, when the user config defines a pool. It runs in passthrough (R§9.5). |

The word "team" keeps its one meaning, the team config of serve mode (§6.1 layer 2) **(operator)**.

## 6. The files (P3-11)

### 6.1 Location and read

**(operator)** A `.zygo/` directory at the repository root holds `.zygo/workspace.toml` and one file per profile under `.zygo/profiles/`. It is read from the default branch, as `.zygo.toml` is (§13.5), so a work branch cannot change the workspace it runs in.

**(author, lead for step 2)** A read of the default branch is these steps, under one 2 s budget (as `GIT_BOUND`, `crates/zygo-config/src/repo_file.rs:40`, `:244-246`), each git call made with `git_command` (`repo_file.rs:80-99`: the caller's `GIT_*` repository variables and every `GIT_CONFIG*` variable removed, `LC_ALL=C`). A pinned read (R§7.4) skips step 2 and uses the saved commit id.

1. Find the git common dir and compute the workspace id from it (`crates/zygo-workspace/src/identity.rs:17`, W§5). The approval is keyed by this id. The id is always computed from what git gives at the read's checkout, never taken from a saved value. A member launch reads at the checkout of its workspace's main worktree (R§7.4), every other read at the checkout of the current folder.
2. Find the default branch as a full ref name. Read the target of `refs/remotes/origin/HEAD` in its full form. The target must start with `refs/remotes/origin/`; a target outside it makes the read **unread** (`origin/HEAD points outside refs/remotes/origin/`). When `refs/remotes/origin/HEAD` does not exist, the ref is `refs/heads/main`. Then resolve the ref to one commit id, the read commit. Each later call names that commit id.
3. List the tree entries of the read commit at `.zygo`, `.zygo/workspace.toml` and `.zygo/profiles`, with mode, type, object id and size.
4. Check the entries before any content is read:
   - `.zygo` absent: the definition is **absent**. `.zygo` that is not a tree (a link, mode `120000`; a submodule, mode `160000`; a file): **invalid**, `.zygo is not a directory`.
   - `.zygo/workspace.toml`: absent, or a regular file (mode `100644` or `100755`). Any other mode: invalid.
   - `.zygo/profiles`: absent, or a tree. Each entry in it whose name ends in `.toml` must be a regular file whose name matches `^[a-z0-9-]{1,32}\.toml$`, with a stem other than `none` (§11.3). Else invalid. Entries whose names do not end in `.toml`, and entries of `.zygo/` other than these two, are not read and are not in the digest; `zygo workspace approve` lists them as ignored.
   - Sizes, from the listing: `.zygo/workspace.toml` at most 64 KiB; each profile file at most 16 KiB; at most 32 profile files. Above a limit: invalid, with the path and the limit.
   - The listings: each listing of step 3 (`.zygo`, `.zygo/`, `.zygo/profiles/`) has at most 256 entries, and zygo keeps at most 131072 bytes (128 KiB) of the output of each git call of the read. When a call gives more, zygo stops the read of its output there and stops the call. A listing above a limit: invalid, `<dir> lists more than 256 entries` or `<dir>: the listing is larger than 131072 bytes`, where `<dir>` is `.zygo` or `.zygo/profiles`. Any other call above the byte limit: unread, `git <call>: output larger than 131072 bytes`. Both limits are far above a valid read (at most 32 profile files; no file larger than 64 KiB), so they refuse only a tree made to fill memory.
   - No workspace file and no profile file: absent.
5. Read each file by the object id of its listing entry. The byte count must equal the listed size, and the bytes must be UTF-8. Else invalid.

Results: **absent**; **unread** (a git call failed or passed the budget, or step 2 refused the target: counts as absent, with one stderr line, as the §13.5 note of 2026-11-19); **invalid** (the first violation, R§6.4); **found** (the full ref, the read commit, the files, the digest, the parsed definition).

**Difference from the `.zygo.toml` read.** The `.zygo.toml` code reads the default branch by short names: `symbolic-ref --short refs/remotes/origin/HEAD`, else `main` (`repo_file.rs:247-249`). git resolves a short name `main` through `refs/tags/main` before `refs/heads/main`, so a tag named `main` wins over the branch, and a short name does not show where the target is. Step 2 uses full names for these reasons. `.zygo.toml` is not changed (R§3); its read is a review boundary only, and the two reads can therefore name different commits when a tag `main` exists.

Hostile input:

- **Path traversal.** zygo builds no file path from a tree entry name, and the profile name pattern holds no `/`, `.` prefix or `..`. Nothing from the files is written to disk.
- **Links.** The working tree is never read (R-C4), so a link in a checkout has no effect. A link inside the git tree is refused by its mode in step 4.
- **Oversized files.** The sizes are checked from the listing, before any content read.
- **Oversized listings.** A `.zygo/` tree with very many entries, or with very long entry names, is refused by the listing limits of step 4; zygo never holds more than 131072 bytes of the output of one git call of the read.
- **A branch that moves.** Step 2 pins one commit for the whole read.
- **A ref name that starts with `-`.** Each call names the commit id or a full ref name (`refs/…`), never a short name, so no name is read as an option.
- **A moved `origin/HEAD`.** Known limit: an agent that can run git can point `origin/HEAD` at another `refs/remotes/origin/` branch, or move such a ref. It can so choose which approved digest applies at the next fresh `up`; it can never apply a digest that is not approved (R-C2).

### 6.2 `.zygo/workspace.toml`

**(operator)** The file is a loadout (W§11), not a new format. The table is `[members]`. Each member has a name (M§6), a `kind` and a profile, and one new key, `job`. `kind` keeps its W§11 meaning, the agent kind. Not allowed: `config_dir`, `args`, `yolo`, `passthrough`, `server` and a shell pane. **(author)** The allow list:

```toml
version = 1                  # required; the integer 1
layout = "main-vertical"     # optional; even-horizontal | even-vertical | main-horizontal | main-vertical | tiled

[members.orchestrator]       # the key is the member name, M§6: ^[a-z][a-z0-9-]{0,31}$; not `all`, `cli` or `zygo`, not of the pane-key form ^k[a-z2-7]{8}$
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
| `members.<name>.job` | string | `orchestrator` or `worker`. Absent: `worker`. |
| `members.<name>.profile` | string | 1-32 of `[a-z0-9-]`, not `none`. A name that no source defines at launch gives the existing line and a launch without it (§11.3, R§8). |
| `members.<name>.pool` | boolean | `true` only with `kind = "claude-code"`. When the user config defines a pool, the member takes a pool directory and runs in passthrough; with no pool, no effect (R§9.5). |
| `members.<name>.cwd` | string | Printable ASCII only (bytes `0x20`-`0x7e`); any other byte is refused **(lead: ruling 5; author: refuse, not escape)**. `branch:<name>`, with 1-255 bytes after the prefix; or a relative path: parts split by `/`, each part 1-255 bytes, not `.` or `..`, not `.git` in any letter case (ASCII), without `\`, at most 1024 bytes in all. An absolute path and a path that starts with `~` are refused. |

Why a table keyed by name and not an array: TOML refuses a key given twice, so two members cannot have one name, and the order is the order of the names, not the order in the file. Why `cwd` refuses non-ASCII: a path is the one repository value that names a place on the machine; a look-alike character would show one folder and start in another. **(author: FX-39)** Why `cwd` refuses a `.git` part: the git dir is never a work folder; an agent started there works next to the hooks and the config of the repository; and a file system that ignores letter case finds the git dir also by `.GIT`. Names that only start with `.git` (`.github`, `.gitignore`) are valid. The refusal is one of two guards: a committed link (`w -> .git`) passes this text check, so the launch also refuses a pane folder that resolves into a git dir (R§7.4 "The pane folder"); and the R§7.4 approval check does not depend on the pane folder.

### 6.3 `.zygo/profiles/<name>.toml`

**(author)** One file holds one profile. Its name is the file stem. The allow list:

```toml
overrides = { review = "gpt-6-sol", code = "claude-sonnet-5-5" }   # role → catalog model id
disabled = ["design"]                                               # bundle roles hidden from the lead
```

| Key | Rule |
|---|---|
| `overrides` | At most 16 entries. Each key 1-32 of `[a-z0-9-]`; each value 1-128 bytes of printable ASCII. |
| `disabled` | At most 16 entries, each 1-32 of `[a-z0-9-]`. |

At least one of the two is not empty (§11.3: a profile with no part set is invalid). That a key is a bundle role and that a model is in the catalog and servable are checked at launch, by the §11.3 validation of the proxy that will serve, because the catalog differs per machine and in serve mode (R§8).

### 6.4 Validation

**(author)** Each file is parsed into a typed shape that refuses any unknown key at every level. The first violation wins, in the order: the workspace file, then the profile files in byte order of name. A violation is reported as `<path>: <key path>: <reason>`, with the key path escaped (R-C7). Examples: `.zygo/workspace.toml: members.a.args: not allowed in a repository file`; `.zygo/profiles/rust.toml: prompt: not allowed in a repository file`; `.zygo/workspace.toml: TOML parse error at line 3, column 1: <message>`. A key from the never-allowed set of R-C3 gets the same "not allowed" reason as any unknown key, so no second list is kept.

One violation in one file makes the whole definition invalid. A partial definition is never approved and never applied. Reason: the digest covers all files (R§6.5), so the operator approves one whole thing.

### 6.5 Digest

**(author)** The digest is SHA-256 over, for each file in the order of R§6.4: the path bytes (`.zygo/workspace.toml`, `.zygo/profiles/<name>.toml`), one `0x00` byte, the byte count in decimal ASCII, one `0x00` byte, the file bytes. It is written as 64 lower-case hex characters. A one-line message that names it `<12>` (R§7.4, R§8, R§10) shows its first 12 characters; the lines of R§7.1 name it `<digest>` and show all 64, so a script can pass what it read to `--digest`.

The bytes come from git objects, so the line endings of a checkout do not change the digest (unlike §15.7, no CRLF rule is needed).

## 7. Approval

### 7.1 `zygo workspace approve [--yes] [--digest <hex>]`

**(operator)** The approval is a named verb. It shows the files and asks. `--yes` approves in scripts. The approval is for a content digest and is stored as plugin approvals are (§15.7). A new digest needs a new approval. **(author)** The steps:

1. Runs on each platform, native Windows included: it starts nothing and needs no tmux. The OP5 check of `zygo workspace` (`crates/zygo/src/cmd/workspace.rs:12`) is not applied to this subverb, so repository profiles work where workspaces do not.
2. Inside an agent session (`ZYGO_SESSION_DIR` set): exit 1, `zygo: approval is not available inside an agent session; run it in your own shell: zygo workspace approve` (the rule of `crates/zygo/src/cmd/plugins.rs:133-136`). The slash form `/zygo:workspace approve` returns that command and approves nothing.
3. Find the checkout from the current folder; not a repository: exit 2. Do one read of the default branch (R§6.1). Absent: exit 1, `zygo: this repository has no workspace files at <full ref>`. Unread: exit 1 with the reason. Invalid: exit 1 with the violation.
4. With `--digest <hex>` (64 hex characters) that is not the digest of step 3: exit 1, `zygo: the workspace files changed: their digest is <digest>, not <hex>; nothing approved`. This check comes before step 5 and applies in every state, approved files included. Use: an operator reads the files, then approves in a script exactly what was read; a script learns that the files it read are not the files that apply.
5. Already approved: print `workspace files already approved (<digest>)`, exit 0.
6. Show:
   - the folder, the full ref (for example `refs/remotes/origin/main`), the full read commit id and the full digest;
   - each file, with its path and size, line by line. In this listing every character other than printable ASCII (`0x20`-`0x7e`) is shown as `\u{<hex>}`, newline ends a line, and tab is shown as `\t` **(lead: ruling 5)**. Reason: the operator reads the bytes that will be approved; a bidi control, a look-alike letter or a terminal sequence in a comment cannot hide or change a line;
   - the ignored entries;
   - a summary: each member (name, kind, job, profile, pool, cwd) with the defaults filled in, and the number of agents the files start; each profile with its overrides and disabled roles;
   - when the user config defines a pool: which members run in passthrough on it (R§9.5), and, for each of them that names a profile, that the profile does not apply.

   Without `--yes`, the listing and the question `approve these workspace files? [y/N]` go to stderr, and one line is read from stdin (`y` or `yes`, any case). With `--yes`, the listing goes to stdout and no question is asked.
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

Why keyed by the workspace id and not by the digest alone: a hostile repository could copy a definition that the operator approved for another repository and start its agents in itself. For the same reason the id of a lookup is computed by git at the checkout that is read (R§6.1 step 1), never read from the state file. Why one file: §15.7 stores approvals there, and the note asks for the same store.

**An approval stays (author, lead: ruling 4).** An entry is never removed by zygo. When the default branch returns to content that was approved before (a revert, or `origin/HEAD` moved back), that digest applies again with no new approval. Reason: the operator approved those exact bytes for this repository, and a second question about the same bytes adds no safety. To withdraw an approval, the user deletes the entry from `approvals.toml`; no verb does it (R§3). This is not an operator decision: the P3-11 note says only that a new digest needs a new approval.

### 7.3 Check at each use

**(author)** Each use (R§10) does its own read (R§6.1), computes the digest of the bytes it read, looks for `(workspace id, digest)` in the store, and applies the parsed bytes of that same read. It never checks one read and applies another. So a change between the approval and the use, or during the use, gives another digest, and nothing is applied: no race between show and approve, and none between check and apply.

A git object is the same on each machine, but each machine has its own approvals file and fetches at its own time. Two machines whose default branches point at different commits can have different digests; each approves what it has.

### 7.4 The pin

**(lead: ruling 1)** The members and their profiles come from one approved read, for the life of each member:

- A fresh `zygo up` with the repository loadout reads the default branch once (R§7.3) and gives each pane of that plan the pin: the read commit and the digest.
- Each member record saves its pin in `member.launch.definition = { commit, digest }` (W§7.2). The pin holds no workspace id and no path: the state file is the user's (W-C7), so the launch finds the repository and its id again by git (next bullets). For the panes of a repository-loadout plan, `zygo up` writes it into the fresh member record at the build (W§8.1 step 8), and each start write of the launcher callback (AU92) keeps it. A member that is not from the repository loadout gets a pin at its first start whose launcher read the default branch and applied a repository profile from that read (R§8): the launcher gives the pin of that read to the callback, which writes it with the other start fields. A saved pin is never replaced. A hand start that resumes a kept record (AU94) is a launch of that member and uses the pin of that record. Known limit: the hand start reads that pin before it takes the state lock, so a first pin that another start writes into the same kept record in that window leaves the record's pin different from the files the agent runs on until the next resume.
- Each later launch of that member (a resume, a `ws run` start, a restore by `up`) resolves a repository profile, for a profile name that neither the user nor the team config has (R§8), by a pinned read: R§6.1 steps 1 and 3-5 at the saved commit, with no default-branch lookup. The pinned read must give the saved digest, and `(workspace id, digest)` must be in the store. **(author: FX-39) The repository of a member launch.** Every definition step of a member launch (the approval check below, the pinned read, and the default-branch read of a member with no pin, R§8) runs at the checkout of the workspace's main worktree (`main_root` of the workspace record, W§7.2: the worktree whose checkout `zygo up` read, R§10), not at the checkout of the pane folder. The workspace id is computed there by R§6.1 step 1, from the common dir that git gives, never from a value in the state file. So the pane folder (a submodule, a nested repository, a linked worktree, a folder that is not in a checkout) plays no part in the check or in the read, and a member in a submodule or a nested repository gets the profile of its pin. When git finds no checkout at the main worktree (no work tree, a failed call, a call past its bound), every launch of a member that holds a pin stops before the session is made, with exit 1 and the R§7.4 line, in which `which were not read (git found no checkout at the main worktree of the workspace)` takes the place of `which this repository no longer has or no longer approves`: a check that cannot be made must not let the launch go on. A member with no pin then reads nothing and gets the R§8 not-found line for a repository-only profile, as a launch outside a checkout.
- **(author: FX-39) The pane folder.** Right after the approval check (next bullet) and before the passthrough rule and every profile step, a member launch whose record holds a pin stops with exit 1 when its pane folder, with every link resolved, is inside the common dir of the main worktree's checkout, or has more path parts named `.git` (in any ASCII letter case) than the resolved path of the main worktree has (a nested repository's git dir): `zygo: the folder of this member is inside a git dir (<path>); the agent was not started` (`<path>` escaped, R-C7). A folder that does not exist is left to the launch, which fails as before (W§8.2). Reason: R§6.2 refuses a `.git` part in the text of `cwd`, but a committed link (`w -> .git`) passes the text check; the git dir holds the hooks and the config of the repository, and an agent there can change what git runs. An approved pinned read that has no profile of that name gives the R§8 line `zygo: profile <n> not found; launching without it`, and the launch goes on.
- **(author: FX-31 review F1)** Each launch of a member whose record holds a pin first checks the approval of the pin, before the passthrough rule and before every profile step: when `(workspace id, digest)` of the pin is not in the store any more (the user withdrew it, R§7.2), the launch stops before the session is made, with exit 1, so the member record does not change (AU92: an exit before the callback writes nothing). This holds whatever profile the launch asks for, and in passthrough too. The check reads the approvals file, with the lookup of the pinned read (`is_workspace_approved_in`), under the workspace id of the main worktree's checkout (the bullet above); it reads nothing from the workspace files and needs only the checkout read of the main worktree. Line: `zygo: profile <n> of this member comes from the workspace files at commit <12> (digest <12>), which this repository no longer has or no longer approves; the agent was not started; to start from the current files: zygo down, then zygo up --reset`; when the launch does not make the pinned read (it asks for no profile, for a profile that a user profile or a team preset defines, or it is in passthrough), `zygo: this member comes from` takes the place of `zygo: profile <n> of this member comes from`. The name, the commit and the digest are each escaped (R-C7), because the commit and the digest come from the state file. The pane then follows the failed-launch path of W§8.2. Reason: the approval is the security boundary (R-C2, R-G2), a withdrawal is the user's only way to take an approval back (R§7.2), and ruling 1 says that the members come from one approved read; so no member of a withdrawn read starts.
- When the approval is still in the store, but the pinned commit or one of its objects is gone (a force-push and a `git gc`), or the pinned read does not give the saved digest, a launch that makes the pinned read stops the same way, with exit 1 and the same line. The launch never goes on without the profile, because that would change the member's models with no notice.
- A pinned read that is unread (a git call fails or passes the budget) also stops the launch, with exit 1 and the same line, in which `which were not read (<why>)` takes the place of `which this repository no longer has or no longer approves`; `<why>` is the R§6.1 reason, escaped. Reason: a launch without the profile changes the member's models.
- A launch in passthrough (a pool member, R§9.5) does not apply a profile (§12.3), so it does no pinned read, and a pin that is gone or unread does not stop it. A withdrawn approval stops it (the approval bullet above).
- **(author: FX-31)** A launch that asks for no profile (neither by `launch.profile` nor by `[user] default_profile`: `profile::requested`), or for a profile that a user profile or a team preset defines, also does no pinned read (R§8 "When the launcher reads"). So, while the approval of the pin is still in the store, a pin that is gone or unread does not stop it. Reason: such a launch applies nothing from the pinned commit (its job is in its member record and its folder in its pane, W§7.2), so a stop for a commit that it does not read adds no safety and only breaks its resume and its restore; the approval, which is the boundary, is still checked (the approval bullet).
- A new digest, approved or not, never changes a pinned member. It applies at the next fresh `zygo up` (`--reset`, or a workspace with no state) after its approval.

Why: a resume that read the default branch again would give a running conversation the profile of another approval, or none.

## 8. Profiles from the repository (P3-11)

**(operator)** A repository profile is one more profile source. The §6.1 layer order does not change. A user profile is used before a repository profile of the same name. A repository profile names roles and models from the user's catalog only; it cannot define a provider.

**(author)** The treatment in §11:

- **§11.1 shape.** The repository source holds `overrides` and `disabled` only (R§6.3). The operator's first pick, "roles and models only", stays. Not allowed: `prompt` and custom roles with prompts, because a repository already has a text channel to the agent (its `CLAUDE.md`), so a prompt key adds risk and no use; `plugins`, because a selection delivers plugin code; `features`, because §15.4 keys turn off the result store, `redaction.content` or work units.
- **Order of sources for one name.** User, then team preset, then repository. A repository profile whose name a user profile or a team preset already has is not used, and `zygo profile list` marks it `(workspace-files, shadowed)`. It is never merged key by key with another profile. Reason: §11.2 already lets the more personal source shadow by name, and the team presets of serve mode are curated; a repository profile only fills names that no config defines.
- **Not a layer.** The approved repository profiles are added to `config.profile` after the merge of the layers, by a new step (`Merged::with_definition`, `crates/zygo-config/src/layers.rs:713-747`), with the source `workspace-files` (R§10), which is not a value of `Layer` (`layers.rs:16-25`): `Layer::Repository` stays the name of `.zygo.toml`. `Merged::apply_profile` (`layers.rs:694-708`) is not changed: a repository profile has no `features`, so it adds no key to layer 5. `load` (`layers.rs:430-433`, which runs `load_with_builtin` and `load_with_repo_reader`, `:435-556`) does not read `.zygo/`: its one repository read is `.zygo.toml` (`read_default_branch`, `crates/zygo-config/src/repo_file.rs:250`). The hooks and the MCP server call it (`session_config`, `crates/zygo-work/src/hooks.rs:52-71`; `McpCtx::resolve_with`, `crates/zygo-mcp/src/tool.rs:60`), and R-C8 keeps their git calls as they are.
- **§11.2 selection.** By `--profile <name>`, by `[user] default_profile` (`zygo profile use <name>` can name a repository profile), or by a member's `profile` (R§6.2). The repository cannot set a default profile.
- **§11.3 validation.** The same rules, by the proxy that will serve: each override key is a bundle role; each model is in the catalog and servable on the agent's dialect; each disabled role is a bundle role, listed once, and not `main_role`. In serve mode the server's catalog decides (§6.1). An invalid profile: the existing line, and the launch goes on without the profile (§11.3; `crates/zygo-launch/src/launcher.rs:331-332`).
- **When the launcher reads.** Only when the requested profile name is in neither the user nor the team config (`launcher.rs:326-328`), so a launch with no repository profile makes no extra git call. A member with a pin reads its pin (R§7.4). Any other launch reads the default branch: a member at the checkout of its workspace's main worktree (R§7.4), a launch that is not a member at the checkout of its current folder. Lines of the default-branch read, each once, in place of `zygo: profile <n> not found; launching without it`:
  - not approved: `zygo: profile <n> is in this repository's workspace files, which are not approved on this machine (digest <12>); launching without it; see zygo workspace approve`;
  - invalid: `zygo: profile <n>: this repository's workspace files are invalid (<why>); launching without it`;
  - not read (R§6.1): `zygo: profile <n>: this repository's workspace files were not read (<why>); launching without it`.
- **Passthrough.** A launch in passthrough sends no role names (§12.3), so a profile, from any source, does not apply to it (R§9.5). A requested profile name gives `zygo: profile <n> does not apply in passthrough`, nothing is read, and the launch goes on.
- **§11.4 effects.** No change. The usage row records the profile name.

## 9. Members (P3-12)

### 9.1 From `[members]` to a loadout

**(author)** The `[members]` table is the source form. A W§11 loadout is the resolved form, and `zygo up` sees only the resolved form. zygo makes the repository loadout as one window:

- No window name (automatic rename; no repository text goes to `-n`, W-C8, AU55).
- Layout: `layout` when set; else `main-vertical` when there is an orchestrator, else `tiled`.
- Panes: the orchestrator first, then the other members in byte order of name. Each pane is one `[[loadout.repository.window.pane]]` entry with `name`, `kind`, `job`, `profile`, `pool` and `cwd` from the member, and every other W§11 field unset, so it takes the default of `zygo claude` or `zygo opencode` (AU45).

The W§11 pane `name` key is owned by P3-1 (M§6); this spec uses it and does not define it **(lead: ruling 7)**. This spec adds two pane keys, in user loadouts too: `job` and `pool`. `zygo config check` (`crates/zygo-config/src/check.rs`) adds five rules: `job` is `orchestrator` or `worker`; at most one orchestrator per loadout; `pool` only with `kind = "claude-code"`; `pool` and `config_dir` not both set; `job` only on an agent pane, `kind = "claude-code"` or `"opencode"` **(operator, 2026-11-24)**. **(author)** A pane with no `kind` is not an agent pane: a user loadout has no default kind, and `config check` refuses the missing `kind` as well. A `job` that the fifth rule refuses does not count toward the orchestrator limit, so a shell pane cannot take the orchestrator slot.

`cwd` resolves as W§11 does (`crates/zygo-workspace/src/loadout.rs:116-128`): `branch:<name>` to the worktree that has the branch checked out, else the main worktree with one warning; a relative path under the main worktree. R§6.2 refuses an absolute path, `~` and `..`, so a repository file cannot name a folder outside the repository by its text. A link inside the checkout can still lead out; that is accepted, because the checkout is the user's own and an agent can change its folder anyway.

### 9.2 The default set

**(operator)** When no member is defined: one orchestrator and four elephant workers. **(author)** This applies when `.zygo/workspace.toml` exists and its `[members]` is absent or empty. Without `.zygo/workspace.toml`, the repository defines no loadout and `zygo up` behaves as today. Reason: a repository with profile files only must not start five agents.

The default set: `orchestrator` (`job = "orchestrator"`) and `worker-1` to `worker-4` (`job = "worker"`), each `kind = "claude-code"`, `pool = true`, no profile, no `cwd`. With no pool configured, `pool = true` has no effect (R§9.5).

### 9.3 The job

**(operator)** The job key is `job = "orchestrator" | "worker"`. **(author)** A member of the repository file with no `job` is a worker (R§6.2). A pane of a user loadout with no `job`, and a hand start (AU9), have **no job** **(lead: ruling 9)**: the user did not give one, and zygo does not guess. The effects:

1. The pane order and the default layout of the repository loadout (R§9.1).
2. The member record keeps it: `member.launch.job` (W§7.2), set by `up` from the loadout pane (AU45), kept at each resume; absent for no job.
3. The session text. With `[features] mail` on (M§8), item 8 of M§8a lists each member with its job in brackets, and a member with no job by its name alone: `Members: orchestrator (orchestrator), worker-1 (worker), elena, …`. It adds one fixed sentence for the member itself:
   - orchestrator: `Your job: orchestrator. Give work to the workers with mail_send, collect their results with mail_read, and check each result against its brief.`
   - worker: `Your job: worker. Take your work from <orchestrator> by mail, do it through your zygo-<role> subagents, and send the result to <orchestrator> with mail_send.` With no orchestrator in the workspace, `<orchestrator>` is `the member that gave you the work`.
   - no job: no sentence.

   The text holds names and tool names only (M-C2, C7). With mail off there is no job sentence, because it names mail tools.

Why a fixed sentence and not a repository prompt: it makes `job` mean something to the agent without any repository text in the session text (R§8).

### 9.4 The member maximum

**(operator)** `[limits]` in user config sets a maximum member count that a repository file cannot exceed. **(author)** The rule:

- `[limits] max_members`, an integer 1-32, default 8. Set by the built-in, team and user layers. When `zygo up` cannot read the team layer, or the team-merged config does not load, it prints `zygo up: team config not read (<why>); [limits] max_members from user config` and goes on with the user config only. When it reads the team layer, it prints only the warnings the team layer causes, each on one line after `zygo: `: `config: <tables> in team config ignored; only user config may set them` and `[loadout] and [workspace] are read from user config only; ignored`; it prints no user-layer warning.
- `.zygo.toml` cannot set it. The repository-file load removes `limits.max_members` and prints `.zygo.toml: limits.max_members ignored; only user or team config may set it`, as it does for `guards.min_level` (`layers.rs:505-509`). Why not the "lower value" rule of the turn cap: a repository cannot raise the maximum by construction, a lower value from it has no use, and one rule needs fewer tests.
- `zygo up` counts the agent panes of the repository loadout. Above `max_members`: exit 1 before any write, `zygo: this repository's workspace files start <n> agents; [limits] max_members is <m>; nothing started`.
- It applies to the repository loadout only. A user loadout is the user's own.
- R§6.2 also caps a file at 32 members, so the parse is bounded whatever the user config says.

### 9.5 The pool

**(operator)** The pool path is user config, not repository content: it is a path on one machine and it leads to credentials. The repository file can only say that a member takes its config directory from the pool. Assignment is round-robin over the pool in name order, stable per member name across restarts. With fewer directories than members, directories are shared and `zygo doctor` reports it. With no pool, each member uses the user's default config directory. zygo does not read the pool's credentials.

**(operator, 2026-11-24)** A pool is a passthrough feature. Separate accounts exist only in passthrough: outside it, Claude Code sends the session token through zygo's key helper and the proxy calls the provider with zygo's keys (§8.5, §12.3), so there is no agent account to separate. Defining a pool in user config is the operator's consent to passthrough for the pool members; no other key exists for it. A member that takes its config directory from the pool runs in passthrough. A member that does not is unchanged: a normal zygo launch with role routing.

User config (W§11 `[workspace]`, user config only, AU22):

```toml
[workspace]
pool = "~/.claude-interns"   # `~` expanded as --config-dir does; must be absolute after that
```

**(author)** The rules:

- **Directories.** Each entry of `pool` whose name does not start with `.` and that is a directory (links followed: the pool is the user's own), in byte order of name. zygo reads names and types only; it opens no file in them (R-C6).
- **Assignment.** At a fresh build only (W§8.1 step 7, a loadout plan). The pool members, in byte order of name (panes with no name after them, in pane order), get the directories in turn: member `i` gets directory `i mod k`. The path becomes the pane's `config_dir`, so the existing path sets `CLAUDE_CONFIG_DIR` (`crates/zygo/src/cmd/claude.rs:37-39`, `:66-90`) and the launcher callback saves it in `member.launch.config_dir` and `member.config_dir` (AU92).
- **Stable across restarts.** A resume never assigns again: `ws run` sets the saved `member.config_dir` (AU61), because a resume with another config directory finds no conversation (PW16, failed sub-check 16.2 on 2.1.330). An `up --reset` with the same members and the same directories gives the same assignment.
- **No pool.** A pane with `pool = true` starts as any other member: no `config_dir`, so the inherited `CLAUDE_CONFIG_DIR` or the default directory, and no passthrough **(operator)**.
- **A pool that does not work.** Only a plan with at least one pool member checks the pool **(lead: ruling 8)**. With `pool` relative after the `~` expansion (also an error of `zygo config check`), not a directory, not readable, or with no subdirectory, `zygo up` exits 1 before any write, `zygo: pool <dir>: <reason>; nothing started`. Reason: a silent fall back would move each member to the default directory and its login. A plan with no pool member does not read the pool.
- **Shared directories.** With `k` directories and `n > k` pool members, `zygo doctor` prints one line per shared directory, `pool: <dir> is shared by <name>, <name>`.
- **OpenCode.** No pool: R§6.2 and `config check` refuse `pool` on an `opencode` pane. Reason: OpenCode sends its model requests to the proxy with the session token (§8.7), so its own login is never used, and zygo already gives each session its own `OPENCODE_CONFIG_DIR` (§8.7). A pool would only move OpenCode's data folders, which hold its sessions, and it would break resume (WA-1).

**Passthrough (operator, 2026-11-24; author for the lines).**

- **Resolved at plan time.** `up` sets `config_dir` and `passthrough = true` on each pool member's pane when it makes the plan, and the launcher callback saves both in the member record (`member.launch.passthrough`, `member.config_dir`; W§7.2, AU92). A resume uses the saved values (`crates/zygo/src/cmd/ws.rs:420-431`), so a later change or removal of the pool never changes the credential of a running or resumed conversation. A change applies at the next fresh `up`.
- **What passthrough is** (§12.3, §12.4; `docs/passthrough.md`): the member runs on the login of its pool directory; the proxy does no role routing for it; its generated `zygo-<role>` subagents carry no `model`; a profile, the repository's included, does not apply to it. This spec does not change passthrough.
- **Shown to the operator.** The approve summary (R§7.1 step 6) and `zygo up` name the members that run in passthrough on the pool. `zygo up` prints, once per fresh build with at least one pool member: `zygo: <name>, <name> run in passthrough on pool <dir>`. For each such member that names a profile, the line adds `; profile <n> of <name> does not apply in passthrough`.
- **Local mode only**, as passthrough is today (§12.1). When the user config sets `[server] url` and the plan has a pool member, `up` exits 1 before any write and before any pane starts: `zygo: pool <dir>: passthrough is local mode only, and [server] url is set; nothing started`.

**Launch (§8.4, §8.5) (author).** The pool sets `CLAUDE_CONFIG_DIR` and `--passthrough`; nothing else. The per-session settings file is passed with `--settings`, the highest scope after managed settings, so zygo's keys win over the pool directory's own `settings.json` (§8.5). `--plugin-dir`, `--mcp-config` and `--agents` are per-session flags and do not depend on the config directory. The preflight reads `$CLAUDE_CONFIG_DIR/settings.json` (`crates/zygo-launch/src/claude_code.rs:565-569`), so a pool directory whose settings set a credential key or a provider switch stops that member's launch with exit 2 (§8.4), and the pane follows the existing failed-launch path (W§8.2). Plugins that the user installed in a pool directory load as they do from the default directory; the doctor check for a plugin that would load twice reads the default directory only (`crates/zygo/src/cmd/doctor.rs:266-292`) and is not run on pool directories (R-C6).

**Claude Code version (author).** No new minimum. A pool directory is a `CLAUDE_CONFIG_DIR`. zygo's minimum is 2.1.328 (`crates/zygo-config/data/agents.toml:2-3`), on which the user settings file is `$CLAUDE_CONFIG_DIR/settings.json` (§8.4 note of N32) and a resume needs the same directory (PW16, 2.1.330). R§19 PR1 and PR2 verify the pool on the pinned version.

## 10. Entry points

**(author)** Each entry point that uses the definition does one read and one check (R§7.3), or one pinned read (R§7.4). No other one reads `.zygo/`.

| Entry point | Behavior |
|---|---|
| `zygo up` (W§8.1 step 7, a loadout plan: `--reset`, no state, or no pane) | The loadout: `--loadout <l>`; else `[workspace] default_loadout` when the user config file sets it; else `repository` when the default branch has `.zygo/workspace.toml`; else `solo`. A `repository` plan is made after step 3, when the checkout is known, and before step 8; each of its panes gets the pin (R§7.4). The read runs at the top of the checkout that step 3 found from the current folder (`Checkout.top`: the work tree that holds the current folder, also from a subfolder; from a bare repository's own folder, the checkout of its main worktree), the folder at which `zygo workspace approve` (R§7.1 step 3) reads; the workspace id is the W§5 id of that checkout's common dir. A workspace found by `<name>` (W§8.1 step 3, AU20) has no checkout from the current folder: the read runs at its main worktree. The panes start in the folders of the plan (W§11), which are based on the main worktree (AU77), always a valid worktree: not bare, not marked `prunable` by git, and not inside zygo's workspaces cache folder, where the mail-sync worktree lives. A new workspace takes the first valid worktree of the list. A workspace with a saved state, by `<name>` or from any checkout of the same repository, keeps its saved `main_root` while that is a valid worktree; when it is not, `up` takes the first valid worktree and prints `zygo: the main worktree <old> is gone; panes start in <new>` (paths escaped). By `<name>` the list is the one that git gives in the saved common dir; with no valid worktree in it, `up` (also `up --all`, which skips that workspace) exits 1 with `zygo: no checkout at the main worktree <saved main_root>` before any read or write. A list there that fails or passes the bound makes `up` exit 1 with its reason (`up --all` skips that workspace). The list and these checks are only for `zygo up`: `zygo down <name>` and `zygo attach <name>` read no definition, run no `git worktree list` and use the saved state, so a user can always stop a workspace whose worktree folders are gone or whose list fails or passes the bound. `up` saves the main worktree as `main_root` in its step 8 write. So no default pane folder is a deleted worktree or zygo's own worktree, and `zygo up <name>` and `zygo up` from any worktree of the same repository plan the same pane folders. The launcher of a member (R§7.4, R§8) reads again at the checkout of the workspace's main worktree (`main_root`), not at the checkout of its pane folder; that can be another worktree than the one `up` read at. All worktrees of one repository share its refs, its objects and its common dir, so the workspace id and what the read finds are the same; a pane folder in a submodule or a nested repository plays no part in the read or the approval check. Chosen by the default and not approved, invalid or unread: plan `solo` and one line: not approved, `zygo: this repository's workspace files (<full ref>, digest <12>) are not approved on this machine; nothing from them was applied; see zygo workspace approve`; invalid, `zygo: this repository's workspace files (<full ref>) are invalid (<why>); nothing from them was applied`; unread, `zygo: this repository's workspace files are unread (<why>); nothing from them was applied`. Files that hold profiles only and are not approved plan `solo` with no line (R§9.2). Chosen by `--loadout repository`, or by a user `[workspace] default_loadout = "repository"` with no user `[loadout.repository]`: the same cases exit 1 with the same line and start nothing; approved files with no `.zygo/workspace.toml`: exit 1, `zygo: this repository's workspace files at <full ref> have no .zygo/workspace.toml; nothing started`; absent: exit 1, `zygo: this repository has no workspace files at <full ref>`. `zygo config check` accepts a user `default_loadout = "repository"` with no user `[loadout.repository]` where this row plans the repository loadout, and reports it as an error where this row refuses it (the `zygo config check` row). `config check` accepts the setting, not the plan: `up` can still stop after the plan, by R§9.4 (`[limits] max_members`) or by an error when the plan resolves, and `config check` does not report that; `zygo doctor` reports the `max_members` case (`workspace files: <n> agents; [limits] max_members is <m>`). Then R§9.4 (the repository loadout only) and R§9.5 (any loadout plan with a pool member), all before any write. A user `[loadout.repository]` is the user's loadout; the repository one is then not used (user first, as for profiles). |
| `zygo up` (a restore), `zygo up --all` | No default-branch read (AU10). Each resumed member uses its saved launch and its pin (R§7.4). A workspace that `up --all` builds from a loadout takes the user `[workspace] default_loadout`, else `solo`; when that is `repository` and the user config has no `[loadout.repository]`, it plans `solo` and prints `zygo: up --all reads no repository workspace files; using solo`. |
| Repair of a dead build (AU67) whose `loadout` is `repository` | Reads nothing. AU67 works from `panes[]` and the member records that step 8 wrote from the checked read; it does not resolve the loadout again. |
| `zygo ws run` (start or resume of one pane) | The saved launch holds the profile name, the job, the config directory, `passthrough` and the pin. A repository profile resolves by the pinned read (R§7.4). |
| `zygo claude`, `zygo opencode` (not a member) | R§8: a default-branch read only when the requested profile is not in user or team config. |
| `zygo config show [--layer <name>]` | In a repository: the approved repository profiles and `loadout.repository`, each key with the source `workspace-files` and its file: `<key> = <value>  # workspace-files .zygo/profiles/rust.toml` (`Merged::show`, `crates/zygo-config/src/layers.rs:830`). `workspace-files` is a source label, not a layer; `--layer workspace-files` shows only these keys, and `--layer repository` stays `.zygo.toml` only **(lead: ruling 6)**. `--layer` with a layer name can show no such key, so it reads nothing. Not approved, invalid or unread: nothing from the files, one stderr line. |
| `zygo config check` | Validates the definition by R§6.4, approved or not. A violation is one warning line, `warning: workspace files: <violation>`; the exit status does not change, as for an invalid `.zygo.toml` (§13.5), with one exception: when the user config sets `default_loadout = "repository"` with no user `[loadout.repository]` and the read gives files that `zygo up` refuses (invalid files included), `config check` also gives `error: workspace.default_loadout: unknown loadout` and exits 11 (below). Unread: one stderr line. A user `[workspace] default_loadout = "repository"` with no user `[loadout.repository]` follows this read, as the `zygo up` row decides it (`crates/zygo-config/src/check.rs` `check_with`, `RepositoryLoadout`): approved files that hold `.zygo/workspace.toml` give no line; a read that adds no loadout (absent, not approved, invalid, approved with profiles only) gives `error: workspace.default_loadout: unknown loadout` and exit 11, where `zygo up` exits 1; no read (no checkout at the current folder, or unread) gives one line, `warning: workspace.default_loadout: no workspace files were read here; zygo up uses "repository" only from approved files that hold .zygo/workspace.toml`, and the exit status does not change. `zygo serve` reads no workspace files and starts no workspace: at start and at reload it checks as with no read, so this setting does not stop it (base design §7.2, N16 note). |
| `zygo profile list`, `zygo profile show <n>` | Approved repository profiles with `(workspace-files)`. `list` also shows each repository profile that a user profile or a team preset shadows, `(workspace-files, shadowed)`, after the row of that name; `show` prints the profile that applies. Not approved, invalid or unread: not listed, one line. The slash form `/zygo:profile` runs in the prompt hook, so it reads nothing (R-C8, R§18 item 20) and shows the profiles of the config only. |
| `zygo loadout list`, `zygo loadout show repository` | `list` keeps its `<name>  <source>` rows (W§12). When the approved files add the repository loadout, its row is `repository  workspace-files`. When the read has a `.zygo/workspace.toml` and its repository loadout is not used, one more row follows the rows named `repository`: `repository  workspace-files, not approved` (not approved), `repository  workspace-files, invalid` (any violation in the read, because the read stops at the first one) or `repository  workspace-files, shadowed` (approved, and the user config has `[loadout.repository]`). A read whose `.zygo/` listing is above an R§6.1 step 4 listing limit counts as a read that has `.zygo/workspace.toml` (it is not known whether it has one, and the row reports the files as they are), so it gives the `invalid` row **(author: FX-39)**. A read with no `.zygo/workspace.toml` gives no such row in any state, an invalid one included (for example, only an invalid profile file). Unread: no row, one stderr line. `show repository` prints the user `[loadout.repository]` when there is one, and reads nothing; else the repository loadout only when approved; not approved, invalid or unread: exit 1 with the one stderr line. |
| `zygo doctor` | `workspace files: <full ref> <commit 12> digest <12>: approved`, or `not approved`, `invalid (<why>)`, `none`, `unread (<why>)`; `pool: <dir>: <k> directories` or `pool: <dir>: <reason>`; the R§9.5 shared lines for the loadout `up` would choose; `workspace files: <n> agents; [limits] max_members is <m>` when above. Each line reports and never refuses (exit 0). |
| `zygo workspace approve` | R§7.1. |
| Hooks, the MCP server, the status line | Read nothing (R-C8). |

Two sessions at once: two `zygo workspace approve` runs take the approvals lock in turn, and an entry already present is not added twice. An `up` that runs during an approval uses its own read (R§7.3). Two `up` runs for one workspace are serialized by `op.lock` (W§8.1 step 4).

## 11. Failure handling

| Case | Behavior |
|---|---|
| A git call fails or passes the 2 s budget | Unread: counts as absent, one stderr line naming the call. `approve` exits 1. |
| `origin/HEAD` points outside `refs/remotes/origin/` | Unread, with that reason. |
| The default branch gets a new commit that changes `.zygo/` | New digest: not approved until `zygo workspace approve`. Running and resumed members keep their pins (R§7.4). A plain `zygo claude --profile` gives the R§8 line. |
| The default branch returns to approved content | That digest applies again (R§7.2). |
| An agent edits `.zygo/` in a worktree or on a work branch | No effect (R-C4). |
| An agent moves `origin/HEAD` or a branch, or pushes to the default branch | It can choose among approved digests only; nothing else applies without an approval (R§6.1). |
| A pinned commit or object is gone, its approval removed, the pinned read unread, or no checkout at the main worktree | The rule splits by cause (R§7.4). **Approval removed** (the user deleted the `(workspace id, digest)` entry of the pin, R§7.2): every member launch whose record holds a pin stops with the R§7.4 line, whatever its profile and in passthrough too, and whatever its pane folder (the id comes from the checkout of the workspace's main worktree); the check reads the approvals file and no workspace file; the record does not change. **No checkout at the main worktree** (no work tree, a failed git call, a call past its bound): every member launch whose record holds a pin stops the same way, with the not-read line. **Pane folder inside a git dir** (links resolved): every member launch whose record holds a pin stops with the R§7.4 pane-folder line. **Pin gone or pinned read unread**, with the approval still in the store: only a member launch that makes the pinned read stops with the R§7.4 line (it asks for a profile, by `--profile` or `[user] default_profile`, that neither a user profile nor a team preset defines, and it is not in passthrough), and the record does not change. Every other member launch (no profile, a profile that a user profile or a team preset defines, a pool member in passthrough) reads nothing from the commit and starts, also at a resume and at a restore by `zygo up`. |
| Approvals file invalid | Nothing is approved; `approve` exits 1 and does not change the file. |
| Approval write fails | Exit 1; the temp file is removed; nothing is stored. |
| The pool directory is gone at a resume | The existing `--config-dir` rule (`claude.rs:80-82`): the launch fails and the pane opens a shell with the W§8.2 line. |
| A pool directory's settings set a credential key | That member's launch exits 2 (§8.4); the other members start. |
| `up` crashes during a repository build | AU67 repair from the saved records reads nothing. When the repair takes the plan back (no pane left), the loadout plan that follows is an R§10 `zygo up` choice and reads once; a kept member keeps its pin (R§7.4). |

## 12. Owners of state

| State | Owner (only writer) | Readers |
|---|---|---|
| `.zygo/` files | The repository's authors, through git. zygo never writes them. | R§10 entry points. |
| `[[workspace]]` entries of `approvals.toml` | `zygo workspace approve`; the user removes one by hand. | R§7.3 and R§7.4 checks. |
| `[workspace] pool`, `[limits] max_members` | The user, in user config (`max_members` also team config). | `zygo up`, `zygo doctor`, `zygo workspace approve` (summary). |
| The pool assignment and its passthrough | `zygo up` at a fresh build, saved through `member.launch.config_dir`, `member.config_dir` and `member.launch.passthrough` by the launcher callback (AU92). | `ws run` at resume (AU61). |
| `member.launch.definition` (the pin) | `zygo up` writes it into the fresh member records of a repository-loadout build; the launcher callback keeps it at each start write (AU92); for a member not from the repository loadout, the callback of its first start that applied a repository profile from a default-branch read writes it once. | The launcher at each later start (R§7.4). |
| `member.launch.job` | `zygo up` (from the loadout pane), the launcher callback (AU92). | The session-start hook (R§9.3), `ws run`. |

## 13. Configuration

User config, the additions of this spec:

```toml
[workspace]
pool = "~/.claude-interns"   # pool members run in passthrough (R§9.5)

[limits]
max_members = 8

[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
name = "elena"          # M§6 (P3-1)
job = "worker"          # R§9.3; absent: no job
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
| W§11 | Pane keys `job` and `pool` (`name` is P3-1's, M§6); key `[workspace] pool`; the `config check` rules of R§9.1. |
| W§7.2 | `member.launch.job`; `member.launch.definition = { commit, digest }` (R§7.4). `member.launch.passthrough` can now be set by the pool (R§9.5). |
| W§8.1 step 7 | The loadout choice and the pin of R§10; the R§9.4 and R§9.5 checks before step 8. |
| W§12 | Row `zygo workspace approve [--yes] [--digest <hex>]`: R§7.1. Not subject to OP5. |
| §6.1 layer 3 | `[workspace] pool`; `[limits] max_members`. |
| §6.1 layer 4, §13.5 | `.zygo.toml` cannot set `limits.max_members` (R§9.4). Its four tables and its short-name read do not change (R§6.1 "Difference"). |
| §6.1 | The layer order does not change **(operator)**. `zygo config show` names the source `workspace-files` and the file for the keys the repository files add (R§10). |
| §11.1 | Profiles come from user config, team config, and, after an approval, the repository's `.zygo/profiles/` with `overrides` and `disabled` only (R§8) **(operator: the source; author: the parts)**. |
| §11.2 | Order of sources for one name: user, team preset, repository (R§8). A member resolves a repository profile from its pin (R§7.4). |
| §11.3 | A repository profile gets the same rules; the shape rules of R§6.3 apply at read. |
| §12 | A pool member starts with `--passthrough` (R§9.5) **(operator, 2026-11-24)**. §12.3 and §12.4 do not change. |
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
| Re-read the default branch at each resume | A running conversation would get the profile of another approval, or lose it (R§7.4) **(lead)**. |
| Start a pinned member without its profile when the pin is gone | Its models change with no notice (R§7.4) **(lead)**. |
| Short ref names for the default branch | A tag `main` wins over the branch, and a name that starts with `-` can be read as an option (R§6.1) **(lead)**. |
| Ask again for content that was approved before | The same bytes for the same repository; a second question adds no safety (R§7.2). |
| Approval by digest alone | A hostile repository could copy a definition approved for another repository. |
| Approval per file | A partial approval gives a definition that no one saw as a whole. |
| Profiles with prompts, custom roles, plugins or features | R§8: no use that `CLAUDE.md` does not give, plugin code, or a weaker guard. |
| Key-by-key merge of a repository profile with a user or team profile of the same name | A mix that no source defined. |
| `[[members]]` as an array | Two members could have one name, and the pane order would depend on the file order. |
| The "lower value" rule for `max_members` from `.zygo.toml` | No use for a lower value from the repository; one rule is simpler (R§9.4). |
| Pool assignment by a hash of the name | Two members can get one directory while others are free. |
| A pool for OpenCode through its data folders | Moves its sessions and breaks resume; its login is never used (R§9.5). |
| Fall back to the default config directory when the pool fails | Moves members to the default directory with no notice. |
| A passthrough key for the pool, global or per pool (earlier drafts) | A pool has no meaning outside passthrough; defining the pool is the consent **(operator, 2026-11-24)**. |
| Read passthrough from user config at each resume | A changed table would change a running conversation's credential (R§9.5). |
| Escape non-ASCII in `cwd` in place of a refusal | The operator would read an escaped name and the agent would start in the real one; a refusal leaves one form (R§6.2). |
| Read `.zygo/` in `load` | Each hook would make more git calls (R-C8). |
| A raw tmux layout string or a window name from the repository | Repository text in tmux arguments (W-C8); the five presets are enough. |

## 16. Files expected to change

- `crates/zygo-config/src/repo_def.rs` (new): the read and the pinned read (R§6.1, R§7.4), the typed shapes and validation (R§6.2-6.4), the digest (R§6.5), the approval store read and write (R§7.2), `Merged::with_definition` with the `workspace-files` source (R§8, R§9.1).
- `crates/zygo-config/src/schema.rs`: `LoadoutPane` (`:722-741`) gets `job` and `pool`; `WorkspaceConfig` (`:685-698`) gets `pool: Option<String>`; `LimitsConfig` (`:599-602`) gets `max_members`.
- `crates/zygo-config/src/layers.rs`: the `.zygo.toml` filter drops `limits.max_members` (next to `:505-509`); `show` (`:752-782`) prints the `workspace-files` source and file.
- `crates/zygo-config/src/check.rs`: the R§9.1 and R§9.5 checks.
- `crates/zygo-workspace/src/loadout.rs`: `Source::Repository` (`:17-22`), the pool assignment and its `passthrough`, `job`, `pool` and the pin in `PlanPane` (`:35-46`).
- `crates/zygo-workspace/src/up.rs`: the step 7 choice and pin (next to `:713`), the R§9.4 and R§9.5 checks and lines.
- `crates/zygo-workspace/src/state.rs`: `Launch.job` and `Launch.definition` (`:148-170`).
- `crates/zygo/src/cmd/ws.rs`: `launch_of` and `launch_open_code_of` (`:420-443`) pass the pin.
- `crates/zygo/src/cmd/workspace.rs`: the `approve` subverb.
- `crates/zygo/src/cmd/doctor.rs`, `config.rs`, `profile.rs`, `loadout.rs`: the R§10 lines.
- `crates/zygo-launch/src/launcher.rs`: `LaunchOpts` (`:127`) gets the pin; the R§8 and R§7.4 reads and lines (next to `:326-343`); the callback saves the pin.
- The session-start text of M§8a, item 8 (the hook file that P3-1 adds).
- The user docs that describe `.zygo.toml`, profiles, loadouts and passthrough.

## 17. Left to the builder

- The exact git commands of R§6.1 steps 1-5, within its rules (one commit id, full ref names, sizes before content, one budget).
- Type and function names, and the module split, within R§16.
- The text of lines this spec does not fix, within R-C7.
- The place of the new `zygo doctor` lines in its output.

## 18. Acceptance

Each check is a test in temp directories with temp git repositories (a bare "origin" and a clone, so `refs/remotes/origin/HEAD` exists), stub agents and a stub tmux where the existing workspace tests use one. No real agent, no real agent config directory, no wall-clock assertion.

1. **Default branch only.** With `.zygo/` committed on the default branch: an edit in the working tree, a commit on another branch, and an edit in a linked worktree each leave the digest the same. A new commit on the default branch that changes a file changes the digest.
2. **Tree entries.** `.zygo` as a link, as a submodule and as a file; `workspace.toml` as a link; `profiles/x.toml` as a link and as a tree: each read is invalid with the R§6.1 reason, and the fake `git` log shows no content read.
3. **Sizes.** `workspace.toml` of 64 KiB + 1 byte, a profile of 16 KiB + 1 byte, and 33 profile files: each read is invalid before any content read (fake `git` log). **Listings (FX-39):** `.zygo/` with 257 entries: invalid, `.zygo lists more than 256 entries`; `.zygo/profiles/` with 257 entries: invalid, `.zygo/profiles lists more than 256 entries`; a `.zygo/` tree whose listing is above 131072 bytes with fewer than 256 entries (long names): invalid, `.zygo: the listing is larger than 131072 bytes`; 256 entries: found. No git call of these reads gives zygo more than 131072 bytes of output. `zygo loadout list` shows the `repository  workspace-files, invalid` row for a `.zygo/` listing above a limit.
4. **Allow list.** Each of `members.a.args`, `members.a.config_dir`, `members.a.passthrough`, `members.a.yolo`, `members.a.server`, `members.a.compact`, `kind = "shell"`, a top-level `[[model]]`, `[redaction]`, `[update]`, `[workspace]`, an unknown top-level key, `version = 2`, two orchestrators, 33 members, `pool = true` on `opencode`, `cwd = "/tmp"`, `cwd = "../x"`, `cwd = "~/x"`, `cwd = "dоcs"` (a Cyrillic `о`), `cwd = ".git"`, `cwd = "a/.git"`, `cwd = ".GIT/x"`, `cwd = "x/.Git/y"` (while `cwd = ".github"` and `cwd = ".gitignore"` stay valid), an override value with a non-ASCII character, a profile `prompt`, `roles`, `plugins`, `features`, an empty profile, and a profile file named `None.toml` or `none.toml` makes the definition invalid with the R§6.4 path, `approve` exits 1, and `up` applies nothing.
5. **Approve.** Answer `y`: one `[[workspace]]` entry with the id and digest. Answer `n` or end of input: none, exit 0, `not approved` line. `--yes`: an entry, listing on stdout, holding the full ref and the full commit id. `--digest` of another value: none, exit 1. `ZYGO_SESSION_DIR` set: none, exit 1. Slash form: the command text, none. A second approve: `already approved`, the file unchanged.
6. **Race.** Approve digest D1; commit a change on the default branch; a fresh `up` plans `solo` and prints the not-approved line. A unit test with a counting reader shows that `up` and the launcher read the definition once per use, and that the applied plan comes from the read whose digest was checked.
7. **Escaping.** A `workspace.toml` whose lines end in CRLF and whose comment holds U+202E and `é`, and an ignored entry of `.zygo/` whose name holds `ESC [ 2 J` and a carriage return: the approve listing holds none of these characters raw, and each is shown as `\u{…}`. A comment that holds ESC, or a carriage return that no newline follows, is not TOML (toml 1.2.1: `invalid comment character`; `carriage return must be followed by newline`), so that definition is invalid: `approve` exits 1, and its line holds the TOML message and position only (R-C7), with no ESC. A member name that holds U+202E: the violation line shows it as `\u{202e}`.
8. **Approval key.** The same `.zygo/` content committed in a second repository is not approved there.
9. **Profiles.** After approval, `zygo claude --profile rust` (stub agent) sends the overrides of `.zygo/profiles/rust.toml` in the session request. A user profile `rust` is used in its place, and `zygo profile list` shows `rust (workspace-files, shadowed)`. Not approved: the R§8 line, no profile. An override to a model not in the catalog: the §11.3 line. A launch without `--profile` makes no `ls-tree` call (fake `git` log).
10. **Default set.** An approved `workspace.toml` with `version = 1` only: the plan has one window, layout `main-vertical`, five `claude-code` panes `orchestrator`, `worker-1` to `worker-4`, in that order, each `pool = true`. Profile files without `workspace.toml`: `up` plans `solo` and prints no line.
11. **Choice.** `--loadout solo` and a user `default_loadout` each win over an approved repository loadout. `--loadout repository` with no `.zygo/`: exit 1, no state file written.
12. **Maximum.** `[limits] max_members = 4` and the default set: exit 1, no state file written. `.zygo.toml` with `[limits] max_members = 30`: the ignored line, and the effective value stays the user value.
13. **Pool.** A pool with directories `a` and `b` and the default set: the member records hold `orchestrator → a`, `worker-1 → b`, `worker-2 → a`, `worker-3 → b`, `worker-4 → a`; `zygo doctor` prints the shared lines for `a` and `b`. After a directory `0` is added, a restore keeps each saved directory. A `pool` that is missing, a file, or holds no directory: exit 1, no state file written. The same broken pool with a plan that has no pool member (`--loadout solo`): `up` succeeds. No pool: no `CLAUDE_CONFIG_DIR` from zygo and no `passthrough` for the `pool = true` panes. A pane without `pool = true` in the same plan: no `config_dir`, no `passthrough`. On unix only (`cfg(unix)`; Windows has no portable way to make a file unreadable to its owner): a pool directory with mode-`000` files does not change the plan or the output of `up`.
14. **Passthrough.** With a pool, each pool member's record has `launch.passthrough = true` and its pool directory, and `up` prints the R§9.5 line once, naming them. After the user removes `pool`, a restore keeps `passthrough = true` and the saved directory for the resumed members, and an `up --reset` gives neither. With `[server] url` set and a pool member: exit 1, no state file written, no pane started (stub tmux log).
15. **Pool and profile.** Member `worker-1` with `profile = "rust"` and `pool = true`: with no pool, its stub launch sends the `rust` overrides; with a pool, it starts with `--passthrough`, sends no overrides, and the `up` line ends with `; profile rust of worker-1 does not apply in passthrough`. With a pool, the approve summary names the members that run in passthrough and the profile that does not apply.
16. **Pin.** After a fresh `up`, each member record holds the read commit and the digest. Commit a profile change on the default branch; then (a) approve the new digest, (b) do not approve it. In both cases a restore and a `ws run` resume send the profile of the pinned commit. A fake `git` that fails the pinned listing: the launch stops with the unread reason, and the record is unchanged. A pool member (passthrough) whose pinned commit is gone starts, with no pinned read, and so do a member with no profile, a member whose profile a user profile defines and a member whose profile a team preset defines; a member with no `launch.profile` whose `[user] default_profile` names a repository profile makes the pinned read and stops. When the user deletes only the approval entry of the pin (another entry stays in `approvals.toml`), each of these members, the pool member included, stops with the R§7.4 line, with no git read, and its record is byte-equal to before. Then make the pinned commit unreachable and run `git gc --prune=now` (with the reflog expired): the member's launch prints the R§7.4 line, starts no agent, and its record is byte-equal to before. **The repository of a member launch (FX-39):** with the approval in place, a member whose pane folder is a submodule or an untracked nested repository of the main worktree starts, with no profile and with a repository-only profile (it gets the pinned profile); after the withdrawal, a member whose pane folder is `<main worktree>/.git`, a submodule, a nested repository or a folder outside any checkout stops with the R§7.4 line. With a stub `git` on `PATH` that fails the checkout read, and (an ignored timing check) one that sleeps past its bound, a member with a pin stops with the not-read line and starts no agent. A main worktree that is not a checkout gives the same stop. With the approval in place, a pane folder that is `<main worktree>/.git`, a committed link to `.git`, or a folder whose resolved path has the `.git` part of a nested repository stops with the pane-folder line. The workspace id of the check is the one git gives at the main worktree: no value in the state file changes it.
17. **Rollback.** Approve D1; commit D2 and approve it; revert to the D1 content: a fresh `up` applies D1 with no new approval.
18. **Ref.** `origin/HEAD` moved to another `refs/remotes/origin/` branch with approved content: that content applies; with content that is not approved: nothing applies. `origin/HEAD` as a symbolic ref to `refs/heads/x`: unread with the R§6.1 reason. No `origin/HEAD`, a branch `main` and a tag `main` that point at commits with different `.zygo/` content: the read uses the branch.
19. **Session text.** With mail on, a worker's session text holds `Your job: worker.` and the orchestrator's name; a user-loadout member with no `job` is listed by name alone and gets no job sentence; with mail off, no job sentence.
20. **Hooks.** The rule: no hook, slash verb, MCP or status-line path reads the definition, from git objects or from the working tree. In a repository whose default branch has an approved `.zygo/` and a `.zygo.toml`, for a main session and for a member: each Claude Code hook that zygo registers (`token`, `statusline`, `role-guard` for Edit, Write and NotebookEdit, `guard`, `trim` for Bash, Read, Grep, Glob and WebFetch, `work` with no event and with `--event begin` and `--event verdict`, `session-start` with no event and with `--event member`, `prompt`, `pre-compact`, and `workspace` with each of its eight events), each of the 19 slash verbs as the prompt `/zygo:<verb>` of a main session (the verb runs in the `prompt` hook process), each `zygo hook` call of the OpenCode plugin (also `workspace --event session` with a session id and `--event pre-tool` for the `Agent` call of the `task` tool), and `zygo mcp` (start, `tools/list` with `[features] workspace_tool` on, one call of each listed tool with arguments that pass its argument checks) exit 0. Three checks see no read in each run:
    1. **Source scans**, on every OS. The crates that the hooks and the MCP server link, `zygo hook`, `zygo mcp`, the `zygo statusline` verb and the slash verb files (`cmd/verbs.rs`, the module of each `VERBS` handler, and `zygo_launch::serve_client`, the one `zygo-launch` module that they name) name no read (`repo_def`, `check_default`, `check_pinned`, `read_default`, `read_at`, `with_definition`, `zygo_workspace::up` other than its items `find_checkout` and `GIT_BOUND`, `zygo_launch` other than `zygo_launch::serve_client`). In `zygo-workspace`, only the private `choose` of `up.rs` names a read. `zygo-config` declares `repo_def` at its root and re-exports nothing of it, and `load` names no `repo_def`. No source that the hooks link names an absolute path to `git`, so every `git` child is the `git` on `PATH`.
    2. **The `repo_def` tripwire.** In a test build (feature `test-hooks`), each public read entry of `repo_def` (`read_default`, `read_at`, `check_default`, `check_pinned`, `SystemGit::git`) writes one line to the file that `ZYGO_TEST_TRAP_REPO_DEF` names. The run and each child that it starts, a detached child too, inherit the variable. The file is empty at the end of each run and again after the last run in each repository. Each `git` child is a fake `git` that records the arguments and the stdout and stderr of the real `git`. In the repository with `.zygo/`, each `.zygo/` file holds a canary line and `.zygo/` holds a file whose name holds the canary. No call writes output that holds the canary, the raw tree entry `.zygo` with its NUL, or a line whose path after the last tab is `.zygo` or starts with `.zygo/`. This output check sees a read of `.zygo/` content or of the root tree whatever the arguments of the call. As a second check, the arguments hold no read of `.zygo/`: no `symbolic-ref --quiet refs/remotes/origin/HEAD`; no argument that holds `.zygo` when each `.zygo.toml` is taken out; no `ls-tree -r` with no pathspec; no `cat-file --batch-all-objects`; and no stdin line of a `cat-file --batch` call that holds `.zygo`, names the root tree, or is the id of the root tree or of a `.zygo` object. Each run starts the same `git` children as in the same repository without `.zygo/`; the status line is not compared, because its git worker stops at the hook bound.
    3. **The working-tree watch**, on Linux. An inotify watch (open and read) of the working-tree `.zygo/` and of each path under it gets no event at the end of each run and after the last run in each repository.

    Limit: checks 2 and 3 see a read by a detached child (for example the mail sync child `zygo mail sync --background`, which starts in its own session) only when the read occurs before the last drain of the tripwire file, the git log and the watch. A read by a detached child after that drain is not checked. For the mail sync child, check 1 covers its code (`zygo-mail-sync` and `cmd/mail.rs`), so it names no `repo_def` read; its `git` calls read the mail branch, its worktree and the remote, not the default branch's `.zygo/` (`zygo-mail-sync/src/branch.rs`, `ops.rs`, `sync.rs`, `public.rs`).
21. **Shared approvals file.** With `[[workspace]]` entries in `approvals.toml`, `is_approved_in` still finds each plugin approval, and `zygo plugins approve` keeps the workspace entries byte for byte.
22. **Config surfaces.** `zygo config show` prints `# workspace-files .zygo/profiles/rust.toml` for the profile keys; `--layer repository` does not show them; not approved: no such line and one stderr line. `zygo config check` with an invalid definition prints the warning and keeps its exit status.
23. **Doctor.** With a fake `git` that fails `ls-tree`, `zygo doctor` prints `workspace files: … unread (…)`; with an invalid definition, `invalid (<why>)`; both exit 0.
24. **Windows.** On native Windows, `zygo workspace approve` runs, and an approved repository profile applies to `zygo claude --profile`.

## 19. Premises the first build verifies

- **PR1** (live check, run by the executor on the operator's machine, recorded in `docs/live-checks.md`; never in CI) On the pinned Claude Code, two pool members of one workspace, on two pool directories each logged in to another account, run in passthrough and each sends its own directory's login. If it fails, the pool does not give separate accounts, and the operator decides again on R§9.5.
- **PR2** (live check, run by the executor on the operator's machine, recorded in `docs/live-checks.md`; never in CI) With `CLAUDE_CONFIG_DIR` set to a pool directory, zygo's `--settings` hooks, the status line and `--plugin-dir` act as with the default directory.
- **PR3** (CI) `toml` 1.2.1 (`Cargo.toml:66`) ends a parse of 64 KiB of nested arrays and nested inline tables with an error or a value, and no stack overflow.

## Change log

- 2026-11-24 · R§9.1 · operator · config check refuses job on a non-agent pane, so a shell pane cannot take the orchestrator slot
- 2026-11-25 · R§8 · author's call (node WD-13) · a named profile that does not apply is reported, as R§9.5 does for up; the not-read line of the launcher's read is listed with the other two; the not-found line is written in full
- 2026-11-25 · R§11 · author's call (node WD-11) · the row "`up` crashes during a repository build" said "no new read", which disagreed with the R§10 `zygo up` row when the repair takes the plan back
- 2026-11-25 · R§9.4 · author's call (node WD-11) · zygo up needs no team server; refusing would block restore and up --all offline
- 2026-11-25 · R§10 · author's call (node WD-11) · the `up --all` row did not say which loadout a workspace with no pane gets; a user default `repository` with no read was an unknown loadout and the workspace was skipped
- 2026-11-25 · R§9.4 · author's call (node FX-14) · up printed user-layer warnings (the ignored user [[model]], the user marketplace source) as if the team read caused them
- 2026-11-26 · R§7.4 · author's call (lead) · up writes the pin into the fresh record at the build and the callback keeps it (the live code of WD-12); the unread line is written in full; a stop exits 1 and escapes the saved commit and digest; a pinned read with no profile of the name, and a launch outside a checkout, are decided; a hand resume of a kept record uses its pin
- 2026-11-26 · R§12 · author's call (lead) · the writers of member.launch.definition follow the live code: up writes it at the build, the callback keeps it, and writes it once for a member whose first start applied a repository profile
- 2026-11-26 · R§7.4 · author's call (lead) · WD-14 review F4 recorded: the pin of a hand resume is read before the lock; the window is stated as a known limit
- 2026-11-26 · R§8 · author's call (lead) · node WD-19: the line references of "Not a layer" follow the live code (load now runs load_with_builtin and load_with_repo_reader; with_definition exists); the hook and MCP callers of load are named
- 2026-11-26 · R§18 item 20 · author's call (lead) · node WD-19: the check covers each entry point of the R§10 row (every hook registration, the OpenCode plugin calls, the MCP server and its tools, the status line), says which git arguments are a read of .zygo/ (the load read of .zygo.toml also holds ".zygo"), and adds the no-new-git-call comparison of review focus 38
- 2026-11-26 · R§18 · author's call (lead) · WD-19 review F3: item 20 said that the crates the hooks link name no zygo_launch, which was false (the slash verbs that the prompt hook runs name zygo_launch::serve_client); item 20 now states the rule (no definition read on any hook, slash verb, MCP or status-line path) and its three checks (source scans, the repo_def tripwire with the git log, the working-tree watch), and names the hook rows, verbs and MCP calls that review F2, F6 found missing
- 2026-11-26 · R§18 · author's call (lead) · WD-19 review round 2 F1, F2: the fake git records the output of each call and a canary in each .zygo/ file finds a read whatever its arguments; the argument check is the second check. F3 recorded: item 20 claims only what is checked: a detached child's read after the last drain is not checked; the mail sync child names no repo_def read (check 1) and its git reads the mail branch
- 2026-11-26 · R§18 item 7 · author's call (node WD-15 planning) · the check could not hold: a TOML comment cannot hold ESC or a lone CR (toml 1.2.1: invalid comment character; carriage return must be followed by newline), so that file is invalid and never listed; the listing is now checked with a CRLF file, U+202E and é in a comment, and ESC and CR in an ignored entry name; the invalid case is checked for its line
- 2026-11-26 · R§6.5 · author's call (node WD-15 planning) · "12 in one-line messages" disagreed with the R§7.1 lines, which name the full <digest> (a script passes it to --digest); <12> applies only where a line names it
- 2026-11-26 · R§7.1 · author's call · a script must learn that the files it read are not the files that apply (WD-15 review F1): the --digest check is now step 4, before the already-approved check (step 5), in every state
- 2026-11-26 · R§10 `zygo loadout list` row · author's call (node WD-16 planning) · `zygo loadout list` prints `<name>  <source>` rows (`crates/zygo/src/cmd/loadout.rs:390-405`, `crates/zygo/tests/workspace_cli.rs:96`), so the text `repository (workspace-files, …)` could not hold; the four states are now source texts on a second `repository` row; unread is one line; `show repository` reads nothing when the user config has `[loadout.repository]`
- 2026-11-26 · R§10 `zygo profile` and `zygo config` rows · author's call (node WD-16 planning) · `/zygo:profile` runs in the prompt hook (`crates/zygo/src/cmd/mod.rs` `slash`), where R-C8 and R§18 item 20 allow no read, so the row says the slash form shows the config profiles only; invalid and unread get the not-approved treatment; `config show --layer <layer>` reads nothing because it can show no file key; the stale `layers.rs:752-782` reference now names `Merged::show`
- 2026-11-26 · R§10 `zygo loadout list` row · author's call · WD-16 review F4: a profile-only invalid read showed a `repository  workspace-files, invalid` row for a repository loadout the files do not define; the extra row is now shown only when the read has `.zygo/workspace.toml`, the same for every state that is not applied
- 2026-11-26 · R§6.2, R§10 `zygo up` row · author's call (node WD-20 planning) · the merged code wins: a member name also may not be `zygo` or of pane-key form (`zygo_core::ids::reserved_member_name`); a user `default_loadout = "repository"` refuses as `--loadout repository` does (`up.rs` `choose`, test `explicit_repository_refuses_and_writes_nothing`); the invalid, unread and no-`workspace.toml` lines are written in full; `zygo config check` still reports that `default_loadout = "repository"` as an error (exit 11) when `zygo up` uses the approved files' loadout (`check.rs` `loadout_violations`; node FX-29)
- 2026-11-26 · R§10 · author's call (node FX-25 planning) · the `zygo up` row did not say where the read runs; up read at the main worktree (the first non-bare entry of the worktree list, which can be a first worktree whose folder was deleted), while approve and the launcher read at the current checkout, so approve could store a digest that up did not find (WD-15 review round 2 finding 5): the read now runs at the top of the checkout of step 3; by name, at the saved main worktree
- 2026-11-26 · R§10 · author's call · FX-25 review F1, F2: the main worktree (pane folders, `main_root`) was the first worktree of the list even when its folder was deleted, and `zygo up <name>` read and started panes in that deleted folder; the row now says where `up` reads (the current checkout; by name, the main worktree found again when the saved one is gone), where panes start (the first worktree whose folder exists) and that the launcher reads at the checkout of the pane folder
- 2026-11-26 · R§10 · author's call · FX-25 review R2 F1: `zygo up <name>` kept a saved `main_root` while its folder existed, but `zygo up` from a checkout took the first worktree whose folder existed, so the two planned different pane folders when a first worktree came back; a saved `main_root` now wins while it is a valid worktree, by name and from any checkout, and a move prints one line
- 2026-11-26 · R§10 · author's call · FX-25 review R2 F2: zygo's own mail-sync worktree, under the workspaces cache folder, could sort first in git's list and become the main worktree; a worktree inside that folder is never the main worktree
- 2026-11-26 · R§10 · author's call · FX-25 review R2 F3: an empty folder at the place of a deleted worktree (an empty mount point) passed the "folder exists" rule and became the main worktree; the rule now uses git's `prunable` mark, not the folder
- 2026-11-26 · R§10 · author's call · FX-25 review R2 F4: by name, with every worktree gone, `up` kept the saved `main_root`, read there and planned panes in a deleted folder; it now exits 1 `no checkout at the main worktree <path>` and saves nothing
- 2026-11-26 · R§10 · author's call · FX-25 close: D4 applies to zygo up only; down and attach must work on a workspace whose folders are gone
- 2026-11-26 · R§10 · author's call · FX-25 close: down and attach by name use the saved state also when the worktree list fails
- 2026-11-27 · R§10 `zygo up` and `zygo config check` rows · author's call (node FX-29) · `zygo config check` reported a user `default_loadout = "repository"` as an error also when `zygo up` used the approved files' loadout; it now follows the read that it already makes, as `zygo up` does: approved files with `.zygo/workspace.toml` accept it, a read that adds no loadout keeps the error (exit 11), and no read (no checkout, unread) is one warning, because the files that `zygo up` will find are not known there
- 2026-11-27 · R§10 `zygo up` and `zygo config check` rows · author's call (node FX-29) · FX-29 review F1, F3, F4: `zygo serve` reads no files and checks as with no read, so it agrees with `config check`; "the exit status does not change" has the `default_loadout` exception; `config check` accepts the setting, not the plan: `up` can still stop after the plan (`max_members`, a resolve error)
- 2026-11-27 · R§11, R§7.4, R§18 item 16 · author's call (node FX-31) · the R§11 row said that a gone or withdrawn pin stops the member, with no condition, but the launcher stops only a launch that resolves a repository profile by the pinned read (R§7.4); the row now names the R§7.4 conditions, R§7.4 says that a launch with no profile, or with a user or team profile, is not stopped, and item 16 checks a member with no profile
- 2026-11-27 · R§11, R§7.4, R§18 item 16 · author's call (FX-31 review F1) · the first FX-31 call let a member launch that makes no pinned read start also when the approval of its pin was withdrawn; but a withdrawal is the user's only way to take an approval back (R§7.2), the approval is the security boundary (R-C2, R-G2), and ruling 1 says that the members come from one approved read; so a withdrawn approval now stops every member launch whose record holds a pin, passthrough included, by a check of the approvals file only (no git read), and a pin that is gone or unread, or a folder outside a git checkout, with the approval still valid, stops only a launch that makes the pinned read
- 2026-11-27 · R§19 PR1, PR2 · operator · the operator never made the "never by an agent" rule; the executor runs and records live checks, the operator approves the result.
- 2026-11-28 · R§6.1, R§6.2, R§7.2, R§7.4, R§8, R§10 `zygo up` and `zygo loadout list` rows, R§11, R§18 items 3, 4, 16 · author's call (FX-39, with its review) · the withdrawal check took the workspace id from the pane folder's checkout, so a member in `.git` or after a failed or slow git call skipped it and started from a withdrawn approval, and a member in a submodule or a nested repository stopped with nothing withdrawn; every definition step of a member launch now runs at the checkout of the workspace's main worktree, with the id computed there by git (not from the state file, W-C7), and stops when that checkout cannot be read; a pane folder that resolves into a git dir stops the launch (a committed link passes the text check); `cwd` refuses a `.git` part; the `.zygo/` listings are capped at 256 entries and each git call of the read at 131072 bytes of output, a listing past a cap is invalid and `zygo loadout list` reports it
