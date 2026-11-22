use std::ffi::{OsStr, OsString};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use semver::Version;
use zygo_config::keys::{default_store, error_kind, resolve_with, SecretStore};
use zygo_config::{load, KeyRef, LoadContext, Merged};
use zygo_core::exit::ExitCode;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_hooks::statusline::render::{glyphs, icon_set, IconSet};
use zygo_plugins::check::esc;
use zygo_plugins::select::selection;
use zygo_plugins::snapshot::missing_commands;
use zygo_plugins::store::{state, ManifestState};
use zygo_work::policy::Policy;

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
    let lines = report(
        &Env {
            paths: &paths,
            repo: repo.as_deref(),
            search_path,
            claude_config_dir,
            secrets: &secrets,
            env: &|n| std::env::var_os(n),
            probe: &|prog| probe_version(prog, probe_path.as_deref(), PROBE_BOUND),
            server: merged.config.server.url.as_deref().map(zygo_launch::serve_client::reachability),
        },
        &merged,
    );
    print_out(&(lines.join("\n") + "\n"));
    0
}
