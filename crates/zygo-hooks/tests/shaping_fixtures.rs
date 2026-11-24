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
