//! The wake of an idle member after a send (M§7.3). Owner: node ML-6.
use std::io::Read;
use std::path::Path;
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
use crate::mux::{parse_pane_line, Multiplexer, MuxError};
use crate::state::{lock_file, open_nofollow, Opened, Store, Workspace};

/// The text typed into the pane of an idle member (M§7.3 step 2). It holds no mail text (M-C5).
pub const WAKE_TEXT: &str = "zygo: you have mail; read it with mail_read";

const PANE_OPTION: &str = "@zygo-pane";
const WS_OPTION: &str = "@zygo-ws";
/// The bound of one proxy status call, the drain's bound for the same AU26 read.
const STATUS_BOUND: Duration = Duration::from_secs(2);
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

/// The idle rule and the clock of a wake; tests replace them.
pub trait Probe {
    /// True when the member of the live session folder `session_dir` is idle (AU26).
    fn idle(&self, session_dir: &Path) -> bool;
    fn now(&self) -> SystemTime;
}

/// The production probe: the AU26 rule over `activity.json` and the proxy status, and the system clock.
pub struct RealProbe;

impl Probe for RealProbe {
    /// `false` when the session's `settings.json` turns hooks off (FIX-24). It reads `activity.json` with no lock
    /// and writes nothing (M§9).
    fn idle(&self, session_dir: &Path) -> bool {
        let sd = SessionDir::new(session_dir.to_path_buf());
        if drain::hooks_off(&sd) {
            return false;
        }
        let act = activity::read(&sd).map_err(|e| e.to_string());
        let status = SessionClient::from_session_dir(session_dir)
            .map_err(|e| e.to_string())
            .and_then(|c| c.timeout(STATUS_BOUND).status().map_err(|e| e.to_string()));
        let r = drain::idle(act.as_ref().map_err(String::as_str), status.as_ref().map_err(String::as_str), self.now());
        matches!(r, Idle::Idle | Idle::IdleRuleSkipped)
    }

    fn now(&self) -> SystemTime { SystemTime::now() }
}

/// Wakes the member `key` when it is idle (M§7.3): checks the key, the member and its maildir, waits for the wake
/// lock of the maildir, checks `wake_gap`, `@zygo-ws` and `@zygo-pane`, writes `wake.json`, then types
/// [`WAKE_TEXT`] and Enter. It returns at the first check that fails. Only `Failed` writes an `errors.log` line.
pub fn wake(store: &Store, ws: &Workspace, mux: &dyn Multiplexer, key: &str, gap: Duration, probe: &dyn Probe) -> Outcome {
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
    if m.launch.events_off || !probe.idle(&dir) {
        return Outcome::Busy;
    }
    // The lock is waited for (A1): a holder that woke the pane wrote `woken_at`, so the gap rule below skips. The
    // hold is bounded by the tmux call bounds.
    let _lock = match lock_file(&md.join(LOCK), Duration::MAX) {
        Ok(Some(f)) => f,
        Ok(None) => return fail(&"wake.lock not taken"),
        Err(e) => return fail(&e),
    };
    let now = probe.now();
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

/// Wakes each of `keys` after a send (M§7.3), with the tmux program `tmux`, [`RealProbe`] and the system time. See
/// [`after_send_with`].
pub fn after_send(paths: &Paths, ws: &Workspace, tmux: &Path, keys: &[String], cfg: &MailConfig) {
    if keys.is_empty() {
        return;
    }
    let mux = Tmux::new(tmux.to_path_buf(), ws.tmux_socket.clone());
    after_send_with(paths, ws, keys, cfg, &mux, &RealProbe, SystemTime::now());
}

/// [`Probe`] with a fixed clock.
struct Clocked<'a, P> {
    probe: &'a P,
    now: SystemTime,
}

impl<P: Probe> Probe for Clocked<'_, P> {
    fn idle(&self, session_dir: &Path) -> bool { self.probe.idle(session_dir) }
    fn now(&self) -> SystemTime { self.now }
}

/// Wakes each of `keys` after a send (M§7.3), each on its own thread, and returns when all are done, so one slow
/// member does not delay the others (M§13 #5). `probe` gives the idle rule and `now` the clock; the clock of `probe`
/// is not used. A `wake_gap` that does not parse is 30 s, with one `errors.log` line (A7). The outcomes are dropped:
/// [`wake`] logs each failure.
pub fn after_send_with<P: Probe + Sync, M: Multiplexer + Sync>(
    paths: &Paths,
    ws: &Workspace,
    keys: &[String],
    cfg: &MailConfig,
    mux: &M,
    probe: &P,
    now: SystemTime,
) {
    if keys.is_empty() {
        return;
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
        for key in keys {
            s.spawn(move || wake(store, ws, mux, key, gap, probe));
        }
    });
}
