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

#[cfg(unix)]
mod member_fixture;
#[cfg(unix)]
use member_fixture as mf;

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
    /// More variables for the child, set after the session variables.
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
    cmd.env_remove("ZYGO_WORKSPACE");
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
    Env { session: Some(id), dir: Some(dir), agent: None, ..Default::default() }
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

/// The slash gate of a member (the launcher sets it for members, AU58).
#[cfg(unix)]
const MEMBER_VARS: &[(&str, &str)] = &[("ZYGO_WORKSPACE", "0123456789abcdef")];

/// The member session folder `home/s` of zygo session `SID` and the state with its running record.
#[cfg(unix)]
fn member(h: &Path) -> (PathBuf, zygo_workspace::state::Store) {
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    (s.clone(), mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::SID))], None))
}

/// `fake_tmux` variables as `&str` pairs for `Env.vars`.
#[cfg(unix)]
fn as_vars<'a>(v: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    v.iter().map(|(k, s)| (*k, s.as_str())).collect()
}

/// AU105 (a), OP12: after a final change, `member.overrides` is the whole new `overrides.json`, CLI and slash.
#[cfg(unix)]
#[test]
fn au105_setting_change_mirrors_overrides_into_the_member_record() {
    let d = home();
    let h = d.path();
    let (s, store) = member(h);
    let env = Env { session: Some(mf::SID), dir: Some(&s), vars: MEMBER_VARS, ..Default::default() };
    assert_eq!(cli(h, &env, "results off"), (0, "results off for this session\n".into(), String::new()));
    assert_eq!(mf::ovr_json(&s), json!({ "results": false, "guards": {} }));
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(mf::ovr_json(&s)));
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:guards claims enforce")), "guards claims enforce for this session");
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(json!({ "results": false, "guards": { "claims": "enforce" } })));
    assert_eq!(cli(h, &env, "units off --save"), (0, "units off for this session; saved to user config\n".into(), String::new()));
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(mf::ovr_json(&s)));
    assert_eq!(mf::record(&store, mf::WS_KEY).compact, None);
}

/// AU105: the mirror writes the file as it is under the lock, not the verb's own change: the stand-in proxy rewrites
/// `overrides.json` while it answers the PATCH, between the save and the mirror. A mirror of the in-memory change
/// would lack `work_units`.
#[cfg(unix)]
#[test]
fn au105_mirror_reads_the_file_not_the_change() {
    let d = home();
    let h = d.path();
    const TEXT: &str = r#"{"redaction_content":false,"guards":{},"work_units":false}"#;
    let s = mf::scripted_session_at(&paths(h).sessions_dir, "S4", Some(TEXT));
    mf::view(&s, "S4", mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running("S4"))], None);
    let env = Env { session: Some("S4"), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "redaction content off"), (0, "redaction content off for this session\n".into(), String::new()));
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(json!({ "redaction_content": false, "guards": {}, "work_units": false })));
}

/// AU105: a `redaction content` change that the proxy refuses is rolled back and not mirrored.
#[cfg(unix)]
#[test]
fn au105_refused_patch_is_not_mirrored() {
    let d = home();
    let h = d.path();
    let s = mf::scripted_session_at(&paths(h).sessions_dir, "S1", None);
    mf::view(&s, "S1", mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running("S1"))], None);
    std::fs::write(s.join("overrides.json"), br#"{"work_units":false}"#).unwrap();
    let env = Env { session: Some("S1"), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "redaction content off"), (1, String::new(), "redaction failed: zygo_refused: no\n".into()));
    assert_eq!(std::fs::read(s.join("overrides.json")).unwrap(), br#"{"work_units":false}"#);
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, None);
}

/// M4 review F3, W§7.3: the mirror reads `overrides.json` under the state lock without waiting on a FIFO. The stand-in
/// proxy puts a FIFO in its place after the save; the reply comes at once with `; not saved for the workspace` and the
/// record does not change. A test thread opens the FIFO for writing (and closes it) only after 3 s, so a reader that
/// waits on the FIFO ends then and the time shows it.
#[cfg(unix)]
#[test]
fn au105_fifo_overrides_is_not_mirrored_and_does_not_wait() {
    fn to_fifo(file: &Path) {
        std::fs::remove_file(file).unwrap();
        let c = std::ffi::CString::new(file.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }
    let d = home();
    let h = d.path();
    let s = mf::patching_session_at(&paths(h).sessions_dir, "S5", to_fifo);
    mf::view(&s, "S5", mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running("S5"))], None);
    let fifo = s.join("overrides.json");
    let t0 = std::time::Instant::now();
    let unblock = std::thread::spawn(move || {
        use std::os::unix::fs::OpenOptionsExt;
        std::thread::sleep(std::time::Duration::from_secs(3));
        // A writer that does not wait: it opens only while a reader waits on the FIFO, then closes it (EOF).
        while t0.elapsed() < std::time::Duration::from_secs(6) {
            if std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(&fifo).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let env = Env { session: Some("S5"), dir: Some(&s), ..Default::default() };
    let got = cli(h, &env, "redaction content off");
    let took = t0.elapsed();
    unblock.join().unwrap();
    assert_eq!(got, (0, "redaction content off for this session; not saved for the workspace\n".into(), String::new()));
    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    assert_eq!(mf::record(&store, mf::WS_KEY), mf::running("S5"));
}

/// M4 review F3, W-C7: an `overrides.json` that is a link is not followed, so no other file's JSON reaches
/// `workspace.json`. The stand-in proxy puts a link to another JSON file in its place after the save.
#[cfg(unix)]
#[test]
fn au105_linked_overrides_is_not_mirrored() {
    fn to_link(file: &Path) {
        let foreign = file.with_file_name("foreign.json");
        std::fs::write(&foreign, br#"{"results":false,"foreign":true}"#).unwrap();
        std::fs::remove_file(file).unwrap();
        std::os::unix::fs::symlink(&foreign, file).unwrap();
    }
    let d = home();
    let h = d.path();
    let s = mf::patching_session_at(&paths(h).sessions_dir, "S6", to_link);
    mf::view(&s, "S6", mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running("S6"))], None);
    let env = Env { session: Some("S6"), dir: Some(&s), ..Default::default() };
    assert_eq!(
        cli(h, &env, "redaction content off"),
        (0, "redaction content off for this session; not saved for the workspace\n".into(), String::new())
    );
    assert!(std::fs::symlink_metadata(s.join("overrides.json")).unwrap().file_type().is_symlink());
    assert_eq!(mf::record(&store, mf::WS_KEY), mf::running("S6"));
}

/// AU105: a PATCH lost to a transport error keeps the new `overrides.json`, so it is mirrored; text and exit as today.
#[cfg(unix)]
#[test]
fn au105_lost_patch_is_mirrored() {
    let d = home();
    let h = d.path();
    let s = mf::dead_session_at(&paths(h).sessions_dir, "S2");
    mf::view(&s, "S2", mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running("S2"))], None);
    let env = Env { session: Some("S2"), dir: Some(&s), ..Default::default() };
    let (code, out, err) = cli(h, &env, "redaction content off");
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("redaction failed: zygo proxy unreachable: "), "{err}");
    assert!(err.ends_with("; the session may already use the new setting\n"), "{err}");
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(json!({ "redaction_content": false, "guards": {} })));
}

/// AU105: `; not saved for the workspace` goes at the end of the reply or error text, CLI and slash. The mirror fails
/// because the record belongs to another zygo session. The lost-PATCH detail is the system's text, so that case
/// checks the exact start and end; the failed `--save` write names the lock file, so that case is exact.
#[cfg(unix)]
#[test]
fn au105_not_saved_text_goes_at_the_end() {
    let d = home();
    let h = d.path();
    let s = mf::dead_session_at(&paths(h).sessions_dir, "S3");
    mf::view(&s, "S3", mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::OTHER))], None);
    let env = Env { session: Some("S3"), dir: Some(&s), vars: MEMBER_VARS, ..Default::default() };
    assert_eq!(
        cli(h, &env, "units off --save"),
        (0, "units off for this session; saved to user config; not saved for the workspace\n".into(), String::new())
    );
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:units on --save")), "units on for this session; saved to user config; not saved for the workspace");
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:results off")), "results off for this session; not saved for the workspace");
    let (code, out, err) = cli(h, &env, "redaction content off");
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("redaction failed: zygo proxy unreachable: "), "{err}");
    assert!(err.ends_with("; the session may already use the new setting; not saved for the workspace\n"), "{err}");
    // A user config in a folder that cannot be written: the `--save` write fails after the session change.
    use std::os::unix::fs::PermissionsExt;
    let cfg = h.join("cfg");
    std::fs::create_dir(&cfg).unwrap();
    std::fs::write(cfg.join("config.toml"), "").unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(cfg.join("probe"), "").is_ok() {
        eprintln!("--save failure case skipped: this user writes into a folder of mode 0500");
    } else {
        let cfg_file = cfg.join("config.toml").display().to_string();
        let vars = [("ZYGO_WORKSPACE", mf::WS_ID), ("ZYGO_CONFIG", cfg_file.as_str())];
        let env = Env { session: Some("S3"), dir: Some(&s), vars: &vars, ..Default::default() };
        assert_eq!(
            cli(h, &env, "units off --save"),
            (1, String::new(), format!("units off for this session; saving to user config failed: {cfg_file}.lock; not saved for the workspace\n"))
        );
    }
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(mf::record(&store, mf::WS_KEY), mf::running(mf::OTHER));
}

/// W§18 #70: with `workspace.json` locked by the test for 2 s, `zygo results off` in a member replies with
/// `; not saved for the workspace` and exits as without it (AU105).
#[cfg(unix)]
#[test]
fn w18_70_locked_state_replies_not_saved_and_exits_as_without_it() {
    let d = home();
    let h = d.path();
    let (s, store) = member(h);
    let t0 = std::time::Instant::now();
    let held = mf::hold(store.dir().join("lock"));
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "results off"), (0, "results off for this session; not saved for the workspace\n".into(), String::new()));
    mf::release_after(held, t0, std::time::Duration::from_secs(2));
    assert_eq!(mf::ovr_json(&s), json!({ "results": false, "guards": {} }));
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, None);
}

/// AU105, decision 2026-11-23: the slash form's lock wait lives inside the 4.5 s hook bound. With the lock held
/// past the hook bound, the reply ends with `; not saved for the workspace` and comes inside the bound.
#[cfg(unix)]
#[test]
fn au105_slash_form_with_the_lock_held_replies_inside_the_bound() {
    let d = home();
    let h = d.path();
    let (s, store) = member(h);
    let t0 = std::time::Instant::now();
    let held = mf::hold(store.dir().join("lock"));
    let env = Env { session: Some(mf::SID), dir: Some(&s), vars: MEMBER_VARS, ..Default::default() };
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:results off")), "results off for this session; not saved for the workspace");
    assert!(t0.elapsed() < std::time::Duration::from_millis(4500), "{:?}", t0.elapsed());
    mf::release_after(held, t0, std::time::Duration::from_secs(6));
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, None);
}

/// AU105: no record for the marker's pane key is a failed write; the state does not change.
#[cfg(unix)]
#[test]
fn au105_member_without_a_record_is_not_saved() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[], None);
    let before = store.read().unwrap().unwrap();
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "units off"), (0, "units off for this session; not saved for the workspace\n".into(), String::new()));
    assert_eq!(store.read().unwrap().unwrap(), before);
}

/// AU105, decision 2026-11-23: a workspace that `ws forget` removed is not made again by the mirror.
#[cfg(unix)]
#[test]
fn au105_forgotten_workspace_is_not_made_again() {
    let d = home();
    let h = d.path();
    let (s, store) = member(h);
    std::fs::remove_dir_all(store.dir()).unwrap();
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "units off"), (0, "units off for this session; not saved for the workspace\n".into(), String::new()));
    assert!(!store.dir().exists());
}

/// AU105, the hook writer's rule: a record of another zygo session is not changed.
#[cfg(unix)]
#[test]
fn au105_record_of_another_session_is_not_saved() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::OTHER))], None);
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "results off"), (0, "results off for this session; not saved for the workspace\n".into(), String::new()));
    assert_eq!(mf::record(&store, mf::WS_KEY), mf::running(mf::OTHER));
}

/// AU105, the hook writer's rule: a record whose `pending` names this zygo session is this session's; only
/// `overrides` changes.
#[cfg(unix)]
#[test]
fn au105_pending_start_of_this_session_is_saved() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    let pending = zygo_workspace::state::Pending { zygo_session: mf::SID.into(), ..Default::default() };
    let rec = zygo_workspace::state::MemberRecord { pending: Some(pending), ..mf::running(mf::OTHER) };
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, rec.clone())], None);
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "results off"), (0, "results off for this session\n".into(), String::new()));
    let want = zygo_workspace::state::MemberRecord { overrides: Some(json!({ "results": false, "guards": {} })), ..rec };
    assert_eq!(mf::record(&store, mf::WS_KEY), want);
}

/// W§18 #89, W-C6, AU84, CLI form: a view that is a link, a FIFO, empty, not JSON or unreadable, and a marker with
/// an invalid workspace id or pane key, are not a member: today's reply, no record change, no `session.log`.
#[cfg(unix)]
#[test]
fn w18_89_unreadable_marker_is_not_a_member() {
    let cases: [(&str, &dyn Fn(&Path, &Path)); 7] = [
        ("link", &|h, s| {
            mf::view(&h.join("real"), mf::SID, mf::WS_ID, mf::WS_KEY);
            std::fs::create_dir_all(s).unwrap();
            std::os::unix::fs::symlink(h.join("real/view.json"), s.join("view.json")).unwrap();
        }),
        ("fifo", &|_, s| {
            std::fs::create_dir_all(s).unwrap();
            let c = std::ffi::CString::new(s.join("view.json").into_os_string().into_encoded_bytes()).unwrap();
            // SAFETY: `c` is a valid NUL-terminated path.
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        }),
        ("empty", &|_, s| {
            std::fs::create_dir_all(s).unwrap();
            std::fs::write(s.join("view.json"), b"").unwrap();
        }),
        ("not json", &|_, s| {
            std::fs::create_dir_all(s).unwrap();
            std::fs::write(s.join("view.json"), b"{").unwrap();
        }),
        ("unreadable", &|_, s| {
            use std::os::unix::fs::PermissionsExt;
            mf::view(s, mf::SID, mf::WS_ID, mf::WS_KEY);
            std::fs::set_permissions(s.join("view.json"), std::fs::Permissions::from_mode(0o000)).unwrap();
        }),
        ("invalid ws id", &|_, s| mf::view(s, mf::SID, "../x", mf::WS_KEY)),
        ("invalid pane key", &|_, s| mf::view(s, mf::SID, mf::WS_ID, "bad")),
    ];
    for (name, make) in cases {
        let d = home();
        let h = d.path();
        let s = h.join("s");
        make(h, &s);
        if name == "unreadable" && std::fs::read(s.join("view.json")).is_ok() {
            eprintln!("{name}: skipped; this user reads a file of mode 000");
            continue;
        }
        let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::SID))], None);
        let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
        assert_eq!(cli(h, &env, "results off"), (0, "results off for this session\n".into(), String::new()), "{name}");
        assert_eq!(mf::record(&store, mf::WS_KEY), mf::running(mf::SID), "{name}");
        assert!(!s.join("session.log").exists(), "{name}");
    }
}

/// W§18 #89, slash form: a launch that is not a member (no `ZYGO_WORKSPACE`) reads no view through `zygo hook
/// prompt`. A FIFO view cannot prove this, because `read_view` opens with `O_NONBLOCK` and never waits; a valid
/// marker with its record proves it: a view that was read would be mirrored.
#[cfg(unix)]
#[test]
fn w18_89_slash_form_without_zygo_workspace_reads_no_view() {
    let d = home();
    let h = d.path();
    let (s, store) = member(h);
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:results off")), "results off for this session");
    assert_eq!(mf::record(&store, mf::WS_KEY), mf::running(mf::SID));
    assert!(!s.join("session.log").exists());
}

/// W-C6, AU58: no `view.json`, or a view with no marker: today's reply, no workspace state folder.
#[cfg(unix)]
#[test]
fn au105_non_member_session_keeps_todays_reply() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    SessionDir::new(s.clone()).create().unwrap();
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    assert_eq!(cli(h, &env, "results off"), (0, "results off for this session\n".into(), String::new()));
    zygo_core::session_view::SessionView { session: mf::SID.into(), ..Default::default() }.save(&SessionDir::new(s.clone())).unwrap();
    assert_eq!(cli(h, &env, "units off"), (0, "units off for this session\n".into(), String::new()));
    assert!(!h.join(".local/state").exists());
}

/// AU105: two setting verbs at the same time leave the record equal to the last `overrides.json`.
#[cfg(unix)]
#[test]
fn au105_parallel_setting_verbs_leave_the_record_equal_to_the_file() {
    let d = home();
    let h = d.path();
    let (s, store) = member(h);
    let env = Env { session: Some(mf::SID), dir: Some(&s), ..Default::default() };
    for round in 0..8 {
        let (a, b) = if round % 2 == 0 { ("results off", "units off") } else { ("results on", "units on") };
        let (ra, rb) = std::thread::scope(|t| {
            let ja = t.spawn(|| cli(h, &env, a));
            let jb = t.spawn(|| cli(h, &env, b));
            (ja.join().unwrap(), jb.join().unwrap())
        });
        assert_eq!(ra, (0, format!("{a} for this session\n"), String::new()), "round {round}");
        assert_eq!(rb, (0, format!("{b} for this session\n"), String::new()), "round {round}");
        assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(mf::ovr_json(&s)), "round {round}");
    }
}

/// W§7.3, AU105: the CLI form runs the pending snapshot inline and after the mirror write: each tmux call has the
/// verb process as its parent and sees the written `overrides`.
#[cfg(unix)]
#[test]
fn au105_cli_verb_runs_the_pending_snapshot_inline_after_the_write() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::SID))], Some(&h.join("tmux.sock")));
    std::fs::write(store.dir().join("snapshot.pending"), b"").unwrap();
    std::fs::write(h.join("tmux-go"), b"").unwrap();
    let vars = mf::fake_tmux(h, &store.dir().join("workspace.json"));
    let vars = as_vars(&vars);
    let env = Env { session: Some(mf::SID), dir: Some(&s), vars: &vars, ..Default::default() };
    assert_eq!(cli(h, &env, "units off"), (0, "units off for this session\n".into(), String::new()));
    let calls = mf::tmux_calls(h);
    assert!(!calls.is_empty());
    for c in &calls {
        assert_eq!(c.when, "after", "{c:?}");
        assert!(c.parent_args.ends_with("zygo units off"), "{c:?}");
    }
    assert!(!store.dir().join("snapshot.pending").exists());
}

/// AU105, W§7.3 hook rule, slash form: the reply comes inside the 4.5 s bound; the verb takes `snapshot.pending`
/// after its write and starts `zygo ws snapshot` as a detached child (its own process group) that outlives the hook
/// process and runs the snapshot. The test waits for the child to exit before the temporary folder goes away.
#[cfg(unix)]
#[test]
fn au105_slash_verb_starts_the_pending_snapshot_as_a_detached_child() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::SID))], Some(&h.join("tmux.sock")));
    std::fs::write(store.dir().join("snapshot.pending"), b"").unwrap();
    let mut vars = mf::fake_tmux(h, &store.dir().join("workspace.json"));
    vars.push(("ZYGO_WORKSPACE", mf::WS_ID.to_string()));
    let vars = as_vars(&vars);
    let env = Env { session: Some(mf::SID), dir: Some(&s), vars: &vars, ..Default::default() };
    let t0 = std::time::Instant::now();
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:units off")), "units off for this session");
    assert!(t0.elapsed() < std::time::Duration::from_millis(4500), "{:?}", t0.elapsed());
    assert!(!store.dir().join("snapshot.pending").exists());
    let calls = mf::wait_for_calls(h, std::time::Duration::from_secs(10));
    let child = calls[0].parent;
    // The hook process has exited (`hook_prompt` returned); the child still runs: tmux waits for `tmux-go`.
    assert!(mf::alive(child), "the snapshot child ended with the hook");
    // SAFETY: `getpgrp` has no preconditions.
    let test_group = unsafe { libc::getpgrp() };
    for c in &calls {
        assert_eq!(c.when, "after", "{c:?}");
        assert!(c.parent_args.contains(&format!("ws snapshot --ws-id {}", mf::WS_ID)), "{c:?}");
        assert!(!c.parent_args.contains("hook prompt"), "{c:?}");
        assert_eq!(c.parent_pgid, c.parent, "{c:?}: the child leads its own group (setsid)");
        assert_ne!(c.parent_pgid, test_group, "{c:?}");
    }
    std::fs::write(h.join("tmux-go"), b"").unwrap();
    mf::wait_gone(child, std::time::Duration::from_secs(10));
    assert_eq!(mf::record(&store, mf::WS_KEY).overrides, Some(json!({ "work_units": false, "guards": {} })));
}

/// AU105: after a failed write the pending snapshot is not taken and no tmux call runs, CLI and slash.
#[cfg(unix)]
#[test]
fn au105_failed_write_runs_no_pending_snapshot() {
    let d = home();
    let h = d.path();
    let s = h.join("s");
    mf::view(&s, mf::SID, mf::WS_ID, mf::WS_KEY);
    let store = mf::state(&paths(h), h, &[(mf::WS_KEY, mf::running(mf::OTHER))], Some(&h.join("tmux.sock")));
    std::fs::write(store.dir().join("snapshot.pending"), b"").unwrap();
    std::fs::write(h.join("tmux-go"), b"").unwrap();
    let mut vars = mf::fake_tmux(h, &store.dir().join("workspace.json"));
    vars.push(("ZYGO_WORKSPACE", mf::WS_ID.to_string()));
    let vars = as_vars(&vars);
    let env = Env { session: Some(mf::SID), dir: Some(&s), vars: &vars, ..Default::default() };
    assert_eq!(cli(h, &env, "units off"), (0, "units off for this session; not saved for the workspace\n".into(), String::new()));
    assert_eq!(reason(&hook_prompt(h, &env, "/zygo:units on")), "units on for this session; not saved for the workspace");
    std::thread::sleep(std::time::Duration::from_millis(500)); // a detached child, had one started, logs by now
    assert!(mf::tmux_calls(h).is_empty());
    assert!(store.dir().join("snapshot.pending").exists());
}
