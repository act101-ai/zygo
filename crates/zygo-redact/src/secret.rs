//! The secret redaction pass (§16.1).

use crate::cache::{self, CacheKey, Entry, SessionCache};
use crate::patterns::{Compiled, Pat};
use crate::scope;
use serde_json::Value;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};
use zygo_config::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlinePolicy {
    /// 200 ms
    pub base: Duration,
    /// 50 ms
    pub per_mib: Duration,
    /// Test hook (§20 #30): time out before the (n+1)-th uncached scan of one call; the first scan always runs.
    pub expire_after_scans: Option<u64>,
}

impl Default for DeadlinePolicy {
    fn default() -> Self {
        DeadlinePolicy { base: Duration::from_millis(200), per_mib: Duration::from_millis(50), expire_after_scans: None }
    }
}

impl DeadlinePolicy {
    /// `base + per_mib * bytes / 2^20`, integer nanoseconds.
    pub fn budget(&self, uncached_bytes: u64) -> Duration {
        let extra = (self.per_mib.as_nanos() * u128::from(uncached_bytes)) >> 20;
        self.base + Duration::from_nanos(u64::try_from(extra).unwrap_or(u64::MAX))
    }
}

/// Redact ids only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Findings {
    pub counts: BTreeMap<String, u64>,
    /// True only when some string changed; when false, forward the original bytes (N12).
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RedactError {
    /// Sorted, deduped; ids only.
    #[error("zygo_redaction_block: {}", .ids.join(", "))]
    Blocked { ids: Vec<String> },
    #[error("zygo_redaction_unavailable")]
    Timeout,
}

thread_local! {
    static SCAN_CALLS: Cell<u64> = const { Cell::new(0) };
    #[cfg(feature = "test-hooks")]
    static LOOKUP_DELAY: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// Test hook: calls of the string scan on this thread.
#[doc(hidden)]
pub fn scan_calls() -> u64 {
    SCAN_CALLS.with(Cell::get)
}

/// Test hook: sleep `d` before the cache lookup of each call on this thread (stands in for hashing a large body).
#[cfg(feature = "test-hooks")]
#[doc(hidden)]
pub fn set_lookup_delay(d: Duration) {
    LOOKUP_DELAY.with(|x| x.set(d));
}

/// Redacts `body` in place; the deadline counts only strings this session has not scanned.
/// `Findings.changed` is false when no string changed; the caller then forwards the original bytes.
pub fn redact(body: &mut Value, c: &Compiled, cache: &mut SessionCache, policy: &DeadlinePolicy) -> Result<Findings, RedactError> {
    redact_after_wait(body, c, cache, policy, Duration::ZERO)
}

/// Redacts `body` in place, as `redact` does, and takes `waited` (the session-lock wait) off the whole budget,
/// stopping at zero. The deadline starts after the scope walk, the hashing and the cache lookup. Every call scans
/// at least one uncached string. So, from the stage's lock request to the return, a call takes at most
/// `waited` + walk, hashing and lookup (O(body size)) + `budget(uncached).saturating_sub(waited)`
/// + one whole-string scan: the first scan under a zero budget, or the scan that runs when the deadline passes.
pub fn redact_after_wait(
    body: &mut Value,
    c: &Compiled,
    cache: &mut SessionCache,
    policy: &DeadlinePolicy,
    waited: Duration,
) -> Result<Findings, RedactError> {
    let ex = scope::excluded(body);
    let mut keys = Vec::new();
    let mut lens = Vec::new();
    scope::for_each_str(body, &ex, &mut |s| {
        keys.push(cache::key(c.version(), s));
        lens.push(s.len() as u64);
    });
    #[cfg(feature = "test-hooks")]
    {
        let d = LOOKUP_DELAY.with(Cell::get);
        if !d.is_zero() {
            std::thread::sleep(d);
        }
    }
    // One lock: the hits are copied here, so eviction during the scans below cannot add unbudgeted work.
    let mut found: HashMap<CacheKey, Entry> = cache.get_many(&keys);
    let uncached: u64 = keys.iter().zip(&lens).filter(|(k, _)| !found.contains_key(*k)).map(|(_, n)| n).sum();
    // Starts after the cache lookup (§16.1 Bound note).
    let deadline = Instant::now() + policy.budget(uncached).saturating_sub(waited);
    let (mut i, mut scans, mut timed_out, mut changed) = (0usize, 0u64, false, false);
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut blocked: BTreeSet<String> = BTreeSet::new();
    scope::for_each_str_mut(body, &ex, &mut |s| {
        let k = keys[i];
        i += 1;
        if timed_out {
            return;
        }
        let e = match found.get(&k) {
            Some(e) => e.clone(),
            None => {
                // Every attempt scans at least one uncached string, so a retry makes progress.
                if scans > 0 && (policy.expire_after_scans.is_some_and(|n| scans >= n) || Instant::now() >= deadline) {
                    timed_out = true;
                    return;
                }
                let e = scan(s, c);
                cache.record_scan();
                scans += 1;
                cache.insert(k, e.clone());
                found.insert(k, e.clone());
                e
            }
        };
        for &(idx, n) in &e.findings {
            let p = &c.pats[usize::from(idx)];
            match p.action {
                Action::Block => {
                    blocked.insert(p.id.clone());
                }
                Action::Redact => *counts.entry(p.id.clone()).or_default() += u64::from(n),
            }
        }
        if let Some(r) = e.replacement {
            changed |= *s != r;
            *s = r;
        }
    });
    if timed_out {
        return Err(RedactError::Timeout);
    }
    if !blocked.is_empty() {
        return Err(RedactError::Blocked { ids: blocked.into_iter().collect() });
    }
    Ok(Findings { counts, changed })
}

/// One string's findings and replacement. Patterns run on the whole string; a span wholly inside
/// a `[REDACTED:<configured id>]` is dropped. Patterns also run on a copy with those placeholders
/// removed (`Stripped`), so a placeholder inside a secret does not split it, even inside a token.
/// Overlapping and touching spans merge into one, labelled with the earliest pattern in config
/// order (act-lite values first), so no part of a longer secret survives. A `redact` span is each
/// participating `secret*` group, else the whole match; a `block` match is always the whole match.
fn scan(s: &str, c: &Compiled) -> Entry {
    SCAN_CALLS.with(|n| n.set(n.get() + 1));
    let ph: Vec<(usize, usize)> = c.placeholder.find_iter(s).map(|m| (m.start(), m.end())).collect();
    let stripped = (!ph.is_empty()).then(|| Stripped::new(s, &ph));
    let matched = c.set.matches(s);
    let matched_t = stripped.as_ref().map(|t| c.set.matches(&t.text));
    if !matched.matched_any() && !matched_t.as_ref().is_some_and(|m| m.matched_any()) {
        return Entry { replacement: None, findings: vec![] };
    }
    let mut findings = Vec::new();
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    for (idx, p) in c.pats.iter().enumerate() {
        let mut found = if matched.matched(idx) { pattern_spans(p, s, &ph) } else { Vec::new() };
        if let (Some(t), Some(true)) = (&stripped, matched_t.as_ref().map(|m| m.matched(idx))) {
            found.extend(pattern_spans(p, &t.text, &[]).into_iter().map(|(a, b)| t.original(a, b)));
        }
        if p.action == Action::Block {
            if !found.is_empty() {
                findings.push((idx as u16, 1));
            }
            continue;
        }
        spans.extend(found.into_iter().map(|(a, b)| (a, b, idx)));
    }
    if spans.is_empty() {
        return Entry { replacement: None, findings };
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize, usize)> = Vec::new();
    for (a, b, idx) in spans {
        match merged.last_mut() {
            Some(m) if a <= m.1 => {
                m.1 = m.1.max(b);
                m.2 = m.2.min(idx);
            }
            _ => merged.push((a, b, idx)),
        }
    }
    let mut per: BTreeMap<usize, u32> = BTreeMap::new();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for &(a, b, idx) in &merged {
        out.push_str(&s[last..a]);
        out.push_str("[REDACTED:");
        out.push_str(&c.pats[idx].id);
        out.push(']');
        last = b;
        *per.entry(idx).or_default() += 1;
    }
    out.push_str(&s[last..]);
    findings.extend(per.into_iter().map(|(idx, n)| (idx as u16, n)));
    Entry { replacement: Some(out), findings }
}

/// `s` with its configured placeholders removed, and the map back to `s`.
///
/// Mapping: `cuts[i] = (t, n)` says that the `i`-th removed placeholder sat at offset `t` of `text`,
/// and that `n` bytes were removed up to and including it. A span `(a, b)` of `text` maps to
/// `(a + n_a, b + n_b)`, where `n_a` counts the placeholders at offsets `<= a` and `n_b` those at
/// offsets `< b`. So a placeholder strictly inside the span is covered by it and replaced whole,
/// and one at either edge stays outside. Building is O(|s|); each span maps in O(log placeholders).
struct Stripped {
    text: String,
    cuts: Vec<(usize, usize)>,
}

impl Stripped {
    fn new(s: &str, ph: &[(usize, usize)]) -> Stripped {
        let (mut text, mut cuts, mut last, mut removed) = (String::with_capacity(s.len()), Vec::with_capacity(ph.len()), 0, 0);
        for &(a, b) in ph {
            text.push_str(&s[last..a]);
            removed += b - a;
            cuts.push((text.len(), removed));
            last = b;
        }
        text.push_str(&s[last..]);
        Stripped { text, cuts }
    }

    fn original(&self, a: usize, b: usize) -> (usize, usize) {
        let shift = |i: usize| if i == 0 { 0 } else { self.cuts[i - 1].1 };
        (a + shift(self.cuts.partition_point(|c| c.0 <= a)), b + shift(self.cuts.partition_point(|c| c.0 < b)))
    }
}

/// `p`'s spans in `s` without the spans wholly inside a placeholder of `ph` (sorted). A `redact` match
/// gives each participating `secret*` group, else the whole match; a `block` match gives the whole match.
fn pattern_spans(p: &Pat, s: &str, ph: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let (mut out, mut pos, mut j) = (Vec::new(), 0, 0);
    let mut parts: Vec<(usize, usize)> = Vec::new();
    while pos <= s.len() {
        let Some(cap) = p.re.captures_at(s, pos) else { break };
        let m0 = cap.get(0).unwrap();
        parts.clear();
        if p.action == Action::Redact {
            parts.extend(p.secret_groups.iter().filter_map(|&g| cap.get(g)).map(|m| (m.start(), m.end())));
        }
        if parts.is_empty() {
            parts.push((m0.start(), m0.end()));
        }
        while j < ph.len() && ph[j].1 <= m0.start() {
            j += 1; // match starts never decrease, so `j` walks `ph` once
        }
        let mut end = m0.end();
        for &(a, b) in &parts {
            let k = j + ph[j..].iter().take_while(|x| x.1 <= a).count(); // placeholders inside this match only
            if k < ph.len() && ph[k].0 <= a && b <= ph[k].1 {
                end = end.min(ph[k].1); // dropped: resume at the placeholder's end at the latest
            } else if a < b {
                out.push((a, b));
            }
        }
        pos = if end > m0.start() { end } else { s[end..].chars().next().map_or(s.len() + 1, |ch| end + ch.len_utf8()) };
    }
    out.sort_unstable();
    out
}
