//! OS-37: the O§8.5 latency budget, on demand, never in CI (O-C6). Run: cargo test -p zygo-hooks --release --test shaping_latency -- --ignored
mod common {
    pub mod shaping;
}
use common::shaping::{self, Case};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use zygo_core::{
    Agent,
    hook::{guarded, HookInput},
    session_dir::SessionDir,
    session_view::SessionView,
};
use zygo_hooks::trim::{self, TrimOpts};
use zygo_results::shape::{self, command, ShapeInput};
use zygo_usage::store::HOOK_DB_BOUND;

const RUNS: usize = 200; // O§8.5
const STEPS_SMALL_BYTES: usize = 64 * 1024; // O§8.5
const STEPS_SMALL_P95: Duration = Duration::from_millis(5); // O§8.5
const STEPS_LARGE_BYTES: usize = 4 * 1024 * 1024; // O§8.5
const STEPS_LARGE_P95: Duration = Duration::from_millis(100); // O§8.5
const PROCESS_EXTRA_P95: Duration = Duration::from_millis(50); // O§8.5: shape on over shape off
const LOCKED_EXTRA_P95: Duration = Duration::from_millis(50); // O§8.5: over HOOK_DB_BOUND
const CHILD: &str = "ZYGO_OS37_LATENCY_CHILD";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Nearest rank: the `q`-th percentile of `sorted` (Decision 6).
fn percentile(sorted: &[Duration], q: usize) -> Duration {
    sorted[(q * sorted.len()).div_ceil(100) - 1]
}

/// Writes p50 and p95 of `times` to stderr, not captured by libtest (Decision 7); returns p95.
fn report(label: &str, mut times: Vec<Duration>) -> Duration {
    assert_eq!(times.len(), RUNS);
    times.sort();
    let (p50, p95) = (percentile(&times, 50), percentile(&times, 95));
    writeln!(std::io::stderr(), "{label}: p50 {p50:?} p95 {p95:?}").unwrap();
    p95
}

/// The case's `stdout` then `stderr`, repeated, cut to at most `n` bytes at a char boundary (Decision 2).
fn sized(c: &Case, n: usize) -> String {
    let base = format!("{}{}", String::from_utf8_lossy(&c.stdout), String::from_utf8_lossy(&c.stderr));
    assert!(!base.is_empty(), "{}/{}: no text", c.shaper, c.name);
    let mut s = base.repeat(n.div_ceil(base.len()));
    let mut end = n.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s
}

/// The case with the most raw bytes; Claude Code `PostToolUse` (Decision 4).
fn largest() -> Case {
    let c = shaping::cases(None).into_iter().max_by_key(|c| c.stdout.len() + c.stderr.len()).unwrap();
    assert_eq!(c.meta["agent"], "claude-code", "{}/{}", c.shaper, c.name);
    assert_eq!(c.meta["event"], "PostToolUse", "{}/{}", c.shaper, c.name);
    c
}

/// A temporary root: session `root/s` with `shape`, a fresh `root/usage.db`.
fn session(shape: bool) -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView {
        session: "sess-1".into(),
        agent: Agent::ClaudeCode,
        results: true,
        shape,
        trim_threshold_bytes: SessionView::default().trim_threshold_bytes,
        head_lines: 60,
        tail_lines: 20,
        ..Default::default()
    }
    .save(&dir)
    .unwrap();
    drop(zygo_usage::Store::open(&t.path().join("usage.db")).unwrap());
    t
}

/// The wall time of one hook process on `stdin` with root `root` (Decision 3).
fn hook_process(root: &Path, stdin: &[u8]) -> Duration {
    let start = Instant::now();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_trim_process", "--nocapture", "--test-threads=1"])
        .env(CHILD, root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let o = child.wait_with_output().unwrap();
    let t = start.elapsed();
    assert!(o.status.success(), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stdout).contains("child_trim_process ... ok"), "{o:?}");
    t
}

/// Times `shape::run` on each case cut to `n` bytes; returns the cases whose p95 is over `budget`.
fn steps_over(n: usize, budget: Duration) -> Vec<String> {
    let mut over = vec![];
    for c in shaping::cases(None) {
        let text = sized(&c, n);
        assert!(text.len() > n - 4, "{}/{}: {} bytes", c.shaper, c.name, text.len());
        assert!(text.len() <= shape::SHAPE_MAX_BYTES, "{}/{}: steps 1-4 skipped", c.shaper, c.name);
        let si = ShapeInput {
            parsed: Some(command::parse(c.meta["command"].as_str().unwrap_or(""))),
            kind: zygo_results::kind::sniff(text.as_bytes()),
            threshold: SessionView::default().trim_threshold_bytes,
        };
        let times = (0..RUNS)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(shape::run(std::hint::black_box(&text), &si));
                start.elapsed()
            })
            .collect();
        let label = format!("steps {} KiB {}/{}", n / 1024, c.shaper, c.name);
        let p95 = report(&label, times);
        if p95 > budget {
            over.push(format!("{label}: p95 {p95:?}"));
        }
    }
    over
}

#[test]
#[ignore = "timing: shaping latency budget, run on demand"]
fn shaping_steps_on_64_kib_p95_within_5_ms() {
    let _g = serial();
    let over = steps_over(STEPS_SMALL_BYTES, STEPS_SMALL_P95);
    assert!(over.is_empty(), "O§8.5 steps on 64 KiB, p95 > 5 ms: {over:?}");
}

#[test]
#[ignore = "timing: shaping latency budget, run on demand"]
fn shaping_steps_on_4_mib_p95_within_100_ms() {
    let _g = serial();
    let over = steps_over(STEPS_LARGE_BYTES, STEPS_LARGE_P95);
    assert!(over.is_empty(), "O§8.5 steps on 4 MiB, p95 > 100 ms: {over:?}");
}

#[test]
#[ignore = "timing: shaping latency budget, run on demand"]
fn hook_process_shape_on_p95_within_50_ms_of_shape_off() {
    let _g = serial();
    let input = serde_json::to_vec(&shaping::hook_input(&largest())).unwrap();
    let (on, off) = (session(true), session(false));
    let (mut on_t, mut off_t) = (vec![], vec![]);
    for _ in 0..RUNS {
        off_t.push(hook_process(off.path(), &input));
        on_t.push(hook_process(on.path(), &input));
    }
    let (off_p95, on_p95) = (report("process shape=false", off_t), report("process shape=true", on_t));
    assert!(on_p95 <= off_p95 + PROCESS_EXTRA_P95, "O§8.5: p95 {on_p95:?} > {off_p95:?} + 50 ms");
}

#[test]
#[ignore = "timing: shaping latency budget, run on demand"]
fn hook_process_under_write_lock_p95_within_hook_db_bound_plus_50_ms() {
    let _g = serial();
    let input = serde_json::to_vec(&shaping::hook_input(&largest())).unwrap();
    let t = session(true);
    let lock = rusqlite::Connection::open(t.path().join("usage.db")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let times = (0..RUNS).map(|_| hook_process(t.path(), &input)).collect();
    lock.execute_batch("ROLLBACK").unwrap();
    let rows: i64 = lock.query_row("SELECT COUNT(*) FROM trims", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0); // the lock was held for every run
    let p95 = report("process with the write lock held", times);
    assert!(p95 <= HOOK_DB_BOUND + LOCKED_EXTRA_P95, "O§8.5: p95 {p95:?} > HOOK_DB_BOUND + 50 ms");
}

/// The hook process: what `zygo hook trim` does, in root `CHILD`; returns at once without `CHILD` (Decision 3).
#[test]
#[ignore = "timing: shaping latency budget, run on demand"]
fn child_trim_process() {
    let Some(root) = std::env::var_os(CHILD) else { return };
    let root = Path::new(&root);
    let mut stdin = vec![];
    std::io::stdin().read_to_end(&mut stdin).unwrap();
    let dir = SessionDir::new(root.join("s"));
    let db = root.join("usage.db");
    // explicit: `TrimOpts::default()` reads `ZYGO_TEST_SHAPE_PANIC`
    let opts = TrimOpts { db_bound: HOOK_DB_BOUND, stderr_cleared: trim::PO2_STDERR_CLEARED, shape_panic: false };
    let (out, code) = guarded(&|b| trim::trim_in_with(&HookInput::parse(Agent::ClaudeCode, b)?, &dir, &db, &opts), &stdin);
    assert_eq!(code, 0);
    assert!(out.contains("\"updatedToolOutput\""), "{out}"); // the hook gave a view, shape on or off, lock or not
}
