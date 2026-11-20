//! N49 acceptance (§17.1, §20 #17b): the session setting verbs `results`, `redaction`, `guards`, `units` and
//! `compact-tool`, slash and CLI.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::{load, LoadContext};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
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

/// Session environment of one child: `ZYGO_SESSION`, `ZYGO_SESSION_DIR`, `ZYGO_AGENT`; `None` removes the name.
#[derive(Default, Clone)]
struct Env<'a> {
    session: Option<&'a str>,
    dir: Option<&'a Path>,
    agent: Option<&'a str>,
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

fn parsed(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

/// The block reason without its `zygo: ` prefix.
fn reason(out: &str) -> String {
    let v = parsed(out);
    v["reason"].as_str().unwrap().strip_prefix("zygo: ").unwrap().to_string()
}

fn cli(home: &Path, env: &Env, line: &str) -> (i32, String, String) {
    zygo(home, &line.split(' ').collect::<Vec<_>>(), env, b"")
}

fn in_s<'a>(id: &'a str, dir: &'a Path) -> Env<'a> {
    Env { session: Some(id), dir: Some(dir), agent: None }
}

fn ovr(dir: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(dir.join("overrides.json"))
}

fn user_config(home: &Path) -> toml::Table {
    std::fs::read_to_string(home.join("config.toml")).unwrap().parse().unwrap()
}

fn compact_tool(home: &Path) -> Option<bool> {
    user_config(home).get("features")?.get("compact_tool")?.as_bool()
}

#[test]
fn slash_and_cli_same_reply_and_effect() {
    let d = home();
    let home = d.path();
    let (h1, id1, s1) = proxy(home);
    let (h2, id2, s2) = proxy(home);
    for line in ["results off", "results threshold 32768", "units off", "guards claims enforce", "redaction content off"] {
        assert_eq!(reason(&hook_prompt(home, &in_s(&id1, &s1), &format!("/zygo:{line}"))), format!("{line} for this session"));
        assert_eq!(cli(home, &in_s(&id2, &s2), line), (0, format!("{line} for this session\n"), String::new()), "{line}");
        assert_eq!(ovr(&s1).unwrap(), ovr(&s2).unwrap(), "{line}");
    }
    assert_eq!(h1.session(&id1).unwrap().settings().redaction_content, Some(false));
    assert_eq!(h2.session(&id2).unwrap().settings().redaction_content, Some(false));

    for line in ["guards", "redaction"] {
        let slash = reason(&hook_prompt(home, &in_s(&id1, &s1), &format!("/zygo:{line}")));
        let (code, out, _) = cli(home, &in_s(&id2, &s2), line);
        assert_eq!(code, 0, "{line}");
        assert_eq!(out, format!("{slash}\n"), "{line}");
        match line {
            "guards" => assert!(slash.starts_with("claims enforce (session)\n"), "{slash}"),
            _ => assert_eq!(slash, "secrets on (status only), content off"),
        }
    }

    let slash = reason(&hook_prompt(home, &in_s(&id1, &s1), "/zygo:compact-tool off"));
    assert_eq!(slash, "compact-tool off (applies at next launch)");
    assert_eq!(compact_tool(home), Some(false));
    std::fs::write(home.join("config.toml"), "").unwrap();
    assert_eq!(
        cli(home, &in_s(&id2, &s2), "compact-tool off"),
        (0, "compact-tool off (applies at next launch)\n".into(), String::new())
    );
    assert_eq!(compact_tool(home), Some(false));

    for (line, code, text) in [
        ("redaction off", 1, "refused: secret redaction cannot be turned off in a session"),
        ("units maybe", 2, "zygo units: usage: units on|off"),
    ] {
        let (c, out, err) = cli(home, &in_s(&id2, &s2), line);
        assert_eq!((c, out.as_str()), (code, ""), "{line}");
        assert_eq!(err, format!("{}\n", reason(&hook_prompt(home, &in_s(&id1, &s1), &format!("/zygo:{line}")))), "{line}");
        assert_eq!(err, format!("{text}\n"), "{line}");
    }
}

#[test]
fn session_change_is_gone_in_the_next_session() {
    let d = home();
    let home = d.path();
    let (_h1, id1, s1) = proxy(home);
    assert_eq!(cli(home, &in_s(&id1, &s1), "units off").0, 0);
    assert!(cli(home, &in_s(&id1, &s1), "help units").1.ends_with("now: off\n"));
    let (_h2, id2, s2) = proxy(home);
    assert_eq!(cli(home, &in_s(&id2, &s2), "help units").1, "units on|off\nWork units\nscope: session, --save\nnow: on\n");
    assert!(!s2.join("overrides.json").exists());
    assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), "");
}

#[test]
fn save_writes_user_config() {
    let d = home();
    let home = d.path();
    let (_h1, id1, s1) = proxy(home);
    assert_eq!(
        cli(home, &in_s(&id1, &s1), "units off --save"),
        (0, "units off for this session; saved to user config\n".into(), String::new())
    );
    assert_eq!(user_config(home)["features"]["work_units"].as_bool(), Some(false));
    let (_h2, id2, s2) = proxy(home);
    assert!(cli(home, &in_s(&id2, &s2), "help units").1.ends_with("now: off\n"));

    assert_eq!(
        cli(home, &Env::default(), "results threshold 32768 --save"),
        (0, "results threshold 32768 saved to user config\n".into(), String::new())
    );
    assert_eq!(user_config(home)["results"]["trim_threshold_bytes"].as_integer(), Some(32768));

    let slash = reason(&hook_prompt(home, &in_s(&id1, &s1), "/zygo:guards claims enforce --save"));
    assert_eq!(slash, "guards claims enforce for this session; saved to user config");
    assert_eq!(user_config(home)["guards"]["claims"].as_str(), Some("enforce"));
}

#[test]
fn limits_are_refused_with_the_limit_named() {
    let d = home();
    let home = d.path();
    let floor = "[guards]\nmin_level = \"notify\"\n";
    std::fs::write(home.join("config.toml"), floor).unwrap();
    let (_h1, id1, s1) = proxy(home);
    let env = in_s(&id1, &s1);
    assert_eq!(
        cli(home, &env, "guards claims monitor"),
        (1, String::new(), "refused: guard claims cannot go below min_level notify\n".into())
    );
    assert_eq!(
        reason(&hook_prompt(home, &env, "/zygo:guards claims monitor")),
        "refused: guard claims cannot go below min_level notify"
    );
    let (code, _, err) = cli(home, &env, "redaction off");
    assert_eq!((code, err.as_str()), (1, "refused: secret redaction cannot be turned off in a session\n"));
    assert!(!s1.join("overrides.json").exists());
    assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), floor);
}

#[test]
fn outside_a_session_needs_save_and_compact_tool_writes_user_config() {
    let d = home();
    let home = d.path();
    let env = Env::default();
    assert_eq!(
        cli(home, &env, "units off"),
        (2, String::new(), "zygo units: not in a zygo session; add --save to change user config\n".into())
    );
    assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), "");
    assert_eq!(
        cli(home, &env, "redaction off"),
        (1, String::new(), "refused: secret redaction cannot be turned off in a session\n".into())
    );
    assert_eq!(
        cli(home, &env, "compact-tool off"),
        (0, "compact-tool off (applies at next launch)\n".into(), String::new())
    );
    assert_eq!(compact_tool(home), Some(false));
}
