//! `zygo ws forget <name>`, `zygo ws forget-pane <key> [--name <name>]` and the hidden `zygo ws snapshot` (W§12,
//! AU44). `zygo ws` does not sweep (AU71).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use zygo_core::paths::Paths;
use zygo_core::session_view::{ResumeNote, SubagentRef};
use zygo_hooks::workspace::snapshot_child;
use zygo_core::Agent;
use zygo_launch::claude_code::ClaudeCode;
use zygo_launch::env::Env;
use zygo_launch::launcher::AgentSession;
use zygo_launch::opencode::OpenCode;
use zygo_launch::AgentAdapter;
use zygo_workspace::admin;
use zygo_workspace::identity::{valid_pane_key, valid_ws_id};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::snapshot::{self, SkipReason, SnapshotOutcome, HOOK_WAIT, OP_WAIT};
use zygo_workspace::state::{CompactOverride, MemberRecord, Store};

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
    /// Delete a workspace that is not up: its state and its results
    Forget { name: String },
    /// Delete one pane of a workspace: its record and its results
    ForgetPane {
        key: String,
        /// Workspace name; default: the workspace of this folder
        #[arg(long)]
        name: Option<String>,
    },
}

/// The OP5 check first, for every subcommand, then the subcommand.
pub fn dispatch(args: WsArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let paths = Paths::from_env();
    match args.sub {
        WsCmd::Forget { name } => admin::forget(
            &paths,
            &tmux_program(),
            std::env::var_os("TMUX").as_deref(),
            &name,
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
        WsCmd::Snapshot { ws_id, event, force } => {
            snapshot_cmd(&paths, &ws_id, event.as_deref(), force);
            0
        }
        WsCmd::Run { ws_id, pane, shell } => run_cmd(&paths, &ws_id, &pane, shell),
    }
}

/// A path for a stderr line, escaped (C6).
fn shown(p: &Path) -> String {
    zygo_core::esc(&p.display().to_string())
}

/// `zygo ws run` (W§8.2): one pane of a workspace. The ids are checked first (AU84, exit 1); every other path ends in
/// the shell of step 8.
fn run_cmd(paths: &Paths, ws_id: &str, pane: &str, shell: bool) -> i32 {
    if !valid_ws_id(ws_id) {
        print_err(&format!("zygo: invalid workspace id {}\n", zygo_core::esc(ws_id)));
        return 1;
    }
    if !valid_pane_key(pane) {
        print_err(&format!("zygo: invalid pane key {}\n", zygo_core::esc(pane)));
        return 1;
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
        let resumable = if shell { None } else { rec.as_ref().and_then(|r| r.agent_session.as_deref().map(|id| (r.launch.kind, id))) };
        match resumable {
            // AU61: zygo does not resume the conversation in another folder.
            Some((kind, id)) => {
                let id = zygo_core::esc(id);
                print_err(&format!(
                    "zygo: {} is gone; agent session {id} not resumed; to resume it: cd {} && {}\n",
                    shown(&cwd),
                    shown(&cwd),
                    member::resume_command(kind, &id)
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
    launch_member(paths, store, ws_id, pane, &rec, &env, &cwd);
    exec_shell(env, ws_id, &cwd)
}

/// W§8.2 steps 2-7: the saved launch of `rec` (AU45, AU32), the resume of its agent session (W§8.3, AU3, AU61, D8),
/// the resume note inputs (W§9.4), then the member wrapper.
fn launch_member(paths: &Paths, store: Store, ws_id: &str, pane: &str, rec: &MemberRecord, env: &[(OsString, OsString)], cwd: &Path) {
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
            let d = ClaudeCode::default();
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
            let d = OpenCode::default();
            let saved = if resumed.is_some() { saved_program(&opts.env, d.program(), rec.program.as_deref(), cwd) } else { None };
            if let Some(saved) = saved {
                match program_folder_on_path(&mut opts.env, &paths.workspace_cache_dir(), ws_id, pane, &saved) {
                    Ok(()) => {}
                    Err(e) => print_err(&format!("zygo: {}\n", zygo_core::esc(&e.to_string()))),
                }
            }
            Box::new(d)
        }
    };
    opts.agent_session = resumed.clone().map(AgentSession::Resume);
    opts.resume_note = resume_note(rec, resumed.as_deref());
    let ctx = MemberCtx { ws_id: ws_id.to_string(), pane: pane.to_string(), store, launch: rec.launch.clone() };
    member::run_wrapped(adapter.as_ref(), opts, &ctx);
}

/// D8 (OpenCode): the program folder of `saved` at the end of the launch `PATH`.
#[cfg(unix)]
fn program_folder_on_path(env: &mut Env, cache: &Path, ws_id: &str, pane: &str, saved: &Path) -> std::io::Result<()> {
    let dir = program_folder(cache, ws_id, pane, saved)?;
    let old = env.get("PATH").map(OsStr::to_os_string).unwrap_or_default();
    let joined = std::env::join_paths(std::env::split_paths(&old).chain([dir])).map_err(std::io::Error::other)?;
    env.0.retain(|(k, _)| k != OsStr::new("PATH"));
    env.0.push(("PATH".into(), joined));
    Ok(())
}

/// D11: the program folder is unix only.
#[cfg(not(unix))]
fn program_folder_on_path(_env: &mut Env, _cache: &Path, _ws_id: &str, _pane: &str, _saved: &Path) -> std::io::Result<()> {
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

/// The `zygo claude` flags of a saved member launch (AU45): the saved fields, the AU32 compaction and, at a resume, no
/// `--config-dir` (AU61: `ws run` sets the saved `CLAUDE_CONFIG_DIR` itself).
pub(crate) fn launch_of(rec: &MemberRecord, resumed: bool) -> Launch {
    let l = &rec.launch;
    Launch {
        profile: l.profile.clone(),
        passthrough: l.passthrough,
        server: l.server.clone(),
        compact: effective_compact(rec),
        yolo: l.yolo,
        config_dir: if resumed { None } else { l.config_dir.clone() },
        rest: l.args.clone(),
    }
}

/// The `zygo opencode` flags of a saved member launch (AU45, AU32).
pub(crate) fn launch_open_code_of(rec: &MemberRecord) -> LaunchOpenCode {
    let l = &rec.launch;
    LaunchOpenCode {
        profile: l.profile.clone(),
        server: l.server.clone(),
        compact: effective_compact(rec),
        yolo: l.yolo,
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

/// AU61 (D8, OpenCode): the folder `<cache>/<ws_id>/program/<pane>` (mode 0700) that holds only the link `opencode`
/// to `saved`; an old entry of that name is replaced.
#[cfg(unix)]
pub(crate) fn program_folder(cache: &Path, ws_id: &str, pane: &str, saved: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = cache.join(ws_id).join("program").join(pane);
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
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
        let cache = t.path().join("cache");
        let dir = program_folder(&cache, "0123456789abcdef", "kaaaaaaaa", &saved).unwrap();
        assert_eq!(dir, cache.join("0123456789abcdef/program/kaaaaaaaa"));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, [OsString::from("opencode")]);
        assert_eq!(std::fs::read_link(dir.join("opencode")).unwrap(), saved);
        assert_eq!(program_folder(&cache, "0123456789abcdef", "kaaaaaaaa", &saved).unwrap(), dir); // again: the link is replaced
    }
}
