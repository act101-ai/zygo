//! Drain idle rule. Owner: node W9 (W§9.3 step 5, AU26).
use std::time::{Duration, SystemTime};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zygo_proxy::client::SessionStatus;

use crate::activity::Activity;

/// How long after the end of the last model request a member still counts as busy (AU26 rule 3).
pub const REQUEST_QUIET: Duration = Duration::from_secs(10);

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
    /// member (round 2, M1). A rule 3 failure, a status read error among them, gives `false`.
    Busy { reason: String, rule_skipped: bool },
    /// Rules 1 and 2 hold and rule 3 is skipped: the proxy status has no `requests_in_flight`, or no request is in
    /// flight and `last_request_end` is not RFC 3339 or is more than `REQUEST_QUIET` after the poll time. `down`
    /// counts the member as idle and prints the AU26 rule-skipped line.
    IdleRuleSkipped,
    /// `busy` is `true` while rules 2 and 3 hold and no tool runs (`tools_running == 0`): a turn with no `Stop` or
    /// `StopFailure`, as after Esc (PW7 7.2). A long tool call sends no model request, so it is `Busy`, never this.
    /// `rule_skipped` is `true` when rule 3 is skipped. Only `TurnOpen { rule_skipped: false }` counts toward the
    /// interrupted-turn 60 s of `down`; with `rule_skipped: true` the member waits until the drain timeout (AU26).
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

/// The idle rule of the drain (AU26, W§9.3 step 5) for one member at the poll time `now`. `activity` is the result
/// of `activity::read` and `status` the result of the proxy status call; each `Err` is the text of the failed read.
/// It holds no state and reads no clock or file: the same inputs give the same result. The 60 s count of an
/// interrupted turn (PW7 7.2) belongs to `down`. OpenCode needs no branch here (AU27): its idle event acts as `Stop`
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
