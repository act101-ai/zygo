//! The member wrapper of `zygo ws run` (W§8.2 steps 4, 5 and 7): the member record at the start and at the exit of
//! the agent, the pre-assigned session id, the exit classification and the exit lines. The hand start of `zygo claude`
//! and `zygo opencode` in a workspace pane (W§8.4) uses the same wrapper.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zygo_core::paths::Paths;
use zygo_core::session_dir::{write_atomic, SessionDir};
use zygo_core::session_view::Member;
use zygo_core::Agent;
use zygo_launch::env::Env;
use zygo_launch::claude_code::{simple_mode, ClaudeCode};
use zygo_launch::launcher::{resume_argv, AgentSession, LaunchOutcome, NotMember};
use zygo_launch::opencode::OpenCode;
use zygo_launch::{AgentAdapter, LaunchOpts, Launcher};
use zygo_proxy::client::{SessionClient, SessionSettings};
use zygo_workspace::{admin, args};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::mux::{check_path, Multiplexer, UNSAFE_PATH_MESSAGE};
use zygo_workspace::{repair, snapshot, up};
use zygo_workspace::identity::{valid_name, valid_ws_id};
use zygo_workspace::state::{valid_session_id, MemberRecord, MemberState, Pending, Phase, StateError, Store, Workspace};

use super::{print_err, tmux_program};

/// A resume that fails with a non-zero code in less than this time from the agent's spawn failed fast (W§8.2 step 7).
const FAST_FAIL: Duration = Duration::from_secs(10);

/// The `lock` wait of the member wrapper's writes (W§7.3: launchers and the member wrapper 5 s).
const WRAPPER_WAIT: Duration = Duration::from_secs(5);

/// The line of a hand start whose server URL holds user info (AU98).
const AU98_LINE: &str = "zygo: --server holds user info; this agent is not saved by the workspace";

/// The head of the line of a hand start that goes on as a v1 launch (W§8.4).
const NOT_SAVED: &str = "zygo: this agent is not saved by the workspace: ";

/// The member of one wrapped launch: its workspace, its pane key, its state and its saved launch as typed (D1).
/// `hand` is `Some` for a hand start (W§8.4, M3 D1): its callback may create or replace the record of the key. `ws
/// run` passes `None`: its callback writes only a record that exists.
pub struct MemberCtx {
    pub ws_id: String,
    pub pane: String,
    pub store: Store,
    pub launch: zygo_workspace::state::Launch,
    pub hand: Option<HandJoin>,
}

/// What the callback of a hand start needs (M3 D1): the launch's own session argument (AU94).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandJoin {
    pub own_session: Option<String>,
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
    // A hand start: the record of the key before the callback's write, or none (review round 2, finding 5).
    let previous: Arc<Mutex<Option<MemberRecord>>> = Arc::new(Mutex::new(None));
    // The record as the callback's write left it (review round 3, findings 7 and 12).
    let written: Arc<Mutex<Option<MemberRecord>>> = Arc::new(Mutex::new(None));
    let paths = opts.paths.clone();
    let cb = Callback {
        paths: opts.paths.clone(),
        ws_id: m.ws_id.clone(),
        pane: m.pane.clone(),
        store: m.store.clone(),
        launch: m.launch.clone(),
        hand: m.hand.clone(),
        pre,
        config_dir,
        seen: seen.clone(),
        previous: previous.clone(),
        written: written.clone(),
    };
    opts.on_session = Some(Box::new(move |sd, session, program, _member, env| cb.run(sd, session, program, env)));
    gate("launch");
    let o = Launcher::run(adapter, opts);
    gate("returned");
    let exit = classify(&o, resumed.is_some());
    let session = seen.lock().unwrap_or_else(|e| e.into_inner()).take();
    let wrote = session.is_some();
    let written = written.lock().unwrap_or_else(|e| e.into_inner()).take();
    let mut kept = None;
    let mut after_exit = None;
    if let Some(session) = session {
        let pane = m.pane.clone();
        let previous = previous.lock().unwrap_or_else(|e| e.into_inner()).take();
        let r = m.store.update_existing(WRAPPER_WAIT, &mut |ws| {
            // Review round 2, finding 5; round 3, finding 12: a hand start whose agent never started puts the record
            // of the key back when nothing else wrote it since.
            if m.hand.is_some() && exit == Exit::NotStarted {
                let Some(w) = &written else { anyhow::bail!("unchanged") };
                if !undo_start(ws, &pane, w, previous.clone()) {
                    anyhow::bail!("unchanged");
                }
                return Ok(());
            }
            let Some(rec) = ws.members.get_mut(&pane) else { anyhow::bail!("no member record for pane {pane}") };
            if !record_exit(rec, &session, exit) {
                anyhow::bail!("unchanged");
            }
            Ok(())
        });
        match r {
            Ok(Some(ws)) => {
                after_exit = ws.members.get(&m.pane).cloned();
                kept = after_exit.as_ref().and_then(|r| r.agent_session.clone());
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
    // Review round 2, finding 4: a hand start prints the exit lines only when its callback wrote the member record;
    // `ws run` prints them as W§8.2 step 7 says, also for a launch that ends before its callback.
    // Review round 3, finding 7: the resume command is made from the record as it is after the exit write, else as the
    // start write left it (a hand resume keeps the saved launch), else from the launch of this member.
    if wrote || m.hand.is_none() {
        let rec = after_exit.or(written).unwrap_or_else(|| {
            let mut r = MemberRecord::new(m.launch.kind);
            r.launch = m.launch.clone();
            r
        });
        for l in exit_lines(&rec, &o, exit, resumed.as_deref(), kept.as_deref(), shown_dir.as_deref()) {
            print_err(&format!("{l}\n"));
        }
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
    hand: Option<HandJoin>,
    pre: Option<String>,
    config_dir: Option<PathBuf>,
    seen: Arc<Mutex<Option<String>>>,
    previous: Arc<Mutex<Option<MemberRecord>>>,
    written: Arc<Mutex<Option<MemberRecord>>>,
}

impl Callback {
    /// D3: the member record write first; only when it gave a state, the results link (AU16), the saved overrides
    /// (W§8.2 step 4, AU11). The errors of the later steps are joined with `; `. For a hand start the write is
    /// [`Callback::hand_write`]; when it refuses or fails, the callback prints the not-saved line and returns
    /// `NotMember`, so the launch goes on as a v1 launch (W§8.4, AU34).
    fn run(self, sd: &SessionDir, session: &str, program: &Path, env: &Env) -> anyhow::Result<()> {
        // FIX-24 diff review round 3, finding 3: bare mode from the environment that the agent gets. `env` is the launch
        // environment after the §8.4 deny list of crates/zygo-launch/src/env.rs (`child_env`), as the launcher passes it
        // to the session callback; the launcher sets no CLAUDE_CODE_SIMPLE itself (crates/zygo-launch/src/launcher.rs, the
        // `set` list). The rule is Claude Code's own (`zygo_launch::claude_code::simple_mode`, round 3, finding 1).
        let bare_env = self.launch.kind == Agent::ClaudeCode && simple_mode(env.get("CLAUDE_CODE_SIMPLE"));
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
            bare_env,
        };
        let pane = self.pane.clone();
        let launch = self.launch.clone();
        let written = match &self.hand {
            None => self.store.update_existing(WRAPPER_WAIT, &mut |ws| {
                let Some(rec) = ws.members.get_mut(&pane) else { anyhow::bail!("no member record for pane {pane}") };
                record_start(rec, &launch, &start);
                Ok(())
            }),
            Some(h) => match self.hand_write(&start, h) {
                Ok(ws) => Ok(Some(ws)),
                Err(reason) => {
                    print_err(&format!("{NOT_SAVED}{reason}\n"));
                    return Err(anyhow::Error::new(NotMember(reason)));
                }
            },
        };
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
        *self.written.lock().unwrap_or_else(|e| e.into_inner()) = ws.members.get(&self.pane).cloned();
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

    /// The start write of a hand start (W§8.4, M3 D2): [`join`] under `lock` with the 5 s bound of launchers, so the
    /// phase, AU94 and W-C9 are checked again in the same lock. `Err` holds the reason of the not-saved line: the
    /// refusal of `join`, `no workspace state for <id>`, or `could not write the member record: <error>`.
    fn hand_write(&self, start: &Start, h: &HandJoin) -> Result<Workspace, String> {
        let sessions = self.paths.sessions_dir.clone();
        let live = |r: &MemberRecord| live_session(&sessions, r);
        let mut refused: Option<String> = None;
        let mut previous: Option<MemberRecord> = None;
        let written = self.store.update_existing(WRAPPER_WAIT, &mut |ws| {
            previous = ws.members.get(&self.pane).cloned();
            join(ws, &self.pane, &self.launch, start, h.own_session.as_deref(), &live).map_err(|reason| {
                refused = Some(reason.clone());
                anyhow::anyhow!(reason)
            })
        });
        if written.as_ref().is_ok_and(Option::is_some) {
            *self.previous.lock().unwrap_or_else(|e| e.into_inner()) = previous;
        }
        match written {
            Ok(Some(ws)) => Ok(ws),
            Ok(None) => Err(format!("no workspace state for {}", self.ws_id)),
            Err(StateError::Refused(_)) if refused.is_some() => Err(refused.unwrap_or_default()),
            Err(e) => Err(format!("could not write the member record: {}", zygo_core::esc(&e.to_string()))),
        }
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
    /// A Claude Code start whose agent gets CLAUDE_CODE_SIMPLE with a value that turns bare mode on by Claude Code's rule
    /// (`zygo_launch::claude_code::simple_mode`), so no member hook runs (FIX-24, AU26).
    pub bare_env: bool,
}

/// The start write (AU92): the AU30-filtered launch with events_off computed from the arguments of this start (W§7.2),
/// `dropped_flags` (D4: the stored list stays when the filtered launch equals the stored one, else the filter's list
/// replaces it), `pending`, `config_dir` and `program`. Nothing else changes.
pub(crate) fn record_start(rec: &mut MemberRecord, launch: &zygo_workspace::state::Launch, s: &Start) {
    let f = args::filter(launch.kind, &launch.args);
    let mut kept = launch.clone();
    kept.args = f.kept;
    // FIX-24 D1: from the arguments of this start, before the filter, and its environment (round 2, finding 4);
    // always computed, so no stale value stays.
    kept.events_off = args::events_off(launch.kind, &launch.args) || s.bare_env;
    // FIX-24 diff review round 2, finding 2: the D4 compare ignores events_off, which describes the start, not the
    // saved launch.
    let same = zygo_workspace::state::Launch { events_off: rec.launch.events_off, ..kept.clone() } == rec.launch;
    if !same {
        rec.dropped_flags = f.dropped_flags;
    }
    rec.launch = kept;
    record_start_fields(rec, s);
}

/// The start fields of [`record_start`] without the launch: `pending`, `config_dir` and `program`. An accepted hand
/// resume writes only these, so the record keeps its saved launch (review round 2, finding 6(b)).
pub(crate) fn record_start_fields(rec: &mut MemberRecord, s: &Start) {
    rec.pending = Some(Pending {
        zygo_session: s.zygo_session.clone(),
        agent_session: s.agent_session.clone(),
        started_at: s.started_at,
        ..Default::default()
    });
    rec.config_dir = s.config_dir.clone();
    rec.program = Some(s.program.clone());
}

/// The undo of a hand start whose agent never started (review round 2, finding 5; round 3, finding 12). When the
/// record of `key` is exactly `written`, the record that the callback wrote, it becomes `previous` again, or the key
/// loses its record when there was none. When another write changed it since, only the `pending` of this start (the
/// zygo session of `written`) goes and the rest stays. Any other record is not changed. Gives whether it changed `ws`.
pub(crate) fn undo_start(ws: &mut Workspace, key: &str, written: &MemberRecord, previous: Option<MemberRecord>) -> bool {
    let Some(session) = written.pending.as_ref().map(|p| p.zygo_session.clone()) else { return false };
    if ws.members.get(key) == Some(written) {
        match previous {
            Some(r) => {
                ws.members.insert(key.to_string(), r);
            }
            None => {
                ws.members.remove(key);
            }
        }
        return true;
    }
    match ws.members.get_mut(key) {
        Some(r) if r.pending.as_ref().is_some_and(|p| p.zygo_session == session) => {
            r.pending = None;
            true
        }
        _ => false,
    }
}

/// The exit write (D5, AU60, AU92, H1 seam b-d). It changes a record only when its `pending` or its `zygo_session`
/// names `zygo_session`; it gives `false` with the record unchanged otherwise. `NotStarted` removes that `pending`.
/// Other exits commit that `pending` when no hook did (its `agent_session` only when set and not the user's own session
/// argument, which only a hook commits; never a stored id replaced with none) and set the state; `closed_units`,
/// `interrupted`, `clean_stop`, `saved_for` and `note_pending` stay.
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
        // Review round 3, finding 6: a typed id that the agent never confirmed is not saved.
        if p.agent_session.is_some() && !p.own_session {
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

/// The command that resumes the agent session `id` of the member `rec` (D7, AU3, AU61, PW16): the `zygo` launch that
/// reproduces the `ws run` resume of `rec`. Its flags are those of `ws::launch_of` (Claude Code) or
/// `ws::launch_open_code_of` (OpenCode), the functions that `ws run` gives `launch_opts`: `--profile`,
/// `--passthrough`, `--server` (the saved value), `--compact` (the effective compaction of AU32; none when cleared),
/// `--yolo`. After `--` come the agent arguments of [`resume_argv`]: the saved `launch.args` with the resume tokens of
/// the adapter where the launcher puts them. Each value is one shell word ([`shell_word`] of the raw bytes); a resume
/// token is `<flag>=<id word>`. For Claude Code the command starts with `CLAUDE_CONFIG_DIR=<dir>` when the member has
/// a config dir, else `env -u CLAUDE_CONFIG_DIR`, as `ws run` sets the variable at a resume (AU61).
pub(crate) fn resume_command(rec: &MemberRecord, id: &str, config_dir: Option<&Path>) -> String {
    let word = |v: &str| shell_word(OsStr::new(v));
    let mut flags = String::new();
    let mut flag = |name: &str, value: Option<&str>| match value {
        Some(v) => flags.push_str(&format!(" --{name} {}", word(v))),
        None => flags.push_str(&format!(" --{name}")),
    };
    let (args, inserted) = match rec.launch.kind {
        Agent::ClaudeCode => {
            let l = super::ws::launch_of(rec, true);
            if let Some(p) = &l.profile {
                flag("profile", Some(p));
            }
            if l.passthrough {
                flag("passthrough", None);
            }
            if let Some(s) = &l.server {
                flag("server", Some(s));
            }
            if let Some(c) = &l.compact {
                flag("compact", Some(c));
            }
            if l.yolo {
                flag("yolo", None);
            }
            resume_argv(&ClaudeCode::for_zygo(PathBuf::new()), &l.rest, id)
        }
        Agent::OpenCode => {
            let l = super::ws::launch_open_code_of(rec);
            if let Some(p) = &l.profile {
                flag("profile", Some(p));
            }
            if let Some(s) = &l.server {
                flag("server", Some(s));
            }
            if let Some(c) = &l.compact {
                flag("compact", Some(c));
            }
            if l.yolo {
                flag("yolo", None);
            }
            resume_argv(&OpenCode::new(PathBuf::new()), &l.rest, id)
        }
    };
    let words: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, a)| match a.split_once('=') {
            Some((f, v)) if inserted.contains(&i) => format!("{f}={}", word(v)),
            _ => word(a),
        })
        .collect();
    let tail = format!("{flags} -- {}", words.join(" "));
    match (rec.launch.kind, config_dir) {
        (Agent::ClaudeCode, Some(d)) => format!("CLAUDE_CONFIG_DIR={} zygo claude{tail}", quoted(d)),
        (Agent::ClaudeCode, None) => format!("env -u CLAUDE_CONFIG_DIR zygo claude{tail}"),
        (Agent::OpenCode, _) => format!("zygo opencode{tail}"),
    }
}

/// The lines `ws run` prints after the agent (D7). `rec` is the member record whose launch the resume command
/// reproduces; `resumed` is the id that this launch resumed; `kept` is the `agent_session` of the record that the exit
/// write gave back (none when it gave none or an error); `config_dir` is the config dir of this launch, for the resume
/// command.
pub(crate) fn exit_lines(
    rec: &MemberRecord, o: &LaunchOutcome, exit: Exit, resumed: Option<&str>, kept: Option<&str>, config_dir: Option<&Path>,
) -> Vec<String> {
    let k = verb(rec.launch.kind);
    let n = o.code;
    match exit {
        Exit::NotStarted => vec![format!("zygo: the {k} launch failed before the agent started (exit {n})")],
        Exit::Resumable { failed_fast: true } => match resumed {
            Some(id) => {
                let cmd = resume_command(rec, id, config_dir);
                let id = zygo_core::esc(id);
                vec![format!("zygo: resume of {k} session {id} failed (exit {n}); try: {cmd}")]
            }
            None => vec![],
        },
        Exit::Ended if n != 0 => match kept {
            Some(id) => {
                let cmd = resume_command(rec, id, config_dir);
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

/// The AU94 and W-C9 checks of a hand start of agent kind `kind` on the pane key `key` (D3): first a record under
/// another key, not `ended`, of any agent kind, whose `agent_session` is the launch's own session argument `own`, or
/// whose `pending.agent_session` is `own` while the launcher of that pending start is live (`live` of the record
/// without its committed `zygo_session`); then a live launcher on `key` (`live` gives its zygo session); then a `running` or
/// `resumable` record of `key` with an `agent_session`, unless that id is `own` and the record's kind is `kind`. Any
/// other record, or none, gives none.
pub(crate) fn conflict(
    ws: &Workspace, key: &str, kind: Agent, own: Option<&str>, live: &dyn Fn(&MemberRecord) -> Option<String>,
) -> Option<Conflict> {
    // W-C9 (review round 2, finding 1): the committed id, and the id of a start that is not committed yet while the
    // launcher of that start is live (review round 3, finding 5: a start whose launcher died holds nothing).
    let pending_live = |r: &MemberRecord| {
        let mut start = r.clone();
        start.zygo_session = None;
        live(&start).is_some()
    };
    let holds = |r: &MemberRecord, id: &str| {
        r.agent_session.as_deref() == Some(id)
            || (r.pending.as_ref().and_then(|p| p.agent_session.as_deref()) == Some(id) && pending_live(r))
    };
    if let Some(own) = own
        && let Some((other, _)) =
            ws.members.iter().find(|(k, r)| k.as_str() != key && r.state != MemberState::Ended && holds(r, own))
    {
        return Some(Conflict::OtherKey { id: own.to_string(), other: other.clone() });
    }
    let rec = ws.members.get(key)?;
    if let Some(z) = live(rec) {
        return Some(Conflict::Live { id: rec.agent_session.clone().unwrap_or(z) });
    }
    // AU94: only a start of the record's own agent kind with its id is the printed resume (review round 2, finding 2).
    let own_resume = |a: &str| own == Some(a) && rec.launch.kind == kind;
    match &rec.agent_session {
        Some(a) if matches!(rec.state, MemberState::Running | MemberState::Resumable) && !own_resume(a) => {
            Some(Conflict::Resumable { id: a.clone(), command: resume_command(rec, a, rec.config_dir.as_deref()) })
        }
        _ => None,
    }
}

/// The one start write of a hand start (D2, W§8.4), in the closure of the callback's locked write: the phase must be
/// `up` or `draining`; then [`conflict`]; then the record of `key` is kept for the accepted resume (`own` is its
/// `agent_session` and its kind is the launch's kind) and replaced by a new one otherwise; then state `running`, and
/// [`record_start_fields`] for a kept record (its saved launch stays, except launch.events_off, which the typed
/// arguments of this start give, W§7.2) or [`record_start`] for a new one; then `own`
/// becomes `pending.agent_session` when the start has no pre-assign (W-C9). An `Err` holds the reason, with `ws`
/// unchanged.
pub(crate) fn join(
    ws: &mut Workspace, key: &str, launch: &zygo_workspace::state::Launch, start: &Start, own: Option<&str>,
    live: &dyn Fn(&MemberRecord) -> Option<String>,
) -> Result<(), String> {
    if !matches!(ws.phase, Phase::Up | Phase::Draining) {
        return Err(format!("a member starts only in phase up or draining, not {}", format!("{:?}", ws.phase).to_lowercase()));
    }
    if let Some(c) = conflict(ws, key, launch.kind, own, live) {
        return Err(c.reason(key));
    }
    let keep = own.is_some()
        && ws.members.get(key).is_some_and(|r| r.agent_session.as_deref() == own && r.launch.kind == launch.kind);
    if !keep {
        ws.members.remove(key);
    }
    let rec = ws.members.entry(key.to_string()).or_insert_with(|| MemberRecord::new(launch.kind));
    rec.state = MemberState::Running;
    if keep {
        // The accepted resume keeps the saved launch (review round 2, finding 6(b)).
        record_start_fields(rec, start);
        // FIX-24 D1: the saved launch stays; events_off describes the arguments typed for this start and its
        // environment (round 2, finding 4).
        rec.launch.events_off = args::events_off(launch.kind, &launch.args) || start.bare_env;
    } else {
        record_start(rec, launch, start);
    }
    // W-C9: the own session argument is the agent session of this start until a hook commits or replaces it (review
    // round 2, finding 1).
    if let (Some(p), Some(o)) = (rec.pending.as_mut(), own)
        && p.agent_session.is_none()
    {
        p.agent_session = Some(o.to_string());
        // Review round 3, finding 6: only a member hook commits it.
        p.own_session = true;
    }
    Ok(())
}

/// The outcome of the W§8.4 checks of a hand start.
pub(crate) enum HandStart {
    /// Not a member: a v1 launch.
    V1,
    /// A member with its pane key.
    Member(Box<MemberCtx>),
    /// AU94 or W-C9 refuses: print the line, exit 1, start nothing.
    Refused(String),
}

/// The not-saved line of W§8.4 with `reason`.
fn not_saved(reason: &str) {
    print_err(&format!("{NOT_SAVED}{reason}\n"));
}

/// The W§8.4 checks of a hand start, in the order of the spec. `env` reads the process environment; `tmux` gives the
/// tmux program and is called only after the `ZYGO_SESSION`, `TMUX`, `TMUX_PANE` and `ZYGO_WORKSPACE` checks (D8). A
/// failure before the pane is known as a workspace pane is a silent v1 launch, except a state whose `tmux_socket`
/// fails AU54 or is not the socket of `$TMUX`, and a socket of `$TMUX` that fails AU54 or AU62; later failures print
/// the not-saved line (paths and ids escaped, C6). Nothing is written before the pane key (W§7.4 step 4, AU79); the record is written by the callback.
pub(crate) fn hand_start(
    launch: zygo_workspace::state::Launch, env: &dyn Fn(&str) -> Option<OsString>, paths: &Paths, tmux: &dyn Fn() -> PathBuf,
) -> HandStart {
    if !cfg!(unix) || env("ZYGO_SESSION").is_some() {
        return HandStart::V1;
    }
    let Some(tmux_env) = env("TMUX").filter(|v| !v.is_empty()) else { return HandStart::V1 };
    let Some(pane_var) = env("TMUX_PANE") else { return HandStart::V1 };
    let Some(pane) = pane_id(&pane_var) else { return HandStart::V1 };
    let Some(ws_var) = env("ZYGO_WORKSPACE") else { return HandStart::V1 };
    let Some(id) = ws_var.to_str().filter(|v| valid_ws_id(v)) else { return HandStart::V1 };
    let tmux = tmux();
    let Some(sock) = up::server(&tmux, None, Some(&tmux_env)).socket().map(Path::to_path_buf) else {
        return HandStart::V1;
    };
    let store = Store::open(paths, id);
    let read = store.read();
    // FIX-30 (AU54): the saved socket before any use; the compare below prints it.
    if let Ok(Some(ws)) = &read
        && let Err(e) = repair::check_saved_socket(ws)
    {
        not_saved(e.to_string().trim_start_matches("zygo: "));
        return HandStart::V1;
    }
    if let Ok(Some(ws)) = &read
        && ws.tmux_socket.as_deref() != Some(sock.as_path())
    {
        let a = ws.tmux_socket.as_deref().map_or_else(|| "none".to_string(), |p| p.display().to_string());
        not_saved(&format!(
            "the workspace state names tmux server {}, not {}",
            zygo_core::esc(&a),
            zygo_core::esc(&sock.display().to_string())
        ));
        return HandStart::V1;
    }
    // FIX-30 (AU54): the socket of `$TMUX` before its first use (AU62, then `display` at the next lines). A saved
    // socket reaches here only when it equals `sock` and passed the check above; this one matters for a state that is
    // missing or cannot be read.
    if check_path(&sock).is_err() {
        not_saved(UNSAFE_PATH_MESSAGE.trim_start_matches("zygo: "));
        return HandStart::V1;
    }
    if let Err(t) = up::socket_is_private(&sock, false) {
        not_saved(&zygo_core::esc(&t));
        return HandStart::V1;
    }
    let mux = Tmux::new(tmux, Some(sock));
    let Ok(answer) = mux.display(pane, "#{session_name} #{@zygo-ws}") else { return HandStart::V1 };
    let Some((session, _)) = workspace_of(&answer, &ws_var) else { return HandStart::V1 };
    if launch.server.as_deref().is_some_and(args::has_user_info) {
        print_err(&format!("{AU98_LINE}\n"));
        return HandStart::V1;
    }
    let ws = match read {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            not_saved(&format!("no workspace state for {id}"));
            return HandStart::V1;
        }
        Err(e) => {
            not_saved(&zygo_core::esc(&e.to_string()));
            return HandStart::V1;
        }
    };
    if ws.tmux_session != session {
        not_saved(&format!(
            "the workspace state names tmux session {}, not {}",
            zygo_core::esc(&ws.tmux_session),
            zygo_core::esc(&session)
        ));
        return HandStart::V1;
    }
    let key = match snapshot::assign_key(&store, &mux, pane) {
        Ok(k) => k,
        Err(e) => {
            not_saved(&zygo_core::esc(&e.to_string()));
            return HandStart::V1;
        }
    };
    let own = own_session(launch.kind, &launch.args);
    if let Some(c) = conflict(&ws, &key, launch.kind, own.as_deref(), &|r| live_session(&paths.sessions_dir, r)) {
        return HandStart::Refused(c.line(&key));
    }
    HandStart::Member(Box::new(MemberCtx { ws_id: id.to_string(), pane: key, store, launch, hand: Some(HandJoin { own_session: own }) }))
}

/// `zygo claude` and `zygo opencode` (W§8.4): the hand-start checks, then a v1 launch, the member wrapper, or the
/// refusal (exit 1). `adapter` makes the adapter for a zygo binary path: a member gets the path by the rule of `ws run`
/// ([`zygo_binary`], M3 A1); a v1 launch gets `current_exe()` as before. The update check of these verbs runs before
/// this and makes no check when `ZYGO_WORKSPACE` is set, so a member's binary is never replaced before the read.
pub(crate) fn launch_hand(
    adapter: &dyn Fn(PathBuf) -> Box<dyn AgentAdapter>, opts: LaunchOpts, launch: zygo_workspace::state::Launch,
) -> i32 {
    let paths = opts.paths.clone();
    match hand_start(launch, &|k| std::env::var_os(k), &paths, &tmux_program) {
        HandStart::V1 => Launcher::run(adapter(std::env::current_exe().unwrap_or_default()).as_ref(), opts).code,
        HandStart::Member(m) => {
            let zygo = zygo_binary(&std::env::current_exe().and_then(std::fs::canonicalize));
            run_wrapped(adapter(zygo).as_ref(), opts, &m).code
        }
        HandStart::Refused(line) => {
            print_err(&format!("{line}\n"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use zygo_core::Agent;
    use zygo_launch::env::Env;
    use zygo_launch::launcher::LaunchOutcome;
    use zygo_workspace::state::{CompactOverride, Launch, MemberRecord, MemberState, Pending, Phase, Subagent};

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
            bare_env: false,
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
            exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("abc"), None, None),
            ["zygo: resume of claude session abc failed (exit 1); try: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'"]
        );
        assert_eq!(
            exit_lines(&MemberRecord::new(Agent::OpenCode), &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("ses_1"), Some("ses_1"), None),
            ["zygo: resume of opencode session ses_1 failed (exit 1); try: zygo opencode -- --session='ses_1'"]
        );
        assert_eq!(
            exit_lines(&MemberRecord::new(Agent::OpenCode), &out(3, None, false, 60), Exit::Ended, None, Some("ses_1"), None),
            ["zygo: opencode exited 3; agent session ses_1 is kept by the agent; to resume it: zygo opencode -- --session='ses_1'"]
        );
        assert_eq!(
            exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(3, None, false, 60), Exit::Ended, None, Some("abc"), None),
            ["zygo: claude exited 3; agent session abc is kept by the agent; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'"]
        );
        for code in [12, 13] {
            assert_eq!(
                exit_lines(&MemberRecord::new(Agent::ClaudeCode), &not_spawned(code), Exit::NotStarted, Some("abc"), None, None),
                [format!("zygo: the claude launch failed before the agent started (exit {code})")]
            );
        }
        assert!(exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(0, None, false, 60), Exit::Ended, None, Some("abc"), None).is_empty());
        assert!(
            exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(143, None, true, 60), Exit::Resumable { failed_fast: false }, None, Some("abc"), None)
                .is_empty()
        );
        assert!(exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(3, None, false, 60), Exit::Ended, None, None, None).is_empty()); // no saved record (D5)
        // The prose names the id escaped (C6); the command names it as one shell word.
        assert_eq!(
            exit_lines(&MemberRecord::new(Agent::OpenCode), &out(3, None, false, 60), Exit::Ended, None, Some("s'\n1"), None),
            [r"zygo: opencode exited 3; agent session s'\n1 is kept by the agent; to resume it: zygo opencode -- --session=$'s\'\x0a1'"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_resume_command_names_the_config_dir_quoted() { // AU61, PW16; review round 2 finding 2, round 3 findings 1-3
        let dir = Path::new("/c c/it's");
        assert_eq!(
            resume_command(&MemberRecord::new(Agent::ClaudeCode), "abc", Some(dir)),
            r"CLAUDE_CONFIG_DIR='/c c/it'\''s' zygo claude -- --resume='abc'"
        );
        assert_eq!(resume_command(&MemberRecord::new(Agent::ClaudeCode), "abc", None), "env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'");
        assert_eq!(resume_command(&MemberRecord::new(Agent::OpenCode), "ses_1", Some(dir)), "zygo opencode -- --session='ses_1'");
        assert_eq!(
            exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("abc"), None, Some(dir)),
            [r"zygo: resume of claude session abc failed (exit 1); try: CLAUDE_CONFIG_DIR='/c c/it'\''s' zygo claude -- --resume='abc'"]
        );
        assert_eq!(
            exit_lines(&MemberRecord::new(Agent::ClaudeCode), &out(3, None, false, 60), Exit::Ended, None, Some("abc"), Some(dir)),
            [r"zygo: claude exited 3; agent session abc is kept by the agent; to resume it: CLAUDE_CONFIG_DIR='/c c/it'\''s' zygo claude -- --resume='abc'"]
        );
        // A control byte: `$'…'` with `\xHH`, so the line stays one line and names the true path.
        assert_eq!(
            resume_command(&MemberRecord::new(Agent::ClaudeCode), "abc", Some(Path::new("/a\nb"))),
            r"CLAUDE_CONFIG_DIR=$'/a\x0ab' zygo claude -- --resume='abc'"
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

    /// The `live` input of [`conflict`] as [`live_session`] gives it when the zygo sessions `ids` are live: the first of
    /// the record's `zygo_session` and `pending.zygo_session` that is one of them.
    fn live_of(ids: &'static [&'static str]) -> impl Fn(&MemberRecord) -> Option<String> {
        move |r: &MemberRecord| {
            r.zygo_session
                .iter()
                .map(String::as_str)
                .chain(r.pending.as_ref().map(|p| p.zygo_session.as_str()))
                .find(|z| ids.contains(z))
                .map(String::from)
        }
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
        let cmd = "env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'";
        let res = Some(Conflict::Resumable { id: "abc".into(), command: cmd.into() });
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]), k, Agent::ClaudeCode, None, &none), res);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Running, Some("abc")))]), k, Agent::ClaudeCode, None, &none), res);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]), k, Agent::ClaudeCode, Some("abc"), &none), None); // the printed resume
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]), k, Agent::ClaudeCode, Some("xyz"), &none), res);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Ended, Some("abc")))]), k, Agent::ClaudeCode, None, &none), None);
        assert_eq!(conflict(&ws_with(&[(k, rec(MemberState::Resumable, None))]), k, Agent::ClaudeCode, None, &none), None);
        assert_eq!(conflict(&ws_with(&[]), k, Agent::ClaudeCode, None, &none), None);
        assert_eq!(
            conflict(&ws_with(&[(k, rec(MemberState::Ended, Some("abc")))]), k, Agent::ClaudeCode, Some("abc"), &live),
            Some(Conflict::Live { id: "abc".into() })
        );
        assert_eq!(
            conflict(&ws_with(&[(k, rec(MemberState::Ended, None))]), k, Agent::ClaudeCode, None, &live),
            Some(Conflict::Live { id: "01JLLLLLLLLLLLLLLLLLLLLLLL".into() })
        );
        let other = ws_with(&[("kbbbbbbbb", rec(MemberState::Resumable, Some("abc")))]);
        assert_eq!(conflict(&other, k, Agent::ClaudeCode, Some("abc"), &none), Some(Conflict::OtherKey { id: "abc".into(), other: "kbbbbbbbb".into() }));
        let ended = ws_with(&[("kbbbbbbbb", rec(MemberState::Ended, Some("abc")))]);
        assert_eq!(conflict(&ended, k, Agent::ClaudeCode, Some("abc"), &none), None);
        let mut c = rec(MemberState::Resumable, Some("abc"));
        c.config_dir = Some("/c c".into());
        assert_eq!(
            conflict(&ws_with(&[(k, c)]), k, Agent::ClaudeCode, None, &none),
            Some(Conflict::Resumable { id: "abc".into(), command: "CLAUDE_CONFIG_DIR='/c c' zygo claude -- --resume='abc'".into() })
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
        Start {
            zygo_session: "S1".into(),
            agent_session: Some("U1".into()),
            started_at: 7,
            config_dir: None,
            program: "/b/claude".into(),
            bare_env: false,
        }
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

    #[test]
    fn conflict_reads_pending_agent_sessions_and_the_kind() { // diff review round 2, findings 1 and 2
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let cc = Agent::ClaudeCode;
        let other = |id: &str| Some(Conflict::OtherKey { id: id.into(), other: "kbbbbbbbb".into() });
        // W-C9: an own session that another key's start wrote as `pending.agent_session` (not committed yet).
        let mut p = rec(MemberState::Running, None);
        p.pending = Some(Pending { zygo_session: "S9".into(), agent_session: Some("abc".into()), ..Default::default() });
        assert_eq!(conflict(&ws_with(&[("kbbbbbbbb", p.clone())]), k, cc, Some("abc"), &live_of(&["S9"])), other("abc"));
        // Round 3, finding 5: a pending id whose launcher is not live (a stale start) does not hold it; a live launcher
        // of the committed session is not the launcher of that start.
        assert_eq!(conflict(&ws_with(&[("kbbbbbbbb", p.clone())]), k, cc, Some("abc"), &none), None);
        assert_eq!(conflict(&ws_with(&[("kbbbbbbbb", p.clone())]), k, cc, Some("abc"), &live_of(&["01JZZZZZZZZZZZZZZZZZZZZZZZ"])), None);
        // A committed id holds it with no live launcher.
        let mut c = rec(MemberState::Resumable, Some("abc"));
        c.pending = None;
        assert_eq!(conflict(&ws_with(&[("kbbbbbbbb", c)]), k, cc, Some("abc"), &none), other("abc"));
        p.state = MemberState::Ended;
        assert_eq!(conflict(&ws_with(&[("kbbbbbbbb", p)]), k, cc, Some("abc"), &live_of(&["S9"])), None);
        // AU94: the own session of a record of another agent kind is not the printed resume.
        let mut oc = MemberRecord::new(Agent::OpenCode);
        oc.state = MemberState::Resumable;
        oc.agent_session = Some("ses_1".into());
        assert_eq!(
            conflict(&ws_with(&[(k, oc.clone())]), k, cc, Some("ses_1"), &none),
            Some(Conflict::Resumable { id: "ses_1".into(), command: "zygo opencode -- --session='ses_1'".into() })
        );
        assert_eq!(conflict(&ws_with(&[(k, oc.clone())]), k, Agent::OpenCode, Some("ses_1"), &none), None);
        // W-C9 across kinds.
        assert_eq!(conflict(&ws_with(&[("kbbbbbbbb", oc)]), k, cc, Some("ses_1"), &none), other("ses_1"));
    }

    #[test]
    fn join_writes_the_own_session_as_pending_agent_session() { // diff review round 2, finding 1
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut l = Launch::new(Agent::ClaudeCode);
        l.args = vec!["--resume".into(), "X1".into()];
        let mut w = ws_with(&[]);
        join(&mut w, k, &l, &Start { agent_session: None, ..start() }, Some("X1"), &none).unwrap();
        let m = &w.members[k];
        assert_eq!(m.pending.as_ref().and_then(|p| p.agent_session.as_deref()), Some("X1"));
        assert!(m.launch.args.is_empty()); // AU30: the session argument is not saved
        let mut w = ws_with(&[]);
        join(&mut w, k, &Launch::new(Agent::ClaudeCode), &start(), None, &none).unwrap();
        assert_eq!(w.members[k].pending.as_ref().and_then(|p| p.agent_session.as_deref()), Some("U1")); // the pre-assign
    }

    #[test]
    fn join_keeps_the_saved_launch_and_kind_only_for_its_own_resume() { // diff review round 2, findings 2 and 6(b)
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut saved = rec(MemberState::Resumable, Some("abc"));
        saved.launch.profile = Some("p1".into());
        saved.launch.compact = Some("600k".into());
        saved.launch.args = vec!["--model".into(), "m".into()];
        saved.dropped_flags = vec!["--agents".into()];
        let mut hand = Launch::new(Agent::ClaudeCode);
        hand.profile = Some("p1".into());
        hand.compact = Some("600k".into());
        hand.args = vec!["--resume".into(), "abc".into()];
        let mut w = ws_with(&[(k, saved.clone())]);
        join(&mut w, k, &hand, &Start { agent_session: None, ..start() }, Some("abc"), &none).unwrap();
        let m = &w.members[k];
        assert_eq!((&m.launch, &m.dropped_flags), (&saved.launch, &saved.dropped_flags));
        assert_eq!((m.state, m.program.clone()), (MemberState::Running, Some("/b/claude".into())));
        // An `ended` record of another kind with that id is replaced, not kept.
        let mut oc = MemberRecord::new(Agent::OpenCode);
        oc.agent_session = Some("abc".into());
        oc.overrides = Some(serde_json::json!({"results": false}));
        let mut w = ws_with(&[(k, oc)]);
        join(&mut w, k, &hand, &Start { agent_session: None, ..start() }, Some("abc"), &none).unwrap();
        let m = &w.members[k];
        assert_eq!((m.launch.kind, m.agent_session.clone(), m.overrides.clone()), (Agent::ClaudeCode, None, None));
    }

    #[test] // FIX-24 D1, W§7.2: events_off comes from the arguments of the start that runs, before the AU30 filter
    fn record_start_sets_events_off_from_the_arguments_of_this_start() {
        use Agent::{ClaudeCode, OpenCode};
        let cases: [(Agent, &[&str], bool, &[&str]); 6] = [
            (OpenCode, &["attach", "http://h"], true, &[]),
            (OpenCode, &["run", "--attach", "http://h"], true, &[]),
            (OpenCode, &["--pure"], true, &["--pure"]),
            (OpenCode, &["--prompt", "attach"], false, &[]),
            (ClaudeCode, &["--bare"], true, &["--bare"]),
            (ClaudeCode, &["attach"], false, &[]),
        ];
        for (kind, a, want_off, want_args) in cases {
            let mut r = MemberRecord::new(kind);
            let mut l = Launch::new(kind);
            l.args = a.iter().map(|s| s.to_string()).collect();
            record_start(&mut r, &l, &start());
            assert_eq!((r.launch.events_off, r.launch.args.clone()), (want_off, want_args.iter().map(|s| s.to_string()).collect::<Vec<_>>()), "{kind:?} {a:?}");
            // The `ws run` start of the saved launch: the TUI runs for a saved `attach` or `run --attach` (P12).
            let saved = r.launch.clone();
            record_start(&mut r, &saved, &start());
            assert_eq!(r.launch.events_off, args::events_off(kind, &saved.args), "ws run start of {kind:?} {a:?}");
            let want_restart = matches!(a.first().copied(), Some("--pure") | Some("--bare"));
            assert_eq!(r.launch.events_off, want_restart, "ws run start of {kind:?} {a:?}");
        }
    }

    #[test] // FIX-24 diff review round 2, finding 4: both start writes take bare mode from the start's environment
    fn start_writes_take_bare_mode_from_the_environment() {
        let bare = Start { bare_env: true, ..start() };
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        record_start(&mut r, &Launch::new(Agent::ClaudeCode), &bare);
        assert!(r.launch.events_off, "record_start");
        assert!(r.launch.args.is_empty());
        record_start(&mut r, &Launch::new(Agent::ClaudeCode), &start());
        assert!(!r.launch.events_off, "computed again without it");
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut w = ws_with(&[(k, rec(MemberState::Resumable, Some("abc")))]);
        let mut hand = Launch::new(Agent::ClaudeCode);
        hand.args = vec!["--resume".into(), "abc".into()];
        join(&mut w, k, &hand, &Start { agent_session: None, ..bare }, Some("abc"), &none).unwrap();
        assert!(w.members[k].launch.events_off, "join, accepted resume");
    }

    #[test] // FIX-24 diff review round 2, finding 2: the D4 compare ignores events_off, so dropped_flags stays
    fn dropped_flags_survive_an_events_off_change() {
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut saved = rec(MemberState::Resumable, Some("abc"));
        saved.launch.args = vec!["--model".into(), "m".into()];
        saved.dropped_flags = vec!["--agents".into()];
        let mut w = ws_with(&[(k, saved)]);
        let mut hand = Launch::new(Agent::ClaudeCode);
        hand.args = vec!["--bare".into(), "--resume".into(), "abc".into()];
        join(&mut w, k, &hand, &Start { agent_session: None, ..start() }, Some("abc"), &none).unwrap();
        assert!(w.members[k].launch.events_off, "the hand resume typed --bare");
        assert_eq!(w.members[k].dropped_flags, ["--agents"], "after the hand resume");
        // The `ws run` start of the saved launch (no --bare in it).
        let r = w.members.get_mut(k).unwrap();
        let l = r.launch.clone();
        record_start(r, &l, &start());
        assert!(!r.launch.events_off, "the ws run start has no --bare");
        assert_eq!(r.dropped_flags, ["--agents"], "after the ws run start");
    }

    #[test] // FIX-24 D1, D5: a value that another zygo left is computed again at the start
    fn record_start_computes_events_off_again_over_a_stale_value() {
        let l: Launch = serde_json::from_value(serde_json::json!({"kind": "opencode", "events_off": true})).unwrap();
        let mut r = MemberRecord::new(Agent::OpenCode);
        record_start(&mut r, &l, &start());
        assert!(!r.launch.events_off);
        assert!(serde_json::to_value(&r.launch).unwrap().get("events_off").is_none());
    }

    #[test] // FIX-24 D1, AU94, W§8.4: the accepted resume keeps the saved launch; events_off follows the typed arguments
    fn join_sets_events_off_from_the_typed_arguments_of_an_accepted_resume() {
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        fn oc_saved(args: &[&str], off: bool) -> MemberRecord {
            let mut r = MemberRecord::new(Agent::OpenCode);
            r.state = MemberState::Resumable;
            r.agent_session = Some("ses_1".into());
            r.zygo_session = Some("01JZZZZZZZZZZZZZZZZZZZZZZZ".into());
            r.launch.args = args.iter().map(|s| s.to_string()).collect();
            r.launch.events_off = off;
            r
        }
        let mut hand = Launch::new(Agent::OpenCode);
        // Typed --pure at the resume of a member saved without it.
        let mut w = ws_with(&[(k, oc_saved(&["--model", "m"], false))]);
        hand.args = vec!["--pure".into(), "--session".into(), "ses_1".into()];
        join(&mut w, k, &hand, &Start { agent_session: None, ..start() }, Some("ses_1"), &none).unwrap();
        assert!(w.members[k].launch.events_off, "typed --pure");
        assert_eq!(w.members[k].launch.args, ["--model", "m"], "the saved launch stays");
        // The reverse: a member saved with --pure, resumed by hand without it.
        let mut w = ws_with(&[(k, oc_saved(&["--pure"], true))]);
        hand.args = vec!["--session".into(), "ses_1".into()];
        join(&mut w, k, &hand, &Start { agent_session: None, ..start() }, Some("ses_1"), &none).unwrap();
        assert!(!w.members[k].launch.events_off, "no typed --pure");
        assert_eq!(w.members[k].launch.args, ["--pure"], "the saved launch stays; a later ws run start computes true from it");
    }

    /// A saved launch with every value that changes a launch (AU45, AU61).
    fn saved(kind: Agent) -> MemberRecord {
        let mut r = MemberRecord::new(kind);
        r.launch.profile = Some("p 1".into());
        r.launch.server = Some("http://127.0.0.1:8080/x".into());
        r.launch.yolo = true;
        r.launch.compact = Some("600k".into());
        r.launch.args = vec!["--model".into(), "m".into()];
        r
    }

    #[test]
    fn the_resume_command_reproduces_the_ws_run_launch() { // diff review round 3, findings 1-4; AU61
        let mut r = saved(Agent::ClaudeCode);
        r.launch.passthrough = true;
        let flags = "--profile 'p 1' --passthrough --server 'http://127.0.0.1:8080/x'";
        let tail = "--yolo -- --resume='abc' '--model' 'm'";
        assert_eq!(resume_command(&r, "abc", None), format!("env -u CLAUDE_CONFIG_DIR zygo claude {flags} --compact '600k' {tail}"));
        // The effective compaction (AU32, `ws::effective_compact`): `cleared` gives no flag, as `ws run` gives none.
        r.compact = Some(CompactOverride::Cleared);
        assert_eq!(resume_command(&r, "abc", None), format!("env -u CLAUDE_CONFIG_DIR zygo claude {flags} {tail}"));
        r.compact = Some(CompactOverride::Set("800k".into()));
        assert_eq!(resume_command(&r, "abc", None), format!("env -u CLAUDE_CONFIG_DIR zygo claude {flags} --compact '800k' {tail}"));
        // The saved server as it is (W-C7: the state holds no user info, so there is nothing to mask).
        r.launch.server = Some("https://u:p@h/x?token=1".into());
        assert!(resume_command(&r, "abc", None).contains(" --server 'https://u:p@h/x?token=1' "), "{}", resume_command(&r, "abc", None));
        // OpenCode: the session after `run`, where the launcher puts it.
        let mut o = saved(Agent::OpenCode);
        o.launch.args = vec!["run".into(), "--model".into(), "m".into()];
        assert_eq!(
            resume_command(&o, "ses_1", None),
            "zygo opencode --profile 'p 1' --server 'http://127.0.0.1:8080/x' --compact '600k' --yolo -- 'run' --session='ses_1' '--model' 'm'"
        );
    }

    #[test]
    fn an_own_session_pending_id_commits_only_through_a_hook() { // diff review round 3, finding 6
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut w = ws_with(&[]);
        join(&mut w, k, &Launch::new(Agent::ClaudeCode), &Start { agent_session: None, ..start() }, Some("typo"), &none).unwrap();
        let typed = w.members[k].clone();
        let p = typed.pending.clone().unwrap();
        assert_eq!((p.agent_session.as_deref(), p.own_session), (Some("typo"), true));
        let mut w = ws_with(&[]);
        join(&mut w, k, &Launch::new(Agent::ClaudeCode), &start(), None, &none).unwrap();
        assert!(!w.members[k].pending.as_ref().unwrap().own_session); // a pre-assign
        for exit in [Exit::Ended, Exit::Resumable { failed_fast: false }] {
            let mut r = typed.clone();
            assert!(record_exit(&mut r, "S1", exit));
            assert_eq!((r.zygo_session.as_deref(), r.agent_session.as_deref(), r.pending.is_none()), (Some("S1"), None, true), "{exit:?}");
            assert!(exit_lines(&r, &out(1, None, false, 60), exit, None, r.agent_session.as_deref(), None).is_empty(), "{exit:?}");
        }
        // A kept record's committed id stays.
        let mut r = typed.clone();
        r.agent_session = Some("abc".into());
        assert!(record_exit(&mut r, "S1", Exit::Ended));
        assert_eq!(r.agent_session.as_deref(), Some("abc"));
        // A state file of an older zygo reads as `false`; `false` is not written.
        let old: Pending = serde_json::from_str(r#"{"zygo_session":"S","started_at":1}"#).unwrap();
        assert!(!old.own_session);
        assert!(!serde_json::to_string(&old).unwrap().contains("own_session"));
    }

    #[test]
    fn undo_start_restores_only_the_exact_written_record() { // diff review round 3, finding 12
        let k = "kaaaaaaaa";
        let none = |_: &MemberRecord| None;
        let mut old = rec(MemberState::Ended, Some("old"));
        old.overrides = Some(serde_json::json!({"results": false}));
        let started = |members: &[(&str, MemberRecord)]| {
            let mut w = ws_with(members);
            join(&mut w, k, &Launch::new(Agent::ClaudeCode), &start(), None, &none).unwrap();
            let written = w.members[k].clone();
            (w, written)
        };
        let (mut w, written) = started(&[(k, old.clone())]);
        assert!(undo_start(&mut w, k, &written, Some(old.clone())));
        assert_eq!(w.members[k], old);
        let (mut w, written) = started(&[]);
        assert!(undo_start(&mut w, k, &written, None));
        assert!(!w.members.contains_key(k));
        // Another write came between: only this start's `pending` goes, nothing is put back.
        let (mut w, written) = started(&[(k, old.clone())]);
        w.members.get_mut(k).unwrap().overrides = Some(serde_json::json!({"results": true}));
        let mut want = w.members[k].clone();
        want.pending = None;
        assert!(undo_start(&mut w, k, &written, Some(old.clone())));
        assert_eq!(w.members[k], want);
        // The record of another start: no change.
        let (mut w, written) = started(&[]);
        w.members.get_mut(k).unwrap().pending.as_mut().unwrap().zygo_session = "S2".into();
        let before = w.clone();
        assert!(!undo_start(&mut w, k, &written, Some(old.clone())));
        assert_eq!(w, before);
    }
}
