//! Shaping fixture harness (O§8.1): reads the cases under `fixtures/shaping/<shaper>/<case>/`.
//! A test binary uses it with `mod common { pub mod shaping; }`.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// One case folder: `meta.json`, `stdout`, `stderr` and `must_keep`.
pub struct Case {
    pub dir: PathBuf,
    pub shaper: String,
    pub name: String,
    pub meta: Value,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The `must_keep` lines: split on `\n`, empty items dropped, `\r` kept.
    pub must_keep: Vec<String>,
}

/// The repository root.
pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The shaping fixture folder.
pub fn root() -> PathBuf {
    repo().join("fixtures/shaping")
}

/// Every case under `fixtures/shaping/`, or only those of `shaper`.
pub fn cases(shaper: Option<&str>) -> Vec<Case> {
    cases_in(&root(), shaper)
}

/// The sorted subfolders of `dir`; none when `dir` is missing.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<PathBuf> = rd.map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// The cases in `root/<shaper>/<case>/`, sorted by shaper then case. Panics naming the path
/// when a file is missing or `meta.json` is not JSON.
pub fn cases_in(root: &Path, shaper: Option<&str>) -> Vec<Case> {
    let mut out = vec![];
    for s in subdirs(root) {
        if shaper.is_some_and(|want| file_name(&s) != want) {
            continue;
        }
        for dir in subdirs(&s) {
            let mp = dir.join("meta.json");
            let meta = serde_json::from_slice(&read(&mp)).unwrap_or_else(|e| panic!("{}: {e}", mp.display()));
            let must_keep = String::from_utf8_lossy(&read(&dir.join("must_keep"))).split('\n').filter(|l| !l.is_empty()).map(String::from).collect();
            out.push(Case {
                shaper: file_name(&s),
                name: file_name(&dir),
                meta,
                stdout: read(&dir.join("stdout")),
                stderr: read(&dir.join("stderr")),
                must_keep,
                dir,
            });
        }
    }
    out
}

fn template(p: &str) -> Value {
    serde_json::from_slice(&read(&repo().join(p))).unwrap()
}

/// A `PostToolUse` or `PostToolUseFailure` hook input in the shape of the case's `agent`,
/// built from the recorded fixture of that shape with the command and output fields replaced.
pub fn hook_input(c: &Case) -> Value {
    let command = c.meta["command"].clone();
    let id = json!(format!("toolu_shaping_{}_{}", c.shaper, c.name));
    let stdout = String::from_utf8_lossy(&c.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&c.stderr).into_owned();
    if c.meta["agent"] == "opencode" {
        let mut v = template("fixtures/opencode/plugin-hooks/tool.execute.after-bash.json");
        let text = json!(format!("{stdout}{stderr}"));
        v["input"]["callID"] = id;
        v["input"]["args"]["command"] = command.clone();
        v["output"]["title"] = command;
        v["output"]["output"] = text.clone();
        v["output"]["metadata"]["output"] = text;
        v["output"]["metadata"]["exit"] = c.meta["exit"].clone();
        return v;
    }
    let mut v = if c.meta["event"] == "PostToolUseFailure" {
        let mut v = template("fixtures/claude-code/live/p-n24-1-exit.json");
        v["error"] = json!(stdout);
        v
    } else {
        let mut v = template("fixtures/claude-code/hooks/PostToolUse-Bash-main.json");
        v["tool_response"]["stdout"] = json!(stdout);
        v["tool_response"]["stderr"] = json!(stderr);
        v
    };
    v["tool_input"]["command"] = command;
    v["tool_use_id"] = id;
    v
}
