//! Drain idle rule. Owner: node W9 (W§9.3 step 5, AU26).
use std::io::Read;
use std::time::{Duration, SystemTime};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zygo_core::session_dir::SessionDir;
use zygo_proxy::client::SessionStatus;

use crate::activity::Activity;
use crate::state::{open_nofollow, Opened};

/// How long after the end of the last model request a member still counts as busy (AU26 rule 3).
pub const REQUEST_QUIET: Duration = Duration::from_secs(10);

/// The busy reason of a member whose merged settings set `disableAllHooks: true` (AU26 hooks off, FIX-24).
pub const HOOKS_OFF: &str = "hooks off: its --settings sets disableAllHooks";

/// The busy reason of a member whose start sends no member events, `launch.events_off` (AU26 events off, FIX-24).
pub const EVENTS_OFF: &str = "member events off: launched with --bare, --pure or attach";

/// AU26 hooks off (FIX-24): `true` only when the session folder's `settings.json` is a JSON object whose top-level
/// `disableAllHooks` is `true`. It reads the merged settings file that zygo wrote and the agent runs with (the only
/// source a launch can turn hooks off from, AU26). It never waits on a FIFO; on unix it never follows a link (on
/// other systems a link is followed, as `activity::read`). No size or time limit. Every other outcome is `false`.
pub fn hooks_off(dir: &SessionDir) -> bool {
    let Ok(Opened::File(mut f)) = open_nofollow(std::fs::OpenOptions::new().read(true), &dir.settings_json()) else {
        return false;
    };
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_err() {
        return false;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    v.get("disableAllHooks") == Some(&serde_json::Value::Bool(true))
}

/// The result of the idle rule for one member at one poll (AU26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Idle {
    /// All three rules hold, rule 3 checked: `busy` is `false`, nothing runs, and the proxy reports no request in
    /// flight and none that ended in the last `REQUEST_QUIET`.
    Idle,
    /// The member is busy. `reason` names the cause, in this order: an `activity.json` that cannot be read or parsed
    /// (`activity file unreadable: <error>`), running subagents (`subagents running: "<id>", "<id>"`, each id with
    /// Debug quoting, so an id that holds `, ` stays one id), then rule 3
    /// (`proxy status not read: <error>`, `model requests in flight: <n>`, `last model request ended <s> s ago`,
    /// `last model request ended after the poll time`), then a running tool (`tool running`: `tools_running > 0`
    /// whatever `busy` holds, W10 round 3: busy over idle). `down` prints it in its timeout report (W§9.3 step 6).
    /// `rule_skipped` is `true` when the status meets a skip case of rule 3, whatever decided `Busy` (an
    /// `activity.json` error or running subagents), so `down` prints the AU26 rule-skipped line also for a busy
    /// member (round 2, M1). A rule 3 failure, a status read error among them, gives `false`. `down` also gives `Busy`
    /// with [EVENTS_OFF] or [HOOKS_OFF] for a member whose agent sends no member events (FIX-24).
    Busy { reason: String, rule_skipped: bool },
    /// Rules 1 and 2 hold and rule 3 is skipped: the proxy status has no `requests_in_flight`, or no request is in
    /// flight and `last_request_end` is not RFC 3339 or is more than `REQUEST_QUIET` after the poll time. `down`
    /// counts the member as idle and prints the AU26 rule-skipped line.
    IdleRuleSkipped,
    /// `busy` is `true` while rules 2 and 3 hold and no tool runs (`tools_running == 0`): a turn with no `Stop` or
    /// `StopFailure`, as after Esc (PW7 7.2). A long tool call sends no model request, so it is `Busy`, never this.
    /// `down` counts the member as busy at every poll, until a `Stop` or `StopFailure`, the agent's exit or the drain
    /// timeout (AU26, FIX-20). `rule_skipped` is `true` when rule 3 is skipped; it only drives the AU26 rule-skipped
    /// line of `down`.
    TurnOpen { rule_skipped: bool },
}

/// The outcome of AU26 rule 3.
enum Rule3 {
    Held,
    Skipped,
    Failed(String),
}

/// AU26 rule 3 in its fixed order: a failed status read, no `requests_in_flight`, a request in flight, no request
/// ended yet, an end time that is not RFC 3339, then the end time against `now` and `REQUEST_QUIET`.
fn rule3(status: Result<&SessionStatus, &str>, now: SystemTime) -> Rule3 {
    let status = match status {
        Ok(s) => s,
        Err(e) => return Rule3::Failed(format!("proxy status not read: {e}")),
    };
    match status.requests_in_flight {
        None => return Rule3::Skipped,
        Some(n) if n > 0 => return Rule3::Failed(format!("model requests in flight: {n}")),
        Some(_) => {}
    }
    let Some(text) = status.last_request_end.as_deref() else {
        return Rule3::Held;
    };
    let Ok(end) = OffsetDateTime::parse(text, &Rfc3339) else {
        return Rule3::Skipped;
    };
    match now.duration_since(SystemTime::from(end)) {
        Ok(since) if since < REQUEST_QUIET => {
            Rule3::Failed(format!("last model request ended {} s ago", since.as_secs()))
        }
        Ok(_) => Rule3::Held,
        Err(ahead) if ahead.duration() <= REQUEST_QUIET => {
            Rule3::Failed("last model request ended after the poll time".to_string())
        }
        Err(_) => Rule3::Skipped,
    }
}

/// The rest of the AU26 rule 3 quiet window (M§7.3, FX-35): `Some(end + REQUEST_QUIET − now)` exactly when [`idle`]
/// gives `Busy` and the only failed rule is the rule 3 time test: `busy` is `false`, no subagent runs, no tool runs,
/// the proxy reports `requests_in_flight` 0 and an RFC 3339 `last_request_end` less than `REQUEST_QUIET` before `now`
/// or at most `REQUEST_QUIET` after it. Every other input gives `None`. Like [`idle`], it holds no state and reads no
/// clock or file.
pub fn quiet_left(activity: Result<&Activity, &str>, status: Result<&SessionStatus, &str>, now: SystemTime) -> Option<Duration> {
    let (Ok(a), Ok(s)) = (activity, status) else { return None };
    if a.busy || !a.running.is_empty() || a.tools_running > 0 || s.requests_in_flight != Some(0) {
        return None;
    }
    let end = SystemTime::from(OffsetDateTime::parse(s.last_request_end.as_deref()?, &Rfc3339).ok()?);
    match now.duration_since(end) {
        Ok(since) if since < REQUEST_QUIET => Some(REQUEST_QUIET - since),
        Ok(_) => None,
        Err(ahead) if ahead.duration() <= REQUEST_QUIET => Some(REQUEST_QUIET + ahead.duration()),
        Err(_) => None,
    }
}

/// The idle rule of the drain (AU26, W§9.3 step 5) for one member at the poll time `now`. `activity` is the result
/// of `activity::read` and `status` the result of the proxy status call; each `Err` is the text of the failed read.
/// It holds no state and reads no clock or file: the same inputs give the same result. OpenCode needs no branch here (AU27): its idle event acts as `Stop`
/// through the member hooks, and without it `busy` stays as the hooks wrote it, so `down` passes the activity as it
/// is.
pub fn idle(activity: Result<&Activity, &str>, status: Result<&SessionStatus, &str>, now: SystemTime) -> Idle {
    let rule3 = rule3(status, now);
    let rule_skipped = matches!(rule3, Rule3::Skipped);
    let activity = match activity {
        Ok(a) => a,
        Err(e) => return Idle::Busy { reason: format!("activity file unreadable: {e}"), rule_skipped },
    };
    if !activity.running.is_empty() {
        let ids: Vec<String> = activity.running.keys().map(|id| format!("{id:?}")).collect();
        return Idle::Busy { reason: format!("subagents running: {}", ids.join(", ")), rule_skipped };
    }
    if let Rule3::Failed(reason) = rule3 {
        return Idle::Busy { reason, rule_skipped: false };
    }
    if activity.tools_running > 0 {
        return Idle::Busy { reason: "tool running".to_string(), rule_skipped };
    }
    match (activity.busy, rule_skipped) {
        (true, _) => Idle::TurnOpen { rule_skipped },
        (false, false) => Idle::Idle,
        (false, true) => Idle::IdleRuleSkipped,
    }
}
