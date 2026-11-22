//! The drain idle rule (AU26, W§9.3 step 5, W§18 #83; PW7 7.2).
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use zygo_core::session_dir::SessionDir;
use zygo_proxy::client::SessionStatus;
use zygo_workspace::activity::{self, Activity};
use zygo_workspace::drain::{self, Idle, REQUEST_QUIET};

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn ago(ms: u64) -> String {
    OffsetDateTime::from(now() - Duration::from_millis(ms)).format(&Rfc3339).unwrap()
}

fn ahead(ms: u64) -> String {
    OffsetDateTime::from(now() + Duration::from_millis(ms)).format(&Rfc3339).unwrap()
}

fn st(in_flight: Option<u64>, end: Option<String>) -> SessionStatus {
    SessionStatus { requests_in_flight: in_flight, last_request_end: end, ..Default::default() }
}

fn quiet() -> SessionStatus {
    st(Some(0), Some(ago(11_000)))
}

fn act(busy: bool, running: &[(&str, &str)]) -> Activity {
    Activity {
        busy,
        running: running.iter().map(|(i, t)| (i.to_string(), t.to_string())).collect::<BTreeMap<_, _>>(),
        ..Default::default()
    }
}

fn idle(a: &Activity, s: &SessionStatus) -> Idle {
    drain::idle(Ok(a), Ok(s), now())
}

fn busy(reason: &str) -> Idle {
    Idle::Busy { reason: reason.to_string() }
}

#[test]
fn all_three_rules_hold_is_idle() {
    assert_eq!(REQUEST_QUIET, Duration::from_secs(10));
    assert_eq!(idle(&act(false, &[]), &quiet()), Idle::Idle);
}

#[test]
fn rule_1_alone_is_a_turn_open() {
    assert_eq!(idle(&act(true, &[]), &quiet()), Idle::TurnOpen { rule_skipped: false });
}

#[test]
fn rule_2_alone_is_busy_with_the_running_ids() {
    assert_eq!(idle(&act(false, &[("b2", "pw-sub"), ("a1", "")]), &quiet()), busy("subagents running: a1, b2"));
}

#[test]
fn rule_3_alone_in_flight_is_busy() {
    assert_eq!(idle(&act(false, &[]), &st(Some(1), Some(ago(60_000)))), busy("model requests in flight: 1"));
}

#[test]
fn rule_3_alone_recent_end_is_busy() {
    assert_eq!(idle(&act(false, &[]), &st(Some(0), Some(ago(3_000)))), busy("last model request ended 3 s ago"));
}

#[test]
fn rule_2_is_reported_before_rule_3() {
    assert_eq!(idle(&act(false, &[("a1", "t")]), &st(Some(2), None)), busy("subagents running: a1"));
}

#[test]
fn no_request_ended_yet_holds_rule_3() {
    assert_eq!(idle(&act(false, &[]), &st(Some(0), None)), Idle::Idle);
}

#[test]
fn ten_second_window_edges() {
    let a = act(false, &[]);
    assert_eq!(idle(&a, &st(Some(0), Some(ago(10_000)))), Idle::Idle);
    assert_eq!(idle(&a, &st(Some(0), Some(ago(9_999)))), busy("last model request ended 9 s ago"));
}

#[test]
fn a_request_in_flight_is_busy_whatever_the_end_time() {
    let a = act(false, &[]);
    assert_eq!(idle(&a, &st(Some(2), Some("yesterday".into()))), busy("model requests in flight: 2"));
    assert_eq!(idle(&a, &st(Some(2), Some(ahead(60_000)))), busy("model requests in flight: 2"));
    assert_eq!(idle(&a, &st(Some(2), Some(ago(60_000)))), busy("model requests in flight: 2"));
}

#[test]
fn an_end_up_to_10_s_after_the_poll_time_just_ended() {
    let a = act(false, &[]);
    assert_eq!(idle(&a, &st(Some(0), Some(ahead(5_000)))), busy("last model request ended after the poll time"));
    assert_eq!(idle(&a, &st(Some(0), Some(ahead(10_000)))), busy("last model request ended after the poll time"));
}

#[test]
fn an_end_more_than_10_s_after_the_poll_time_skips_rule_3() {
    assert_eq!(idle(&act(false, &[]), &st(Some(0), Some(ahead(10_001)))), Idle::IdleRuleSkipped);
}

#[test]
fn rule_3_skipped_when_requests_in_flight_is_absent() {
    assert_eq!(idle(&act(false, &[]), &SessionStatus::default()), Idle::IdleRuleSkipped);
    assert_eq!(idle(&act(false, &[]), &st(None, Some(ago(1_000)))), Idle::IdleRuleSkipped);
}

#[test]
fn rule_3_skipped_when_the_end_time_is_not_rfc3339_and_nothing_is_in_flight() {
    assert_eq!(idle(&act(false, &[]), &st(Some(0), Some("yesterday".into()))), Idle::IdleRuleSkipped);
}

#[test]
fn skipped_rule_3_does_not_hide_rule_2() {
    assert_eq!(idle(&act(false, &[("a1", "t")]), &SessionStatus::default()), busy("subagents running: a1"));
}

#[test]
fn a_proxy_status_read_error_is_busy_not_a_skip() {
    assert_eq!(
        drain::idle(Ok(&act(false, &[])), Err("connection refused"), now()),
        busy("proxy status not read: connection refused")
    );
    assert_eq!(
        drain::idle(Ok(&act(true, &[])), Err("connection refused"), now()),
        busy("proxy status not read: connection refused")
    );
}

#[test]
fn an_activity_read_error_is_busy_and_comes_first() {
    assert_eq!(drain::idle(Err("x: bad"), Ok(&quiet()), now()), busy("activity file unreadable: x: bad"));
    assert_eq!(drain::idle(Err("x: bad"), Err("refused"), now()), busy("activity file unreadable: x: bad"));
}

#[test]
fn rule_2_is_reported_before_a_status_read_error() {
    assert_eq!(drain::idle(Ok(&act(false, &[("a1", "t")])), Err("refused"), now()), busy("subagents running: a1"));
}

#[test]
fn turn_open_with_rule_3_skipped_says_so() {
    assert_eq!(idle(&act(true, &[]), &SessionStatus::default()), Idle::TurnOpen { rule_skipped: true });
    assert_eq!(idle(&act(true, &[]), &st(Some(0), Some(ahead(60_000)))), Idle::TurnOpen { rule_skipped: true });
}

#[test]
fn busy_true_with_a_request_in_flight_is_busy() {
    assert_eq!(idle(&act(true, &[]), &st(Some(1), None)), busy("model requests in flight: 1"));
}

#[test]
fn busy_true_with_a_running_entry_is_busy() {
    assert_eq!(idle(&act(true, &[("agent-id-1", "pw-sub")]), &quiet()), busy("subagents running: agent-id-1"));
}

#[test]
fn busy_true_with_a_recent_end_is_busy() {
    assert_eq!(idle(&act(true, &[]), &st(Some(0), Some(ago(2_500)))), busy("last model request ended 2 s ago"));
}

#[test]
fn idle_has_no_clock_state() {
    let (a, s) = (act(true, &[]), quiet());
    assert_eq!(drain::idle(Ok(&a), Ok(&s), now()), drain::idle(Ok(&a), Ok(&s), now()));
    assert_eq!(drain::idle(Ok(&a), Ok(&s), now() + Duration::from_secs(120)), Idle::TurnOpen { rule_skipped: false });
}

#[test]
fn missing_activity_file_reads_as_idle() {
    let t = TempDir::new().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    assert_eq!(idle(&activity::read(&d).unwrap(), &quiet()), Idle::Idle);
}

#[cfg(unix)]
#[test]
fn unreadable_activity_file_reads_as_busy() {
    let t = TempDir::new().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    std::fs::create_dir(d.activity_json()).unwrap();
    let e = format!("{:#}", activity::read(&d).unwrap_err());
    match drain::idle(Err(&e), Ok(&quiet()), now()) {
        Idle::Busy { reason } => {
            assert!(reason.starts_with("activity file unreadable: ") && reason.contains("activity.json"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}
