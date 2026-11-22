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
use zygo_core::session_view::SessionView;
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
    /// Sets `NO_COLOR=1`; else `NO_COLOR` is removed.
    no_color: bool,
    /// The fixed environment of a status line run: Nerd icons, 256 colors, `COLUMNS=200`, no `ZYGO_WORK`, no
    /// `ZYGO_URL`, and on unix no controlling terminal (so `/dev/tty` gives no width).
    hud: bool,
    /// More variables of the child.
    vars: &'a [(&'a str, &'a str)],
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
    if env.no_color {
        cmd.env("NO_COLOR", "1");
    } else {
        cmd.env_remove("NO_COLOR");
    }
    if env.block_fetch {
        cmd.env("ZYGO_TEST_STATUSLINE_BLOCK", "1");
    } else {
        cmd.env_remove("ZYGO_TEST_STATUSLINE_BLOCK");
    }
    if env.hud {
        cmd.env("ZYGO_NERD_FONT", "1").env("TERM", "xterm-256color").env("COLUMNS", "200");
        for name in ["COLORTERM", "WT_SESSION", "TERM_PROGRAM", "ZYGO_WORK", "ZYGO_URL"] {
            cmd.env_remove(name);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: `setsid` is async-signal-safe and touches no memory of this process; the child has no
            // controlling terminal after it.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
    }
    cmd.envs(env.vars.iter().copied());
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
    assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), None));
    assert_eq!(parsed(&hook_prompt(d.path(), &env, "/zygo:compact on")), block("compact on for this session"));
    let s = h.session(&id).unwrap().status();
    assert_eq!((s.compact, s.effective_window), (None, Some(800_000)));
    assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), Some(800_000)));
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

/// H§14 #18, AU26: `zygo status` and `/zygo:status` of a live session print the trimmed bytes as estimated tokens
/// (92,160 bytes → `23k tok`) and the session spend; outside a session there is no `spend:` line.
#[test]
fn h14_18_status_prints_trimmed_tokens_and_spend() {
    let d = home();
    let (_h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    let both = |want: &str| {
        let (code, cli, err) = zygo(d.path(), &["status"], &env, b"");
        assert_eq!(code, 0, "{err}");
        let slash = reason(&hook_prompt(d.path(), &env, "/zygo:status"));
        for out in [&cli, &slash] {
            assert!(out.trim_end().ends_with(want), "{out}");
            assert!(!out.contains("bytes"), "{out}");
        }
    };
    // No usage store yet: the count is not known. No request yet: `$0.00` (AU15).
    both("\nsavings: cache -, trimmed - tok\nspend: $0.00");
    let store = zygo_usage::Store::open(&paths(d.path()).usage_db).unwrap();
    store.insert_trim(&zygo_usage::TrimRow { session: id.clone(), stored_bytes: 100_000, view_bytes: 7_840, ..Default::default() }).unwrap();
    store.insert_trim(&zygo_usage::TrimRow { session: "other".into(), stored_bytes: 4_000, ..Default::default() }).unwrap();
    both("\nsavings: cache -, trimmed 23k tok\nspend: $0.00");
    let (code, out, _) = zygo(d.path(), &["status"], &Env::default(), b"");
    assert_eq!(code, 0);
    assert!(!out.contains("spend:") && !out.contains("savings:"), "{out}");
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

const HUD_LOGO: &str = "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}";
const HUD_MODEL: &str = "\u{f4bc}";

/// The status line HUD of a live session (H§6, H§7): the served model, `ctx -/<window>` before the first request,
/// the threshold mark, tokens and spend from the proxy. The cache is version 2 and holds data, not a line.
#[test]
fn statusline_hud_with_and_without_override() {
    let d = home();
    let (_h, _id, dir) = proxy(d.path());
    let env = Env { dir: Some(&dir), hud: true, ..Default::default() };
    let (code, out, err) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (
            0,
            format!(
                "\u{1b}[31m▌\u{1b}[33m▌\u{1b}[36m▌\u{1b}[0m \u{1b}[1;97m{HUD_LOGO}\u{1b}[0m \u{1b}[1;34mENG-42\u{1b}[0m  \u{1b}[35m{HUD_MODEL} opus-5-5\u{1b}[0m  ctx -/\u{1b}[2m1m ⇣800k  ↑0 ↓0\u{1b}[0m  \u{1b}[32m$0.00\u{1b}[0m"
            )
            .as_str(),
            ""
        )
    );
    let cached = std::fs::read_to_string(dir.join("statusline.json")).unwrap();
    let cache: Value = serde_json::from_str(&cached).unwrap();
    assert_eq!((cache["v"].as_u64(), cache["outcome"].as_str()), (Some(2), Some("ok")), "{cached}");
    assert_eq!(cache["status"]["work"].as_str(), Some("ENG-42"), "{cached}");
    assert!(!cached.contains("\"line\""), "{cached}");
    assert!(!dir.join("statusline.lock").exists());
    assert!(!paths(d.path()).usage_db.exists());
    // P-HUD-1: the session log names the width source and the width, once.
    let log = std::fs::read_to_string(dir.join("session.log")).unwrap();
    assert_eq!(log, "statusline: width source COLUMNS, width 200\n");

    std::fs::remove_file(dir.join("statusline.json")).unwrap();
    SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
    let env = Env { no_color: true, ..env };
    let (code, out, _) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    assert_eq!((code, out), (0, format!("{HUD_LOGO} ENG-42  {HUD_MODEL} opus-5-5  ctx -/1m ⇣600k  ↑0 ↓0  $0.00")));
    assert!(!paths(d.path()).usage_db.exists());
    assert_eq!(std::fs::read_to_string(dir.join("session.log")).unwrap(), log, "the width did not change");

    // Bad stdin does not break the HUD (AU24); the cache answers within 30 s.
    for stdin in [&b""[..], b"[]", b"not json", b"{\"cwd\":"] {
        let (code, out, err) = zygo(d.path(), &["hook", "statusline"], &env, stdin);
        assert_eq!((code, out, err), (0, format!("{HUD_LOGO} ENG-42  {HUD_MODEL} opus-5-5  ctx -/1m ⇣600k  ↑0 ↓0  $0.00"), String::new()));
    }
}

/// Without `ZYGO_SESSION_DIR` the hook prints nothing (H§6.8).
#[test]
fn statusline_without_a_session_dir_prints_nothing() {
    let d = home();
    let env = Env { hud: true, ..Default::default() };
    assert_eq!(zygo(d.path(), &["hook", "statusline"], &env, b"{}"), (0, String::new(), String::new()));
}

/// AU1: the style of a session is `statusline` of `overrides.json`, else `statusline_style` of `view.json`. A value
/// other than `compact` or `verbose` in `overrides.json` is ignored.
#[test]
fn statusline_style_from_the_overrides_then_the_view() {
    let d = home();
    let (_h, id, dir) = proxy(d.path());
    let sd = SessionDir::new(dir.clone());
    let env = Env { dir: Some(&dir), hud: true, no_color: true, ..Default::default() };
    let rows = |want: usize| {
        let (code, out, err) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
        assert_eq!((code, err.as_str(), out.lines().count()), (0, "", want), "{out}");
        out
    };
    rows(1);
    SessionView { session: id.clone(), statusline_style: "verbose".into(), ..Default::default() }.save(&sd).unwrap();
    let verbose = rows(2);
    assert_eq!(
        verbose,
        format!("{HUD_LOGO}  ENG-42   {HUD_MODEL} opus-5-5   local\nz\u{28e}\u{183}o  ctx -/1m ⇣800k   ↑ 0 in  ↓ 0 out   $0.00")
    );
    std::fs::write(sd.overrides_json(), r#"{"statusline":"wide"}"#).unwrap();
    assert_eq!(rows(2), verbose);
    std::fs::write(sd.overrides_json(), r#"{"statusline":"compact"}"#).unwrap();
    rows(1);
    SessionView { session: id, ..Default::default() }.save(&sd).unwrap();
    std::fs::write(sd.overrides_json(), r#"{"statusline":"verbose"}"#).unwrap();
    assert_eq!(rows(2), verbose);
}

/// H§14 #5: `zygo work <item>` in the session changes the repository setting only; the HUD still shows the
/// launch-time item of `ZYGO_WORK`.
#[test]
fn h14_05_hud_keeps_the_launch_time_item_after_zygo_work() {
    let d = home();
    let (_h, id, dir) = proxy(d.path());
    // `zygo work` needs a repository.
    let init = Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(d.path()).output().unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    let hud = Env { dir: Some(&dir), hud: true, no_color: true, vars: &[("ZYGO_WORK", "github:o/n#123")], ..Default::default() };
    let before = zygo(d.path(), &["hook", "statusline"], &hud, b"{}");
    assert!(before.1.starts_with(&format!("{HUD_LOGO} \u{f408} #123  ")), "{before:?}");
    let session = Env { session: Some(&id), dir: Some(&dir), ..Default::default() };
    let (code, out, err) = zygo(d.path(), &["work", "ENG-9"], &session, b"");
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(zygo(d.path(), &["hook", "statusline"], &hud, b"{}"), before);
    std::fs::remove_file(dir.join("statusline.json")).unwrap();
    assert_eq!(zygo(d.path(), &["hook", "statusline"], &hud, b"{}"), before);
}

/// H§14 #23 on the binary: with the fetch seam and the git seam blocking, the hook ends within its 3 s bound and
/// prints the HUD with `offline`. An old `{at, line}` cache file counts as no cache.
#[test]
fn statusline_ends_within_the_bound_when_the_workers_block() {
    let d = home();
    let dir = dead_session(d.path(), "S4");
    let env = Env {
        dir: Some(&dir),
        block_fetch: true,
        hud: true,
        no_color: true,
        vars: &[("ZYGO_TEST_GIT_DELAY_MS", "20000")],
        ..Default::default()
    };
    // The bound is 3 s from the start of the hook; 700 ms covers process start and exit.
    let limit = std::time::Duration::from_millis(3700);
    let t = std::time::Instant::now();
    let (code, out, err) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    let took = t.elapsed();
    assert_eq!((code, out, err), (0, format!("{HUD_LOGO}  offline"), String::new()));
    assert!(took < limit, "no cache: took {took:?}");
    assert!(took > std::time::Duration::from_millis(2000), "the fetch gets the time to the deadline: took {took:?}");
    // The failed try is in the cache: the next run does not wait.
    let cache: Value = serde_json::from_slice(&std::fs::read(dir.join("statusline.json")).unwrap()).unwrap();
    assert_eq!((cache["v"].as_u64(), cache["outcome"].as_str()), (Some(2), Some("offline")));
    let t = std::time::Instant::now();
    assert_eq!(zygo(d.path(), &["hook", "statusline"], &env, b"{}").1, format!("{HUD_LOGO}  offline"));
    assert!(t.elapsed() < std::time::Duration::from_millis(2000), "backoff: took {:?}", t.elapsed());
    // An old cache file holds a line: it is not printed, and the fetch runs again.
    std::fs::write(dir.join("statusline.json"), r#"{"at":1,"line":"zygo ◇ cached"}"#).unwrap();
    let t = std::time::Instant::now();
    let (code, out, _) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    let took = t.elapsed();
    assert_eq!((code, out), (0, format!("{HUD_LOGO}  offline")));
    assert!(took < limit, "old cache: took {took:?}");
}

/// H§14 #30 (C6): a forced panic in the render gives empty stdout, exit 0 and one stderr line.
#[test]
fn h14_30_render_panic_prints_nothing_and_exits_0() {
    let d = home();
    let (_h, _id, dir) = proxy(d.path());
    let env = Env { dir: Some(&dir), hud: true, vars: &[("ZYGO_TEST_RENDER_PANIC", "1")], ..Default::default() };
    let (code, out, err) = zygo(d.path(), &["hook", "statusline"], &env, b"{}");
    assert_eq!((code, out.as_str()), (0, ""));
    assert_eq!(err, "zygo hook: forced by ZYGO_TEST_RENDER_PANIC\n");
    assert!(!dir.join("statusline.lock").exists());
    // Without the seam the same session prints its HUD.
    let env = Env { dir: Some(&dir), hud: true, ..Default::default() };
    assert!(zygo(d.path(), &["hook", "statusline"], &env, b"{}").1.contains("opus-5-5"));
}

#[test]
fn trailing_newline_after_args_dispatches() {
    let d = home();
    let (h, id, dir) = proxy(d.path());
    let env = Env { session: Some(&id), dir: Some(&dir), agent: None, ..Default::default() };
    assert_eq!(parsed(&hook_prompt(d.path(), &env, "/zygo:compact 600k\n")), block("compact 600k for this session"));
    assert_eq!(compact_of(&h, &id), Some(WindowArg::Tokens(600_000)));
}

