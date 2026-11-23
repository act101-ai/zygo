//! `zygo hook workspace --event <event>` and the member part of `session-start` (workspaces design W§9.1, W§9.2,
//! W§9.4, W-C1 (C6); AU56, AU58, AU60, AU83, AU84, AU86, AU92, AU104). Owner: node H1 (decisions D1-D20 of
//! `docs/work/phase2/nodes/H1.md`).
//!
//! Membership comes only from the session view marker in `ZYGO_SESSION_DIR`, never from `ZYGO_WORKSPACE` or
//! `ZYGO_PANE` (AU58, D2). The library reads no process environment: the session folder variable comes through `env`,
//! the state folder through `paths` and the zygo program through `zygo`. A hook never breaks the agent (C6): a failure
//! writes one line, to the session log for the session folder's files and to `errors.log` for the state folder's,
//! and the hook exits 0 (AU56, D14). The `session` event never changes `activity.json`: an open turn stays busy until
//! its stop, the agent exit, the drain timeout or `down --now` (D5). No wait here is longer than the 1 s lock bounds
//! of AU56 and W§7.3, and nothing here ends or removes a session, pane, member, turn or file (D19).

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail};
use serde_json::{json, Value};
use zygo_core::esc;
use zygo_core::hook::{slash_dispatch, HookOutput, SlashDispatch};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{ResumeNote, SessionView};
use zygo_core::Agent;
use zygo_workspace::activity::{self, Activity};
use zygo_workspace::identity::{valid_pane_key, valid_ws_id};
use zygo_workspace::snapshot;
use zygo_workspace::state::{MemberState, Phase, StateError, Store};

/// The `--event` values of `zygo hook workspace`.
pub const EVENTS: [&str; 8] =
    ["session", "prompt", "stop", "pre-tool", "tool-end", "session-end", "subagent-start", "subagent-stop"];

/// The deny reason of an `Agent` call while `down` drains (W§9.2).
pub const DRAINING: &str = "zygo: workspace is draining for down; finish your turn";
/// The deny reason of an `Agent` call and the block reason of a prompt while `down` stops the workspace (W§9.2).
pub const STOPPING: &str = "zygo: workspace is stopping";

/// One hook call of a member: the CLI event name, the session folder, its view and the marker values that passed
/// their checks (AU84).
struct Ctx<'a> {
    event: &'a str,
    dir: &'a SessionDir,
    view: SessionView,
    ws_id: String,
    pane: String,
    store: Store,
    paths: &'a Paths,
}

/// The session folder of `ZYGO_SESSION_DIR` (D2): an absolute path whose last part is a folder and not a link. A
/// link above the last part is followed, as for the state folder (FIX-17).
pub fn check_folder(dir: &SessionDir) -> anyhow::Result<()> {
    let p = &dir.root;
    if !p.is_absolute() {
        bail!("ZYGO_SESSION_DIR is not an absolute path: {}", esc(&p.to_string_lossy()));
    }
    match fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => bail!("{}: session folder is a link", p.display()),
        Ok(m) if !m.is_dir() => bail!("{}: session folder is not a folder", p.display()),
        Ok(_) => Ok(()),
        Err(e) => bail!("{}: {e}", p.display()),
    }
}

/// `true` when the failed open of `path` met a link: `ELOOP` from `O_NOFOLLOW`, or a link seen after the failure.
fn open_met_link(e: &std::io::Error, path: &Path) -> bool {
    #[cfg(unix)]
    if e.raw_os_error() == Some(libc::ELOOP) {
        return true;
    }
    #[cfg(not(unix))]
    let _ = e;
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// The member's session view (D2): no `view.json` gives `None`. On unix the file is opened with
/// `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, so a link is refused and a FIFO does not make the hook wait; the opened
/// handle must be a regular file. The whole file is read; there is no size cap.
pub fn read_view(dir: &SessionDir) -> anyhow::Result<Option<SessionView>> {
    let path = dir.view_json();
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let mut f = match o.open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if open_met_link(&e, &path) => bail!("{}: is a link; not read", path.display()),
        Err(e) => bail!("{}: {e}", path.display()),
    };
    match f.metadata() {
        Ok(m) if m.is_file() => {}
        Ok(_) => bail!("{}: is not a regular file; not read", path.display()),
        Err(e) => bail!("{}: {e}", path.display()),
    }
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map(Some).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// One session-log line `workspace hook <event>: <text>` (D14).
fn slog(dir: &SessionDir, event: &str, text: &str) {
    dir.append_log(&format!("workspace hook {event}: {text}"));
}

/// One `errors.log` line of the member's workspace, in the form of `slog` (D14).
fn elog(c: &Ctx, text: &str) {
    c.store.log_error(&format!("workspace hook {}: {text}", c.event));
}

/// The events of `zygo hook workspace` that write the line of a view that cannot be read, of a marker value that
/// fails its check (AU84, D6) or of a persistent `activity.json` condition (AU56; round 3, S2 and S4; see `change`):
/// a few calls per turn, so the `*` tool matchers do not write a line per tool call.
const MARKER_LOG_EVENTS: [&str; 3] = ["prompt", "session", "session-end"];

/// The member context of `view` (D2, D6): `None` for a view with no marker. With `log`, a workspace id that fails its
/// check writes one session-log line, since it names no state folder (AU84), and a pane key that fails its check
/// writes one `errors.log` line in that workspace's state folder; without `log` they write nothing. Neither changes
/// anything else.
fn ctx_of<'a>(
    event: &'a str, dir: &'a SessionDir, view: SessionView, paths: &'a Paths, log: bool,
) -> Option<Ctx<'a>> {
    let m = view.workspace.as_ref()?.member.clone();
    if !valid_ws_id(&m.ws_id) {
        if log {
            slog(dir, event, &format!("invalid workspace id \"{}\" in the session view", esc(&m.ws_id)));
        }
        return None;
    }
    let store = Store::open(paths, &m.ws_id);
    if !valid_pane_key(&m.pane) {
        if log {
            store.log_error(&format!(
                "workspace hook {event}: invalid pane key \"{}\" in the session view",
                esc(&m.pane)
            ));
        }
        return None;
    }
    Some(Ctx { event, dir, view, ws_id: m.ws_id, pane: m.pane, store, paths })
}

/// The member context of the session folder `dir` (D2, D6): a refused folder is an error (one stderr line through
/// `guarded`); a `view.json` that cannot be read gives `None` and, for the events of `MARKER_LOG_EVENTS` only, writes
/// one session-log line (the same flag as the marker checks of `ctx_of`).
fn member<'a>(event: &'a str, dir: &'a SessionDir, paths: &'a Paths) -> anyhow::Result<Option<Ctx<'a>>> {
    check_folder(dir)?;
    let log = MARKER_LOG_EVENTS.contains(&event);
    let view = match read_view(dir) {
        Ok(Some(v)) => v,
        Ok(None) => return Ok(None),
        Err(e) => {
            if log {
                slog(dir, event, &format!("{e:#}"));
            }
            return Ok(None);
        }
    };
    Ok(ctx_of(event, dir, view, paths, log))
}

/// One change of `activity.json` under `activity.lock` with the hook bound (AU56, D14). An `activity.json` that is a
/// link or not a regular file is not changed. Each failure writes one session-log line by its class (round 3, S2 and
/// S4). A persistent condition of the file repeats on every event, so its line is written only for the events of
/// `MARKER_LOG_EVENTS`, a line per turn and not per tool call: the file is a link, is not a regular file, or the
/// update gives `activity::Unreadable` (the file cannot be read, does not parse or is over the read limit). A one-off
/// failure writes its line on every event where it happens: a missed lock (the AU56 bound), a file of a newer zygo,
/// a refused over-limit result, and each other update error.
fn change(c: &Ctx, f: &mut dyn FnMut(&mut Activity)) {
    let path = c.dir.activity_json();
    let persistent = |text: String| {
        if MARKER_LOG_EVENTS.contains(&c.event) {
            slog(c.dir, c.event, &text);
        }
    };
    match fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_symlink() => return persistent(format!("{}: is a link; not changed", path.display())),
        Ok(m) if !m.is_file() => {
            return persistent(format!("{}: is not a regular file; not changed", path.display()));
        }
        _ => {}
    }
    match activity::update(c.dir, activity::LOCK_BOUND, f) {
        Ok(()) => {}
        Err(e) if e.is::<activity::Unreadable>() => persistent(format!("{e:#}")),
        Err(e) => slog(c.dir, c.event, &format!("{e:#}")),
    }
}

/// The main loop: the input object has no `agent_id` key (W§9.1, the test of W§8.3; D5).
fn main_loop(p: &Value) -> bool {
    !p.as_object().is_some_and(|o| o.contains_key("agent_id"))
}

/// `zygo hook workspace --event <event>` (W§9.1, D4, D5). It checks the event, then reads `ZYGO_SESSION_DIR`
/// through `env` (none or empty: nothing), then parses the input; a prompt line that `zygo hook prompt` consumes ends
/// here (D4, R1); then the session folder, the view and the marker. `pre-tool` can give the deny JSON of W§9.2 and a
/// Claude Code `prompt` the block JSON of AU86; every other call gives `None`. The dispatcher is the one that the
/// `zygo` binary registers (`slash_dispatch`).
pub fn run(
    event: &str, stdin: &[u8], paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, zygo: &Path,
) -> anyhow::Result<Option<String>> {
    run_with(event, stdin, paths, env, zygo, slash_dispatch())
}

/// `run` with the `/zygo:` dispatcher `dispatch`.
pub fn run_with(
    event: &str, stdin: &[u8], paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, zygo: &Path,
    dispatch: Option<SlashDispatch>,
) -> anyhow::Result<Option<String>> {
    if !EVENTS.contains(&event) {
        bail!("workspace: unknown event \"{}\"", esc(event));
    }
    let Some(sd) = env("ZYGO_SESSION_DIR").filter(|v| !v.is_empty()) else { return Ok(None) };
    let payload: Value = serde_json::from_slice(stdin)?;
    if !payload.is_object() {
        bail!("hook input is not a JSON object");
    }
    // D4, R1: a line that `zygo hook prompt` consumes runs there as a verb and never reaches the model, so it sets
    // no `busy` and reads no view. The rule is the dispatcher's own (`prompt::consumes`): a `/zygo:` line that the
    // dispatcher passes through starts a turn like any other line.
    if event == "prompt"
        && payload.get("prompt").and_then(Value::as_str).is_some_and(|p| crate::prompt::consumes(p, dispatch))
    {
        return Ok(None);
    }
    let dir = SessionDir::new(PathBuf::from(sd));
    let Some(c) = member(event, &dir, paths)? else { return Ok(None) };
    let main = main_loop(&payload);
    match event {
        "pre-tool" => {
            if payload.get("tool_name").and_then(Value::as_str) == Some("Agent")
                && let Some(reason) = deny_reason(&c)
            {
                return Ok(Some(deny_json(reason)));
            }
            if main {
                change(&c, &mut |a| a.tools_running = a.tools_running.saturating_add(1));
            }
        }
        "tool-end" if main => change(&c, &mut |a| a.tools_running = a.tools_running.saturating_sub(1)),
        "prompt" => {
            // AU86, D4: a Claude Code prompt while `down` holds `op.lock` in `stopping` is blocked and changes
            // nothing; an OpenCode prompt is never blocked, and `draining` blocks no prompt.
            if matches!(c.view.agent, Agent::ClaudeCode)
                && c.store.phase_unlocked() == Some(Phase::Stopping)
                && !c.store.op_lock_free()
            {
                return Ok(Some(json!({"decision": "block", "reason": STOPPING}).to_string()));
            }
            if main {
                change(&c, &mut |a| {
                    a.busy = true;
                    a.tools_running = 0;
                });
            }
        }
        "stop" if main => change(&c, &mut |a| {
            a.busy = false;
            a.tools_running = 0;
        }),
        // W§9.1 names no `busy` change for `SessionEnd`.
        "session-end" if main => change(&c, &mut |a| a.tools_running = 0),
        // D5, D8-D10: `session` changes nothing in `activity.json`; it commits and records (one write), then applies
        // the note rule whatever the record part gave (D17).
        "session" if main => {
            let id = payload_id(&payload, "sessionID");
            record_session(&c, id, zygo);
            if let Some(id) = id {
                bind_note(&c, id);
            }
        }
        "subagent-start" => {
            if let Some(id) = payload_id(&payload, "agent_id") {
                let ty = payload.get("agent_type").and_then(Value::as_str).unwrap_or("");
                change(&c, &mut |a| a.subagent_start(id, ty));
            }
        }
        "subagent-stop" => {
            if let Some(id) = payload_id(&payload, "agent_id") {
                change(&c, &mut |a| {
                    a.subagent_stop(id);
                });
            }
        }
        _ => {}
    }
    Ok(None)
}

/// `drain_block_dispatch` of the user config (W§9.2, W-C5); a config that does not load counts as `true` (D16).
fn block_dispatch(paths: &Paths) -> bool {
    zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None })
        .map_or(true, |m| m.config.workspace.drain_block_dispatch)
}

/// The deny reason of an `Agent` call (W§9.2, AU83, D16): the phase is read first, with no lock; `op.lock` is
/// opened only in phase `draining` with blocking on and in phase `stopping`, and a free lock gives no deny.
fn deny_reason(c: &Ctx) -> Option<&'static str> {
    match c.store.phase_unlocked() {
        Some(Phase::Draining) if block_dispatch(c.paths) && !c.store.op_lock_free() => Some(DRAINING),
        Some(Phase::Stopping) if !c.store.op_lock_free() => Some(STOPPING),
        _ => None,
    }
}

fn deny_json(reason: &str) -> String {
    json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}})
        .to_string()
}

/// The id at `key` of a payload (an agent session id, C8, D9, or a subagent id, D7): a non-empty string; a missing
/// key, another type or `""` is no id.
fn payload_id<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The closure of `Store::update` found nothing to change: no write, no log line, no pending check (D8).
#[derive(Debug)]
struct Unchanged;

impl std::fmt::Display for Unchanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unchanged")
    }
}

impl std::error::Error for Unchanged {}

/// A state error of the member record: a state folder that is a link or not a folder gets its line in the session log,
/// since `errors.log` lives in that folder (D14); every other error gets one `errors.log` line.
fn log_state(c: &Ctx, e: &StateError) {
    match e {
        StateError::NotADirectory { .. } => slog(c.dir, c.event, &e.to_string()),
        _ => elog(c, &e.to_string()),
    }
}

/// The member record at a start, in one write (AU60, AU92, AU104, C8, W§7.4; D8, D9). No state writes nothing and
/// makes no folder. Commit: a `pending` that names this zygo session moves into the record (its own agent session id
/// when it has one), `note_pending` is cleared, and the values that `down` saved are cleared unless they were saved
/// for this zygo session (`saved_for`). A record of another zygo session is not changed. Record: a payload id that
/// differs from the stored one is written; no payload replaces a stored id with none. A record equal to the one read
/// is not written. After a write the `snapshot.pending` check runs, and a changed id starts the same snapshot child
/// (W§7.3, D15).
fn record_session(c: &Ctx, id: Option<&str>, zygo: &Path) {
    match c.store.read() {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(e) => return log_state(c, &e),
    }
    let session = c.view.session.clone();
    let mut id_changed = false;
    let result = c.store.update(snapshot::HOOK_WAIT, &mut |w| {
        let Some(m) = w.members.get_mut(&c.pane) else {
            bail!("no member record for pane {}; not changed", c.pane);
        };
        let read = m.clone();
        let before = m.agent_session.clone();
        match m.pending.clone() {
            Some(p) if p.zygo_session == session => {
                m.zygo_session = Some(p.zygo_session);
                if let Some(a) = p.agent_session {
                    m.agent_session = Some(a);
                }
                m.state = MemberState::Running;
                m.pending = None;
                m.note_pending = false;
                // AU60: values saved for this zygo session (a `down --now` in its first turn) are kept.
                if m.saved_for.as_deref() != Some(session.as_str()) {
                    m.closed_units.clear();
                    m.interrupted.clear();
                    m.clean_stop = false;
                    m.saved_for = None;
                }
            }
            _ if m.zygo_session.as_deref() != Some(session.as_str()) => {
                bail!("member record of pane {} belongs to another session; not changed", c.pane);
            }
            _ => {}
        }
        if let Some(id) = id.filter(|id| m.agent_session.as_deref() != Some(*id)) {
            m.agent_session = Some(id.to_string());
        }
        if *m == read {
            return Err(Unchanged.into());
        }
        id_changed = m.agent_session != before;
        Ok(())
    });
    match result {
        Ok(_) => {}
        Err(StateError::Refused(e)) if e.is::<Unchanged>() => return,
        Err(e) => return log_state(c, &e),
    }
    let want = id_changed.then_some(false);
    let taken = snapshot::take_pending(c.paths, &c.ws_id);
    if want.is_some() || taken.is_some() {
        start_snapshot(c, zygo, taken == Some(true));
    }
}

/// Starts `<zygo> ws snapshot --ws-id <id> [--force]` as a detached child and does not wait for it (W§7.3, D15):
/// stdin, stdout and stderr to `/dev/null`, on unix in `/` and in a new session (`setsid`). A child that cannot start
/// queues the flag again and writes one `errors.log` line.
// The hook does not wait for the child: it ends soon after, and `init` reaps the child (W§7.3: the hook does not wait
// for the 2 s check of AU89).
#[allow(clippy::zombie_processes)]
fn start_snapshot(c: &Ctx, zygo: &Path, force: bool) {
    let mut cmd = Command::new(zygo);
    cmd.args(["ws", "snapshot", "--ws-id", &c.ws_id]);
    if force {
        cmd.arg("--force");
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.current_dir("/");
        // SAFETY: the closure runs in the child between fork and exec and calls only `setsid`, which is
        // async-signal-safe; it allocates nothing.
        unsafe {
            cmd.pre_exec(|| if libc::setsid() == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
        }
    }
    if let Err(e) = cmd.spawn() {
        match snapshot::queue_pending(c.paths, &c.ws_id, force) {
            Ok(()) => elog(c, &format!("cannot start the snapshot: {e}; snapshot.pending set again")),
            Err(q) => elog(c, &format!("cannot start the snapshot: {e}; snapshot.pending not set: {q:#}")),
        }
    }
}

/// Runs the body of `zygo ws snapshot` (M1, C10; D15). An `Err` or a panic of `body` queues `snapshot.pending`
/// again with `force`, so the next trigger takes the snapshot, and gives `Err` with the body's text (a panic:
/// `the snapshot panicked: <message>`), followed by `; snapshot.pending not set: <error>` when the queue fails.
pub fn snapshot_child(
    paths: &Paths, ws_id: &str, force: bool, body: &mut dyn FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let text = match panic::catch_unwind(AssertUnwindSafe(&mut *body)) {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(e)) => format!("{e:#}"),
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".to_string());
            format!("the snapshot panicked: {msg}")
        }
    };
    match snapshot::queue_pending(paths, ws_id, force) {
        Ok(()) => Err(anyhow!("{text}")),
        Err(q) => Err(anyhow!("{text}; snapshot.pending not set: {q:#}")),
    }
}

/// The resume note of W§9.4 (AU42, AU72; D11): with `clean_stop = false` only the crash sentence; else the
/// interrupted line and the units line, each when its list is not empty, joined by `\n`; `None` when there is nothing
/// to say. Each type, id and unit is escaped; an empty type prints the id alone.
pub fn note(n: &ResumeNote) -> Option<String> {
    if !n.clean_stop {
        return Some("zygo did not stop this session cleanly; the list of closed units was not saved.".to_string());
    }
    let mut lines = Vec::new();
    if !n.interrupted.is_empty() {
        let items: Vec<String> = n
            .interrupted
            .iter()
            .map(|s| {
                if s.agent_type.is_empty() {
                    esc(&s.agent_id)
                } else {
                    format!("{} {}", esc(&s.agent_type), esc(&s.agent_id))
                }
            })
            .collect();
        lines.push(format!(
            "zygo stopped this session while subagents {} were running; their results were lost; dispatch them again if still needed.",
            items.join(", ")
        ));
    }
    if !n.closed_units.is_empty() {
        let units: Vec<String> = n.closed_units.iter().map(|u| esc(u)).collect();
        lines.push(format!(
            "Units {} were closed when zygo stopped this session; call work_begin to open them again if work continues.",
            units.join(", ")
        ));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Removes the resume note inputs from the session view (W§9.4, AU60; D11); a failure writes one session-log line.
fn clear_note(c: &Ctx) {
    let cleared = || -> anyhow::Result<()> {
        let Some(mut v) = read_view(c.dir)? else { return Ok(()) };
        match v.workspace.as_mut() {
            Some(w) if w.resume_note.is_some() => w.resume_note = None,
            _ => return Ok(()),
        }
        v.save(c.dir)
    };
    if let Err(e) = cleared() {
        slog(c.dir, c.event, &format!("{e:#}"));
    }
}

/// The note rule of the OpenCode `session` call with agent session `id` (W§9.4, AU104; D10): the resume note inputs
/// belong to one agent session. Inputs with no `agent_session` get `id` and stay; inputs bound to `id` stay; inputs
/// bound to another id are removed from the session view. No marker or no inputs: nothing. A read or save error
/// writes one session-log line. It runs whatever the record part gave (D17).
fn bind_note(c: &Ctx, id: &str) {
    let applied = || -> anyhow::Result<()> {
        let Some(mut v) = read_view(c.dir)? else { return Ok(()) };
        let Some(w) = v.workspace.as_mut() else { return Ok(()) };
        let Some(n) = w.resume_note.as_mut() else { return Ok(()) };
        let owner = n.agent_session.clone();
        match owner.as_deref() {
            None => n.agent_session = Some(id.to_string()),
            Some(o) if o == id => return Ok(()),
            Some(_) => w.resume_note = None,
        }
        v.save(c.dir)
    };
    if let Err(e) = applied() {
        slog(c.dir, c.event, &format!("{e:#}"));
    }
}

/// The member part of `SessionStart` (W§9.1, W§9.4, AU60, AU92, AU104; D11) for the main loop of a member view;
/// `text` is the session text. The output is `text`, an empty line, then the note (the note alone when `text` is
/// empty). Claude Code: the whole hook output is written to `out` and flushed first (a write error returns `Err` with
/// nothing written to any file), then the member record is written (D8, D9), then the note inputs are removed from the
/// view when it held them; the result is `None`. OpenCode adds the session text to every model request, so the result
/// is `Some(output)` and nothing is written: the plugin's `session` call owns the record and the note rule (D10).
pub fn session_start(
    payload: &Value, dir: &SessionDir, view: &SessionView, paths: &Paths, zygo: &Path, out: &mut dyn Write, text: &str,
) -> anyhow::Result<Option<String>> {
    // The OpenCode call runs per model request: a marker that fails its check writes no line there (D6).
    let log = matches!(view.agent, Agent::ClaudeCode);
    let Some(c) = ctx_of("session-start", dir, view.clone(), paths, log) else { return Ok(Some(text.to_string())) };
    let inputs = view.workspace.as_ref().and_then(|w| w.resume_note.as_ref());
    let output = match inputs.and_then(note) {
        Some(n) if text.is_empty() => n,
        Some(n) => format!("{text}\n\n{n}"),
        None => text.to_string(),
    };
    match view.agent {
        Agent::ClaudeCode => {
            let rendered = HookOutput::Json(
                json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": output}}),
            )
            .to_stdout();
            out.write_all(rendered.as_bytes())?;
            out.flush()?;
            record_session(&c, payload_id(payload, "session_id"), zygo);
            if inputs.is_some() {
                clear_note(&c);
            }
            Ok(None)
        }
        Agent::OpenCode => Ok(Some(output)),
    }
}
