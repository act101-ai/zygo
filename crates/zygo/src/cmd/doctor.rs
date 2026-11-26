use std::ffi::{OsStr, OsString};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use semver::Version;
use zygo_config::keys::{default_store, error_kind, resolve_with, SecretStore};
use zygo_config::repo_def::{
    check_default, repository_loadout, short, GitRunner, SystemGit, Use, REPOSITORY_LOADOUT,
};
use zygo_config::{load, KeyRef, Layer, LoadContext, LoadoutPane, Merged, BUILTIN_LOADOUT};
use zygo_core::exit::ExitCode;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_hooks::statusline::render::{glyphs, icon_set, IconSet};
use zygo_mail_sync::identity::Stop;
use zygo_plugins::check::esc;
use zygo_plugins::select::selection;
use zygo_plugins::snapshot::missing_commands;
use zygo_plugins::store::{state, ManifestState};
use zygo_work::policy::Policy;
use zygo_workspace::args::has_user_info;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::loadout::builtin_solo;
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::mux::{Multiplexer, MuxError, MIN_VERSION};
use zygo_workspace::pool;
use zygo_workspace::up::{find_checkout, GIT_BOUND};

use super::status::repo_top;
use super::verbs::VerbCtx;
use super::{print_err, print_out};

/// What running `<program> --version` gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    NotFound,
    Failed,
    Output(String),
}

pub struct Env<'a> {
    pub paths: &'a Paths,
    pub repo: Option<&'a Path>,
    pub search_path: Option<OsString>,
    pub claude_config_dir: Option<PathBuf>,
    pub secrets: &'a dyn SecretStore,
    pub env: &'a dyn Fn(&str) -> Option<OsString>,
    pub probe: &'a dyn Fn(&str) -> Probe,
    pub server: Option<String>,
    /// The version of the first `tmux` in the absolute entries of `PATH` (W§12, AU7; D2, D9); `Err(Spawn)` of kind
    /// `NotFound` when there is none.
    pub tmux: Result<(u32, u32), MuxError>,
}

/// The `act_lite` version of the compiled language list, without the leading `v` (§10.5).
pub fn compiled_act_lite_version() -> String {
    let v: serde_json::Value = serde_json::from_str(include_str!("../../../../fixtures/act-lite/languages.json"))
        .expect("the act-lite fixture is JSON");
    v["act_lite"].as_str().unwrap_or_default().trim_start_matches('v').to_string()
}

/// The first token of the first output line that is a semver version (after a leading `v`); never other probe text.
fn parsed_version(p: &Probe) -> Option<Version> {
    let Probe::Output(text) = p else { return None };
    text.lines().next()?.split_whitespace().find_map(|t| Version::parse(t.trim_start_matches('v')).ok())
}

/// The most a probe may take (§10.5 gives it for act-lite; the agents use the same bound).
const PROBE_BOUND: Duration = Duration::from_secs(2);
/// The most probe output read.
const PROBE_CAP: usize = 4096;

/// The absolute entries of `search_path`, rejoined; a relative or empty entry would resolve inside the current directory.
pub fn absolute_entries(search_path: Option<&OsStr>) -> Option<OsString> {
    let v: Vec<PathBuf> = std::env::split_paths(search_path?).filter(|p| p.is_absolute()).collect();
    if v.is_empty() { None } else { std::env::join_paths(v).ok() }
}

/// Kills the child (on unix its whole process group, so no program it started survives) and reaps it; the exit status
/// of a child that had already exited is kept.
fn kill_and_reap(child: &mut Child) -> Option<std::process::ExitStatus> {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `kill` takes plain integers; the child leads its own group (`process_group(0)`) and is not reaped yet
        // (`has_exited` uses `WNOWAIT`), so the group id cannot name another process.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    let _ = child.kill();
    child.wait().ok()
}

/// Whether the child has exited. On unix it is not reaped (`WNOWAIT`): the zombie keeps its pid, and so its process
/// group id, reserved until `kill_and_reap` has signalled the group.
#[cfg(unix)]
fn has_exited(child: &mut Child) -> std::io::Result<bool> {
    let pid = libc::id_t::from(child.id());
    loop {
        // SAFETY: `info` is a zeroed `siginfo_t` that `waitid` may fill; the other arguments are plain integers.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
        if r == 0 {
            // With `WNOHANG` and no state change the zeroed `info` stays zero.
            return Ok(info.si_signo == libc::SIGCHLD);
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

#[cfg(not(unix))]
fn has_exited(child: &mut Child) -> std::io::Result<bool> {
    child.try_wait().map(|s| s.is_some())
}

/// The first non-blank line of `r`, trimmed, or the first `PROBE_CAP` bytes when no such line ends before them.
fn first_line(r: &mut impl Read) -> Vec<u8> {
    let mut seen = Vec::new();
    let mut buf = [0u8; 512];
    while seen.len() < PROBE_CAP {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen.extend_from_slice(&buf[..n]);
                // Only lines whose end has been read count.
                let ended = seen.iter().rposition(|b| *b == b'\n').map_or(&[][..], |end| &seen[..end]);
                if let Some(line) = ended.split(|b| *b == b'\n').find(|l| !l.trim_ascii().is_empty()) {
                    return line.trim_ascii().to_vec();
                }
            }
        }
    }
    seen.truncate(PROBE_CAP);
    seen.trim_ascii().to_vec()
}

/// Runs `<program> --version` found in the absolute entries of `search_path`, for at most `bound`.
pub fn probe_version(program: &str, search_path: Option<&OsStr>, bound: Duration) -> Probe {
    let deadline = Instant::now() + bound;
    let Some(entries) = absolute_entries(search_path) else { return Probe::NotFound };
    let Ok(found) = which::which_in(program, Some(entries), std::env::current_dir().unwrap_or_default()) else {
        return Probe::NotFound;
    };
    if bound.is_zero() {
        return Probe::Failed;
    }
    let mut cmd = Command::new(found);
    cmd.arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).stdout(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let Ok(mut child) = cmd.spawn() else { return Probe::Failed };
    let Some(mut stdout) = child.stdout.take() else {
        kill_and_reap(&mut child);
        return Probe::Failed;
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // The first line is sent as soon as it is read: a program it started may hold the pipe open past its own exit.
        let _ = tx.send(first_line(&mut stdout));
        // Drain the rest so the program is not killed by a closed pipe; the thread ends when the process group does.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    });
    let exited = loop {
        match has_exited(&mut child) {
            Ok(true) => break true,
            Ok(false) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => break false,
        }
    };
    // After every outcome nothing the probe started is left running.
    let status = kill_and_reap(&mut child);
    if !exited || !status.is_some_and(|s| s.success()) {
        return Probe::Failed;
    }
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(line) => Probe::Output(String::from_utf8_lossy(&line).trim().to_string()),
        Err(_) => Probe::Failed,
    }
}

/// Whether a `MuxError::Spawn` message is the text of an `io::Error` of kind `NotFound` (the tmux runner keeps only
/// `e.to_string()`): the text of the plain kind, or an OS error text `<detail> (os error <code>)` whose code maps to
/// `NotFound` and whose whole text is the text of that code.
fn spawn_not_found(message: &str) -> bool {
    let kind = std::io::ErrorKind::NotFound;
    if message == std::io::Error::from(kind).to_string() {
        return true;
    }
    let code = message.strip_suffix(')').and_then(|m| m.rsplit_once(" (os error ")).and_then(|(_, c)| c.parse::<i32>().ok());
    code.map(std::io::Error::from_raw_os_error).is_some_and(|e| e.kind() == kind && e.to_string() == message)
}

pub fn report(env: &Env, merged: &Merged) -> Vec<String> {
    let mut out = Vec::new();
    for (agent, program) in [(Agent::ClaudeCode, "claude"), (Agent::OpenCode, "opencode")] {
        let min = zygo_config::builtin::min_agent_version(agent);
        let name = agent.as_str();
        let probe = (env.probe)(program);
        out.push(match (&probe, parsed_version(&probe)) {
            (Probe::NotFound, _) => format!("agent {name}: not found"),
            (_, None) => format!("agent {name}: unknown (minimum {min})"),
            (_, Some(v)) if v < min => format!("agent {name}: {v} (minimum {min}; below minimum)"),
            (_, Some(v)) => format!("agent {name}: {v} (minimum {min})"),
        });
    }
    let compiled = compiled_act_lite_version();
    let probe = (env.probe)("act-lite");
    out.push(match (&probe, parsed_version(&probe)) {
        (Probe::NotFound, _) => "act-lite: not found".to_string(),
        (_, None) => "act-lite: unknown".to_string(),
        (_, Some(v)) if v.to_string() != compiled => format!("act-lite: {v} (list compiled for {compiled}; version mismatch)"),
        (_, Some(v)) => format!("act-lite: {v} (list compiled for {compiled})"),
    });
    let (min_major, min_minor) = MIN_VERSION;
    out.push(match &env.tmux {
        Ok(v) if *v < MIN_VERSION => format!("tmux: {}.{} (minimum {min_major}.{min_minor}; below minimum)", v.0, v.1),
        Ok(v) => format!("tmux: {}.{} (minimum {min_major}.{min_minor})", v.0, v.1),
        Err(MuxError::Spawn { message }) if spawn_not_found(message) => "tmux: not found".to_string(),
        Err(MuxError::Spawn { message }) => format!("tmux: cannot run: {}", esc(message)),
        Err(_) => format!("tmux: unknown (minimum {min_major}.{min_minor})"),
    });
    out.push(format!("secret store: {}", env.secrets.backend()));
    // The value is read to learn that it is present and is never printed.
    let keys: BTreeMap<(String, String), Option<&KeyRef>> = merged.config.catalog.0.iter().flat_map(|m| &m.provider)
        .filter(|p| p.enabled).map(|p| ((p.name.clone(), p.key.as_ref().map(ToString::to_string).unwrap_or_default()), p.key.as_ref())).collect();
    for ((name, key), r) in keys {
        let state = match r {
            None => "no key".to_string(),
            Some(r) => match resolve_with(r, env.secrets, env.env) {
                Ok(Some(_)) => format!("{} set", esc(&key)),
                Ok(None) => format!("{} unset", esc(&key)),
                Err(e) => format!("{} unreadable ({})", esc(&key), error_kind(&e)),
            },
        };
        out.push(format!("provider {}: {state}", esc(&name)));
    }
    out.push(match &env.server {
        None => "server: local mode".to_string(),
        Some(url) => format!("server: {}", esc(url)),
    });
    plugin_lines(env, merged, &mut out);
    out.push(icon_line(icon_set(env.env)));
    // `describe` prints the repo path as is; every line of it is escaped (D10).
    out.push(esc(&Policy::resolve(merged, env.repo.unwrap_or(Path::new(""))).describe()));
    out
}

/// R§10 `zygo doctor`: the workspace files of the default branch (only where `zygo up` finds a checkout), the member maximum of their
/// repository loadout, the `[workspace] pool` directory, and the pool directories that the loadout `zygo up` would
/// choose shares. Never fails: each case is a line. R-C6: the pool is listed by names and types only, no file opened.
pub fn definition_lines(cwd: &Path, merged: &Merged, approvals: &Path, git: &dyn GitRunner) -> Vec<String> {
    let mut out = Vec::new();
    // The checkout of `zygo up` (a bare repository's folder and `.git` give the main worktree); an `Err` is up's
    // "not a repository": no workspace-files line.
    let found = find_checkout(cwd, GIT_BOUND)
        .ok()
        .map(|(co, ..)| check_default(&co.top, &workspace_id(&co.common_dir), approvals, git));
    // R§10 `up` row: `[workspace] default_loadout` chooses only when the user config file sets it.
    let user_default =
        (merged.source_of("workspace.default_loadout") == Layer::User).then_some(&merged.config.workspace.default_loadout);
    let c12 = |c: &str| c.get(..12).unwrap_or(c).to_string();
    let def = match &found {
        None => None,
        Some(Use::Approved { full_ref, pin, def, .. }) => {
            out.push(format!("workspace files: {} {} digest {}: approved", esc(full_ref), c12(&pin.commit), short(&pin.digest)));
            Some(def)
        }
        Some(Use::NotApproved { full_ref, files, digest, def }) => {
            out.push(format!("workspace files: {} {} digest {}: not approved", esc(full_ref), c12(&files.commit), short(digest)));
            Some(def)
        }
        Some(Use::Invalid { why, .. }) => {
            out.push(format!("workspace files: invalid ({})", esc(why)));
            None
        }
        Some(Use::Absent { .. }) => {
            out.push("workspace files: none".to_string());
            None
        }
        Some(Use::Unread { why }) => {
            out.push(format!("workspace files: unread ({})", esc(why)));
            None
        }
    };
    // R§9.4 applies to the repository loadout only: not when the user chooses another loadout, and not when a user
    // `[loadout.repository]` is used in its place (R§10 `up` row).
    let repository_chosen = user_default.is_none_or(|n| n == REPOSITORY_LOADOUT);
    let used = repository_chosen && !merged.config.loadout.contains_key(REPOSITORY_LOADOUT);
    if let Some(lo) = def.and_then(repository_loadout).filter(|_| used) {
        let n = lo.window.iter().flat_map(|w| &w.pane).filter(|p| p.kind != "shell").count();
        let m = merged.config.limits.max_members;
        if n > m as usize {
            out.push(format!("workspace files: {n} agents; [limits] max_members is {m}"));
        }
    }
    let Some(text) = &merged.config.workspace.pool else { return out };
    let dir = match pool::expand(text, pool::home().as_deref()) {
        Ok(d) => d,
        Err(why) => {
            out.push(format!("pool: {}: {}", esc(text), esc(&why)));
            return out;
        }
    };
    let shown = esc(&dir.to_string_lossy());
    let dirs = match pool::list(&dir) {
        Ok(d) => d,
        Err(why) => {
            out.push(format!("pool: {shown}: {}", esc(&why)));
            return out;
        }
    };
    out.push(format!("pool: {shown}: {} directories", dirs.len()));
    // The loadout of `zygo up` with no `--loadout` (R§10 `up` row).
    let approved = match &found {
        Some(Use::Approved { def, .. }) => Some(def),
        _ => None,
    };
    let name = if let Some(n) = user_default {
        n.clone()
    } else if approved.is_some_and(|d| d.workspace.is_some()) {
        REPOSITORY_LOADOUT.to_string()
    } else {
        BUILTIN_LOADOUT.to_string()
    };
    let with = approved.map(|d| merged.with_definition(d));
    let lo = match with.as_ref().unwrap_or(merged).config.loadout.get(&name) {
        Some(lo) => lo.clone(),
        None if name == BUILTIN_LOADOUT => builtin_solo(),
        None => return out,
    };
    // The name `zygo up` keeps (loadout.rs:212-214): an agent pane's valid member name that no earlier agent pane holds.
    let mut seen: Vec<String> = Vec::new();
    let members: Vec<(usize, &LoadoutPane, Option<String>)> = lo
        .window
        .iter()
        .flat_map(|w| &w.pane)
        .enumerate()
        .map(|(i, p)| {
            let name = p.name.clone().filter(|n| {
                p.kind.parse::<Agent>().is_ok()
                    && zygo_core::ids::valid_member_name(n)
                    && !seen.contains(n)
            });
            seen.extend(name.clone());
            (i, p, name)
        })
        .filter(|(_, p, _)| {
            p.pool && p.kind == "claude-code" && !p.server.as_deref().is_some_and(has_user_info)
        })
        .collect();
    let names: Vec<Option<String>> = members.iter().map(|(_, _, n)| n.clone()).collect();
    let dir_of: Vec<(String, PathBuf)> = members
        .iter()
        .zip(pool::assign(&names, dirs.len()))
        .map(|((i, _, n), k)| {
            (
                n.clone().unwrap_or_else(|| format!("pane {}", i + 1)),
                dirs[k].clone(),
            )
        })
        .collect();
    out.extend(pool::shared_lines(&dir_of));
    out
}

/// The icon test line (AU2, H§14 #32): the icon set that the status line uses in this environment (`active`), the
/// `ZYGO_NERD_FONT` value that selects the other set, and every Nerd glyph of H§6.5 in the order of its table. The
/// glyphs are the Nerd ones with each set: a user who sees them all can use the Nerd set, and a user who sees boxes
/// can select the plain set.
fn icon_line(active: IconSet) -> String {
    let test: Vec<&str> = glyphs(IconSet::Nerd).iter().map(|(_, glyph)| *glyph).collect();
    let (name, other) = match active {
        IconSet::Nerd => ("nerd", "ZYGO_NERD_FONT=0 selects the plain set"),
        IconSet::Plain => ("plain", "ZYGO_NERD_FONT=1 selects the nerd set"),
    };
    format!("icons: {name} set ({other}); test: {}", test.join(" "))
}

/// §15.3 last bullet and §17: the plugin store, the selected plugins, and the same plugins in Claude Code's own config.
fn plugin_lines(env: &Env, merged: &Merged, out: &mut Vec<String>) {
    let st = state(&env.paths.plugin_store);
    out.push(match &st.manifest {
        ManifestState::Missing => "plugin store: manifest missing".to_string(),
        ManifestState::Invalid(why) => format!("plugin store: manifest invalid ({})", esc(why.lines().next().unwrap_or_default())),
        ManifestState::Valid => format!("plugin store: manifest valid, plugins: {}", st.plugins.len()),
    });
    let own = env.claude_config_dir.as_ref().map(|d| d.join("plugins").join("installed_plugins.json"));
    let mut own_keys = Vec::new();
    if let Some(path) = &own {
        match std::fs::read(path) {
            Ok(b) => match serde_json::from_slice::<serde_json::Value>(&b) {
                Ok(j) => own_keys = j.get("plugins").and_then(|m| m.as_object()).map(|m| m.keys().cloned().collect()).unwrap_or_default(),
                Err(e) => out.push(format!("claude config: cannot read {} ({})", esc(&path.display().to_string()), esc(&e.to_string()))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => out.push(format!("claude config: cannot read {} ({})", esc(&path.display().to_string()), esc(&e.to_string()))),
        }
    }
    let sel = selection(merged, None);
    for id in &sel.selected {
        let name = esc(&id.to_string());
        let source = sel.sources.get(&id.marketplace).cloned().unwrap_or_default();
        match st.plugins.iter().find(|p| p.entry.marketplace == id.marketplace && p.entry.plugin == id.plugin && p.source == source) {
            None => out.push(format!("plugin {name}: not in store; run zygo plugins sync")),
            Some(sp) if !sp.intact => out.push(format!("plugin {name}: store copy changed; run zygo plugins sync")),
            Some(sp) => {
                for (server, command) in missing_commands(&sp.dir, env.search_path.as_deref()) {
                    out.push(format!("plugin {name}: {} not found (mcp server {})", esc(&command), esc(&server)));
                }
            }
        }
        for key in own_keys.iter().filter(|k| k.split_once('@').map_or(k.as_str(), |(p, _)| p) == id.plugin) {
            out.push(format!("plugin {name}: also installed in Claude Code as {}; it would load twice", esc(key)));
        }
    }
}

/// Mail sync design S§7.5: the twin line while the `state.json` `last_error` of workspace `ws_id` is the twin check,
/// which the stopped pass writes and a pass that ends clears. Reads `state.json` only: no git, no lock. A missing or
/// unreadable file gives no line, so `zygo doctor` still never fails (§8.2).
pub fn twin_line(paths: &Paths, ws_id: &str) -> Option<String> {
    let sp = zygo_mail_sync::state::SyncPaths::new(paths, ws_id).ok()?;
    let st = zygo_mail_sync::state::load(&sp).ok()?;
    (st.last_error.as_deref() == Some(Stop::Twin.check_name())).then(|| Stop::Twin.line())
}

/// Reports and never refuses: exit 0, except an invalid config (§8.2).
pub fn run() -> i32 {
    let paths = Paths::from_env();
    let repo = repo_top();
    let local = LoadContext { paths: paths.clone(), repo: repo.clone(), team: None };
    let invalid = |e: zygo_config::ConfigError| {
        print_err(&format!("zygo doctor: {}\n", esc(&e.to_string())));
        ExitCode::ConfigInvalid as i32
    };
    let merged = match load(&local) {
        Ok(m) => m,
        Err(e) => return invalid(e),
    };
    // Reports and never refuses (§8.2): a team layer that cannot be read is one stderr line; the report uses the local config.
    let verb = VerbCtx { slash: false, save: false, session_dir: SessionDir::from_env(), paths: paths.clone() };
    let merged = match super::verbs::team(&verb, "doctor") {
        Ok(None) => merged,
        Ok(Some(t)) => match load(&LoadContext { team: Some(t), ..local }) {
            Ok(m) => m,
            Err(e) => return invalid(e),
        },
        Err(e) => {
            print_err(&format!("{}\n", esc(&e.to_string())));
            merged
        }
    };
    let secrets = default_store(&paths);
    let search_path = std::env::var_os("PATH");
    let claude_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".claude")));
    let probe_path = search_path.clone();
    let mut lines = report(
        &Env {
            paths: &paths,
            repo: repo.as_deref(),
            search_path,
            claude_config_dir,
            secrets: &secrets,
            env: &|n| std::env::var_os(n),
            probe: &|prog| probe_version(prog, probe_path.as_deref(), PROBE_BOUND),
            server: merged.config.server.url.as_deref().map(zygo_launch::serve_client::reachability),
            tmux: match super::find_tmux() {
                Some(p) => Tmux::new(p, None).version(),
                None => Err(MuxError::Spawn { message: std::io::Error::from(std::io::ErrorKind::NotFound).to_string() }),
            },
        },
        &merged,
    );
    if let Ok(cwd) = std::env::current_dir() {
        lines.extend(definition_lines(&cwd, &merged, &paths.approvals_file, &SystemGit));
        // S§7.5: the workspace of the mail verbs (`ZYGO_WORKSPACE`, else the checkout of this folder); none: no line.
        if let Ok(ws) = super::mail::workspace_of(&paths, &cwd, std::env::var_os("ZYGO_WORKSPACE").as_deref(), GIT_BOUND) {
            lines.extend(twin_line(&paths, &ws.id));
        }
    }
    print_out(&(lines.join("\n") + "\n"));
    0
}
