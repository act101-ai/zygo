//! O§9, O§16 item 11: with `[results] shape = false` the trim hook gives the hook stdout and stored raw file of `97d8767`.
mod common {
    pub mod shaping;
}
use common::shaping;
use serde_json::Value;
use std::path::Path;
use zygo_core::{
    Agent,
    hook::{HookInput, guarded},
    session_dir::SessionDir,
    session_view::SessionView,
};
use zygo_hooks::trim::{self, TrimOpts};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/shape_off");

struct Got {
    stdout: String,
    stored: Option<Vec<u8>>,
}

fn run_off(agent: Agent, input: &[u8]) -> Got {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView {
        session: "sess-1".into(),
        agent,
        results: true,
        shape: false,
        ..Default::default()
    }
    .save(&dir)
    .unwrap();
    assert!(!SessionView::load(&dir).unwrap().shape);
    let db = t.path().join("usage.db");
    let (out, code) = guarded(
        &|b| {
            trim::trim_in_with(
                &HookInput::parse(agent, b)?,
                &dir,
                &db,
                &TrimOpts::default(),
            )
        },
        input,
    );
    assert_eq!(code, 0);
    let ids: Vec<String> = std::fs::read_dir(dir.results())
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| zygo_results::is_id(n))
                .collect()
        })
        .unwrap_or_default();
    assert!(ids.len() <= 1, "{ids:?}");
    match ids.first() {
        Some(id) => Got {
            stdout: out.replace(id.as_str(), &"0".repeat(26)),
            stored: Some(std::fs::read(dir.results().join(id)).unwrap()),
        },
        None => Got {
            stdout: out,
            stored: None,
        },
    }
}

fn bless() -> bool {
    std::env::var("ZYGO_BLESS").as_deref() == Ok("1")
}

/// Compares `want` with the file at `p`, or writes it under `ZYGO_BLESS=1`; `None` means the file must be absent.
fn compare(p: &Path, want: Option<&[u8]>, problems: &mut Vec<String>) {
    if bless() {
        match want {
            Some(b) => {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, b).unwrap();
            }
            None if p.exists() => std::fs::remove_file(p).unwrap(),
            None => {}
        }
        return;
    }
    match (want, std::fs::read(p).ok()) {
        (Some(w), Some(g)) if w == g.as_slice() => {}
        (Some(_), Some(_)) => {
            problems.push(format!("{}: differs from the hook result", p.display()))
        }
        (Some(_), None) => problems.push(format!("{}: missing", p.display())),
        (None, Some(_)) => {
            problems.push(format!("{}: present, but nothing was stored", p.display()))
        }
        (None, None) => {}
    }
}

/// Compares `got` with `GOLDEN/rel/{hook_stdout,stored}`, or writes them under `ZYGO_BLESS=1`; returns each problem, naming the path.
fn check(rel: &str, got: &Got) -> Vec<String> {
    let d = Path::new(GOLDEN).join(rel);
    let mut p = vec![];
    compare(&d.join("hook_stdout"), Some(got.stdout.as_bytes()), &mut p);
    compare(&d.join("stored"), got.stored.as_deref(), &mut p);
    p
}

/// An input whose event is `PostToolUseFailure`: empty stdout and nothing stored.
fn empty(rel: &str, got: &Got) -> Vec<String> {
    let mut p = vec![];
    if !got.stdout.is_empty() {
        p.push(format!(
            "{rel}: PostToolUseFailure gave stdout {:?}",
            got.stdout
        ));
    }
    if got.stored.is_some() {
        p.push(format!("{rel}: PostToolUseFailure stored a result"));
    }
    p
}

fn agent_of(c: &shaping::Case) -> Agent {
    if c.meta["agent"] == "opencode" {
        Agent::OpenCode
    } else {
        Agent::ClaudeCode
    }
}

#[test]
fn shape_off_shaping_cases_match_goldens() {
    let cases = shaping::cases(None);
    assert!(!cases.is_empty());
    let mut p = vec![];
    for c in &cases {
        let rel = format!("shaping/{}/{}", c.shaper, c.name);
        let got = run_off(
            agent_of(c),
            &serde_json::to_vec(&shaping::hook_input(c)).unwrap(),
        );
        if c.meta["event"] == "PostToolUseFailure" {
            p.extend(empty(&rel, &got));
            continue;
        }
        // O§7.2 shaping off: the stored raw text is Bash stdout only; stderr is not read.
        if agent_of(c) == Agent::ClaudeCode
            && let Some(s) = &got.stored
        {
            assert_eq!(s, &c.stdout, "{rel}");
        }
        p.extend(check(&rel, &got));
    }
    assert!(p.is_empty(), "{}", p.join("\n"));
}

/// A `*.json` file of `fixtures/claude-code/hooks/` whose stem has `Bash` as its second `-` field.
fn is_bash_fixture(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "json")
        && p.file_stem().unwrap().to_string_lossy().split('-').nth(1) == Some("Bash")
}

#[test]
fn shape_off_claude_code_bash_fixtures_match_goldens() {
    let dir = shaping::repo().join("fixtures/claude-code/hooks");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| is_bash_fixture(p))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let mut p = vec![];
    for f in &files {
        let rel = format!("claude-code/{}", f.file_stem().unwrap().to_string_lossy());
        let input = std::fs::read(f).unwrap();
        let v: Value = serde_json::from_slice(&input).unwrap();
        let got = run_off(Agent::ClaudeCode, &input);
        if v["hook_event_name"] == "PostToolUseFailure" {
            p.extend(empty(&rel, &got));
        } else {
            p.extend(check(&rel, &got));
        }
    }
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
fn shape_off_opencode_after_bash_matches_golden() {
    let input = std::fs::read(
        shaping::repo().join("fixtures/opencode/plugin-hooks/tool.execute.after-bash.json"),
    )
    .unwrap();
    let p = check(
        "opencode/tool.execute.after-bash",
        &run_off(Agent::OpenCode, &input),
    );
    assert!(p.is_empty(), "{}", p.join("\n"));
}
