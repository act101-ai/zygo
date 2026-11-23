//! The launch sequence (§8.2): config (with the team layer in serve mode), profile, repository and work item, proxy
//! or team server and session, plugins, the agent's config, the child, and the cleanup at exit.

use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
#[cfg(feature = "test-hooks")]
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zygo_config::profile::{requested, select, validate, Profile};
use zygo_config::keys::default_store;
use zygo_config::{load, Dialect, LoadContext, Merged};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{LegendEntry, Member, ResumeNote, SessionView, ViewWorkspace, WorkItemView};
use zygo_core::Agent;
use zygo_plugins::select::{known_plugins, selection};
use zygo_plugins::snapshot::{delivered_in, missing_commands, snapshot_in, Snapshot};
use zygo_plugins::sync::{sync_all_in, TOTAL};
use zygo_proxy::client::{ClientError, SessionClient, WindowArg};
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{SessionGrant, SessionRequest};
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::act_lite_store::{read_with, Lookup};
use zygo_usage::Store;
use zygo_work::current::{resolve_noted, stderr_lines};
use zygo_work::repo::{checkout, repository_id};
use zygo_work::WorkItem;

use crate::adapter::{AgentAdapter, LaunchCtx, Plugins};
use crate::env::{child_env, Env};
use crate::serve_client::{self, end_rows, write_team, Bounds, Heartbeat, Refused, ServeClient, HEARTBEAT_EVERY};
use crate::{session_dir, signals};

/// The bound of the repository lookup (§8.2 step 1), of `act-lite --version` (§10.5) and of the agent `--version`
/// probe (spec §7 note 2026-11-19: as act-lite, §10.5).
const BOUND: Duration = Duration::from_secs(2);

pub struct Launcher;

/// How a launch ended (workspaces design AU34, AU60, AU73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// The process exit code: the agent's own code when it ran; `128 + n` when signal n ended it or stopped the launch.
    pub code: i32,
    /// The signal that ended the agent; `None` for an exit with a code and on every non-unix target.
    pub signal: Option<i32>,
    /// The agent process was started.
    pub spawned: bool,
    /// The launcher received SIGTERM or SIGHUP between the start of its signal handling and the end of its cleanup.
    pub received: bool,
}

/// What a launch records for its outcome besides the code.
#[derive(Default)]
struct Marks {
    signal: Cell<Option<i32>>,
    spawned: Cell<bool>,
    received: Cell<bool>,
}

/// The signal handling of one launch. Its drop copies `signals::received()` into the marks; the handling ends after that.
struct Handling<'a> {
    marks: &'a Marks,
    _installed: signals::Installed,
}

impl Drop for Handling<'_> {
    fn drop(&mut self) {
        self.marks.received.set(signals::received());
    }
}

/// Called once, after the session directory exists and before the agent starts, with the session directory, the
/// session id, the resolved agent program and the member of this launch (workspaces design AU34): `LaunchOpts.member`,
/// with or without a user `--settings` (AU59).
pub type SessionCallback = Box<dyn FnOnce(&SessionDir, &str, &Path, Option<&Member>) -> anyhow::Result<()> + Send>;

/// The agent session that the launcher starts or resumes (W§8.3, AU3). The launcher puts the adapter's
/// `preassign_args` or `resume_args` before the user's arguments, so before any `--`; for OpenCode, right after a
/// `run` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSession {
    /// A fresh session with this id (`AgentAdapter::preassign_args`).
    Preassign(String),
    /// Resume the session with this id (`AgentAdapter::resume_args`).
    Resume(String),
}

pub struct LaunchOpts {
    /// The user's arguments after `--`.
    pub args: Vec<String>,
    pub profile: Option<String>,
    pub passthrough: bool,
    pub server: Option<String>,
    pub compact: Option<WindowArg>,
    pub yolo: bool,
    pub env: Env,
    pub cwd: PathBuf,
    pub paths: Paths,
    /// `Some`: this launch is a workspace member (workspaces design AU58, AU102).
    pub member: Option<Member>,
    /// The inputs of the resume note; written to the session view only with `member` (AU72).
    pub resume_note: Option<ResumeNote>,
    /// `Some`: the launcher adds the session arguments of the adapter for it (W§8.3, AU3).
    pub agent_session: Option<AgentSession>,
    /// An error is logged to the session log and the launch goes on.
    pub on_session: Option<SessionCallback>,
    /// `Some`: the built-in layer is this table (`load_with_builtin`).
    #[cfg(feature = "test-hooks")]
    pub builtin: Option<toml::Table>,
    /// `Some`: the launch lines go here, not to stderr.
    #[cfg(feature = "test-hooks")]
    pub stderr: Option<Arc<Mutex<Vec<u8>>>>,
    /// `Some`: the serve-mode heartbeat interval in place of `HEARTBEAT_EVERY`.
    #[cfg(feature = "test-hooks")]
    pub heartbeat: Option<Duration>,
    /// `Some`: the serve client's bounds in place of the §7 values.
    #[cfg(feature = "test-hooks")]
    pub serve_bounds: Option<Bounds>,
}

impl LaunchOpts {
    pub fn new(env: Env, cwd: PathBuf, paths: Paths) -> LaunchOpts {
        LaunchOpts {
            args: Vec::new(),
            profile: None,
            passthrough: false,
            server: None,
            compact: None,
            yolo: false,
            env,
            cwd,
            paths,
            member: None,
            resume_note: None,
            agent_session: None,
            on_session: None,
            #[cfg(feature = "test-hooks")]
            builtin: None,
            #[cfg(feature = "test-hooks")]
            stderr: None,
            #[cfg(feature = "test-hooks")]
            heartbeat: None,
            #[cfg(feature = "test-hooks")]
            serve_bounds: None,
        }
    }
}

/// The launch lines; a write error is ignored (a closed stderr never changes the exit code).
#[derive(Clone)]
struct Out {
    #[cfg(feature = "test-hooks")]
    buf: Option<Arc<Mutex<Vec<u8>>>>,
}

impl Out {
    fn line(&self, s: &str) {
        #[cfg(feature = "test-hooks")]
        if let Some(b) = &self.buf {
            let _ = writeln!(b.lock().unwrap_or_else(|e| e.into_inner()), "{s}");
            return;
        }
        let _ = writeln!(std::io::stderr(), "{s}");
    }
}

/// What the session part of the launch reads; the proxy and the directory are cleaned up by the caller.
struct Session<'a> {
    opts: &'a LaunchOpts,
    out: &'a Out,
    cfg: Arc<Merged>,
    profile: Option<Profile>,
    program: PathBuf,
    agent_version: String,
    repo: Option<String>,
    work: Option<WorkItem>,
    search: Option<OsString>,
    grant: SessionGrant,
    sd: SessionDir,
    proxy_url: zygo_proxy::Url,
    top: Option<PathBuf>,
}

impl Launcher {
    /// §8.2 in local or serve mode. `code` of the result is the process exit code (the agent's own code when it ran).
    pub fn run(adapter: &dyn AgentAdapter, opts: LaunchOpts) -> LaunchOutcome {
        let marks = Marks::default();
        let code = Self::launch(adapter, opts, &marks);
        LaunchOutcome {
            code,
            signal: marks.signal.get(),
            spawned: marks.spawned.get(),
            received: marks.received.get(),
        }
    }

    /// The launch; returns the process exit code and records the rest of the outcome in `marks`.
    fn launch(adapter: &dyn AgentAdapter, mut opts: LaunchOpts, marks: &Marks) -> i32 {
        let on_session = opts.on_session.take();
        if let Some(s) = &opts.agent_session {
            let extra = match s {
                AgentSession::Preassign(id) => adapter.preassign_args(id),
                AgentSession::Resume(id) => adapter.resume_args(id),
            };
            insert_session_args(adapter.agent(), &mut opts.args, extra);
        }
        let out = Out {
            #[cfg(feature = "test-hooks")]
            buf: opts.stderr.clone(),
        };
        let search = opts.env.get("PATH").map(OsStr::to_os_string);
        let top = checkout(&opts.cwd, Instant::now() + BOUND).map(|c| c.top);
        let cfg = match load_cfg(&opts, LoadContext { paths: opts.paths.clone(), repo: top.clone(), team: None }) {
            Ok(c) => c,
            Err(e) => {
                out.line(&format!("zygo: {e}"));
                return 11;
            }
        };
        // Serve mode (§7.1): `--server` over `[server] url`; C8 before the token.
        let client = if opts.server.is_none() && cfg.config.server.url.is_none() {
            None
        } else {
            let env = |n: &str| opts.env.get(n).map(OsStr::to_os_string);
            match serve_client::from_config_with(opts.server.as_deref(), &cfg.config, &default_store(&opts.paths), &env, serve_bounds(&opts)) {
                Ok(c) => c,
                Err(r) => {
                    out.line(&format!("zygo: {r}"));
                    return if matches!(r, Refused::Url) { 2 } else { 1 };
                }
            }
        };
        // The team layer (§6.1 layer 2); the config is loaded again with it.
        let (cfg, team) = match &client {
            None => (cfg, None),
            Some(c) => {
                let t = match c.team_table() {
                    Ok(t) => t,
                    Err(ClientError::Unreachable(d)) => {
                        out.line(&unreachable_line(&d));
                        return 12;
                    }
                    Err(e) => {
                        out.line(&format!("zygo: team config refused: {}", zygo_plugins::check::esc(&e.to_string())));
                        return 1;
                    }
                };
                match load_cfg(&opts, LoadContext { paths: opts.paths.clone(), repo: top.clone(), team: Some(t.clone()) }) {
                    Ok(m) => (m, Some(t)),
                    Err(e) => {
                        out.line(&format!("zygo: {}", zygo_plugins::check::esc(&e.to_string())));
                        return 11;
                    }
                }
            }
        };
        for w in &cfg.warnings {
            out.line(&format!("zygo: {w}"));
        }
        if let Ok(s) = Store::open(&opts.paths.usage_db) {
            let _ = s.prune(cfg.config.usage.retain_days);
        }

        let Ok(program) = which::which_in(adapter.program(), search.as_ref(), &opts.cwd) else {
            out.line(adapter.install_hint());
            return 13;
        };
        let probe_env = child_env(&opts.env, &cfg, &[]).0;
        let agent_version = agent_version(&program, &probe_env, &opts.cwd);

        let repo = repository_id(&opts.cwd);
        let (work, notes) = resolve_noted(&opts.cwd, &cfg.config.work);
        for l in stderr_lines(work.as_ref(), &notes) {
            out.line(&l);
        }

        let mut cfg = cfg;
        let mut profile = None;
        let known = known_plugins(&selection(&cfg, None), &zygo_plugins::store::state(&opts.paths.plugin_store));
        let name = requested(&cfg, opts.profile.as_deref());
        match (name, select(&cfg, opts.profile.as_deref())) {
            (Some(n), None) => out.line(&format!("zygo: profile {n} not found; launching without it")),
            (_, Some(p)) => {
                let c = &cfg.config;
                match validate(&p, &c.role, &c.main_role, &c.catalog.view(), Dialect::Anthropic, &known) {
                    Err(v) => out.line(&format!("zygo: {v}; launching without the profile")),
                    Ok(()) => match cfg.apply_profile(&p.name) {
                        Ok(m) => {
                            cfg = m;
                            profile = Some(p);
                        }
                        Err(e) => out.line(&format!("zygo: {e}; launching without the profile")),
                    },
                }
            }
            (None, None) => {}
        }
        let cfg = Arc::new(cfg);

        let req = |stored_values| SessionRequest {
            agent: adapter.agent(),
            agent_version: agent_version.clone(),
            profile: profile.clone(),
            passthrough: opts.passthrough,
            repo: repo.clone(),
            work: work.as_ref().map(|w| format!("{}:{}", w.source, w.id)),
            stored_values,
            known_plugins: known,
        };
        // §8.2 step 2 note 2026-11-19: held until `finish` has run (declared before the `Finish` guard).
        let _signals = Handling { marks, _installed: signals::install() };
        let (grant, mut end, url, store_err) = match client {
            None => {
                let store_env = opts.env.clone();
                let lookup = Lookup { env: Box::new(move |k| store_env.get(k).map(OsStr::to_os_string)), ..Lookup::system() };
                let (stored_values, store_err) = match read_with(top.as_deref().unwrap_or(&opts.cwd), &lookup) {
                    Ok(v) => (v, None),
                    Err(e) => (Vec::new(), Some(e.to_string())),
                };
                let start = StartOpts { usage_db: Some(opts.paths.usage_db.clone()), ..Default::default() };
                let proxy = match Proxy::start_local_with(cfg.clone(), start) {
                    Ok(p) => p,
                    Err(e) => {
                        out.line(&format!("zygo: cannot start the proxy: {e}"));
                        return signals::or_stopped(1);
                    }
                };
                let grant = match proxy.create_session(req(stored_values)) {
                    Ok(g) => g,
                    Err(z) => {
                        out.line(&format!("zygo: session refused: {}", z.message));
                        proxy.shutdown();
                        return signals::or_stopped(1);
                    }
                };
                let url = proxy.url();
                (grant, End::Local(proxy), url, store_err)
            }
            // §16.1 A1: no act-lite stored value leaves the machine (§20 #29d).
            Some(c) => {
                let grant = match c.create_session(&req(Vec::new())) {
                    Ok(g) => g,
                    Err(ClientError::Unreachable(d)) => {
                        out.line(&unreachable_line(&d));
                        return signals::or_stopped(12);
                    }
                    Err(ClientError::Reply { status, message, .. }) => {
                        let why = if message.is_empty() { format!("HTTP {status}") } else { message };
                        out.line(&format!("zygo: session refused: {}", zygo_plugins::check::esc(&why)));
                        return signals::or_stopped(1);
                    }
                    Err(e) => {
                        out.line(&format!("zygo: {e}"));
                        return signals::or_stopped(1);
                    }
                };
                let url = c.url();
                let end = End::Serve { client: c, session: grant.session.clone(), heartbeat: None, out: out.clone() };
                (grant, end, url, None)
            }
        };
        if let Some(w) = &grant.profile_warning {
            out.line(&format!("zygo: {}; launching without the profile", zygo_plugins::check::esc(w)));
        }
        if let Some(n) = &grant.usage_note {
            out.line(&format!("zygo: {}", zygo_plugins::check::esc(n)));
        }
        let (sd, pid) = match session_dir::open(&opts.paths, &grant.session, &grant.token, &url) {
            Ok(o) => o,
            Err(e) if e.is::<session_dir::Taken>() => {
                out.line(&format!("zygo: session refused: {e}"));
                if let End::Local(p) = end {
                    p.shutdown();
                }
                return signals::or_stopped(1);
            }
            Err(e) => {
                out.line(&format!("zygo: cannot create the session directory: {e}"));
                finish(&SessionDir::new(opts.paths.sessions_dir.join(&grant.session)), end, &opts.paths);
                return signals::or_stopped(1);
            }
        };
        if let End::Serve { client, session, heartbeat, .. } = &mut end {
            // S5: pending until its rows are local; the usage store fails open. Only for a directory this launch made
            // (B-1).
            if let Ok(s) = Store::open(&opts.paths.usage_db) {
                let _ = s.mark_sync_pending(session);
            }
            let beat_out = out.clone();
            *heartbeat = client
                .try_clone()
                .ok()
                .map(|hc| Heartbeat::start(hc, session.clone(), heartbeat_every(&opts), Box::new(move |m| beat_out.line(m))));
        }
        let guard = Finish { sd: sd.clone(), end: Some(end), paths: &opts.paths, _pid: pid };
        // AU34: the session directory exists and the agent is not started; an error never stops the launch (C6).
        if let Some(call) = on_session
            && let Err(e) = call(&sd, &grant.session, &program, opts.member.as_ref())
        {
            sd.append_log(&format!("session callback: {}", zygo_plugins::check::esc(&format!("{e:#}"))));
        }
        if let Some(t) = &team
            && let Err(e) = write_team(&sd.root, t)
        {
            out.line(&format!("zygo: cannot write the team config: {e}"));
            return signals::or_stopped(1);
        }
        if let Some(e) = store_err {
            sd.append_log(&format!("act-lite store: {e}"));
        }
        if let Some(n) = &grant.redaction_note {
            sd.append_log(&zygo_plugins::check::esc(n));
        }
        let s = Session {
            opts: &opts,
            out: &out,
            cfg,
            profile,
            program,
            agent_version,
            repo,
            work,
            search,
            proxy_url: url,
            grant,
            sd,
            top,
        };
        let code = s.run(adapter, marks);
        drop(guard);
        code
    }
}

impl Session<'_> {
    /// §8.2 steps 2a-5.
    fn run(self, adapter: &dyn AgentAdapter, marks: &Marks) -> i32 {
        let (opts, out, sd, paths) = (self.opts, self.out, &self.sd, &self.opts.paths);
        if let Some(s) = signals::stopped() {
            return 128 + s;
        }
        let compact = opts.compact.map(|w| SessionClient::from_session_dir(&sd.root).and_then(|c| c.set_compact(w)));
        if let Some(s) = signals::stopped() {
            return 128 + s;
        }
        if let Some(Err(e)) = compact {
            out.line(&format!("zygo: {}", zygo_plugins::check::esc(&e.to_string())));
            return if matches!(e, ClientError::Unreachable(_)) { 12 } else { 1 };
        }

        for l in sync_all_in(&self.cfg, paths, TOTAL).lines {
            out.line(&l);
        }
        if let Some(s) = signals::stopped() {
            return 128 + s;
        }
        let sel = selection(&self.cfg, self.profile.as_ref());
        let deadline = Instant::now() + TOTAL;
        let search = self.search.as_deref();
        let (plugins, navigation) = match adapter.agent() {
            Agent::ClaudeCode => {
                let snap = match snapshot_in(paths, &sd.root, Agent::ClaudeCode, &sel, search, deadline) {
                    Ok(s) => {
                        for l in &s.lines {
                            out.line(l);
                        }
                        s
                    }
                    Err(_) => {
                        out.line("plugins: partial");
                        Snapshot::default()
                    }
                };
                let nav = snap.plugins.iter().any(|p| p.marketplace == "act101" && p.name == "act-lite" && !p.mcp.is_empty());
                (Plugins::Snapshot(snap), nav)
            }
            Agent::OpenCode => {
                let d = delivered_in(paths, &sel, deadline).ok();
                let nav = d.iter().flat_map(|d| &d.plugins).any(|p| {
                    p.id.marketplace == "act101"
                        && p.id.plugin == "act-lite"
                        && p.entry.files.contains_key(".mcp.json")
                        && missing_commands(&p.dir, search).is_empty()
                });
                (Plugins::Delivered(d), nav)
            }
        };
        if navigation && let Ok(p) = which::which_in("act-lite", self.search.as_ref(), &opts.cwd) {
            let env = child_env(&opts.env, &self.cfg, &[]).0;
            let compiled = compiled_act_lite();
            if let Some(v) = version_within_bound(&p, &env, &opts.cwd)
                && v != compiled
            {
                sd.append_log(&format!("act-lite: installed {v}, language list for {compiled}"));
            }
        }

        let config = self.grant.config.clone();
        let effective_role = match adapter.picked_role(&opts.args, &config) {
            None => Some(config.main_role.clone()),
            Some(_) if opts.passthrough => None,
            Some(m) if config.roles.contains_key(&m) || config.custom_roles.contains_key(&m) => Some(m),
            Some(_) => None,
        };
        let role_prompt = effective_role.as_ref().and_then(|r| {
            config.roles.get(r).map(|x| x.prompt.clone()).or_else(|| config.custom_roles.get(r).map(|x| x.prompt.clone()))
        });
        let legend = config
            .roles
            .iter()
            .map(|(k, r)| LegendEntry { role: k.clone(), description: r.description.clone(), custom: false })
            .chain(config.custom_roles.iter().map(|(k, r)| LegendEntry {
                role: k.clone(),
                description: r.description.clone(),
                custom: true,
            }))
            .collect();
        let c = &self.cfg.config;
        // A user `--settings` does not change the member (AU59): its settings are merged into zygo's file (§8.5 Flags).
        let member = opts.member.clone();
        // The base stays although every field is named today: a later field then needs no change here
        // until the launcher sets it (statusline HUD design H§17 note 1).
        #[allow(clippy::needless_update)]
        let view = SessionView {
            session: self.grant.session.clone(),
            agent: adapter.agent(),
            main_role: c.main_role.clone(),
            effective_role: effective_role.clone(),
            role_prompt,
            user_prompt: c.user.prompt.clone(),
            profile: self.profile.as_ref().map(|p| p.name.clone()),
            profile_prompt: self.profile.as_ref().and_then(|p| p.prompt.clone()),
            work_item: self.work.as_ref().map(|w| WorkItemView { unit: w.unit(), title: w.title.clone() }),
            legend,
            disabled_roles: self.profile.as_ref().map(|p| p.disabled.clone()).unwrap_or_default(),
            work_units: c.features.work_units,
            results: c.features.results,
            compact_tool: c.features.compact_tool,
            navigation,
            trim_threshold_bytes: c.results.trim_threshold_bytes,
            head_lines: c.results.head_lines,
            tail_lines: c.results.tail_lines,
            statusline_style: c.statusline.style.clone(),
            workspace: member.clone().map(|member| ViewWorkspace { member, resume_note: opts.resume_note.clone() }),
            ..Default::default()
        };
        if let Err(e) = view.save(sd) {
            out.line(&format!("zygo: cannot write the session view: {e}"));
            return signals::or_stopped(1);
        }

        let ctx = LaunchCtx {
            agent: adapter.agent(),
            program: self.program.clone(),
            agent_version: self.agent_version.clone(),
            cfg: self.cfg.clone(),
            profile: self.profile.clone(),
            session: self.grant.session.clone(),
            token: self.grant.token.clone(),
            session_dir: sd.clone(),
            proxy_url: self.proxy_url.clone(),
            config,
            view,
            effective_role: effective_role.clone(),
            repo: self.repo.clone(),
            work: self.work.clone(),
            plugins,
            args: opts.args.clone(),
            passthrough: opts.passthrough,
            yolo: opts.yolo,
            compact: opts.compact,
            cwd: opts.cwd.clone(),
            repo_root: self.top.clone(),
            env: opts.env.clone(),
            paths: paths.clone(),
        };
        if let Some(m) = adapter.refusal(&ctx) {
            out.line(&format!("zygo: {m}"));
            return signals::or_stopped(2);
        }
        let spec = match adapter.build(&ctx) {
            Ok(s) => s,
            Err(e) => {
                out.line(&format!("zygo: {e}"));
                return signals::or_stopped(1);
            }
        };
        if let Err(p) = adapter.preflight(&ctx) {
            out.line(&format!("zygo: {}: sets {}; zygo cannot launch with it", p.file.display(), p.key));
            return signals::or_stopped(2);
        }
        // The OpenCode store lock (`Delivered.lock`) is not held for the session.
        drop(ctx);
        for l in &spec.lines {
            out.line(l);
        }
        for l in &spec.log {
            sd.append_log(l);
        }

        let mut set: Vec<(String, String)> = vec![
            ("ZYGO_SESSION".into(), self.grant.session.clone()),
            ("ZYGO_SESSION_DIR".into(), sd.root.to_string_lossy().into_owned()),
            ("ZYGO_AGENT".into(), adapter.agent().as_str().into()),
            ("ZYGO_URL".into(), self.proxy_url.to_string()),
            ("ZYGO_REPO".into(), self.repo.clone().unwrap_or_default()),
            ("ZYGO_WORK".into(), self.work.as_ref().map(|w| format!("{}:{}", w.source, w.id)).unwrap_or_default()),
            ("ZYGO_MAIN_ROLE".into(), effective_role.unwrap_or_default()),
        ];
        set.extend(spec.env);
        // §8.4 "Set" for a member (AU58); for every other launch the filter removes both names.
        if let Some(m) = &member {
            set.push(("ZYGO_WORKSPACE".into(), m.ws_id.clone()));
            set.push(("ZYGO_PANE".into(), m.pane.clone()));
        }
        let (env, removed) = child_env(&opts.env, &self.cfg, &set);
        if !removed.is_empty() {
            sd.append_log(&format!("env: removed {}", removed.join(", ")));
        }

        if let Some(s) = signals::stopped() {
            return 128 + s;
        }
        let child = Command::new(&self.program)
            .args(&spec.args)
            .env_clear()
            .envs(env.0.iter().map(|(k, v)| (k, v)))
            .current_dir(&opts.cwd)
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                out.line(adapter.install_hint());
                return signals::or_stopped(13);
            }
            Err(e) => {
                out.line(&format!("zygo: cannot start {}: {e}", self.program.display()));
                return signals::or_stopped(1);
            }
        };
        marks.spawned.set(true);
        match signals::wait(&mut child) {
            Ok(st) => {
                marks.signal.set(signal_of(st));
                exit_code(st)
            }
            Err(e) => {
                out.line(&format!("zygo: {e}"));
                1
            }
        }
    }
}

/// Puts the agent session arguments `extra` before the user's arguments, so before any `--` (W§8.3, AU3). For
/// OpenCode, when the first positional token before `--` (by W4's walk, so not a flag value) is the subcommand `run`,
/// they go right after it.
fn insert_session_args(agent: Agent, args: &mut Vec<String>, extra: Vec<String>) {
    let at = match agent {
        Agent::ClaudeCode => 0,
        Agent::OpenCode => {
            let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
            let taken: Vec<std::ops::Range<usize>> = zygo_workspace::args::occurrences(agent, args).into_iter().map(|o| o.tokens).collect();
            (0..end)
                .find(|i| !taken.iter().any(|r| r.contains(i)) && !args[*i].starts_with('-'))
                .filter(|i| args[*i] == "run")
                .map_or(0, |i| i + 1)
        }
    };
    args.splice(at..at, extra);
}

#[cfg(feature = "test-hooks")]
fn load_cfg(opts: &LaunchOpts, ctx: LoadContext) -> Result<Merged, zygo_config::ConfigError> {
    match &opts.builtin {
        Some(b) => zygo_config::load_with_builtin(&ctx, b.clone()),
        None => load(&ctx),
    }
}

#[cfg(not(feature = "test-hooks"))]
fn load_cfg(_opts: &LaunchOpts, ctx: LoadContext) -> Result<Merged, zygo_config::ConfigError> {
    load(&ctx)
}

/// Runs [`finish`] once when dropped: at the normal end of the launch, on every early return after the session
/// directory exists, and on unwind, so a panic leaves no live session directory (`sweep` skips a directory whose `pid`
/// is locked).
struct Finish<'a> {
    sd: SessionDir,
    end: Option<End>,
    paths: &'a Paths,
    /// The lock on `pid` (U3): dropped after `drop` ran `finish`, so `zygo usage --sync` skips the session until its
    /// rows are fetched.
    _pid: std::fs::File,
}

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        if let Some(e) = self.end.take() {
            finish(&self.sd, e, self.paths);
        }
    }
}

/// What ends the session: the local proxy, or the team server's session with its heartbeat.
enum End {
    Local(ProxyHandle),
    Serve { client: ServeClient, session: String, heartbeat: Option<Heartbeat>, out: Out },
}

/// §8.2 step 6: fold the open units; local mode: stop the proxy; serve mode (§14): stop the heartbeat, fetch the
/// session's rows into the local store (the pending mark kept unless they are complete), then delete the session
/// unless the fetch found the server unreachable (S3: against a stalled server the exit costs at most one call bound
/// for the rows fetch, plus an in-flight heartbeat); remove the directory except `results/`.
fn finish(sd: &SessionDir, end: End, paths: &Paths) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    session_dir::fold_units(sd, &paths.usage_db, now);
    match end {
        End::Local(p) => p.shutdown(),
        End::Serve { client, session, heartbeat, out } => {
            drop(heartbeat);
            let fetched = client.requests(&session);
            let unreachable = matches!(fetched, Err(ClientError::Unreachable(_)));
            match Store::open(&paths.usage_db) {
                Ok(s) => {
                    if let Err(e) = end_rows(&s, &session, fetched) {
                        out.line(&format!("zygo: {}; zygo usage --sync retries", zygo_plugins::check::esc(&e.to_string())));
                    }
                }
                Err(e) => out.line(&format!("zygo: usage rows not fetched ({e}); zygo usage --sync retries")),
            }
            // S3: after an unreachable fetch DELETE would cost a second call bound; the server suspends the session
            // after 5 min without a heartbeat and keeps it until a server restart (§7.2: no idle expiry, FIX-18).
            if !unreachable {
                let _ = client.delete_session(&session);
            }
        }
    }
    session_dir::remove_except_results(sd);
}

fn heartbeat_every(#[allow(unused_variables)] opts: &LaunchOpts) -> Duration {
    #[cfg(feature = "test-hooks")]
    if let Some(d) = opts.heartbeat {
        return d;
    }
    HEARTBEAT_EVERY
}

fn serve_bounds(#[allow(unused_variables)] opts: &LaunchOpts) -> Bounds {
    #[cfg(feature = "test-hooks")]
    if let Some(b) = opts.serve_bounds {
        return b;
    }
    Bounds::default()
}

/// The launch line for a team server that cannot be reached; the transport text is escaped (S2).
fn unreachable_line(d: &str) -> String {
    format!("zygo: server unreachable: {}", zygo_plugins::check::esc(d))
}

/// The agent's code; a child ended by a signal gives 128 + the signal (not re-raised).
fn exit_code(st: ExitStatus) -> i32 {
    if let Some(c) = st.code() {
        return c;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(s) = st.signal() {
            return 128 + s;
        }
    }
    1
}

/// The signal that ended the child; `None` for an exit with a code and on every non-unix target.
fn signal_of(st: ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(&st)
    }
    #[cfg(not(unix))]
    {
        let _ = st;
        None
    }
}

/// The first token of the first line that is a semver version (after a leading `v`).
fn first_version(text: &str) -> Option<String> {
    text.lines().next()?.split_whitespace().find_map(|t| semver::Version::parse(t.trim_start_matches('v')).ok()).map(|v| v.to_string())
}

fn probe(program: &Path, env: &Env, cwd: &Path) -> Command {
    let mut c = Command::new(program);
    c.arg("--version")
        .env_clear()
        .envs(env.0.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    c
}

/// `<program> --version` (§7.2 `agent_version`) within `BOUND`; `""` when no version is printed.
fn agent_version(program: &Path, env: &Env, cwd: &Path) -> String {
    version_within_bound(program, env, cwd).unwrap_or_default()
}

/// `<program> --version` within `BOUND` (§10.5): the first stdout line, read until its end, EOF or the bound. Then the
/// probe is stopped and reaped; on unix it runs in its own process group and the whole group is killed, so a
/// background child it started does not outlive it. No line within the bound: no version.
fn version_within_bound(program: &Path, env: &Env, cwd: &Path) -> Option<String> {
    let mut cmd = probe(program, env, cwd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let ok = stdout.is_some_and(|o| std::io::BufReader::new(o.take(4096)).read_line(&mut line).is_ok());
        let _ = tx.send(ok.then_some(line));
    });
    let line = rx.recv_timeout(BOUND).ok().flatten();
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `kill` takes plain integers; the child leads its own group (`process_group(0)`) and is not reaped
        // yet, so the group id cannot name another process group.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    // The child itself too: it may have left its group, and the wait below must not block.
    let _ = child.kill();
    let _ = child.wait();
    first_version(&line?)
}

/// The `act_lite` version of the compiled language list, without the leading `v` (§10.5).
fn compiled_act_lite() -> String {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/act-lite/languages.json")).expect("the act-lite fixture is JSON");
    v["act_lite"].as_str().unwrap_or_default().trim_start_matches('v').to_string()
}

#[cfg(test)]
mod tests {
    use super::unreachable_line;

    #[test]
    fn unreachable_text_is_escaped() { // N70: launcher.rs:164, :267
        assert_eq!(unreachable_line("name one\ntwo\u{1b}[31m"), "zygo: server unreachable: name one\\ntwo\\u{1b}[31m");
    }
}
