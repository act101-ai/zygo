//! The member wrapper of `zygo ws run` (W§8.2 steps 4, 5 and 7): the member record at the start and at the exit of
//! the agent, the pre-assigned session id, the exit classification and the exit lines.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zygo_core::paths::Paths;
use zygo_core::session_dir::{write_atomic, SessionDir};
use zygo_core::session_view::Member;
use zygo_core::Agent;
use zygo_launch::env::Env;
use zygo_launch::launcher::{AgentSession, LaunchOutcome};
use zygo_launch::{AgentAdapter, LaunchOpts, Launcher};
use zygo_proxy::client::{SessionClient, SessionSettings};
use zygo_workspace::{admin, args};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::snapshot;
use zygo_workspace::identity::{valid_name, valid_ws_id};
use zygo_workspace::state::{valid_session_id, MemberRecord, MemberState, Pending, Phase, StateError, Store, Workspace};

use super::{print_err, tmux_program};

/// A resume that fails with a non-zero code in less than this time from the agent's spawn failed fast (W§8.2 step 7).
const FAST_FAIL: Duration = Duration::from_secs(10);

/// The `lock` wait of the member wrapper's writes (W§7.3: launchers and the member wrapper 5 s).
const WRAPPER_WAIT: Duration = Duration::from_secs(5);

/// The member of one wrapped launch: its workspace, its pane key, its state and its saved launch as typed (D1).
pub struct MemberCtx {
    pub ws_id: String,
    pub pane: String,
    pub store: Store,
    pub launch: zygo_workspace::state::Launch,
}

/// The member wrapper (W§8.2 steps 4, 5 and 7): marks the launch as a member (AU102), pre-assigns a Claude Code
/// session id (W§8.3, D2), writes the member record from the session callback (AU92, D3), runs the launch, then
/// writes the exit record (D5) and prints the exit lines (D7). It does not touch any signal handling (D13). The
/// outcome is returned unchanged, so the agent's exit code stays.
pub fn run_wrapped(adapter: &dyn AgentAdapter, mut opts: LaunchOpts, m: &MemberCtx) -> LaunchOutcome {
    let kind = adapter.agent();
    opts.member = Some(Member { ws_id: m.ws_id.clone(), pane: m.pane.clone(), ..Default::default() });
    if opts.agent_session.is_none() && kind == Agent::ClaudeCode && !adapter.has_session_arg(&opts.args) {
        opts.agent_session = Some(AgentSession::Preassign(new_session_uuid()));
    }
    let pre = match &opts.agent_session {
        Some(AgentSession::Preassign(id)) => Some(id.clone()),
        _ => None,
    };
    let resumed = match &opts.agent_session {
        Some(AgentSession::Resume(id)) => Some(id.clone()),
        _ => None,
    };
    let config_dir = match kind {
        Agent::ClaudeCode => effective_config_dir(&opts.env, &opts.cwd),
        Agent::OpenCode => None,
    };
    // The config dir of the printed resume command (AU61, PW16).
    let shown_dir = config_dir.clone();
    // The zygo session of this start, set only when the callback's record write gave a state (D3, D5).
    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let paths = opts.paths.clone();
    let cb = Callback {
        paths: opts.paths.clone(),
        ws_id: m.ws_id.clone(),
        pane: m.pane.clone(),
        store: m.store.clone(),
        launch: m.launch.clone(),
        pre,
        config_dir,
        seen: seen.clone(),
    };
    opts.on_session = Some(Box::new(move |sd, session, program, _member| cb.run(sd, session, program)));
    gate("launch");
    let o = Launcher::run(adapter, opts);
    gate("returned");
    let exit = classify(&o, resumed.is_some());
    let session = seen.lock().unwrap_or_else(|e| e.into_inner()).take();
    let mut kept = None;
    if let Some(session) = session {
        let pane = m.pane.clone();
        let r = m.store.update_existing(WRAPPER_WAIT, &mut |ws| {
            let Some(rec) = ws.members.get_mut(&pane) else { anyhow::bail!("no member record for pane {pane}") };
            if !record_exit(rec, &session, exit) {
                anyhow::bail!("unchanged");
            }
            Ok(())
        });
        match r {
            Ok(Some(ws)) => {
                kept = ws.members.get(&m.pane).and_then(|r| r.agent_session.clone());
                // W§7.3 writer check, then the snapshot at agent exit (W§7.4, PW6 option A).
                after_write(&paths, &ws, &m.ws_id);
                agent_snapshot(&paths, &ws, &m.ws_id);
            }
            Ok(None) | Err(StateError::Refused(_)) => {}
            Err(e) => print_err(&format!(
                "zygo: could not save the exit record of pane {}: {}\n",
                m.pane,
                zygo_core::esc(&e.to_string())
            )),
        }
    }
    for l in exit_lines(kind, &o, exit, resumed.as_deref(), kept.as_deref(), shown_dir.as_deref()) {
        print_err(&format!("{l}\n"));
    }
    o
}

/// What the session callback of one wrapped launch needs (D3).
struct Callback {
    paths: Paths,
    ws_id: String,
    pane: String,
    store: Store,
    launch: zygo_workspace::state::Launch,
    pre: Option<String>,
    config_dir: Option<PathBuf>,
    seen: Arc<Mutex<Option<String>>>,
}

impl Callback {
    /// D3: the member record write first; only when it gave a state, the results link (AU16), the saved overrides
    /// (W§8.2 step 4, AU11). The errors of the later steps are joined with `; `.
    fn run(self, sd: &SessionDir, session: &str, program: &Path) -> anyhow::Result<()> {
        // D8: the saved path, not the link of the OpenCode program folder.
        let folder = self.paths.workspace_cache_dir().join(&self.ws_id).join("program");
        let recorded =
            if program.starts_with(&folder) { std::fs::read_link(program).unwrap_or_else(|_| program.to_path_buf()) } else { program.to_path_buf() };
        let start = Start {
            zygo_session: session.to_string(),
            agent_session: self.pre.clone(),
            started_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            config_dir: self.config_dir.clone(),
            program: recorded,
        };
        let pane = self.pane.clone();
        let launch = self.launch.clone();
        let written = self.store.update_existing(WRAPPER_WAIT, &mut |ws| {
            let Some(rec) = ws.members.get_mut(&pane) else { anyhow::bail!("no member record for pane {pane}") };
            record_start(rec, &launch, &start);
            Ok(())
        });
        let ws = match written {
            Ok(Some(ws)) => ws,
            Ok(None) => anyhow::bail!("no workspace state for {}; no member record written", self.ws_id),
            // The record is gone (`ws forget-pane`): no member, so nothing to see (D3).
            Err(e @ StateError::Refused(_)) => anyhow::bail!("{e}"),
            Err(e) => {
                // W§8.2 step 4: the agent starts with no record of this start, so no live check sees it.
                print_err(&format!(
                    "zygo: could not record the launch of pane {}: {}; zygo cannot see this agent as live\n",
                    self.pane,
                    zygo_core::esc(&e.to_string())
                ));
                anyhow::bail!("{e}")
            }
        };
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(session.to_string());
        let mut errors = Vec::new();
        if let Err(e) = results_link(&self.paths, &self.ws_id, &self.pane, sd) {
            errors.push(format!("results link: {e}"));
        }
        if let Some(ov) = ws.members.get(&self.pane).and_then(|r| r.overrides.clone()) {
            match serde_json::to_vec(&ov) {
                Ok(bytes) => {
                    if let Err(e) = write_atomic(&sd.overrides_json(), &bytes) {
                        errors.push(format!("overrides.json: {e}"));
                    }
                }
                Err(e) => errors.push(format!("overrides.json: {e}")),
            }
            if let Some(b) = ov.get("redaction_content").and_then(serde_json::Value::as_bool) {
                let sent = SessionClient::from_session_dir(&sd.root)
                    .and_then(|c| c.set_settings(&SessionSettings { redaction_content: Some(b) }));
                if let Err(e) = sent {
                    errors.push(format!("session settings: {e}"));
                }
            }
        }
        // W§7.3 writer check, the test gate (D9), then the snapshot at agent start (W§7.4).
        after_write(&self.paths, &ws, &self.ws_id);
        gate("callback");
        agent_snapshot(&self.paths, &ws, &self.ws_id);
        if errors.is_empty() { Ok(()) } else { Err(anyhow::anyhow!(errors.join("; "))) }
    }
}

/// The multiplexer of the state `ws` (AU62).
fn mux_of(ws: &Workspace) -> Tmux {
    Tmux::new(tmux_program(), ws.tmux_socket.clone())
}

/// The `snapshot.pending` check after a member write (W§7.3). The outcome and the error are ignored: the snapshot
/// writes its own `errors.log` line.
fn after_write(paths: &Paths, ws: &Workspace, ws_id: &str) {
    let _ = snapshot::after_write(paths, &mux_of(ws), ws_id, snapshot::HOOK_WAIT);
}

/// The snapshot at agent start or at agent exit (W§7.4, PW6 option A); outcome and error ignored as in
/// [`after_write`].
fn agent_snapshot(paths: &Paths, ws: &Workspace, ws_id: &str) {
    let _ = snapshot::run(paths, &mux_of(ws), ws_id, None, false, snapshot::HOOK_WAIT);
}

/// The test gate (D9, feature `test-hooks` only): with `ZYGO_TEST_MEMBER_GATE=<dir>` and `<dir>/<point>.hold`, the
/// wrapper writes `<dir>/<point>` and waits, polling every 10 ms, until the test removes `<point>.hold`. Points:
/// `program` (`ws run`, before the OpenCode program folder), `launch`, `callback`, `returned`.
#[cfg(feature = "test-hooks")]
pub(crate) fn gate(point: &str) {
    let Some(dir) = std::env::var_os("ZYGO_TEST_MEMBER_GATE").map(PathBuf::from) else { return };
    let hold = dir.join(format!("{point}.hold"));
    if !hold.exists() {
        return;
    }
    let _ = std::fs::write(dir.join(point), "");
    while hold.exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// D9: a build without `test-hooks` has no gate.
#[cfg(not(feature = "test-hooks"))]
pub(crate) fn gate(_point: &str) {}

/// AU16: the folder `<workspace cache>/<ws_id>/results/<pane>` and the absolute link `results` of the session folder
/// to it. `admin::pane_cache_create` makes or opens each part with mode 0700 and follows no link (the rule of
/// `ws forget-pane`); a refusal makes nothing.
#[cfg(unix)]
fn results_link(paths: &Paths, ws_id: &str, pane: &str, sd: &SessionDir) -> std::io::Result<()> {
    let dir = admin::pane_cache_create(paths, ws_id, "results", pane).map_err(std::io::Error::other)?;
    std::os::unix::fs::symlink(&dir, sd.results())
}

/// D11: the results link is unix only.
#[cfg(not(unix))]
fn results_link(_paths: &Paths, _ws_id: &str, _pane: &str, _sd: &SessionDir) -> std::io::Result<()> {
    Err(std::io::Error::other("needs unix"))
}

/// The 16 bytes `b` as a UUID version 4 (RFC 9562): the version 4 in byte 6, the variant `0b10` in byte 8; lowercase
/// hex in groups 8-4-4-4-12.
pub(crate) fn uuid_v4_from(mut b: [u8; 16]) -> String {
    b[6] = b[6] & 0x0f | 0x40;
    b[8] = b[8] & 0x3f | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// A fresh random session id for the pre-assign of a Claude Code member (W§8.3).
pub(crate) fn new_session_uuid() -> String {
    uuid_v4_from(rand::random())
}

/// How a member launch ended (AU17, AU60, AU73, W§8.2 step 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    /// A signal ended the agent or reached the launcher, or a resume failed fast.
    Resumable { failed_fast: bool },
    Ended,
    /// The agent was not spawned.
    NotStarted,
}

/// AU17, AU73, W§8.2 step 7: no spawn is `NotStarted`; a signal received by the launcher or one that ended the agent
/// is `Resumable`; a resume that exits non-zero within [`FAST_FAIL`] of its spawn failed fast; else `Ended`.
pub(crate) fn classify(o: &LaunchOutcome, resumed: bool) -> Exit {
    if !o.spawned {
        return Exit::NotStarted;
    }
    if o.received || o.signal.is_some() {
        return Exit::Resumable { failed_fast: false };
    }
    if resumed && o.code != 0 && o.ran.is_some_and(|r| r < FAST_FAIL) {
        return Exit::Resumable { failed_fast: true };
    }
    Exit::Ended
}

/// What the session callback records for one start (AU92).
pub(crate) struct Start {
    pub zygo_session: String,
    pub agent_session: Option<String>,
    pub started_at: u64,
    pub config_dir: Option<PathBuf>,
    pub program: PathBuf,
}

/// The start write (AU92): the AU30-filtered `launch`, `dropped_flags` (D4: the stored list stays when the filtered
/// launch equals the stored one, else the filter's list replaces it), `pending`, `config_dir` and `program`. Nothing
/// else changes.
pub(crate) fn record_start(rec: &mut MemberRecord, launch: &zygo_workspace::state::Launch, s: &Start) {
    let f = args::filter(launch.kind, &launch.args);
    let mut kept = launch.clone();
    kept.args = f.kept;
    if kept != rec.launch {
        rec.dropped_flags = f.dropped_flags;
    }
    rec.launch = kept;
    rec.pending = Some(Pending {
        zygo_session: s.zygo_session.clone(),
        agent_session: s.agent_session.clone(),
        started_at: s.started_at,
        ..Default::default()
    });
    rec.config_dir = s.config_dir.clone();
    rec.program = Some(s.program.clone());
}

/// The exit write (D5, AU60, AU92, H1 seam b-d). It changes a record only when its `pending` or its `zygo_session`
/// names `zygo_session`; it gives `false` with the record unchanged otherwise. `NotStarted` removes that `pending`.
/// Other exits commit that `pending` when no hook did (its `agent_session` only when set, never a stored id replaced
/// with none) and set the state; `closed_units`, `interrupted`, `clean_stop`, `saved_for` and `note_pending` stay.
pub(crate) fn record_exit(rec: &mut MemberRecord, zygo_session: &str, exit: Exit) -> bool {
    let names = |r: &MemberRecord| r.pending.as_ref().is_some_and(|p| p.zygo_session == zygo_session);
    if exit == Exit::NotStarted {
        if names(rec) {
            rec.pending = None;
            return true;
        }
        return false;
    }
    if names(rec)
        && let Some(p) = rec.pending.take()
    {
        rec.zygo_session = Some(p.zygo_session);
        if p.agent_session.is_some() {
            rec.agent_session = p.agent_session;
        }
    }
    if rec.zygo_session.as_deref() != Some(zygo_session) {
        return false;
    }
    rec.state = match exit {
        Exit::Resumable { .. } => MemberState::Resumable,
        _ => MemberState::Ended,
    };
    true
}

/// The effective `CLAUDE_CONFIG_DIR` of a launch environment (AU61): the non-empty value, joined to `cwd` when it is
/// relative; else none.
pub(crate) fn effective_config_dir(env: &Env, cwd: &Path) -> Option<PathBuf> {
    let v = env.get("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty())?;
    let p = PathBuf::from(v);
    Some(if p.is_absolute() { p } else { cwd.join(p) })
}

/// The `zygo` subcommand of an agent: `claude` or `opencode` (D7).
pub(crate) fn verb(kind: Agent) -> &'static str {
    match kind {
        Agent::ClaudeCode => "claude",
        Agent::OpenCode => "opencode",
    }
}

/// The raw bytes of `s` as one shell word for a printed command (M2 review round 3 finding 3; C6). Bytes that are
/// UTF-8 with no character that `zygo_core::esc` changes (control characters, bidi controls) give single quotes, a
/// `'` written as `'\''`. Otherwise the word is `$'…'`: each such character and each byte that is not UTF-8 as
/// `\xHH` per byte, `\` as `\\`, `'` as `\'`, the rest as it is. So the line stays one line, a terminal shows no
/// control text, and the word names the true bytes.
pub(crate) fn shell_word(s: &OsStr) -> String {
    let b = s.as_encoded_bytes();
    let escaped = |c: char| {
        let t = c.to_string();
        zygo_core::esc(&t) != t
    };
    if let Ok(t) = std::str::from_utf8(b)
        && !t.chars().any(escaped)
    {
        return format!("'{}'", t.replace('\'', r"'\''"));
    }
    let mut o = String::from("$'");
    for chunk in b.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '\\' => o.push_str(r"\\"),
                '\'' => o.push_str(r"\'"),
                c if escaped(c) => {
                    for x in c.to_string().bytes() {
                        o.push_str(&format!("\\x{x:02x}"));
                    }
                }
                c => o.push(c),
            }
        }
        for x in chunk.invalid() {
            o.push_str(&format!("\\x{x:02x}"));
        }
    }
    o.push('\'');
    o
}

/// A path as one shell word ([`shell_word`]).
pub(crate) fn quoted(p: &Path) -> String {
    shell_word(p.as_os_str())
}

/// The command that resumes the agent session `id` (D7, AU3, AU61, PW16), each path and the id as one shell word
/// ([`shell_word`] of the raw value): for Claude Code `CLAUDE_CONFIG_DIR=<dir> zygo claude -- --resume <id>` when the
/// member has a config dir, else `env -u CLAUDE_CONFIG_DIR zygo claude -- --resume <id>`, as `ws run` removes the
/// variable at a resume (AU61); for OpenCode `zygo opencode -- --session=<id>`.
pub(crate) fn resume_command(kind: Agent, id: &str, config_dir: Option<&Path>) -> String {
    let id = shell_word(OsStr::new(id));
    match (kind, config_dir) {
        (Agent::ClaudeCode, Some(d)) => format!("CLAUDE_CONFIG_DIR={} zygo claude -- --resume {id}", quoted(d)),
        (Agent::ClaudeCode, None) => format!("env -u CLAUDE_CONFIG_DIR zygo claude -- --resume {id}"),
        (Agent::OpenCode, _) => format!("zygo opencode -- --session={id}"),
    }
}

/// The lines `ws run` prints after the agent (D7). `resumed` is the id that this launch resumed; `kept` is the
/// `agent_session` of the record that the exit write gave back (none when it gave none or an error); `config_dir` is
/// the config dir of this launch, for the resume command.
pub(crate) fn exit_lines(
    kind: Agent, o: &LaunchOutcome, exit: Exit, resumed: Option<&str>, kept: Option<&str>, config_dir: Option<&Path>,
) -> Vec<String> {
    let k = verb(kind);
    let n = o.code;
    match exit {
        Exit::NotStarted => vec![format!("zygo: the {k} launch failed before the agent started (exit {n})")],
        Exit::Resumable { failed_fast: true } => match resumed {
            Some(id) => {
                let cmd = resume_command(kind, id, config_dir);
                let id = zygo_core::esc(id);
                vec![format!("zygo: resume of {k} session {id} failed (exit {n}); try: {cmd}")]
            }
            None => vec![],
        },
        Exit::Ended if n != 0 => match kept {
            Some(id) => {
                let cmd = resume_command(kind, id, config_dir);
                let id = zygo_core::esc(id);
                vec![format!("zygo: {k} exited {n}; agent session {id} is kept by the agent; to resume it: {cmd}")]
            }
            None => vec![],
        },
        _ => vec![],
    }
}

/// The zygo binary that the hooks and the plugin of a member run (M5 round 3, the rule of `ws run`): `exe` is
/// `current_exe()` then `canonicalize`, read before any update check, so a hook never names the deleted file after an
/// install; only when that read failed, the path of `current_exe()`.
pub(crate) fn zygo_binary(exe: &std::io::Result<PathBuf>) -> PathBuf {
    match exe {
        Ok(p) => p.clone(),
        Err(_) => std::env::current_exe().unwrap_or_default(),
    }
}

/// `%` and 1 to 10 ASCII digits (W§8.4): the tmux pane id of `$TMUX_PANE`; any other value is not a pane id and is
/// never looked up.
pub(crate) fn pane_id(v: &OsStr) -> Option<&str> {
    let s = v.to_str()?;
    let digits = s.strip_prefix('%')?;
    ((1..=10).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}

/// The workspace pane check of W§8.4 on the answer of `display -p '#{session_name} #{@zygo-ws}'`: the session is
/// `zygo-<name>` with a valid name, and the id is valid (AU84) and equal to `zygo_workspace`. Gives (session, id).
pub(crate) fn workspace_of(display: &str, zygo_workspace: &OsStr) -> Option<(String, String)> {
    let (session, id) = display.split_once(' ')?;
    let name = session.strip_prefix("zygo-")?;
    (valid_name(name) && valid_ws_id(id) && OsStr::new(id) == zygo_workspace).then(|| (session.to_string(), id.to_string()))
}

/// The launch's own session argument (AU94): the value of Claude Code `--resume`/`-r` or `--session-id`, or OpenCode
/// `--session`/`-s`, by AU30's walk (a flag value or a token after `--` is not a flag); only a flag with exactly one
/// value names a session, so `--resume` with no id (the picker) gives none.
pub(crate) fn own_session(kind: Agent, args: &[String]) -> Option<String> {
    let names: &[&str] = match kind {
        Agent::ClaudeCode => &["--resume", "--session-id"],
        Agent::OpenCode => &["--session"],
    };
    args::occurrences(kind, args)
        .into_iter()
        .find(|o| names.contains(&o.name) && o.values.len() == 1)
        .and_then(|o| o.values.into_iter().next())
}

/// The zygo session of a live launcher of `rec` (S9, the rule of `Store::live_launcher`): the first of its
/// `zygo_session` and its `pending.zygo_session` that is a valid session id and names a live session folder.
pub(crate) fn live_session(sessions_dir: &Path, rec: &MemberRecord) -> Option<String> {
    let committed = rec.zygo_session.as_deref();
    let pending = rec.pending.as_ref().map(|p| p.zygo_session.as_str());
    committed
        .into_iter()
        .chain(pending)
        .find(|id| valid_session_id(id) && zygo_core::session_dir::live(&sessions_dir.join(id)))
        .map(String::from)
}

/// Why a hand start may not use a pane key (AU94, W-C9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Conflict {
    /// The record is `running` or `resumable` with this agent session; `command` resumes it (AU61).
    Resumable { id: String, command: String },
    /// The record has a live launcher; `id` is its agent session, else the zygo session of that launcher.
    Live { id: String },
    /// The agent session `id` belongs to the record of the pane key `other`.
    OtherKey { id: String, other: String },
}

impl Conflict {
    /// The reason in `zygo: this agent is not saved by the workspace: <reason>` (W§8.4); ids escaped (C6).
    pub(crate) fn reason(&self, key: &str) -> String {
        match self {
            Conflict::Resumable { id, .. } => format!("pane {key} holds resumable session {}", zygo_core::esc(id)),
            Conflict::Live { id } => format!("pane {key} holds session {}, whose launcher is still running", zygo_core::esc(id)),
            Conflict::OtherKey { id, other } => format!("agent session {} belongs to pane {other}", zygo_core::esc(id)),
        }
    }

    /// The refusal line of a hand start (AU94, W-C9), without the line end.
    pub(crate) fn line(&self, key: &str) -> String {
        let reason = self.reason(key);
        match self {
            Conflict::Resumable { command, .. } => {
                format!("zygo: {reason}; to resume it: {command}; or run zygo ws forget-pane {key}")
            }
            Conflict::Live { .. } => format!("zygo: {reason}; zygo starts no second agent on it"),
            Conflict::OtherKey { other, .. } => {
                format!("zygo: {reason}; resume it in that pane, or run zygo ws forget-pane {other}")
            }
        }
    }
}

/// The AU94 and W-C9 checks of a hand start on the pane key `key` (D3): first a record under another key, not
/// `ended`, whose `agent_session` is the launch's own session argument `own`; then a live launcher on `key` (`live`
/// gives its zygo session); then a `running` or `resumable` record of `key` with an `agent_session` that is not
/// `own`. Any other record, or none, gives none.
pub(crate) fn conflict(
    ws: &Workspace, key: &str, own: Option<&str>, live: &dyn Fn(&MemberRecord) -> Option<String>,
) -> Option<Conflict> {
    if let Some(own) = own
        && let Some((other, _)) = ws
            .members
            .iter()
            .find(|(k, r)| k.as_str() != key && r.state != MemberState::Ended && r.agent_session.as_deref() == Some(own))
    {
        return Some(Conflict::OtherKey { id: own.to_string(), other: other.clone() });
    }
    let rec = ws.members.get(key)?;
    if let Some(z) = live(rec) {
        return Some(Conflict::Live { id: rec.agent_session.clone().unwrap_or(z) });
    }
    match &rec.agent_session {
        Some(a) if matches!(rec.state, MemberState::Running | MemberState::Resumable) && own != Some(a.as_str()) => {
            Some(Conflict::Resumable { id: a.clone(), command: resume_command(rec.launch.kind, a, rec.config_dir.as_deref()) })
        }
        _ => None,
    }
}

/// The one start write of a hand start (D2, W§8.4), in the closure of the callback's locked write: the phase must be
/// `up` or `draining`; then [`conflict`]; then the record of `key` is kept for the accepted resume (`own` is its
/// `agent_session`) and replaced by a new one otherwise; then state `running` and [`record_start`]. An `Err` holds the
/// reason, with `ws` unchanged.
pub(crate) fn join(
    ws: &mut Workspace, key: &str, launch: &zygo_workspace::state::Launch, start: &Start, own: Option<&str>,
    live: &dyn Fn(&MemberRecord) -> Option<String>,
) -> Result<(), String> {
    if !matches!(ws.phase, Phase::Up | Phase::Draining) {
        return Err(format!("a member starts only in phase up or draining, not {}", format!("{:?}", ws.phase).to_lowercase()));
    }
    if let Some(c) = conflict(ws, key, own, live) {
        return Err(c.reason(key));
    }
    let keep = own.is_some() && ws.members.get(key).is_some_and(|r| r.agent_session.as_deref() == own);
    if !keep {
        ws.members.remove(key);
    }
    let rec = ws.members.entry(key.to_string()).or_insert_with(|| MemberRecord::new(launch.kind));
    rec.state = MemberState::Running;
    record_start(rec, launch, start);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use zygo_core::Agent;
    use zygo_launch::env::Env;
    use zygo_launch::launcher::LaunchOutcome;
    use zygo_workspace::state::{Launch, MemberRecord, MemberState, Pending, Phase, Subagent};

    fn out(code: i32, signal: Option<i32>, received: bool, ran: u64) -> LaunchOutcome {
        LaunchOutcome { code, signal, spawned: true, received, ran: Some(Duration::from_secs(ran)) }
    }
    fn not_spawned(code: i32) -> LaunchOutcome {
        LaunchOutcome { code, signal: None, spawned: false, received: false, ran: None }
    }

    #[test]
    fn uuid_v4_from_sets_only_the_version_and_variant_bits() { // RFC 9562: version 4 in byte 6, variant 0b10 in byte 8
        let b: [u8; 16] = std::array::from_fn(|i| i as u8); // 0x00..=0x0f
        assert_eq!(uuid_v4_from(b), "00010203-0405-4607-8809-0a0b0c0d0e0f");
        assert_eq!(uuid_v4_from([0x00; 16]), "00000000-0000-4000-8000-000000000000");
        assert_eq!(uuid_v4_from([0xff; 16]), "ffffffff-ffff-4fff-bfff-ffffffffffff");
    }

    #[test]
    fn new_session_uuid_is_a_fresh_v4() {
        let re = regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$").unwrap();
        let (a, b) = (new_session_uuid(), new_session_uuid());
        assert!(re.is_match(&a) && re.is_match(&b), "{a} {b}");
        assert_ne!(a, b);
    }

    #[test]
    fn classify_follows_au17_and_the_fast_fail_rule() {
        assert_eq!(classify(&out(143, Some(15), true, 60), false), Exit::Resumable { failed_fast: false });
        assert_eq!(classify(&out(143, None, true, 60), false), Exit::Resumable { failed_fast: false }); // controller M3
        assert_eq!(classify(&out(0, None, true, 60), false), Exit::Resumable { failed_fast: false });
        assert_eq!(classify(&out(129, Some(1), false, 60), false), Exit::Resumable { failed_fast: false });
        assert_eq!(classify(&out(1, None, false, 9), true), Exit::Resumable { failed_fast: true });
        assert_eq!(classify(&out(1, None, false, 10), true), Exit::Ended);
        assert_eq!(classify(&out(0, None, false, 1), true), Exit::Ended);
        assert_eq!(classify(&out(1, None, false, 1), false), Exit::Ended);
        assert_eq!(classify(&out(130, None, false, 1), false), Exit::Ended); // AU73: a real 130 is not a signal
    }

    #[test]
    fn classify_without_a_spawn_is_not_started() { // controller M1
        assert_eq!(classify(&not_spawned(12), true), Exit::NotStarted);
        assert_eq!(classify(&not_spawned(13), true), Exit::NotStarted);
        assert_eq!(classify(&not_spawned(13), false), Exit::NotStarted);
    }

    #[test]
    fn record_start_writes_pending_with_the_filtered_launch() {
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        r.dropped_flags = vec!["--old".into()];
        let mut l = Launch::new(Agent::ClaudeCode);
        l.args = ["--model", "lead", "--resume", "x"].map(String::from).to_vec();
        l.compact = Some("600000".into());
        let start = |zs: &str| Start {
            zygo_session: zs.into(),
            agent_session: Some("U1".into()),
            started_at: 7,
            config_dir: Some("/c".into()),
            program: "/b/claude".into(),
        };
        record_start(&mut r, &l, &start("S1"));
        assert_eq!(
            r.pending,
            Some(Pending { zygo_session: "S1".into(), agent_session: Some("U1".into()), started_at: 7, ..Default::default() })
        );
        assert_eq!(r.launch.args, ["--model", "lead"]);
        assert_eq!(r.launch.compact.as_deref(), Some("600000"));
        assert!(r.dropped_flags.is_empty()); // D4
        assert_eq!((r.config_dir.clone(), r.program.clone()), (Some("/c".into()), Some("/b/claude".into())));
        assert_eq!((r.zygo_session.clone(), r.state), (None, MemberState::Ended));
        r.dropped_flags = vec!["--agents".into()];
        let saved = r.launch.clone();
        record_start(&mut r, &saved, &start("S2"));
        assert_eq!(r.dropped_flags, ["--agents"]); // D4
        assert_eq!(r.pending.as_ref().unwrap().zygo_session, "S2");
    }

    fn base() -> MemberRecord {
        let mut r = MemberRecord::new(Agent::OpenCode);
        r.zygo_session = Some("Z0".into());
        r.agent_session = Some("OLD".into());
        r.state = MemberState::Resumable;
        r.closed_units = vec!["u1".into()];
        r.interrupted = vec![Subagent { agent_id: "a1".into(), agent_type: "t1".into(), ..Default::default() }];
        r.clean_stop = true;
        r.saved_for = Some("Z0".into());
        r.note_pending = true;
        r.pending = Some(Pending { zygo_session: "S1".into(), agent_session: None, started_at: 1, ..Default::default() });
        r
    }

    #[test]
    fn record_exit_commits_without_clears_and_keeps_a_hook_id() { // H1 seam b, c, d
        let mut r = base();
        assert!(record_exit(&mut r, "S1", Exit::Ended));
        assert_eq!(
            (r.zygo_session.as_deref(), r.agent_session.as_deref(), r.pending.is_none(), r.state),
            (Some("S1"), Some("OLD"), true, MemberState::Ended)
        );
        assert_eq!(
            (r.closed_units.clone(), r.interrupted.len(), r.clean_stop, r.saved_for.as_deref(), r.note_pending),
            (vec!["u1".to_string()], 1, true, Some("Z0"), true)
        );
        let mut r = base();
        r.pending.as_mut().unwrap().agent_session = Some("NEW".into());
        assert!(record_exit(&mut r, "S1", Exit::Resumable { failed_fast: false }));
        assert_eq!((r.agent_session.as_deref(), r.state), (Some("NEW"), MemberState::Resumable));
        let mut r = base();
        r.pending = None;
        r.zygo_session = Some("S1".into());
        r.agent_session = Some("ses_A".into());
        assert!(record_exit(&mut r, "S1", Exit::Ended));
        assert_eq!((r.agent_session.as_deref(), r.state), (Some("ses_A"), MemberState::Ended));
        let mut r = base();
        let before = r.clone();
        assert!(record_exit(&mut r, "S1", Exit::NotStarted));
        assert_eq!(r, MemberRecord { pending: None, ..before }); // AU60, AU92
    }

    #[test]
    fn record_exit_changes_nothing_for_another_session() { // minor 3
        for exit in [Exit::Ended, Exit::Resumable { failed_fast: false }, Exit::NotStarted] {
            let mut r = base(); // pending names S1, zygo_session is Z0
            let before = r.clone();
            assert!(!record_exit(&mut r, "S9", exit));
            assert_eq!(r, before, "{exit:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn effective_config_dir_is_absolute_or_none() {
        let e = |v: &str| Env(vec![("CLAUDE_CONFIG_DIR".into(), v.into())]);
        let w = Path::new("/w");
        assert_eq!(effective_config_dir(&e("/a/b"), w), Some(PathBuf::from("/a/b")));
        assert_eq!(effective_config_dir(&e("rel"), w), Some(PathBuf::from("/w/rel")));
        assert_eq!(effective_config_dir(&e(""), w), None);
        assert_eq!(effective_config_dir(&Env::default(), w), None);
    }

    #[test]
    fn exit_lines_are_the_spec_texts() { // D7; review round 3 findings 1 and 2
        assert_eq!(
            exit_lines(Agent::ClaudeCode, &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("abc"), None, None),
            ["zygo: resume of claude session abc failed (exit 1); try: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume 'abc'"]
        );
        assert_eq!(
            exit_lines(Agent::OpenCode, &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("ses_1"), Some("ses_1"), None),
            ["zygo: resume of opencode session ses_1 failed (exit 1); try: zygo opencode -- --session='ses_1'"]
        );
        assert_eq!(
            exit_lines(Agent::OpenCode, &out(3, None, false, 60), Exit::Ended, None, Some("ses_1"), None),
            ["zygo: opencode exited 3; agent session ses_1 is kept by the agent; to resume it: zygo opencode -- --session='ses_1'"]
        );
        assert_eq!(
            exit_lines(Agent::ClaudeCode, &out(3, None, false, 60), Exit::Ended, None, Some("abc"), None),
            ["zygo: claude exited 3; agent session abc is kept by the agent; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume 'abc'"]
        );
        for code in [12, 13] {
            assert_eq!(
                exit_lines(Agent::ClaudeCode, &not_spawned(code), Exit::NotStarted, Some("abc"), None, None),
                [format!("zygo: the claude launch failed before the agent started (exit {code})")]
            );
        }
        assert!(exit_lines(Agent::ClaudeCode, &out(0, None, false, 60), Exit::Ended, None, Some("abc"), None).is_empty());
        assert!(
            exit_lines(Agent::ClaudeCode, &out(143, None, true, 60), Exit::Resumable { failed_fast: false }, None, Some("abc"), None)
                .is_empty()
        );
        assert!(exit_lines(Agent::ClaudeCode, &out(3, None, false, 60), Exit::Ended, None, None, None).is_empty()); // no saved record (D5)
        // The prose names the id escaped (C6); the command names it as one shell word.
        assert_eq!(
            exit_lines(Agent::OpenCode, &out(3, None, false, 60), Exit::Ended, None, Some("s'\n1"), None),
            [r"zygo: opencode exited 3; agent session s'\n1 is kept by the agent; to resume it: zygo opencode -- --session=$'s\'\x0a1'"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_resume_command_names_the_config_dir_quoted() { // AU61, PW16; review round 2 finding 2, round 3 findings 1-3
        let dir = Path::new("/c c/it's");
        assert_eq!(
            resume_command(Agent::ClaudeCode, "abc", Some(dir)),
            r"CLAUDE_CONFIG_DIR='/c c/it'\''s' zygo claude -- --resume 'abc'"
        );
        assert_eq!(resume_command(Agent::ClaudeCode, "abc", None), "env -u CLAUDE_CONFIG_DIR zygo claude -- --resume 'abc'");
        assert_eq!(resume_command(Agent::OpenCode, "ses_1", Some(dir)), "zygo opencode -- --session='ses_1'");
        assert_eq!(
            exit_lines(Agent::ClaudeCode, &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("abc"), None, Some(dir)),
            [r"zygo: resume of claude session abc failed (exit 1); try: CLAUDE_CONFIG_DIR='/c c/it'\''s' zygo claude -- --resume 'abc'"]
        );
        assert_eq!(
            exit_lines(Agent::ClaudeCode, &out(3, None, false, 60), Exit::Ended, None, Some("abc"), Some(dir)),
            [r"zygo: claude exited 3; agent session abc is kept by the agent; to resume it: CLAUDE_CONFIG_DIR='/c c/it'\''s' zygo claude -- --resume 'abc'"]
        );
        // A control byte: `$'…'` with `\xHH`, so the line stays one line and names the true path.
        assert_eq!(
            resume_command(Agent::ClaudeCode, "abc", Some(Path::new("/a\nb"))),
            r"CLAUDE_CONFIG_DIR=$'/a\x0ab' zygo claude -- --resume 'abc'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_word_quotes_the_raw_bytes() { // review round 3 finding 3
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let w = |b: &[u8]| shell_word(OsStr::from_bytes(b));
        assert_eq!(w(b"/a b"), "'/a b'");
        assert_eq!(w(b"it's"), r"'it'\''s'");
        assert_eq!(w(b""), "''");
        assert_eq!(w(b"a\\b"), r"'a\b'"); // no control byte: a backslash stays as it is in single quotes
        assert_eq!(w(b"/a\nb"), r"$'/a\x0ab'");
        assert_eq!(w(b"\t\x1b"), r"$'\x09\x1b'");
        assert_eq!(w(b"/a\xffb"), r"$'/a\xffb'"); // not UTF-8: the byte as it is
        assert_eq!(w(b"it's\n\\"), r"$'it\'s\x0a\\'"); // in `$'…'`, `'` and `\` are escaped
        assert_eq!(w("a\u{202e}b".as_bytes()), r"$'a\xe2\x80\xaeb'"); // a bidi control (C6) too
        assert_eq!(w("é".as_bytes()), "'é'"); // other UTF-8 as it is
        for b in [&b"/a\nb"[..], b"it's\n\\", b"/a\xffb", b"x\x7fy"] {
            assert!(!w(b).contains(['\n', '\r', '\x7f']), "{:?}", w(b));
        }
    }

    #[test]
    fn pane_id_takes_only_a_tmux_pane_id() {
        let p = |s: &str| pane_id(std::ffi::OsStr::new(s)).map(String::from);
        assert_eq!(p("%0"), Some("%0".into()));
        assert_eq!(p("%1234567890"), Some("%1234567890".into()));
        for bad in ["", "%", "%1x", "1", "%-1", "%12345678901", " %1", "=zygo-app:"] {
            assert_eq!(p(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn workspace_of_needs_a_zygo_session_and_the_same_id() {
        let id = "0123456789abcdef";
        let w = |out: &str, env: &str| workspace_of(out, std::ffi::OsStr::new(env));
        assert_eq!(w("zygo-app 0123456789abcdef", id), Some(("zygo-app".into(), id.into())));
        assert_eq!(w("zygo-my-app-2 0123456789abcdef", id), Some(("zygo-my-app-2".into(), id.into())));
        for (out, env) in [
            ("zygo-app ", id),
            ("plain 0123456789abcdef", id),
            ("zygo-App 0123456789abcdef", id),
            ("zygo- 0123456789abcdef", id),
            ("zygo-app 0123456789abcdef", "fedcba9876543210"),
            ("zygo-app 0123456789ABCDEF", "0123456789ABCDEF"),
            ("zygo-app 0123456789abcdef x", id),
            ("zygo-app 0123456789abcdef", ""),
        ] {
            assert_eq!(w(out, env), None, "{out:?} {env:?}");
        }
    }

    #[test]
    fn own_session_reads_the_session_argument_by_au30() {
        let s = |kind, a: &[&str]| own_session(kind, &a.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert_eq!(s(Agent::ClaudeCode, &["--resume", "abc"]), Some("abc".into())); // the printed command, unquoted by the shell
        assert_eq!(s(Agent::ClaudeCode, &["-r", "abc", "--model", "m"]), Some("abc".into()));
        assert_eq!(s(Agent::ClaudeCode, &["--resume=abc"]), Some("abc".into()));
        assert_eq!(s(Agent::ClaudeCode, &["--session-id", "u-1"]), Some("u-1".into()));
        assert_eq!(s(Agent::ClaudeCode, &["--resume"]), None); // the picker: no id
        assert_eq!(s(Agent::ClaudeCode, &["--model", "--resume"]), None); // a flag value is not a flag
        assert_eq!(s(Agent::ClaudeCode, &["--", "--resume", "abc"]), None);
        assert_eq!(s(Agent::OpenCode, &["--session=ses_1"]), Some("ses_1".into()));
        assert_eq!(s(Agent::OpenCode, &["-s", "ses_1"]), Some("ses_1".into()));
        assert_eq!(s(Agent::OpenCode, &["--resume", "abc"]), None);
    }

    fn ws_with(members: &[(&str, MemberRecord)]) -> Workspace {
        let mut w = Workspace::new("0123456789abcdef");
        w.phase = Phase::Up;
        for (k, r) in members {
            w.members.insert((*k).into(), r.clone());
        }
        w
    }

    fn rec(state: MemberState, agent: Option<&str>) -> MemberRecord {
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        r.state = state;
        r.agent_session = agent.map(String::from);
        r.zygo_session = Some("01JZZZZZZZZZZZZZZZZZZZZZZZ".into());
        r
    }

    #[test]
    fn conflict_follows_au94_and_w_c9() {
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let live = |_: &MemberRecord| Some("01JLLLLLLLLLLLLLLLLLLLLLLL".to_string());
        let cmd = "env -u CLAUDE_CONFIG_DIR zygo claude -- --resume 'abc'";
        let res = Some(Conflict::Resumable { id: "abc".into(), command: cmd.into() });
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]), k, None, &none), res);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Running, Some("abc")))]), k, None, &none), res);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]), k, Some("abc"), &none), None); // the printed resume
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]), k, Some("xyz"), &none), res);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Ended, Some("abc")))]), k, None, &none), None);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, None))]), k, None, &none), None);
        assert_eq!(conflict(&ws_with(&[]), k, None, &none), None);
        assert_eq!(
            conflict(&ws_with(&[(k, rec(MemberState::Ended, Some("abc")))]), k, Some("abc"), &live),
            Some(Conflict::Live { id: "abc".into() })
        );
        assert_eq!(
            conflict(&ws_with(&[(k, rec(MemberState::Ended, None))]), k, None, &live),
            Some(Conflict::Live { id: "01JLLLLLLLLLLLLLLLLLLLLLLL".into() })
        );
        let other = ws_with(&[("kbbbbbbbb", rec(MemberState::Resumable, Some("abc")))]);
        assert_eq!(conflict(&other, k, Some("abc"), &none), Some(Conflict::OtherKey { id: "abc".into(), other: "kbbbbbbbb".into() }));
        let ended = ws_with(&[("kbbbbbbbb", rec(MemberState::Ended, Some("abc")))]);
        assert_eq!(conflict(&ended, k, Some("abc"), &none), None);
        let mut c = rec(MemberState::Resumable, Some("abc"));
        c.config_dir = Some("/c c".into());
        assert_eq!(
            conflict(&ws_with(&[(k, c)]), k, None, &none),
            Some(Conflict::Resumable { id: "abc".into(), command: "CLAUDE_CONFIG_DIR='/c c' zygo claude -- --resume 'abc'".into() })
        );
    }

    #[test]
    fn conflict_lines_and_reasons_are_the_spec_texts() {
        let k = "kaaaaaaaa";
        let r = Conflict::Resumable { id: "a\nb".into(), command: "zygo opencode -- --session='x'".into() };
        assert_eq!(
            r.line(k),
            r"zygo: pane kaaaaaaaa holds resumable session a\nb; to resume it: zygo opencode -- --session='x'; or run zygo ws forget-pane kaaaaaaaa"
        );
        assert_eq!(r.reason(k), r"pane kaaaaaaaa holds resumable session a\nb");
        let l = Conflict::Live { id: "abc".into() };
        assert_eq!(l.line(k), "zygo: pane kaaaaaaaa holds session abc, whose launcher is still running; zygo starts no second agent on it");
        assert_eq!(l.reason(k), "pane kaaaaaaaa holds session abc, whose launcher is still running");
        let o = Conflict::OtherKey { id: "abc".into(), other: "kbbbbbbbb".into() };
        assert_eq!(o.line(k), "zygo: agent session abc belongs to pane kbbbbbbbb; resume it in that pane, or run zygo ws forget-pane kbbbbbbbb");
        assert_eq!(o.reason(k), "agent session abc belongs to pane kbbbbbbbb");
    }

    fn start() -> Start {
        Start { zygo_session: "S1".into(), agent_session: Some("U1".into()), started_at: 7, config_dir: None, program: "/b/claude".into() }
    }

    #[test]
    fn join_writes_one_record_with_every_start_field() { // decision 3
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut l = Launch::new(Agent::ClaudeCode);
        l.args = vec!["--model".into(), "m".into(), "--resume".into(), "x".into()];
        let mut ended = rec(MemberState::Ended, Some("old"));
        ended.overrides = Some(serde_json::json!({"results": false}));
        for w in [ws_with(&[]), ws_with(&[(k, ended)])] {
            let mut w = w;
            join(&mut w, k, &l, &start(), None, &none).unwrap();
            let m = &w.members[k];
            assert_eq!(
                (m.state, m.launch.kind, m.launch.args.clone()),
                (MemberState::Running, Agent::ClaudeCode, vec!["--model".to_string(), "m".to_string()])
            );
            assert_eq!(
                m.pending,
                Some(Pending { zygo_session: "S1".into(), agent_session: Some("U1".into()), started_at: 7, ..Default::default() })
            );
            assert_eq!(
                (m.program.clone(), m.zygo_session.clone(), m.agent_session.clone(), m.overrides.clone()),
                (Some("/b/claude".into()), None, None, None)
            );
        }
        // the accepted resume keeps the record's fields
        let mut keep = rec(MemberState::Resumable, Some("abc"));
        keep.overrides = Some(serde_json::json!({"results": false}));
        let mut w = ws_with(&[(k, keep)]);
        join(&mut w, k, &l, &Start { agent_session: None, ..start() }, Some("abc"), &none).unwrap();
        let m = &w.members[k];
        assert_eq!(
            (m.state, m.agent_session.as_deref(), m.overrides.clone()),
            (MemberState::Running, Some("abc"), Some(serde_json::json!({"results": false})))
        );
        assert_eq!(m.pending.as_ref().map(|p| p.zygo_session.as_str()), Some("S1"));
    }

    #[test]
    fn join_refuses_with_the_reason_and_changes_nothing() {
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let l = Launch::new(Agent::ClaudeCode);
        let mut w = ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]);
        let before = w.clone();
        assert_eq!(join(&mut w, k, &l, &start(), None, &none), Err("pane kaaaaaaaa holds resumable session abc".into()));
        assert_eq!(w, before);
        for (ph, word) in [(Phase::Down, "down"), (Phase::Building, "building"), (Phase::Stopping, "stopping")] {
            let mut w = ws_with(&[]);
            w.phase = ph;
            assert_eq!(join(&mut w, k, &l, &start(), None, &none), Err(format!("a member starts only in phase up or draining, not {word}")));
            assert!(w.members.is_empty());
        }
        let mut w = ws_with(&[]);
        w.phase = Phase::Draining;
        assert!(join(&mut w, k, &l, &start(), None, &none).is_ok());
    }

    #[test]
    fn live_session_names_the_live_zygo_session() { // S9: the rule of Store::live_launcher, state.rs:588-592
        let t = tempfile::tempdir().unwrap();
        let id = "01JLLLLLLLLLLLLLLLLLLLLLLL";
        let d = t.path().join(id);
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("token"), "t").unwrap();
        std::fs::write(d.join("pid"), "1").unwrap();
        let f = std::fs::File::options().read(true).write(true).open(d.join("pid")).unwrap();
        assert!(fs4::FileExt::try_lock(&f).is_ok());
        let mut r = rec(MemberState::Ended, None);
        assert_eq!(live_session(t.path(), &r), None); // zygo_session 01JZZZ… is not live
        r.pending = Some(Pending { zygo_session: id.into(), ..Default::default() });
        assert_eq!(live_session(t.path(), &r), Some(id.into()));
        r.pending = Some(Pending { zygo_session: "../x".into(), ..Default::default() });
        assert_eq!(live_session(t.path(), &r), None);
    }

    #[test]
    fn zygo_binary_is_the_canonical_path_read_before_else_current_exe() { // A1: the rule of ws run (M5)
        assert_eq!(zygo_binary(&Ok(PathBuf::from("/opt/zygo/bin/zygo"))), PathBuf::from("/opt/zygo/bin/zygo"));
        let failed: std::io::Result<PathBuf> = Err(std::io::Error::other("gone"));
        assert_eq!(zygo_binary(&failed), std::env::current_exe().unwrap());
    }
}
