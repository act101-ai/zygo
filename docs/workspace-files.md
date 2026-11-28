# Workspace files (`.zygo/`)

A repository can define its workspace members and model profiles in files, so each person and each machine starts the same workspace. `.zygo/workspace.toml` defines the members. Each `.zygo/profiles/<name>.toml` defines one model profile. Nothing in these files applies on a machine until you approve them there with `zygo workspace approve` (spec `docs/specs/2026-11-24-workspace-definition-design.md`).

## Where zygo reads them

zygo reads the files from git objects on the default branch only. The default branch is the target of your local `refs/remotes/origin/HEAD`, as of your last fetch; with no `refs/remotes/origin/HEAD`, it is your local `refs/heads/main`. A target outside `refs/remotes/origin/` is not read (`origin/HEAD points outside refs/remotes/origin/`).

* zygo does not fetch. With `origin/HEAD`, a new commit is used only when your `refs/remotes/origin/<branch>` has it: after you push it from this clone, or after you fetch it. With no `origin/HEAD`, a commit on your local `main` is used at once.
* An edit in the working tree, and a commit on another branch, have no effect.
* `zygo doctor` shows the ref and the commit that it read: `workspace files: refs/remotes/origin/main 120e99a97b01 digest c63b23b7c352: approved`.

zygo reads only `.zygo/workspace.toml` and the `.toml` files in `.zygo/profiles/`, and only these files are in the digest that you approve. Other entries of `.zygo/`, and files in `.zygo/profiles/` whose names do not end in `.toml`, are not read and need no approval; `zygo workspace approve` lists them on its `ignored:` line.

## `.zygo/workspace.toml`

```toml
version = 1

[members.lead]
job = "orchestrator"

[members.ana]
profile = "rust"
pool = true

[members.ben]
pool = true
cwd = "branch:docs"
```

| Key | Value |
|---|---|
| `version` | Required, `1`. |
| `layout` | `even-horizontal`, `even-vertical`, `main-horizontal`, `main-vertical` or `tiled`. Default: `main-vertical` when there is an orchestrator, else `tiled`. |
| `[members.<name>]` | One table per member. The name is 1-32 characters of `a-z`, `0-9` and `-`, and starts with a letter. It is not `all`, `cli` or `zygo`, and not of the pane-key form `^k[a-z2-7]{8}$` (`k` and 8 characters of `a-z` and `2-7`, for example `kabcdefgh`). At most 32 members. |
| `kind` | `claude-code` (default) or `opencode`. |
| `job` | `orchestrator` or `worker` (default). At most one orchestrator. |
| `profile` | A profile name: 1-32 characters of `a-z`, `0-9` and `-`, not `none`. |
| `pool` | `true`: the member takes a login from your pool and runs in passthrough ([passthrough.md](passthrough.md#a-pool-of-logins-for-workspace-members)). `claude-code` only. With no pool configured, no effect. |
| `cwd` | The folder the member starts in. See [The `cwd` key](#the-cwd-key). |

With no `[members]`, the members are `orchestrator` and `worker-1` to `worker-4`, each `claude-code` with `pool = true`. The orchestrator is the first pane; the other members follow in name order. `[limits] max_members` in user or team config (1-32, default 8) caps the agents that the files start; `.zygo.toml` cannot set it. Above it, `zygo up` starts nothing: `zygo: this repository's workspace files start <n> agents; [limits] max_members is <m>; nothing started`.

### The `cwd` key

`cwd` is printable ASCII only (bytes `0x20`-`0x7e`). It must not start with `~` (a `~` later in the text is allowed), and it must not be an absolute path (a leading `/`, or a drive such as `C:`). It has one of two forms:

* `branch:<name>`: the worktree that has branch `<name>` checked out. `<name>` is 1-255 bytes, with no other check. When no worktree has that branch checked out, the member starts in the main worktree, and `zygo up` prints `` zygo: loadout repository: no worktree has branch `<name>` checked out; using the main worktree ``.
* A path relative to the main worktree: at most 1024 bytes in all; parts split by `/`, each part 1-255 bytes (so no empty part and no `/` at the end), and no part `.` or `..`; no `\`.

## `.zygo/profiles/<name>.toml`

```toml
overrides = { review = "claude-opus-5-5", code = "claude-sonnet-5-5" }
disabled = ["design"]
```

A profile file sets `overrides` (a role to a model id of your catalog, at most 16) and `disabled` (roles hidden from the lead, at most 16); at least one of them. Its name is the file name without `.toml`.

## Invalid files

One violation makes the files invalid as a whole, and nothing from them applies. zygo shows the first violation, for example `.zygo/workspace.toml: members.a.args: not allowed in a repository file`. The files are invalid when:

* a key is not in the tables above. A repository file cannot set a provider, a key, an endpoint, `args`, `config_dir`, `yolo`, `passthrough`, `server`, a prompt or a shell pane;
* a value breaks its rule, or a file is not valid TOML;
* `.zygo` is not a directory (`.zygo is not a directory`), or `.zygo/profiles` is not a directory;
* `.zygo/workspace.toml`, or a `.toml` entry in `.zygo/profiles/`, is not a regular file: a symlink, a directory or a submodule (`.zygo/profiles/link.toml is not a regular file`);
* a profile file name does not match `^[a-z0-9-]{1,32}\.toml$` (`.zygo/profiles/Rust.toml: a profile file name must match ^[a-z0-9-]{1,32}\.toml$`), or is `none.toml` (`.zygo/profiles/none.toml: the profile name none is reserved`);
* a file is not UTF-8 (`.zygo/profiles/rust.toml is not UTF-8`);
* `.zygo/workspace.toml` is larger than 64 KiB, a profile file is larger than 16 KiB, or `.zygo/profiles/` holds more than 32 profile files.

## Approve

`zygo workspace approve` shows the folder, the ref, the commit, the digest, each file line by line (each character other than printable ASCII as `\u{…}`, a tab as `\t`), the entries of `.zygo/` that zygo does not read, and a summary, then asks `approve these workspace files? [y/N]`:

```
$ zygo workspace approve --yes
folder: /home/me/src/app
ref: refs/remotes/origin/main
commit: 120e99a97b01ea58dddd1852435219f65a84d9ba
digest: c63b23b7c352fb19fb2dca3a3ba408acb4f922c5820dae47ef4d905450a5dead
.zygo/workspace.toml (140 bytes):
    version = 1
    …
.zygo/profiles/rust.toml (93 bytes):
    overrides = { review = "claude-opus-5-5", code = "claude-sonnet-5-5" }
    disabled = ["design"]
ignored: none
summary:
  agents: 3
  member lead: kind claude-code, job orchestrator, profile -, pool false, cwd -
  member ana: kind claude-code, job worker, profile rust, pool true, cwd -
  member ben: kind claude-code, job worker, profile -, pool true, cwd "branch:docs"
  profile rust: overrides code = "claude-sonnet-5-5", review = "claude-opus-5-5"; disabled design
  ana, ben run in passthrough on pool /home/me/.claude-interns; profile rust of ana does not apply in passthrough
approved c63b23b7c352fb19fb2dca3a3ba408acb4f922c5820dae47ef4d905450a5dead
to start it: zygo up (a workspace that exists already: zygo down, then zygo up --reset)
```

* `--yes` approves with no question and prints the listing on stdout, for scripts. It approves the files as they are when it runs: read the listing before you approve, and approve only files that you have read. `--digest <hex>` approves only when the files still have that digest; else it prints `zygo: the workspace files changed: their digest is <digest>, not <hex>; nothing approved` and exits 1, so a script approves exactly what you read.
* The last summary line shows only when your user config sets `[workspace] pool` and a member takes a pool directory. When `zygo up` would refuse that pool because its path cannot be expanded or `[server] url` is set, the line is the refusal and ends `; zygo up starts nothing`, for example `  pool /home/me/.claude-interns: passthrough is local mode only, and [server] url is set; zygo up starts nothing`. A pool that you set after the approval is not in the listing that you approved, but the pool members still run in passthrough on it at the next fresh `zygo up`.
* Files that are approved already: `workspace files already approved (<digest>)`. Absent, invalid or unread files: one line, exit 1, nothing approved. Any answer other than `y` or `yes`: `not approved; run zygo workspace approve`.
* Run it in your own shell; it works on native Windows too. Inside an agent session (`ZYGO_SESSION_DIR` set) it exits 1 with `zygo: approval is not available inside an agent session; run it in your own shell: zygo workspace approve`, and `/zygo:workspace approve` replies `run in your shell: zygo workspace approve`. This check guards against a mistake, not against a hostile agent: an agent that runs shell commands can remove the variable or write `approvals.toml` itself, and an agent session that zygo did not start has no such variable.
* The approval is for these exact bytes, in this repository, on this machine. A new commit that changes these files needs a new approval; files that you approved before apply again with no question.
* On Windows, workspaces run in WSL, so approve in WSL. An approval made with native Windows zygo is stored for the Windows path of the repository in `%LOCALAPPDATA%\zygo\approvals.toml`; it applies only to native Windows commands such as `zygo claude --profile`, not to `zygo up` in WSL.

### Withdraw an approval

No command withdraws an approval. Each approval is a `[[workspace]]` entry in `approvals.toml` (`~/.local/share/zygo/`; on Windows `%LOCALAPPDATA%\zygo\`):

```toml
[[workspace]]
id = "63ddc0a3688d370e"
digest = "c63b23b7c352fb19fb2dca3a3ba408acb4f922c5820dae47ef4d905450a5dead"
```

Find the entry by its `digest`: `zygo doctor` in the repository shows its first 12 characters. The `id` is an opaque hash; it does not show the repository's name. The file also holds your plugin approvals, and when it is not valid TOML, zygo approves no plugin and no workspace files. To remove one entry safely:

1. Make sure that no `zygo workspace approve` or `zygo plugins approve` runs.
2. Keep a copy: `cp ~/.local/share/zygo/approvals.toml ~/.local/share/zygo/approvals.toml.bak`.
3. In a text editor, delete the three lines of that entry (`[[workspace]]`, its `id` line and its `digest` line) and nothing else.
4. Run `zygo doctor` in the repository: the line ends `: not approved`. If anything else changed, restore the copy.

## How `zygo up` chooses the loadout

At a fresh build (a new workspace, or `zygo up --reset`), `zygo up` uses `--loadout <name>`; else the user `[workspace] default_loadout`, when your user config sets it; else the `repository` loadout, when the default branch has approved files with a `.zygo/workspace.toml`; else `solo`. A user `[loadout.repository]` is used in place of the files' loadout.

When `zygo up` comes to the files by that default and does not use them, it starts `solo` and prints one line:

* not approved: `zygo: this repository's workspace files (refs/remotes/origin/main, digest <12>) are not approved on this machine; nothing from them was applied; see zygo workspace approve`;
* invalid: `zygo: this repository's workspace files (<ref>) are invalid (<why>); nothing from them was applied`;
* unread (a git call failed or passed its time limit, or `origin/HEAD` points outside `refs/remotes/origin/`): `zygo: this repository's workspace files are unread (<why>); nothing from them was applied`.

Files with profiles only and no `.zygo/workspace.toml` define no loadout: when they are not approved, `zygo up` starts `solo` with no line.

When you choose `repository` yourself (`--loadout repository`, or `default_loadout = "repository"`) and your user config has no `[loadout.repository]`, the same cases exit 1 with the same line and start nothing. Approved files with no `.zygo/workspace.toml` give `zygo: this repository's workspace files at <ref> have no .zygo/workspace.toml; nothing started`, and no files give `zygo: this repository has no workspace files at <ref>`, each with exit 1. `zygo config check` decides `default_loadout = "repository"` as `zygo up` does, from the same read in the current folder. It accepts the setting, with no line, where `zygo up` plans the repository loadout: with approved files that hold `.zygo/workspace.toml`, or with a user `[loadout.repository]`. It checks the setting, not the plan: `zygo up` can still stop after the plan, when the files start more agents than `[limits] max_members` or when the plan does not resolve, and `config check` does not report that; `zygo doctor` reports the `max_members` case. Where `zygo up` refuses the files (no files, not approved, invalid, or profiles only), `config check` gives `error: workspace.default_loadout: unknown loadout` and exits 11. Outside a checkout, or when the files were not read, it gives one warning, `warning: workspace.default_loadout: no workspace files were read here; zygo up uses "repository" only from approved files that hold .zygo/workspace.toml`, and does not change the exit status.

`zygo up --all` reads no workspace files. With `default_loadout = "repository"` and no user `[loadout.repository]`, a workspace that it builds from a loadout starts `solo`, and zygo prints `zygo: up --all reads no repository workspace files; using solo`.

## Members keep their files

Each member keeps the files it started with: its record saves the commit and the digest (the pin), and a resume or a restore uses them, even after the default branch changes. New files apply after `zygo down`, then `zygo up --reset`. A restore reads no workspace files from the default branch.

The pin is checked only for a member that asks for a profile (its `profile`, or your default profile from `zygo profile use`) that only the workspace files define, not a user profile or a team preset, and that does not run in passthrough. For such a member, when the pinned commit is gone, its approval is withdrawn, or the member's folder is not in a git checkout, the member does not start, and zygo prints `zygo: profile <n> of this member comes from the workspace files at commit <12> (digest <12>), which this repository no longer has or no longer approves; the agent was not started; to start from the current files: zygo down, then zygo up --reset`. When the read of the pinned commit fails, the line says `which were not read (<why>)` in place of `which this repository no longer has or no longer approves`.

Every other member starts when its pinned commit is gone, its approval is withdrawn or the read of the pinned commit fails, also at a resume and at a restore: a member with no profile, a member whose profile is a user profile or a team preset, and a pool member in passthrough. zygo reads nothing at the pin for these members, because they take nothing from the pinned commit; their jobs and folders are in the saved workspace state.

## Profiles

An approved repository profile is one more profile: choose it with `--profile <name>`, `zygo profile use <name>`, or a member's `profile`. A user profile or a team preset of the same name wins. When the files are not approved, `zygo claude --profile rust` prints `zygo: profile rust is in this repository's workspace files, which are not approved on this machine (digest <12>); launching without it; see zygo workspace approve` and starts without it. Invalid files give `zygo: profile <n>: this repository's workspace files are invalid (<why>); launching without it`, and a failed read gives `zygo: profile <n>: this repository's workspace files were not read (<why>); launching without it`.

## What zygo shows

With the files above approved:

```
$ zygo profile list
  rust (workspace-files)
$ zygo loadout list
repository  workspace-files
solo  built-in
$ zygo config show --layer workspace-files
loadout.repository.window = [ … ]  # workspace-files .zygo/workspace.toml
profile.rust.disabled = ["design"]  # workspace-files .zygo/profiles/rust.toml
profile.rust.overrides.code = "claude-sonnet-5-5"  # workspace-files .zygo/profiles/rust.toml
profile.rust.overrides.review = "claude-opus-5-5"  # workspace-files .zygo/profiles/rust.toml
$ zygo doctor
…
workspace files: refs/remotes/origin/main 120e99a97b01 digest c63b23b7c352: approved
```

* Not approved: `zygo profile list`, `zygo config show` and `zygo loadout show repository` show nothing from the files and print `zygo: this repository's workspace files (refs/remotes/origin/main, digest <12>) are not approved on this machine; not shown; see zygo workspace approve` (`zygo loadout show repository` then exits 1). Invalid files give `zygo: this repository's workspace files (<ref>) are invalid (<why>); not shown`, and a failed read gives `zygo: this repository's workspace files were not read (<why>); not shown`.
* `zygo loadout list` shows `repository  workspace-files, not approved` when the files are not approved; invalid files with a `.zygo/workspace.toml` give `repository  workspace-files, invalid`; a user `[loadout.repository]` gives `repository  workspace-files, shadowed` after the `repository  user` row. A user profile of the same name gives `rust (workspace-files, shadowed)` after the `rust (user)` row.
* `zygo config check` checks the files, approved or not. A violation is one line, for example `warning: workspace files: .zygo/workspace.toml: members.a.args: not allowed in a repository file`, and the exit status does not change.
* `zygo doctor` prints `workspace files: <ref> <commit> digest <12>: approved` (or `: not approved`), `workspace files: invalid (<why>)`, `workspace files: none` or `workspace files: unread (<why>)`, and `workspace files: <n> agents; [limits] max_members is <m>` when the files start too many. It never fails because of them.
