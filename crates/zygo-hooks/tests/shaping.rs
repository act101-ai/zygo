//! OS-35, OS-36: each shaping case through the trim hook: goldens, floors, store recovery, bless guard, never-worse check
//! (O§8.1, O§8.2, O§8.3, O§8.4, O§16 items 1, 2 and 3).
mod common {
    pub mod shaping;
}
use common::shaping::{self, Case};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use zygo_core::{
    Agent,
    hook::HookInput,
    session_dir::SessionDir,
    session_view::SessionView,
};
use zygo_config::ResultsConfig;
use zygo_hooks::trim::{self, TrimOpts};
use zygo_results::shape::{clean::clean, command::Runner, dedup::dedup, runner, MIN_SAVING_BYTES, SHAPE_MAX_BYTES};
use zygo_results::view::size_view;

/// The masked result id (O§8.1).
const ZERO_ID: &str = "00000000000000000000000000";

/// The O§8.2 row a case is checked against.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Floor {
    Clean,
    DedupWarning,
    LibtestPassing,
    Failing,
    Capped,
    NoChange,
    /// O§8.2 has no row for a passing pytest or go test run.
    NoRow,
    /// A Claude Code `PostToolUseFailure` case in `CASES`; checked as `FAILED_CC`.
    FailedCc,
}

/// O§7.13 "If PO1 fails", O§8.2: a Claude Code `PostToolUseFailure` case.
const FAILED_CC: Floor = if trim::PO1_HOLDS { Floor::Failing } else { Floor::NoChange };

/// Each O§8.1 table row's case folders, with the O§8.2 row each is checked against.
const CASES: &[(&str, &str, Floor)] = &[
    ("clean", "cargo-test-color", Floor::Clean),
    ("clean", "git-clone-progress", Floor::Clean),
    ("clean", "git-diff-color-crlf", Floor::Clean),
    // O§8.1 "no change"
    ("dedup", "already-repeated", Floor::NoChange),
    ("dedup", "build-repeated-warning", Floor::DedupWarning),
    ("diff", "git-diff-repo", Floor::Capped),
    ("failure", "cargo-test-failing", Floor::FailedCc),
    ("failure", "short", Floor::FailedCc),
    ("gotest", "failing-parallel", Floor::FailedCc),
    ("gotest", "panic", Floor::FailedCc),
    ("gotest", "passing-verbose", Floor::NoRow),
    ("json", "cargo-metadata", Floor::Capped),
    // no runner summary
    ("libtest", "compile-error", Floor::NoChange),
    ("libtest", "failing-nocapture", Floor::Failing),
    ("libtest", "passing", Floor::LibtestPassing),
    ("libtest", "signal-crash", Floor::Failing),
    ("log", "errors-in-middle", Floor::Capped),
    ("pytest", "collection-error", Floor::FailedCc),
    ("pytest", "failing-verbose", Floor::FailedCc),
    ("pytest", "passing", Floor::NoRow),
];

/// The sorted Bash fixtures of `fixtures/claude-code/hooks/`: the O§8.2 `none` row's inputs.
fn bash_fixtures() -> Vec<PathBuf> {
    let d = shaping::repo().join("fixtures/claude-code/hooks");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && p.file_stem().unwrap().to_string_lossy().split('-').nth(1) == Some("Bash")
        })
        .collect();
    v.sort();
    v
}

#[test]
fn required_cases_present() {
    let have: BTreeSet<(String, String)> = shaping::cases(None).into_iter().map(|c| (c.shaper, c.name)).collect();
    let want: BTreeSet<(String, String)> = CASES.iter().map(|(s, n, _)| (s.to_string(), n.to_string())).collect();
    assert_eq!(have, want);
    for s in ["clean", "dedup", "libtest", "pytest", "gotest", "diff", "json", "log", "failure"] {
        assert!(CASES.iter().any(|(t, _, _)| *t == s), "O§8.1 row {s} has no case");
    }
    // passing, failing, signal, compile error
    assert!(CASES.iter().filter(|(s, _, _)| *s == "libtest").count() >= 4);
    assert!(CASES.iter().filter(|(s, _, _)| *s == "pytest" || *s == "gotest").count() >= 6);
    // the none row's inputs
    assert_eq!(bash_fixtures().len(), 6);
    // F8: a case is listed as a failed Claude Code call exactly when its meta.json says so
    for c in shaping::cases(None) {
        let listed = CASES.iter().any(|(s, n, f)| *s == c.shaper && *n == c.name && *f == Floor::FailedCc);
        let is = c.meta["agent"] == "claude-code" && c.meta["event"] == "PostToolUseFailure";
        assert_eq!(listed, is, "{}/{}: listed as FailedCc {listed}, meta says {is}", c.shaper, c.name);
    }
}

/// The `ResultsConfig` of `env`: the default threshold, head 60, tail 20, shape on (§10.1).
fn results_cfg() -> ResultsConfig {
    ResultsConfig { trim_threshold_bytes: SessionView::default().trim_threshold_bytes, head_lines: 60, tail_lines: 20, shape: true }
}

/// A temporary session as `shaping_hook.rs` builds it: `results`, `shape`, the default threshold; a fresh usage db.
struct Env {
    _t: tempfile::TempDir,
    dir: SessionDir,
    db: PathBuf,
    agent: Agent,
}

fn env(agent: Agent) -> Env {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView {
        session: "sess-1".into(),
        agent,
        results: true,
        shape: true,
        trim_threshold_bytes: SessionView::default().trim_threshold_bytes,
        head_lines: results_cfg().head_lines,
        tail_lines: results_cfg().tail_lines,
        ..Default::default()
    }
    .save(&dir)
    .unwrap();
    let db = t.path().join("usage.db");
    drop(zygo_usage::Store::open(&db).unwrap());
    Env { _t: t, dir, db, agent }
}

/// The hook stdout of `input`, with the PO2 branch and no panic seam set explicitly. Calls `trim_in_with`
/// directly: `guarded` would turn an error or a panic into "no change" and swaps the process panic hook.
fn run(e: &Env, input: &[u8]) -> String {
    // explicit: `TrimOpts::default()` reads `ZYGO_TEST_SHAPE_PANIC`
    let opts = TrimOpts { db_bound: zygo_usage::store::HOOK_DB_BOUND, stderr_cleared: trim::PO2_STDERR_CLEARED, shape_panic: false };
    trim::trim_in_with(&HookInput::parse(e.agent, input).unwrap(), &e.dir, &e.db, &opts).unwrap().to_stdout()
}

/// (stored_bytes, view_bytes, shaper, result_id) of a `trims` row.
type Row = (u64, u64, String, Option<String>);

/// The stored result ids.
fn ids(e: &Env) -> Vec<String> {
    std::fs::read_dir(e.dir.results())
        .map(|d| d.map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| zygo_results::is_id(n)).collect())
        .unwrap_or_default()
}

/// (stored_bytes, view_bytes, shaper, result_id) of each `trims` row, by rowid.
fn rows(e: &Env) -> Vec<Row> {
    let c = rusqlite::Connection::open(&e.db).unwrap();
    let mut q = c.prepare("SELECT stored_bytes, view_bytes, shaper, result_id FROM trims ORDER BY rowid").unwrap();
    q.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn none_row_bash_fixtures_give_no_change() {
    // O§8.2 `none`: each Bash fixture of `fixtures/claude-code/hooks/`
    for p in bash_fixtures() {
        let e = env(Agent::ClaudeCode);
        assert_eq!(run(&e, &std::fs::read(&p).unwrap()), "", "{}", p.display());
        assert!(rows(&e).is_empty() && ids(&e).is_empty(), "{}", p.display());
    }
}

/// O§8.2: the saving `stored - view` is at least `pct` % of `stored` (integers, the row's bytes, O§7.8).
fn meets(stored: u64, view: u64, pct: u64) -> bool {
    view <= stored && (stored - view) * 100 >= pct * stored
}

/// Decision 4: some line equals `line`, or, in the log view, `"{n}: {line}"` for a decimal `n` (O§8.1, O§8.2).
fn holds(lines: &[&str], line: &str, log: bool) -> bool {
    lines.iter().any(|l| {
        *l == line
            || (log
                && l.split_once(": ").is_some_and(|(n, rest)| rest == line && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
    })
}

#[test]
fn saving_floor_is_exact_at_the_boundary() {
    assert!(meets(1000, 200, 80));
    assert!(!meets(1000, 201, 80));
    assert!(meets(1000, 700, 30));
    assert!(!meets(1000, 701, 30));
    assert!(meets(100, 95, 5));
    assert!(!meets(100, 96, 5));
    assert!(meets(1000, 500, 50));
    assert!(!meets(1000, 501, 50));
}

#[test]
fn holds_accepts_a_numbered_line_only_in_the_log_view() {
    assert!(holds(&["ERROR x"], "ERROR x", false));
    assert!(holds(&["212: ERROR x"], "ERROR x", true));
    assert!(!holds(&["212: ERROR x"], "ERROR x", false));
    assert!(!holds(&["a: ERROR x"], "ERROR x", true));
}

/// What the hook gave for one case.
#[derive(Clone)]
struct Got {
    /// The hook stdout, stored id masked.
    out: String,
    /// `updatedToolOutput.stdout`, id masked; "" when the hook gives no output.
    view: String,
    /// `updatedToolOutput.stderr` when it is a string.
    stderr_out: Option<String>,
    /// (stored_bytes, view_bytes, shaper, result_id) of the one `trims` row.
    row: Option<Row>,
    /// (stored id, `Store::get(..).data`).
    stored: Option<(String, Vec<u8>)>,
}

/// The O§8.2 row of `c`; panics naming a case that `CASES` does not list.
fn floor_of(c: &Case) -> Floor {
    CASES
        .iter()
        .find(|(s, n, _)| *s == c.shaper && *n == c.name)
        .map(|(_, _, f)| if *f == Floor::FailedCc { FAILED_CC } else { *f })
        .unwrap_or_else(|| panic!("{}/{}: not in CASES", c.shaper, c.name))
}

/// Runs `c` through the hook in a fresh session (Decisions 1, 2).
fn run_case(c: &Case) -> Got {
    let agent = if c.meta["agent"] == "opencode" { Agent::OpenCode } else { Agent::ClaudeCode };
    let e = env(agent);
    let out = run(&e, &serde_json::to_vec(&shaping::hook_input(c)).unwrap());
    let ids = ids(&e);
    assert!(ids.len() <= 1, "{}/{}: {ids:?}", c.shaper, c.name);
    let rows = rows(&e);
    assert!(rows.len() <= 1, "{}/{}: {rows:?}", c.shaper, c.name);
    let stored = ids.first().map(|id| {
        assert!(out.contains(id.as_str()), "{}/{}: the hook output does not name {id}", c.shaper, c.name);
        assert!(!out.contains(ZERO_ID), "{}/{}: the hook output names {ZERO_ID}", c.shaper, c.name);
        (id.clone(), zygo_results::Store::get(&e.dir.results(), id).unwrap().data)
    });
    let mask = |s: &str| match &stored {
        Some((id, _)) => s.replace(id.as_str(), ZERO_ID),
        None => s.to_string(),
    };
    let (mut view, mut stderr_out) = (String::new(), None);
    if !out.is_empty() {
        let v: Value = serde_json::from_str(&out).unwrap();
        let h = &v["hookSpecificOutput"];
        assert_eq!(h["hookEventName"], "PostToolUse", "{}/{}: O§7.13 item 5", c.shaper, c.name);
        let u = &h["updatedToolOutput"];
        view = mask(u["stdout"].as_str().unwrap_or_else(|| panic!("{}/{}: no updatedToolOutput.stdout", c.shaper, c.name)));
        stderr_out = u["stderr"].as_str().map(String::from);
    }
    Got { out: mask(&out), view, stderr_out, row: rows.into_iter().next(), stored }
}

/// The raw text the hook stores for `c` (O§7.2, Decision 3).
fn raw_text(c: &Case) -> Vec<u8> {
    let mut r = c.stdout.clone();
    if c.meta["agent"] == "opencode" {
        r.extend_from_slice(&c.stderr);
    } else if !c.stderr.is_empty() {
        if !r.is_empty() && !r.ends_with(b"\n") {
            r.push(b'\n');
        }
        r.extend_from_slice(b"[stderr]\n");
        r.extend_from_slice(&c.stderr);
    }
    r
}

/// The text the hook shapes for `c` (O§7.8): the raw text when PO2_STDERR_CLEARED or OpenCode, else stdout.
fn shaped_src(c: &Case) -> String {
    let b = if trim::PO2_STDERR_CLEARED || c.meta["agent"] == "opencode" { raw_text(c) } else { c.stdout.clone() };
    String::from_utf8_lossy(&b).into_owned()
}

/// An O§8.3 built input: a Claude Code `PostToolUse` case named `built/<name>` in its own temp folder,
/// `meta` = {"command", "exit": 0, "event": "PostToolUse", "agent": "claude-code"}, empty `stderr` and `must_keep` file.
fn built(name: &str, command: Value, stdout: Vec<u8>) -> (tempfile::TempDir, Case) {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("built").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let meta = json!({"command": command, "exit": 0, "event": "PostToolUse", "agent": "claude-code"});
    std::fs::write(dir.join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
    std::fs::write(dir.join("stdout"), &stdout).unwrap();
    std::fs::write(dir.join("stderr"), b"").unwrap();
    std::fs::write(dir.join("must_keep"), b"").unwrap();
    let c = Case { dir, shaper: "built".into(), name: name.into(), meta, stdout, stderr: vec![], must_keep: vec![] };
    (t, c)
}

/// `running {n} tests`, then `test t{i:04} ... ok` for i in 0..n, a blank line, then
/// `test result: ok. {n} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`; each line ends in `\n`.
fn libtest_text(n: usize) -> Vec<u8> {
    let mut s = format!("running {n} tests\n");
    for i in 0..n {
        s.push_str(&format!("test t{i:04} ... ok\n"));
    }
    s.push_str(&format!("\ntest result: ok. {n} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"));
    s.into_bytes()
}

/// `text` without its color codes: each `ESC [ <digits and ;> m` sequence. `zygo-results` has no public strip function.
fn no_color(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("\x1b[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let n = after.bytes().take_while(|b| b.is_ascii_digit() || *b == b';').count();
        if after.as_bytes().get(n) == Some(&b'm') {
            rest = &after[n + 1..];
        } else {
            out.push('\x1b');
            rest = &rest[i + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// O§7.3: the case's text is reuse text.
fn reuse(c: &Case) -> bool {
    let cmd = c.meta["command"].as_str().unwrap_or("");
    zygo_results::shape::command::parse(cmd).reuse
        || matches!(zygo_results::kind::sniff(&c.stdout), zygo_results::Kind::Diff | zygo_results::Kind::Json | zygo_results::Kind::Jsonl)
}

/// Each way `g` misses the case's O§8.1 and O§8.2 checks (Decisions 4-6); empty when it meets them.
fn problems(c: &Case, f: Floor, g: &Got) -> Vec<String> {
    let f = if f == Floor::FailedCc { FAILED_CC } else { f };
    let at = format!("{}/{}", c.shaper, c.name);
    let mut p = never_worse(c, g);
    let raw = raw_text(c);
    let raw_s = String::from_utf8_lossy(&raw).into_owned();
    if f == Floor::NoChange {
        if !g.out.is_empty() {
            p.push(format!("{at}: the hook changed the output; O§8.2 wants no change"));
        }
        if g.row.is_some() || g.stored.is_some() {
            p.push(format!("{at}: a row or a stored result; O§8.2 wants no change"));
        }
        let raw_lines: Vec<&str> = raw_s.split('\n').collect();
        for m in &c.must_keep {
            if !raw_lines.contains(&m.as_str()) {
                p.push(format!("{at}: must_keep line not in the raw text: {m:?}"));
            }
        }
        return p;
    }
    let (Some((s, v, _, row_id)), Some((id, _))) = (&g.row, &g.stored) else {
        p.push(format!("{at}: no hook view, row or stored result"));
        return p;
    };
    if g.out.is_empty() {
        p.push(format!("{at}: no hook output"));
    }
    if row_id.as_deref() != Some(id.as_str()) {
        p.push(format!("{at}: row result_id {row_id:?} is not the stored id (O§7.9)"));
    }
    if trim::PO2_STDERR_CLEARED && c.meta["agent"] != "opencode" && !c.stderr.is_empty() && g.stderr_out.as_deref() != Some("") {
        p.push(format!("{at}: stderr is not cleared (O§7.8)"));
    }
    let floor = |pct: u64, p: &mut Vec<String>| {
        if !meets(*s, *v, pct) {
            p.push(format!("{at}: saving {} of {s} bytes is under the {pct} % floor (O§8.2)", *s as i64 - *v as i64));
        }
    };
    match f {
        Floor::Clean => {
            floor(5, &mut p);
            if g.view.contains('\x1b') {
                p.push(format!("{at}: the view holds ESC (O§8.2)"));
            }
            if g.view.contains('\r') && !reuse(c) {
                p.push(format!("{at}: the view holds CR and the text is not reuse text (O§8.2)"));
            }
            // O§8.1 clean row: the CRLF `git diff --color=always` case stays byte-equal except the color codes
            if c.name == "git-diff-color-crlf" && g.view.split_once('\n').map(|(_, b)| b) != Some(no_color(&raw_s).as_str()) {
                p.push(format!("{at}: the view after the marker is not the raw text byte-equal except the color codes (O§8.1)"));
            }
        }
        Floor::DedupWarning => {
            floor(50, &mut p);
            let body = g.view.split_once('\n').map_or("", |(_, b)| b);
            let cleaned = zygo_results::shape::clean::clean(&raw_s, reuse(c)).map_or(raw_s.clone(), |(t, _)| t);
            if zygo_results::shape::dedup::expand(body) != cleaned {
                p.push(format!("{at}: the expanded view differs from the cleaned text (O§8.2)"));
            }
        }
        Floor::LibtestPassing => {
            floor(80, &mut p);
            for l in raw_s.split('\n').filter(|l| l.starts_with("test result: ")) {
                if !g.view.split('\n').any(|x| x == l) {
                    p.push(format!("{at}: summary line not in the view: {l:?}"));
                }
            }
        }
        Floor::Failing => floor(30, &mut p),
        Floor::Capped => {
            if *v > zygo_results::view::VIEW_CAP_BYTES as u64 {
                p.push(format!("{at}: view of {v} bytes is over the {} byte cap (O§8.2)", zygo_results::view::VIEW_CAP_BYTES));
            }
        }
        Floor::NoChange | Floor::NoRow | Floor::FailedCc => {}
    }
    p
}

/// The 1-based numbers and texts of the lines of `text` (split at `\n`) that `body` does not keep, in order.
/// `body` is `text` with lines removed and O§7.6 count lines added; a count line starts with `[zygo: ` and
/// ends with ` hidden]`. `None` when a line of `body` is neither the next kept line of `text` nor a count line.
fn hidden_lines<'a>(text: &'a str, body: &str) -> Option<Vec<(usize, &'a str)>> {
    let t: Vec<&'a str> = text.split('\n').collect();
    let (mut i, mut hidden) = (0, vec![]);
    for b in body.split('\n') {
        loop {
            if t.get(i) == Some(&b) {
                i += 1;
                break;
            }
            if b.starts_with("[zygo: ") && b.ends_with(" hidden]") {
                break;
            }
            let l = t.get(i)?;
            hidden.push((i + 1, *l));
            i += 1;
        }
    }
    hidden.extend(t[i..].iter().enumerate().map(|(k, l)| (i + k + 1, *l)));
    Some(hidden)
}

/// The 1-based numbers of the lines of `text` (split at `\n`) inside a libtest or pytest failure section (O§7.6
/// "Failure section"), from the spec's definitions, not the runner's code. libtest: each `test <name> ... FAILED`
/// line, each `---- <name> stdout ----` block to the next `---- ` header or `failures:` line, each `failures:` line
/// and the indented name list after it. pytest: every line from the first `FAILURES`, `ERRORS`, `PASSES` or
/// `short test summary info` header to the end of the text.
fn failure_section_lines(text: &str) -> BTreeSet<usize> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut s = BTreeSet::new();
    let header = regex::Regex::new(r"^=+ (FAILURES|ERRORS|PASSES|short test summary info) =+$").unwrap();
    if let Some(i) = lines.iter().position(|l| header.is_match(l)) {
        s.extend(i + 1..=lines.len());
    }
    let failed = regex::Regex::new(r"^test .+ \.\.\. FAILED$").unwrap();
    let block = regex::Regex::new(r"^---- .+ stdout ----$").unwrap();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let (opens, more): (bool, fn(&str) -> bool) = if block.is_match(l) {
            (true, |x| !x.starts_with("---- ") && x != "failures:")
        } else if l == "failures:" {
            (true, |x| x.starts_with("    "))
        } else {
            (failed.is_match(l), |_| false)
        };
        if opens {
            s.insert(i + 1);
        }
        i += 1;
        while opens && i < lines.len() && more(lines[i]) {
            s.insert(i + 1);
            i += 1;
        }
    }
    s
}

/// O§8.3 items 2-6 (Decision 3) on what the hook gave for `c`; empty when the view is never worse.
/// Item 1 is `run`: an `Err` or a panic fails the test.
fn never_worse(c: &Case, g: &Got) -> Vec<String> {
    never_worse_counted(c, g).0
}

/// `never_worse`, and the number of hidden lines item 5 checked: `None` when item 5 did not run.
fn never_worse_counted(c: &Case, g: &Got) -> (Vec<String>, Option<usize>) {
    let at = format!("{}/{}", c.shaper, c.name);
    let mut p = vec![];
    let mk = std::fs::read(c.dir.join("must_keep")).unwrap();
    if mk.starts_with(b"\n") || mk.windows(2).any(|w| w == b"\n\n") {
        p.push(format!("{at}: empty must_keep line (O§8.1, O§8.3 item 4)"));
    }
    let raw = raw_text(c);
    // item 6 (O§7.3, O-C1): above SHAPE_MAX_BYTES the view is the size view of the raw text
    if raw.len() > SHAPE_MAX_BYTES {
        let s = zygo_results::Stored {
            id: ZERO_ID.into(),
            tool: "Bash".into(),
            bytes: raw.len() as u64,
            lines: String::from_utf8_lossy(&raw).lines().count() as u64,
            kind: zygo_results::Kind::Text,
            created_at: 0,
            session: None,
            data: vec![],
        };
        if g.view != size_view(&shaped_src(c), &s, &results_cfg()) {
            p.push(format!("{at}: above SHAPE_MAX_BYTES the view is not the size view of the raw text (O§8.3 item 6)"));
        }
    }
    // empty output is "no change"
    if g.out.is_empty() {
        return (p, None);
    }
    let (Some((s, v, shaper, _)), Some((_, data))) = (&g.row, &g.stored) else {
        p.push(format!("{at}: a view and no trims row or stored result (O§8.3 items 2, 3)"));
        return (p, None);
    };
    // O§7.8, O§7.9: stored_bytes is the raw text (stdout only in the PO2-fail branch on Claude Code), view_bytes the view
    let counted = shaped_src(c).len();
    if *s != counted as u64 {
        p.push(format!("{at}: row stored_bytes {s} is not the {counted} bytes of the raw text (O§7.8)"));
    }
    // the mask keeps the length: an id and ZERO_ID are both 26 bytes
    if *v != g.view.len() as u64 {
        p.push(format!("{at}: row view_bytes {v} is not the {} bytes of the view (O§7.8)", g.view.len()));
    }
    // item 2 (O-C4, O§7.8 use rule)
    if v > s {
        p.push(format!("{at}: view of {v} bytes is larger than the {s} stored bytes (O§8.3 item 2)"));
    } else if s - v < MIN_SAVING_BYTES as u64 {
        p.push(format!("{at}: saving {} of {s} bytes is under MIN_SAVING_BYTES {MIN_SAVING_BYTES} (O§8.3 item 2)", s - v));
    }
    // item 3
    if !g.view.contains(ZERO_ID) {
        p.push(format!("{at}: the view does not name the stored id (O§8.3 item 3, O§16 item 2)"));
    }
    if *data != raw {
        p.push(format!("{at}: the stored result differs from the raw text (O§8.3 item 3)"));
    }
    // item 4 (O-C9)
    let mut lines: Vec<&str> = g.view.split('\n').collect();
    if let Some(e) = g.stderr_out.as_deref().filter(|e| !e.is_empty()) {
        lines.extend(e.split('\n'));
    }
    let log = shaper == "log";
    for m in &c.must_keep {
        if !holds(&lines, m, log) {
            p.push(format!("{at}: must_keep line not in the view: {m:?}"));
        }
    }
    // item 5 (O§7.6, O-C9): only when the runner is the last step (spec gap 1)
    let r = match shaper.as_str() {
        "libtest" => Runner::Libtest,
        "pytest" => Runner::Pytest,
        "gotest" => Runner::Gotest,
        _ => return (p, None),
    };
    if !g.view.starts_with("[zygo shaped this output: ") {
        p.push(format!("{at}: the {shaper} row's view has no shaped marker, so item 5 cannot follow it (O§8.3 item 5, spec gap 1)"));
        return (p, None);
    }
    let checked = {
        let src = shaped_src(c);
        let mut t = clean(&src, reuse(c)).map_or(src, |(t, _)| t);
        if !reuse(c) {
            if let Some(d) = dedup(&t) {
                t = d;
            }
        }
        let body = g.view.split_once('\n').map_or("", |(_, b)| b);
        match hidden_lines(&t, body) {
            None => {
                p.push(format!("{at}: the {shaper} view holds a line that is not in the text it shaped (O§8.3 item 5)"));
                None
            }
            Some(h) => {
                let passed = if r == Runner::Gotest { runner::gotest_passed_lines(&t) } else { vec![] };
                // O§7.6: no hidden form applies inside a libtest or pytest failure section, whichever runner's view it is
                let kept = failure_section_lines(&t);
                for &(n, line) in &h {
                    if kept.contains(&n) {
                        p.push(format!("{at}: line {n} hidden by the {shaper} view is in a failure section (O§8.3 item 5, O§7.6): {line:?}"));
                    } else if !runner::hidden_by(r, line) && !passed.contains(&n) {
                        p.push(format!("{at}: line {n} hidden by the {shaper} view matches no hidden form of its row (O§8.3 item 5): {line:?}"));
                    }
                }
                Some(h.len())
            }
        }
    };
    (p, checked)
}

#[test]
fn case_views_meet_floors_and_keep_lines() {
    let mut p = vec![];
    for c in shaping::cases(None) {
        let f = floor_of(&c);
        p.extend(problems(&c, f, &run_case(&c)));
    }
    assert!(p.is_empty(), "{}", p.join("\n"));
}

/// A copy of `fixtures/shaping/libtest/failing-nocapture/` under a temp root, with `must_keep` as given
/// (the real one when `None`) and no `view`.
fn copy_case(must_keep: Option<&[u8]>) -> (tempfile::TempDir, Case) {
    let t = tempfile::tempdir().unwrap();
    let src = shaping::root().join("libtest/failing-nocapture");
    let dst = t.path().join("libtest/failing-nocapture");
    std::fs::create_dir_all(&dst).unwrap();
    for f in ["meta.json", "stdout", "stderr", "must_keep"] {
        std::fs::copy(src.join(f), dst.join(f)).unwrap();
    }
    if let Some(m) = must_keep {
        std::fs::write(dst.join("must_keep"), m).unwrap();
    }
    let c = shaping::cases_in(t.path(), Some("libtest")).remove(0);
    (t, c)
}

#[test]
fn empty_must_keep_line_makes_the_case_invalid() {
    let m = shaping::cases(Some("libtest")).into_iter().find(|c| c.name == "failing-nocapture").unwrap().must_keep[0].clone();
    let (_t, c) = copy_case(Some(format!("{m}\n\n{m}\n").as_bytes()));
    let p = problems(&c, Floor::Failing, &run_case(&c));
    assert!(p.iter().any(|l| l.contains("empty must_keep line")), "{p:?}");
}

/// O§8.1 "Bless refuses" (Decision 7): the problems of `g`; with none, writes `view` under `bless`, else compares it.
fn settle(c: &Case, f: Floor, g: &Got, bless: bool) -> Vec<String> {
    let p = problems(c, f, g);
    if !p.is_empty() {
        return p;
    }
    let path = c.dir.join("view");
    if bless {
        std::fs::write(&path, &g.view).unwrap();
        return vec![];
    }
    match std::fs::read(&path) {
        Err(_) => vec![format!("{}: missing; write it with ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping", path.display())],
        Ok(b) if b != g.view.as_bytes() => vec![format!("{}: differs from the hook view", path.display())],
        Ok(_) => vec![],
    }
}

/// `g` as a shaper changed to hide `line` would give it (O§16 item 1, Decision 8): each view line equal to `line` removed.
fn hiding(g: &Got, line: &str) -> Got {
    let end = g.view.ends_with('\n');
    let body = g.view.strip_suffix('\n').unwrap_or(&g.view);
    let mut view = body.split('\n').filter(|l| *l != line).collect::<Vec<_>>().join("\n");
    if end {
        view.push('\n');
    }
    Got { view, ..g.clone() }
}

#[test]
fn bless_refuses_a_view_that_hides_a_must_keep_line() {
    let (_t, c) = copy_case(None);
    let g = run_case(&c);
    let m = c.must_keep[0].clone();
    let p = settle(&c, Floor::Failing, &hiding(&g, &m), true);
    assert!(p.iter().any(|l| l.contains(&format!("must_keep line not in the view: {m:?}"))), "{p:?}");
    assert!(!c.dir.join("view").exists());
}

#[test]
fn bless_writes_the_view_and_the_check_reads_it() {
    let (_t, c) = copy_case(None);
    let g = run_case(&c);
    assert_eq!(settle(&c, Floor::Failing, &g, true), Vec::<String>::new());
    assert_eq!(std::fs::read(c.dir.join("view")).unwrap(), g.view.as_bytes());
    assert_eq!(settle(&c, Floor::Failing, &g, false), Vec::<String>::new());
    std::fs::write(c.dir.join("view"), b"x").unwrap();
    assert!(settle(&c, Floor::Failing, &g, false)[0].ends_with("differs from the hook view"));
}

#[test]
fn case_views_match_goldens() {
    // O§8.1: the only test that reads ZYGO_BLESS
    let bless = std::env::var("ZYGO_BLESS").as_deref() == Ok("1");
    let mut p = vec![];
    for c in shaping::cases(None) {
        p.extend(settle(&c, floor_of(&c), &run_case(&c), bless));
    }
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
#[should_panic]
fn run_fails_on_a_hook_error() {
    // F1: a hook error is a test failure, not "no change"
    run(&env(Agent::ClaudeCode), b"not json");
}

/// The case `shaper/name` and its hook result.
fn got(shaper: &str, name: &str) -> (Case, Got) {
    let c = shaping::cases(Some(shaper)).into_iter().find(|c| c.name == name).unwrap();
    let g = run_case(&c);
    (c, g)
}

#[test]
fn problems_name_a_row_that_does_not_match_the_view() {
    // O§7.8, O§7.9: the row's numbers and id are those of the stored raw text and the view
    let (c, g) = got("libtest", "failing-nocapture");
    assert_eq!(problems(&c, Floor::Failing, &g), Vec::<String>::new());
    let mut b = g.clone();
    b.row.as_mut().unwrap().0 += 1;
    assert!(problems(&c, Floor::Failing, &b).iter().any(|l| l.contains("stored_bytes")), "{:?}", problems(&c, Floor::Failing, &b));
    let mut b = g.clone();
    b.row.as_mut().unwrap().1 -= 1;
    assert!(problems(&c, Floor::Failing, &b).iter().any(|l| l.contains("view_bytes")), "{:?}", problems(&c, Floor::Failing, &b));
    let mut b = g.clone();
    b.row.as_mut().unwrap().3 = Some(ZERO_ID.into());
    assert!(problems(&c, Floor::Failing, &b).iter().any(|l| l.contains("result_id")), "{:?}", problems(&c, Floor::Failing, &b));
}

#[test]
fn problems_name_a_view_without_the_id() {
    // O§16 item 2: recovery reads the id from the view
    let (c, g) = got("libtest", "failing-nocapture");
    let b = Got { view: g.view.replace(ZERO_ID, &"1".repeat(26)), ..g.clone() };
    let p = problems(&c, Floor::Failing, &b);
    assert!(p.iter().any(|l| l.contains("does not name the stored id")), "{p:?}");
    assert!(!settle(&c, Floor::Failing, &b, false).is_empty());
}

#[test]
fn problems_name_a_crlf_diff_view_that_changed_more_than_color() {
    // O§8.1 clean row: `git diff --color=always` with a CRLF file stays byte-equal except the color codes
    let (c, g) = got("clean", "git-diff-color-crlf");
    assert_eq!(problems(&c, Floor::Clean, &g), Vec::<String>::new());
    let b = Got { view: g.view.replacen("\r\n", " \n", 1), ..g.clone() };
    let p = problems(&c, Floor::Clean, &b);
    assert!(p.iter().any(|l| l.contains("byte-equal")), "{p:?}");
}

/// The view `trim_in` builds for `c`'s raw text when the hook runs on it: `shape::run` as `trim.rs` calls it,
/// then the O§7.8 shaped marker with the id masked; `None` when the size view would run.
fn runner_view(c: &Case) -> Option<String> {
    use zygo_results::shape::{command, run, ShapeInput};
    let raw = String::from_utf8(raw_text(c)).unwrap();
    let cmd = c.meta["command"].as_str().unwrap();
    let si = ShapeInput { parsed: Some(command::parse(cmd)), kind: zygo_results::kind::sniff(&c.stdout), threshold: SessionView::default().trim_threshold_bytes };
    let sh = run(&raw, &si);
    let s = zygo_results::Stored {
        id: ZERO_ID.into(),
        tool: "Bash".into(),
        bytes: raw.len() as u64,
        lines: raw.lines().count() as u64,
        kind: si.kind,
        created_at: 0,
        session: None,
        data: vec![],
    };
    (!sh.size_view).then(|| zygo_results::view::shaped_view(&sh.steps, &sh.text, &s))
}

#[test]
fn failed_cc_runner_views_meet_the_failing_floor() {
    // O§8.2 "libtest, pytest, go test, failing" while PO1 fails: the pytest and go test failing cases are checked on
    // the runner view of their raw text, as `trim_in` would shape it on a `PostToolUse` call
    let mut p = vec![];
    let mut n = 0;
    for c in shaping::cases(None) {
        if !(c.shaper == "gotest" || c.shaper == "pytest") || !CASES.iter().any(|(s, m, f)| *s == c.shaper && *m == c.name && *f == Floor::FailedCc) {
            continue;
        }
        n += 1;
        let at = format!("{}/{}", c.shaper, c.name);
        let Some(v) = runner_view(&c) else {
            p.push(format!("{at}: no runner view"));
            continue;
        };
        let s = raw_text(&c).len() as u64;
        if !meets(s, v.len() as u64, 30) {
            p.push(format!("{at}: saving {} of {s} bytes is under the 30 % floor (O§8.2)", s as i64 - v.len() as i64));
        }
        let lines: Vec<&str> = v.split('\n').collect();
        for m in &c.must_keep {
            if !holds(&lines, m, false) {
                p.push(format!("{at}: must_keep line not in the runner view: {m:?}"));
            }
        }
    }
    assert_eq!(n, 4);
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
fn never_worse_aligns_hidden_lines() {
    assert_eq!(hidden_lines("a\nt1\nt2\nb\n", "a\n[zygo: 2 passed and 0 ignored test lines hidden]\nb\n"), Some(vec![(2, "t1"), (3, "t2")]));
    assert_eq!(hidden_lines("a\nb\n", "a\nb\n"), Some(vec![]));
    assert_eq!(hidden_lines("a\nb\nc", "a"), Some(vec![(2, "b"), (3, "c")]));
    assert_eq!(hidden_lines("a\nb\n", "a\nz\nb\n"), None);
}

#[test]
fn never_worse_holds_for_every_case() {
    // O§8.3, O§16 item 3: every case; `required_cases_present` keeps the set equal to CASES
    let (mut p, mut runner_views, mut hidden) = (vec![], 0, 0);
    for c in shaping::cases(None) {
        let (q, checked) = never_worse_counted(&c, &run_case(&c));
        if let Some(n) = checked {
            runner_views += 1;
            hidden += n;
        }
        p.extend(q);
    }
    assert!(runner_views > 0, "item 5 checked no runner view");
    assert!(hidden > 0, "item 5 checked no hidden line");
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
fn never_worse_names_a_saving_under_256_bytes() {
    // O§8.3 item 2, O-C4: 256 passes, 255 does not
    let (c, g) = got("libtest", "failing-nocapture");
    let with = |saving: u64| {
        let mut b = g.clone();
        let r = b.row.as_mut().unwrap();
        r.1 = r.0 - saving;
        never_worse(&c, &b)
    };
    assert!(!with(256).iter().any(|l| l.contains("O§8.3 item 2")), "{:?}", with(256));
    assert!(with(255).iter().any(|l| l.contains("O§8.3 item 2")), "{:?}", with(255));
}

#[test]
fn never_worse_names_a_stored_text_that_differs() {
    // O§8.3 item 3
    let (c, g) = got("libtest", "failing-nocapture");
    let mut b = g.clone();
    b.stored.as_mut().unwrap().1[0] ^= 1;
    assert!(never_worse(&c, &b).iter().any(|l| l.contains("O§8.3 item 3")), "{:?}", never_worse(&c, &b));
}

#[test]
fn never_worse_names_a_missing_must_keep_line() {
    // O§8.3 item 4, O-C9
    let (c, g) = got("libtest", "failing-nocapture");
    let m = c.must_keep[0].clone();
    assert!(never_worse(&c, &hiding(&g, &m)).iter().any(|l| l.contains(&format!("must_keep line not in the view: {m:?}"))));
}

#[test]
fn never_worse_names_a_hidden_line_of_no_hidden_form() {
    // O§8.3 item 5: `running 122 tests` is not a libtest hidden form (O§7.6)
    let (c, g) = got("libtest", "failing-nocapture");
    assert_eq!(never_worse(&c, &g), Vec::<String>::new());
    let p = never_worse(&c, &hiding(&g, "running 122 tests"));
    assert!(p.iter().any(|l| l.contains("O§8.3 item 5") && l.contains("running 122 tests")), "{p:?}");
}

#[test]
fn never_worse_refuses_bless_of_a_view_that_fails_it() {
    // O§8.1: bless refuses a view that fails an O§8.3 check
    let (_t, c) = copy_case(None);
    let g = run_case(&c);
    let p = settle(&c, Floor::Failing, &hiding(&g, "running 122 tests"), true);
    assert!(p.iter().any(|l| l.contains("O§8.3 item 5")), "{p:?}");
    assert!(!c.dir.join("view").exists());
}

#[test]
fn never_worse_names_a_size_view_that_differs() {
    // O§8.3 item 6: above SHAPE_MAX_BYTES (4,194,304) the view is the size view of the raw text
    assert_eq!(SHAPE_MAX_BYTES, 4_194_304);
    let (_t, c) = built("5-mib-output", json!("cargo test"), (0..327_680u32).flat_map(|i| format!("line {i:010}\n").into_bytes()).collect());
    let g = run_case(&c);
    assert_eq!(never_worse(&c, &g), Vec::<String>::new());
    let b = Got { view: g.view.replacen("line 0000000000", "line 0000000001", 1), ..g.clone() };
    assert!(never_worse(&c, &b).iter().any(|l| l.contains("O§8.3 item 6")), "{:?}", never_worse(&c, &b));
}

#[test]
fn never_worse_empty_output() {
    // O§8.3 built input: empty output
    let (_t, c) = built("empty-output", json!("true"), vec![]);
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_one_line() {
    // O§8.3 built input: one line
    let (_t, c) = built("one-line", json!("echo one"), b"one line\n".to_vec());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_three_equal_blank_lines() {
    // O§8.3 built input: 3 equal blank lines
    let (_t, c) = built("three-equal-blank-lines", json!("printf x"), b"\n\n\n".to_vec());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_lone_esc() {
    // O§8.3 built input: a lone ESC
    let (_t, c) = built("lone-esc", json!("printf x"), b"\x1b".to_vec());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_osc_with_no_end() {
    // O§8.3 built input: an OSC with no end, over the 4 KiB OSC bound (O§7.4)
    let (_t, c) = built("osc-with-no-end", json!("printf x"), [&b"\x1b]0;"[..], &b"a".repeat(5000), b"\n"].concat());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_5_mib_output() {
    // O§8.3 built input: a 5 MiB output
    let (_t, c) = built("5-mib-output", json!("cargo test"), (0..327_680u32).flat_map(|i| format!("line {i:010}\n").into_bytes()).collect());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_test_result_line_with_no_tests() {
    // O§8.3 built input: a `test result:` line with no tests
    let (_t, c) = built("test-result-line-with-no-tests", json!("cargo test"), b"test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n".to_vec());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_command_not_a_string() {
    // O§8.3 built input: a command that is not a string
    let (_t, c) = built("command-not-a-string", json!(42), libtest_text(1000));
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_command_with_an_unclosed_quote() {
    // O§8.3 built input: a command with an unclosed quote
    let (_t, c) = built("command-with-an-unclosed-quote", json!("cargo test 'x"), libtest_text(1000));
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_10000_deep_json_array() {
    // O§8.3 built input: a 10,000-deep JSON array
    let (_t, c) = built("10000-deep-json-array", json!("jq -c . deep.json"), [b"[".repeat(10_000), b"]".repeat(10_000), b"\n".to_vec()].concat());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

#[test]
fn never_worse_400000_character_line() {
    // O§8.3 built input: a 400,000-character line
    let (_t, c) = built("400000-character-line", json!("echo"), [b"a".repeat(400_000), b"\n".to_vec()].concat());
    assert_eq!(never_worse(&c, &run_case(&c)), Vec::<String>::new());
}

/// O§7.6 failure section: libtest output whose `---- x stdout ----` block holds a line of a hidden form
/// (trybuild-like), with 1000 passed test lines to hide and a `test result:` summary.
fn libtest_with_failure_section() -> Vec<u8> {
    let mut s = String::from("running 1001 tests\n");
    for i in 0..1000 {
        s.push_str(&format!("test t{i:04} ... ok\n"));
    }
    s.push_str("test x ... FAILED\n\nfailures:\n\n---- x stdout ----\ntest y ... ok\nassertion failed\n\nfailures:\n    x\n\n");
    s.push_str("test result: FAILED. 1000 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n");
    s.into_bytes()
}

#[test]
fn never_worse_failure_section_line_of_a_hidden_form() {
    // O§8.3 item 5, O§7.6: the hook keeps a line of a hidden form inside a libtest failure section
    let (_t, c) = built("failure-section", json!("cargo test"), libtest_with_failure_section());
    let g = run_case(&c);
    assert!(g.view.split('\n').any(|l| l == "test y ... ok"), "{}", g.view);
    let (p, checked) = never_worse_counted(&c, &g);
    assert_eq!(p, Vec::<String>::new());
    assert!(checked.is_some_and(|n| n > 0), "{checked:?}");
}

#[test]
fn never_worse_names_a_hidden_failure_section_line() {
    // O§8.3 item 5, O§7.6: a view that hides `test y ... ok` inside `---- x stdout ----` fails item 5
    let (_t, c) = built("failure-section", json!("cargo test"), libtest_with_failure_section());
    let p = never_worse(&c, &hiding(&run_case(&c), "test y ... ok"));
    assert!(p.iter().any(|l| l.contains("in a failure section") && l.contains("test y ... ok")), "{p:?}");
}

#[test]
fn never_worse_finds_failure_sections() {
    // O§7.6 "Failure section", libtest and pytest
    let lt = "test a ... ok\ntest x ... FAILED\n\nfailures:\n\n---- x stdout ----\ntest y ... ok\n\nfailures:\n    x\n\ntest result: FAILED.";
    assert_eq!(failure_section_lines(lt).into_iter().collect::<Vec<_>>(), vec![2, 4, 6, 7, 8, 9, 10]);
    let py = "a.py ..\n==== FAILURES ====\nb.py ..\n== 1 failed in 0.1s ==";
    assert_eq!(failure_section_lines(py).into_iter().collect::<Vec<_>>(), vec![2, 3, 4]);
}
