//! The wake of an idle member after a send (M§7.3). Owner: node ML-6; the quiet-window deferral: node FX-35.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zygo_config::{parse_duration, MailConfig};
use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_core::session_dir::{write_atomic, SessionDir};
use zygo_proxy::client::SessionClient;

use crate::activity;
use crate::drain::{self, Idle};
use crate::identity::valid_pane_key;
use crate::mux::tmux::Tmux;
use crate::mux::{check_path, format_escape, parse_pane_line, Multiplexer, MuxError, UnsafePath};
use crate::state::{lock_file, open_nofollow, Opened, Store, Workspace};

/// The text typed into the pane of an idle member (M§7.3 step 2). It holds no mail text (M-C5).
pub const WAKE_TEXT: &str = "zygo: you have mail; read it with mail_read";

const PANE_OPTION: &str = "@zygo-pane";
const WS_OPTION: &str = "@zygo-ws";
/// The bound of one proxy status call of the sender, the drain's bound for the same AU26 read (M§7.3 "Waits").
const STATUS_BOUND: Duration = Duration::from_secs(2);
/// The bound of the proxy status read and of the wake lock on a hook path: the OpenCode `stop` call and the
/// `mail-wake` event (M§7.3 "Waits"; AU56 `activity::LOCK_BOUND`).
pub const HOOK_BOUND: Duration = Duration::from_secs(1);
/// The last try of a wake that meets the quiet window: the first wake is try 1, its retries 2 and 3 (M§7.3).
pub const MAX_TRIES: u8 = 3;
const WAKE_FILE: &str = "wake.json";
const LOCK: &str = "wake.lock";
/// The `wake_gap` used when the configured one does not parse (M§11 default).
const DEFAULT_GAP: Duration = Duration::from_secs(30);

/// The result of one wake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The text and the Enter were sent.
    Woken,
    /// The member is not idle (AU26), or it sends no member events.
    Busy,
    /// The member is idle by AU26 rules 1 and 2 and only the rule 3 time test fails: its last model request ended
    /// less than `REQUEST_QUIET` ago. The value is the rest of the window. Nothing was locked or sent.
    Quiet(Duration),
    /// A `Quiet` wake for which a retry after the value was queued with tmux ([`settle`]).
    Deferred(Duration),
    /// The key is not a pane key or not a member, the member's launcher is not live, or its maildir is missing.
    NotLive,
    /// `@zygo-ws` of the session or `@zygo-pane` of the pane is not this workspace's or this key's, or two panes
    /// hold the key.
    Mismatch,
    /// The last wake, or the last try of a wake, was within `wake_gap`.
    Gap,
    /// No pane of the session holds the key.
    NoPane,
    /// A tmux or file error; it is in `errors.log`.
    Failed(String),
}

/// The AU26 answer for a member at the wake time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ready {
    Idle,
    Busy,
    /// Only the rule 3 time test fails; the value is the rest of the quiet window ([`drain::quiet_left`]).
    Quiet(Duration),
}

/// The idle rule and the clock of a wake; tests replace them.
pub trait Probe {
    /// True when the member of the live session folder `session_dir` is idle (AU26).
    fn idle(&self, session_dir: &Path) -> bool;
    fn now(&self) -> SystemTime;
    /// The AU26 answer at the wake time `now`, so the idle rule and `wake_gap` use one clock (M§7.3). The default
    /// knows no quiet window: `idle` gives `Idle`, else `Busy`.
    fn ready(&self, session_dir: &Path, now: SystemTime) -> Ready {
        let _ = now;
        if self.idle(session_dir) { Ready::Idle } else { Ready::Busy }
    }
}

/// The AU26 answer for the member of the live session folder `session_dir` at `now` (M§7.3): `Busy` when the
/// session's `settings.json` turns hooks off (FIX-24); else `activity.json` (read with no lock; nothing is written,
/// M§9) and one proxy status call bounded by `status_bound` go through [`drain::idle`] (`Idle` or `IdleRuleSkipped`:
/// `Idle`), then [`drain::quiet_left`] (`Some`: `Quiet`); every other answer, a status that cannot be read among them,
/// is `Busy`.
pub fn ready_at(session_dir: &Path, now: SystemTime, status_bound: Duration) -> Ready {
    let sd = SessionDir::new(session_dir.to_path_buf());
    if drain::hooks_off(&sd) {
        return Ready::Busy;
    }
    let act = activity::read(&sd).map_err(|e| e.to_string());
    let status = SessionClient::from_session_dir(session_dir)
        .map_err(|e| e.to_string())
        .and_then(|c| c.timeout(status_bound).status().map_err(|e| e.to_string()));
    let (act, status) = (act.as_ref().map_err(String::as_str), status.as_ref().map_err(String::as_str));
    match drain::idle(act, status, now) {
        Idle::Idle | Idle::IdleRuleSkipped => Ready::Idle,
        _ => drain::quiet_left(act, status, now).map_or(Ready::Busy, Ready::Quiet),
    }
}

/// The production probe: the AU26 rule over `activity.json` and the proxy status ([`ready_at`] with the sender's
/// 2 s status bound), and the system clock.
pub struct RealProbe;

impl Probe for RealProbe {
    /// [`Probe::ready`] at the system time is `Idle`.
    fn idle(&self, session_dir: &Path) -> bool { self.ready(session_dir, self.now()) == Ready::Idle }

    fn now(&self) -> SystemTime { SystemTime::now() }

    fn ready(&self, session_dir: &Path, now: SystemTime) -> Ready { ready_at(session_dir, now, STATUS_BOUND) }
}

/// Wakes the member `key` when it is idle (M§7.3): checks the key, the member and its maildir, applies the AU26 rule
/// at `probe.now()`, waits at most `lock_bound` for the wake lock of the maildir, checks `wake_gap` at the same time,
/// `@zygo-ws` and `@zygo-pane`, writes `wake.json`, then types [`WAKE_TEXT`] and Enter. It returns at the first check
/// that fails; a member in the quiet window gives `Quiet` before the lock. Only `Failed` writes an `errors.log` line.
pub fn wake(
    store: &Store, ws: &Workspace, mux: &dyn Multiplexer, key: &str, gap: Duration, probe: &dyn Probe,
    lock_bound: Duration,
) -> Outcome {
    let fail = |e: &dyn std::fmt::Display| {
        store.log_error(&format!("mail wake {key}: {e}"));
        Outcome::Failed(e.to_string())
    };
    if !valid_pane_key(key) {
        return Outcome::NotLive;
    }
    let Some(m) = ws.members.get(key) else {
        return Outcome::NotLive;
    };
    let Some(dir) = store.live_session(m) else {
        return Outcome::NotLive;
    };
    // A missing maildir (a forgotten pane) is not created again (review fix R9).
    let md = store.dir().join("mail").join(key);
    if !std::fs::symlink_metadata(&md).is_ok_and(|t| t.is_dir()) {
        return Outcome::NotLive;
    }
    if m.launch.events_off {
        return Outcome::Busy;
    }
    let now = probe.now();
    match probe.ready(&dir, now) {
        Ready::Idle => {}
        Ready::Busy => return Outcome::Busy,
        Ready::Quiet(left) => return Outcome::Quiet(left),
    }
    // The sender waits for the lock (A1): a holder that woke the pane wrote `woken_at`, so the gap rule below skips.
    // The hold is bounded by the tmux call bounds. A hook waits at most `HOOK_BOUND` (M§7.3 "Waits").
    let _lock = match lock_file(&md.join(LOCK), lock_bound) {
        Ok(Some(f)) => f,
        Ok(None) => return fail(&"wake.lock not taken"),
        Err(e) => return fail(&e),
    };
    if read_woken(&md.join(WAKE_FILE)).is_some_and(|w| within(now, w, gap)) {
        return Outcome::Gap;
    }
    let pane = match find_pane(ws, mux, key) {
        Ok(Some(p)) => p,
        Ok(None) => return Outcome::NoPane,
        Err(Check::Mismatch) => return Outcome::Mismatch,
        Err(Check::Mux(e)) => return fail(&e),
        Err(Check::Line(e)) => return fail(&e),
    };
    // Written before the text (A4): a wake that then fails still counts for `wake_gap`, so the text is never typed
    // twice inside the gap.
    if let Err(e) = write_woken(&md.join(WAKE_FILE), now) {
        store.log_error(&format!("mail wake {key}: wake.json not written: {e}"));
    }
    if let Err(e) = mux.send_keys_literal(&pane, WAKE_TEXT).and_then(|()| mux.send_enter(&pane)) {
        return fail(&e);
    }
    Outcome::Woken
}

/// The queue of a later try of a wake that met the quiet window (M§7.3); tests replace it.
pub trait Defer {
    /// Asks for try `try_no` of the wake of the member of the live session folder `session_dir` after `after`.
    fn defer(&self, ws: &Workspace, session_dir: &Path, after: Duration, try_no: u8) -> anyhow::Result<()>;
}

/// The production deferral: one `run-shell -b` [`deferred_command`] on the workspace's own tmux server (its `-S`
/// socket, W-C3), through the tmux program `tmux`, for the zygo program `zygo`.
#[derive(Debug, Clone)]
pub struct TmuxDefer {
    pub tmux: PathBuf,
    pub zygo: PathBuf,
}

impl Defer for TmuxDefer {
    fn defer(&self, ws: &Workspace, session_dir: &Path, after: Duration, try_no: u8) -> anyhow::Result<()> {
        if !self.zygo.is_absolute() {
            anyhow::bail!("the zygo program path is not absolute");
        }
        let command = deferred_command(&self.zygo, session_dir, after, try_no)?;
        Tmux::new(self.tmux.clone(), ws.tmux_socket.clone()).run_shell_background(&command)?;
        Ok(())
    }
}

/// The shell command of a deferred wake (M§7.3): `sleep <s>; printf '%s' '{"try":<n>}' | ZYGO_SESSION_DIR='<dir>'
/// '<zygo>' hook workspace --agent claude-code --event mail-wake >/dev/null 2>&1`, `<s>` = `after` rounded up to whole
/// seconds. Both paths pass the AU54 check ([`check_path`]), and each `#` of the text is doubled (AU55).
pub fn deferred_command(zygo: &Path, session_dir: &Path, after: Duration, try_no: u8) -> Result<String, UnsafePath> {
    let (zygo, dir) = (check_path(zygo)?, check_path(session_dir)?);
    let s = whole_secs(after);
    Ok(format_escape(&format!(
        "sleep {s}; printf '%s' '{{\"try\":{try_no}}}' | ZYGO_SESSION_DIR='{dir}' '{zygo}' hook workspace --agent \
         claude-code --event mail-wake >/dev/null 2>&1"
    )))
}

/// `d` rounded up to whole seconds.
fn whole_secs(d: Duration) -> u64 { d.as_secs() + u64::from(d.subsec_nanos() > 0) }

/// The end of try `try_no` of a wake of `key` (M§7.3): `Quiet(left)` before try [`MAX_TRIES`] queues try
/// `try_no + 1` after `left` rounded up plus 1 s and gives `Deferred` with that delay; a deferral that fails gives
/// `Failed` with one `errors.log` line; a member whose launcher is no longer live gives `NotLive`. `Quiet` at try
/// [`MAX_TRIES`] and every other outcome are returned as they are.
pub fn settle(store: &Store, ws: &Workspace, key: &str, out: Outcome, defer: &dyn Defer, try_no: u8) -> Outcome {
    let Outcome::Quiet(left) = out else { return out };
    if try_no >= MAX_TRIES {
        return out;
    }
    let Some(dir) = ws.members.get(key).and_then(|m| store.live_session(m)) else {
        return Outcome::NotLive;
    };
    let after = Duration::from_secs(whole_secs(left) + 1);
    match defer.defer(ws, &dir, after, try_no + 1) {
        Ok(()) => Outcome::Deferred(after),
        Err(e) => {
            let e = format!("{e:#}");
            store.log_error(&format!("mail wake {key}: deferral not queued: {e}"));
            Outcome::Failed(e)
        }
    }
}

/// Why a pane check stopped a wake.
enum Check {
    Mismatch,
    Mux(MuxError),
    /// A `list_panes` line that `parse_pane_line` refuses.
    Line(String),
}

/// M§7.3 step 1: the session's `@zygo-ws` is the workspace id, exactly one pane holds `key`, and that pane's
/// `@zygo-pane`, read again just before the send, is `key`. `Ok(None)`: no pane holds `key`.
fn find_pane(ws: &Workspace, mux: &dyn Multiplexer, key: &str) -> Result<Option<String>, Check> {
    match mux.show_session_option(&ws.tmux_session, WS_OPTION).map_err(Check::Mux)? {
        Some(v) if v == ws.id => {}
        _ => return Err(Check::Mismatch),
    }
    let lines = mux.list_panes(&ws.tmux_session).map_err(Check::Mux)?;
    let mut ids = Vec::new();
    for l in &lines {
        let (_, p) = parse_pane_line(l).map_err(|e| Check::Line(format!("pane list line not read: {e}")))?;
        if p.key == key {
            ids.push(p.id);
        }
    }
    if ids.len() > 1 {
        return Err(Check::Mismatch);
    }
    let Some(id) = ids.pop() else {
        return Ok(None);
    };
    match mux.show_pane_option(&id, PANE_OPTION).map_err(Check::Mux)? {
        Some(v) if v == key => Ok(Some(id)),
        _ => Err(Check::Mismatch),
    }
}

/// True when `now` and `woken` are less than `gap` apart, in either direction (A3).
fn within(now: SystemTime, woken: SystemTime, gap: Duration) -> bool {
    match now.duration_since(woken) {
        Ok(d) => d < gap,
        Err(e) => e.duration() < gap,
    }
}

/// `woken_at` of `wake.json`; `None` when the file is missing, cannot be read, is a link or is not a regular file,
/// does not parse, or holds no RFC 3339 `woken_at` (A2).
fn read_woken(path: &Path) -> Option<SystemTime> {
    let Ok(Opened::File(mut f)) = open_nofollow(std::fs::OpenOptions::new().read(true), path) else {
        return None;
    };
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let text = v.get("woken_at")?.as_str()?;
    OffsetDateTime::parse(text, &Rfc3339).ok().map(SystemTime::from)
}

/// Writes `{"woken_at":"<RFC 3339, UTC, full precision>"}` to `path` with mode `0600` (A2). The fraction of a
/// second is kept, so the gap is never shorter than `wake_gap` (review fix R7).
fn write_woken(path: &Path, now: SystemTime) -> anyhow::Result<()> {
    let at = OffsetDateTime::from(now).format(&Rfc3339)?;
    write_atomic(path, &serde_json::to_vec(&serde_json::json!({ "woken_at": at }))?)
}

/// The zygo program of a sender, for its deferred wakes: every sender is the zygo binary (`zygo mail send`,
/// `zygo mcp`, the sync child). `current_exe` canonicalized, else as given; none: an empty path, which
/// [`TmuxDefer`] refuses with one `errors.log` line.
fn sender_zygo() -> PathBuf {
    std::env::current_exe().map(|p| p.canonicalize().unwrap_or(p)).unwrap_or_default()
}

/// Wakes each of `keys` after a send (M§7.3), with the tmux program `tmux`, [`RealProbe`] and the system time. See
/// [`after_send_at`].
pub fn after_send(paths: &Paths, ws: &Workspace, tmux: &Path, keys: &[String], cfg: &MailConfig) {
    after_send_at(paths, ws, tmux, keys, cfg, SystemTime::now());
}

/// Wakes each of `keys` after a send at `now` (M§7.3): [`after_send_with`] through the tmux program `tmux` with
/// [`RealProbe`], then [`settle`] as try 1 with [`TmuxDefer`] for the sender's own zygo program, so a member in the
/// quiet window gets a retry. Returns each key with its settled outcome.
pub fn after_send_at(
    paths: &Paths, ws: &Workspace, tmux: &Path, keys: &[String], cfg: &MailConfig, now: SystemTime,
) -> Vec<(String, Outcome)> {
    if keys.is_empty() {
        return Vec::new();
    }
    let mux = Tmux::new(tmux.to_path_buf(), ws.tmux_socket.clone());
    let outs = after_send_with(paths, ws, keys, cfg, &mux, &RealProbe, now);
    let store = Store::open(paths, &ws.id);
    let defer = TmuxDefer { tmux: tmux.to_path_buf(), zygo: sender_zygo() };
    outs.into_iter().map(|(k, o)| {
        let o = settle(&store, ws, &k, o, &defer, 1);
        (k, o)
    }).collect()
}

/// [`Probe`] with a fixed clock.
struct Clocked<'a, P> {
    probe: &'a P,
    now: SystemTime,
}

impl<P: Probe> Probe for Clocked<'_, P> {
    fn idle(&self, session_dir: &Path) -> bool { self.probe.idle(session_dir) }
    fn now(&self) -> SystemTime { self.now }
    fn ready(&self, session_dir: &Path, now: SystemTime) -> Ready { self.probe.ready(session_dir, now) }
}

/// Wakes each of `keys` after a send (M§7.3), each on its own thread, and returns when all are done, so one slow
/// member does not delay the others (M§13 #5). `probe` gives the idle rule and `now` the clock of the idle rule and
/// the gap; the clock of `probe` is not used. The sender waits for the wake lock (A1). A `wake_gap` that does not
/// parse is 30 s, with one `errors.log` line (A7). Returns each key with its outcome, in the order of `keys`; it
/// queues no retry ([`after_send_at`] does). [`wake`] logs each failure.
pub fn after_send_with<P: Probe + Sync, M: Multiplexer + Sync>(
    paths: &Paths,
    ws: &Workspace,
    keys: &[String],
    cfg: &MailConfig,
    mux: &M,
    probe: &P,
    now: SystemTime,
) -> Vec<(String, Outcome)> {
    if keys.is_empty() {
        return Vec::new();
    }
    let store = Store::open(paths, &ws.id);
    let gap = parse_duration(&cfg.wake_gap).unwrap_or_else(|| {
        store.log_error(&format!(
            "mail wake: [mail] wake_gap \"{}\" is not <n>s, <n>m, <n>h or <n>d; using 30s",
            esc(&cfg.wake_gap)
        ));
        DEFAULT_GAP
    });
    let probe = Clocked { probe, now };
    let (store, probe) = (&store, &probe);
    std::thread::scope(|s| {
        let handles: Vec<_> =
            keys.iter().map(|key| (key, s.spawn(move || wake(store, ws, mux, key, gap, probe, Duration::MAX)))).collect();
        handles
            .into_iter()
            .map(|(key, h)| {
                (key.clone(), h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            })
            .collect()
    })
}
