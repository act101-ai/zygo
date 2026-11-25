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
    let root = shaping::root();
    let mut p: BTreeSet<String> = symlinks(&root).iter().map(|s| format!("{} is a symlink", s.display())).collect();
    p.extend(case_dirs(&root).iter().flat_map(|d| format_problems(d)));
    assert!(p.is_empty(), "{}", p.into_iter().collect::<Vec<_>>().join("\n"));
}

#[test]
fn failing_case_with_empty_must_keep_fails() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let exit = write_case(r, "libtest", "exit", &meta(101), b"x\n", b"", b"");
    let summary = write_case(r, "libtest", "summary", &meta(0), b"test result: FAILED. 1 passed; 1 failed\n", b"", b"");
    let pytest = write_case(r, "pytest", "summary", &meta(0), b"==== 2 failed, 3 passed in 0.10s ====\n", b"", b"");
    let go = write_case(r, "gotest", "summary", &meta(0), b"--- FAIL: TestX (0.00s)\nFAIL\n", b"", b"");
    let color = write_case(r, "libtest", "color", &meta(0), b"test result: \x1b[31mFAILED\x1b[0m. 1 passed; 1 failed\n", b"", b"");
    let quiet = write_case(r, "pytest", "quiet", &meta(0), b"..F\n2 failed, 1 passed in 0.05s\n", b"", b"");
    let blank = write_case(r, "libtest", "blank", &meta(101), b"x\n", b"", b"\r\n  \n");
    for d in [&exit, &summary, &pytest, &go, &color, &quiet, &blank] {
        assert!(format_problems(d).iter().any(|p| p.contains("failing test and empty must_keep")), "{}", d.display());
    }
    let kept = write_case(r, "libtest", "kept", &meta(101), b"x\n", b"", b"x\n");
    let passing = write_case(r, "libtest", "passing", &meta(0), b"test result: ok. 3 passed; 0 failed\n", b"", b"");
    assert_eq!(format_problems(&kept), Vec::<String>::new());
    assert_eq!(format_problems(&passing), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn format_check_names_a_symlink() {
    let t = tempfile::tempdir().unwrap();
    let d = write_case(t.path(), "clean", "link", &meta(0), b"o", b"e", b"");
    std::fs::write(t.path().join("big"), b"o").unwrap();
    std::fs::remove_file(d.join("stdout")).unwrap();
    std::os::unix::fs::symlink(t.path().join("big"), d.join("stdout")).unwrap();
    let p = format_problems(&d).join("\n");
    assert!(p.contains("stdout") && p.contains("symlink"), "{p}");
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

/// A failing test run in a runner summary (author's call 5): libtest, pytest (with or without the
/// `=` banner), go. Matched after ANSI CSI sequences are removed.
const FAILING: &str = r"(?m)^test result: FAILED\.|^=+ .*\b\d+ (failed|errors?)\b.*=+\r?$|^(=+ )?(\d+ failed|.*, \d+ failed)\b.* in \d+(\.\d+)?s( =+)?\r?$|^--- FAIL: |^FAIL(\s|$)";
const ANSI_CSI: &str = r"\x1b\[[0-9;?]*[ -/]*[@-~]";

/// `p` and every path under it that is a symlink, sorted; symlinks are not followed.
fn symlinks(p: &Path) -> Vec<PathBuf> {
    let Ok(m) = std::fs::symlink_metadata(p) else { return vec![] };
    if m.file_type().is_symlink() {
        return vec![p.to_path_buf()];
    }
    let mut v = vec![];
    if m.is_dir() {
        for e in std::fs::read_dir(p).unwrap() {
            v.extend(symlinks(&e.unwrap().path()));
        }
    }
    v.sort();
    v
}

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
    for s in symlinks(dir) {
        bad(format!("{} is a symlink", s.display()));
    }
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
    let text = regex::bytes::Regex::new(ANSI_CSI).unwrap().replace_all(&text, &b""[..]);
    let failing = meta["exit"].as_i64().is_some_and(|e| e != 0) || regex::bytes::Regex::new(FAILING).unwrap().is_match(&text);
    let kept = must_keep.is_some_and(|b| b.split(|c| *c == b'\n').any(|l| l.iter().any(|c| !c.is_ascii_whitespace())));
    if failing && !kept {
        bad("failing test and empty must_keep".into());
    }
    p
}

const CASE_MAX: u64 = 256 * 1024; // O§8.1 "Size"
const FOLDER_MAX: u64 = 4 * 1024 * 1024;

#[test]
fn cases_fit_the_size_bounds() {
    let p = size_problems(&shaping::root());
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
fn size_check_finds_a_large_case_and_folder() {
    let t = tempfile::tempdir().unwrap();
    let d = write_case(t.path(), "log", "edge", &meta(0), b"", b"", b"");
    let rest: u64 = ["meta.json", "stderr", "must_keep"].iter().map(|f| std::fs::metadata(d.join(f)).unwrap().len()).sum();
    std::fs::write(d.join("stdout"), vec![b'a'; (CASE_MAX - rest) as usize]).unwrap();
    assert!(size_problems(t.path()).is_empty(), "exactly 256 KiB passes");
    std::fs::write(d.join("view"), b"v").unwrap();
    assert!(size_problems(t.path()).iter().any(|p| p.contains("edge")), "256 KiB + 1 fails");
    let t = tempfile::tempdir().unwrap();
    for i in 0..17 {
        write_case(t.path(), "log", &format!("c{i}"), &meta(0), &vec![b'a'; 250 * 1024], b"", b"");
    }
    assert!(size_problems(t.path()).iter().any(|p| p.contains("4 MiB")));
}

/// Every regular file under `root`, recursive, sorted; none when `root` is missing.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd {
            let e = e.unwrap();
            let ft = e.file_type().unwrap();
            if ft.is_dir() {
                todo.push(e.path());
            } else if ft.is_file() {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

fn total_size(root: &Path) -> u64 {
    files_under(root).iter().map(|f| std::fs::metadata(f).unwrap().len()).sum()
}

/// A case over 256 KiB or the folder over 4 MiB (author's call 6).
fn size_problems(root: &Path) -> Vec<String> {
    let mut p = vec![];
    for d in case_dirs(root) {
        let n = total_size(&d);
        if n > CASE_MAX {
            p.push(format!("{}: {n} bytes, over 256 KiB", d.display()));
        }
    }
    let n = total_size(root);
    if n > FOLDER_MAX {
        p.push(format!("{}: {n} bytes, over 4 MiB", root.display()));
    }
    p
}

#[test]
fn fixtures_have_no_secrets() {
    let p = secret_problems(&shaping::root());
    assert!(p.is_empty(), "{}", p.join("\n"));
}

#[test]
fn secret_check_finds_a_builtin_pattern() {
    let t = tempfile::tempdir().unwrap();
    let token = format!("zyg_s_{}", "A".repeat(43)); // built at run time: no key-shaped literal in the source
    write_case(t.path(), "log", "leak", &meta(0), format!("line\n{token}\n").as_bytes(), b"", b"");
    write_case(t.path(), "log", "clean", &meta(0), b"line\n", b"", b"");
    let p = secret_problems(t.path());
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].contains("leak") && p[0].contains("stdout"), "{p:?}");
}

/// One `<file>: <pattern id>` per §16.1 built-in pattern that matches a file under `root`
/// (O§16 item 16). Names the file and the id only, never the matched text.
fn secret_problems(root: &Path) -> Vec<String> {
    let patterns: Vec<(String, regex::bytes::Regex)> = zygo_config::builtin_patterns()
        .into_iter()
        .map(|p| {
            let re = regex::bytes::Regex::new(&p.pattern).unwrap_or_else(|e| panic!("pattern {}: {e}", p.id));
            (p.id, re)
        })
        .collect();
    let mut out = vec![];
    for f in files_under(root) {
        let bytes = std::fs::read(&f).unwrap();
        for (id, re) in &patterns {
            if re.is_match(&bytes) {
                out.push(format!("{}: {id}", f.display()));
            }
        }
    }
    out
}

#[cfg(unix)]
#[test]
fn capture_script_writes_four_files_and_scrubs() {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let (home, out) = (root.join("home"), root.join("out"));
    let scratch = home.join("scratch"); // under HOME: proves the scratch rule runs first
    std::fs::create_dir_all(&scratch).unwrap();
    let h = String::from_utf8(Command::new("hostname").output().unwrap().stdout).unwrap();
    let h = h.trim().split('.').next().unwrap().to_string();
    let host = if h.len() >= 4 && h != "localhost" { h } else { "host".into() };
    let script = shaping::repo().join("scripts/fixtures/capture-shaping.sh");
    let st = Command::new("sh")
        .arg(&script)
        .args(["clean", "probe", "--", "sh", "-c", r#"printf '%s\n' "$1/a" "$2/b" "$3" "$4"; printf 'crlf\r\n'; printf '%s\n' "$1/e" >&2; exit 3"#, "x"])
        .arg(&home)
        .arg(&scratch)
        .arg("capuser")
        .arg(&host)
        .current_dir(&scratch)
        .env("HOME", &home)
        .env("PWD", &scratch)
        .env("USER", "capuser")
        .env("ZYGO_FIXTURES_DIR", &out)
        .env("ZYGO_CAPTURE_TOOL", "sh 0.0")
        .status()
        .unwrap();
    assert!(st.success());
    let d = out.join("clean/probe");
    assert_eq!(std::fs::read(d.join("stdout")).unwrap(), b"/home/user/a\n/repo/b\nuser\nhost\ncrlf\r\n");
    assert_eq!(std::fs::read(d.join("stderr")).unwrap(), b"/home/user/e\n");
    assert_eq!(std::fs::read(d.join("must_keep")).unwrap(), b"");
    assert!(!d.join("view").exists());
    let m: Value = serde_json::from_slice(&std::fs::read(d.join("meta.json")).unwrap()).unwrap();
    assert_eq!(m["exit"], 3);
    assert_eq!(m["event"], "PostToolUse");
    assert_eq!(m["agent"], "claude-code");
    assert!(m["command"].as_str().unwrap().starts_with("sh -c "));
    let mut bads = vec![root.to_str().unwrap().to_string(), "capuser".to_string()];
    if host != "host" {
        bads.push(host.clone());
    }
    for f in ["stdout", "stderr", "meta.json"] {
        let s = String::from_utf8(std::fs::read(d.join(f)).unwrap()).unwrap();
        for bad in &bads {
            assert!(!s.contains(bad.as_str()), "{f} keeps {bad}");
        }
    }
    std::fs::write(d.join("must_keep"), b"x\n").unwrap(); // exit 3 needs a must_keep line
    assert!(format_problems(&d).is_empty(), "{:?}", format_problems(&d)); // meta.json has the O§8.1 keys
}

/// A temp tree for one capture-shaping.sh run: `home`, the run dir `run` and the output dir `out`.
#[cfg(unix)]
struct Cap {
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    out: PathBuf,
    _t: tempfile::TempDir,
}

#[cfg(unix)]
impl Cap {
    fn new(run: &str) -> Cap {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        let (home, run, out) = (root.join("home"), root.join(run), root.join("out"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        Cap { root, home, run, out, _t: t }
    }
    /// The script's command with the default environment; `args` follow the script path.
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new("sh");
        c.arg(shaping::repo().join("scripts/fixtures/capture-shaping.sh"))
            .args(args)
            .current_dir(&self.run)
            .env("HOME", &self.home)
            .env("PWD", &self.run)
            .env("USER", "capuser")
            .env("ZYGO_FIXTURES_DIR", &self.out)
            .env("ZYGO_CAPTURE_TOOL", "sh 0.0");
        c
    }
    fn read(&self, f: &str) -> String {
        String::from_utf8(std::fs::read(self.out.join("clean/x").join(f)).unwrap()).unwrap()
    }
    fn meta(&self) -> Value {
        serde_json::from_str(&self.read("meta.json")).unwrap()
    }
}

#[cfg(unix)]
#[test]
fn capture_script_scrubs_user_name_as_a_whole_word() {
    let c = Cap::new("w");
    let st = c
        .cmd(&["clean", "x", "--", "printf", r"%s\n", "running 3 tests", "testing test_a contest 3tests", "/home/test/x", "by test."])
        .env("USER", "test")
        .status()
        .unwrap();
    assert!(st.success());
    // A whole-word name is still replaced, so `test result:` would still become `user result:`.
    assert_eq!(c.read("stdout"), "running 3 tests\ntesting test_a contest 3tests\n/home/user/x\nby user.\n");
}

#[cfg(unix)]
#[test]
fn capture_script_scrubs_paths_at_a_path_boundary() {
    let c = Cap::new("w");
    let t = c.root.to_str().unwrap();
    let (sibling, inside) = (format!("{t}/work/x"), format!("{t}/w/y"));
    let st = c.cmd(&["clean", "x", "--", "printf", r"%s\n", &sibling, &inside]).status().unwrap();
    assert!(st.success());
    assert_eq!(c.read("stdout"), format!("{sibling}\n/repo/y\n"));
}

#[cfg(unix)]
#[test]
fn capture_script_leaves_no_folder_without_home() {
    let c = Cap::new("w");
    let st = c.cmd(&["clean", "x", "--", "true"]).env_remove("HOME").status().unwrap();
    assert!(!st.success());
    assert!(!c.out.join("clean/x").exists());
}

#[cfg(unix)]
#[test]
fn capture_script_refuses_an_existing_case() {
    let c = Cap::new("w");
    let d = c.out.join("clean/x");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("stdout"), b"old").unwrap();
    let st = c.cmd(&["clean", "x", "--", "printf", "new"]).status().unwrap();
    assert_eq!(st.code(), Some(1));
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    assert_eq!(std::fs::read(d.join("stdout")).unwrap(), b"old");
}

#[cfg(unix)]
#[test]
fn capture_script_refuses_bad_names() {
    let c = Cap::new("w");
    for (shaper, name) in [("Clean", "x"), ("clean", "a_b")] {
        let st = c.cmd(&[shaper, name, "--", "true"]).status().unwrap();
        assert_eq!(st.code(), Some(2), "{shaper}/{name}");
        assert!(!c.out.exists(), "{shaper}/{name}");
    }
}

#[cfg(unix)]
#[test]
fn capture_script_hides_a_relative_output_dir_from_the_command() {
    let c = Cap::new("w");
    let st = c.cmd(&["clean", "x", "--", "sh", "-c", "ls -R"]).env("ZYGO_FIXTURES_DIR", "rel").status().unwrap();
    assert!(st.success());
    let d = c.run.join("rel/clean/x");
    let stdout = String::from_utf8(std::fs::read(d.join("stdout")).unwrap()).unwrap();
    for f in ["stdout", "stderr", "must_keep", "meta.json"] {
        assert!(d.join(f).exists(), "{f}");
        assert!(!stdout.contains(f), "{f} listed: {stdout}");
    }
}

#[cfg(unix)]
#[test]
fn capture_script_runs_the_program_once_and_takes_tool_from_env() {
    use std::os::unix::fs::PermissionsExt;
    let c = Cap::new("w");
    let (prog, log) = (c.root.join("prog"), c.root.join("log"));
    std::fs::write(&prog, format!("#!/bin/sh\necho ran >>'{}'\n", log.display())).unwrap();
    std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = prog.to_str().unwrap();
    let st = c.cmd(&["clean", "x", "--", p]).env("ZYGO_CAPTURE_TOOL", "prog 1.2.3").status().unwrap();
    assert!(st.success());
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "ran\n");
    assert_eq!(c.meta()["tool"], "prog 1.2.3");
    for unset in [None, Some("")] {
        let c = Cap::new("w");
        let mut cmd = c.cmd(&["clean", "x", "--", p]);
        match unset {
            None => cmd.env_remove("ZYGO_CAPTURE_TOOL"),
            Some(v) => cmd.env("ZYGO_CAPTURE_TOOL", v),
        };
        assert_eq!(cmd.status().unwrap().code(), Some(2));
        assert!(!c.out.exists());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "ran\n", "the program ran with no tool");
    }
}

#[cfg(unix)]
#[test]
fn capture_script_records_the_command_as_shell_text() {
    let c = Cap::new("w");
    assert!(c.cmd(&["clean", "x", "--", "sh", "-c", "cargo test 2>&1 | tail -n 3"]).status().unwrap().success());
    assert_eq!(c.meta()["command"], "cargo test 2>&1 | tail -n 3");
    let c = Cap::new("w");
    assert!(c.cmd(&["clean", "x", "--", "printf", r"%s\n", "a  b"]).status().unwrap().success());
    assert_eq!(c.meta()["command"], r"printf '%s\n' 'a  b'");
    let c = Cap::new("w");
    assert!(c.cmd(&["clean", "x", "--", "printf", "it's", ""]).status().unwrap().success());
    assert_eq!(c.meta()["command"], r"printf 'it'\''s' ''");
}
