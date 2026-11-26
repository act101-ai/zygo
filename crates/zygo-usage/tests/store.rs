use std::{collections::BTreeMap, path::{Path, PathBuf}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use rusqlite::Connection;
use zygo_usage::*;

const DAY: i64 = 86_400;
fn now() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 }
struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let d = std::env::temp_dir().join(format!("zygo-usage-{}", zygo_core::ids::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
    fn db(&self) -> PathBuf { self.0.join("usage.db") }
}
impl Drop for Tmp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn cols(db: &Path, table: &str) -> Vec<String> {
    let c = Connection::open(db).unwrap();
    let mut s = c.prepare(&format!("PRAGMA table_info({table})")).unwrap();
    s.query_map([], |r| r.get::<_, String>(1)).unwrap().map(|r| r.unwrap()).collect()
}
fn count(db: &Path, table: &str) -> i64 {
    Connection::open(db).unwrap().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}
fn req(session: &str, ts: i64) -> RequestRow {
    RequestRow { ts, session: session.into(), agent: "claude-code".into(), route: "/anthropic/v1/messages".into(),
        model: "m1".into(), provider: "p1".into(), status: 200, ..Default::default() }
}
fn trim(session: &str, ts: i64, stored: u64, view: u64) -> TrimRow {
    TrimRow { ts, session: session.into(), agent_id: None, tool: "Bash".into(), kind: "head".into(), stored_bytes: stored, view_bytes: view, result_id: None, command: None, shaper: None }
}
fn unit(session: &str, name: &str, began_at: i64) -> WorkUnitRow {
    WorkUnitRow { unit: name.into(), round: 1, session: session.into(), agent_id: "main".into(), began_at, ..Default::default() }
}
/// The `SCHEMA` text of commit `97d8767`: lines 6-77 of `git show 97d8767:crates/zygo-usage/src/schema.rs`, copied
/// byte for byte (from `CREATE TABLE IF NOT EXISTS requests (` to `… upstream_errors_ts ON upstream_errors(ts);`).
const SCHEMA_97D8767: &str = "
CREATE TABLE IF NOT EXISTS requests (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    token_label TEXT,
    agent TEXT NOT NULL,
    agent_version TEXT,
    agent_id TEXT,
    agent_type TEXT,
    request_class TEXT,
    compaction INTEGER NOT NULL,
    route TEXT NOT NULL,
    role_requested TEXT,
    role_served TEXT,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    profile TEXT,
    passthrough INTEGER NOT NULL,
    status INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    cache_write_tokens INTEGER NOT NULL,
    estimated_usd REAL NOT NULL,
    adjustments TEXT NOT NULL,
    redactions TEXT NOT NULL,
    repo TEXT,
    work_item TEXT
);
CREATE TABLE IF NOT EXISTS trims (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    agent_id TEXT,
    tool TEXT NOT NULL,
    kind TEXT NOT NULL,
    stored_bytes INTEGER NOT NULL,
    view_bytes INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS work_units (
    unit TEXT NOT NULL,
    round INTEGER NOT NULL,
    session TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    agent_type TEXT,
    began_at INTEGER NOT NULL,
    ended_at INTEGER,
    tool_calls INTEGER NOT NULL,
    failed_calls INTEGER NOT NULL,
    check_ran INTEGER NOT NULL,
    check_passed INTEGER NOT NULL,
    guard_events TEXT NOT NULL,
    levels TEXT NOT NULL,
    verdict TEXT,
    PRIMARY KEY (session, agent_id, unit, round, began_at)
);
CREATE INDEX IF NOT EXISTS requests_session_ts ON requests(session, ts);
CREATE INDEX IF NOT EXISTS requests_ts ON requests(ts);
CREATE INDEX IF NOT EXISTS trims_session ON trims(session);
CREATE INDEX IF NOT EXISTS trims_ts ON trims(ts);
CREATE INDEX IF NOT EXISTS work_units_session ON work_units(session);
CREATE TABLE IF NOT EXISTS sync_pending (session TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS upstream_errors (
    ts INTEGER NOT NULL,
    session TEXT NOT NULL,
    request_class TEXT,
    model TEXT NOT NULL,
    status INTEGER NOT NULL,
    error_type TEXT NOT NULL,
    error_message TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS upstream_errors_session_ts ON upstream_errors(session, ts);
CREATE INDEX IF NOT EXISTS upstream_errors_ts ON upstream_errors(ts);
";
const TRIM_SEVEN: [&str; 7] = ["ts", "session", "agent_id", "tool", "kind", "stored_bytes", "view_bytes"];
const TRIM_TEN: [&str; 10] = ["ts", "session", "agent_id", "tool", "kind", "stored_bytes", "view_bytes", "result_id", "command", "shaper"];
const RID: &str = "01J00000000000000000000000";
/// A database as the code at `97d8767` leaves it: its schema text, `user_version` 1, one seven-column `trims` row.
fn old_db(db: &Path) {
    let c = Connection::open(db).unwrap();
    c.execute_batch(SCHEMA_97D8767).unwrap();
    c.execute_batch("PRAGMA user_version = 1").unwrap();
    c.execute("INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (5, 'S1', NULL, 'Bash', 'head', 1000, 100)", []).unwrap();
}
fn pragma(db: &Path, name: &str) -> i64 {
    Connection::open(db).unwrap().query_row(&format!("PRAGMA {name}"), [], |r| r.get(0)).unwrap()
}
/// Makes `trims` a seven-column table again under an open `Store` (SQLite 3.35+ `DROP COLUMN`).
fn drop_new_trim_columns(db: &Path) {
    let c = Connection::open(db).unwrap();
    for col in ["result_id", "command", "shaper"] { c.execute_batch(&format!("ALTER TABLE trims DROP COLUMN {col}")).unwrap(); }
}
fn readback(session: &str, ts: i64) -> ReadbackRow {
    ReadbackRow { ts, session: session.into(), read_in: session.into(), result_id: RID.into(), tool: "result_view".into(), bytes: 10 }
}
type TenCols = (i64, String, Option<String>, String, String, i64, i64, Option<String>, Option<String>, Option<String>);
fn ten_cols(db: &Path, session: &str) -> TenCols {
    Connection::open(db).unwrap().query_row(
        "SELECT ts, session, agent_id, tool, kind, stored_bytes, view_bytes, result_id, command, shaper FROM trims WHERE session = ?1",
        [session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))).unwrap()
}

#[test]
fn schema_creates_three_tables_in_wal_mode() {
    let t = Tmp::new();
    let _s = Store::open(&t.db()).unwrap();
    let c = Connection::open(t.db()).unwrap();
    let mut names: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    names.sort();
    assert_eq!(names, ["readbacks", "requests", "sync_pending", "trims", "upstream_errors", "work_units"]); // N37 S4: sync_pending; FIX-10: upstream_errors; O§6.5: readbacks
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn never_list_columns_are_exact() {
    let t = Tmp::new();
    let _s = Store::open(&t.db()).unwrap();
    assert_eq!(cols(&t.db(), "requests"), ["ts","session","token_label","agent","agent_version","agent_id","agent_type","request_class","compaction","route","role_requested","role_served","model","provider","profile","passthrough","status","latency_ms","input_tokens","output_tokens","cache_read_tokens","cache_write_tokens","estimated_usd","adjustments","redactions","repo","work_item"]);
    assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
    assert_eq!(cols(&t.db(), "work_units"), ["unit","round","session","agent_id","agent_type","began_at","ended_at","tool_calls","failed_calls","check_ran","check_passed","guard_events","levels","verdict"]);
    assert_eq!(cols(&t.db(), "upstream_errors"), ["ts","session","request_class","model","status","error_type","error_message"]);
}

#[test]
fn open_creates_missing_parent_directories() {
    let t = Tmp::new();
    let p = t.0.join("a").join("b").join("usage.db");
    Store::open(&p).unwrap();
    assert!(p.exists());
}

#[test]
fn request_round_trip_all_columns() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let full = RequestRow { ts: 1_000, session: "s1".into(), token_label: Some("team".into()), agent: "opencode".into(),
        agent_version: Some("1.2.3".into()), agent_id: Some("a1".into()), agent_type: Some("zygo-builder".into()),
        request_class: Some("subagent".into()), compaction: true, route: "/v1/messages".into(),
        role_requested: Some("lead".into()), role_served: Some("code".into()), model: "m1".into(), provider: "p1".into(),
        profile: Some("fast".into()), passthrough: true, status: 429, latency_ms: 1234, input_tokens: 11, output_tokens: 22,
        cache_read_tokens: 33, cache_write_tokens: 44, estimated_usd: 0.125,
        adjustments: vec!["effort_clamped:high>medium".into(), "thinking_dropped:2".into()],
        redactions: vec![RedactionCount { id: "aws_key".into(), count: 3 }],
        repo: Some("r1".into()), work_item: Some("work-graph:zygo-v1/N7".into()) };
    let sparse = req("s1", 2_000); // every Option None, empty lists, false flags
    let other = req("s2", 1_500);
    s.insert_request(&sparse).unwrap();
    s.insert_request(&other).unwrap();
    s.insert_request(&full).unwrap();
    assert_eq!(s.requests_for_session("s1").unwrap(), vec![full, sparse]); // ts order, s2 excluded
    assert!(s.requests_for_session("none").unwrap().is_empty());
}

#[test]
fn insert_requests_is_all_or_nothing() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_requests(&[req("ok", 1), req("ok", 2), req("ok", 3)]).unwrap();
    assert_eq!(s.requests_for_session("ok").unwrap().len(), 3);

    let mut bad = req("bad", 3);
    bad.estimated_usd = f64::NAN;
    assert!(s.insert_requests(&[req("bad", 1), req("bad", 2), bad]).is_err());
    assert!(s.requests_for_session("bad").unwrap().is_empty()); // rows 1 and 2 rolled back
    assert_eq!(s.insert_requests(&[]).unwrap(), 0); // empty batch is a no-op
}

#[test]
fn insert_requests_is_idempotent_per_session() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let rows = [req("s1", 1), req("s1", 2), req("s1", 3)];
    assert_eq!(s.insert_requests(&rows).unwrap(), 3);
    assert_eq!(s.insert_requests(&rows).unwrap(), 0); // replay of a stored session inserts nothing
    assert_eq!(count(&t.db(), "requests"), 3);
    s.insert_request(&req("live", 1)).unwrap(); // a session with any row is not replayed
    assert_eq!(s.insert_requests(&[req("live", 2)]).unwrap(), 0);
    assert_eq!(s.requests_for_session("live").unwrap().len(), 1);
}

#[test]
fn insert_requests_rejects_mixed_sessions() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    assert!(s.insert_requests(&[req("a", 1), req("b", 2)]).is_err());
    assert_eq!(count(&t.db(), "requests"), 0);
}

#[test]
fn insert_rejects_non_finite_estimated_usd() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let bad = RequestRow { estimated_usd: v, ..req("s1", 1) };
        assert!(s.insert_request(&bad).is_err(), "{v}");
        assert!(s.insert_requests(&[bad]).is_err(), "{v}");
    }
    assert_eq!(count(&t.db(), "requests"), 0);
}

#[test]
fn reads_reject_negative_stored_integers() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_request(&req("s1", 1)).unwrap();
    Connection::open(t.db()).unwrap().execute("UPDATE requests SET input_tokens = -5", []).unwrap();
    assert!(s.requests_for_session("s1").is_err());
    assert!(s.summary(&Filter::default()).is_err());
}

#[test]
fn trim_round_trip() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let mut tr = trim("s1", 5, 1000, 100);
    tr.agent_id = Some("a1".into());
    s.insert_trim(&tr).unwrap();
    let row: (i64, String, Option<String>, String, String, i64, i64) = Connection::open(t.db()).unwrap()
        .query_row("SELECT ts, session, agent_id, tool, kind, stored_bytes, view_bytes FROM trims", [], |r|
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).unwrap();
    assert_eq!(row, (5, "s1".into(), Some("a1".into()), "Bash".into(), "head".into(), 1000, 100));
}

#[test]
fn work_unit_round_trip_and_idempotent_insert() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let mut u = unit("s1", "work-graph:zygo-v1/N7", 10);
    u.agent_type = Some("zygo-builder".into());
    u.ended_at = Some(20); u.tool_calls = 7; u.failed_calls = 1; u.check_ran = true; u.check_passed = true;
    u.guard_events = BTreeMap::from([("scope".into(), BTreeMap::from([("notify".into(), 2u32)]))]);
    u.levels = BTreeMap::from([("scope".into(), "notify".into())]);
    u.verdict = Some("keep".into());
    s.insert_work_unit(&u).unwrap();
    s.insert_work_unit(&u).unwrap(); // same key: updated, not duplicated
    let c = Connection::open(t.db()).unwrap();
    assert_eq!(count(&t.db(), "work_units"), 1);
    let (ge, lv, verdict, ran, passed, ended): (String, String, Option<String>, bool, bool, Option<i64>) = c
        .query_row("SELECT guard_events, levels, verdict, check_ran, check_passed, ended_at FROM work_units", [], |r|
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&ge).unwrap(), serde_json::json!({"scope": {"notify": 2}}));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&lv).unwrap(), serde_json::json!({"scope": "notify"}));
    assert_eq!((verdict.as_deref(), ran, passed, ended), (Some("keep"), true, true, Some(20)));
}

fn seed(s: &Store, session: &str, ts: i64) {
    s.insert_request(&req(session, ts)).unwrap();
    s.insert_trim(&trim(session, ts, 10, 1)).unwrap();
    s.insert_work_unit(&unit(session, "u", ts)).unwrap();
}
fn survivors(db: &Path, table: &str) -> Vec<String> {
    let c = Connection::open(db).unwrap();
    let mut q = c.prepare(&format!("SELECT session FROM {table} ORDER BY session")).unwrap();
    q.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
}

#[test]
fn prune_90_deletes_only_rows_older_than_90_days() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    seed(&s, "old", now() - 91 * DAY);
    seed(&s, "young", now() - 89 * DAY);
    s.prune(90).unwrap();
    for table in ["requests", "trims", "work_units"] {
        assert_eq!(survivors(&t.db(), table), ["young"], "{table}");
    }
}

#[test]
#[ignore = "timing: fails when more than 5 s pass between seed and prune"]
fn prune_boundary_within_a_five_second_margin() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    seed(&s, "just_inside", now() - 90 * DAY + 5);
    seed(&s, "just_outside", now() - 90 * DAY - 5);
    s.prune(90).unwrap();
    for table in ["requests", "trims", "work_units"] {
        assert_eq!(survivors(&t.db(), table), ["just_inside"], "{table}");
    }
}

#[test]
fn prune_zero_keeps_everything() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    seed(&s, "ancient", 1);
    s.prune(0).unwrap();
    for table in ["requests", "trims", "work_units"] {
        assert_eq!(survivors(&t.db(), table), ["ancient"], "{table}");
    }
}

#[allow(clippy::too_many_arguments)] // test row builder: one positional field per column
fn r(session: &str, ts: i64, role: &str, model: &str, i: u64, o: u64, cr: u64, cw: u64, usd: f64) -> RequestRow {
    RequestRow { role_served: Some(role.into()), model: model.into(), input_tokens: i, output_tokens: o,
        cache_read_tokens: cr, cache_write_tokens: cw, estimated_usd: usd, ..req(session, ts) }
}

#[test]
fn summary_totals_filters_and_groups() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let n = now();
    s.insert_request(&r("A", n - 10, "code", "m1", 100, 10, 300, 100, 0.5)).unwrap();
    let mut signal = r("A", n - 5, "code", "m1", 100, 10, 100, 0, 0.25);
    signal.compaction = true;
    signal.adjustments = vec!["context_signal".into(), "effort_clamped:high>medium".into()];
    s.insert_request(&signal).unwrap();
    let mut safety = r("A", n - 9, "code", "m1", 0, 0, 0, 0, 0.0);
    safety.adjustments = vec!["context_safety".into()];
    s.insert_request(&safety).unwrap();
    let mut compact = r("A", n - 7, "code", "m1", 0, 0, 0, 0, 0.0);
    compact.adjustments = vec!["context_compact".into()];
    s.insert_request(&compact).unwrap();
    s.insert_request(&r("A", n - 1, "lead", "m2", 50, 5, 0, 0, 1.0)).unwrap();
    s.insert_request(&r("B", n - 1, "code", "m1", 1000, 0, 0, 0, 9.0)).unwrap();
    s.insert_trim(&trim("A", n - 5, 1000, 100)).unwrap();
    s.insert_trim(&trim("A", n - 4, 500, 500)).unwrap();
    s.insert_trim(&trim("A", n - 3, 10, 50)).unwrap(); // view larger than stored counts 0
    s.insert_trim(&trim("A", n - 20, 700, 0)).unwrap(); // older than the `since` used below
    let mut u1 = unit("A", "u1", n - 8);
    u1.guard_events = BTreeMap::from([("scope".into(), BTreeMap::from([("notify".into(), 2u32)])), ("claims".into(), BTreeMap::from([("enforce".into(), 1u32)]))]);
    let mut u2 = unit("A", "u2", n - 2);
    u2.guard_events = BTreeMap::from([("scope".into(), BTreeMap::from([("notify".into(), 1u32)]))]);
    s.insert_work_unit(&u1).unwrap();
    s.insert_work_unit(&u2).unwrap();

    let a = s.summary(&Filter { session: Some("A".into()), since: None }).unwrap();
    assert_eq!((a.requests, a.input_tokens, a.output_tokens, a.cache_read_tokens, a.cache_write_tokens), (5, 250, 25, 400, 100));
    assert!((a.cache_read_share - 400.0 / 750.0).abs() < 1e-9);
    assert!((a.estimated_usd - 1.75).abs() < 1e-9);
    assert_eq!((a.bytes_trimmed, a.compaction_signals, a.compactions_observed), (1600, 3, 1)); // context_signal, context_safety and context_compact each count once
    assert_eq!(a.by_role_model.iter().map(|g| (g.role.as_str(), g.model.as_str(), g.requests, g.input_tokens)).collect::<Vec<_>>(),
               [("code", "m1", 4, 200), ("lead", "m2", 1, 50)]);
    assert_eq!(a.guard_events["scope"]["notify"], 3);
    assert_eq!(a.guard_events["claims"]["enforce"], 1);

    let all = s.summary(&Filter::default()).unwrap();
    assert_eq!((all.requests, all.input_tokens), (6, 1250));
    let recent = s.summary(&Filter { session: Some("A".into()), since: Some(n - 6) }).unwrap();
    assert_eq!(recent.requests, 2); // n-5 and n-1; the n-7 and n-9 rows are older
    assert_eq!(recent.bytes_trimmed, 900); // the n-20 trim (700) is excluded by `since`
    assert_eq!(recent.guard_events["scope"]["notify"], 1); // only u2 began after since
    let empty = s.summary(&Filter { session: Some("none".into()), since: None }).unwrap();
    assert_eq!(empty, Summary::default());
}

#[test]
#[ignore = "timing: a 300 ms sleep races the 5 s busy bound"]
fn insert_waits_for_a_lock_held_by_another_writer() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap(); // second process holds the write lock
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        raw.execute_batch("COMMIT").unwrap();
    });
    s.insert_request(&req("s1", now())).unwrap(); // must wait, not fail with SQLITE_BUSY
    release.join().unwrap();
    assert_eq!(s.requests_for_session("s1").unwrap().len(), 1);
}

#[test]
#[ignore = "timing: contending writers rely on the 5 s busy bound"]
fn two_stores_open_and_write_concurrently() {
    let t = Tmp::new();
    let db = t.db();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let hs: Vec<_> = ["hooks", "proxy"].into_iter().map(|who| {
        let (db, b) = (db.clone(), barrier.clone());
        std::thread::spawn(move || {
            b.wait();
            let s = Store::open(&db).unwrap(); // both race the first open and the WAL switch
            for i in 0..200 { s.insert_request(&req(who, i)).unwrap(); s.insert_trim(&trim(who, i, 2, 1)).unwrap(); }
        })
    }).collect();
    for h in hs { h.join().unwrap(); }
    let s = Store::open(&db).unwrap();
    assert_eq!(s.requests_for_session("hooks").unwrap().len(), 200);
    assert_eq!(s.requests_for_session("proxy").unwrap().len(), 200);
    assert_eq!(count(&db, "trims"), 400);
}

#[test]
fn open_sets_a_five_second_busy_timeout() {
    let t = Tmp::new();
    assert_eq!(Store::open(&t.db()).unwrap().busy_timeout_ms().unwrap(), 5000);
}

#[test]
fn set_verdict_updates_every_row_of_the_unit_round() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let builder = WorkUnitRow { agent_id: "builder".into(), ..unit("A", "gh-1", 10) };
    let reviewer = WorkUnitRow { agent_id: "reviewer".into(), ..unit("A", "gh-1", 20) };
    let round2 = WorkUnitRow { round: 2, ..unit("A", "gh-1", 30) };
    let other = unit("B", "gh-1", 40);
    for u in [&builder, &reviewer, &round2, &other] { s.insert_work_unit(u).unwrap(); }
    assert!(s.set_verdict("A", "gh-1", 1, "rework").unwrap());
    let c = Connection::open(t.db()).unwrap();
    let rows: Vec<(i64, Option<String>)> = c.prepare("SELECT began_at, verdict FROM work_units ORDER BY began_at").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(rows, [(10, Some("rework".into())), (20, Some("rework".into())), (30, None), (40, None)]);
    assert!(!s.set_verdict("A", "gh-9", 1, "keep").unwrap()); // no such unit
    assert!(!s.set_verdict("A", "gh-1", 3, "keep").unwrap()); // no such round
}

#[test]
fn set_verdict_rejects_unknown_values() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_work_unit(&unit("A", "gh-1", 10)).unwrap();
    for ok in ["keep", "discard", "rework"] { assert!(s.set_verdict("A", "gh-1", 1, ok).unwrap()); }
    for bad in ["", "KEEP", "approve", "keep; DROP TABLE work_units"] {
        assert!(s.set_verdict("A", "gh-1", 1, bad).is_err(), "{bad:?}");
    }
    let v: Option<String> = Connection::open(t.db()).unwrap().query_row("SELECT verdict FROM work_units", [], |r| r.get(0)).unwrap();
    assert_eq!(v.as_deref(), Some("rework")); // last valid value stays
}

#[test]
fn refold_does_not_erase_a_recorded_verdict() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let mut u = unit("A", "gh-1", 10);
    s.insert_work_unit(&u).unwrap();
    assert!(s.set_verdict("A", "gh-1", 1, "keep").unwrap());
    u.verdict = None; u.tool_calls = 9; u.ended_at = Some(30); // re-fold of the same key
    s.insert_work_unit(&u).unwrap();
    let (v, calls, ended): (Option<String>, i64, Option<i64>) = Connection::open(t.db()).unwrap()
        .query_row("SELECT verdict, tool_calls, ended_at FROM work_units", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((v.as_deref(), calls, ended), (Some("keep"), 9, Some(30))); // other columns updated
    u.verdict = Some("discard".into()); // an explicit verdict in a re-fold wins
    s.insert_work_unit(&u).unwrap();
    let v: Option<String> = Connection::open(t.db()).unwrap().query_row("SELECT verdict FROM work_units", [], |r| r.get(0)).unwrap();
    assert_eq!(v.as_deref(), Some("discard"));
}

#[test]
fn work_unit_rejects_unknown_verdict() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    for ok in [None, Some("keep"), Some("discard"), Some("rework")] {
        s.insert_work_unit(&WorkUnitRow { verdict: ok.map(Into::into), ..unit("A", "u", 1) }).unwrap();
    }
    for bad in ["", "KEEP", "approve"] {
        assert!(s.insert_work_unit(&WorkUnitRow { verdict: Some(bad.into()), ..unit("A", "u", 2) }).is_err(), "{bad:?}");
    }
    assert_eq!(count(&t.db(), "work_units"), 1);
}

#[test]
fn open_sets_user_version_and_rejects_newer() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    let c = Connection::open(t.db()).unwrap();
    assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    c.execute_batch("PRAGMA user_version = 2").unwrap();
    let e = Store::open(&t.db()).err().expect("newer schema must fail").to_string();
    assert!(e.contains('2') && e.contains('1'), "{e}");
}

// ---- N37: serve sessions pending sync (S4) ----

#[test]
fn sync_pending_table_is_created_on_every_open() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    Connection::open(t.db()).unwrap().execute_batch("DROP TABLE sync_pending").unwrap();
    let s = Store::open(&t.db()).unwrap();
    s.mark_sync_pending("S1").unwrap();
    assert_eq!(s.sync_pending().unwrap(), ["S1"]);
    let c = Connection::open(t.db()).unwrap();
    assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
} // mutation: the table not in `SCHEMA`

#[test]
fn sync_pending_mark_list_clear() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    for id in ["S2", "S1", "S2"] {
        s.mark_sync_pending(id).unwrap();
    }
    assert_eq!(s.sync_pending().unwrap(), ["S1", "S2"]);
    s.clear_sync_pending("S1").unwrap();
    assert_eq!(s.sync_pending().unwrap(), ["S2"]);
    s.clear_sync_pending("S9").unwrap();
    assert_eq!(s.sync_pending().unwrap(), ["S2"]);
} // mutations: no `OR IGNORE`; clear does nothing

#[test]
fn replace_requests_swaps_a_session_in_one_transaction() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_request(&req("R1", 1)).unwrap();
    s.insert_request(&req("R2", 1)).unwrap();
    let (a, b) = (req("R1", 2), req("R1", 3));
    assert_eq!(s.replace_requests("R1", &[a.clone(), b.clone()]).unwrap(), 2);
    assert_eq!(s.requests_for_session("R1").unwrap(), [a.clone(), b.clone()]);
    assert_eq!(s.requests_for_session("R2").unwrap().len(), 1);
    assert!(s.replace_requests("R1", &[req("R2", 4)]).is_err());
    assert_eq!(s.requests_for_session("R1").unwrap(), [a, b]); // rolled back
    assert_eq!(s.requests_for_session("R2").unwrap().len(), 1);
} // mutations: no delete (→ 3 rows); delete outside the transaction (→ 0 rows after the error)

#[test]
fn bytes_trimmed_equals_summary_for_the_session() { // H§7.4
    let t = Tmp::new();
    let w = Store::open(&t.db()).unwrap();
    w.insert_trim(&trim("A", 1, 1000, 100)).unwrap();
    w.insert_trim(&trim("A", 2, 500, 500)).unwrap();
    w.insert_trim(&trim("A", 3, 10, 50)).unwrap(); // view larger than stored counts 0
    w.insert_trim(&trim("A", 4, 700, 0)).unwrap();
    w.insert_trim(&trim("B", 5, 300, 0)).unwrap();
    let s = Store::open_read_only(&t.db()).unwrap(); // the HUD opens the store read-only
    assert_eq!(s.bytes_trimmed("A").unwrap(), 1600);
    assert_eq!(s.bytes_trimmed("A").unwrap(),
               s.summary(&Filter { session: Some("A".into()), since: None }).unwrap().bytes_trimmed);
    assert_eq!(s.bytes_trimmed("B").unwrap(), 300);
    assert_eq!(s.bytes_trimmed("none").unwrap(), 0);
}

/// H§14 #18 (store part): for the 92,160-byte fixture `bytes_trimmed` equals `summary` for each session, the estimate
/// is 23,040 tokens, and one read-only handle gives a row written after its first read (the HUD caches no count).
#[test]
fn h14_18_bytes_trimmed_equals_summary_and_follows_a_new_row() {
    let t = Tmp::new();
    let w = Store::open(&t.db()).unwrap();
    w.insert_trim(&trim("S1", 1, 100_000, 7_840)).unwrap();
    w.insert_trim(&trim("S2", 2, 4_000, 0)).unwrap();
    let r = Store::open_read_only(&t.db()).unwrap();
    let both = |session: &str| {
        let n = r.bytes_trimmed(session).unwrap();
        assert_eq!(n, r.summary(&Filter { session: Some(session.into()), since: None }).unwrap().bytes_trimmed, "{session}");
        n
    };
    assert_eq!(both("S1"), 92_160);
    assert_eq!(both("S1").div_ceil(zygo_core::tokens::BYTES_PER_TOKEN), 23_040);
    assert_eq!(both("S2"), 4_000);
    w.insert_trim(&trim("S1", 3, 5, 0)).unwrap();
    assert_eq!(both("S1"), 92_165);
    assert_eq!(both("S1").div_ceil(zygo_core::tokens::BYTES_PER_TOKEN), 23_042);
    assert_eq!(both("S2"), 4_000);
}

#[test]
fn bytes_trimmed_query_uses_the_session_index() { // H§7.4: one indexed SUM
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap()); // creates the schema and its indexes
    let c = Connection::open(t.db()).unwrap();
    let mut q = c.prepare(&format!("EXPLAIN QUERY PLAN {}", query::BYTES_TRIMMED_SQL)).unwrap();
    let plan: Vec<String> = q.query_map(["A"], |r| r.get::<_, String>(3)).unwrap().map(|r| r.unwrap()).collect();
    assert!(plan.iter().any(|d| d.starts_with("SEARCH trims USING INDEX trims_session")), "{plan:?}");
    assert!(!plan.iter().any(|d| d.starts_with("SCAN trims")), "{plan:?}");
}

// ---- FIX-10: upstream_errors ----

fn uerr(session: &str, ts: i64) -> UpstreamErrorRow {
    UpstreamErrorRow { ts, session: session.into(), request_class: Some("auxiliary".into()), model: "m1".into(), status: 429,
        error_type: "rate_limit_error".into(), error_message: "limit".into() }
}
fn tables(db: &Path) -> Vec<String> {
    let c = Connection::open(db).unwrap();
    let mut q = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
    q.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
}

#[test] // FIX-10: insert and read by session, in ts order
fn upstream_error_round_trip_by_session() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let a = uerr("S1", 10);
    let b = UpstreamErrorRow { request_class: None, status: 500, error_type: String::new(), error_message: String::new(), ..uerr("S1", 11) };
    s.insert_upstream_error(&b).unwrap();
    s.insert_upstream_error(&a).unwrap();
    s.insert_upstream_error(&uerr("S2", 5)).unwrap();
    assert_eq!(s.upstream_errors("S1").unwrap(), vec![a, b]);
    assert_eq!(s.upstream_errors("S2").unwrap(), vec![uerr("S2", 5)]);
    assert!(s.upstream_errors("S9").unwrap().is_empty());
    assert_eq!(count(&t.db(), "requests"), 0);
}

#[test] // FIX-10: an existing usage.db with only the old tables gets the table on open; no schema version change
fn upstream_errors_table_is_created_on_an_existing_database() {
    let t = Tmp::new();
    Store::open(&t.db()).unwrap().insert_request(&req("S1", 1)).unwrap();
    Connection::open(t.db()).unwrap().execute_batch("DROP TABLE upstream_errors").unwrap();
    assert_eq!(tables(&t.db()), ["readbacks", "requests", "sync_pending", "trims", "work_units"]);
    let s = Store::open(&t.db()).unwrap();
    s.insert_upstream_error(&uerr("S1", 2)).unwrap();
    assert_eq!(s.upstream_errors("S1").unwrap(), vec![uerr("S1", 2)]);
    assert_eq!(s.requests_for_session("S1").unwrap(), vec![req("S1", 1)]);
    let c = Connection::open(t.db()).unwrap();
    assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
} // mutation: the table not in `SCHEMA`

#[test] // FIX-10: retention as `requests` (§14 `[usage] retain_days`)
fn prune_deletes_upstream_errors_older_than_retain_days() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_upstream_error(&uerr("old", now() - 91 * DAY)).unwrap();
    s.insert_upstream_error(&uerr("young", now() - 89 * DAY)).unwrap();
    s.prune(90).unwrap();
    assert_eq!(survivors(&t.db(), "upstream_errors"), ["young"]);
    s.prune(0).unwrap();
    assert_eq!(survivors(&t.db(), "upstream_errors"), ["young"]);
}

#[test] // FIX-10 M3: the request row and its error row, both or neither
fn insert_request_and_error_is_one_transaction() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_request_and_error(&req("S1", 1), &uerr("S1", 1)).unwrap();
    assert_eq!((s.requests_for_session("S1").unwrap().len(), s.upstream_errors("S1").unwrap().len()), (1, 1));
    let bad = RequestRow { estimated_usd: f64::NAN, ..req("S2", 1) };
    assert!(s.insert_request_and_error(&bad, &uerr("S2", 1)).is_err());
    assert!(s.upstream_errors("S2").unwrap().is_empty());
    Connection::open(t.db()).unwrap().execute_batch("DROP TABLE upstream_errors").unwrap();
    assert!(s.insert_request_and_error(&req("S3", 1), &uerr("S3", 1)).is_err());
    assert!(s.requests_for_session("S3").unwrap().is_empty());
} // mutations: two separate inserts; commit before the error insert

#[test] // FIX-10 M9: `zygo usage --sync` replaces the session's rows, its upstream_errors rows included
fn replace_requests_deletes_the_sessions_upstream_errors() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_upstream_error(&uerr("S1", 1)).unwrap();
    s.insert_upstream_error(&uerr("S2", 1)).unwrap();
    assert_eq!(s.replace_requests("S1", &[req("S1", 2)]).unwrap(), 1);
    assert!(s.upstream_errors("S1").unwrap().is_empty());
    assert_eq!(s.upstream_errors("S2").unwrap(), vec![uerr("S2", 1)]);
}

// ---- OS-2: readbacks and the additive trims columns (O§6.5) ----

#[test] // O§6.5 item 2: the table, its NOT NULL columns, its two indexes
fn readbacks_schema_table_and_indexes_exist_after_open() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    assert_eq!(cols(&t.db(), "readbacks"), ["ts", "session", "read_in", "result_id", "tool", "bytes"]);
    let c = Connection::open(t.db()).unwrap();
    let notnull: Vec<i64> = c.prepare("PRAGMA table_info(readbacks)").unwrap().query_map([], |r| r.get(3)).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(notnull, [1, 1, 1, 1, 1, 1]);
    let idx: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'readbacks' ORDER BY name").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(idx, ["readbacks_session", "readbacks_ts"]);
    let on = |i: &str| -> Vec<String> { c.prepare(&format!("PRAGMA index_info({i})")).unwrap().query_map([], |r| r.get(2)).unwrap().map(|r| r.unwrap()).collect() };
    assert_eq!(on("readbacks_session"), ["session"]);
    assert_eq!(on("readbacks_ts"), ["ts"]);
}

#[test] // O-C8, O§6.5 preamble: no schema version change, on a fresh and on a 97d8767 database
fn readbacks_schema_user_version_stays_1() {
    assert_eq!(store::SCHEMA_VERSION, 1);
    let (fresh, old) = (Tmp::new(), Tmp::new());
    drop(Store::open(&fresh.db()).unwrap());
    old_db(&old.db());
    drop(Store::open(&old.db()).unwrap());
    assert_eq!(pragma(&fresh.db(), "user_version"), 1);
    assert_eq!(pragma(&old.db(), "user_version"), 1);
}

#[test] // O§6.5 item 4: ten columns written; item 4 `#[serde(default)]`: a row without the new keys reads them as None
fn readbacks_schema_insert_trim_writes_ten_columns() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    let tr = TrimRow { agent_id: Some("a1".into()), result_id: Some(RID.into()), command: Some("git diff".into()), shaper: Some("diff".into()), ..trim("S1", 5, 1000, 100) };
    s.insert_trim(&tr).unwrap();
    assert_eq!(ten_cols(&t.db(), "S1"), (5, "S1".into(), Some("a1".into()), "Bash".into(), "head".into(), 1000, 100, Some(RID.into()), Some("git diff".into()), Some("diff".into())));
    let old: TrimRow = serde_json::from_str(r#"{"ts":5,"session":"S1","agent_id":null,"tool":"Bash","kind":"head","stored_bytes":1000,"view_bytes":100}"#).unwrap();
    assert_eq!(old, trim("S1", 5, 1000, 100));
}

#[test] // O§6.4, O§6.5 item 2: one row, every column
fn readbacks_schema_insert_readback_round_trip() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_readback(&ReadbackRow { ts: 7, session: "S1".into(), read_in: "S2".into(), result_id: RID.into(), tool: "result_view".into(), bytes: 4096 }).unwrap();
    let row: (i64, String, String, String, String, i64) = Connection::open(t.db()).unwrap()
        .query_row("SELECT ts, session, read_in, result_id, tool, bytes FROM readbacks", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).unwrap();
    assert_eq!(row, (7, "S1".into(), "S2".into(), RID.into(), "result_view".into(), 4096));
}

#[test] // O§6.5 item 6; fixed ts values far from any cutoff, so no clock margin is involved
fn readbacks_schema_prune_deletes_old_readbacks_and_keeps_new() {
    const OLD_TS: i64 = 1; // 1970-01-01: older than every cutoff
    const NEW_TS: i64 = 4_102_444_800; // 2100-01-01T00:00:00Z: newer than every cutoff before 2100
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    s.insert_readback(&readback("old", OLD_TS)).unwrap();
    s.insert_readback(&readback("new", NEW_TS)).unwrap();
    s.prune(90).unwrap();
    assert_eq!(survivors(&t.db(), "readbacks"), ["new"]);
    s.prune(0).unwrap(); // 0 keeps everything
    assert_eq!(survivors(&t.db(), "readbacks"), ["new"]);
}

#[test] // O§6.5 item 3; O§16 item 15 "a full open then adds the three columns"; old rows read the new columns as null
fn readbacks_schema_old_database_gets_three_columns_and_old_rows_read_null() {
    let t = Tmp::new();
    old_db(&t.db());
    assert_eq!(cols(&t.db(), "trims"), TRIM_SEVEN);
    assert!(!tables(&t.db()).iter().any(|n| n == "readbacks"));
    drop(Store::open(&t.db()).unwrap());
    assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
    assert!(tables(&t.db()).iter().any(|n| n == "readbacks"));
    assert_eq!(ten_cols(&t.db(), "S1"), (5, "S1".into(), None, "Bash".into(), "head".into(), 1000, 100, None, None, None));
}

#[test] // O§10 "Two full opens add a column at once"; O§16 item 15: an injected missing column gets "duplicate column name" and succeeds
fn readbacks_schema_injected_missing_column_is_success() {
    let t = Tmp::new();
    old_db(&t.db());
    drop(Store::open(&t.db()).unwrap()); // adds the three columns
    let s = Store::open_with_column_check(&t.db(), &["shaper"]).unwrap();
    assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
    s.insert_trim(&TrimRow { shaper: Some("size".into()), ..trim("S2", 6, 10, 1) }).unwrap();
    assert_eq!(ten_cols(&t.db(), "S2").9.as_deref(), Some("size"));
}

#[test] // O§6.5 item 4, O§10 "Database without the new columns (hook insert)"; item 3 "one time per process"
fn readbacks_schema_insert_trim_on_seven_column_database_writes_seven_columns() {
    let t = Tmp::new();
    old_db(&t.db());
    drop(Store::open(&t.db()).unwrap());
    drop_new_trim_columns(&t.db());
    let s = Store::open(&t.db()).unwrap(); // this process checked this path already: no second check
    assert_eq!(cols(&t.db(), "trims"), TRIM_SEVEN);
    s.insert_trim(&TrimRow { result_id: Some(RID.into()), command: Some("cargo test".into()), shaper: Some("libtest".into()), ..trim("S2", 6, 2000, 200) }).unwrap();
    assert_eq!(count(&t.db(), "trims"), 2);
    let row: (i64, String, Option<String>, String, String, i64, i64) = Connection::open(t.db()).unwrap()
        .query_row("SELECT ts, session, agent_id, tool, kind, stored_bytes, view_bytes FROM trims WHERE session = 'S2'", [], |r|
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).unwrap();
    assert_eq!(row, (6, "S2".into(), None, "Bash".into(), "head".into(), 2000, 200));
}

#[test] // review focus 21: no ALTER (no schema write) when the columns exist; the seam always checks
fn readbacks_schema_column_check_writes_no_schema_when_columns_exist() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    let v = pragma(&t.db(), "schema_version");
    drop(Store::open_with_column_check(&t.db(), &[]).unwrap()); // the check runs; nothing is missing
    assert_eq!(pragma(&t.db(), "schema_version"), v);
    drop_new_trim_columns(&t.db());
    drop(Store::open_with_column_check(&t.db(), &[]).unwrap()); // the seam checks a path already checked
    assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
}

#[test] // review focus 21: the fallback hides no error other than a missing column
fn readbacks_schema_insert_trim_fallback_keeps_other_errors() {
    let t = Tmp::new();
    let s = Store::open(&t.db()).unwrap();
    Connection::open(t.db()).unwrap().execute_batch("DROP TABLE trims").unwrap();
    let e = s.insert_trim(&trim("S1", 1, 10, 1)).expect_err("a missing table is not a missing column").to_string();
    assert!(e.contains("no such table: trims"), "{e}");
    // A prepare error that a seven-column insert would not meet (the `Store` opens after the table exists, so the
    // error comes from prepare, not from a re-prepare at execute): the fallback must not write the row.
    let g = Tmp::new();
    Connection::open(g.db()).unwrap().execute_batch("CREATE TABLE trims (ts INTEGER NOT NULL, session TEXT NOT NULL, agent_id TEXT, tool TEXT NOT NULL, kind TEXT NOT NULL, stored_bytes INTEGER NOT NULL, view_bytes INTEGER NOT NULL, result_id TEXT GENERATED ALWAYS AS ('x') VIRTUAL, command TEXT, shaper TEXT)").unwrap();
    let e = Store::open(&g.db()).unwrap().insert_trim(&trim("S1", 1, 10, 1)).expect_err("a generated column is not a missing column").to_string();
    assert!(e.contains("cannot INSERT into generated column"), "{e}");
    assert_eq!(count(&g.db(), "trims"), 0);
}

#[test] // O§6.5 preamble, O-C8: the read-only open of each binary still opens the database, before and after the full open
fn readbacks_schema_open_read_only_opens_old_and_migrated_database() {
    let t = Tmp::new();
    old_db(&t.db());
    assert_eq!(Store::open_read_only(&t.db()).unwrap().bytes_trimmed("S1").unwrap(), 900);
    drop(Store::open(&t.db()).unwrap());
    assert_eq!(Store::open_read_only(&t.db()).unwrap().bytes_trimmed("S1").unwrap(), 900);
}

#[test] // review round 1 F3: a generated (hidden) `shaper` column is present; `open` runs no ALTER for it
fn readbacks_schema_generated_column_counts_as_present() {
    let t = Tmp::new();
    Connection::open(t.db()).unwrap().execute_batch("CREATE TABLE trims (ts INTEGER NOT NULL, session TEXT NOT NULL, agent_id TEXT, tool TEXT NOT NULL, kind TEXT NOT NULL, stored_bytes INTEGER NOT NULL, view_bytes INTEGER NOT NULL, result_id TEXT, command TEXT, shaper TEXT GENERATED ALWAYS AS ('x') VIRTUAL)").unwrap();
    drop(Store::open(&t.db()).unwrap()); // creates the other tables
    let v = pragma(&t.db(), "schema_version");
    drop(Store::open_with_column_check(&t.db(), &[]).unwrap());
    assert_eq!(pragma(&t.db(), "schema_version"), v);
}

// ---- OS-3: bounded open (O§7.11, O-C10) ----

#[test]
fn open_bounded_replaces_the_sqlite_busy_timeout() {
    // O§7.11: the open's own handler; sqlite3_busy_handler sets busyTimeout to 0, so SQLite's requested-time handler is off
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    assert_eq!(store::HOOK_DB_BOUND, Duration::from_millis(100));
    assert_eq!(Store::open_bounded(&t.db(), Duration::ZERO).unwrap().busy_timeout_ms().unwrap(), 0);
    assert_eq!(Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).unwrap().busy_timeout_ms().unwrap(), 0);
}

// ---- OS-45: the wait is bounded in elapsed time (O§7.11, O-C10); each instant is an offset from one origin (O§8.4) ----

#[test]
fn busy_wait_gives_up_at_bound_zero_without_a_sleep() {
    let t0 = Instant::now();
    assert_eq!(store::BusyWait::new(Duration::ZERO).next(0, t0), None);
}

#[test]
fn busy_wait_gives_up_when_elapsed_reaches_the_bound() {
    let ms = Duration::from_millis;
    let t0 = Instant::now();
    let mut w = store::BusyWait::new(store::HOOK_DB_BOUND);
    assert_eq!(w.next(0, t0), Some(ms(1)));
    assert_eq!(w.next(1, t0 + ms(2)), Some(ms(2)));
    // OS-38: at SQLite's 8th call 78 ms were requested but about 145 ms had passed
    assert_eq!(w.next(7, t0 + ms(145)), None);
    assert_eq!(w.next(2, t0 + store::HOOK_DB_BOUND), None);
}

#[test]
fn busy_wait_never_asks_a_sleep_past_the_bound() {
    let t0 = Instant::now();
    let b = store::HOOK_DB_BOUND;
    for count in 1..40 {
        for e in 0..=110u64 {
            let mut w = store::BusyWait::new(b);
            assert!(w.next(0, t0).is_some());
            let elapsed = Duration::from_millis(e);
            let got = w.next(count, t0 + elapsed);
            if elapsed >= b {
                assert_eq!(got, None, "count {count}, elapsed {e} ms");
            } else {
                let d = got.unwrap_or_else(|| panic!("count {count}, elapsed {e} ms: gave up before the bound"));
                assert!(!d.is_zero() && elapsed + d <= b, "count {count}, elapsed {e} ms, sleep {d:?}");
            }
        }
    }
}

#[test]
fn busy_wait_restarts_its_clock_at_count_zero() {
    let ms = Duration::from_millis;
    let t0 = Instant::now();
    let mut w = store::BusyWait::new(store::HOOK_DB_BOUND);
    assert_eq!(w.next(0, t0), Some(ms(1)));
    assert_eq!(w.next(5, t0 + ms(99)), Some(ms(1))); // step 20 ms, 1 ms left
    // a new busy statement: SQLite sets the count to 0 at each run (sqlite3VdbeExec), so the clock restarts
    assert_eq!(w.next(0, t0 + ms(500)), Some(ms(1)));
    assert_eq!(w.next(1, t0 + ms(502)), Some(ms(2)));
}

#[test]
fn busy_wait_open_bounded_waits_for_a_released_lock() {
    // The real handler waits for a released lock: the bound is `Duration::MAX`, so no deadline can fail the test
    // (O§8.4); the lock is released only after the insert is about to run, so the insert almost surely meets it.
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap(); // another process holds the write lock
    let (go, ready) = std::sync::mpsc::channel::<()>();
    let release = std::thread::spawn(move || {
        ready.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        raw.execute_batch("COMMIT").unwrap();
    });
    let s = Store::open_bounded(&t.db(), Duration::MAX).unwrap();
    go.send(()).unwrap();
    let r = s.insert_trim(&trim("S1", 5, 1000, 100));
    release.join().unwrap();
    r.expect("the bounded insert must wait for the released lock");
    assert_eq!(count(&t.db(), "trims"), 1);
}

#[test]
fn busy_wait_open_bounded_insert_is_busy_when_the_lock_is_never_released() {
    // The lock is never released, so the outcome does not depend on time: only the error kind is asserted.
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    let s = Store::open_bounded(&t.db(), Duration::from_millis(50)).unwrap();
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap();
    let e = s.insert_trim(&trim("S1", 5, 1000, 100)).expect_err("a lock held past the bound must fail the insert");
    let code = e.downcast_ref::<rusqlite::Error>().and_then(|e| e.sqlite_error_code());
    assert_eq!(code, Some(rusqlite::ErrorCode::DatabaseBusy), "{e}");
    raw.execute_batch("ROLLBACK").unwrap();
    assert_eq!(count(&t.db(), "trims"), 0);
}

#[test]
fn busy_wait_clocks_are_per_store() {
    let ms = Duration::from_millis;
    let t0 = Instant::now();
    let (mut a, mut b) = (store::BusyWait::new(store::HOOK_DB_BOUND), store::BusyWait::new(store::HOOK_DB_BOUND));
    assert_eq!(a.next(0, t0), Some(ms(1)));
    assert_eq!(b.next(0, t0 + ms(90)), Some(ms(1)));
    assert_eq!(a.next(3, t0 + ms(95)), Some(ms(5))); // a: 95 ms passed, step 10 ms, 5 ms left
    assert_eq!(b.next(3, t0 + ms(95)), Some(ms(10))); // b: 5 ms passed
    assert_eq!(a.next(4, t0 + ms(100)), None);
    assert_eq!(b.next(4, t0 + ms(100)), Some(ms(15)));
}

// ---- OS-46: the hook's wait is bounded in total for one call (O§7.11, O-C10); instants are offsets (O§8.4) ----

#[test]
fn busy_wait_total_gives_up_at_bound_zero_without_a_sleep() {
    let t0 = Instant::now();
    assert_eq!(store::BusyWait::total(Duration::ZERO).next(0, t0), None);
}

#[test]
fn busy_wait_total_keeps_its_clock_across_statements() {
    let ms = Duration::from_millis;
    let t0 = Instant::now();
    let mut w = store::BusyWait::total(store::HOOK_DB_BOUND);
    assert_eq!(w.next(0, t0), Some(ms(1))); // the open's read waits
    assert_eq!(w.next(1, t0 + ms(40)), Some(ms(2)));
    // the insert: SQLite passes count 0 again, but the call's clock does not restart
    assert_eq!(w.next(0, t0 + ms(60)), Some(ms(1)));
    assert_eq!(w.next(4, t0 + ms(95)), Some(ms(5))); // step 15 ms, 5 ms left
    assert_eq!(w.next(0, t0 + store::HOOK_DB_BOUND), None);
    assert_eq!(w.next(0, t0 + ms(500)), None); // a later statement: the call's bound stays spent
}

#[test]
fn busy_wait_total_never_asks_a_sleep_past_the_bound() {
    let t0 = Instant::now();
    let b = store::HOOK_DB_BOUND;
    for count in 0..40 {
        for e in 0..=110u64 {
            let mut w = store::BusyWait::total(b);
            assert!(w.next(0, t0).is_some());
            let elapsed = Duration::from_millis(e);
            let got = w.next(count, t0 + elapsed);
            if elapsed >= b {
                assert_eq!(got, None, "count {count}, elapsed {e} ms");
            } else {
                let d = got.unwrap_or_else(|| panic!("count {count}, elapsed {e} ms: gave up before the bound"));
                assert!(!d.is_zero() && elapsed + d <= b, "count {count}, elapsed {e} ms, sleep {d:?}");
            }
        }
    }
}

#[test]
fn open_bounded_total_replaces_the_sqlite_busy_timeout() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    assert_eq!(Store::open_bounded_total(&t.db(), store::HOOK_DB_BOUND).unwrap().busy_timeout_ms().unwrap(), 0);
}

#[test]
fn open_bounded_total_waits_for_a_released_lock_on_a_rollback_journal_database() {
    // The total open waits for a released lock: the bound is `Duration::MAX`, so no deadline can fail the test
    // (O§8.4); the lock is released only after the open is about to run, so its read almost surely meets it.
    let t = Tmp::new();
    old_db(&t.db()); // journal_mode delete
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN EXCLUSIVE").unwrap(); // blocks readers too in rollback-journal mode
    let (go, ready) = std::sync::mpsc::channel::<()>();
    let release = std::thread::spawn(move || {
        ready.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        raw.execute_batch("COMMIT").unwrap();
    });
    go.send(()).unwrap();
    let r = Store::open_bounded_total(&t.db(), Duration::MAX).and_then(|s| s.insert_trim(&trim("S4", 8, 4000, 400)));
    release.join().unwrap();
    r.expect("the total open must wait for the released lock");
    assert_eq!(count(&t.db(), "trims"), 2);
}

#[test]
fn open_bounded_total_is_busy_when_the_lock_is_never_released_on_a_rollback_journal_database() {
    // The lock is never released, so the outcome does not depend on time: only the error kind is asserted.
    let t = Tmp::new();
    old_db(&t.db());
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let e = Store::open_bounded_total(&t.db(), Duration::from_millis(50))
        .and_then(|s| s.insert_trim(&trim("S4", 8, 4000, 400)))
        .expect_err("a lock held past the bound must fail the call");
    let code = e.downcast_ref::<rusqlite::Error>().and_then(|e| e.sqlite_error_code());
    assert_eq!(code, Some(rusqlite::ErrorCode::DatabaseBusy), "{e}");
    raw.execute_batch("ROLLBACK").unwrap();
    assert_eq!(count(&t.db(), "trims"), 1);
}

#[test]
fn open_bounded_opens_under_a_held_write_lock() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap(); // another process holds the write lock
    // The open takes no write lock, so with bound 0 it still succeeds.
    assert!(Store::open_bounded(&t.db(), Duration::ZERO).is_ok());
    raw.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn open_bounded_insert_is_busy_at_bound_zero_under_a_held_write_lock() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    let s = Store::open_bounded(&t.db(), Duration::ZERO).unwrap();
    let raw = Connection::open(t.db()).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap();
    let e = s.insert_trim(&trim("S1", 5, 1000, 100)).expect_err("a held write lock with bound 0 must fail the insert");
    let code = e.downcast_ref::<rusqlite::Error>().and_then(|e| e.sqlite_error_code());
    assert_eq!(code, Some(rusqlite::ErrorCode::DatabaseBusy), "{e}");
    raw.execute_batch("ROLLBACK").unwrap();
    assert_eq!(count(&t.db(), "trims"), 0); // no row, also not through the seven-column fallback
}

#[test]
fn open_bounded_on_seven_column_database_adds_no_column() {
    let t = Tmp::new();
    old_db(&t.db());
    let before = pragma(&t.db(), "schema_version");
    let s = Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).unwrap();
    s.insert_trim(&trim("S2", 6, 2000, 200)).unwrap();
    drop(s);
    assert_eq!(cols(&t.db(), "trims"), TRIM_SEVEN);
    assert_eq!(pragma(&t.db(), "schema_version"), before);
    assert_eq!(count(&t.db(), "trims"), 2);
}

#[test]
fn open_bounded_on_missing_path_is_an_error_and_creates_nothing() {
    let t = Tmp::new();
    assert!(Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).is_err());
    assert!(!t.db().exists());
    let deep = t.0.join("no").join("such").join("usage.db");
    assert!(Store::open_bounded(&deep, store::HOOK_DB_BOUND).is_err());
    assert!(!t.0.join("no").exists());
}

#[test]
fn open_bounded_rejects_another_schema_version() {
    let t = Tmp::new();
    drop(Store::open(&t.db()).unwrap());
    Connection::open(t.db()).unwrap().execute_batch("PRAGMA user_version = 2").unwrap();
    let e = Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).err().expect("another schema version must fail").to_string();
    assert!(e.contains('2') && e.contains('1'), "{e}");
}

#[test]
fn open_bounded_leaves_the_journal_mode_unchanged() {
    let t = Tmp::new();
    old_db(&t.db());
    let mode = |db: &Path| Connection::open(db).unwrap().query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0)).unwrap();
    let before = mode(&t.db());
    assert_eq!(before, "delete");
    let s = Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).unwrap();
    s.insert_trim(&trim("S3", 7, 3000, 300)).unwrap();
    drop(s);
    assert_eq!(mode(&t.db()), before);
}
