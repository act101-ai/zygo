use std::cell::Cell;
use std::path::PathBuf;
use zygo_usage::*;

const R: Rates = Rates { input: 4.0, cache_read: 0.4, write: 5.0 };
struct Tmp(PathBuf);
impl Drop for Tmp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn trim(ts: i64, kept: u64) -> TrimRow {
    TrimRow { ts, session: "S".into(), agent_id: None, tool: "Bash".into(), kind: "shaped".into(),
        stored_bytes: kept + 100, view_bytes: 100, result_id: None, command: Some("cargo test".into()),
        shaper: Some("libtest".into()) }
}

/// FX-8: a trim written by a second connection while `report` runs (from inside the rates callback) shows in
/// neither the summary nor the savings figures.
#[test]
fn report_reads_summary_and_savings_from_one_snapshot() {
    let t = Tmp(std::env::temp_dir().join(format!("zygo-report-{}", zygo_core::ids::ulid())));
    let db = t.0.join("usage.db");
    let w = Store::open(&db).unwrap();
    // One main-loop request at 10 (rate `write` = 5.0 per Mtok) carries the 4,000,000-byte trim at 5.
    w.insert_request(&RequestRow { ts: 10, session: "S".into(), agent: "claude-code".into(),
        route: "/anthropic/v1/messages".into(), model: "m1".into(), provider: "p1".into(), status: 200,
        input_tokens: 100, cache_write_tokens: 1_000, ..Default::default() }).unwrap();
    w.insert_trim(&trim(5, 4_000_000)).unwrap();
    let r = Store::open_read_only(&db).unwrap();
    let calls = Cell::new(0u32);
    let price = |m: &str, _: &str, _: u64| {
        if calls.replace(calls.get() + 1) == 0 {
            w.insert_trim(&trim(6, 1_000)).unwrap(); // second connection, committed mid-report
        }
        (m == "m1").then_some(R)
    };
    let (s, v) = r.report(&Filter::default(), &price).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!((s.requests, s.input_tokens, s.cache_write_tokens), (1, 100, 1_000));
    assert_eq!(s.bytes_trimmed, 4_000_000);
    assert_eq!((v.kept_out_bytes, v.kept_out_tokens, v.views, v.readbacks, v.unpriced), (4_000_000, 1_000_000, 1, 0, 0));
    assert!((v.spend_usd - 5.0).abs() < 1e-9, "{}", v.spend_usd);
    assert_eq!(v.by_key.len(), 1);
    assert_eq!(v.by_key[0].kept_out_bytes, 4_000_000);
    // Control: the write committed; a new read sees it.
    assert_eq!(r.summary(&Filter::default()).unwrap().bytes_trimmed, 4_001_000);
}
