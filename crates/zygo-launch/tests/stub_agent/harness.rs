//! Shared by `stub_launch.rs` and `premise.rs` (N34): the rig, the launch and the capture the stub agent writes.
//! No process-environment change: every variable goes to the launched child through `LaunchOpts.env`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env::consts::EXE_SUFFIX;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_launch::claude_code::ClaudeCode;
use zygo_launch::env::Env;
use zygo_launch::launcher::LaunchOutcome;
use zygo_launch::opencode::OpenCode;
use zygo_launch::{AgentAdapter, LaunchOpts, Launcher};

/// The minimum versions in each client's descriptor (`Agent::spec().min_version`), equal to the `docs/premises.md` header.
pub const CLAUDE_VERSION: &str = Agent::ClaudeCode.spec().min_version;
pub const OPENCODE_VERSION: &str = Agent::OpenCode.spec().min_version;

pub struct Rig {
    pub _t: TempDir,
    pub root: PathBuf,
    pub home: PathBuf,
    pub user: PathBuf,
    pub cwd: PathBuf,
    pub bin: PathBuf,
    pub zygo: PathBuf,
    pub paths: Paths,
}

/// A temp tree (not canonicalized: on Windows that gives `\\?\` paths). `zygo` sits in a directory whose name has a space and an apostrophe.
pub fn rig() -> Rig {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    let (home, user, cwd, bin, zdir) = (root.join("home"), root.join("user"), root.join("work"), root.join("bin"), root.join("z y'g o"));
    for d in [&home, &user, &cwd, &bin, &zdir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let paths = Paths {
        config_file: home.join("config.toml"),
        cache_dir: home.join("cache"),
        data_dir: home.join("data"),
        sessions_dir: home.join("cache/sessions"),
        marketplaces_dir: home.join("cache/marketplaces"),
        plugin_store: home.join("data/plugins"),
        approvals_file: home.join("data/approvals.toml"),
        usage_db: home.join("data/usage.db"),
    };
    let zygo = zdir.join(format!("zygo{EXE_SUFFIX}"));
    let stub = env!("CARGO_BIN_EXE_stub-agent");
    for to in [zygo.clone(), bin.join(format!("claude{EXE_SUFFIX}")), bin.join(format!("opencode{EXE_SUFFIX}"))] {
        std::fs::copy(stub, to).unwrap();
    }
    Rig { _t: t, root, home, user, cwd, bin, zygo, paths }
}

/// The built-in layer without its marketplace (no network).
pub fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

pub fn claude(rig: &Rig) -> ClaudeCode {
    ClaudeCode::new("claude", rig.root.join("managed-settings.json"), rig.zygo.clone())
}

pub fn opencode(rig: &Rig) -> OpenCode {
    OpenCode { zygo: rig.zygo.clone() }
}

/// The shell the stub runs the generated commands in: `/bin/sh`, or Git Bash on Windows (the shell Claude Code uses there).
pub fn shell() -> PathBuf {
    if cfg!(windows) {
        let p = PathBuf::from(std::env::var_os("ProgramFiles").expect("ProgramFiles is unset; Git for Windows is required (the shell Claude Code uses on Windows)")).join(r"Git\bin\bash.exe");
        assert!(p.exists(), "Git for Windows is required (the shell Claude Code uses on Windows)");
        p
    } else {
        PathBuf::from("/bin/sh")
    }
}

/// The §8.5 note text: one shell word, written independently of `shell_quote`.
pub fn spec_quote(p: &Path) -> String {
    if cfg!(windows) { format!("\"{}\"", p.display()) } else { format!("'{}'", p.display().to_string().replace('\'', r"'\''")) }
}

pub struct Cap {
    pub cmdline: Option<String>,
    pub settings_mode: Option<u32>,
    pub argv: Vec<String>,
    /// `(flag, id)` of the session argument (`--session-id`, `--resume` or `-s`); `None` when the argv has none.
    pub session_arg: Option<(String, String)>,
    pub env: BTreeMap<String, String>,
    pub settings: Value,
    pub mcp: Value,
    pub plugins: BTreeMap<String, String>,
    pub session: BTreeMap<String, String>,
    pub ocdir: BTreeMap<String, String>,
    pub shell: BTreeMap<String, Value>,
    pub mcp_run: BTreeMap<String, String>,
}

pub struct Run {
    pub code: i32,
    pub outcome: LaunchOutcome,
    pub stderr: String,
    pub cap: Cap,
}

fn json_file(out: &Path, name: &str) -> Value {
    std::fs::read(out.join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

fn map(out: &Path, name: &str) -> BTreeMap<String, Value> {
    json_file(out, name).as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
}

fn strs(out: &Path, name: &str) -> BTreeMap<String, String> {
    map(out, name).into_iter().map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string())).collect()
}

/// Launches `adapter` with the stub as the agent. The capture directory is emptied first, so a file of an earlier launch is never read.
pub fn launch(rig: &Rig, adapter: &dyn AgentAdapter, version: &str, tweak: impl FnOnce(&mut LaunchOpts)) -> Run {
    let out = rig.root.join("out");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let os = |s: &str| OsString::from(s);
    let mut env = vec![
        (os("PATH"), rig.bin.clone().into_os_string()),
        (os("HOME"), rig.user.clone().into_os_string()),
        (os("USERPROFILE"), rig.user.clone().into_os_string()),
        (os("ACT_CONFIG_DIR"), rig.root.join("act").into_os_string()),
        (os("ZYGO_STUB_OUT"), out.clone().into_os_string()),
        (os("ZYGO_STUB_VERSION"), os(version)),
    ];
    if cfg!(windows) {
        env.extend(std::env::var_os("SystemRoot").map(|v| (os("SystemRoot"), v)));
    }
    let mut o = LaunchOpts::new(Env(env), rig.cwd.clone(), rig.paths.clone());
    o.builtin = Some(builtin());
    let buf = Arc::new(Mutex::new(Vec::new()));
    o.stderr = Some(buf.clone());
    tweak(&mut o);
    let outcome = Launcher::run(adapter, o);
    let stderr = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let arg = json_file(&out, "session-arg.json");
    let cap = Cap {
        session_arg: arg["flag"].as_str().zip(arg["id"].as_str()).map(|(f, i)| (f.to_string(), i.to_string())),
        cmdline: std::fs::read_to_string(out.join("cmdline.txt")).ok(),
        settings_mode: std::fs::read_to_string(out.join("settings-mode.txt")).ok().and_then(|t| u32::from_str_radix(t.trim(), 8).ok()),
        argv: serde_json::from_value(json_file(&out, "argv.json")).unwrap_or_default(),
        env: strs(&out, "env.json"),
        settings: json_file(&out, "settings.json"),
        mcp: json_file(&out, "mcp.json"),
        plugins: strs(&out, "plugins.json"),
        session: strs(&out, "session.json"),
        ocdir: strs(&out, "ocdir.json"),
        shell: map(&out, "shell.json"),
        mcp_run: strs(&out, "mcp-run.json"),
    };
    Run { code: outcome.code, outcome, stderr, cap }
}

/// Appends one step line to `dir/steps`, the control file of a stub agent started with `ZYGO_STUB_CONTROL=dir`
/// (the protocol is in the head of `stub.rs`). It makes `dir` and checks nothing else.
pub fn stub_control(dir: &Path, step: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    std::fs::OpenOptions::new().create(true).append(true).open(dir.join("steps"))?.write_all(format!("{step}\n").as_bytes())
}

/// The pid of the stub that serves `ctl`: polls `ctl/done` every 10 ms for a complete first line `start <pid>`.
/// After `bound` it appends the step `exit 3` (a stub that started late then ends, and its launch returns) and gives `None`.
pub fn stub_started(ctl: &Path, bound: Duration) -> Option<i32> {
    let end = Instant::now() + bound;
    loop {
        let done = std::fs::read_to_string(ctl.join("done")).unwrap_or_default();
        let pid = done.split_once('\n').and_then(|(first, _)| first.strip_prefix("start ")).and_then(|p| p.parse().ok());
        if pid.is_some() {
            return pid;
        }
        if Instant::now() >= end {
            let _ = stub_control(ctl, "exit 3");
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn git(dir: &Path, args: &[&str]) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git on PATH");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

pub fn git_repo(dir: &Path, branch: &str, remote: &str) {
    git(dir, &["init", "-q", "-b", branch]);
    git(dir, &["config", "remote.origin.url", remote]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
}

/// Every file under `dir`: relative `/` path -> bytes.
pub fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel: Vec<_> = p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(rel.join("/"), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A parent environment full of names the denylist must remove (§8.4); `MYPROVIDER_KEY` is named by a catalog provider, not by a prefix.
pub fn denied_parent() -> Vec<(OsString, OsString)> {
    let names = [
        "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY", "ZYGO_TEAM_TOKEN", "MYPROVIDER_KEY",
    ];
    let mut v: Vec<(OsString, OsString)> = names.iter().enumerate().map(|(i, n)| ((*n).into(), format!("denied-value-{}", i + 1).into())).collect();
    v.push(("ANTHROPIC_BASE_URL".into(), "http://parent.invalid".into()));
    v.push(("GH_TOKEN".into(), "gh-launch-1".into()));
    if cfg!(windows) {
        v.push(("Anthropic_Api_Key".into(), "denied-value-10".into()));
        v.push(("openai_api_key".into(), "denied-value-11".into()));
    }
    v
}
