//! OS-35: each shaping case through the trim hook: goldens, floors, store recovery, bless guard
//! (O§8.1, O§8.2, O§8.4, O§16 items 1 and 2).
mod common {
    pub mod shaping;
}
use common::shaping::{self, Case};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use zygo_core::{
    Agent,
    hook::HookInput,
    session_dir::SessionDir,
    session_view::SessionView,
};
use zygo_hooks::trim::{self, TrimOpts};

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
        head_lines: 60,
        tail_lines: 20,
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
    let mut p = vec![];
    let mk = std::fs::read(c.dir.join("must_keep")).unwrap();
    if mk.starts_with(b"\n") || mk.windows(2).any(|w| w == b"\n\n") {
        p.push(format!("{at}: empty must_keep line (O§8.1)"));
    }
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
    let (Some((s, v, shaper, row_id)), Some((id, data))) = (&g.row, &g.stored) else {
        p.push(format!("{at}: no hook view, row or stored result"));
        return p;
    };
    if g.out.is_empty() {
        p.push(format!("{at}: no hook output"));
    }
    // O§7.8, O§7.9: stored_bytes is the raw text (stdout only in the PO2-fail branch on Claude Code), view_bytes the view
    let counted = if trim::PO2_STDERR_CLEARED || c.meta["agent"] == "opencode" { raw.len() } else { c.stdout.len() };
    if *s != counted as u64 {
        p.push(format!("{at}: row stored_bytes {s} is not the {counted} bytes of the raw text (O§7.8)"));
    }
    // the mask keeps the length: an id and ZERO_ID are both 26 bytes
    if *v != g.view.len() as u64 {
        p.push(format!("{at}: row view_bytes {v} is not the {} bytes of the view (O§7.8)", g.view.len()));
    }
    if row_id.as_deref() != Some(id.as_str()) {
        p.push(format!("{at}: row result_id {row_id:?} is not the stored id (O§7.9)"));
    }
    if !g.view.contains(ZERO_ID) {
        p.push(format!("{at}: the view does not name the stored id (O§16 item 2)"));
    }
    if *data != raw {
        p.push(format!("{at}: the stored result differs from the raw text (O§8.4)"));
    }
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
