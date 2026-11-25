//! The secret pass on one message before it leaves the machine (S§8.6, S-G4).
use serde_json::Value;
use std::path::Path;
use zygo_config::schema::RedactionConfig;
use zygo_redact::act_lite_store::{read_with, store_path, Lookup};
use zygo_redact::cache::{CachePool, SessionCache, SESSION_CAP_BYTES};
use zygo_redact::patterns::compile_with_store;
use zygo_redact::secret::{redact, DeadlinePolicy, RedactError};

/// The result of the pass (S§8.6 "Three results"). No error text, store value or message text is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No pattern matched.
    Clear,
    /// The pattern ids that matched, sorted.
    Finding(Vec<String>),
    /// The pass could not run; the value is the check name.
    CouldNotRun(&'static str),
}

/// Runs the pass on `msg` with the act-lite store found through the process environment.
pub fn check(msg: &Value, redaction: &RedactionConfig, main_root: &Path, policy: &DeadlinePolicy) -> Outcome {
    check_with(msg, redaction, main_root, policy, &Lookup::system())
}

/// Runs the pass on a copy of `msg`; `lookup` finds the act-lite store. No store file gives regex patterns only.
pub fn check_with(msg: &Value, redaction: &RedactionConfig, main_root: &Path, policy: &DeadlinePolicy, lookup: &Lookup) -> Outcome {
    let stored = match read_with(main_root, lookup) {
        Ok(v) => v,
        Err(_) => match std::fs::symlink_metadata(store_path(main_root, lookup)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(), // no store: regex patterns only
            _ => return Outcome::CouldNotRun("act_lite_store"),
        },
    };
    let Ok(compiled) = compile_with_store(redaction, &stored) else { return Outcome::CouldNotRun("redaction_patterns") };
    let mut copy = msg.clone();
    let mut cache = SessionCache::new(CachePool::new(SESSION_CAP_BYTES, SESSION_CAP_BYTES)); // fresh per call
    match redact(&mut copy, &compiled, &mut cache, policy) {
        Ok(f) if !f.changed => Outcome::Clear,
        Ok(f) => Outcome::Finding(f.counts.into_keys().collect()),
        Err(RedactError::Blocked { ids }) => Outcome::Finding(ids),
        Err(RedactError::Timeout) => Outcome::CouldNotRun("secret_timeout"),
    }
}
