//! Stage 6 (§7.3): the redaction passes (§16). Secret pass, then content pass; the last stage before the body leaves.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use http::StatusCode;
use serde_json::Value;
use zygo_config::Dialect;
use zygo_redact::content::{admits, filter, Layout, Shape};
use zygo_redact::secret::{redact, redact_after_wait, RedactError};
use zygo_redact::DeadlinePolicy;

use crate::errors::ZygoError;
use crate::pipeline::{RequestCtx, Stage};
use crate::session::SessionState;

/// Deadlines of the two passes (§16.1: 200 ms + 50 ms/MiB; §16.2: 200 ms).
#[derive(Debug, Clone)]
pub struct RedactLimits {
    pub secret: DeadlinePolicy,
    pub content_budget: Duration,
    /// The next run of the pass it names panics once, while holding its cache lock; the other pass leaves it alone.
    #[cfg(feature = "test-hooks")]
    pub panic_once: Option<RedactPass>,
    /// Added to the measured lock wait of both passes (default zero): a deterministic over-long wait.
    #[cfg(feature = "test-hooks")]
    pub extra_wait: Duration,
}

impl Default for RedactLimits {
    fn default() -> Self {
        RedactLimits {
            secret: DeadlinePolicy::default(),
            content_budget: Duration::from_millis(200),
            #[cfg(feature = "test-hooks")]
            panic_once: None,
            #[cfg(feature = "test-hooks")]
            extra_wait: Duration::ZERO,
        }
    }
}

#[cfg(feature = "test-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactPass {
    Secret,
    Content,
}

pub struct RedactStage {
    limits: RedactLimits,
    #[cfg(feature = "test-hooks")]
    panic_once: Mutex<Option<RedactPass>>,
}

impl RedactStage {
    pub fn new() -> RedactStage {
        RedactStage {
            limits: RedactLimits::default(),
            #[cfg(feature = "test-hooks")]
            panic_once: Mutex::new(None),
        }
    }

    #[cfg(feature = "test-hooks")]
    pub fn with_limits(l: RedactLimits) -> RedactStage {
        RedactStage { panic_once: Mutex::new(l.panic_once), limits: l }
    }

    /// The test-only addition to the lock wait; zero in a build without test hooks.
    fn extra_wait(&self) -> Duration {
        #[cfg(feature = "test-hooks")]
        return self.limits.extra_wait;
        #[cfg(not(feature = "test-hooks"))]
        Duration::ZERO
    }

    /// Panics once when the hook names `pass`; the other pass leaves it alone.
    #[cfg(feature = "test-hooks")]
    fn hook(&self, pass: RedactPass) {
        let mut g = lock(&self.panic_once);
        if *g == Some(pass) {
            *g = None;
            panic!("redaction test hook");
        }
    }
}

/// A cache holds plain entries: a lock poisoned by a panic is used again.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `text` through the session's secret pass under `policy`; `None` on a `block` match, a timeout or a panic. Shared by
/// the usage row's model name (`forward::row_name`) and the `upstream_errors` fields (§14, FIX-10).
pub(crate) fn secret_pass(s: &SessionState, text: &str, policy: &DeadlinePolicy) -> Option<String> {
    let mut v = Value::String(text.to_string());
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut cache = lock(&s.redaction.secret_cache);
        redact(&mut v, &s.redaction.compiled, &mut cache, policy)
    }));
    match (r, v) {
        (Ok(Ok(_)), Value::String(t)) => Some(t),
        _ => None,
    }
}

fn unavailable(m: &'static str) -> ZygoError {
    ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_redaction_unavailable", m)
}

/// Adds `found` to `into` per id, `u64` → `u32` saturating.
fn add_counts(into: &mut BTreeMap<String, u32>, found: &BTreeMap<String, u64>) {
    for (id, n) in found {
        let e = into.entry(id.clone()).or_insert(0);
        *e = e.saturating_add(u32::try_from(*n).unwrap_or(u32::MAX));
    }
}

impl Default for RedactStage {
    fn default() -> Self {
        RedactStage::new()
    }
}

impl Stage for RedactStage {
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        let features = ctx.session.features();
        if features.redaction.secrets {
            let t0 = Instant::now();
            // The payload of a panic is dropped (C3).
            let r = catch_unwind(AssertUnwindSafe(|| {
                let red = &ctx.session.redaction;
                let mut cache = lock(&red.secret_cache);
                let waited = t0.elapsed() + self.extra_wait();
                #[cfg(feature = "test-hooks")]
                self.hook(RedactPass::Secret);
                // The lock wait comes off the whole budget (§16.1, C5).
                redact_after_wait(&mut ctx.body, &red.compiled, &mut cache, &self.limits.secret, waited)
            }));
            match r {
                Ok(Ok(f)) => {
                    add_counts(&mut ctx.redactions, &f.counts);
                    ctx.changed |= f.changed;
                    ctx.redacted |= !f.counts.is_empty();
                }
                Ok(Err(RedactError::Blocked { ids })) => {
                    for id in &ids {
                        ctx.redactions.insert(id.clone(), 1);
                    }
                    return Err(ZygoError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "zygo_redaction_block",
                        format!("blocked by redaction pattern: {}", ids.join(", ")),
                    ));
                }
                Ok(Err(RedactError::Timeout)) => return Err(unavailable("redaction did not finish in time")),
                Err(_) => return Err(unavailable("redaction failed")),
            }
        }
        // An absent class admits; a header that is not UTF-8 is a class that is not admitted.
        let class = ctx.headers.get("x-claude-code-request-class").map(|v| v.to_str().unwrap_or(""));
        if features.redaction.content && admits(class) {
            let t0 = Instant::now();
            let layout = if ctx.dialect == Dialect::Openai { Layout::Input } else { Layout::Messages };
            let r = catch_unwind(AssertUnwindSafe(|| {
                let red = &ctx.session.redaction;
                let mut cache = lock(&red.content_cache);
                let waited = t0.elapsed() + self.extra_wait();
                #[cfg(feature = "test-hooks")]
                self.hook(RedactPass::Content);
                let shape = Shape { layout, agent: ctx.session.agent };
                filter(&mut ctx.body, shape, &red.compiled, &mut cache, self.limits.content_budget.saturating_sub(waited))
            }));
            match r {
                Ok(f) => {
                    add_counts(&mut ctx.redactions, &f.counts);
                    ctx.changed |= f.changed;
                    // A timeout redacts nothing.
                    ctx.redacted |= f.counts.keys().any(|id| id != "content_timeout");
                }
                Err(_) => return Err(unavailable("redaction failed")),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // reading 4: u64 → u32 saturating, per value and per sum
    fn add_counts_saturates() {
        let m = |v: &[(&str, u64)]| v.iter().map(|(k, n)| (k.to_string(), *n)).collect::<BTreeMap<String, u64>>();
        let mut into = BTreeMap::from([("c".to_string(), u32::MAX - 1), ("d".to_string(), 2)]);
        add_counts(&mut into, &m(&[("a", u64::MAX), ("b", u64::from(u32::MAX) + 1), ("c", 5), ("d", 3)]));
        let want = BTreeMap::from([("a".to_string(), u32::MAX), ("b".to_string(), u32::MAX), ("c".to_string(), u32::MAX), ("d".to_string(), 5)]);
        assert_eq!(into, want);
    }
}
