//! Shaping fixture checks (O§8.1, O§16 item 16): byte-exact files, format, size and secrets.
mod common { pub mod shaping; }
use common::shaping::{self, Case};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use zygo_core::{hook::HookInput, Agent};

/// Writes the four case files into `root/shaper/name/` and returns the folder.
fn write_case(root: &Path, shaper: &str, name: &str, meta: &Value, stdout: &[u8], stderr: &[u8], must_keep: &[u8]) -> PathBuf {
    let d = root.join(shaper).join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("meta.json"), serde_json::to_vec_pretty(meta).unwrap()).unwrap();
    std::fs::write(d.join("stdout"), stdout).unwrap();
    std::fs::write(d.join("stderr"), stderr).unwrap();
    std::fs::write(d.join("must_keep"), must_keep).unwrap();
    d
}

fn meta(exit: i64) -> Value {
    json!({"command":"cargo test","exit":exit,"event":"PostToolUse","agent":"claude-code","tool":"cargo 1.90.0","captured_on":"2026-11-24","source":"cargo test, run by capture-shaping.sh in /repo"})
}

#[test]
fn gitattributes_keeps_shaping_bytes() {
    let text = std::fs::read_to_string(shaping::repo().join(".gitattributes")).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let auto = lines.iter().position(|l| *l == "* text=auto eol=lf").expect("* text=auto eol=lf");
    let raw = lines.iter().position(|l| *l == "fixtures/shaping/** -text").expect("fixtures/shaping/** -text");
    assert!(auto < raw, "the -text line must come after * text=auto eol=lf");
    let out = Command::new("git").args(["check-attr", "text", "--", "fixtures/shaping/clean/c/stdout"]).current_dir(shaping::repo()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "fixtures/shaping/clean/c/stdout: text: unset\n");
}

fn case(agent: &str, event: &str) -> Case {
    let mut m = meta(1);
    m["agent"] = json!(agent);
    m["event"] = json!(event);
    m["command"] = json!("cargo test -q");
    Case { dir: PathBuf::new(), shaper: "libtest".into(), name: "x".into(), meta: m, stdout: b"out\n".to_vec(), stderr: b"err\n".to_vec(), must_keep: vec![] }
}
fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().unwrap().keys().cloned().collect()
}
fn fixture(p: &str) -> Value {
    serde_json::from_slice(&std::fs::read(shaping::repo().join(p)).unwrap()).unwrap()
}

#[test]
fn hook_input_claude_code_post_tool_use() {
    let v = shaping::hook_input(&case("claude-code", "PostToolUse"));
    let f = fixture("fixtures/claude-code/hooks/PostToolUse-Bash-main.json");
    assert_eq!(keys(&v), keys(&f));
    assert_eq!(keys(&v["tool_response"]), keys(&f["tool_response"]));
    assert_eq!(v["hook_event_name"], "PostToolUse");
    assert_eq!(v["tool_name"], "Bash");
    assert_eq!(v["tool_input"]["command"], "cargo test -q");
    assert_eq!(v["tool_response"]["stdout"], "out\n");
    assert_eq!(v["tool_response"]["stderr"], "err\n");
    let h = HookInput::parse(Agent::ClaudeCode, &serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(h.event.as_deref(), Some("PostToolUse"));
    assert_eq!(h.tool_name.as_deref(), Some("Bash"));
}

#[test]
fn hook_input_claude_code_failure() {
    let mut c = case("claude-code", "PostToolUseFailure");
    c.stdout = b"Exit code 1\nout\n".to_vec();
    c.stderr = vec![];
    let v = shaping::hook_input(&c);
    assert_eq!(keys(&v), keys(&fixture("fixtures/claude-code/live/p-n24-1-exit.json")));
    assert_eq!(v["hook_event_name"], "PostToolUseFailure");
    assert_eq!(v["tool_name"], "Bash");
    assert_eq!(v["tool_input"]["command"], "cargo test -q");
    assert_eq!(v["error"], "Exit code 1\nout\n");
}

#[test]
fn hook_input_opencode() {
    let v = shaping::hook_input(&case("opencode", "PostToolUse"));
    assert_eq!(v["input"]["tool"], "bash");
    assert_eq!(v["input"]["args"]["command"], "cargo test -q");
    assert_eq!(v["output"]["output"], "out\nerr\n");
    assert_eq!(v["output"]["metadata"]["exit"], 1);
    let h = HookInput::parse(Agent::OpenCode, &serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(h.event.as_deref(), Some("PostToolUse"));
    assert_eq!(h.raw["tool_response"]["stdout"], "out\nerr\n");
}

#[test]
fn cases_reads_folders_in_order() {
    let t = tempfile::tempdir().unwrap();
    write_case(t.path(), "pytest", "b", &meta(0), b"o", b"e", b"l1\n\nl2\r\n");
    write_case(t.path(), "libtest", "a", &meta(0), b"o", b"e", b"");
    let all = shaping::cases_in(t.path(), None);
    assert_eq!(all.iter().map(|c| (c.shaper.as_str(), c.name.as_str())).collect::<Vec<_>>(), [("libtest", "a"), ("pytest", "b")]);
    assert_eq!(all[1].must_keep, ["l1", "l2\r"]);
    assert_eq!(all[1].stdout, b"o");
    assert_eq!(all[1].meta["exit"], 0);
    assert_eq!(shaping::cases_in(t.path(), Some("pytest")).len(), 1);
    assert!(shaping::cases_in(&t.path().join("absent"), None).is_empty());
}

#[test]
fn each_case_has_the_format() {
    let p: Vec<String> = case_dirs(&shaping::root()).iter().flat_map(|d| format_problems(d)).collect();
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
fn failing_case_with_empty_must_keep_fails() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let exit = write_case(r, "libtest", "exit", &meta(101), b"x\n", b"", b"");
    let summary = write_case(r, "libtest", "summary", &meta(0), b"test result: FAILED. 1 passed; 1 failed\n", b"", b"");
    let pytest = write_case(r, "pytest", "summary", &meta(0), b"==== 2 failed, 3 passed in 0.10s ====\n", b"", b"");
    let go = write_case(r, "gotest", "summary", &meta(0), b"--- FAIL: TestX (0.00s)\nFAIL\n", b"", b"");
    for d in [&exit, &summary, &pytest, &go] {
        assert!(format_problems(d).iter().any(|p| p.contains("failing test and empty must_keep")), "{}", d.display());
    }
    let kept = write_case(r, "libtest", "kept", &meta(101), b"x\n", b"", b"x\n");
    let passing = write_case(r, "libtest", "passing", &meta(0), b"test result: ok. 3 passed; 0 failed\n", b"", b"");
    assert_eq!(format_problems(&kept), Vec::<String>::new());
    assert_eq!(format_problems(&passing), Vec::<String>::new());
}

#[test]
fn format_check_names_missing_files_and_bad_meta() {
    let t = tempfile::tempdir().unwrap();
    let mut m = meta(0);
    m.as_object_mut().unwrap().remove("source");
    m["event"] = json!("Stop");
    let d = write_case(t.path(), "clean", "bad", &m, b"o", b"e", b"");
    std::fs::remove_file(d.join("stderr")).unwrap();
    let p = format_problems(&d).join("\n");
    for want in ["stderr", "source", "event"] {
        assert!(p.contains(want), "{want}: {p}");
    }
    let mut f = meta(1);
    f["event"] = json!("PostToolUseFailure");
    f["agent"] = json!("opencode");
    let d = write_case(t.path(), "failure", "bad", &f, b"Exit code 1\n", b"e", b"Exit code 1\n");
    let p = format_problems(&d).join("\n");
    assert!(p.contains("agent") && p.contains("stderr"), "{p}");
    let d = write_case(t.path(), "clean", "bytes", &meta(0), b"\xff", b"", b"");
    assert!(format_problems(&d).iter().any(|p| p.contains("UTF-8")));
}

/// A failing test run in a runner summary (author's call 5): libtest, pytest, go.
const FAILING: &str = r"(?m)^test result: FAILED\.|^=+ .*\b\d+ (failed|errors?)\b.*=+\r?$|^--- FAIL: |^FAIL(\s|$)";

/// Every `root/*/*/` folder, sorted; none when `root` is missing.
fn case_dirs(root: &Path) -> Vec<PathBuf> {
    let dirs = |d: &Path| -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(d) else { return vec![] };
        rd.map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect()
    };
    let mut v: Vec<PathBuf> = dirs(root).iter().flat_map(|s| dirs(s)).collect();
    v.sort();
    v
}

/// The O§8.1 format problems of one case folder, each starting with the folder path.
fn format_problems(dir: &Path) -> Vec<String> {
    let mut p = vec![];
    let mut bad = |what: String| p.push(format!("{}: {what}", dir.display()));
    let file = |f: &str| std::fs::read(dir.join(f)).ok();
    let (meta_bytes, stdout, stderr, must_keep) = (file("meta.json"), file("stdout"), file("stderr"), file("must_keep"));
    for (f, v) in [("meta.json", &meta_bytes), ("stdout", &stdout), ("stderr", &stderr), ("must_keep", &must_keep)] {
        if v.is_none() {
            bad(format!("{f} missing"));
        }
    }
    for (f, v) in [("stdout", &stdout), ("stderr", &stderr)] {
        if v.as_ref().is_some_and(|b| std::str::from_utf8(b).is_err()) {
            bad(format!("{f} not UTF-8"));
        }
    }
    let meta = meta_bytes.map(|b| serde_json::from_slice::<Value>(&b).ok().filter(Value::is_object));
    let meta = match meta {
        None => Value::Null,
        Some(None) => {
            bad("meta.json not a JSON object".into());
            Value::Null
        }
        Some(Some(m)) => {
            let date = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
            let checks: [(&str, bool); 7] = [
                ("command", m["command"].is_string()),
                ("exit", m["exit"].is_i64()),
                ("event", matches!(m["event"].as_str(), Some("PostToolUse" | "PostToolUseFailure"))),
                ("agent", matches!(m["agent"].as_str(), Some("claude-code" | "opencode"))),
                ("tool", m["tool"].is_string()),
                ("captured_on", m["captured_on"].as_str().is_some_and(|s| date.is_match(s))),
                ("source", m["source"].is_string()),
            ];
            for (k, ok) in checks {
                if !ok {
                    bad(format!("meta.json key {k} missing or wrong: {}", m[k]));
                }
            }
            m
        }
    };
    if meta["event"] == "PostToolUseFailure" {
        if meta["agent"] != "claude-code" {
            bad("PostToolUseFailure needs agent claude-code".into());
        }
        if stderr.as_ref().is_some_and(|b| !b.is_empty()) {
            bad("PostToolUseFailure needs an empty stderr".into());
        }
    }
    let text = [stdout.unwrap_or_default(), stderr.unwrap_or_default()].join(&b'\n');
    let failing = meta["exit"].as_i64().is_some_and(|e| e != 0) || regex::bytes::Regex::new(FAILING).unwrap().is_match(&text);
    let kept = must_keep.is_some_and(|b| b.split(|c| *c == b'\n').any(|l| !l.is_empty()));
    if failing && !kept {
        bad("failing test and empty must_keep".into());
    }
    p
}
