//! The MCP result tools record one read-back row per successful reply (OS-5; O§6.4, O§6.5 item 2, O§10, O§16 item 14).

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde_json::json;
use zygo_mcp::tools::results::{tools_with, Readbacks};
use zygo_mcp::{Tool, ToolError};

const READ_IN: &str = "S-read";
const TRIM: &str = "S-trim";
type Row = (i64, String, String, String, String, i64); // ts, session, read_in, result_id, tool, bytes

struct Env {
    _t: tempfile::TempDir,
    dir: PathBuf,
    db: PathBuf,
}

/// `dir` = <tmp>/results; `db` = <tmp>/usage.db, created by the full `zygo_usage::Store::open`, as the launcher does.
fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let db = t.path().join("usage.db");
    drop(zygo_usage::Store::open(&db).unwrap());
    Env { _t: t, dir, db }
}

fn tools(e: &Env, bound: Duration) -> Vec<Box<dyn Tool>> {
    tools_with(e.dir.clone(), Some(Readbacks::new(e.db.clone(), READ_IN.into(), bound)))
}

fn by_name<'a>(t: &'a [Box<dyn Tool>], n: &str) -> &'a dyn Tool {
    t.iter().find(|x| x.def().name == n).map(|b| b.as_ref()).unwrap_or_else(|| panic!("no tool {n}"))
}

fn call(t: &dyn Tool, args: serde_json::Value) -> Result<String, ToolError> {
    t.call(args).map(|v| v.as_str().expect("a string reply").to_string())
}

fn rows(db: &std::path::Path) -> Vec<Row> {
    let c = Connection::open(db).unwrap();
    let mut st = c.prepare("SELECT ts, session, read_in, result_id, tool, bytes FROM readbacks ORDER BY rowid").unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).unwrap().map(Result::unwrap).collect()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

/// 500 lines `row {i} payload\n`, i = 1..=500.
fn text() -> String {
    (1..=500).map(|i| format!("row {i} payload\n")).collect()
}

#[test]
fn readback_view_writes_one_row_with_the_reply_bytes() {
    let e = env();
    let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), text().as_bytes()).unwrap();
    let t = tools(&e, zygo_usage::store::HOOK_DB_BOUND);
    let before = now();
    let reply = call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap();
    let after = now();
    let first_400: Vec<String> = (1..=400).map(|i| format!("row {i} payload")).collect();
    assert_eq!(reply, first_400.join("\n"));
    let r = rows(&e.db);
    assert_eq!(r.len(), 1);
    assert!((before..=after).contains(&r[0].0), "{r:?}");
    assert_eq!((&r[0].1[..], &r[0].2[..], &r[0].3[..], &r[0].4[..], r[0].5), (TRIM, READ_IN, &s.id[..], "result_view", reply.len() as i64));
}

#[test]
fn readback_without_meta_session_takes_the_reading_session() {
    let e = env();
    let t = tools(&e, zygo_usage::store::HOOK_DB_BOUND);
    let s = zygo_results::Store::put(&e.dir, "Bash", None, text().as_bytes()).unwrap();
    let reply = call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "61-120"})).unwrap();
    let want: Vec<String> = (61..=120).map(|i| format!("row {i} payload")).collect();
    assert_eq!(reply, want.join("\n"));
    let r = rows(&e.db);
    assert_eq!(r.len(), 1);
    assert_eq!((&r[0].1[..], &r[0].2[..], &r[0].3[..], &r[0].4[..], r[0].5), (READ_IN, READ_IN, &s.id[..], "result_view", reply.len() as i64));
}

#[test]
fn readback_search_and_tree_each_write_one_row() {
    let e = env();
    let t = tools(&e, zygo_usage::store::HOOK_DB_BOUND);
    let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), text().as_bytes()).unwrap();
    let j = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), br#"{"b":{"a":[1,2]}}"#).unwrap();
    let found = call(by_name(&t, "result_search"), json!({"id": s.id, "pattern": "row 7 "})).unwrap();
    assert_eq!(found, "6- row 6 payload\n7: row 7 payload\n8- row 8 payload");
    let tree = call(by_name(&t, "result_tree"), json!({"id": j.id})).unwrap();
    assert_eq!(tree, "b: object\n  a: array[2]");
    let r: Vec<(String, String, String, String, i64)> = rows(&e.db).into_iter().map(|r| (r.1, r.2, r.3, r.4, r.5)).collect();
    assert_eq!(
        r,
        [
            (TRIM.to_string(), READ_IN.to_string(), s.id.clone(), "result_search".to_string(), found.len() as i64),
            (TRIM.to_string(), READ_IN.to_string(), j.id.clone(), "result_tree".to_string(), 23),
        ]
    );
}

#[test]
fn readback_error_replies_write_no_row() {
    let e = env();
    let t = tools(&e, zygo_usage::store::HOOK_DB_BOUND);
    let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), b"a\nb\n").unwrap();
    let other = tempfile::tempdir().unwrap();
    let unknown = zygo_results::Store::put(other.path(), "Bash", None, b"x\n").unwrap().id; // a valid id, absent from e.dir
    let (v, se, tr) = (by_name(&t, "result_view"), by_name(&t, "result_search"), by_name(&t, "result_tree"));
    assert!(matches!(call(v, json!({"id": "../x"})), Err(ToolError::InvalidParams(_)))); // bad id
    assert!(matches!(call(v, json!({"id": unknown})), Err(ToolError::Failed(_)))); // unknown id
    assert!(matches!(call(tr, json!({"id": unknown})), Err(ToolError::Failed(_))));
    assert!(matches!(call(se, json!({"id": unknown, "pattern": "a"})), Err(ToolError::Failed(_))));
    for lines in ["0-5", "1-401", "abc"] {
        // bad range
        assert!(matches!(call(v, json!({"id": s.id, "lines": lines})), Err(ToolError::InvalidParams(_))), "{lines}");
    }
    assert!(matches!(call(se, json!({"id": s.id, "pattern": "("})), Err(ToolError::InvalidParams(_)))); // bad pattern
    assert!(matches!(call(se, json!({"id": s.id})), Err(ToolError::InvalidParams(_)))); // no pattern
    assert_eq!(rows(&e.db), Vec::<Row>::new());
}

#[test]
fn readback_busy_store_keeps_the_reply_and_writes_no_row() {
    let e = env();
    let s = zygo_results::Store::put(&e.dir, "Bash", Some(TRIM), text().as_bytes()).unwrap();
    let t = tools(&e, Duration::ZERO);
    let raw = Connection::open(&e.db).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap(); // a second connection holds the write lock
    let locked = call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "61-120"})).unwrap();
    assert!(matches!(call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "0-5"})), Err(ToolError::InvalidParams(_))));
    assert_eq!(rows(&e.db).len(), 0); // WAL: a reader is not blocked by the held lock
    raw.execute_batch("ROLLBACK").unwrap();
    let free = call(by_name(&t, "result_view"), json!({"id": s.id, "lines": "61-120"})).unwrap();
    assert_eq!(locked, free); // byte-equal to the reply with no lock
    let r = rows(&e.db);
    assert_eq!(r.len(), 1); // only the reply after the lock ended
    assert_eq!(r[0].5, free.len() as i64);
}

#[test]
fn readback_missing_store_creates_nothing_and_opens_again_at_the_next_readback() {
    let t0 = tempfile::tempdir().unwrap();
    let dir = t0.path().join("results");
    let db = t0.path().join("data").join("usage.db");
    let s = zygo_results::Store::put(&dir, "Bash", Some(TRIM), b"a\nb\n").unwrap();
    let t = tools_with(dir, Some(Readbacks::new(db.clone(), READ_IN.into(), zygo_usage::store::HOOK_DB_BOUND)));
    assert_eq!(call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap(), "a\nb");
    assert!(!t0.path().join("data").exists()); // no directory, no file
    drop(zygo_usage::Store::open(&db).unwrap());
    assert_eq!(call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap(), "a\nb");
    let r = rows(&db);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].5, 3);
}

/// Parent half: runs `tools_child` in a new process with a scratch `HOME`, so `Paths::from_env` never names the real
/// usage store.
#[test]
fn tools_takes_the_session_view_and_the_home_usage_db() {
    let h = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tools_child", "--nocapture", "--test-threads=1"])
        .env("ZYGO_READBACKS_CHILD", "1")
        .env("HOME", h.path())
        .env("LOCALAPPDATA", h.path())
        .env("APPDATA", h.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("1 passed"), "{stdout}");
}

/// Child half: runs only when the parent sets `ZYGO_READBACKS_CHILD`.
#[test]
fn tools_child() {
    if std::env::var_os("ZYGO_READBACKS_CHILD").is_none() {
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let db = zygo_core::paths::Paths::from_env().usage_db;
    assert!(db.starts_with(&home), "{}", db.display());
    drop(zygo_usage::Store::open(&db).unwrap());
    let sd = zygo_core::session_dir::SessionDir::new(home.join("s"));
    sd.create().unwrap();
    zygo_core::session_view::SessionView { session: "S-view".into(), ..Default::default() }.save(&sd).unwrap();
    let s = zygo_results::Store::put(&sd.results(), "Bash", Some(TRIM), b"a\nb\n").unwrap();
    let ctx = zygo_mcp::McpCtx { agent: zygo_core::Agent::ClaudeCode, features: zygo_config::Features::default(), session_dir: Some(sd) };
    let t = zygo_mcp::tools::results::tools(&ctx);
    assert_eq!(call(by_name(&t, "result_view"), json!({"id": s.id})).unwrap(), "a\nb");
    let r = rows(&db);
    assert_eq!(r.len(), 1);
    assert_eq!((&r[0].1[..], &r[0].2[..], &r[0].3[..], &r[0].4[..], r[0].5), (TRIM, "S-view", &s.id[..], "result_view", 3));
}
