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
use zygo_usage::{RequestRow, Store};

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
    usage: Usage,
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
        let Some(d) = line.strip_prefix(b"data:") else { return };
        if !d.windows(5).any(|w| w == b"usage") {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(d.trim_ascii()) else { return };
        match v.get("type").and_then(Value::as_str) {
            Some("message_start") => self.set(&v["message"]["usage"]),
            Some("message_delta") => self.max(&v["usage"]),
            _ => {}
        }
    }
}

impl Tee {
    pub fn new(streamed: bool) -> Tee {
        Tee { streamed, line_mode: false, held: Vec::new(), over: false, copy: Vec::new(), copy_over: false, usage: Usage::default() }
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
            }
            self.copy = Vec::new();
            return Bytes::new();
        }
        let tail = std::mem::take(&mut self.held);
        if self.line_mode { Bytes::from(tail) } else { Bytes::new() }
    }

    pub fn usage(&self) -> Usage {
        self.usage
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
        self.usage.observe(line);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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

/// A row and its ticket, to the writer.
type RowSender = mpsc::Sender<(RequestRow, Option<Ticket>)>;

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
    /// `None`: no rows. A store that does not open: no rows and `USAGE_OFF_NOTE` (fail open).
    pub(crate) fn open(db: Option<&Path>, #[allow(unused_variables)] hooks: WriterHooks) -> (UsageSink, Option<String>) {
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
                let (tx, rx) = mpsc::channel::<(RequestRow, Option<Ticket>)>();
                let h = handled.clone();
                let writer = std::thread::Builder::new().name("zygo-usage".into()).spawn(move || {
                    // The proxy has no log: a failed insert drops the row.
                    for (row, ticket) in rx {
                        #[cfg(feature = "test-hooks")]
                        {
                            if let Some(h) = &hooks.hold {
                                h.wait();
                            }
                            assert!(!hooks.panic, "usage writer: test panic");
                        }
                        let _ = store.insert_request(&row);
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

    /// No store or closed: the row is counted and dropped (with its ticket).
    pub(crate) fn send(&self, r: RequestRow, t: Option<Ticket>) {
        let sent = lock(&self.tx).as_ref().is_some_and(|tx| tx.send((r, t)).is_ok());
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
