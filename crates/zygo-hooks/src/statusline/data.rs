//! The data of the status line HUD (H§7): bounded stdin (AU24), the cache `statusline.json` version 2 with its lease
//! `statusline.lock` (AU4, H§7.7), the three workers under the deadline, and the failure display inputs (AU20).
//!
//! This module reads no process environment and no terminal: each value comes in through `env`, `paths` and `stdin`.
//! The one exception is the hook's own working directory, the last source of the git directory (H§7.1).

use std::ffi::OsString;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_proxy::client::{ClientError, SessionClient, SessionStatus, WindowArg};
use zygo_usage::Store;
use zygo_work::repo::{hud_git, HudGit};

use super::render::{HudData, Outcome};
use super::width::terminal_width;

/// The cache file in the session directory (H§7.7).
pub const CACHE_FILE: &str = "statusline.json";
/// The lease file in the session directory (H§7.7).
pub const LOCK_FILE: &str = "statusline.lock";
/// The last logged width source and width, in the session directory (`log_width`).
pub const WIDTH_FILE: &str = "statusline.width";

/// The cache format; a file with another `v` counts as no cache.
const VERSION: u32 = 2;
/// Stdin kept for the parse (AU24).
const STDIN_CAP: usize = 64 * 1024;
/// The longest wait for the stdin value (AU24).
const STDIN_WAIT: Duration = Duration::from_millis(200);
/// A part is due when it is this old (AU4).
const REFRESH_SECS: u64 = 30;
/// Status data older than this is stale; up to this age the fetch cap follows the last latency (AU4, AU20).
const STALE_SECS: u64 = 90;
/// The earliest next status fetch after `offline` or `suspended`, and after `ended` (AU20).
const RETRY_SECS: u64 = 30;
const ENDED_RETRY_SECS: u64 = 300;
/// `retry_after` of the lease: longer than the 3 s run (H§7.7).
const LOCK_RETRY_AFTER: u64 = 5;
/// The git worker's budget (AU6), and the time the hook waits past it for the worker's answer.
const GIT_BUDGET: Duration = Duration::from_millis(400);
const GIT_GRACE: Duration = Duration::from_millis(300);
/// The cap of the trimmed query (H§7.4).
const TRIM_CAP: Duration = Duration::from_secs(1);
/// The workers end this long before the deadline: time for the cache write and the print.
const WRITE_RESERVE: Duration = Duration::from_millis(50);
/// A leftover temp file older than this is removed after a cache write (H§7.7).
const TMP_MAX_AGE: Duration = Duration::from_secs(60);

// ---- cache (H§7.7) ----

/// `statusline.json`: `{v: 2, at, outcome, retry_at, latency_ms, status: {at, …}, git: {at, cwd, …}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Cache {
    v: u32,
    /// Unix seconds of the write.
    #[serde(default)]
    at: u64,
    /// The result of the last status fetch (AU20).
    #[serde(default)]
    outcome: Outcome,
    /// The earliest next status fetch, Unix seconds; 0 after a success.
    #[serde(default)]
    retry_at: u64,
    /// The time of the last successful status fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<StatusPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git: Option<GitPart>,
}

impl Default for Cache {
    fn default() -> Self {
        Cache { v: VERSION, at: 0, outcome: Outcome::Ok, retry_at: 0, latency_ms: None, status: None, git: None }
    }
}

/// What the proxy-derived segments render, and no more (H-C4): no named window field of the status reply.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct StatusPart {
    /// Unix seconds of the fetch.
    at: u64,
    /// The display name: the served model without a leading `claude-` (AU10).
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    /// The fallback of `ZYGO_WORK` (AU11).
    #[serde(skip_serializing_if = "Option::is_none")]
    work: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    used: Option<u64>,
    /// The number printed after `/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<u64>,
    /// The bar denominator D (AU17); present only with a known window.
    #[serde(skip_serializing_if = "Option::is_none")]
    denominator: Option<u64>,
    /// The `⇣` value: a number that is below the window, or `off` (AU28).
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold: Option<WindowArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens_out: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spend_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spend_notional: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requests_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requests_priced: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_share: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    passthrough: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    team_session: Option<bool>,
}

impl StatusPart {
    fn of(s: &SessionStatus, now: u64) -> StatusPart {
        let window = s.context_window;
        let denominator =
            window.filter(|w| *w > 0).map(|w| s.effective_window.filter(|e| *e > 0 && *e < w).unwrap_or(w));
        let threshold = if s.compact == Some(WindowArg::Off) {
            Some(WindowArg::Off)
        } else {
            s.threshold.zip(window).filter(|(t, w)| t < w).map(|(t, _)| WindowArg::Tokens(t))
        };
        let model = s.model.as_deref().map(|m| m.strip_prefix("claude-").unwrap_or(m)).filter(|m| !m.is_empty());
        StatusPart {
            at: now,
            model: model.map(str::to_string),
            profile: s.profile.clone(),
            work: s.work.clone(),
            used: s.used_context,
            window,
            denominator,
            threshold,
            tokens_in: s.tokens_in,
            tokens_out: s.tokens_out,
            spend_usd: s.spend_usd,
            spend_notional: s.spend_notional,
            requests_total: s.requests_total,
            requests_priced: s.requests_priced,
            cache_read_share: s.cache_read_share,
            passthrough: s.passthrough,
            team_session: s.team_session,
        }
    }

    /// The status data in the form that `render` takes. The render removes one leading `claude-` from `model`, so a
    /// display name that starts with it gets one more.
    fn to_status(&self) -> SessionStatus {
        let model = self.model.as_ref().map(|m| if m.starts_with("claude-") { format!("claude-{m}") } else { m.clone() });
        SessionStatus {
            profile: self.profile.clone(),
            work: self.work.clone(),
            used_context: self.used,
            effective_window: self.denominator,
            compact: (self.threshold == Some(WindowArg::Off)).then_some(WindowArg::Off),
            cache_read_share: self.cache_read_share,
            model,
            context_window: self.window,
            threshold: match self.threshold {
                Some(WindowArg::Tokens(n)) => Some(n),
                _ => None,
            },
            tokens_in: self.tokens_in,
            tokens_out: self.tokens_out,
            spend_usd: self.spend_usd,
            spend_notional: self.spend_notional,
            requests_total: self.requests_total,
            requests_priced: self.requests_priced,
            passthrough: self.passthrough,
            team_session: self.team_session,
            ..Default::default()
        }
    }
}

/// The git part. A failed try has `at`, `cwd` and no data, so it renders no git segment and is not due for 30 s.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct GitPart {
    at: u64,
    /// The stdin cwd of the try; data of another cwd is never rendered (H§7.7, M3).
    cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    behind: Option<u32>,
    /// The cwd is in a linked worktree (OP9).
    linked: bool,
    /// The final directory name of the worktree's top-level path.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
}

impl GitPart {
    fn of(now: u64, cwd: &str, found: Option<HudGit>) -> GitPart {
        let Some(g) = found else { return GitPart { at: now, cwd: cwd.to_string(), ..Default::default() } };
        GitPart {
            at: now,
            cwd: cwd.to_string(),
            linked: g.git_dir != g.common_dir,
            worktree: g.top.file_name().map(|n| n.to_string_lossy().into_owned()),
            branch: g.branch,
            ahead: g.ahead,
            behind: g.behind,
        }
    }
}

fn load_cache(path: &Path) -> Option<Cache> {
    let cache: Cache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (cache.v == VERSION).then_some(cache)
}

/// A part with this `at` is due: missing, in the future, or 30 s old or more.
fn part_due(at: Option<u64>, now: u64) -> bool {
    at.is_none_or(|at| at > now || now - at >= REFRESH_SECS)
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// `bytes` to a unique temp file next to `target` (`<name>.<pid>.<n>.tmp`, mode 0600 on unix), then a rename over
/// `target`. A failed write leaves no temp file and is ignored (C6).
fn write_atomic(target: &Path, bytes: &[u8]) {
    let Some(name) = target.file_name() else { return };
    let mut tmp_name = name.to_os_string();
    tmp_name.push(format!(".{}.{}.tmp", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp = target.with_file_name(tmp_name);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let done = o.open(&tmp).and_then(|mut f| f.write_all(bytes)).and_then(|()| std::fs::rename(&tmp, target));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Removes `statusline-*.tmp` and `statusline.json.*.tmp` files older than 60 s (H§7.7).
fn sweep_tmp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.ends_with(".tmp") && (name.starts_with("statusline-") || name.starts_with("statusline.json."))) {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|m| m.elapsed().is_ok_and(|age| age > TMP_MAX_AGE));
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

// ---- lease (H§7.7) ----

#[derive(Serialize, Deserialize)]
struct LockBody {
    tried_at: u64,
    retry_after: u64,
}

/// The lease of one run; the drop removes the lock file.
struct Lease {
    path: PathBuf,
}

impl Lease {
    /// Creates `statusline.lock` with `create_new`. A lock that a dead run left (older than its `retry_after`) is
    /// removed and taken once. `None` when another run holds the lock or the file cannot be created.
    fn take(dir: &Path, now: u64) -> Option<Lease> {
        let path = dir.join(LOCK_FILE);
        let body = serde_json::to_vec(&LockBody { tried_at: now, retry_after: LOCK_RETRY_AFTER }).ok()?;
        for second_try in [false, true] {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = f.write_all(&body);
                    return Some(Lease { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !second_try && lock_is_dead(&path, now) => {
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A lock older than its `retry_after` (at most 60 s), or with a `tried_at` further in the future than that. A lock
/// with no readable body (its holder has not written it yet) is dead when the file is older than 5 s.
fn lock_is_dead(path: &Path, now: u64) -> bool {
    let body: Option<LockBody> = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    match body {
        Some(b) => {
            let after = b.retry_after.min(60);
            b.tried_at > now.saturating_add(after) || now.saturating_sub(b.tried_at) > after
        }
        None => std::fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|m| m.elapsed().is_ok_and(|age| age > Duration::from_secs(LOCK_RETRY_AFTER))),
    }
}

// ---- stdin (AU24, H§7.1) ----

enum Parsed {
    Complete(Value),
    Incomplete,
    Invalid,
}

/// The first JSON value of `buf`.
fn first_value(buf: &[u8]) -> Parsed {
    match serde_json::Deserializer::from_slice(buf).into_iter::<Value>().next() {
        Some(Ok(v)) => Parsed::Complete(v),
        Some(Err(e)) if e.is_eof() => Parsed::Incomplete,
        None => Parsed::Incomplete,
        Some(Err(_)) => Parsed::Invalid,
    }
}

/// The stdin object: parsed as soon as the value is complete, or at 64 KiB, end of file or `wait`, whichever comes
/// first. A detached thread owns the reader; after the parse it reads and discards the rest, and the hook never joins
/// it (PH11). A value that is not a JSON object, no value, and a value that is not in by `wait` are an empty object.
fn read_stdin(mut stdin: Box<dyn Read + Send + 'static>, wait: Duration) -> Map<String, Value> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("zygo-hud-stdin".into()).spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8 * 1024];
        let mut value = None;
        loop {
            let room = STDIN_CAP - buf.len();
            if room == 0 {
                break;
            }
            let n = match stdin.read(&mut chunk[..room.min(8 * 1024)]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            buf.extend_from_slice(&chunk[..n]);
            // An object can end only at a `}`: no parse for a chunk without one.
            if chunk[..n].contains(&b'}') {
                match first_value(&buf) {
                    Parsed::Complete(v) => {
                        value = Some(v);
                        break;
                    }
                    Parsed::Invalid => break,
                    Parsed::Incomplete => {}
                }
            }
        }
        let value = value.or_else(|| match first_value(&buf) {
            Parsed::Complete(v) => Some(v),
            _ => None,
        });
        let _ = tx.send(value);
        drop(buf);
        let _ = std::io::copy(&mut stdin, &mut std::io::sink());
    });
    if spawned.is_err() {
        return Map::new();
    }
    match rx.recv_timeout(wait) {
        Ok(Some(Value::Object(o))) => o,
        _ => Map::new(),
    }
}

/// The keys that the HUD reads (H§7.1); a missing or mistyped key gives `None` for its own value only.
struct Input {
    /// `cwd`, else `workspace.current_dir`.
    cwd: Option<String>,
    /// `effort`: a string, or an object with a string `level` (PH5); another shape drops the effort part.
    effort: Option<String>,
}

impl Input {
    fn of(o: &Map<String, Value>) -> Input {
        let text = |v: Option<&Value>| v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let cwd = text(o.get("cwd")).or_else(|| text(o.get("workspace").and_then(|w| w.get("current_dir"))));
        let effort = text(o.get("effort")).or_else(|| text(o.get("effort").and_then(|e| e.get("level"))));
        Input { cwd, effort }
    }
}

// ---- test seams ----

/// The seams of the `test-hooks` feature, each read through `env`. Without the feature every field keeps its default.
#[derive(Default)]
struct Seams {
    /// `ZYGO_TEST_NOW`: the clock, Unix seconds.
    now: Option<u64>,
    /// `ZYGO_TEST_STATUSLINE_BLOCK`: the status fetch waits 10 s before it starts.
    block_fetch: bool,
    /// `ZYGO_TEST_FETCH_DELAY_MS`: the status fetch waits this long before it starts (P-HUD-1).
    fetch_delay: Option<Duration>,
    /// `ZYGO_TEST_GIT_DELAY_MS`: the git worker waits this long before it starts.
    git_delay: Option<Duration>,
    /// `ZYGO_TEST_GIT_PANIC=1`: the git worker panics.
    git_panic: bool,
    /// `ZYGO_TEST_GIT_BUDGET_MS`: the git worker's budget in place of 400 ms (a loaded test machine).
    git_budget: Option<Duration>,
    /// `ZYGO_TEST_HUD_DUMP`: the file that gets the parsed stdin keys and the HUD environment (P-HUD-1).
    dump: Option<PathBuf>,
}

impl Seams {
    #[cfg(feature = "test-hooks")]
    fn read(env: &dyn Fn(&str) -> Option<OsString>) -> Seams {
        let set = |key: &str| env(key).filter(|v| !v.is_empty());
        let number = |key: &str| set(key).and_then(|v| v.to_str()?.trim().parse::<u64>().ok());
        let millis = |key: &str| number(key).map(Duration::from_millis);
        Seams {
            now: number("ZYGO_TEST_NOW"),
            block_fetch: set("ZYGO_TEST_STATUSLINE_BLOCK").is_some(),
            fetch_delay: millis("ZYGO_TEST_FETCH_DELAY_MS"),
            git_delay: millis("ZYGO_TEST_GIT_DELAY_MS"),
            git_panic: set("ZYGO_TEST_GIT_PANIC").is_some_and(|v| v == "1"),
            git_budget: millis("ZYGO_TEST_GIT_BUDGET_MS"),
            dump: set("ZYGO_TEST_HUD_DUMP").map(PathBuf::from),
        }
    }

    #[cfg(not(feature = "test-hooks"))]
    fn read(_env: &dyn Fn(&str) -> Option<OsString>) -> Seams {
        Seams::default()
    }
}

/// The environment variables of H§7 that `dump` records (P-HUD-1). `TERM_PROGRAM` is read by no HUD rule since AU2
/// of 2026-11-22 (the PH8 and PH10 fallback: on Windows the plain set unless `ZYGO_NERD_FONT=1`); the dump records it
/// as evidence for a later Windows run of PH10.
const HUD_ENV: &[&str] = &[
    "ZYGO_SESSION_DIR",
    "ZYGO_WORK",
    "ZYGO_URL",
    "NO_COLOR",
    "ZYGO_NERD_FONT",
    "TERM",
    "COLORTERM",
    "WT_SESSION",
    "TERM_PROGRAM",
    "COLUMNS",
];

/// `{"stdin_keys": [...], "effort": <the stdin value>, "env": {<name>: <value or null>}}` to `path` (P-HUD-1: PH4,
/// PH5).
fn dump(path: &Path, stdin: &Map<String, Value>, env: &dyn Fn(&str) -> Option<OsString>) {
    let vars: Map<String, Value> = HUD_ENV
        .iter()
        .map(|k| (k.to_string(), env(k).map_or(Value::Null, |v| Value::String(v.to_string_lossy().into_owned()))))
        .collect();
    let out = serde_json::json!({
        "stdin_keys": stdin.keys().collect::<Vec<_>>(),
        "effort": stdin.get("effort").cloned().unwrap_or(Value::Null),
        "env": vars,
    });
    write_atomic(path, out.to_string().as_bytes());
}

// ---- workers ----

/// Runs `f` on a thread that the hook never joins. A panic in `f` writes one stderr line,
/// `zygo hook: statusline <part> failed`, and sends nothing (C6); the receiver then sees a closed channel at once.
fn worker<T: Send + 'static>(part: &'static str, f: impl FnOnce() -> T + Send + 'static) -> mpsc::Receiver<T> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new().name(format!("zygo-hud-{part}")).spawn(move || {
        match panic::catch_unwind(AssertUnwindSafe(f)) {
            Ok(v) => {
                let _ = tx.send(v);
            }
            Err(_) => zygo_core::hook::error_line(&format!("statusline {part} failed")),
        }
    });
    rx
}

/// The worker's answer, or `None` when it is not in by `until` or the worker failed.
fn wait<T>(rx: &mpsc::Receiver<T>, until: Instant) -> Option<T> {
    rx.recv_timeout(until.saturating_duration_since(Instant::now())).ok()
}

/// The result of one status fetch (AU20).
enum Fetched {
    Ok(Box<SessionStatus>, u64),
    Offline,
    Suspended,
    Ended,
}

/// `GET …/status` within `cap`. `401` with `zygo_session_unknown` or `zygo_token_invalid` is `Ended`;
/// `zygo_session_suspended` is `Suspended`; every other failure is `Offline` (H§6.8). No error text is kept (H-C6).
fn fetch_status(root: &Path, cap: Duration) -> Fetched {
    let start = Instant::now();
    let Ok(client) = SessionClient::from_session_dir(root) else { return Fetched::Offline };
    match client.timeout(cap).status() {
        Ok(s) => Fetched::Ok(Box::new(s), u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
        Err(ClientError::Reply { status: 401, code, .. }) if code == "zygo_session_unknown" || code == "zygo_token_invalid" => {
            Fetched::Ended
        }
        Err(ClientError::Reply { code, .. }) if code == "zygo_session_suspended" => Fetched::Suspended,
        Err(_) => Fetched::Offline,
    }
}

/// The wait of a status fetch (AU4): with a status part 90 s old or less, `clamp(1.5 × latency, 1 s, left)`; with
/// none, an older one or no latency, all of `left`.
pub fn fetch_cap(status_age_s: Option<u64>, latency_ms: Option<u64>, left: Duration) -> Duration {
    match (status_age_s, latency_ms) {
        (Some(age), Some(ms)) if age <= STALE_SECS => {
            Duration::from_millis(ms.saturating_mul(3) / 2).max(Duration::from_secs(1)).min(left)
        }
        _ => left,
    }
}

/// The session's trimmed bytes: one indexed query on the store opened read-only (H§7.4). `None` when the store is
/// missing, busy or unreadable.
fn trimmed(usage_db: &Path, session: &str) -> Option<u64> {
    Store::open_read_only(usage_db).ok()?.bytes_trimmed(session).ok()
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A variable that is set and not empty.
fn var(env: &dyn Fn(&str) -> Option<OsString>, key: &str) -> Option<String> {
    env(key).filter(|v| !v.is_empty()).map(|v| v.to_string_lossy().into_owned())
}

/// Everything one render shows (H§7), within `deadline`.
///
/// Fresh on each call: the stdin object (at most 64 KiB, 200 ms; `stdin` moves into a detached thread), `view.json`,
/// `ZYGO_WORK` and `ZYGO_URL` through `env`, the trimmed count from `paths.usage_db` for the session of
/// `view.json`, the workspace phase from `paths.workspaces_dir()` (AU25, never cached), and the member name and the unread count (M§9a, never cached). From the cache `statusline.json` in `session_dir`, refreshed under the lease `statusline.lock` when a
/// part is due: the status part and the git part. The call writes the cache before it returns and nothing after.
/// It never fails: a source that gives no answer drops only its own segments (H§6.8, C6).
pub fn collect(
    stdin: Box<dyn Read + Send + 'static>,
    session_dir: &Path,
    paths: &Paths,
    env: &dyn Fn(&str) -> Option<OsString>,
    deadline: Instant,
) -> HudData {
    let seams = Seams::read(env);
    let left = |until: Instant| until.saturating_duration_since(Instant::now());
    let object = read_stdin(stdin, STDIN_WAIT.min(left(deadline)));
    if let Some(path) = &seams.dump {
        dump(path, &object, env);
    }
    let input = Input::of(&object);
    let now = seams.now.unwrap_or_else(unix_now);
    let view = SessionView::load(&SessionDir::new(session_dir.to_path_buf())).ok();
    let phase = super::phase::read(paths, env);
    let cwd: PathBuf = input.cwd.map(PathBuf::from).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
    let cwd_key = cwd.to_string_lossy().into_owned();
    let work_end = deadline.checked_sub(WRITE_RESERVE).unwrap_or(deadline);
    let member_name = view
        .as_ref()
        .and_then(|v| v.workspace.as_ref()?.member.name.clone())
        .filter(|n| zygo_core::ids::valid_member_name(n));

    let cache_path = session_dir.join(CACHE_FILE);
    let mut cache = load_cache(&cache_path).unwrap_or_default();
    // A `retry_at` further away than the longest backoff is a clock change, not a backoff.
    let backoff = now < cache.retry_at && cache.retry_at <= now.saturating_add(ENDED_RETRY_SECS);
    let status_due = part_due(cache.status.as_ref().map(|s| s.at), now) && !backoff;
    let git_here = cache.git.as_ref().is_some_and(|g| g.cwd == cwd_key);
    let git_due = !git_here || part_due(cache.git.as_ref().map(|g| g.at), now);
    let lease = if status_due || git_due { Lease::take(session_dir, now) } else { None };
    let holder = lease.is_some();

    // Worker 3: the trimmed count, never cached (AU4).
    let trimmed_until = work_end.min(Instant::now() + TRIM_CAP);
    let trimmed_rx = view.as_ref().map(|v| {
        let (db, session) = (paths.usage_db.clone(), v.session.clone());
        worker("trimmed", move || trimmed(&db, &session))
    });

    // Worker 2: git. A cwd change ignores the lease (M3).
    let git_run = (git_due && (holder || !git_here)).then(|| {
        let budget = seams.git_budget.unwrap_or(GIT_BUDGET).min(left(work_end));
        let (dir, delay, forced) = (cwd.clone(), seams.git_delay, seams.git_panic);
        let rx = worker("git", move || {
            if let Some(d) = delay {
                std::thread::sleep(d);
            }
            if forced {
                panic!("forced by ZYGO_TEST_GIT_PANIC");
            }
            hud_git(&dir, budget)
        });
        (rx, work_end.min(Instant::now() + budget + GIT_GRACE))
    });

    // Worker 1: the status fetch, by the holder of the lease only.
    let fetch = (holder && status_due).then(|| {
        let age = cache.status.as_ref().filter(|s| s.at <= now).map(|s| now - s.at);
        let cap = fetch_cap(age, cache.latency_ms, left(work_end));
        if cap.is_zero() {
            return None;
        }
        let (root, block, delay) = (session_dir.to_path_buf(), seams.block_fetch, seams.fetch_delay);
        let rx = worker("status", move || {
            if block {
                std::thread::sleep(Duration::from_secs(10));
            }
            if let Some(d) = delay {
                std::thread::sleep(d);
            }
            fetch_status(&root, cap)
        });
        Some((rx, Instant::now() + cap))
    });

    // The unread count (M§9a), on this thread while the workers run, so that it never shrinks their budgets.
    let unread = view.as_ref().and_then(|v| super::mail::unread(paths, v, work_end));

    let trimmed_bytes = trimmed_rx.and_then(|rx| wait(&rx, trimmed_until)).flatten();
    // A try that fails, times out or panics is a git part with no data.
    let git_new = git_run.map(|(rx, until)| GitPart::of(now, &cwd_key, wait(&rx, until).flatten()));
    let fetched = fetch.map(|f| f.and_then(|(rx, until)| wait(&rx, until)).unwrap_or(Fetched::Offline));

    match fetched {
        Some(Fetched::Ok(s, latency_ms)) => {
            cache.status = Some(StatusPart::of(&s, now));
            cache.outcome = Outcome::Ok;
            cache.retry_at = 0;
            cache.latency_ms = Some(latency_ms);
        }
        Some(Fetched::Offline) => {
            cache.outcome = Outcome::Offline;
            cache.retry_at = now.saturating_add(RETRY_SECS);
        }
        Some(Fetched::Suspended) => {
            cache.outcome = Outcome::Suspended;
            cache.retry_at = now.saturating_add(RETRY_SECS);
        }
        Some(Fetched::Ended) => {
            cache.outcome = Outcome::Ended;
            cache.retry_at = now.saturating_add(ENDED_RETRY_SECS);
            cache.status = None;
        }
        None => {}
    }
    // The git data of this render: the new try, else the cached part of this cwd.
    let git = git_new.clone().or_else(|| cache.git.clone().filter(|g| g.cwd == cwd_key)).unwrap_or_default();
    if holder {
        if git_new.is_some() {
            cache.git = git_new;
        }
        cache.at = now;
        if let Ok(bytes) = serde_json::to_vec(&cache) {
            write_atomic(&cache_path, &bytes);
        }
        sweep_tmp(session_dir);
    }
    drop(lease);

    HudData {
        status: cache.status.as_ref().map(StatusPart::to_status),
        stale: cache.status.as_ref().is_some_and(|s| now.saturating_sub(s.at) > STALE_SECS),
        outcome: cache.outcome,
        work: var(env, "ZYGO_WORK"),
        worktree: git.worktree.filter(|_| git.linked),
        ahead: git.ahead,
        behind: git.behind,
        branch: git.branch,
        profile: view.and_then(|v| v.profile),
        effort: input.effort,
        trimmed_bytes,
        phase,
        member_name,
        unread,
        team_host: var(env, "ZYGO_URL").and_then(|u| zygo_proxy::Url::parse(&u).ok()?.host_str().map(str::to_string)),
    }
}

// ---- width source log (P-HUD-1, PH4) ----

/// The source that gives the assumed width, and the width: `tty` (the terminal probe), `COLUMNS`, or `default`
/// (H§6.7). The rule is `terminal_width`'s; a source with a value out of range is skipped.
pub fn width_source(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> (&'static str, u16) {
    let probed = tty();
    let width = terminal_width(env, &|| probed);
    let columns = env("COLUMNS").and_then(|v| v.to_str()?.trim().parse::<u16>().ok());
    // A value out of range is never the width, so a source whose value is the width is the one that answered.
    let source = if probed == Some(width) {
        "tty"
    } else if columns == Some(width) {
        "COLUMNS"
    } else {
        "default"
    };
    (source, width)
}

/// Appends `statusline: width source <source>, width <n>` to the session log when the pair differs from the last
/// logged pair, which `statusline.width` holds. So the log has one line per change, not one per run.
pub fn log_width(dir: &SessionDir, source: &str, width: u16) {
    let state = format!("{source} {width}");
    let file = dir.root.join(WIDTH_FILE);
    if std::fs::read_to_string(&file).is_ok_and(|have| have == state) {
        return;
    }
    write_atomic(&file, state.as_bytes());
    dir.append_log(&format!("statusline: width source {source}, width {width}"));
}
