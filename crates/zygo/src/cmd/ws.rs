//! `zygo ws forget [--yes] <name>`, `zygo ws forget-pane <key> [--name <name>]`, `zygo ws name <key> <name> [--workspace <w>]`
//! (M§6) and the hidden `zygo ws snapshot` (W§12, AU44). `zygo ws` does not sweep (AU71).

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_core::session_view::{ResumeNote, SubagentRef};
use zygo_hooks::workspace::snapshot_child;
use zygo_core::Agent;
use zygo_launch::claude_code::ClaudeCode;
use zygo_launch::env::Env;
use zygo_launch::launcher::AgentSession;
use zygo_launch::opencode::OpenCode;
use zygo_launch::AgentAdapter;
use zygo_mail_sync::branch::remove_worktree;
use zygo_mail_sync::outbox;
use zygo_mail_sync::state::SyncPaths;
use zygo_workspace::admin;
use zygo_workspace::identity::{valid_pane_key, valid_ws_id};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::snapshot::{self, SkipReason, SnapshotOutcome, HOOK_WAIT, OP_WAIT};
use zygo_workspace::state::{CompactOverride, MemberRecord, Store, Workspace};

use super::member::{self, MemberCtx};
use super::{cwd_or_exit, op5_exit, print_err, tmux_program, Launch, LaunchOpenCode};

#[derive(clap::Args)]
pub struct WsArgs {
    #[command(subcommand)]
    pub sub: WsCmd,
}

#[derive(clap::Subcommand)]
pub enum WsCmd {
    /// Internal: the snapshot child started by the tmux hooks
    #[command(hide = true)]
    Snapshot {
        #[arg(long = "ws-id")]
        ws_id: String,
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Internal: one pane of a workspace
    #[command(hide = true)]
    Run {
        #[arg(long = "ws-id")]
        ws_id: String,
        #[arg(long)]
        pane: String,
        #[arg(long)]
        shell: bool,
    },
    /// Delete a workspace that is not up: its state, its results and its mail sync worktree
    Forget {
        name: String,
        /// Answer yes to the question about unsent mail
        #[arg(long)]
        yes: bool,
    },
    /// Delete one pane of a workspace: its record and its results
    ForgetPane {
        key: String,
        /// Workspace name; default: the workspace of this folder
        #[arg(long)]
        name: Option<String>,
    },
    /// Name one member of a workspace; mail stays in its pane-key folder
    Name {
        key: String,
        name: String,
        /// Workspace name; default: the workspace of this folder
        #[arg(long)]
        workspace: Option<String>,
    },
}

/// The OP5 check first, for every subcommand, then the subcommand.
pub fn dispatch(args: WsArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let paths = Paths::from_env();
    match args.sub {
        // S§7.6: the question goes to stderr through its own handle (the std stderr lock is reentrant).
        WsCmd::Forget { name, yes } => forget_cmd(
            &paths,
            &tmux_program(),
            std::env::var_os("TMUX").as_deref(),
            &name,
            yes,
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
            &mut std::io::stdout().lock(),
            &mut std::io::stderr().lock(),
        ),
        WsCmd::ForgetPane { key, name } => {
            // D10: the folder names the workspace only when there is no `--name`; `forget_pane` does not read `cwd`
            // when it has a name, so a folder that cannot be read does not fail that form.
            let cwd = if name.is_some() {
                std::path::PathBuf::new()
            } else {
                match cwd_or_exit() {
                    Ok(d) => d,
                    Err(code) => return code,
                }
            };
            admin::forget_pane(
                &paths,
                &tmux_program(),
                name.as_deref(),
                &cwd,
                &key,
                &mut std::io::stdout().lock(),
                &mut std::io::stderr().lock(),
            )
        }
        WsCmd::Name { key, name, workspace } => {
            let cwd = if workspace.is_some() {
                std::path::PathBuf::new()
            } else {
                match cwd_or_exit() {
                    Ok(d) => d,
                    Err(code) => return code,
                }
            };
            admin::rename_member(
                &paths,
                &tmux_program(),
                workspace.as_deref(),
                &cwd,
                &key,
                &name,
                &mut std::io::stdout().lock(),
                &mut std::io::stderr().lock(),
            )
        }
        WsCmd::Snapshot { ws_id, event, force } => {
            snapshot_cmd(&paths, &ws_id, event.as_deref(), force);
            0
        }
        WsCmd::Run { ws_id, pane, shell } => run_cmd(&paths, &ws_id, &pane, shell),
    }
}

/// S§7.6 (MS-21): `zygo ws forget` with its inputs and writers given (a test seam; `dispatch` calls it). `input` gives
/// the answer to the step 2 question, which goes to `ask`; `out` and `err` are the writers of `admin::forget`.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn forget_cmd(
    paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, name: &str, yes: bool, input: &mut dyn BufRead,
    ask: &mut dyn Write, out: &mut dyn Write, err: &mut dyn Write,
) -> i32 {
    let mut before = |ws: &Workspace| forget_mail(paths, ws, yes, input, ask);
    admin::forget(paths, tmux, tmux_env, name, &mut before, out, err)
}

/// S§6.3 Remove (MS-21): the deadline of `git worktree remove`. The worktree has no checkout (S§6.3 Make), so the
/// removal deletes its `.git` file, its index and git's entry; the same 10 s as the S§8.2 probe.
const WORKTREE_REMOVE_BOUND: Duration = Duration::from_secs(10);

/// S§7.6 step 2: the question of `zygo ws forget`: `n` messages in `mail/outbox/` and `r` in `mail/outbox/refused/`
/// (review F5).
pub(crate) fn forget_prompt(n: usize, r: usize) -> String {
    format!("zygo: {n} unsent messages and {r} refused messages will be deleted. Forget the workspace? [y/N]")
}

/// S§7.6 steps 2-3, S§6.3 Remove (MS-21): the `before_remove` step of `zygo ws forget`, which `admin::forget` runs
/// after its checks with `op.lock` and the sync lock held. It counts the outbox and its refused messages (a folder
/// that cannot be read is an `Err`); when either count is not 0 it asks [`forget_prompt`] on `err` (`yes`: the question and ` y (--yes)`), and an answer that
/// is not `y` or `yes`, or the end of input, is `Ok(false)`. Then [`remove_sync_worktree`]; a failed removal is an
/// `Err`. An `Err` is the reason that `admin::forget` prints after `zygo: cannot forget <name>: `.
pub(crate) fn forget_mail(
    paths: &Paths, ws: &Workspace, yes: bool, input: &mut dyn BufRead, err: &mut dyn Write,
) -> Result<bool, String> {
    let sp = SyncPaths::new(paths, &ws.id).map_err(|e| e.to_string())?;
    // S§7.6 step 1 (review F3): a `mail/` that is a link or not a folder is no mail, and it is not followed.
    let no_mail = std::fs::symlink_metadata(&sp.mail).is_ok_and(|m| !m.is_dir());
    let unreadable = |e: std::io::Error| format!("the outbox cannot be read: {}", esc(&e.to_string()));
    let (n, r) = if no_mail {
        (0, 0)
    } else {
        (outbox::count(&sp).map_err(unreadable)?, outbox::refused_count(&sp).map_err(unreadable)?)
    };
    if n > 0 || r > 0 {
        let prompt = forget_prompt(n, r);
        if yes {
            let _ = writeln!(err, "{prompt} y (--yes)");
        } else {
            let _ = write!(err, "{prompt} ");
            let _ = err.flush();
            if !super::plugins::confirm(input) {
                return Ok(false);
            }
        }
    }
    remove_sync_worktree(&sp, &ws.main_root)?;
    Ok(true)
}

/// S§6.3 Remove (MS-21): removes the sync worktree `<cache>/<id>/mail-sync`. No link is followed (review F2): a link
/// at `<cache>/<id>` or at `mail-sync` is removed as a link (`remove_file`) and no git runs, so git never acts on a
/// worktree that the link names. A real `mail-sync` folder gets `git worktree remove --force` when `main_root` is a
/// folder. What git does not remove (no `main_root` folder, git fails or times out, a regular file at `mail-sync`) goes
/// by the filesystem, as the sync's own recovery (`ensure_worktree`, review F4): `remove_dir_all` (it follows no link)
/// or `remove_file`, on that path inside `<cache>/<id>/` only. `<cache>/<id>` and `mail-sync` are read again with
/// `symlink_metadata` right before that removal (review R2-4): git ran for up to 10 s, and a link put at either
/// meanwhile is removed as a link. Only a failed filesystem removal is an `Err`, which names the folder.
fn remove_sync_worktree(sp: &SyncPaths, main_root: &Path) -> Result<(), String> {
    remove_sync_worktree_with(sp, &mut || {
        main_root.is_dir() && remove_worktree(main_root, &sp.worktree, Instant::now() + WORKTREE_REMOVE_BOUND).is_ok()
    })
}

/// [`remove_sync_worktree`] with the git step `git` (a test seam): `true` when `git worktree remove` ran and exited 0.
fn remove_sync_worktree_with(sp: &SyncPaths, git: &mut dyn FnMut() -> bool) -> Result<(), String> {
    let not_removed =
        |reason: &str| format!("the mail sync worktree {} was not removed: {}", shown(&sp.worktree), esc(reason));
    let id_dir = sp.worktree.parent().unwrap_or(&sp.worktree);
    let look = || worktree_at(id_dir, &sp.worktree).map_err(|e| not_removed(&e.to_string()));
    if matches!(look()?, At::Folder) && git() && std::fs::symlink_metadata(&sp.worktree).is_err() {
        return Ok(());
    }
    // Review R2-4: look again right before the removal; a link made while git ran is removed as a link.
    let gone = match look()? {
        At::Nothing => Ok(()),
        At::Link(p) => std::fs::remove_file(p),
        At::Folder => std::fs::remove_dir_all(&sp.worktree),
        At::File => std::fs::remove_file(&sp.worktree),
    };
    match gone {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(not_removed(&e.to_string())),
        _ => Ok(()),
    }
}

/// What is at the sync worktree path, read with `symlink_metadata` (no link followed).
enum At<'a> {
    /// No `<cache>/<id>` folder (W§12 removes a file there with the cache), or no `mail-sync`.
    Nothing,
    /// A link at `<cache>/<id>` or at `mail-sync`: this path.
    Link(&'a Path),
    /// A real `mail-sync` folder.
    Folder,
    /// Anything else at `mail-sync` (a regular file).
    File,
}

/// [`At`] for `<cache>/<id>` = `id_dir` and its `mail-sync` = `wt`; an error other than `NotFound` on `wt` is `Err`.
fn worktree_at<'a>(id_dir: &'a Path, wt: &'a Path) -> std::io::Result<At<'a>> {
    match std::fs::symlink_metadata(id_dir) {
        Ok(m) if m.file_type().is_symlink() => return Ok(At::Link(id_dir)),
        Ok(m) if m.is_dir() => {}
        _ => return Ok(At::Nothing),
    }
    match std::fs::symlink_metadata(wt) {
        Ok(m) if m.file_type().is_symlink() => Ok(At::Link(wt)),
        Ok(m) if m.is_dir() => Ok(At::Folder),
        Ok(_) => Ok(At::File),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(At::Nothing),
        Err(e) => Err(e),
    }
}

/// A path for a stderr line, escaped (C6).
fn shown(p: &Path) -> String {
    zygo_core::esc(&p.display().to_string())
}

/// `zygo ws run` (W§8.2): one pane of a workspace. The ids are checked first (AU84, exit 1); every other path ends in
/// the shell of step 8. The update check runs after the id checks and before the environment cleaning; on an
/// installed update `ws run` relaunches itself (D4).
fn run_cmd(paths: &Paths, ws_id: &str, pane: &str, shell: bool) -> i32 {
    if !valid_ws_id(ws_id) {
        print_err(&format!("zygo: invalid workspace id {}\n", zygo_core::esc(ws_id)));
        return 1;
    }
    if !valid_pane_key(pane) {
        print_err(&format!("zygo: invalid pane key {}\n", zygo_core::esc(pane)));
        return 1;
    }
    // The update check (auto-update design; W§8.2 step 1a), before the environment cleaning (AU64), so it sees
    // `ZYGO_WORKSPACE`. A relaunched `ws run` (`ZYGO_RELAUNCHED` set) does not check again, also when the cache was
    // not written. The binary path is read first: after an install, `/proc/self/exe` names a deleted file. When the
    // relaunch fails, `ws run` continues as a normal `ws run`, so the member starts and its record is not left
    // `running` without a launcher.
    let exe = std::env::current_exe().and_then(std::fs::canonicalize);
    // The zygo binary of every launch in this `ws run` (M5 round 3): the path read before the check, so hook commands
    // never name the deleted file after an install. Only when that read failed, the path of `current_exe`.
    let zygo = member::zygo_binary(&exe);
    if std::env::var_os(zygo_workspace::env::RELAUNCHED).is_none() && super::start_check(&[]) == Some(0) {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let e = match &exe {
            Ok(p) => relaunch(p, &args),
            Err(e) => std::io::Error::new(e.kind(), e.to_string()),
        };
        print_err(&format!("zygo: cannot relaunch zygo ws run: {}\n", zygo_core::esc(&e.to_string())));
    }
    // AU64: the environment of the pane; the process environment itself is not changed.
    let env = zygo_workspace::env::cleaned(std::env::vars_os().collect());
    let here = || std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let store = Store::open(paths, ws_id);
    let ws = match store.read() {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            print_err(&format!("zygo: no workspace with id {ws_id}\n"));
            return exec_shell(env, ws_id, &here());
        }
        Err(e) => {
            print_err(&format!("zygo: {}\n", zygo_core::esc(&e.to_string())));
            return exec_shell(env, ws_id, &here());
        }
    };
    let main_root = ws.main_root.clone();
    let Some(entry) = ws.panes.iter().find(|p| p.key == pane) else {
        print_err(&format!("zygo: pane {pane} is not in the workspace layout\n"));
        let _ = std::env::set_current_dir(&main_root);
        return exec_shell(env, ws_id, &main_root);
    };
    let cwd = entry.cwd.clone();
    let rec = ws.members.get(pane).cloned();
    if cwd.is_dir() {
        // AU55: the process cwd is the pane folder.
        let _ = std::env::set_current_dir(&cwd);
    } else {
        let _ = std::env::set_current_dir(&main_root);
        let resumable = if shell { None } else { rec.as_ref().and_then(|r| r.agent_session.as_deref().map(|id| (r, id))) };
        match resumable {
            // AU61: zygo does not resume the conversation in another folder. The command names each path and the id
            // as one shell word; the text before it escapes them (C6).
            Some((r, id)) => {
                let cmd = member::resume_command(r, id, r.config_dir.as_deref());
                print_err(&format!(
                    "zygo: {} is gone; agent session {} not resumed; to resume it: cd {} && {cmd}\n",
                    shown(&cwd),
                    zygo_core::esc(id),
                    member::quoted(&cwd),
                ));
            }
            None => print_err(&format!("zygo: {} is gone\n", shown(&cwd))),
        }
        return exec_shell(env, ws_id, &main_root);
    }
    if shell {
        return exec_shell(env, ws_id, &cwd);
    }
    let Some(rec) = rec else {
        print_err(&format!("zygo: pane {pane} has no member record; started a shell\n"));
        return exec_shell(env, ws_id, &cwd);
    };
    launch_member(paths, store, ws_id, pane, &rec, &env, &cwd, &zygo);
    exec_shell(env, ws_id, &cwd)
}

/// W§8.2 steps 2-7: the saved launch of `rec` (AU45, AU32), the resume of its agent session (W§8.3, AU3, AU61, D8),
/// the resume note inputs (W§9.4), then the member wrapper. `zygo` is the binary that the hooks and the plugin run.
#[allow(clippy::too_many_arguments)]
fn launch_member(
    paths: &Paths, store: Store, ws_id: &str, pane: &str, rec: &MemberRecord, env: &[(OsString, OsString)], cwd: &Path,
    zygo: &Path,
) {
    let kind = rec.launch.kind;
    let resumed = if zygo_workspace::args::has_session_arg(kind, &rec.launch.args) { None } else { rec.agent_session.clone() };
    let built = match kind {
        Agent::ClaudeCode => super::claude::launch_opts(&launch_of(rec, resumed.is_some()), Env(env.to_vec()), cwd.to_path_buf(), paths.clone()),
        Agent::OpenCode => super::opencode::launch_opts(&launch_open_code_of(rec), Env(env.to_vec()), cwd.to_path_buf(), paths.clone()),
    };
    let mut opts = match built {
        Ok(o) => o,
        Err(text) => {
            print_err(&format!("{text}\n"));
            return;
        }
    };
    let adapter: Box<dyn AgentAdapter> = match kind {
        Agent::ClaudeCode => {
            let d = ClaudeCode::for_zygo(zygo.to_path_buf());
            if resumed.is_some() {
                saved_config_dir(&mut opts.env, rec.config_dir.as_deref());
            }
            let saved = if resumed.is_some() { saved_program(&opts.env, &d.program, rec.program.as_deref(), cwd) } else { None };
            match saved.as_deref().and_then(Path::to_str) {
                Some(p) => Box::new(ClaudeCode::new(p, d.managed_settings, d.zygo)),
                None => Box::new(d),
            }
        }
        Agent::OpenCode => {
            let d = OpenCode::new(zygo.to_path_buf());
            let saved = if resumed.is_some() { saved_program(&opts.env, d.program(), rec.program.as_deref(), cwd) } else { None };
            if let Some(saved) = saved {
                member::gate("program");
                match program_folder_on_path(&mut opts.env, paths, ws_id, pane, &saved) {
                    Ok(()) => {
                        // W§12 (M2 review round 3 finding 4): a `ws forget` or `ws forget-pane` that ran after the
                        // read of step 1 may have removed the cache before the folder was made. The state is read
                        // again; when the state or the record is gone, the folder made here goes, and `ws run` ends
                        // as at step 1, with the shell and no launch.
                        if let Some((line, state_gone)) = forgotten(&store, ws_id, pane) {
                            remove_program_folder(paths, ws_id, pane, state_gone);
                            print_err(&line);
                            return;
                        }
                    }
                    Err(e) => print_err(&format!("zygo: {}\n", zygo_core::esc(&e.to_string()))),
                }
            }
            Box::new(d)
        }
    };
    opts.agent_session = resumed.clone().map(AgentSession::Resume);
    opts.resume_note = resume_note(rec, resumed.as_deref());
    // D1: `ws run` writes only a record that exists (no hand join).
    // M§9a, AU84: the record's member name when it passes the grammar.
    let name = rec.valid_name().map(String::from);
    let ctx = MemberCtx { ws_id: ws_id.to_string(), pane: pane.to_string(), store, launch: rec.launch.clone(), hand: None, name };
    member::run_wrapped(adapter.as_ref(), opts, &ctx);
}

/// W§12: the line of step 1 when the state of `ws_id` is gone (with `true`) or has no record for `pane`; `None` when
/// both are there or the state cannot be read (that is no forget; the callback reports it, W§8.2 step 4).
fn forgotten(store: &Store, ws_id: &str, pane: &str) -> Option<(String, bool)> {
    match store.read() {
        Ok(None) => Some((format!("zygo: no workspace with id {ws_id}\n"), true)),
        Ok(Some(ws)) if !ws.members.contains_key(pane) => {
            Some((format!("zygo: pane {pane} has no member record; started a shell\n"), false))
        }
        Ok(Some(_)) | Err(_) => None,
    }
}

/// W§12: removes the program folder of `pane` that this `ws run` made: its link `opencode`, then the folder; when the
/// state is gone, also `program` and `<ws_id>` when they are empty (they were made for it). A link is removed as a
/// link and never followed; an error other than a missing entry is one line.
fn remove_program_folder(paths: &Paths, ws_id: &str, pane: &str, state_gone: bool) {
    let top = paths.workspace_cache_dir().join(ws_id);
    let dir = top.join("program").join(pane);
    let report = |p: &Path, r: std::io::Result<()>| match r {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => print_err(&format!("zygo: {}: {}\n", shown(p), zygo_core::esc(&e.to_string()))),
        _ => {}
    };
    report(&dir.join("opencode"), std::fs::remove_file(dir.join("opencode")));
    report(&dir, std::fs::remove_dir(&dir));
    if state_gone {
        let _ = std::fs::remove_dir(top.join("program"));
        let _ = std::fs::remove_dir(&top);
    }
}

/// D8 (OpenCode): the program folder of `saved` at the end of the launch `PATH`.
#[cfg(unix)]
fn program_folder_on_path(env: &mut Env, paths: &Paths, ws_id: &str, pane: &str, saved: &Path) -> std::io::Result<()> {
    let dir = program_folder(paths, ws_id, pane, saved)?;
    let old = env.get("PATH").map(OsStr::to_os_string).unwrap_or_default();
    let joined = std::env::join_paths(std::env::split_paths(&old).chain([dir])).map_err(std::io::Error::other)?;
    env.0.retain(|(k, _)| k != OsStr::new("PATH"));
    env.0.push(("PATH".into(), joined));
    Ok(())
}

/// D11: the program folder is unix only.
#[cfg(not(unix))]
fn program_folder_on_path(_env: &mut Env, _paths: &Paths, _ws_id: &str, _pane: &str, _saved: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("needs unix"))
}

/// W§8.2 step 8: the default handling of SIGHUP, SIGTERM, SIGINT and SIGQUIT (AU88, D13), then `exec` of `$SHELL`
/// from `env` (else `/bin/sh`) in `cwd` with `env` and `ZYGO_WORKSPACE` (AU15, AU93; no `ZYGO_PANE`). Returns only
/// when the `exec` fails: one line and exit 1.
#[cfg(unix)]
fn exec_shell(env: Vec<(OsString, OsString)>, ws_id: &str, cwd: &Path) -> i32 {
    use std::os::unix::process::CommandExt;
    for s in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT, libc::SIGQUIT] {
        // SAFETY: `signal` with `SIG_DFL` changes only this process's disposition of `s`.
        unsafe {
            libc::signal(s, libc::SIG_DFL);
        }
    }
    let shell = env
        .iter()
        .find(|(k, _)| k == OsStr::new("SHELL"))
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from("/bin/sh"));
    let e = std::process::Command::new(&shell).env_clear().envs(env).env("ZYGO_WORKSPACE", ws_id).current_dir(cwd).exec();
    print_err(&format!("zygo: cannot start the shell {}: {}\n", shown(Path::new(&shell)), zygo_core::esc(&e.to_string())));
    1
}

/// Not reached: OP5 ends every workspace verb on Windows first.
#[cfg(not(unix))]
fn exec_shell(_env: Vec<(OsString, OsString)>, _ws_id: &str, _cwd: &Path) -> i32 {
    2
}

/// D4: `exec` of the updated binary `exe` with the `ws run` arguments `args`, the uncleaned environment and
/// `ZYGO_RELAUNCHED=1` (W§8.2 step 1a). Returns only when the `exec` fails.
#[cfg(unix)]
fn relaunch(exe: &Path, args: &[OsString]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(relaunch_target(exe)).args(args).env(zygo_workspace::env::RELAUNCHED, "1").exec()
}

/// The test seam (feature `test-hooks` only, as the member gate of D9): with `ZYGO_TEST_RELAUNCH_FAIL` set, the
/// relaunch executes a path that does not exist, so a test sees the `exec` failure.
#[cfg(all(unix, feature = "test-hooks"))]
fn relaunch_target(exe: &Path) -> PathBuf {
    if std::env::var_os("ZYGO_TEST_RELAUNCH_FAIL").is_some() {
        return PathBuf::from("/nonexistent/zygo-test-relaunch");
    }
    exe.to_path_buf()
}

/// A build without `test-hooks` has no seam.
#[cfg(all(unix, not(feature = "test-hooks")))]
fn relaunch_target(exe: &Path) -> PathBuf {
    exe.to_path_buf()
}

/// Not reached: OP5 ends every workspace verb on Windows first.
#[cfg(not(unix))]
fn relaunch(_exe: &Path, _args: &[OsString]) -> std::io::Error {
    std::io::ErrorKind::Unsupported.into()
}

/// The whole snapshot body runs through `snapshot_child` (H1 D15), so a body that fails queues `snapshot.pending`
/// again. An invalid id (AU84) and an error are one stderr line each; the exit code is always 0.
fn snapshot_cmd(paths: &Paths, ws_id: &str, event: Option<&str>, force: bool) {
    let mut outcome = None;
    let r = snapshot_child(paths, ws_id, force, &mut || {
        let socket = match Store::open(paths, ws_id).read() {
            Ok(Some(ws)) => ws.tmux_socket,
            _ => None,
        };
        let wait = if force { OP_WAIT } else { HOOK_WAIT };
        outcome = Some(snapshot::run(paths, &Tmux::new(tmux_program(), socket), ws_id, event, force, wait)?);
        Ok(())
    });
    match r {
        Ok(()) => {
            if matches!(outcome, Some(SnapshotOutcome::Skipped(SkipReason::InvalidId))) {
                print_err(&format!("zygo: invalid workspace id {}\n", zygo_core::esc(ws_id)));
            }
        }
        Err(e) => print_err(&format!("zygo: {}\n", zygo_core::esc(&e.to_string()))),
    }
}

/// The compaction window of a member launch (AU32): the override `set <w>` gives `w`, `cleared` gives none, no
/// override gives the saved `--compact`.
pub(crate) fn effective_compact(rec: &MemberRecord) -> Option<String> {
    match &rec.compact {
        Some(CompactOverride::Set(w)) => Some(w.clone()),
        Some(CompactOverride::Cleared) => None,
        None => rec.launch.compact.clone(),
    }
}

/// The resume note inputs of a member launch (W§8.2 step 4, W§9.4, AU72): none unless `note_pending`; bound to the
/// agent session that the launch resumes, none for a fresh start.
pub(crate) fn resume_note(rec: &MemberRecord, resumed: Option<&str>) -> Option<ResumeNote> {
    if !rec.note_pending {
        return None;
    }
    Some(ResumeNote {
        clean_stop: rec.clean_stop,
        interrupted: rec
            .interrupted
            .iter()
            .map(|s| SubagentRef { agent_id: s.agent_id.clone(), agent_type: s.agent_type.clone(), ..Default::default() })
            .collect(),
        closed_units: rec.closed_units.clone(),
        agent_session: resumed.map(String::from),
        ..Default::default()
    })
}

/// The `zygo claude` flags of a saved member launch (AU45): the saved fields, the AU32 compaction, at a resume no
/// `--config-dir` (AU61: `ws run` sets the saved `CLAUDE_CONFIG_DIR` itself), and the saved pin (R§7.4).
pub(crate) fn launch_of(rec: &MemberRecord, resumed: bool) -> Launch {
    let l = &rec.launch;
    Launch {
        profile: l.profile.clone(),
        passthrough: l.passthrough,
        server: l.server.clone(),
        compact: effective_compact(rec),
        yolo: l.yolo,
        config_dir: if resumed { None } else { l.config_dir.clone() },
        name: None,
        definition: l.definition.clone(),
        rest: l.args.clone(),
    }
}

/// The `zygo opencode` flags of a saved member launch (AU45, AU32), and the saved pin (R§7.4).
pub(crate) fn launch_open_code_of(rec: &MemberRecord) -> LaunchOpenCode {
    let l = &rec.launch;
    LaunchOpenCode {
        profile: l.profile.clone(),
        server: l.server.clone(),
        compact: effective_compact(rec),
        yolo: l.yolo,
        name: None,
        definition: l.definition.clone(),
        rest: l.args.clone(),
    }
}

/// AU61, PW16 (D8; Claude Code members, at a resume): removes `CLAUDE_CONFIG_DIR` from `env`, then sets it to `saved`
/// when there is one.
pub(crate) fn saved_config_dir(env: &mut Env, saved: Option<&Path>) {
    const VAR: &str = "CLAUDE_CONFIG_DIR";
    let same = |n: &OsStr| if cfg!(windows) { n.to_str().is_some_and(|n| n.eq_ignore_ascii_case(VAR)) } else { n == VAR };
    env.0.retain(|(k, _)| !same(k));
    if let Some(s) = saved {
        env.0.push((VAR.into(), s.as_os_str().to_os_string()));
    }
}

/// AU61 (D8, at a resume): `None` when the launch `PATH` has `program`; else `saved` when it is an existing file.
pub(crate) fn saved_program(env: &Env, program: &str, saved: Option<&Path>, cwd: &Path) -> Option<PathBuf> {
    if which::which_in(program, env.get("PATH"), cwd).is_ok() {
        return None;
    }
    saved.filter(|p| p.is_file()).map(Path::to_path_buf)
}

/// AU61 (D8, OpenCode): the folder `<workspace cache>/<ws_id>/program/<pane>` that holds only the link `opencode` to
/// `saved`; an old entry of that name is replaced. `admin::pane_cache_create` makes or opens each part with mode 0700
/// and follows no link (the rule of `ws forget-pane`); a refusal makes and deletes nothing.
#[cfg(unix)]
pub(crate) fn program_folder(paths: &Paths, ws_id: &str, pane: &str, saved: &Path) -> std::io::Result<PathBuf> {
    let dir = admin::pane_cache_create(paths, ws_id, "program", pane).map_err(std::io::Error::other)?;
    let link = dir.join("opencode");
    match std::fs::remove_file(&link) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::os::unix::fs::symlink(saved, &link)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use zygo_core::Agent;
    use zygo_launch::env::Env;
    use zygo_workspace::state::{CompactOverride, MemberRecord, Subagent};

    fn paths() -> Paths {
        let p = |s: &str| PathBuf::from(s);
        Paths {
            config_file: p("c"),
            cache_dir: p("k"),
            data_dir: p("d"),
            sessions_dir: p("s"),
            marketplaces_dir: p("m"),
            plugin_store: p("ps"),
            approvals_file: p("a"),
            usage_db: p("u"),
        }
    }

    #[test]
    fn effective_compact_has_three_states() { // AU32
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        r.launch.compact = Some("600000".into());
        assert_eq!(effective_compact(&r).as_deref(), Some("600000"));
        r.compact = Some(CompactOverride::Set("300000".into()));
        assert_eq!(effective_compact(&r).as_deref(), Some("300000"));
        r.compact = Some(CompactOverride::Cleared);
        assert_eq!(effective_compact(&r), None);
    }

    #[test]
    fn the_compact_argument_of_the_m4_values() { // AU32, AU105: the values M4 stores (M4.md:1129-1131)
        use zygo_proxy::client::WindowArg;
        for (o, want) in [
            (Some(CompactOverride::Set("off".into())), Some(WindowArg::Off)),
            (Some(CompactOverride::Set("600000".into())), Some(WindowArg::Tokens(600_000))),
            (Some(CompactOverride::Cleared), None),
        ] {
            let mut r = MemberRecord::new(Agent::ClaudeCode);
            r.launch.compact = Some("300000".into());
            r.compact = o.clone();
            let c = super::super::claude::launch_opts(&launch_of(&r, false), Env::default(), PathBuf::from("w"), paths()).unwrap();
            assert_eq!(c.compact, want, "{o:?}");
            let c = super::super::opencode::launch_opts(&launch_open_code_of(&r), Env::default(), PathBuf::from("w"), paths()).unwrap();
            assert_eq!(c.compact, want, "{o:?}");
        }
    }

    #[test]
    fn resume_note_only_with_note_pending_and_bound_to_the_resume_id() { // W§9.4, AU72, H1 seam a
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        assert_eq!(resume_note(&r, Some("abc")), None);
        r.note_pending = true;
        r.clean_stop = true;
        r.closed_units = vec!["u1".into()];
        r.interrupted = vec![Subagent { agent_id: "a1".into(), agent_type: "t1".into(), ..Default::default() }];
        let n = resume_note(&r, Some("abc")).unwrap();
        assert_eq!((n.clean_stop, n.closed_units.clone(), n.agent_session.as_deref()), (true, vec!["u1".to_string()], Some("abc")));
        assert_eq!((n.interrupted[0].agent_id.as_str(), n.interrupted[0].agent_type.as_str()), ("a1", "t1"));
        assert_eq!(resume_note(&r, None).unwrap().agent_session, None);
    }

    #[test]
    fn launch_of_maps_the_saved_fields() { // AU45, D8
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        r.launch.profile = Some("p".into());
        r.launch.passthrough = true;
        r.launch.server = Some("https://s.invalid".into());
        r.launch.compact = Some("600000".into());
        r.launch.yolo = true;
        r.launch.config_dir = Some("/c".into());
        r.launch.args = vec!["--model".into(), "lead".into()];
        let l = launch_of(&r, false);
        assert_eq!(
            (l.profile.as_deref(), l.passthrough, l.server.as_deref(), l.compact.as_deref(), l.yolo, l.config_dir.as_deref()),
            (Some("p"), true, Some("https://s.invalid"), Some("600000"), true, Some("/c"))
        );
        assert_eq!(l.rest, ["--model", "lead"]);
        assert_eq!(launch_of(&r, true).config_dir, None);
        let o = launch_open_code_of(&r);
        assert_eq!(
            (o.profile.as_deref(), o.server.as_deref(), o.compact.as_deref(), o.yolo, o.rest.clone()),
            (Some("p"), Some("https://s.invalid"), Some("600000"), true, vec!["--model".to_string(), "lead".to_string()])
        );
    }

    #[cfg(unix)]
    #[test]
    fn saved_config_dir_replaces_or_removes_the_variable() { // AU61, PW16 (Claude Code members only, D8)
        let mut e = Env(vec![("CLAUDE_CONFIG_DIR".into(), "/other".into()), ("PATH".into(), "/bin".into())]);
        saved_config_dir(&mut e, None);
        assert_eq!((e.get("CLAUDE_CONFIG_DIR"), e.get("PATH")), (None, Some(OsStr::new("/bin"))));
        saved_config_dir(&mut e, Some(Path::new("/saved")));
        assert_eq!(e.get("CLAUDE_CONFIG_DIR"), Some(OsStr::new("/saved")));
        assert_eq!(e.0.iter().filter(|(k, _)| k == "CLAUDE_CONFIG_DIR").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn saved_program_only_when_path_has_none() { // AU61, D8
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let exe = |d: &str| -> PathBuf {
            let dir = t.path().join(d);
            std::fs::create_dir_all(&dir).unwrap();
            let p = dir.join("claude");
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let (on_path, saved) = (exe("bin"), exe("saved"));
        let empty = t.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let e = Env(vec![("PATH".into(), on_path.parent().unwrap().into())]);
        assert_eq!(saved_program(&e, "claude", Some(&saved), t.path()), None);
        let e = Env(vec![("PATH".into(), empty.clone().into())]);
        assert_eq!(saved_program(&e, "claude", Some(&saved), t.path()), Some(saved.clone()));
        assert_eq!(saved_program(&e, "claude", Some(&t.path().join("gone/claude")), t.path()), None);
        assert_eq!(saved_program(&e, "claude", None, t.path()), None);
    }

    #[cfg(unix)]
    #[test]
    fn program_folder_holds_only_the_link() { // AU61, D8 (OpenCode)
        let t = tempfile::tempdir().unwrap();
        let saved = t.path().join("saved/opencode");
        std::fs::create_dir_all(saved.parent().unwrap()).unwrap();
        std::fs::write(&saved, "").unwrap();
        let mut p = paths();
        p.cache_dir = t.path().join("cache");
        let dir = program_folder(&p, "0123456789abcdef", "kaaaaaaaa", &saved).unwrap();
        assert_eq!(dir, p.workspace_cache_dir().join("0123456789abcdef/program/kaaaaaaaa"));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, [OsString::from("opencode")]);
        assert_eq!(std::fs::read_link(dir.join("opencode")).unwrap(), saved);
        assert_eq!(program_folder(&p, "0123456789abcdef", "kaaaaaaaa", &saved).unwrap(), dir); // again: the link is replaced
    }

    /// D4: the exec failure path. `relaunch` sets `ZYGO_RELAUNCHED`, so std points the process-wide `environ` at a
    /// temporary copy during the `exec` and frees it after the failure; a parallel test that spawns `git` by `PATH`
    /// then reads a freed `PATH` (FX-7). The child test runs the `exec` in its own process, with one test thread.
    #[cfg(unix)]
    #[test]
    fn relaunch_returns_the_exec_error() {
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cmd::ws::tests::relaunch_exec_error_in_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env("ZYGO_TEST_CHILD", "cmd::ws::tests::relaunch_exec_error_in_child")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success(), "{text}");
        assert!(text.contains("test result: ok. 1 passed"), "{text}");
        assert!(text.contains("child ran: relaunch_exec_error_in_child"), "{text}");
    }

    /// The child of `relaunch_returns_the_exec_error`: the failed `exec` swaps `environ`, so it runs alone. Outside
    /// its parent (`--include-ignored`) it does nothing.
    #[cfg(unix)]
    #[test]
    #[ignore = "child process of relaunch_returns_the_exec_error"]
    fn relaunch_exec_error_in_child() {
        if std::env::var("ZYGO_TEST_CHILD").as_deref() != Ok("cmd::ws::tests::relaunch_exec_error_in_child") {
            return;
        }
        let e = super::relaunch(Path::new("/nonexistent/zygo-m5"), &[OsString::from("ws")]);
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        println!("child ran: relaunch_exec_error_in_child");
    }

    #[test] // M§6: `zygo ws name <key> <name> [--workspace <w>]`
    fn ws_name_parses_key_name_and_workspace() {
        use clap::Parser;
        use super::super::{Cli, Cmd};
        let Cmd::Ws(WsArgs { sub: WsCmd::Name { key, name, workspace } }) =
            Cli::try_parse_from(["zygo", "ws", "name", "kaaaaaaaa", "elena", "--workspace", "app"]).unwrap().command
        else { panic!() };
        assert_eq!((key.as_str(), name.as_str(), workspace.as_deref()), ("kaaaaaaaa", "elena", Some("app")));
        let Cmd::Ws(WsArgs { sub: WsCmd::Name { workspace, .. } }) =
            Cli::try_parse_from(["zygo", "ws", "name", "kaaaaaaaa", "elena"]).unwrap().command
        else { panic!() };
        assert_eq!(workspace, None);
        assert!(Cli::try_parse_from(["zygo", "ws", "name", "kaaaaaaaa"]).is_err());
    }

    #[test]
    fn launch_of_passes_the_saved_pin() { // R§7.4 bullet 3, R§10 `ws run`
        use zygo_config::repo_def::Pin;
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        assert_eq!(launch_of(&r, false).definition, None);
        let pin = Pin::new("a".repeat(40), "b".repeat(64));
        r.launch.definition = Some(pin.clone());
        assert_eq!(launch_of(&r, false).definition.as_ref(), Some(&pin));
        assert_eq!(launch_of(&r, true).definition.as_ref(), Some(&pin));
        assert_eq!(launch_open_code_of(&r).definition.as_ref(), Some(&pin));
        let c = super::super::claude::launch_opts(&launch_of(&r, true), Env::default(), PathBuf::from("w"), paths()).unwrap();
        let o = super::super::opencode::launch_opts(&launch_open_code_of(&r), Env::default(), PathBuf::from("w"), paths()).unwrap();
        assert_eq!((c.definition.as_ref(), o.definition.as_ref()), (Some(&pin), Some(&pin)));
        assert!(c.definition_out.is_none() && o.definition_out.is_none());
    }

    #[test] // S§7.6 step 2 (MS-21, review F5): the question text, with both counts
    fn forget_prompt_is_the_spec_text() {
        assert_eq!(
            forget_prompt(2, 3),
            "zygo: 2 unsent messages and 3 refused messages will be deleted. Forget the workspace? [y/N]"
        );
    }

    #[cfg(unix)]
    const WS: &str = "0123456789abcdef";

    /// The `Paths` of the scratch folder `t` (state under `t/d/state`, cache under `t/k`).
    #[cfg(unix)]
    fn temp_paths(t: &Path) -> Paths {
        Paths {
            config_file: t.join("c.toml"),
            cache_dir: t.join("k"),
            data_dir: t.join("d"),
            sessions_dir: t.join("k/s"),
            marketplaces_dir: t.join("k/m"),
            plugin_store: t.join("d/p"),
            approvals_file: t.join("d/a.toml"),
            usage_db: t.join("d/u.db"),
        }
    }

    /// `forget_mail` with `input`: its result, what it wrote on `err`, and how many input bytes it read.
    #[cfg(unix)]
    fn ask(p: &Paths, ws: &Workspace, yes: bool, input: &str) -> (Result<bool, String>, String, u64) {
        let mut cur = std::io::Cursor::new(input.as_bytes().to_vec());
        let mut err = Vec::new();
        let r = forget_mail(p, ws, yes, &mut cur, &mut err);
        (r, String::from_utf8(err).unwrap(), cur.position())
    }

    #[cfg(unix)]
    #[test] // S§7.6 step 2 (MS-21): an empty outbox asks nothing; 2 files ask; n, end of input, y, --yes
    fn forget_mail_asks_only_for_a_non_empty_outbox() {
        let td = tempfile::tempdir().unwrap();
        let p = temp_paths(td.path());
        let ws = Workspace::new(WS); // main_root empty: no git
        assert_eq!(ask(&p, &ws, false, "n\n"), (Ok(true), String::new(), 0));
        let outbox = zygo_mail_sync::state::SyncPaths::new(&p, WS).unwrap().outbox;
        std::fs::create_dir_all(&outbox).unwrap();
        for f in ["01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "01ARZ3NDEKTSV4RRFFQ69G5FAW.json"] {
            std::fs::write(outbox.join(f), b"{}").unwrap();
        }
        let q = "zygo: 2 unsent messages and 0 refused messages will be deleted. Forget the workspace? [y/N]";
        assert_eq!(ask(&p, &ws, false, "n\n"), (Ok(false), format!("{q} "), 2));
        assert_eq!(ask(&p, &ws, false, ""), (Ok(false), format!("{q} "), 0));
        assert_eq!(ask(&p, &ws, false, "y\n"), (Ok(true), format!("{q} "), 2));
        assert_eq!(ask(&p, &ws, true, "n\n"), (Ok(true), format!("{q} y (--yes)\n"), 0));
    }

    #[cfg(unix)]
    #[test] // S§7.6 step 2 (MS-21 review F5): an empty outbox with refused messages asks too, with both counts
    fn forget_mail_asks_for_refused_messages() {
        let td = tempfile::tempdir().unwrap();
        let p = temp_paths(td.path());
        let ws = Workspace::new(WS);
        let refused = zygo_mail_sync::state::SyncPaths::new(&p, WS).unwrap().refused;
        std::fs::create_dir_all(&refused).unwrap();
        for f in ["01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "01ARZ3NDEKTSV4RRFFQ69G5FAW.json", "01ARZ3NDEKTSV4RRFFQ69G5FAX.json"] {
            std::fs::write(refused.join(f), b"{}").unwrap();
        }
        let q = "zygo: 0 unsent messages and 3 refused messages will be deleted. Forget the workspace? [y/N]";
        assert_eq!(ask(&p, &ws, false, "n\n"), (Ok(false), format!("{q} "), 2));
        assert_eq!(ask(&p, &ws, true, ""), (Ok(true), format!("{q} y (--yes)\n"), 0));
    }

    #[cfg(unix)]
    #[test] // S§6.3 Remove, S§7.6 steps 2-3 (MS-21, review F4): a folder git cannot remove goes by the filesystem; only a
            // failed filesystem removal refuses, with the folder named; an unreadable outbox refuses
    fn forget_mail_removes_what_git_cannot_and_refuses_an_unreadable_outbox() {
        use std::os::unix::fs::PermissionsExt;
        let td = tempfile::tempdir().unwrap();
        let p = temp_paths(td.path());
        let sp = zygo_mail_sync::state::SyncPaths::new(&p, WS).unwrap();
        let mut ws = Workspace::new(WS);
        ws.main_root = td.path().join("plain"); // a folder that is no repository: git fails there
        std::fs::create_dir_all(&ws.main_root).unwrap();
        assert_eq!(ask(&p, &ws, false, ""), (Ok(true), String::new(), 0)); // no worktree folder: nothing to do
        for root in ["moved", "plain"] {
            ws.main_root = td.path().join(root); // no such folder (no git), then no repository (git fails)
            std::fs::create_dir_all(sp.worktree.join("sub")).unwrap();
            assert_eq!(ask(&p, &ws, false, ""), (Ok(true), String::new(), 0), "{root}");
            assert!(std::fs::symlink_metadata(&sp.worktree).is_err(), "{root}");
        }
        std::fs::create_dir_all(sp.worktree.join("sub")).unwrap();
        std::fs::set_permissions(&sp.worktree, std::fs::Permissions::from_mode(0o500)).unwrap();
        let r = ask(&p, &ws, false, "").0;
        std::fs::set_permissions(&sp.worktree, std::fs::Permissions::from_mode(0o700)).unwrap();
        let prefix = format!("the mail sync worktree {} was not removed: ", sp.worktree.display());
        assert!(r.as_ref().is_err_and(|e| e.starts_with(&prefix) && e.contains("ermission denied")), "{r:?}");
        std::fs::create_dir_all(&sp.mail).unwrap();
        std::fs::write(&sp.outbox, b"").unwrap();
        let unreadable = format!("the outbox cannot be read: {} is not a folder", sp.outbox.display());
        assert_eq!(ask(&p, &ws, true, "").0, Err(unreadable));
    }

    #[cfg(unix)]
    #[test] // S§6.3 Remove (MS-21 review R2-4): `<cache>/<id>` or `mail-sync` made a link while git ran: the filesystem
            // removal removes the link only, never what it names
    fn remove_after_git_removes_a_link_made_during_git_and_not_its_target() {
        let td = tempfile::tempdir().unwrap();
        let p = temp_paths(td.path());
        let sp = zygo_mail_sync::state::SyncPaths::new(&p, WS).unwrap();
        let id_dir = sp.worktree.parent().unwrap().to_path_buf();
        let other = td.path().join("other");
        for swap_id_dir in [true, false] {
            std::fs::create_dir_all(sp.worktree.join("sub")).unwrap();
            std::fs::create_dir_all(other.join("mail-sync/keep")).unwrap();
            let (at, to) = if swap_id_dir { (id_dir.clone(), other.clone()) } else { (sp.worktree.clone(), other.join("mail-sync")) };
            // The git step fails, and while it runs a same-user process puts a link at `at`.
            let mut git = || {
                std::fs::remove_dir_all(&at).unwrap();
                std::os::unix::fs::symlink(&to, &at).unwrap();
                false
            };
            assert_eq!(remove_sync_worktree_with(&sp, &mut git), Ok(()), "swap <cache>/<id>: {swap_id_dir}");
            assert!(std::fs::symlink_metadata(&at).is_err(), "swap <cache>/<id>: {swap_id_dir}: the link stays");
            assert!(other.join("mail-sync/keep").is_dir(), "swap <cache>/<id>: {swap_id_dir}: the target was removed");
            std::fs::create_dir_all(&id_dir).unwrap();
        }
    }
}
