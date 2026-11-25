use std::path::PathBuf;
use rusqlite::Connection;
use zygo_core::Agent;
use zygo_usage::*;

const R: Rates = Rates { input: 4.0, cache_read: 0.4, write: 5.0 };
const RID: &str = "01J00000000000000000000000";
fn rates(m: &str, _: &str, _: u64) -> Option<Rates> {
    match m {
        "m1" => Some(R),
        "m2" => Some(Rates { input: 40.0, cache_read: 4.0, write: 50.0 }),
        _ => None,
    }
}
struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let d = std::env::temp_dir().join(format!("zygo-savings-{}", zygo_core::ids::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
    fn db(&self) -> PathBuf { self.0.join("usage.db") }
}
impl Drop for Tmp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn row(s: &str, ts: i64, agent_id: Option<&str>, class: Option<&str>, model: &str, cr: u64, cw: u64) -> RequestRow {
    RequestRow { ts, session: s.into(), agent: "claude-code".into(), route: "/anthropic/v1/messages".into(),
        agent_id: agent_id.map(Into::into), request_class: class.map(Into::into), compaction: class == Some("compaction"),
        model: model.into(), provider: "p1".into(), status: 200, input_tokens: 100, cache_read_tokens: cr,
        cache_write_tokens: cw, ..Default::default() }
}
#[allow(clippy::too_many_arguments)]
fn req(st: &Store, s: &str, ts: i64, agent_id: Option<&str>, class: Option<&str>, model: &str, cr: u64, cw: u64) {
    st.insert_request(&row(s, ts, agent_id, class, model, cr, cw)).unwrap();
}
fn trim(s: &str, ts: i64, agent_id: Option<&str>, kept: u64, rid: Option<&str>) -> TrimRow {
    TrimRow { ts, session: s.into(), agent_id: agent_id.map(Into::into), tool: "Bash".into(), kind: "shaped".into(),
        stored_bytes: kept + 100, view_bytes: 100, result_id: rid.map(Into::into), command: Some("cargo test".into()),
        shaper: Some("libtest".into()) }
}
fn rb(s: &str, read_in: &str, ts: i64, bytes: u64, rid: &str) -> ReadbackRow {
    ReadbackRow { ts, session: s.into(), read_in: read_in.into(), result_id: rid.into(), tool: "result_view".into(), bytes }
}
fn sess(s: &str) -> Filter { Filter { session: Some(s.into()), since: None } }
fn near(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }
/// Two growing main-loop rows (write, then read: 5.4 per Mtok with m1) and two subagent `a1` rows with `sub_model`.
fn two_contexts(st: &Store, s: &str, ts: [i64; 2], main_model: &str, sub_model: &str) {
    for (t, cr) in [(ts[0], 1_000), (ts[1], 2_000)] {
        req(st, s, t, None, None, main_model, cr, 1_000);
        req(st, s, t, Some("a1"), None, sub_model, cr, 1_000);
    }
}
/// O§6.3 worked example in session "S": the rows of tests/carry.rs `example()`, the trim (40,000 bytes) at 100,
/// the read-back (4,000 bytes) at 155.
fn worked_example(st: &Store) {
    for (ts, cr, cw) in [(90, 10_000, 2_000), (110, 10_000, 12_000), (120, 22_000, 1_000), (130, 23_000, 1_000),
        (140, 24_000, 1_000), (150, 25_000, 1_000), (160, 0, 27_000), (170, 27_000, 500), (180, 27_500, 100)] {
        req(st, "S", ts, None, if ts == 170 { Some("compaction") } else { None }, "m1", cr, cw);
    }
    st.insert_trim(&trim("S", 100, None, 40_000, Some(RID))).unwrap();
    st.insert_readback(&rb("S", "S", 155, 4_000, RID)).unwrap();
}
fn key(tool: &str, command: &str, shaper: &str, views: u64, readbacks: u64, kept: i64, usd: f64) -> SavingsKey {
    SavingsKey { tool: tool.into(), command: command.into(), shaper: shaper.into(), views, readbacks, kept_out_bytes: kept,
        spend_usd: usd }
}
fn open(t: &Tmp) -> Store { Store::open(&t.db()).unwrap() }

#[test]
fn savings_worked_example_is_0_1146_and_9000_tokens() {
    let t = Tmp::new();
    let st = open(&t);
    worked_example(&st);
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, 0.1146), "{}", v.spend_usd);
    assert_eq!((v.kept_out_bytes, v.kept_out_tokens, v.bytes_read_back, v.views, v.readbacks, v.unpriced),
        (36_000, 9_000, 4_000, 1, 1, 0));
    assert_eq!(v.by_key.len(), 1);
    let k = &v.by_key[0];
    assert_eq!((k.tool.as_str(), k.command.as_str(), k.shaper.as_str(), k.views, k.readbacks, k.kept_out_bytes),
        ("Bash", "cargo test", "libtest", 1, 1, 36_000));
    assert!(near(k.spend_usd, 0.1146), "{}", k.spend_usd);
}

#[test]
fn savings_growing_context_reads_after_the_first_row() {
    let t = Tmp::new();
    let st = open(&t);
    for (ts, cr, cw) in [(10, 100, 1_000), (20, 1_100, 2_000), (30, 3_100, 4_000), (40, 7_100, 8_000)] {
        req(&st, "G", ts, None, None, "m1", cr, cw);
    }
    st.insert_trim(&trim("G", 5, None, 4_000_000, None)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, 5.0 + 3.0 * 0.4), "{}", v.spend_usd);
}

#[test]
fn savings_cut_before_the_first_request_adds_0() {
    let t = Tmp::new();
    let st = open(&t);
    for (ts, cr, cw) in [(10, 40_000, 10_000), (30, 0, 20_000), (40, 20_000, 1_000)] {
        req(&st, "C", ts, None, None, "m1", cr, cw);
    }
    st.insert_trim(&trim("C", 20, None, 40_000, None)).unwrap();
    let v = st.savings(&Filter { session: Some("C".into()), since: Some(20) }, &rates).unwrap();
    assert_eq!(v.spend_usd, 0.0);
    assert_eq!(v.kept_out_tokens, 10_000);
}

#[test]
fn savings_subagent_trim_carried_by_own_rows_only() {
    let t = Tmp::new();
    let st = open(&t);
    two_contexts(&st, "S", [10, 20], "m2", "m1");
    st.insert_trim(&trim("S", 5, Some("a1"), 4_000_000, None)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, 5.4), "{}", v.spend_usd);
}

#[test]
fn savings_auxiliary_row_carries_nothing() {
    let t = Tmp::new();
    let st = open(&t);
    req(&st, "A", 10, None, None, "m1", 1_000, 1_000);
    req(&st, "A", 20, None, Some("auxiliary"), "m2", 0, 100);
    req(&st, "A", 30, None, Some("main"), "m1", 2_000, 1_000);
    st.insert_trim(&trim("A", 5, None, 4_000_000, None)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, 5.4), "{}", v.spend_usd);
}

#[test]
fn savings_opencode_trim_counts_first_row_only() {
    let t = Tmp::new();
    let st = open(&t);
    for (ts, cr, cw) in [(10, 100, 1_000), (20, 1_100, 2_000), (30, 3_100, 4_000), (40, 7_100, 8_000)] {
        st.insert_request(&RequestRow { agent: Agent::OpenCode.as_str().into(), ..row("O", ts, None, None, "m1", cr, cw) })
            .unwrap();
    }
    st.insert_trim(&trim("O", 5, None, 4_000_000, None)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, 5.0), "{}", v.spend_usd);
}

#[test]
fn savings_readback_in_s2_of_s1_result() {
    let t = Tmp::new();
    let st = open(&t);
    req(&st, "S1", 110, Some("a1"), None, "m1", 1_000, 1_000);
    req(&st, "S1", 120, Some("a1"), None, "m1", 2_000, 1_000);
    req(&st, "S1", 210, None, None, "m2", 1_000, 1_000);
    req(&st, "S1", 220, None, None, "m2", 2_000, 1_000);
    two_contexts(&st, "S2", [210, 220], "m1", "m2");
    st.insert_trim(&trim("S1", 100, Some("a1"), 40_000, Some(RID))).unwrap();
    st.insert_readback(&rb("S1", "S2", 200, 4_000, RID)).unwrap();
    let v1 = st.savings(&sess("S1"), &rates).unwrap();
    assert!(near(v1.spend_usd, 0.054 - 0.0054), "{}", v1.spend_usd);
    assert_eq!((v1.readbacks, v1.bytes_read_back, v1.kept_out_bytes, v1.kept_out_tokens), (1, 4_000, 36_000, 9_000));
    assert_eq!(st.savings(&sess("S2"), &rates).unwrap(), Savings::default());
}

#[test]
fn savings_doubled_rates_double_spend() {
    let t = Tmp::new();
    let st = open(&t);
    worked_example(&st);
    let double = |m: &str, _: &str, _: u64| (m == "m1").then_some(Rates { input: 8.0, cache_read: 0.8, write: 10.0 });
    let v1 = st.savings(&Filter::default(), &rates).unwrap();
    let v2 = st.savings(&Filter::default(), &double).unwrap();
    assert!(near(v2.spend_usd, 0.2292), "{}", v2.spend_usd);
    assert!(near(v2.spend_usd, 2.0 * v1.spend_usd));
}

#[test]
fn savings_97d8767_database_reads_dash_keys() {
    let t = Tmp::new();
    let db = t.db();
    let c = Connection::open(&db).unwrap();
    c.execute_batch(SCHEMA).unwrap();
    c.execute_batch("DROP TABLE readbacks; ALTER TABLE trims DROP COLUMN result_id; ALTER TABLE trims DROP COLUMN command; \
        ALTER TABLE trims DROP COLUMN shaper; PRAGMA user_version = 1").unwrap();
    c.execute("INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (5, 'S1', NULL, 'Bash', 'head', 1000, 100)", []).unwrap();
    drop(c);
    let v = Store::open_read_only(&db).unwrap().savings(&Filter::default(), &rates).unwrap();
    assert_eq!(v.bytes_read_back, 0);
    assert_eq!(v.by_key, vec![key("Bash", "-", "-", 1, 0, 900, 0.0)]);
}

#[test]
fn savings_unpriced_counts_distinct_rowids() {
    let t = Tmp::new();
    let st = open(&t);
    for (ts, m, cr) in [(10, "m1", 1_000), (20, "unknown", 2_000), (30, "m1", 3_000)] {
        req(&st, "U", ts, None, None, m, cr, 1_000);
    }
    st.insert_trim(&trim("U", 5, None, 4_000_000, None)).unwrap();
    st.insert_trim(&trim("U", 6, None, 4_000_000, None)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert_eq!(v.unpriced, 1);
    assert!(near(v.spend_usd, 2.0 * 5.4), "{}", v.spend_usd);
}

#[test]
fn savings_no_request_rows_adds_0_keeps_tokens() {
    let t = Tmp::new();
    let st = open(&t);
    st.insert_trim(&trim("N", 5, None, 40_000, None)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert_eq!((v.spend_usd, v.kept_out_tokens, v.unpriced), (0.0, 10_000, 0));
}

#[test]
fn savings_readback_without_trim_row_is_dash_key_in_main_loop() {
    let t = Tmp::new();
    let st = open(&t);
    two_contexts(&st, "R", [10, 20], "m1", "m2");
    st.insert_readback(&rb("R", "R", 5, 4_000, RID)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, -0.0054), "{}", v.spend_usd);
    assert_eq!((v.kept_out_bytes, v.kept_out_tokens), (-4_000, -1_000));
    assert_eq!(v.by_key.len(), 1);
    let k = &v.by_key[0];
    assert_eq!((k.tool.as_str(), k.command.as_str(), k.shaper.as_str(), k.readbacks), ("-", "-", "-", 1));
}

#[test]
fn savings_readback_in_trim_session_carried_by_trim_context() {
    let t = Tmp::new();
    let st = open(&t);
    two_contexts(&st, "S", [10, 20], "m2", "m1");
    st.insert_trim(&trim("S", 5, Some("a1"), 40_000, Some(RID))).unwrap();
    st.insert_readback(&rb("S", "S", 15, 4_000, RID)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert!(near(v.spend_usd, 0.054 - 0.005), "{}", v.spend_usd);
}

#[test]
fn savings_by_key_sorted_largest_first() {
    let t = Tmp::new();
    let st = open(&t);
    st.insert_trim(&TrimRow { command: Some("git diff".into()), shaper: Some("diff".into()), ..trim("K", 1, None, 100, None) })
        .unwrap();
    st.insert_trim(&trim("K", 1, None, 300, None)).unwrap();
    st.insert_trim(&TrimRow { tool: "Read".into(), command: None, shaper: Some("size".into()), ..trim("K", 1, None, 200, None) })
        .unwrap();
    st.insert_readback(&rb("K", "K", 1, 50, RID)).unwrap();
    let v = st.savings(&Filter::default(), &rates).unwrap();
    assert_eq!(v.by_key, vec![key("Bash", "cargo test", "libtest", 1, 0, 300, 0.0), key("Read", "-", "size", 1, 0, 200, 0.0),
        key("Bash", "git diff", "diff", 1, 0, 100, 0.0), key("-", "-", "-", 0, 1, -50, 0.0)]);
}
