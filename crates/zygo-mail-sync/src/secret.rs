//! The secret pass on one message before it leaves the machine (S§8.6, S-G4).
use serde_json::Value;
use std::path::Path;
use std::collections::BTreeSet;
use zygo_config::schema::{Action, RedactionConfig};
use zygo_redact::act_lite_store::{read_with, store_path, Lookup};
use zygo_redact::cache::{CachePool, SessionCache, SESSION_CAP_BYTES};
use zygo_redact::patterns::{compile_with_store, Compiled};
use zygo_redact::secret::{redact, DeadlinePolicy, Findings, RedactError};

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

/// Runs the pass on every key and string of `msg`; `lookup` finds the act-lite store. No store file gives regex
/// patterns only; the caller's value is never changed.
pub fn check_with(msg: &Value, redaction: &RedactionConfig, main_root: &Path, policy: &DeadlinePolicy, lookup: &Lookup) -> Outcome {
    let stored = match read_with(main_root, lookup) {
        Ok(v) => v,
        Err(_) => match std::fs::symlink_metadata(store_path(main_root, lookup)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(), // no store: regex patterns only
            _ => return Outcome::CouldNotRun("act_lite_store"),
        },
    };
    let Ok(compiled) = compile_with_store(redaction, &stored) else {
        let store_caused = !stored.is_empty() && compile_with_store(redaction, &[]).is_ok();
        return Outcome::CouldNotRun(if store_caused { "act_lite_store" } else { "redaction_patterns" });
    };
    let mut strings = Vec::new();
    flatten(msg, &mut strings);
    let flat = Value::Array(strings);
    match run(&flat, &compiled, policy) {
        // Any match is a finding, even one whose replacement equals the original text (`changed` is false).
        Ok(f) => match matched_ids(f) {
            ids if ids.is_empty() => Outcome::Clear,
            ids => Outcome::Finding(ids),
        },
        Err(RedactError::Blocked { ids }) => {
            // `Blocked` names only block ids; a second pass with every pattern as `redact` names the rest.
            let mut all: BTreeSet<String> = ids.into_iter().collect();
            let mut as_redact = redaction.clone();
            for p in &mut as_redact.pattern {
                p.action = Action::Redact;
            }
            // A failed or timed-out second pass still leaves the block ids: a finding stays a finding.
            if let Ok(c) = compile_with_store(&as_redact, &stored) {
                if let Ok(f) = run(&flat, &c, policy) {
                    all.extend(matched_ids(f));
                }
            }
            Outcome::Finding(all.into_iter().collect())
        }
        Err(RedactError::Timeout) => Outcome::CouldNotRun("secret_timeout"),
    }
}

/// Every object key and every string leaf of `v`, at any depth, as one flat list. A root array's paths are
/// never excluded by zygo-redact's API-body scope, and its keys are never skipped, so all of the message is scanned.
fn flatten(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::String(s) => out.push(Value::String(s.clone())),
        Value::Array(a) => a.iter().for_each(|x| flatten(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                out.push(Value::String(k.clone()));
                flatten(x, out);
            }
        }
        _ => {}
    }
}

/// The ids with a match count above zero, sorted (`counts` is a `BTreeMap`).
fn matched_ids(f: Findings) -> Vec<String> {
    f.counts.into_iter().filter(|(_, n)| *n > 0).map(|(id, _)| id).collect()
}

/// One redact run on a copy of `flat` with a fresh cache.
fn run(flat: &Value, compiled: &Compiled, policy: &DeadlinePolicy) -> Result<Findings, RedactError> {
    let mut copy = flat.clone();
    let mut cache = SessionCache::new(CachePool::new(SESSION_CAP_BYTES, SESSION_CAP_BYTES)); // fresh per call
    redact(&mut copy, compiled, &mut cache, policy)
}
