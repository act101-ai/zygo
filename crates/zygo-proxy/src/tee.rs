//! The response tee (§14): chunks pass to the client as they arrive; a copy is parsed for usage. Line mode (§10.2.6)
//! and the usage-row writer.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use serde_json::Value;
use zygo_config::keys::Secret;
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RequestRow, Store, UpstreamErrorRow};

use crate::redact_stage::secret_pass;
use crate::session::SessionState;
use crate::translate::{error_message, error_object, scrub};

pub const HOLD_MAX: usize = 4 << 20; // §10.2.6
pub const COPY_MAX: usize = 4 << 20; // §14
pub const USAGE_OFF_NOTE: &str = "usage rows off: the usage database did not open";
/// The rows reply's wait for a session's queued rows. Spec §7 note 2026-11-19 (owner: follow sensible standards).
pub const ROWS_WAIT: Duration = Duration::from_secs(25);

/// One SSE line without its "\n".
pub struct SseLine(pub Vec<u8>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// The cache-write tokens of one response by TTL (AU33): Anthropic `usage.cache_creation`. Parts of
/// `Usage::cache_write`, which stays the total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheSplit {
    /// `ephemeral_5m_input_tokens`.
    pub m5: u64,
    /// `ephemeral_1h_input_tokens`.
    pub h1: u64,
}

/// Line mode's output and line callback.
type Armed<'a> = (&'a mut Vec<u8>, &'a mut dyn FnMut(&mut SseLine));

pub struct Tee {
    streamed: bool,
    line_mode: bool,
    /// The partial line: held (armed) or scanned (unarmed).
    held: Vec<u8>,
    /// The current line is over HOLD_MAX: passed unchanged (armed) or skipped (unarmed) up to its newline.
    over: bool,
    /// Non-streamed copy (§14).
    copy: Vec<u8>,
    copy_over: bool,
    /// Keep the non-streamed copy after `finish`, for `body` (FIX-10).
    keep: bool,
    usage: Usage,
    /// `None` until a usage object carries a part of the split.
    split: Option<CacheSplit>,
}

/// The usage object of one SSE line, and whether it starts the numbers (`message_start` `message.usage`) or
/// continues them (`message_delta` `usage`) (§14).
fn line_usage(line: &[u8]) -> Option<(bool, Value)> {
    let d = line.strip_prefix(b"data:")?;
    if !d.windows(5).any(|w| w == b"usage") {
        return None;
    }
    let mut v = serde_json::from_slice::<Value>(d.trim_ascii()).ok()?;
    let (start, u) = match v.get("type").and_then(Value::as_str) {
        Some("message_start") => (true, v.get_mut("message").and_then(|m| m.get_mut("usage"))),
        Some("message_delta") => (false, v.get_mut("usage")),
        _ => return None,
    };
    u.map(|u| (start, u.take()))
}

impl CacheSplit {
    /// The parts `u.cache_creation` carries: as they are, or (`max`) when larger (`message_delta` usage is
    /// cumulative). A part it does not carry keeps its value; with no part, `cur` is unchanged.
    fn read(cur: &mut Option<CacheSplit>, u: &Value, max: bool) {
        let Some(c) = u.get("cache_creation") else { return };
        let (m5, h1) = (Usage::field(c, "ephemeral_5m_input_tokens"), Usage::field(c, "ephemeral_1h_input_tokens"));
        if m5.is_none() && h1.is_none() {
            return;
        }
        let old = cur.unwrap_or_default();
        let f = |new: Option<u64>, old: u64| new.map_or(old, |n| if max { n.max(old) } else { n });
        *cur = Some(CacheSplit { m5: f(m5, old.m5), h1: f(h1, old.h1) });
    }
}

impl Usage {
    fn field(v: &Value, k: &str) -> Option<u64> {
        v.get(k).and_then(Value::as_u64)
    }

    /// Every field the object carries.
    fn set(&mut self, u: &Value) {
        let f = |k: &str, cur: u64| Usage::field(u, k).unwrap_or(cur);
        *self = Usage {
            input: f("input_tokens", self.input),
            output: f("output_tokens", self.output),
            cache_read: f("cache_read_input_tokens", self.cache_read),
            cache_write: f("cache_creation_input_tokens", self.cache_write),
        };
    }

    /// Per-field maxima (`message_delta` usage is cumulative).
    fn max(&mut self, u: &Value) {
        let f = |k: &str, cur: u64| Usage::field(u, k).map_or(cur, |n| n.max(cur));
        *self = Usage {
            input: f("input_tokens", self.input),
            output: f("output_tokens", self.output),
            cache_read: f("cache_read_input_tokens", self.cache_read),
            cache_write: f("cache_creation_input_tokens", self.cache_write),
        };
    }

    /// One SSE line: Anthropic `message_start` `message.usage`, then per-field maxima of `message_delta` `usage` (§14).
    pub(crate) fn observe(&mut self, line: &[u8]) {
        if let Some((start, u)) = line_usage(line) {
            self.apply(start, &u);
        }
    }

    /// `set` for a `message_start` usage object, `max` for a `message_delta` one.
    fn apply(&mut self, start: bool, u: &Value) {
        if start {
            self.set(u);
        } else {
            self.max(u);
        }
    }
}

impl Tee {
    pub fn new(streamed: bool) -> Tee {
        Tee { streamed, line_mode: false, held: Vec::new(), over: false, copy: Vec::new(), copy_over: false, keep: false, usage: Usage::default(), split: None }
    }

    /// `body` returns the non-streamed copy after `finish` (FIX-10). No effect on a streamed response.
    pub fn keep_body(&mut self) {
        self.keep = true;
    }

    /// The non-streamed body as copied (at most COPY_MAX), when `keep_body` was called; `None` for a streamed body or
    /// a body over COPY_MAX.
    pub fn body(&self) -> Option<&[u8]> {
        (self.keep && !self.streamed && !self.copy_over).then_some(self.copy.as_slice())
    }

    /// No effect when the response is not streamed.
    pub fn arm_line_mode(&mut self) {
        self.line_mode = self.streamed;
    }

    pub fn line_mode(&self) -> bool {
        self.line_mode
    }

    /// Unarmed: `chunk` itself. Armed: the whole lines completed by `chunk`, each through `on_line`; a held partial
    /// line over HOLD_MAX is returned unchanged with the rest of that line and never reaches `on_line`.
    pub fn push(&mut self, chunk: Bytes, on_line: &mut dyn FnMut(&mut SseLine)) -> Bytes {
        if !self.streamed {
            if !self.copy_over {
                if self.copy.len() + chunk.len() > COPY_MAX {
                    self.copy_over = true;
                    self.copy = Vec::new();
                } else {
                    self.copy.extend_from_slice(&chunk);
                }
            }
            return chunk;
        }
        if !self.line_mode {
            self.scan(&chunk, None);
            return chunk;
        }
        let mut out = Vec::with_capacity(chunk.len() + self.held.len());
        self.scan(&chunk, Some((&mut out, on_line)));
        Bytes::from(out)
    }

    /// The held tail, unchanged.
    pub fn finish(&mut self, _on_line: &mut dyn FnMut(&mut SseLine)) -> Bytes {
        if !self.streamed {
            if !self.copy_over
                && let Ok(v) = serde_json::from_slice::<Value>(&self.copy)
                && let Some(u) = v.get("usage")
            {
                self.usage.set(u);
                CacheSplit::read(&mut self.split, u, false);
            }
            if !self.keep {
                self.copy = Vec::new();
            }
            return Bytes::new();
        }
        let tail = std::mem::take(&mut self.held);
        if self.line_mode { Bytes::from(tail) } else { Bytes::new() }
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// The 5-minute and 1-hour parts of the cache writes, when the upstream reports them (AU33).
    pub fn cache_split(&self) -> Option<CacheSplit> {
        self.split
    }

    /// Splits `data` into lines. Each whole line is read for usage, then (armed) passed through `on_line` and written
    /// to `out` with its "\n". A line over HOLD_MAX is not read: armed, it goes to `out` unchanged; unarmed, it is
    /// skipped.
    fn scan(&mut self, mut data: &[u8], mut armed: Option<Armed<'_>>) {
        while !data.is_empty() {
            let nl = data.iter().position(|&b| b == b'\n');
            let seg = &data[..nl.map_or(data.len(), |i| i + 1)];
            data = &data[seg.len()..];
            let whole = nl.is_some();
            if !self.over && self.held.len() + seg.len() - usize::from(whole) > HOLD_MAX {
                self.over = true;
                if let Some((out, _)) = armed.as_mut() {
                    out.extend_from_slice(&self.held);
                }
                self.held = Vec::new();
            }
            if self.over {
                if let Some((out, _)) = armed.as_mut() {
                    out.extend_from_slice(seg);
                }
                self.over = !whole;
                continue;
            }
            if !whole {
                self.held.extend_from_slice(seg);
                continue;
            }
            self.held.extend_from_slice(&seg[..seg.len() - 1]);
            let mut line = SseLine(std::mem::take(&mut self.held));
            self.read(&line.0);
            if let Some((out, on_line)) = armed.as_mut() {
                on_line(&mut line);
                out.extend_from_slice(&line.0);
                out.push(b'\n');
            }
            // Keeps the allocation for the next line.
            line.0.clear();
            self.held = line.0;
        }
    }

    fn read(&mut self, line: &[u8]) {
        if let Some((start, u)) = line_usage(line) {
            self.usage.apply(start, &u);
            CacheSplit::read(&mut self.split, &u, !start);
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The most bytes of the relayed body that `Flow::drop` copies for the usage writer (§14, FIX-10 B1).
pub(crate) const ERROR_BODY_MAX: usize = 16 << 10;
/// The most bytes of `error.type` and `error.message` that reach the scrub and the secret pass (FIX-10 B1, M7).
const ERROR_FIELD_IN_MAX: usize = 2 << 10;
/// The most characters stored in each field (§14, FIX-10).
const ERROR_TEXT_MAX: usize = 300;
/// The shortest run of the login that `scrub_partial` removes (FIX-10 M6).
const PARTIAL_MIN: usize = 8;

/// What the usage writer needs for a passthrough request's `upstream_errors` row (§14, FIX-10). Built on the response
/// path with no parse and no pass (B1).
pub(crate) struct ErrorCapture {
    /// At most ERROR_BODY_MAX bytes of the relayed body; empty for a streamed body or one over COPY_MAX.
    pub head: Vec<u8>,
    /// The client's login (the `Authorization` value after its scheme): removed from both fields, never stored.
    pub credential: Option<Secret>,
    pub session: Arc<SessionState>,
}

/// The longest prefix of `t` of at most `max` bytes that ends at a character boundary.
fn head(t: &str, max: usize) -> &str {
    let mut end = t.len().min(max);
    while !t.is_char_boundary(end) {
        end -= 1;
    }
    &t[..end]
}

/// Each control character as a space; at most ERROR_TEXT_MAX characters.
fn clean(t: &str) -> String {
    t.chars().map(|c| if c.is_control() { ' ' } else { c }).take(ERROR_TEXT_MAX).collect()
}

/// Each run of at least PARTIAL_MIN characters of `m` that is also a part of `cred` becomes the marker of
/// `translate::scrub` (M6: an upstream that echoes a part of the login). Runs on at most ERROR_FIELD_IN_MAX bytes.
fn scrub_partial(m: &str, cred: &str) -> String {
    let mark = scrub(cred, Some(cred));
    let c: Vec<char> = m.chars().collect();
    let (mut out, mut i) = (String::with_capacity(m.len()), 0);
    while i < c.len() {
        let (mut run, mut n) = (String::new(), 0);
        while i + n < c.len() {
            run.push(c[i + n]);
            if !cred.contains(run.as_str()) {
                break;
            }
            n += 1;
        }
        if n >= PARTIAL_MIN {
            out.push_str(&mark);
            i += n;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// One stored field (D2, D5): the 2 KiB cut, the login removed (whole, then each run), the secret pass under
/// `policy` (empty on a block, a timeout or a panic), then `clean`.
fn error_field(raw: &str, cred: Option<&str>, s: &SessionState, policy: &DeadlinePolicy) -> String {
    let mut t = scrub(head(raw, ERROR_FIELD_IN_MAX), cred);
    if let Some(c) = cred.filter(|c| !c.is_empty()) {
        t = scrub_partial(&t, c);
    }
    secret_pass(s, &t, policy).map(|t| clean(&t)).unwrap_or_default()
}

/// The `upstream_errors` row of `row` from its capture (D8): `error.type` and `error.message` of the head, each
/// through `error_field`; empty when the head is not such JSON.
fn error_row(row: &RequestRow, c: &ErrorCapture, policy: &DeadlinePolicy) -> UpstreamErrorRow {
    let v = serde_json::from_slice::<Value>(&c.head).ok();
    let cred = c.credential.as_ref().map(Secret::expose);
    let field = |x: Option<&str>| x.map(|x| error_field(x, cred, &c.session, policy)).unwrap_or_default();
    UpstreamErrorRow {
        ts: row.ts,
        session: row.session.clone(),
        request_class: row.request_class.clone(),
        model: row.model.clone(),
        status: row.status,
        error_type: field(v.as_ref().and_then(|v| error_object(v).get("type")).and_then(Value::as_str)),
        error_message: field(v.as_ref().and_then(error_message)),
    }
}

/// Rows begun but not yet inserted or dropped, per session.
type Queued = Arc<(Mutex<HashMap<String, u64>>, Condvar)>;

/// One row of `session` on its way to the store; dropping it (after the insert, or with the row) releases the count,
/// so a writer that dies leaves no count (S6).
pub(crate) struct Ticket {
    queued: Queued,
    session: String,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let (m, cv) = &*self.queued;
        let mut g = lock(m);
        if let Some(n) = g.get_mut(&self.session) {
            *n -= 1;
            if *n == 0 {
                g.remove(&self.session);
            }
        }
        cv.notify_all();
    }
}

/// Pauses the usage writer before each insert until `release` (the rows-wait seam).
#[cfg(feature = "test-hooks")]
pub struct UsageHold {
    open: Mutex<bool>,
    cv: Condvar,
}

#[cfg(feature = "test-hooks")]
impl UsageHold {
    /// Closed: the writer waits.
    pub fn new() -> Arc<UsageHold> {
        Arc::new(UsageHold { open: Mutex::new(false), cv: Condvar::new() })
    }
    pub fn release(&self) {
        *lock(&self.open) = true;
        self.cv.notify_all();
    }
    fn wait(&self) {
        let _g = self.cv.wait_while(lock(&self.open), |o| !*o).unwrap_or_else(|e| e.into_inner());
    }
}

/// The writer's test seams: a hold before each insert, a panic on the first row.
#[derive(Default)]
pub(crate) struct WriterHooks {
    #[cfg(feature = "test-hooks")]
    pub hold: Option<Arc<UsageHold>>,
    #[cfg(feature = "test-hooks")]
    pub panic: bool,
}

/// A row, its `upstream_errors` capture and its ticket, to the writer.
type RowSender = mpsc::Sender<(RequestRow, Option<ErrorCapture>, Option<Ticket>)>;

/// The usage-row writer (§14): one thread owns the store (`Store` is not `Sync`).
pub(crate) struct UsageSink {
    tx: Mutex<Option<RowSender>>,
    writer: Mutex<Option<JoinHandle<()>>>,
    /// Rows handled (inserted or dropped).
    handled: Arc<(Mutex<u64>, Condvar)>,
    queued: Queued,
}

fn count(h: &(Mutex<u64>, Condvar)) {
    *lock(&h.0) += 1;
    h.1.notify_all();
}

impl UsageSink {
    /// `None`: no rows. A store that does not open: no rows and `USAGE_OFF_NOTE` (fail open). `secret`: the deadline of
    /// the secret pass on the `upstream_errors` fields (FIX-10).
    pub(crate) fn open(db: Option<&Path>, #[allow(unused_variables)] hooks: WriterHooks, secret: DeadlinePolicy) -> (UsageSink, Option<String>) {
        let handled = Arc::new((Mutex::new(0), Condvar::new()));
        let queued: Queued = Arc::default();
        let store = match db.map(Store::open) {
            None => None,
            Some(Ok(s)) => Some(s),
            Some(Err(_)) => {
                let sink = UsageSink { tx: Mutex::new(None), writer: Mutex::new(None), handled, queued };
                return (sink, Some(USAGE_OFF_NOTE.to_string()));
            }
        };
        let (tx, writer) = match store {
            None => (None, None),
            Some(store) => {
                let (tx, rx) = mpsc::channel::<(RequestRow, Option<ErrorCapture>, Option<Ticket>)>();
                let h = handled.clone();
                let writer = std::thread::Builder::new().name("zygo-usage".into()).spawn(move || {
                    // The proxy has no log: a failed insert drops the row.
                    for (row, error, ticket) in rx {
                        #[cfg(feature = "test-hooks")]
                        {
                            if let Some(h) = &hooks.hold {
                                h.wait();
                            }
                            assert!(!hooks.panic, "usage writer: test panic");
                        }
                        // The error row is built here, on the writer thread, and written with its request row (D6).
                        let e = error.map(|c| error_row(&row, &c, &secret));
                        let _ = match &e {
                            Some(e) => store.insert_request_and_error(&row, e),
                            None => store.insert_request(&row),
                        };
                        drop(ticket);
                        count(&h);
                    }
                });
                match writer {
                    Ok(w) => (Some(tx), Some(w)),
                    Err(_) => {
                        let sink = UsageSink { tx: Mutex::new(None), writer: Mutex::new(None), handled, queued };
                        return (sink, Some(USAGE_OFF_NOTE.to_string()));
                    }
                }
            }
        };
        (UsageSink { tx: Mutex::new(tx), writer: Mutex::new(writer), handled, queued }, None)
    }

    /// A row of `session` is on its way: `flush` waits for it until the ticket is dropped.
    pub(crate) fn begin(&self, session: &str) -> Ticket {
        *lock(&self.queued.0).entry(session.to_string()).or_insert(0) += 1;
        Ticket { queued: self.queued.clone(), session: session.to_string() }
    }

    /// No store or closed: the row and its capture are counted and dropped (with the ticket). `e`: the request's
    /// `upstream_errors` capture (FIX-10).
    pub(crate) fn send(&self, r: RequestRow, e: Option<ErrorCapture>, t: Option<Ticket>) {
        let sent = lock(&self.tx).as_ref().is_some_and(|tx| tx.send((r, e, t)).is_ok());
        if !sent {
            count(&self.handled);
        }
    }

    /// Waits while `session` has rows begun and not yet inserted or dropped, at most `limit`; true when none is left.
    pub(crate) fn flush(&self, session: &str, limit: Duration) -> bool {
        let (m, cv) = &*self.queued;
        let g = cv.wait_timeout_while(lock(m), limit, |q| q.contains_key(session)).unwrap_or_else(|e| e.into_inner());
        !g.0.contains_key(session)
    }

    /// Drops the sender and joins the writer: every queued row is inserted or dropped.
    pub(crate) fn close(&self) {
        drop(lock(&self.tx).take());
        if let Some(w) = lock(&self.writer).take() {
            let _ = w.join();
        }
    }

    /// True once `n` rows were handled, false after `limit`.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn wait(&self, n: u64, limit: Duration) -> bool {
        let (m, cv) = &*self.handled;
        let g = cv.wait_timeout_while(lock(m), limit, |c| *c < n).unwrap_or_else(|e| e.into_inner());
        *g.0 >= n
    }
}
