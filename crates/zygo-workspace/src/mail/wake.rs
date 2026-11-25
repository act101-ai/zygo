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
    /// The key is not a member, or the member's launcher is not live.
    NotLive,
    /// `@zygo-ws` of the session or `@zygo-pane` of the pane is not this workspace's or this key's, or two panes
    /// hold the key.
    Mismatch,
    /// Another wake of this pane is running, or the last wake was within `wake_gap`.
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

/// Wakes the member `key` when it is idle (M§7.3): checks the member, takes the wake lock of its maildir, checks
/// `wake_gap`, `@zygo-ws` and `@zygo-pane`, then types [`WAKE_TEXT`] and Enter and writes `wake.json`. It returns at
/// the first check that fails. Only `Failed` writes an `errors.log` line.
pub fn wake(store: &Store, ws: &Workspace, mux: &dyn Multiplexer, key: &str, gap: Duration, probe: &dyn Probe) -> Outcome {
    let fail = |e: &dyn std::fmt::Display| {
        store.log_error(&format!("mail wake {key}: {e}"));
        Outcome::Failed(e.to_string())
    };
    let Some(m) = ws.members.get(key) else {
        return Outcome::NotLive;
    };
    let Some(dir) = store.live_session(m) else {
        return Outcome::NotLive;
    };
    if m.launch.events_off || !probe.idle(&dir) {
        return Outcome::Busy;
    }
    let md = store.dir().join("mail").join(key);
    let _lock = match lock_file(&md.join(LOCK), Duration::ZERO) {
        Ok(Some(f)) => f,
        Ok(None) => return Outcome::Gap,
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
    };
    if let Err(e) = mux.send_keys_literal(&pane, WAKE_TEXT).and_then(|()| mux.send_enter(&pane)) {
        return fail(&e);
    }
    if let Err(e) = write_woken(&md.join(WAKE_FILE), now) {
        store.log_error(&format!("mail wake {key}: wake.json not written: {e}"));
    }
    Outcome::Woken
}

/// Why a pane check stopped a wake.
enum Check {
    Mismatch,
    Mux(MuxError),
}

/// M§7.3 step 1: the session's `@zygo-ws` is the workspace id, exactly one pane holds `key`, and that pane's
/// `@zygo-pane`, read again just before the send, is `key`. `Ok(None)`: no pane holds `key`.
fn find_pane(ws: &Workspace, mux: &dyn Multiplexer, key: &str) -> Result<Option<String>, Check> {
    match mux.show_session_option(&ws.tmux_session, WS_OPTION).map_err(Check::Mux)? {
        Some(v) if v == ws.id => {}
        _ => return Err(Check::Mismatch),
    }
    let lines = mux.list_panes(&ws.tmux_session).map_err(Check::Mux)?;
    let mut ids = lines.iter().filter_map(|l| parse_pane_line(l).ok()).filter(|(_, p)| p.key == key).map(|(_, p)| p.id);
    let Some(id) = ids.next() else {
        return Ok(None);
    };
    if ids.next().is_some() {
        return Err(Check::Mismatch);
    }
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

/// Writes `{"woken_at":"<RFC 3339, UTC, whole seconds>"}` to `path` with mode `0600` (A2).
fn write_woken(path: &Path, now: SystemTime) -> anyhow::Result<()> {
    let at = OffsetDateTime::from(now).replace_nanosecond(0)?.format(&Rfc3339)?;
    write_atomic(path, &serde_json::to_vec(&serde_json::json!({ "woken_at": at }))?)
}

/// Wakes each of `keys`, in order, after a send (M§7.3). A `wake_gap` that does not parse is 30 s, with one
/// `errors.log` line (A7). The outcomes are dropped: [`wake`] logs each failure.
pub fn after_send(paths: &Paths, ws: &Workspace, tmux: &Path, keys: &[String], cfg: &MailConfig) {
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
    let mux = Tmux::new(tmux.to_path_buf(), ws.tmux_socket.clone());
    for key in keys {
        let _ = wake(&store, ws, &mux, key, gap, &RealProbe);
    }
}
