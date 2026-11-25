//! `zygo compact <tokens|on|off>` and `/zygo:compact` (§10.2.4, §10.2.5): one handler for both. After the proxy took
//! the change, the compaction override is mirrored into the member record of the session folder of the id, when that
//! session is a workspace member (AU32, AU105).

use std::str::FromStr;
use std::time::Duration;

use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_plugins::check::esc;
use zygo_proxy::client::{ClientError, SessionClient, WindowArg};
use zygo_proxy::session::{WINDOW_MAX, WINDOW_MIN};
use zygo_workspace::state::{valid_session_id, CompactOverride};

use super::verbs::{mirror, Mirror, VerbCtx, NOT_SAVED};
use super::UsageError;

/// A session-API failure; `Unreachable` exits 12, any other 1. `{e}` holds the whole reason (no `source()`).
#[derive(Debug)]
pub struct CompactFailed(pub ClientError);

impl std::fmt::Display for CompactFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compact failed: {}", esc(&self.0.to_string()))
    }
}

impl std::error::Error for CompactFailed {}

#[derive(Debug, PartialEq)]
pub enum Change {
    Set(WindowArg),
    Clear,
}

/// `on`, `off`, or `<n>`, `<n>k`, `<n>m` within `WINDOW_MIN..=WINDOW_MAX`.
pub fn parse_window(s: &str) -> Option<Change> {
    match s {
        "on" => return Some(Change::Clear),
        "off" => return Some(Change::Set(WindowArg::Off)),
        _ => {}
    }
    let (digits, unit) = match (s.strip_suffix('k'), s.strip_suffix('m')) {
        (Some(d), _) => (d, 1_000),
        (_, Some(d)) => (d, 1_000_000),
        _ => (s, 1),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = digits.parse::<u64>().ok()?.checked_mul(unit)?;
    (WINDOW_MIN..=WINDOW_MAX).contains(&n).then_some(Change::Set(WindowArg::Tokens(n)))
}

/// The session-API timeout: 4 s for the slash form (inside the hook's bound), 10 s for the CLI (§10.2.4).
pub fn client_timeout(slash: bool) -> Duration {
    Duration::from_secs(if slash { 4 } else { 10 })
}

pub fn run(ctx: &VerbCtx, args: &[String], env_session: Option<&str>, agent: Option<Agent>) -> anyhow::Result<String> {
    if agent == Some(Agent::OpenCode) {
        return Err(UsageError("OpenCode reads its window at start; relaunch with zygo opencode --compact <n>".into()).into());
    }
    if ctx.save {
        return Err(UsageError("compact failed: compact has no --save; it lasts for this session".into()).into());
    }
    let (mut words, mut session, mut flag) = (Vec::new(), None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--session" {
            flag = true;
            session = it.next().map(String::as_str);
        } else if let Some(id) = a.strip_prefix("--session=") {
            flag = true;
            session = Some(id);
        } else {
            words.push(a.as_str());
        }
    }
    let arg = words.join(" ");
    let Some(change) = parse_window(&arg) else {
        return Err(UsageError(format!(
            "compact failed: bad value `{arg}`: use <tokens>, <n>k or <n>m from 100k to 10m, on or off"
        ))
        .into());
    };
    let session = if flag { session } else { env_session }.filter(|s| !s.is_empty());
    let Some(id) = session else {
        return Err(UsageError("compact failed: no session; pass --session <id> or run it inside a zygo session".into()).into());
    };
    // Before any read: an id that is not a folder name inside the sessions folder names no session (AU105).
    if !valid_session_id(id) {
        return Err(UsageError(format!("compact failed: invalid session id {}", esc(id))).into());
    }
    let dir = SessionDir::new(ctx.paths.sessions_dir.join(id));
    let client = SessionClient::from_session_dir(&dir.root)
        .map_err(CompactFailed)?
        .timeout(client_timeout(ctx.slash));
    let value = match &change {
        Change::Set(w) => CompactOverride::Set(window_text(*w)),
        Change::Clear => CompactOverride::Cleared,
    };
    match change {
        Change::Set(w) => client.set_compact(w),
        Change::Clear => client.clear_compact(),
    }
    .map_err(CompactFailed)?;
    // The record of `<sessions dir>/<id>`, the folder the client used, never `ctx.session_dir` (AU105).
    let note = if mirror(ctx, &dir, Mirror::Compact(value)).is_err() { NOT_SAVED } else { "" };
    Ok(format!("compact {arg} for this session{note}"))
}

/// The window of a `set` in `member.compact` (AU105): the token count as digits, or `off`.
fn window_text(w: WindowArg) -> String {
    match w {
        WindowArg::Tokens(n) => n.to_string(),
        WindowArg::Off => "off".to_string(),
    }
}

/// Reads `ZYGO_SESSION` and `ZYGO_AGENT`.
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    let session = std::env::var("ZYGO_SESSION").ok();
    let agent = std::env::var("ZYGO_AGENT").ok().and_then(|a| Agent::from_str(&a).ok());
    run(ctx, args, session.as_deref(), agent)
}

#[cfg(test)]
#[path = "../../tests/dead_port/mod.rs"]
mod dead_port;

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use tempfile::TempDir;
    use zygo_config::{load, LoadContext};
    use zygo_core::paths::Paths;
    use zygo_core::session_dir::SessionDir;
    use zygo_core::Agent;
    use zygo_proxy::client::{ClientError, WindowArg};
    use zygo_proxy::session::SessionRequest;
    use zygo_proxy::{Proxy, ProxyHandle};

    use super::*;
    use crate::cmd::UsageError;

    pub(crate) fn paths(d: &Path) -> Paths {
        Paths {
            config_file: d.join("config.toml"),
            cache_dir: d.join("cache"),
            data_dir: d.join("data"),
            sessions_dir: d.join("cache/sessions"),
            marketplaces_dir: d.join("cache/marketplaces"),
            plugin_store: d.join("data/plugins"),
            approvals_file: d.join("data/approvals.toml"),
            usage_db: d.join("data/usage.db"),
        }
    }

    pub(crate) fn ctx(d: &Path, session_dir: Option<&Path>) -> VerbCtx {
        VerbCtx { slash: false, save: false, session_dir: session_dir.map(|p| SessionDir::new(p.to_path_buf())), paths: paths(d) }
    }

    /// A live loopback proxy with one session (work `ENG-42`, soft window 800k); returns the session id and its dir.
    pub(crate) fn proxy(d: &Path) -> (ProxyHandle, String, PathBuf) {
        let mut m = load(&LoadContext { paths: paths(d), repo: None, team: None }).unwrap();
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
        let dir = paths(d).sessions_dir.join(&g.session);
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token(&g.token).unwrap();
        std::fs::write(sd.url(), h.url().as_str()).unwrap();
        (h, g.session, dir)
    }

    /// A session dir `id` whose url is a closed loopback port.
    pub(crate) fn dead_session(d: &Path, id: &str) -> PathBuf {
        let dir = paths(d).sessions_dir.join(id);
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token("t").unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        std::fs::write(sd.url(), format!("http://127.0.0.1:{port}/")).unwrap();
        dir
    }

    /// A session dir `id` whose url is a loopback server that answers every request with `status` and `body`.
    pub(crate) fn canned_session(d: &Path, id: &str, status: u16, body: String) -> PathBuf {
        use std::io::{Read, Write};
        let dir = paths(d).sessions_dir.join(id);
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token("t").unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(sd.url(), format!("http://{}/", l.local_addr().unwrap())).unwrap();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { return };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                        _ => break,
                    }
                }
                let _ = write!(
                    s,
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        dir
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    fn usage_text(r: anyhow::Result<String>) -> String {
        let e = r.unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some(), "{e}");
        e.to_string()
    }

    #[test]
    fn parse_window_forms() {
        assert_eq!(parse_window("600000"), Some(Change::Set(WindowArg::Tokens(600_000))));
        assert_eq!(parse_window("600k"), Some(Change::Set(WindowArg::Tokens(600_000))));
        assert_eq!(parse_window("1m"), Some(Change::Set(WindowArg::Tokens(1_000_000))));
        assert_eq!(parse_window("100k"), Some(Change::Set(WindowArg::Tokens(100_000))));
        assert_eq!(parse_window("10m"), Some(Change::Set(WindowArg::Tokens(10_000_000))));
        assert_eq!(parse_window("off"), Some(Change::Set(WindowArg::Off)));
        assert_eq!(parse_window("on"), Some(Change::Clear));
        for bad in ["99999", "10000001", "11m", "600K", "1.5m", "", "k", "-1", "600kk"] {
            assert_eq!(parse_window(bad), None, "{bad}");
        }
    }

    #[test]
    fn sets_and_clears_on_a_live_session() {
        let d = TempDir::new().unwrap();
        let (h, id, _) = proxy(d.path());
        let c = ctx(d.path(), None);
        for (a, want) in [("600k", Some(WindowArg::Tokens(600_000))), ("off", Some(WindowArg::Off)), ("on", None)] {
            assert_eq!(run(&c, &v(&[a]), Some(&id), None).unwrap(), format!("compact {a} for this session"));
            assert_eq!(h.session(&id).unwrap().status().compact, want, "{a}");
        }
    }

    #[test]
    fn opencode_refused_first() {
        let d = TempDir::new().unwrap();
        let t = usage_text(run(&ctx(d.path(), None), &v(&["frob"]), None, Some(Agent::OpenCode)));
        assert_eq!(t, "OpenCode reads its window at start; relaunch with zygo opencode --compact <n>");
    }

    #[test]
    fn bad_value_no_session_and_save_are_usage_errors() {
        let d = TempDir::new().unwrap();
        let c = ctx(d.path(), None);
        assert_eq!(
            usage_text(run(&c, &v(&["600x"]), Some("S1"), None)),
            "compact failed: bad value `600x`: use <tokens>, <n>k or <n>m from 100k to 10m, on or off"
        );
        assert_eq!(
            usage_text(run(&c, &v(&["600k"]), None, None)),
            "compact failed: no session; pass --session <id> or run it inside a zygo session"
        );
        let save = VerbCtx { save: true, ..ctx(d.path(), None) };
        assert_eq!(
            usage_text(run(&save, &v(&["600k"]), Some("S1"), None)),
            "compact failed: compact has no --save; it lasts for this session"
        );
    }

    #[test]
    fn unreachable_is_compact_failed() {
        let d = TempDir::new().unwrap();
        dead_session(d.path(), "S1");
        let e = run(&ctx(d.path(), None), &v(&["600k"]), Some("S1"), None).unwrap_err();
        let f = e.downcast_ref::<CompactFailed>().expect("CompactFailed");
        assert!(matches!(f.0, ClientError::Unreachable(_)), "{f:?}");
        assert!(e.to_string().starts_with("compact failed: zygo proxy unreachable"), "{e}");
        assert!(std::error::Error::source(&CompactFailed(ClientError::Unreachable("x".into()))).is_none());
    }

    #[test]
    fn session_flag_wins() {
        let d = TempDir::new().unwrap();
        let (h, id, _) = proxy(d.path());
        let c = ctx(d.path(), None);
        assert_eq!(run(&c, &v(&["600k", "--session", &id]), Some("nope"), None).unwrap(), "compact 600k for this session");
        assert_eq!(h.session(&id).unwrap().status().compact, Some(WindowArg::Tokens(600_000)));
        let flag = format!("--session={id}");
        assert_eq!(run(&c, &v(&["off", &flag]), Some("nope"), None).unwrap(), "compact off for this session");
        assert_eq!(h.session(&id).unwrap().status().compact, Some(WindowArg::Off));
    }

    #[test]
    fn a_server_refusal_cannot_forge_a_line() { // N70: compact.rs:17-21
        let d = TempDir::new().unwrap();
        let body = serde_json::json!({ "error": { "type": "bad", "message": "one\ntwo\u{1b}[31m" } }).to_string();
        canned_session(d.path(), "S1", 400, body);
        let e = run(&ctx(d.path(), None), &v(&["600k"]), Some("S1"), None).unwrap_err();
        assert!(e.downcast_ref::<CompactFailed>().is_some(), "{e}");
        assert_eq!(e.to_string(), "compact failed: bad: one\\ntwo\\u{1b}[31m");
    } // mutation: `esc` removed from `CompactFailed`'s Display

    #[test]
    fn closed_port_refuses_and_no_port_0_bind_gets_it() { // FX-6, CI run 45557591675
        let port = super::dead_port::closed_port();
        let e = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused, "{e}");
        assert!(port < 1024, "{port} is in an ephemeral range, so a parallel `127.0.0.1:0` bind can get it");
        assert_eq!(super::dead_port::closed_port(), port);
    }
}
