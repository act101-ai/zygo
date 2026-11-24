//! The drain idle rule (AU26, W§9.3 step 5, W§18 #83; PW7 7.2).
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use zygo_core::session_dir::SessionDir;
use zygo_proxy::client::SessionStatus;
use zygo_workspace::activity::{self, Activity};
use zygo_workspace::drain::{self, Idle, EVENTS_OFF, HOOKS_OFF, REQUEST_QUIET};

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
    Idle::Busy { reason: reason.to_string(), rule_skipped: false }
}

fn busy_skipped(reason: &str) -> Idle {
    Idle::Busy { reason: reason.to_string(), rule_skipped: true }
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

fn tools(busy: bool, n: u32) -> Activity { Activity { tools_running: n, ..act(busy, &[]) } }

#[test] // W10 round 2 item 1: a long tool call (70 s and more, no model request) is never a turn open
fn a_running_tool_is_busy_not_a_turn_open() {
    assert_eq!(idle(&tools(true, 1), &quiet()), busy("tool running"));
    assert_eq!(idle(&tools(true, 2), &st(Some(0), None)), busy("tool running"));
    // A skipped rule 3 is still reported, and a request in flight comes first.
    assert_eq!(idle(&tools(true, 1), &st(None, None)), busy_skipped("tool running"));
    assert_eq!(idle(&tools(true, 1), &st(Some(1), None)), busy("model requests in flight: 1"));
    // Running subagents come before a running tool.
    let mut a = tools(true, 1);
    a.running.insert("a1".into(), "Plan".into());
    assert_eq!(idle(&a, &quiet()), busy("subagents running: \"a1\""));
}

#[test] // W10 round 3 B1: busy over idle: a running tool is busy whatever `busy` holds (never lose work)
fn busy_false_with_a_running_tool_is_busy() {
    assert_eq!(idle(&tools(false, 1), &quiet()), busy("tool running"));
    assert_eq!(idle(&tools(false, 1), &SessionStatus::default()), busy_skipped("tool running"));
    assert_eq!(idle(&tools(false, 0), &quiet()), Idle::Idle);
}

#[test] // W10 round 3 B1: an Esc during a tool fires no hook (P-N24-1); the count holds at every poll time
fn an_escaped_tool_is_busy_at_every_poll() {
    let a = tools(true, 1);
    for secs in [0, 61, 600, 86_400] {
        assert_eq!(drain::idle(Ok(&a), Ok(&quiet()), now() + Duration::from_secs(secs)), busy("tool running"));
    }
}

#[test] // FIX-20: an Esc-interrupted turn with no running tool is a turn open; down keeps it busy until a Stop or the drain timeout
fn an_interrupted_turn_with_no_running_tool_is_a_turn_open() {
    assert_eq!(idle(&tools(true, 0), &quiet()), Idle::TurnOpen { rule_skipped: false });
    assert_eq!(idle(&tools(true, 0), &st(None, None)), Idle::TurnOpen { rule_skipped: true });
}

#[test] // FIX-20: no quiet time makes an open turn idle; `drain::idle` holds no clock state
fn an_open_turn_is_a_turn_open_at_every_poll() {
    let (a, s) = (tools(true, 0), quiet());
    for secs in [0, 61, 600, 86_400] {
        let at = now() + Duration::from_secs(secs);
        assert_eq!(drain::idle(Ok(&a), Ok(&s), at), Idle::TurnOpen { rule_skipped: false }, "{secs} s");
    }
}

#[test]
fn rule_2_alone_is_busy_with_the_running_ids() {
    assert_eq!(idle(&act(false, &[("b2", "pw-sub"), ("a1", "")]), &quiet()), busy(r#"subagents running: "a1", "b2""#));
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
    assert_eq!(idle(&act(false, &[("a1", "t")]), &st(Some(2), None)), busy(r#"subagents running: "a1""#));
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
    assert_eq!(idle(&act(false, &[("a1", "t")]), &SessionStatus::default()), busy_skipped(r#"subagents running: "a1""#));
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
    assert_eq!(drain::idle(Ok(&act(false, &[("a1", "t")])), Err("refused"), now()), busy(r#"subagents running: "a1""#));
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
    assert_eq!(idle(&act(true, &[("agent-id-1", "pw-sub")]), &quiet()), busy(r#"subagents running: "agent-id-1""#));
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
        Idle::Busy { reason, rule_skipped: false } => {
            assert!(reason.starts_with("activity file unreadable: ") && reason.contains("activity.json"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

// FIX-22: W-C1 (C6), AU26. A FIFO, a link and a dangling link at activity.json make the member busy, and the read
// that the drain poll makes returns at once.
#[cfg(unix)]
#[test]
fn fifo_and_linked_activity_files_are_busy_without_a_wait() {
    use std::os::unix::ffi::OsStrExt;
    // Under /tmp, not TMPDIR: a long TMPDIR makes the socket path longer than `sun_path` (review round 2).
    let t = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
    std::fs::write(t.path().join("outside.json"), br#"{"version":1,"busy":false,"running":{},"updated_at":1}"#).unwrap();
    // The bound sockets stay open until the end of the test (FIX-22 review round 1, F1).
    let mut sockets = Vec::new();
    for case in ["fifo", "link", "dangling link", "socket"] {
        let d = SessionDir::new(t.path().join(case.replace(' ', "-")));
        std::fs::create_dir(&d.root).unwrap();
        let act = d.activity_json();
        match case {
            "fifo" => {
                let c = std::ffi::CString::new(act.as_os_str().as_bytes()).unwrap();
                // SAFETY: `c` is a valid NUL-terminated path.
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            }
            "link" => std::os::unix::fs::symlink(t.path().join("outside.json"), &act).unwrap(),
            "socket" => sockets.push(std::os::unix::net::UnixListener::bind(&act).unwrap()),
            _ => std::os::unix::fs::symlink(t.path().join("missing.json"), &act).unwrap(),
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let d2 = d.clone();
        std::thread::spawn(move || {
            let read = activity::read(&d2).map_err(|e| format!("{e:#}"));
            let _ = tx.send(drain::idle(read.as_ref().map_err(String::as_str), Ok(&quiet()), now()));
        });
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("{case}: the drain read waited"));
        let why = if case.ends_with("link") { "is a link" } else { "is not a regular file" };
        assert_eq!(
            got,
            Idle::Busy { reason: format!("activity file unreadable: {}: {why}", act.display()), rule_skipped: false },
            "{case}"
        );
    }
}

// H1 attempt-2 review round 1, F1: an `activity.json` over the read limit or one that does not parse is busy
// (AU26), and an `update` keeps it, so it stays busy.
#[test]
fn over_limit_and_unparsable_activity_files_stay_busy() {
    let t = TempDir::new().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let mut big = br#"{"version":1,"busy":false,"running":{},"updated_at":1}"#.to_vec();
    big.resize(2 << 20, b' ');
    for text in [big, b"{not json".to_vec()] {
        std::fs::write(d.activity_json(), &text).unwrap();
        for _ in 0..2 {
            let e = format!("{:#}", activity::read(&d).unwrap_err());
            match drain::idle(Err(&e), Ok(&quiet()), now()) {
                Idle::Busy { reason, rule_skipped: false } => {
                    assert!(reason.starts_with("activity file unreadable: ") && reason.contains("activity.json"), "{reason}")
                }
                other => panic!("{other:?}"),
            }
            assert!(activity::update(&d, Duration::from_secs(10), &mut |a| a.tools_running += 1).is_err());
        }
        assert_eq!(std::fs::read(d.activity_json()).unwrap(), text);
    }
}

// Round 2, M1: a skipped rule 3 is reported whatever decides `Busy`.
#[test]
fn busy_from_rule_2_reports_a_skipped_rule_3() {
    assert_eq!(idle(&act(true, &[("a1", "t")]), &SessionStatus::default()), busy_skipped(r#"subagents running: "a1""#));
    assert_eq!(idle(&act(false, &[("a1", "t")]), &st(Some(0), Some("yesterday".into()))), busy_skipped(r#"subagents running: "a1""#));
}

#[test]
fn busy_from_an_activity_error_reports_a_skipped_rule_3() {
    assert_eq!(drain::idle(Err("x: bad"), Ok(&SessionStatus::default()), now()), busy_skipped("activity file unreadable: x: bad"));
}

// Round 2, M2: ids in the reason are quoted, so an id with ", " stays one id.
#[test]
fn running_ids_are_quoted_in_the_reason() {
    assert_eq!(idle(&act(false, &[("b, c", "t"), ("a1", "t")]), &quiet()), busy(r#"subagents running: "a1", "b, c""#));
}

// FIX-24 D3, AU26 hooks off: only a top-level `disableAllHooks: true` in the merged settings file counts.
#[test]
fn hooks_off_reads_only_a_top_level_true() {
    let t = TempDir::new().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let cases = [
        (r#"{"disableAllHooks": true}"#, true),
        (r#"{"disableAllHooks": false}"#, false),
        (r#"{"hooks": {}}"#, false),
        ("not json", false),
        ("[true]", false),
        (r#"{"disableAllHooks": "true"}"#, false),
        (r#"{"env": {"disableAllHooks": true}}"#, false),
    ];
    for (text, want) in cases {
        std::fs::write(d.settings_json(), text).unwrap();
        assert_eq!(drain::hooks_off(&d), want, "{text}");
    }
    std::fs::remove_file(d.settings_json()).unwrap();
    assert!(!drain::hooks_off(&d), "a missing file");
}

// FIX-24 D3: a link at settings.json is not followed, and a FIFO there gives an answer at once.
#[cfg(unix)]
#[test]
fn hooks_off_never_follows_a_link_or_waits_on_a_fifo() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::FileTypeExt;
    let t = TempDir::new().unwrap();
    let outside = t.path().join("outside.json");
    let body = br#"{"disableAllHooks": true}"#;
    std::fs::write(&outside, body).unwrap();
    for case in ["link", "fifo"] {
        let d = SessionDir::new(t.path().join(case));
        std::fs::create_dir(&d.root).unwrap();
        let path = d.settings_json();
        match case {
            "link" => std::os::unix::fs::symlink(&outside, &path).unwrap(),
            _ => {
                let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                // SAFETY: `c` is a valid NUL-terminated path.
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let d2 = d.clone();
        std::thread::spawn(move || {
            let _ = tx.send(drain::hooks_off(&d2));
        });
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("{case}: the hooks_off read waited"));
        assert!(!got, "{case}");
        match case {
            "link" => assert_eq!(std::fs::read(&outside).unwrap(), body, "the link target is unchanged"),
            _ => assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_fifo(), "the FIFO stays"),
        }
    }
}

#[test] // FIX-24: the busy reasons are the texts of AU26 and W§9.3 step 6
fn reason_texts_are_the_spec_texts() {
    assert_eq!(HOOKS_OFF, "hooks off: its --settings sets disableAllHooks");
    assert_eq!(EVENTS_OFF, "member events off: launched with --bare, --pure or attach");
}
