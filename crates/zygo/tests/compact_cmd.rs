//! N28 acceptance (§20 #17): the `/zygo:` dispatcher, `compact`, `help`, `status` and the status line, slash and CLI.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::{load, LoadContext};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::verbs::VERB_SPECS;
use zygo_core::Agent;
use zygo_proxy::client::{SessionClient, WindowArg};
use zygo_proxy::session::SessionRequest;
use zygo_proxy::{Proxy, ProxyHandle};

fn home() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.toml"), "").unwrap();
    d
}

/// The `Paths` that `Paths::from_env` gives for `HOME=LOCALAPPDATA=APPDATA=home`, `ZYGO_CONFIG=home/config.toml`.
fn paths(home: &Path) -> Paths {
    #[cfg(unix)]
    let (cache_dir, data_dir) = (home.join(".cache/zygo"), home.join(".local/share/zygo"));
    #[cfg(windows)]
    let (cache_dir, data_dir) = (home.join("zygo").join("cache"), home.join("zygo"));
    Paths {
        config_file: home.join("config.toml"),
        sessions_dir: cache_dir.join("sessions"),
        marketplaces_dir: cache_dir.join("marketplaces"),
        plugin_store: data_dir.join("plugins"),
        approvals_file: data_dir.join("approvals.toml"),
        usage_db: data_dir.join("usage.db"),
        cache_dir,
        data_dir,
    }
}

/// A live loopback proxy with one session (work `ENG-42`, soft window 800k); returns the session id and its dir.
fn proxy(home: &Path) -> (ProxyHandle, String, PathBuf) {
    let mut m = load(&LoadContext { paths: paths(home), repo: None, team: None }).unwrap();
    m.config.catalog.0.iter_mut().find(|r| r.id == "claude-opus-5-5").unwrap().soft_context_window = Some(800_000);
    let h = Proxy::start_local(Arc::new(m)).unwrap();
    let g = h
        .create_session(SessionRequest {
            agent: Agent::ClaudeCode,
            agent_version: "2.1.328".into(),
            profile: None,
            passthrough: false,
            repo: None,
            work: Some("ENG-42".into()),
            stored_values: vec![],
            known_plugins: Default::default(),
        })
        .unwrap();
    let dir = paths(home).sessions_dir.join(&g.session);
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    sd.write_token(&g.token).unwrap();
    std::fs::write(sd.url(), h.url().as_str()).unwrap();
    (h, g.session, dir)
}

/// A session dir `id` whose url is a closed loopback port.
fn dead_session(home: &Path, id: &str) -> PathBuf {
    let dir = paths(home).sessions_dir.join(id);
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    sd.write_token("t").unwrap();
    std::fs::write(sd.url(), closed_url()).unwrap();
    dir
}

fn closed_url() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}/")
}

/// Session environment of one child: `ZYGO_SESSION`, `ZYGO_SESSION_DIR`, `ZYGO_AGENT`; `None` removes the name.
#[derive(Default, Clone)]
struct Env<'a> {
    session: Option<&'a str>,
    dir: Option<&'a Path>,
    agent: Option<&'a str>,
    /// Sets `ZYGO_TEST_STATUSLINE_BLOCK`: the status line's fetch blocks (feature `test-hooks`).
    block_fetch: bool,
}

fn zygo(home: &Path, args: &[&str], env: &Env, stdin: &[u8]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    cmd.args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("LOCALAPPDATA", home)
        .env("APPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"));
    for (k, v) in [("ZYGO_SESSION", env.session.map(Path::new)), ("ZYGO_SESSION_DIR", env.dir), ("ZYGO_AGENT", env.agent.map(Path::new))] {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    if env.block_fetch {
        cmd.env("ZYGO_TEST_STATUSLINE_BLOCK", "1");
    } else {
        cmd.env_remove("ZYGO_TEST_STATUSLINE_BLOCK");
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

/// `zygo hook prompt` with `line` as the prompt; asserts exit 0 and returns stdout.
fn hook_prompt(home: &Path, env: &Env, line: &str) -> String {
    let stdin = json!({ "hook_event_name": "UserPromptSubmit", "prompt": line }).to_string();
    let (code, out, _) = zygo(home, &["hook", "prompt"], env, stdin.as_bytes());
    assert_eq!(code, 0);
    out
}

fn block(text: &str) -> Value {
    json!({
        "decision": "block",
        "reason": format!("zygo: {text}"),
        "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "suppressOriginalPrompt": true }
    })
}

fn parsed(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

/// The block reason without its `zygo: ` prefix.
fn reason(out: &str) -> String {
    let v = parsed(out);
    v["reason"].as_str().unwrap().strip_prefix("zygo: ").unwrap().to_string()
}

fn compact_of(h: &ProxyHandle, id: &str) -> Option<WindowArg> {
    h.session(id).unwrap().status().compact
}

#[test]
fn slash_compact_600k_sets_the_override() {
    let d = home();
    let (h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    assert_eq!(parsed(&hook_prompt(d.path(), &env, "/zygo:compact 600k")), block("compact 600k for this session"));
    assert_eq!(compact_of(&h, &id), Some(WindowArg::Tokens(600_000)));
}

#[test]
fn slash_off_disables_and_on_clears() {
    let d = home();
    let (h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    assert_eq!(parsed(&hook_prompt(d.path(), &env, "/zygo:compact off")), block("compact off for this session"));
    let s = h.session(&id).unwrap().status();
    assert_eq!((s.compact, s.effective_window), (Some(WindowArg::Off), None));
    assert_eq!(parsed(&hook_prompt(d.path(), &env, "/zygo:compact on")), block("compact on for this session"));
    let s = h.session(&id).unwrap().status();
    assert_eq!((s.compact, s.effective_window), (None, Some(800_000)));
}

#[test]
fn cli_and_slash_same_reply_and_effect() {
    let d = home();
    let (h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    for a in ["600k", "off", "on"] {
        assert_eq!(parsed(&hook_prompt(d.path(), &env, &format!("/zygo:compact {a}"))), block(&format!("compact {a} for this session")));
        let slash = compact_of(&h, &id);
        // Reset so that the CLI run must make the change itself.
        SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(5_000_000)).unwrap();
        let (code, out, err) = zygo(d.path(), &["compact", a], &env, b"");
        assert_eq!((code, out.as_str(), err.as_str()), (0, format!("compact {a} for this session\n").as_str(), ""));
        assert_eq!(compact_of(&h, &id), slash, "{a}");
    }
    let (code, _, err) = zygo(d.path(), &["compact", "600x"], &env, b"");
    assert_eq!(code, 2);
    let t = reason(&hook_prompt(d.path(), &env, "/zygo:compact 600x"));
    assert!(t.starts_with("compact failed: bad value `600x`"), "{t}");
    assert_eq!(err, format!("{t}\n"));
    // `help` outside a session: same text. Inside one the CLI hides the window value (D6, C7), the slash form shows it.
    let (code, out, _) = zygo(d.path(), &["help", "compact"], &Env::default(), b"");
    assert_eq!(code, 0);
    let t = reason(&hook_prompt(d.path(), &Env::default(), "/zygo:help compact"));
    assert_eq!(out, format!("{t}\n"));
    let t = reason(&hook_prompt(d.path(), &env, "/zygo:help compact"));
    assert!(t.contains("now: on"), "{t}");
}

#[test]
fn cli_in_the_agent_shell_hides_the_window() {
    let d = home();
    let (_h, id, dir) = proxy(d.path());
    SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    let (c1, status, _) = zygo(d.path(), &["status"], &env, b"");
    let (c2, help, _) = zygo(d.path(), &["help"], &env, b"");
    assert_eq!((c1, c2), (0, 0));
    assert!(status.lines().any(|l| l == "compact: override set"), "{status}");
    assert!(status.lines().any(|l| l == "context: -"), "{status}");
    assert!(help.contains("(now: override set)"), "{help}");
    for t in [&status, &help] {
        assert!(!t.contains("600k") && !t.contains("800k"), "{t}");
    }
    let status = reason(&hook_prompt(d.path(), &env, "/zygo:status"));
    let help = reason(&hook_prompt(d.path(), &env, "/zygo:help"));
    assert!(status.lines().any(|l| l == "compact: 600k"), "{status}");
    assert!(status.lines().any(|l| l == "context: -/600k"), "{status}");
    assert!(help.contains("(now: 600k)"), "{help}");
}

#[test]
fn cli_with_only_the_session_dir_hides_the_window() {
    let d = home();
    let (_h, _id, dir) = proxy(d.path());
    SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
    let env = Env { session: None, dir: Some(&dir), agent: None, ..Default::default() };
    let (c1, status, _) = zygo(d.path(), &["status"], &env, b"");
    let (c2, help, _) = zygo(d.path(), &["help"], &env, b"");
    assert_eq!((c1, c2), (0, 0));
    assert!(status.lines().any(|l| l == "compact: override set"), "{status}");
    assert!(help.contains("(now: override set)"), "{help}");
    for t in [&status, &help] {
        assert!(!t.contains("600k") && !t.contains("800k"), "{t}");
    }
}

#[test]
fn unknown_verb_blocks_with_the_help_text() {
    let d = home();
    let env = Env::default();
    let unknown = reason(&hook_prompt(d.path(), &env, "/zygo:frob x"));
    let help = reason(&hook_prompt(d.path(), &env, "/zygo:help"));
    assert_eq!(unknown, help);
    let lines: Vec<&str> = help.lines().collect();
    assert_eq!(lines.len(), 1 + VERB_SPECS.len(), "{help}");
    for (i, s) in VERB_SPECS.iter().enumerate() {
        assert!(lines[i + 1].starts_with(s.name), "{}", lines[i + 1]);
    }
}

#[test]
fn cli_compact_exit_codes() {
    let d = home();
    let (h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: Some("opencode"), ..Default::default() };
    let (code, _, err) = zygo(d.path(), &["compact", "600k"], &env, b"");
    assert_eq!((code, err.as_str()), (2, "OpenCode reads its window at start; relaunch with zygo opencode --compact <n>\n"));
    assert_eq!(compact_of(&h, &id), None);

    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    assert_eq!(zygo(d.path(), &["compact", "600x"], &env, b"").0, 2);
    assert_eq!(zygo(d.path(), &["compact", "600k"], &Env::default(), b"").0, 2);
    dead_session(d.path(), "S2");
    let (code, _, err) = zygo(d.path(), &["compact", "600k", "--session", "S2"], &Env::default(), b"");
    assert_eq!(code, 12);
    assert!(err.starts_with("compact failed: zygo proxy unreachable"), "{err}");
    assert_eq!(zygo(d.path(), &["compact", "600k", "--save"], &env, b"").0, 2);
    assert_eq!(compact_of(&h, &id), None);
}

#[test]
fn slash_failure_blocks_with_compact_failed() {
    let d = home();
    let dir = dead_session(d.path(), "S3");
    let env = Env { session: Some("S3"), dir: Some(&dir), agent: None, ..Default::default() };
    let out = hook_prompt(d.path(), &env, "/zygo:compact 600k");
    let r = parsed(&out)["reason"].as_str().unwrap().to_string();
    assert!(r.starts_with("zygo: compact failed: zygo proxy unreachable"), "{r}");
}

#[test]
fn statusline_with_and_without_override() {
    let d = home();
    let (_h, _id, dir) = proxy(d.path());
    let env = Env { session: None, dir: Some(&dir), agent: None, ..Default::default() };
    let (code, out, _) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    assert_eq!((code, out.as_str()), (0, "zygo ◇ ENG-42 ◇ - ◇ ctx -/800k ◇ cache - ◇ trimmed -"));
    assert!(!paths(d.path()).usage_db.exists());
    std::fs::remove_file(dir.join("statusline.json")).unwrap();
    SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
    let (code, out, _) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    assert_eq!((code, out.as_str()), (0, "zygo ◇ ENG-42 ◇ - ◇ ctx -/600k ◇ compact 600k ◇ cache - ◇ trimmed -"));
    assert!(!paths(d.path()).usage_db.exists());
}

#[test]
fn trailing_newline_after_args_dispatches() {
    let d = home();
    let (h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    assert_eq!(parsed(&hook_prompt(d.path(), &env, "/zygo:compact 600k\n")), block("compact 600k for this session"));
    assert_eq!(compact_of(&h, &id), Some(WindowArg::Tokens(600_000)));
}

#[test]
fn statusline_ends_within_the_bound_when_the_fetch_blocks() {
    let d = home();
    let dir = dead_session(d.path(), "S4");
    let env = Env { dir: Some(&dir), block_fetch: true, ..Default::default() };
    // The bound is 3 s from the start of the hook; 700 ms covers process start and exit.
    let limit = std::time::Duration::from_millis(3700);
    let t = std::time::Instant::now();
    let (code, out, _) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    let took = t.elapsed();
    assert_eq!((code, out.as_str()), (0, ""));
    assert!(took < limit, "no cache: took {took:?}");
    // A cached line of any age is printed on a timeout.
    std::fs::write(dir.join("statusline.json"), r#"{"at":1,"line":"zygo ◇ cached"}"#).unwrap();
    let t = std::time::Instant::now();
    let (code, out, _) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    let took = t.elapsed();
    assert_eq!((code, out.as_str()), (0, "zygo ◇ cached"));
    assert!(took < limit, "stale cache: took {took:?}");
}
