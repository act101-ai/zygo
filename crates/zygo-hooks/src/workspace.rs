//! `zygo hook workspace --event <event>` and the member part of `session-start` (workspaces design W§9.1, W§9.2,
//! W§9.4, W-C1 (C6); AU56, AU58, AU60, AU83, AU84, AU86, AU92, AU104). Owner: node H1 (decisions D1-D20 of
//! `docs/work/phase2/nodes/H1.md`).
//!
//! Membership comes only from the session view marker in `ZYGO_SESSION_DIR`, never from `ZYGO_WORKSPACE` or
//! `ZYGO_PANE` (AU58, D2); `zygo hook prompt` reads `ZYGO_WORKSPACE` only as a gate before `mark_busy` looks for the
//! marker, as the OpenCode plugin does. The library reads no process environment: the session folder variable comes through `env`,
//! the state folder through `paths` and the zygo program through `zygo`. A hook never breaks the agent (C6): a failure
//! writes one line, to the session log for the session folder's files and to `errors.log` for the state folder's,
//! and the hook exits 0 (AU56, D14). The `session` event never changes `activity.json`: an open turn stays busy until
//! its stop, the agent exit, the drain timeout or `down --now` (D5). `session` (main loop), `prompt`, `stop` and
//! `tool-end` start the rate-limited mail sync child (mail sync design S§8.1, S§8.4, S-C1). No wait here is longer than the 1 s lock bounds
//! of AU56 and W§7.3, and nothing here ends or removes a session, pane, member, turn or file (D19).

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, bail};
use serde_json::{json, Value};
use zygo_core::esc;
use zygo_core::hook::{error_line, slash_dispatch, HookOutput, SlashDispatch};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{ResumeNote, SessionView};
use zygo_core::Agent;
use zygo_mail_sync::env::{Env, NoSeam, SystemClock, TmuxWake};
use zygo_mail_sync::public::LsRemote;
use zygo_mail_sync::spawn::ChildSpawner;
use zygo_workspace::activity::{self, Activity};
use zygo_workspace::identity::{valid_pane_key, valid_ws_id};
use std::time::SystemTime;
use zygo_config::MailConfig;
use zygo_workspace::mail::wake::{self as mail_wake, Outcome, Probe, RealProbe};
use zygo_workspace::mail::{mail_dir, store};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::state::Workspace;
use zygo_workspace::snapshot;
use zygo_workspace::state::{MemberRecord, MemberState, Phase, StateError, Store};

/// The `--event` values of `zygo hook workspace`.
pub const EVENTS: [&str; 9] =
    ["session", "prompt", "stop", "pre-tool", "tool-end", "session-end", "subagent-start", "subagent-stop", "message"];

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

/// The member's session view (D2): no `view.json` gives `None`. The file is read by the rule of `read_session_file`.
pub fn read_view(dir: &SessionDir) -> anyhow::Result<Option<SessionView>> {
    let path = dir.view_json();
    let Some(bytes) = read_session_file(&path)? else { return Ok(None) };
    serde_json::from_slice(&bytes).map(Some).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// The bytes of a file of the session folder (D2; FIX-22): a missing file gives `None`. On unix the file is opened
/// with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, so a link is refused and a FIFO does not make the reader wait; the
/// opened handle must be a regular file. The whole file is read; there is no size cap. Its callers: `read_view`
/// (`view.json`), and in `zygo::cmd::verbs` the setting verbs' `set_session` (its one read of the old `overrides.json`,
/// base spec §17.1) and `mirror` (its read of `overrides.json` for the member record, AU105).
pub fn read_session_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let mut f = match o.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if open_met_link(&e, path) => bail!("{}: is a link; not read", path.display()),
        Err(e) => bail!("{}: {e}", path.display()),
    };
    match f.metadata() {
        Ok(m) if m.is_file() => {}
        Ok(_) => bail!("{}: is not a regular file; not read", path.display()),
        Err(e) => bail!("{}: {e}", path.display()),
    }
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    Ok(Some(bytes))
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
/// link or not a regular file is not changed: `activity::update` refuses it with a `FileCondition` before it takes
/// or waits for the lock (FIX-22). Each failure writes one session-log line by its class (round 3, S2 and S4; round
/// 4, T3). A persistent condition repeats on every event, so its line is written only for the events of
/// `MARKER_LOG_EVENTS`, a line per turn and not per tool call: the update gives `activity::FileCondition` (the file
/// is a link or is not a regular file, cannot be read, is over the read limit, does not parse or is of a newer
/// version; the lock file cannot be opened; the write failed). A one-off failure writes its line on every event where
/// it happens: the lock not taken within the AU56 bound, and a refused over-limit result.
fn change(c: &Ctx, f: &mut dyn FnMut(&mut Activity)) {
    if let Err(e) = change_within(c, activity::LOCK_BOUND, f) {
        slog(c.dir, c.event, &format!("{e:#}"));
    }
}

/// `change` with the lock wait `bound`: it writes the line of a persistent condition by the rule of `change` and
/// returns the one-off failure (the lock not taken within `bound`, a refused over-limit result) to the caller.
fn change_within(c: &Ctx, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()> {
    let persistent = |text: String| {
        if MARKER_LOG_EVENTS.contains(&c.event) {
            slog(c.dir, c.event, &text);
        }
    };
    match activity::update(c.dir, bound, f) {
        Err(e) if e.is::<activity::FileCondition>() => {
            persistent(format!("{e:#}"));
            Ok(())
        }
        r => r,
    }
}

/// The change of a main-loop `prompt` (W§9.1, AU43): a turn starts, `busy = true`, `tools_running = 0`.
fn start_turn(a: &mut Activity) {
    a.busy = true;
    a.tools_running = 0;
}

/// The busy mark of a member whose `/zygo:` verb panicked (W§9.1 `UserPromptSubmit` row; H1 attempt-2 review
/// round 4, T2). `zygo hook prompt` then lets the line reach the model (C6), while `workspace --event prompt` skipped
/// that line as consumed (`prompt::consumes`), so the prompt hook calls this when `ZYGO_WORKSPACE` is set. It is the
/// `prompt` change of `run_with` (`start_turn`) through `member` and `change_within`; membership still comes only from
/// the view marker (AU58), and the AU86 block does not apply, since the line has already passed. It waits for
/// `activity.lock` at most `bound`, which the prompt hook gives as the smaller of `activity::LOCK_BOUND` and the time
/// left of its verb wait, so the hook keeps its 5 s bound (§17.1; round 4 follow-up F1); a zero `bound` tries the lock
/// once and does not wait. A persistent condition of the files writes the session-log line of the `prompt` event; a
/// one-off failure (the lock not taken within `bound`) is returned, so it goes into the one stderr line of C6. No or an
/// empty `ZYGO_SESSION_DIR` in `env`, no `view.json` or no marker: nothing. A refused session folder is an error (D2).
pub fn mark_busy(paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, bound: Duration) -> anyhow::Result<()> {
    let Some(sd) = env("ZYGO_SESSION_DIR").filter(|v| !v.is_empty()) else { return Ok(()) };
    let dir = SessionDir::new(PathBuf::from(sd));
    match member("prompt", &dir, paths)? {
        Some(c) => change_within(&c, bound, &mut start_turn),
        None => Ok(()),
    }
}

/// The number of unread messages of the member (M§7, ML-2 `store::unread`), counted from the names in `new/` with no
/// message opened. A read error writes one stderr line and counts as 0, so no hook decision changes (M-C1, M§10, C6;
/// ML-10 D4). A member with `[features] mail` off (`view.mail`) has no mail tools: 0, and `mail/` is not read (M§7.1,
/// M§7.2, M§8; ML-10 review R1).
fn unread_mail(c: &Ctx) -> usize {
    unread_ids(c).len()
}

/// The ids of the unread messages of the member, oldest first, by the rules of `unread_mail` (an error or mail off:
/// none).
fn unread_ids(c: &Ctx) -> Vec<String> {
    if !c.view.mail {
        return Vec::new();
    }
    match store::unread_ids(&mail_dir(c.paths, &c.ws_id), &c.pane) {
        Ok(ids) => ids,
        Err(e) => {
            error_line(&format!("workspace hook {}: {e}", c.event));
            Vec::new()
        }
    }
}

/// The file of the session folder that holds the ids of the unread messages that this launch announced to the
/// OpenCode member: by the `message` line or by a turn-end wake (M§7.2, M§7.3; FX-33 review F1). A JSON array of ids.
const MAIL_SEEN: &str = "mail-seen.json";

/// The announced ids of `MAIL_SEEN`; a missing file, or one that cannot be read or parsed, holds none (the next turn
/// end then wakes at most once more, C6).
fn mail_seen(c: &Ctx) -> Vec<String> {
    match read_session_file(&c.dir.root.join(MAIL_SEEN)) {
        Ok(Some(b)) => serde_json::from_slice(&b).unwrap_or_default(),
        Ok(None) => Vec::new(),
        Err(e) => {
            slog(c.dir, c.event, &format!("{e:#}"));
            Vec::new()
        }
    }
}

/// Writes `ids` as the announced ids (temporary file, then rename; `0600`); a failure writes one session-log line.
fn set_mail_seen(c: &Ctx, ids: &[String]) {
    let written = serde_json::to_vec(ids)
        .map_err(anyhow::Error::from)
        .and_then(|b| zygo_core::session_dir::write_atomic(&c.dir.root.join(MAIL_SEEN), &b));
    if let Err(e) = written {
        slog(c.dir, c.event, &format!("{MAIL_SEEN} not written: {e:#}"));
    }
}

/// True when `id` is the OpenCode main session that the plugin's `session` call recorded for the member
/// (`member.agent_session`, AU104). A child session is never recorded (AU27), so a child's user message gets no line
/// whatever the order in which the plugin saw its events (FX-33 review). No `workspace.json` or no record: false.
fn main_session(c: &Ctx, id: Option<&str>) -> bool {
    let Some(id) = id else { return false };
    matches!(c.store.read(), Ok(Some(ws)) if ws.members.get(&c.pane).and_then(|m| m.agent_session.as_deref()) == Some(id))
}

/// M§7.3 at the end of an OpenCode turn (the plugin's `stop` call, AU27; FX-33 review F1): a member that was busy when
/// mail came was not woken (M§7.3) and OpenCode has no Stop block (ML-10 D1), so when `new/` holds an id that this
/// launch has not announced, the hook wakes the member's own pane by the M§7.3 steps (idle rule, `wake_gap`, pane
/// checks) and, when the wake typed its text, records the ids as announced, so one arrival gives at most one wake. Not in phase `draining` or
/// `stopping` (M§9). The wake text is fixed (M-C5); its prompt then gets the `message` line.
fn wake_after_turn(c: &Ctx, sync: &Env, turn_wake: &dyn TurnWake) {
    if matches!(c.store.phase_unlocked(), Some(Phase::Draining | Phase::Stopping)) {
        return;
    }
    let ids = unread_ids(c);
    let seen = mail_seen(c);
    if !ids.iter().any(|i| !seen.contains(i)) {
        return;
    }
    let Ok(Some(ws)) = c.store.read() else { return };
    let cfg = zygo_config::load(&zygo_config::LoadContext { paths: c.paths.clone(), repo: None, team: None })
        .map(|m| m.config.mail)
        .unwrap_or_default();
    // Only a sent wake announces: after `Gap`, `Busy`, `NoPane`, `Mismatch`, `NotLive` or `Failed` no prompt was typed,
    // so the ids stay unannounced and a later turn end tries again (FX-33 review).
    if turn_wake.wake(c.paths, &ws, &c.pane, &cfg, sync.clock.now()) == Outcome::Woken {
        set_mail_seen(c, &ids);
    }
}

/// The turn-end wake of one member (M§7.3; FX-33 review): its [`Outcome`] says whether the text was typed. Tests
/// replace it.
pub trait TurnWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, key: &str, cfg: &MailConfig, now: SystemTime) -> Outcome;
}

/// The production turn-end wake: [`zygo_workspace::mail::wake::wake`] through the tmux program `tmux`, with the AU26
/// idle rule of [`RealProbe`] and the clock `now`. A `wake_gap` that does not parse is 30 s, with one `errors.log`
/// line, as the wake after a send (`after_send_with`, A7).
pub struct TmuxTurnWake {
    pub tmux: PathBuf,
}

/// [`RealProbe`]'s idle rule with a fixed clock.
struct ProbeAt(SystemTime);

impl Probe for ProbeAt {
    fn idle(&self, session_dir: &Path) -> bool { RealProbe.idle(session_dir) }
    fn now(&self) -> SystemTime { self.0 }
}

impl TurnWake for TmuxTurnWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, key: &str, cfg: &MailConfig, now: SystemTime) -> Outcome {
        let store = Store::open(paths, &ws.id);
        let gap = zygo_config::parse_duration(&cfg.wake_gap).unwrap_or_else(|| {
            store.log_error(&format!(
                "mail wake: [mail] wake_gap \"{}\" is not <n>s, <n>m, <n>h or <n>d; using 30s",
                esc(&cfg.wake_gap)
            ));
            Duration::from_secs(30)
        });
        let mux = Tmux::new(self.tmux.clone(), ws.tmux_socket.clone());
        mail_wake::wake(&store, ws, &mux, key, gap, &ProbeAt(now), mail_wake::HOOK_BOUND)
    }
}

/// The main loop: the input object has no `agent_id` key (W§9.1, the test of W§8.3; D5).
fn main_loop(p: &Value) -> bool {
    !p.as_object().is_some_and(|o| o.contains_key("agent_id"))
}

/// `zygo hook workspace --event <event>` (W§9.1, D4, D5). It checks the event, then reads `ZYGO_SESSION_DIR`
/// through `env` (none or empty: nothing), then parses the input; a prompt line that `zygo hook prompt` consumes ends
/// here (D4, R1); then the session folder, the view and the marker. `pre-tool` can give the deny JSON of W§9.2, a
/// Claude Code `prompt` the block JSON of AU86 or the mail delivery line (M§7.2), a Claude Code `stop` the mail
/// block JSON of M§7.1, and a main-loop `message` (the OpenCode plugin's `chat.message` call, M§7.2; FX-33) the
/// delivery line as plain text while `new/` holds mail; every other call gives `None`. `session` (main loop), `prompt`, `stop` and `tool-end` start the
/// mail sync child when sync is on and the gap has passed; a child that does not start changes no output (S§8.1,
/// S§8.4, S-C1). The dispatcher is the one that the `zygo` binary registers (`slash_dispatch`).
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
    run_with_sync(event, stdin, paths, env, zygo, dispatch, &sync_env(paths, zygo))
}

/// `run_with` with the mail sync seams `sync` (S-C9): the clock and the spawn of the sync child.
pub fn run_with_sync(
    event: &str, stdin: &[u8], paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, zygo: &Path,
    dispatch: Option<SlashDispatch>, sync: &Env,
) -> anyhow::Result<Option<String>> {
    run_with_wake(event, stdin, paths, env, zygo, dispatch, sync, &TmuxTurnWake { tmux: tmux_program() })
}

/// `run_with_sync` with the OpenCode turn-end wake `turn_wake` (M§7.3; FX-33 review).
#[allow(clippy::too_many_arguments)]
pub fn run_with_wake(
    event: &str, stdin: &[u8], paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>, zygo: &Path,
    dispatch: Option<SlashDispatch>, sync: &Env, turn_wake: &dyn TurnWake,
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
    if matches!(event, "prompt" | "stop" | "tool-end") || (event == "session" && main) {
        start_sync(&c, sync);
    }
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
                change(&c, &mut start_turn);
                // M§7.2, ML-10 D3: a Claude Code prompt adds the delivery line; the OpenCode plugin drops this output
                // and gets the line from its `chat.message` call (the `message` event, FX-33).
                if matches!(c.view.agent, Agent::ClaudeCode) {
                    let n = unread_mail(&c);
                    if n > 0 {
                        let line = crate::mail::delivery_line(n);
                        return Ok(Some(
                            json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": line}})
                                .to_string(),
                        ));
                    }
                }
            }
        }
        "stop" if main => {
            // M§7.1, ML-10 D1, D2: a Claude Code `Stop` that is not already a forced continuation blocks once while
            // `new/` holds mail, and changes nothing in `activity.json`; every other main-loop stop clears. M§9, ML-10
            // review R3: no block while the phase is `draining` or `stopping`, so `down` does not wait for the mail.
            if payload.get("hook_event_name").and_then(Value::as_str) == Some("Stop")
                && payload.get("stop_hook_active") == Some(&Value::Bool(false))
            {
                let n = unread_mail(&c);
                if n > 0 && !matches!(c.store.phase_unlocked(), Some(Phase::Draining | Phase::Stopping)) {
                    return Ok(Some(crate::mail::stop_block(n)));
                }
            }
            change(&c, &mut |a| {
                a.busy = false;
                a.tools_running = 0;
            });
            // M§7.3 (FX-33 review F1): after the clear, so the wake sees an idle member.
            if matches!(c.view.agent, Agent::OpenCode) {
                wake_after_turn(&c, sync, turn_wake);
            }
        }
        // M§7.2 (FX-33): the OpenCode plugin's `chat.message` call for a user message. Only for the recorded main
        // session (`main_session`), it gives the delivery line, which the plugin adds to that message, so the
        // per-request session text holds no mail line, and records the ids as announced (`MAIL_SEEN`). It changes
        // nothing in `activity.json` and starts no sync child (the turn's `prompt` does). A mail read error is ML-10 D4.
        "message" if main => {
            if main_session(&c, payload_id(&payload, "sessionID")) {
                let ids = unread_ids(&c);
                if !ids.is_empty() {
                    set_mail_seen(&c, &ids);
                    return Ok(Some(crate::mail::delivery_line(ids.len())));
                }
            }
        }
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

/// The production mail sync seams of the hook: the system clock and the `<zygo> mail sync` child. The wake is the
/// real one (mail sync design S§18) with the D2 tmux program; `secret_policy` is the default deadline policy of the
/// secret pass (S§8.6, `DeadlinePolicy::default()`).
fn sync_env(paths: &Paths, zygo: &Path) -> Env {
    Env {
        paths: paths.clone(),
        clock: Box::new(SystemClock),
        probe: Box::new(LsRemote),
        seam: Box::new(NoSeam),
        spawner: Box::new(ChildSpawner { zygo: zygo.to_path_buf() }),
        wake: Box::new(TmuxWake { tmux: tmux_program() }),
        secret_policy: Default::default(),
    }
}

/// The first `<entry>/tmux` over the absolute entries of `PATH` that is an executable file, else `/dev/null/tmux`:
/// rule D2 (`docs/work/phase2/nodes/M1.md:39`) of `crates/zygo/src/cmd/mod.rs:109`, which is `pub(crate)` there.
fn tmux_program() -> PathBuf {
    let found = std::env::var_os("PATH").and_then(|v| {
        std::env::split_paths(&v).filter(|p| p.is_absolute()).map(|p| p.join("tmux")).find(|p| executable(p))
    });
    found.unwrap_or_else(|| PathBuf::from("/dev/null/tmux"))
}

fn executable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

/// Starts the mail sync child of the member's workspace by the gap of S§8.4 (S§8.1). A config that does not load,
/// no `[mail.workspace.*]` table, no `workspace.json` or sync off: nothing, and no line. `workspace.json` is read
/// only when some `[mail.workspace.*]` table exists. A failure of the start gives one `errors.log` line (S-C1).
fn start_sync(c: &Ctx, sync: &Env) {
    let Ok(m) = zygo_config::load(&zygo_config::LoadContext { paths: c.paths.clone(), repo: None, team: None }) else {
        return;
    };
    if m.config.mail.workspace.is_empty() {
        return;
    }
    let Ok(Some(ws)) = c.store.read() else { return };
    let Ok(s) = zygo_config::sync_for(&m.config, &ws.name) else { return };
    zygo_mail_sync::spawn::trigger(sync, &c.ws_id, Some(s.sync_gap), &|t| elog(c, t));
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
/// when it has one), `first_started_at` is set from `pending.started_at` when it is absent or 0 and that time is not 0
/// (T§3.2), `note_pending` is cleared, and the values that `down` saved are cleared unless they were saved
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
                if m.first_started_at.unwrap_or(0) == 0 && p.started_at != 0 {
                    m.first_started_at = Some(p.started_at);
                }
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
            _ if !record_of(m, &session) => {
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

/// Starts the snapshot child of the member `c` (W§7.3, D15); a failure writes one `errors.log` line.
fn start_snapshot(c: &Ctx, zygo: &Path, force: bool) {
    start_snapshot_child(c.paths, &c.ws_id, zygo, force, &|t| elog(c, t));
}

/// The member that a session verb mirrors to (AU105): the workspace id, the pane key and the zygo session of the
/// view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbMember {
    pub ws_id: String,
    pub pane: String,
    pub session: String,
}

/// A reader of process variables (`std::env::var_os` outside tests): the `ZYGO_WORKSPACE` gate of `verb_member`.
pub type EnvGate<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// The marker of `dir` by the hook rule. With `gate` (the slash form, which runs in the prompt hook), `None` unless
/// `gate("ZYGO_WORKSPACE")` is set and not empty, before any read (the gate of `prompt::run_in_env`; W§18 #89). Then
/// `check_folder`, `read_view` and `ctx_of` with `log = false`: `None` (not a member) for a refused folder, a
/// `view.json` that is missing, a link, a FIFO, empty, not JSON or unreadable, no marker, or a workspace id or pane
/// key that fails AU84. It writes no line and no file.
pub fn verb_member(dir: &SessionDir, paths: &Paths, gate: Option<EnvGate<'_>>) -> Option<VerbMember> {
    if let Some(gate) = gate
        && !gate("ZYGO_WORKSPACE").is_some_and(|v| !v.is_empty())
    {
        return None;
    }
    check_folder(dir).ok()?;
    let view = read_view(dir).ok()??;
    let c = ctx_of("verb", dir, view, paths, false)?;
    Some(VerbMember { ws_id: c.ws_id, pane: c.pane, session: c.view.session })
}

/// `true` when `m` belongs to zygo session `session`: its `pending` names it, or its `zygo_session` is it.
pub fn record_of(m: &MemberRecord, session: &str) -> bool {
    m.pending.as_ref().is_some_and(|p| p.zygo_session == session) || m.zygo_session.as_deref() == Some(session)
}

/// Starts `<zygo> ws snapshot --ws-id <ws_id> [--force]` as a detached child and does not wait for it (W§7.3, D15):
/// stdin, stdout and stderr to `/dev/null`, on unix in `/` and in a new session (`setsid`). A child that cannot start
/// queues the flag again and gives one line to `log`.
// The caller does not wait for the child: it ends soon after, and `init` reaps the child (W§7.3: the hook does not
// wait for the 2 s check of AU89).
#[allow(clippy::zombie_processes)]
pub fn start_snapshot_child(paths: &Paths, ws_id: &str, zygo: &Path, force: bool, log: &dyn Fn(&str)) {
    let mut cmd = Command::new(zygo);
    cmd.args(["ws", "snapshot", "--ws-id", ws_id]);
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
        match snapshot::queue_pending(paths, ws_id, force) {
            Ok(()) => log(&format!("cannot start the snapshot: {e}; snapshot.pending set again")),
            Err(q) => log(&format!("cannot start the snapshot: {e}; snapshot.pending not set: {q:#}")),
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
/// `text` is the session text. The output is the session text, an empty line, item 8 when `view.mail` is on (with the
/// S§10.4 clause when sync is on and the member cache is not empty), the
/// state reads and holds a record for the pane, then the note, then, for Claude Code only, the mail delivery line
/// when `view.mail` is on and `new/` holds mail (M§7.2); empty parts are left out. OpenCode gets the delivery line in
/// the user turn through the `message` event, so its per-request text does not change with the unread count and
/// stays in the cached prefix (base design §10.4, session text design T§11; FX-33). The OpenCode note is shown only
/// while its inputs are not bound to an agent session or are bound to the payload `input.sessionID` (W§9.4, AU72,
/// AU104; FX-33): until the plugin's `session` call removes inputs of another session (D10), they are not shown to
/// this one, and a payload with no id shows bound inputs to no session. Claude Code: the whole hook
/// output is written to `out` and flushed first (a write error returns `Err` with nothing written to any file), then
/// the member record is written (D8, D9), then the note inputs are removed from the view when it held them; the result
/// is `None`; for the main loop the mail sync child then starts by the gap, and a child that does not start changes no
/// output (S§8.1 row 2, S§8.4, S-C1). OpenCode adds the session text to every model request, so the result is
/// `Some(output)` and nothing is written but the item 8 store: the plugin's `session` call owns the record, the note
/// rule (D10) and the sync start. For OpenCode with a payload `input.sessionID`, item 8 comes from
/// `session_start::fixed_item`: built at the first request of that session id and reused as it is by its later
/// requests (M§8a item 1, FX-9); a payload with no id builds it at each request.
pub fn session_start(
    payload: &Value, dir: &SessionDir, view: &SessionView, paths: &Paths, zygo: &Path, out: &mut dyn Write, text: &str,
) -> anyhow::Result<Option<String>> {
    session_start_with_sync(payload, dir, view, paths, zygo, out, text, &sync_env(paths, zygo))
}

/// `session_start` with the mail sync seams `sync` (S-C9): the clock and the spawn of the sync child.
#[allow(clippy::too_many_arguments)]
pub fn session_start_with_sync(
    payload: &Value, dir: &SessionDir, view: &SessionView, paths: &Paths, zygo: &Path, out: &mut dyn Write, text: &str,
    sync: &Env,
) -> anyhow::Result<Option<String>> {
    // The OpenCode call runs per model request: a marker that fails its check writes no line there (D6).
    let log = matches!(view.agent, Agent::ClaudeCode);
    let Some(c) = ctx_of("session-start", dir, view.clone(), paths, log) else { return Ok(Some(text.to_string())) };
    let inputs = view.workspace.as_ref().and_then(|w| w.resume_note.as_ref());
    // M§8a item 1: item 8 after the mechanics text, before the resume note, from `workspace.json` read at this start,
    // with the S§10.4 clause when sync is on and the member cache is not empty.
    // A read error or no state gives no item and no line of its own (record_session's read writes the one line, C6).
    let mut built = || c.store.read().ok().flatten().and_then(|ws| crate::mail::session_item_with(&ws, &c.pane, &crate::mail::remote_addresses(c.paths, &c.ws_id, &ws.name)));
    // FX-9: OpenCode calls this per model request; the main-loop SessionStart of M§8a item 1 is the first request of
    // an OpenCode session id, and later requests of that id reuse its stored item 8.
    let item = match (view.mail, view.agent, payload.pointer("/input/sessionID").and_then(Value::as_str)) {
        (false, ..) => None,
        // A store that cannot be used builds item 8 and writes no line (D6).
        (true, Agent::OpenCode, Some(id)) => crate::session_start::fixed_item(dir, id, &mut built),
        (true, ..) => built(),
    };
    // M§7.2, ML-10 D5: the Claude Code mail delivery line comes after the resume note while `new/` holds mail. OpenCode
    // gets it from the `message` event and does not read `mail/` here (FX-33).
    let line = match view.agent {
        Agent::ClaudeCode => match unread_mail(&c) {
            0 => None,
            n => Some(crate::mail::delivery_line(n)),
        },
        Agent::OpenCode => None,
    };
    // W§9.4, AU104 (FX-33): OpenCode shows the note only to the agent session its inputs are bound to.
    let shown = inputs.filter(|n| match view.agent {
        Agent::ClaudeCode => true,
        Agent::OpenCode => match n.agent_session.as_deref() {
            None => true,
            Some(owner) => payload.pointer("/input/sessionID").and_then(Value::as_str) == Some(owner),
        },
    });
    let output = [Some(text.to_string()), item, shown.and_then(note), line]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
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
            // S§8.1 row 2, S§10.2 item 2: this is the member `session` event of Claude Code (W§9.1); the main loop
            // starts the sync child by the gap, after the output is written (S-C1).
            if main_loop(payload) {
                start_sync(&c, sync);
            }
            Ok(None)
        }
        Agent::OpenCode => Ok(Some(output)),
    }
}
