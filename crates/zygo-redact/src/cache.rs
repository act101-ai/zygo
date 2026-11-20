//! The secret and content passes' per-session caches, on one process-wide LRU pool (§16.1).

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

pub type CacheKey = [u8; 16];
pub const SESSION_CAP_BYTES: usize = 64 << 20;
/// Conservative per-entry estimate of key, two LRU index nodes, map slot and Vec/String headers.
pub const ENTRY_OVERHEAD: usize = 256;

/// A scanned string's result. `findings` index into the pass's own id table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub replacement: Option<String>,
    pub findings: Vec<(u16, u32)>,
}

impl Entry {
    pub fn size(&self) -> usize {
        ENTRY_OVERHEAD + self.replacement.as_ref().map_or(0, String::len) + 8 * self.findings.len()
    }
}

/// The process-wide store behind every session cache (one LRU order across sessions and passes).
pub struct CachePool {
    inner: Mutex<Pool>,
}

/// Per session: its bytes and its entries by LRU tick.
type Sessions = HashMap<u64, (usize, BTreeMap<u64, CacheKey>)>;

struct Pool {
    total_cap: usize,
    session_cap: usize,
    total: usize,
    tick: u64,
    next_id: u64,
    map: HashMap<(u64, CacheKey), (Entry, u64)>,
    order: BTreeMap<u64, (u64, CacheKey)>,
    sessions: Sessions,
}

impl Pool {
    fn remove(&mut self, id: u64, k: &CacheKey) {
        if let Some((e, t)) = self.map.remove(&(id, *k)) {
            self.order.remove(&t);
            if let Some((bytes, ticks)) = self.sessions.get_mut(&id) {
                ticks.remove(&t);
                *bytes -= e.size();
            }
            self.total -= e.size();
        }
    }

    fn touch(&mut self, id: u64, k: &CacheKey) -> Option<Entry> {
        let (e, t) = self.map.get_mut(&(id, *k))?;
        let old = *t;
        self.tick += 1;
        *t = self.tick;
        let e = e.clone();
        self.order.remove(&old);
        self.order.insert(self.tick, (id, *k));
        if let Some((_, ticks)) = self.sessions.get_mut(&id) {
            ticks.remove(&old);
            ticks.insert(self.tick, *k);
        }
        Some(e)
    }

    fn insert(&mut self, id: u64, k: CacheKey, e: Entry) {
        self.remove(id, &k);
        let size = e.size();
        if size > self.session_cap {
            return;
        }
        self.tick += 1;
        let t = self.tick;
        self.map.insert((id, k), (e, t));
        self.order.insert(t, (id, k));
        let (bytes, ticks) = self.sessions.entry(id).or_default();
        *bytes += size;
        ticks.insert(t, k);
        self.total += size;
        while self.sessions[&id].0 > self.session_cap {
            let Some((_, &oldest)) = self.sessions[&id].1.first_key_value() else { break };
            self.remove(id, &oldest);
        }
        while self.total > self.total_cap {
            let Some((_, &(oid, ok))) = self.order.first_key_value() else { break };
            self.remove(oid, &ok);
        }
    }
}

impl CachePool {
    pub fn new(total_cap_bytes: usize, session_cap_bytes: usize) -> Arc<CachePool> {
        Arc::new(CachePool {
            inner: Mutex::new(Pool {
                total_cap: total_cap_bytes,
                session_cap: session_cap_bytes,
                total: 0,
                tick: 0,
                next_id: 0,
                map: HashMap::new(),
                order: BTreeMap::new(),
                sessions: HashMap::new(),
            }),
        })
    }

    pub fn used_bytes(&self) -> usize {
        self.lock().total
    }

    fn lock(&self) -> MutexGuard<'_, Pool> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// One per pass per session (the secret pass and N9's content pass each own one).
pub struct SessionCache {
    pool: Arc<CachePool>,
    id: u64,
    scans: u64,
}

impl SessionCache {
    pub fn new(pool: Arc<CachePool>) -> SessionCache {
        let id = {
            let mut p = pool.lock();
            p.next_id += 1;
            p.next_id
        };
        SessionCache { pool, id, scans: 0 }
    }
    /// No LRU bump.
    pub fn contains(&self, k: &CacheKey) -> bool {
        self.pool.lock().map.contains_key(&(self.id, *k))
    }
    /// Bumps LRU.
    pub fn get(&mut self, k: &CacheKey) -> Option<Entry> {
        self.pool.lock().touch(self.id, k)
    }
    /// Under one lock: copies of the entries found for `keys` (bumps LRU). A caller that budgets
    /// by these hits uses the copies, so a later eviction cannot turn a hit into a scan.
    pub fn get_many(&mut self, keys: &[CacheKey]) -> HashMap<CacheKey, Entry> {
        let mut p = self.pool.lock();
        keys.iter().filter_map(|k| p.touch(self.id, k).map(|e| (*k, e))).collect()
    }
    /// Replaces; evicts LRU over the session cap, then over the total cap.
    pub fn insert(&mut self, k: CacheKey, e: Entry) {
        self.pool.lock().insert(self.id, k, e)
    }
    pub fn record_scan(&mut self) {
        self.scans += 1;
    }
    /// Test hook (§20 #29a).
    pub fn scan_count(&self) -> u64 {
        self.scans
    }
}

impl Drop for SessionCache {
    fn drop(&mut self) {
        let mut p = self.pool.lock();
        if let Some((_, ticks)) = p.sessions.remove(&self.id) {
            for (t, k) in ticks {
                if let Some((e, _)) = p.map.remove(&(self.id, k)) {
                    p.total -= e.size();
                }
                p.order.remove(&t);
            }
        }
    }
}

/// First 16 bytes of SHA-256(version ‖ s).
pub fn key(version: &[u8; 16], s: &str) -> CacheKey {
    let mut h = Sha256::new();
    h.update(version);
    h.update(s.as_bytes());
    let mut k = [0u8; 16];
    k.copy_from_slice(&h.finalize()[..16]);
    k
}
