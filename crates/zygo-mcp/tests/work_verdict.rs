//! `work_verdict` (DR-13): the D§9.2 input keys and the D§9.3 `dispositions` guard, with an injected `VerdictGit`.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use serde_json::{json, Value};
use zygo_config::Level;
use zygo_core::{overrides::Overrides, paths::Paths, session_dir::SessionDir, session_view::SessionView};
use zygo_mcp::tools::work::{dispositions_level, WorkVerdict, ARGS_MAX};
use zygo_mcp::{Tool, ToolError};
use zygo_usage::{Filter, Store, WorkUnitRow};
use zygo_work::gate::check::format_problems;
use zygo_work::gate::git::{Blob, GitFail, LogCommit, VerdictGit};

const UNIT: &str = "phase3:DR-13";
const ROOT: &str = "1111111111111111111111111111111111111111";
const BASE: &str = "2222222222222222222222222222222222222222";
const C1: &str = "3333333333333333333333333333333333333333";
const HEAD: &str = "4444444444444444444444444444444444444444";
const ROOT_DIR: &str = "/r/top";

const DESCRIPTION_TEXT: &str = "Lead only. Record your verdict on a unit of work and round after its subagent has stopped: keep, discard or rework, with your findings. On keep, give `head` (the commit you judged) and `dispositions` (an empty list when the review had no findings): one entry for each review finding, fixed with its commit, rejected with its reason, or recorded with its DDR id; in one checkout, also give `base`.";

// ---- records (copied from crates/zygo-work/tests/decision_verdict.rs, unit UNIT) ----

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Finding\n\nF.\n\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
const AGENT: &str = "\n\nZygo-Session: s";

/// The record id `DDR-0…0<c>`; `c` is never I, L, O or U.
fn id(c: char) -> String { format!("DDR-{}{c}", "0".repeat(25)) }

fn path(c: char) -> String { format!("docs/decisions/{}.md", id(c)) }

/// Record `id(c)`: an open `accepted-limit`, class `none`, title `T`, unit UNIT, round 3, role `code`, model
/// `unknown`, commit C40, date 2026-11-24, reverse_cost `low`, `depends_on = []`, body BODY. Each `(key, value)` of
/// `set` replaces or adds a key; values are TOML strings, except `round` and `depends_on`, written as given.
fn record(c: char, set: &[(&str, &str)]) -> String {
    let q = |v: &str| format!("\"{v}\"");
    let mut keys: Vec<(String, String)> = [
        ("id", q(&id(c))),
        ("kind", q("accepted-limit")),
        ("status", q("open")),
        ("class", q("none")),
        ("title", q("T")),
        ("unit", q(UNIT)),
        ("round", "3".to_string()),
        ("role", q("code")),
        ("model", q("unknown")),
        ("commit", q(C40)),
        ("date", q("2026-11-24")),
        ("reverse_cost", q("low")),
        ("depends_on", "[]".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    for (k, v) in set {
        let v = if matches!(*k, "round" | "depends_on") { v.to_string() } else { q(v) };
        match keys.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v,
            None => keys.push((k.to_string(), v)),
        }
    }
    let front: String = keys.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    format!("+++\n{front}+++\n{BODY}")
}

// ---- the scripted git ----

/// The git facts of one scenario. `Clone`: the `git` seam builds a fresh `Fake` from it per call.
#[derive(Clone)]
struct Script {
    commits: HashMap<String, String>,
    blobs: HashMap<(String, String), Blob>,
    merge_bases: HashMap<(String, String), String>,
    range: Vec<String>,
    log: Vec<LogCommit>,
    log_fail: Option<GitFail>,
}

fn s(v: &str) -> String { v.to_string() }

/// ROOT, BASE, C1, HEAD resolve to themselves; merge_base(BASE, HEAD) = merge_base("HEAD", HEAD) = BASE;
/// range [HEAD, C1]; log [HEAD (parent C1, "head{AGENT}"), C1 (parent BASE, "code{AGENT}")]; path P at BASE, C1, HEAD.
fn script() -> Script {
    let commits = [ROOT, BASE, C1, HEAD].iter().map(|c| (s(c), s(c))).collect();
    let p = format!("{}.md", id('P'));
    let blobs = [BASE, C1, HEAD].iter().map(|r| ((s(r), p.clone()), Blob::Data(record('P', &[]).into_bytes()))).collect();
    let merge_bases = [((s(BASE), s(HEAD)), s(BASE)), ((s("HEAD"), s(HEAD)), s(BASE))].into_iter().collect();
    let log = vec![
        LogCommit { id: s(HEAD), parents: vec![s(C1)], message: format!("head{AGENT}"), paths: vec![] },
        LogCommit { id: s(C1), parents: vec![s(BASE)], message: format!("code{AGENT}"), paths: vec![] },
    ];
    Script { commits, blobs, merge_bases, range: vec![s(HEAD), s(C1)], log, log_fail: None }
}

/// HEAD's log entry gains the path `(status, path(c))`; the blob (HEAD, "<id(c)>.md") becomes `Data(text)`, or is
/// removed when `text` is None.
fn change(s: &mut Script, c: char, status: char, text: Option<String>) {
    s.log[0].paths.push((status, path(c)));
    let key = (HEAD.to_string(), format!("{}.md", id(c)));
    match text {
        Some(t) => s.blobs.insert(key, Blob::Data(t.into_bytes())),
        None => s.blobs.remove(&key),
    };
}

/// A VerdictGit over a Script; each method first pushes "<method> <args>" to `calls`.
struct Fake {
    s: Script,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn note(&self, line: String) { self.calls.lock().unwrap().push(line) }
}

impl VerdictGit for Fake {
    fn commit(&mut self, rev: &str) -> Result<Option<String>, GitFail> {
        self.note(format!("commit {rev}"));
        Ok(self.s.commits.get(rev).cloned())
    }
    fn record_blob(&mut self, rev: &str, name: &str) -> Result<Blob, GitFail> {
        self.note(format!("record_blob {rev} {name}"));
        Ok(self.s.blobs.get(&(rev.to_string(), name.to_string())).cloned().unwrap_or(Blob::Missing))
    }
    fn merge_base(&mut self, a: &str, b: &str) -> Result<Option<String>, GitFail> {
        self.note(format!("merge_base {a} {b}"));
        Ok(self.s.merge_bases.get(&(a.to_string(), b.to_string())).cloned())
    }
    fn rev_list(&mut self, base: &str, head: &str, max: usize) -> Result<Option<Vec<String>>, GitFail> {
        self.note(format!("rev_list {base} {head} {max}"));
        Ok((self.s.range.len() <= max).then(|| self.s.range.clone()))
    }
    fn log(&mut self, base: &str, head: &str) -> Result<Vec<LogCommit>, GitFail> {
        self.note(format!("log {base} {head}"));
        match &self.s.log_fail {
            Some(e) => Err(e.clone()),
            None => Ok(self.s.log.clone()),
        }
    }
}

// ---- the harness ----

struct H {
    _t: tempfile::TempDir,
    db: PathBuf,
    calls: Arc<Mutex<Vec<String>>>,
    roots: Arc<Mutex<Vec<PathBuf>>>,
    levels_read: Arc<AtomicUsize>,
}

/// A temp dir; usage.db with a row for UNIT rounds 1 to 3 in session S1; a session dir whose view.json has
/// `repo_root` = `root` (no view.json when `view` is false); the tool with a fixed level and a scripted git.
fn harness(level: Level, script: Script, root: Option<&str>, view: bool) -> (WorkVerdict, H) {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("usage.db");
    let store = Store::open(&db).unwrap();
    for r in 1..=3 {
        store
            .insert_work_unit(&WorkUnitRow { unit: UNIT.into(), round: r, session: "S1".into(), agent_id: "a1".into(),
                began_at: 100, ..Default::default() })
            .unwrap();
    }
    let sd = SessionDir::new(t.path().join("s"));
    sd.create().unwrap();
    if view {
        SessionView { session: "S1".into(), repo_root: root.map(str::to_string), ..Default::default() }.save(&sd).unwrap();
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let roots = Arc::new(Mutex::new(Vec::new()));
    let levels_read = Arc::new(AtomicUsize::new(0));
    let lr = levels_read.clone();
    let (rs, cs) = (roots.clone(), calls.clone());
    let tool = WorkVerdict::new(
        db.clone(),
        "S1".into(),
        sd,
        Box::new(move |_| {
            lr.fetch_add(1, Ordering::SeqCst);
            level
        }),
        Box::new(move |root: &Path| {
            rs.lock().unwrap().push(root.to_path_buf());
            Box::new(Fake { s: script.clone(), calls: cs.clone() }) as Box<dyn VerdictGit>
        }),
    );
    (tool, H { _t: t, db, calls, roots, levels_read })
}

fn at(level: Level, script: Script) -> (WorkVerdict, H) { harness(level, script, Some(ROOT_DIR), true) }

fn verdict_of(h: &H, round: u32) -> Option<String> {
    Store::open(&h.db).unwrap().work_units(&Filter::default()).unwrap().into_iter().find(|u| u.unit == UNIT && u.round == round)?.verdict
}

fn no_git(h: &H) -> bool { h.roots.lock().unwrap().is_empty() && h.calls.lock().unwrap().is_empty() }

/// `{"unit": UNIT, "round": round, "verdict": "keep", "findings": ""}` plus each key of `keys`.
fn keep(round: u32, keys: Value) -> Value {
    let mut v = json!({"unit": UNIT, "round": round, "verdict": "keep", "findings": ""});
    for (k, x) in keys.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn fx(l: &str, c: &str) -> Value { json!({"finding": l, "class": "none", "disposition": "fixed", "commit": c}) }
fn rj(l: &str, class: &str) -> Value { json!({"finding": l, "class": class, "disposition": "rejected", "reason": "not a defect"}) }
fn rc(l: &str, class: &str, c: char) -> Value { json!({"finding": l, "class": class, "disposition": "recorded", "record": id(c)}) }
fn recorded_reply(round: u32) -> Value { json!(format!("recorded keep for {UNIT} round {round}")) }

/// A keep whose compact JSON is exactly ARGS_MAX bytes.
fn exactly_args_max() -> Value {
    let mut v = keep(1, json!({"head": HEAD, "dispositions": []}));
    let len = serde_json::to_vec(&v).unwrap().len();
    v["findings"] = json!("f".repeat(ARGS_MAX - len));
    assert_eq!(serde_json::to_vec(&v).unwrap().len(), ARGS_MAX);
    v
}

/// The regular files under `dir`, recursively.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else if p.is_file() {
            out.push(p);
        }
    }
    out
}

fn paths(root: &Path, config: &str) -> Paths {
    let config_file = root.join("config.toml");
    std::fs::write(&config_file, config).unwrap();
    Paths {
        config_file, cache_dir: root.join("cache"), data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"), marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"), approvals_file: root.join("data/approvals.toml"), usage_db: root.join("data/usage.db"),
    }
}

// ---- cycle 1: schema, description and argument refusals ----

#[test]
fn schema_and_description_match_d9_2() {
    let (t, _h) = at(Level::Enforce, script());
    let d = t.def();
    assert_eq!(d.name, "work_verdict");
    assert_eq!(d.description, DESCRIPTION_TEXT);
    let p = &d.input_schema["properties"];
    assert_eq!(p["findings"], json!({"type": "string"}));
    assert_eq!(p["head"], json!({"type": "string", "pattern": "^[0-9a-f]{7,64}$"}));
    assert_eq!(p["base"], json!({"type": "string", "pattern": "^[0-9a-f]{7,64}$"}));
    assert_eq!(
        p["dispositions"],
        json!({"type": "array", "maxItems": 256, "items": {"type": "object", "properties": {
            "finding":     {"type": "string", "minLength": 1, "maxLength": 200},
            "class":       {"type": "string", "enum": ["none","acceptance","security","irreversible","contract","spec","rule"]},
            "disposition": {"type": "string", "enum": ["fixed","rejected","recorded"]},
            "commit":      {"type": "string", "pattern": "^[0-9a-f]{7,64}$"},
            "reason":      {"type": "string", "minLength": 1, "maxLength": 2000},
            "record":      {"type": "string", "pattern": "^DDR-[0-9A-HJKMNP-TV-Z]{26}$"}
        }, "required": ["finding","class","disposition"]}})
    );
    assert_eq!(d.input_schema["required"], json!(["unit", "round", "verdict"]));
}

/// Each refused at Monitor, Notify and Enforce: InvalidParams, verdict unchanged, level not read, no git.
#[test]
fn argument_refusals_at_every_level() {
    let ok = rj("R1-1", "none");
    let many: Vec<Value> = (0..257).map(|i| rj(&format!("R1-{i}"), "none")).collect();
    let big = "f".repeat(ARGS_MAX);
    let bad: Vec<Value> = vec![
        keep(1, json!({"head": 7, "dispositions": []})),
        keep(1, json!({"head": HEAD, "base": null, "dispositions": []})),
        keep(1, json!({"head": HEAD, "base": true, "dispositions": []})),
        keep(1, json!({"head": HEAD, "dispositions": {}})),
        keep(1, json!({"head": HEAD, "dispositions": ["x"]})),
        keep(1, json!({"head": "abcdef", "dispositions": []})),
        keep(1, json!({"head": "A".repeat(40), "dispositions": []})),
        keep(1, json!({"head": "0".repeat(65), "dispositions": []})),
        keep(1, json!({"head": HEAD, "base": "g".repeat(40), "dispositions": []})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "minor", "disposition": "rejected", "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "deferred"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "", "class": "none", "disposition": "rejected", "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "f".repeat(201), "class": "none", "disposition": "rejected", "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "rejected", "reason": ""}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "rejected", "reason": "r".repeat(2001)}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "fixed", "commit": "xyz1234"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "fixed", "commit": 5}]})),
        keep(3, json!({"head": HEAD, "dispositions": [{"finding": "R3-1", "class": "none", "disposition": "recorded", "record": "DDR-bad"}]})),
        keep(3, json!({"head": HEAD, "dispositions": [{"finding": "R3-1", "class": "none", "disposition": "recorded", "record": format!("DDR-{}", "I".repeat(26))}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"class": "none", "disposition": "rejected", "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "disposition": "rejected", "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": many})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "rejected", "reason": "r", "commit": C1}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "fixed", "commit": C1, "reason": "r"}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "rejected", "reason": "r", "record": id('R')}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "fixed", "commit": C1, "record": id('R')}]})),
        keep(1, json!({"head": HEAD, "dispositions": [ok.clone()], "findings": big})),
        json!({"unit": UNIT, "round": 1, "verdict": "discard", "head": 7}),
    ];
    for level in [Level::Monitor, Level::Notify, Level::Enforce] {
        for args in &bad {
            let (t, h) = at(level, script());
            assert!(matches!(t.call(args.clone()), Err(ToolError::InvalidParams(_))), "{level:?} {args}");
            assert_eq!(verdict_of(&h, 1), None);
            assert_eq!(verdict_of(&h, 3), None);
            assert_eq!(h.levels_read.load(Ordering::SeqCst), 0);
            assert!(no_git(&h));
        }
    }
}

/// At Monitor (no check): each value at its bound is accepted and recorded.
#[test]
fn limits_at_the_bound_are_accepted() {
    let max: Vec<Value> = (0..256).map(|i| rj(&format!("R1-{i}"), "none")).collect();
    let ok: Vec<Value> = vec![
        keep(1, json!({"head": "abcdef1", "base": "0".repeat(64), "dispositions": max})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "é".repeat(200), "class": "none", "disposition": "rejected", "reason": "é".repeat(2000)}]})),
        keep(1, json!({"head": HEAD, "dispositions": [{"finding": "R1-1", "class": "none", "disposition": "rejected", "reason": "r", "note": 1}]})),
        exactly_args_max(),
    ];
    for args in ok {
        let (t, h) = at(Level::Monitor, script());
        assert_eq!(t.call(args), Ok(recorded_reply(1)));
        assert_eq!(verdict_of(&h, 1).as_deref(), Some("keep"));
    }
    let mut over = exactly_args_max();
    over["findings"] = json!(format!("{}f", over["findings"].as_str().unwrap()));
    assert!(matches!(at(Level::Monitor, script()).0.call(over), Err(ToolError::InvalidParams(_))));
}

#[test]
fn discard_and_rework_without_the_keys_are_recorded_as_today() {
    let (t, h) = at(Level::Enforce, script());
    assert_eq!(t.call(json!({"unit": UNIT, "round": 1, "verdict": "discard"})), Ok(json!(format!("recorded discard for {UNIT} round 1"))));
    assert_eq!(
        t.call(json!({"unit": UNIT, "round": 2, "verdict": "rework", "findings": "x"})),
        Ok(json!(format!("recorded rework for {UNIT} round 2")))
    );
    assert_eq!(verdict_of(&h, 1).as_deref(), Some("discard"));
    assert_eq!(verdict_of(&h, 2).as_deref(), Some("rework"));
    assert!(no_git(&h));
}

#[test]
fn dispositions_level_resolution() {
    let lvl = |config: &str, over: Option<&str>| {
        let d = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(d.path().join("s"));
        sd.create().unwrap();
        if let Some(l) = over {
            Overrides { guards: [("dispositions".to_string(), l.to_string())].into(), ..Default::default() }.save(&sd).unwrap();
        }
        dispositions_level(&paths(d.path(), config), &sd, None)
    };
    assert_eq!(lvl("", None), Level::Enforce);
    assert_eq!(lvl("[guards]\ndispositions = \"notify\"\n", None), Level::Notify);
    assert_eq!(lvl("[guards]\ndispositions = \"monitor\"\n", Some("enforce")), Level::Enforce);
    assert_eq!(lvl("[guards]\nmin_level = \"notify\"\ndispositions = \"notify\"\n", Some("monitor")), Level::Notify);
}
