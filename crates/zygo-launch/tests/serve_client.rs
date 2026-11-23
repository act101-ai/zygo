//! N37: serve mode client (§7, §7.1, §7.2, C8, §8.2, §14, §15.3, §17, §20 #13, #29d). Loopback only; tokens from
//! team::new_token() at run time; no process-environment change.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use zygo_config::keys::{MemStore, Secret};
use zygo_config::{load_with_builtin, Config, KeyRef, LoadContext, Marketplace, Merged};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_launch::env::{child_env_with, Env};
use zygo_launch::opencode::OpenCode;
use zygo_launch::serve_client::*;
use zygo_launch::{AgentAdapter, ChildSpec, LaunchCtx, LaunchOpts, Launcher, Preflight};
use zygo_proxy::client::{SessionClient, SessionStatus, WindowArg};
use zygo_proxy::serve::{self, ServeHandle, ServeOpts};
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{ManualClock, SessionGrant, SessionRequest, SUSPEND_AFTER};
use zygo_proxy::team::{new_token, PluginsReply, TeamConfigReply};
use zygo_proxy::{Proxy, Url};
use zygo_redact::act_lite_store::{store_path, Lookup};
use zygo_usage::{RequestRow, Store};

// ---- fixtures ----

#[allow(dead_code)]
struct Fx {
    _t: TempDir,
    root: PathBuf,
    paths: Paths,
    cwd: PathBuf,
    out: PathBuf,
    bin: PathBuf,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let h = root.join("home");
    let paths = Paths {
        config_file: h.join("config.toml"),
        cache_dir: h.join("cache"),
        data_dir: h.join("data"),
        sessions_dir: h.join("cache/sessions"),
        marketplaces_dir: h.join("cache/marketplaces"),
        plugin_store: h.join("data/plugins"),
        approvals_file: h.join("data/approvals.toml"),
        usage_db: h.join("data/usage.db"),
    };
    let (cwd, out, bin) = (root.join("work"), root.join("out"), root.join("bin"));
    for d in [&h, &cwd, &out, &bin] {
        std::fs::create_dir_all(d).unwrap();
    }
    Fx { _t: t, root, paths, cwd, out, bin }
}

#[allow(dead_code)]
impl Fx {
    fn config(&self, text: &str) {
        std::fs::write(&self.paths.config_file, text).unwrap();
    }
    fn merged(&self) -> Merged {
        load_with_builtin(&LoadContext { paths: self.paths.clone(), repo: None, team: None }, builtin()).unwrap()
    }
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

/// Polls `done` every 10 ms until it holds or `bound` passes; returns whether it held.
fn until(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + bound;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[allow(dead_code)]
struct Srv {
    _d: TempDir,
    root: PathBuf,
    h: ServeHandle,
    alice: Secret,
    bob: Secret,
    clock: Arc<ManualClock>,
    lines: Arc<Mutex<Vec<String>>>,
}

/// A team server on 127.0.0.1:0: tokens for alice and bob, then `extra`; built-in layer without marketplace.
fn server(extra: &str) -> Srv {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let (alice, ah) = new_token();
    let (bob, bh) = new_token();
    let config_file = root.join("config.toml");
    std::fs::write(
        &config_file,
        format!("[serve]\ntokens = [{{ label = \"alice\", sha256 = \"{ah}\" }}, {{ label = \"bob\", sha256 = \"{bh}\" }}]\n{extra}"),
    )
    .unwrap();
    let paths = Paths {
        config_file: config_file.clone(),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"),
        marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"),
        approvals_file: root.join("data/approvals.toml"),
        usage_db: root.join("data/usage.db"),
    };
    let ctx = LoadContext { paths, repo: None, team: None };
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let l = lines.clone();
    let clock = ManualClock::new();
    let h = serve::start(ServeOpts {
        listen: "127.0.0.1:0".into(),
        tls: None,
        config_file,
        load: Arc::new(move || Ok(load_with_builtin(&ctx, builtin())?)),
        report: Arc::new(move |s: &str| l.lock().unwrap().push(s.to_string())),
        pid_file: None,
        signals: false,
        start: StartOpts { clock: clock.clone(), usage_db: Some(root.join("data/usage.db")), ..Default::default() },
    })
    .unwrap();
    Srv { _d: d, root, h, alice: Secret::new(alice), bob: Secret::new(bob), clock, lines }
}

/// A loopback URL nothing listens on.
fn closed_url() -> String {
    let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}/")
}

/// Accepts every connection on 127.0.0.1:0, never writes, closes each after `hold`.
fn silent_server(hold: Duration) -> Url {
    let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = Url::parse(&format!("http://{}/", l.local_addr().unwrap())).unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { return };
            std::thread::spawn(move || {
                std::thread::sleep(hold);
                drop(s);
            });
        }
    });
    url
}

/// One recorded request.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct Rec {
    method: String,
    path: String,
    headers: std::collections::BTreeMap<String, String>,
    body: Vec<u8>,
}

type Route = Arc<dyn Fn(&Rec) -> Option<(u16, String)> + Send + Sync>;

/// A raw HTTP server on 127.0.0.1:0: each request is recorded, then answered by `route` with `connection: close`;
/// `None` holds the connection open 20 s without a reply.
fn recorder(route: impl Fn(&Rec) -> Option<(u16, String)> + Send + Sync + 'static) -> (Url, Arc<Mutex<Vec<Rec>>>) {
    let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = Url::parse(&format!("http://{}/", l.local_addr().unwrap())).unwrap();
    let recs: Arc<Mutex<Vec<Rec>>> = Arc::default();
    let (r2, route): (_, Route) = (recs.clone(), Arc::new(route));
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let (recs, route) = (r2.clone(), route.clone());
            std::thread::spawn(move || {
                let Some(rec) = read_request(&mut s) else { return };
                recs.lock().unwrap().push(rec.clone());
                match route(&rec) {
                    Some((status, body)) => {
                        let ctype = if body.is_empty() { "" } else { "content-type: application/json\r\n" };
                        let _ = write!(
                            s,
                            "HTTP/1.1 {status} X\r\n{ctype}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    None => std::thread::sleep(Duration::from_secs(20)),
                }
            });
        }
    });
    (url, recs)
}

fn read_request(s: &mut std::net::TcpStream) -> Option<Rec> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut chunk).ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: std::collections::BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())))
        .collect();
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    while buf.len() < head_end + 4 + len {
        let n = s.read(&mut chunk).ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
    }
    Some(Rec { method, path, headers, body: buf[head_end + 4..head_end + 4 + len].to_vec() })
}

fn client(s: &Srv, t: &Secret) -> ServeClient {
    ServeClient::connect(&s.h.url(), t).unwrap()
}

fn req() -> SessionRequest {
    SessionRequest {
        agent: Agent::ClaudeCode,
        agent_version: String::new(),
        profile: None,
        passthrough: false,
        repo: None,
        work: None,
        stored_values: Vec::new(),
        known_plugins: Default::default(),
    }
}

/// `GET …/status` with the session token, through a session directory `<dir>/<id>` holding `token` and `url`.
fn status_of(dir: &Path, url: &Url, g: &SessionGrant) -> Result<SessionStatus, zygo_proxy::client::ClientError> {
    let sd = SessionDir::new(dir.join(&g.session));
    if !sd.root.exists() {
        sd.create().unwrap();
        sd.write_token(&g.token).unwrap();
        std::fs::write(sd.url(), url.as_str()).unwrap();
    }
    SessionClient::from_session_dir(&sd.root)?.status()
}

fn unknown(e: &ClientError) -> bool {
    matches!(e, ClientError::Reply { status: 401, code, .. } if code == "zygo_session_unknown")
}

fn row(id: &str, label: &str) -> RequestRow {
    RequestRow { session: id.into(), token_label: Some(label.into()), status: 200, ..Default::default() }
}

fn local_store(d: &TempDir) -> Store {
    Store::open(&d.path().join("usage.db")).unwrap()
}

// ---- client library ----

#[test]
fn connect_refuses_plain_http_to_a_remote_host() { // C8, §20 #13
    let t = Secret::new(new_token().0);
    for u in ["http://10.0.0.5:8443", "http://example.com/", "ftp://127.0.0.1/"] {
        let e = ServeClient::connect(&Url::parse(u).unwrap(), &t).unwrap_err();
        assert!(matches!(e.downcast_ref::<Refused>(), Some(Refused::Url)), "{u}");
    }
    for u in ["https://10.0.0.5:8443", "http://127.0.0.1:9/", "http://localhost:9/", "http://[::1]:9/"] {
        assert!(ServeClient::connect(&Url::parse(u).unwrap(), &t).is_ok(), "{u}");
    }
} // mutation: no transport check

#[test]
fn bounds_are_the_spec_values() { // §7 note
    assert_eq!((CONNECT_TIMEOUT, CALL_TIMEOUT), (Duration::from_secs(10), Duration::from_secs(30)));
    assert_eq!(Bounds::default(), Bounds { connect: CONNECT_TIMEOUT, call: CALL_TIMEOUT });
} // mutation: another constant value

#[test]
fn a_silent_server_ends_the_call_at_the_call_bound() { // §7 note: 30 s per call
    let url = silent_server(Duration::from_secs(20));
    let c = connect_with(&url, &Secret::new(new_token().0), Bounds { connect: Duration::from_secs(10), call: Duration::from_millis(300) })
        .unwrap();
    let t0 = Instant::now();
    assert!(matches!(c.team_config(), Err(ClientError::Unreachable(_))));
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
} // mutation: no `.timeout(bounds.call)` (the call ends only when the server closes at 20 s)

#[cfg(target_os = "linux")]
#[test]
fn a_full_accept_queue_ends_the_call_at_the_connect_bound() { // §7 note: connect 10 s
    use std::os::fd::FromRawFd;
    // SAFETY: plain socket calls on a new descriptor; the listener takes ownership of it.
    let l = unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let mut a: libc::sockaddr_in = std::mem::zeroed();
        a.sin_family = libc::AF_INET as libc::sa_family_t;
        a.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
        let len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        assert_eq!(libc::bind(fd, &a as *const libc::sockaddr_in as *const libc::sockaddr, len), 0);
        assert_eq!(libc::listen(fd, 0), 0);
        TcpListener::from_raw_fd(fd)
    };
    let addr = l.local_addr().unwrap();
    // Linux drops further SYNs while the queue is full, so the next connect hangs.
    let _fill: Vec<_> = (0..2).filter_map(|_| std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)).ok()).collect();
    let url = Url::parse(&format!("http://{addr}/")).unwrap();
    let c = connect_with(&url, &Secret::new(new_token().0), Bounds { connect: Duration::from_millis(300), call: Duration::from_secs(60) })
        .unwrap();
    let t0 = Instant::now();
    assert!(matches!(c.team_config(), Err(ClientError::Unreachable(_))));
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    drop(l);
} // mutation: no `.connect_timeout(bounds.connect)` (only the 60 s call bound ends it). Linux only: macOS and Windows
  // refuse or reset a connect to a full queue instead of dropping it.

#[test]
fn doctor_health_check_ends_at_the_call_bound() { // §7 note
    let url = silent_server(Duration::from_secs(20));
    let t0 = Instant::now();
    let line = reachability_with(url.as_str(), Bounds { connect: Duration::from_secs(10), call: Duration::from_millis(300) });
    assert!(line.starts_with(&format!("{url} unreachable (")), "{line}");
    assert!(t0.elapsed() < Duration::from_secs(10));
} // mutation: reachability built without the bounds

#[test]
fn from_config_flag_wins_and_token_is_required() {
    let store = MemStore::new();
    let none = |_: &str| -> Option<OsString> { None };
    let mut cfg = Config::default();
    cfg.server.url = Some("http://127.0.0.1:9/".into());
    let r = from_config(Some("http://10.0.0.5:8443"), &cfg, &store, &none);
    assert!(matches!(r, Err(Refused::Url)), "{r:?}");
    assert!(matches!(from_config(None, &Config::default(), &store, &none), Ok(None)));
    assert!(matches!(from_config(None, &cfg, &store, &none), Err(Refused::NoToken)));
    cfg.server.token = Some(KeyRef::Env("ZYGO_TEAM_TOKEN".into()));
    assert!(matches!(from_config(None, &cfg, &store, &none), Err(Refused::NoToken)));
    let t = new_token().0;
    let env = move |n: &str| (n == "ZYGO_TEAM_TOKEN").then(|| OsString::from(&t));
    let c = from_config(None, &cfg, &store, &env).unwrap().unwrap();
    assert_eq!(c.url().as_str(), "http://127.0.0.1:9/");
    let c = from_config(Some("http://localhost:7/"), &cfg, &store, &env).unwrap().unwrap();
    assert_eq!(c.url().as_str(), "http://localhost:7/");
    assert_eq!(Refused::NoToken.to_string(), "serve mode needs a team token: [server] token is not set or names an empty value");
} // mutations: config url preferred over the flag; token check skipped

#[test]
fn team_table_takes_marketplaces_from_the_plugins_reply() { // §15.3
    let mut team: toml::Table = "[guards]\nclaims = \"enforce\"\n".parse().unwrap();
    team.insert("marketplace".into(), toml::Value::Array(vec![toml::Value::Table(toml::toml! { name = "stale" })]));
    let c = TeamConfigReply { team, model: vec![] };
    let p = PluginsReply {
        marketplace: vec![Marketplace {
            name: "team".into(),
            source: "github:example/agent-plugins".into(),
            git_ref: "v1.4.0".into(),
            commit: None,
            default_on: vec!["fmt".into()],
            required: vec!["lint".into()],
            approvals: vec![],
        }],
    };
    let t = team_table(&c, &p).unwrap();
    assert_eq!(t["marketplace"].as_array().unwrap().len(), 1);
    assert_eq!(t["marketplace"][0]["name"].as_str(), Some("team"));
    assert_eq!(t["guards"]["claims"].as_str(), Some("enforce"));
    assert!(!team_table(&c, &PluginsReply { marketplace: vec![] }).unwrap().contains_key("marketplace"));
} // mutations: the config reply's marketplace list kept; an empty marketplace array sent

#[test]
fn team_table_keeps_only_the_team_sections() { // N66 J1, §7.2: a server cannot set `[user]`, `[work]` or another section on the client
    let team: toml::Table = "[guards]\nclaims = \"enforce\"\n[user]\ndefault_profile = \"p\"\n[work]\nunit_pattern = \"x\"\n".parse().unwrap();
    let t = team_table(&TeamConfigReply { team, model: vec![] }, &PluginsReply { marketplace: vec![] }).unwrap();
    assert_eq!(t.keys().collect::<Vec<_>>(), ["guards", "model"]); // `model` is the reply's catalog (§6.1)
    assert_eq!(t["guards"]["claims"].as_str(), Some("enforce"));
} // mutation: no filter (→ `user` and `work` present)

#[test]
fn team_routes_use_the_team_token() {
    let s = server("");
    let c = client(&s, &s.alice);
    let r = c.team_config().unwrap();
    assert!(r.team.is_empty());
    assert!(!r.model.is_empty());
    assert!(c.plugins().unwrap().marketplace.is_empty());
    assert!(matches!(client(&s, &Secret::new(new_token().0)).team_config(), Err(ClientError::Reply { status: 401, .. })));
    s.h.shutdown();
} // mutation: no authorization header

#[test]
fn unusable_token_is_a_request_error() {
    let s = server("");
    let c = ServeClient::connect(&s.h.url(), &Secret::new("bad\nvalue")).unwrap();
    let e = c.team_config().unwrap_err();
    assert!(matches!(e, ClientError::Request(_)));
    assert_eq!(e.to_string(), "the team token is not a valid header value");
    assert!(matches!(client(&s, &s.alice).heartbeat("../x"), Err(ClientError::Request(m)) if m == "invalid session id"));
    s.h.shutdown();
} // mutations: Session variant used; the id not checked

#[test]
fn unreachable_is_reported_as_unreachable() {
    let c = ServeClient::connect(&Url::parse(&closed_url()).unwrap(), &Secret::new(new_token().0)).unwrap();
    assert!(matches!(c.team_config(), Err(ClientError::Unreachable(_))));
} // mutation: transport errors mapped to Reply

#[test]
fn unreachable_text_keeps_its_cause() { // S4 (round 2), C3
    let t = new_token().0;
    let c = ServeClient::connect(&Url::parse("http://127.0.0.1:1/").unwrap(), &Secret::new(t.clone())).unwrap();
    let Err(ClientError::Unreachable(d)) = c.team_config() else { panic!("not unreachable") };
    assert!(d.contains("onnect"), "{d}");
    assert!(!d.contains(&t) && !d.contains("Bearer") && !d.contains("127.0.0.1"), "{d}");
    let line = reachability("http://127.0.0.1:1/");
    assert!(line.starts_with("http://127.0.0.1:1/ unreachable (") && line["http://127.0.0.1:1/".len()..].contains("onnect"), "{line}");
} // mutation: only the top-level text ("error sending request") kept

#[test]
fn heartbeat_resumes_a_suspended_session() { // §7.2, §20 #13
    let s = server("");
    let c = client(&s, &s.alice);
    let d = tempfile::tempdir().unwrap();
    let g = c.create_session(&req()).unwrap();
    s.clock.advance(SUSPEND_AFTER + Duration::from_secs(1));
    assert!(
        matches!(status_of(d.path(), &s.h.url(), &g), Err(ClientError::Reply { status: 401, ref code, .. }) if code == "zygo_session_suspended")
    );
    c.heartbeat(&g.session).unwrap();
    assert_eq!(status_of(d.path(), &s.h.url(), &g).unwrap().session, g.session);
    s.h.shutdown();
} // mutation: heartbeat posts to another path

#[test]
fn heartbeat_loop_sends_until_dropped() {
    let s = server("");
    let c = client(&s, &s.alice);
    let d = tempfile::tempdir().unwrap();
    let g = c.create_session(&req()).unwrap();
    let hb = Heartbeat::start(c.try_clone().unwrap(), g.session.clone(), Duration::from_millis(20), Box::new(|_| {}));
    s.clock.advance(SUSPEND_AFTER + Duration::from_secs(1));
    assert!(until(Duration::from_secs(10), || status_of(d.path(), &s.h.url(), &g).is_ok()));
    drop(hb); // joins: no beat after this line
    s.clock.advance(SUSPEND_AFTER + Duration::from_secs(1));
    std::thread::sleep(Duration::from_millis(200)); // ten intervals
    assert!(status_of(d.path(), &s.h.url(), &g).is_err());
    s.h.shutdown();
} // mutations: the loop never sends; drop does not stop the loop

#[test]
fn session_unknown_reports_the_relaunch_message_once() { // §7.2
    let s = server("");
    let c = client(&s, &s.alice);
    let g = c.create_session(&req()).unwrap();
    c.delete_session(&g.session).unwrap();
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let l = lines.clone();
    let hb = Heartbeat::start(
        c.try_clone().unwrap(),
        g.session.clone(),
        Duration::from_millis(10),
        Box::new(move |m| l.lock().unwrap().push(m.to_string())),
    );
    assert!(until(Duration::from_secs(10), || !lines.lock().unwrap().is_empty()));
    std::thread::sleep(Duration::from_millis(200));
    drop(hb);
    assert_eq!(*lines.lock().unwrap(), vec![RESTARTED.to_string()]);
    assert_eq!(RESTARTED, "zygo server restarted; relaunch the session");
    s.h.shutdown();
} // mutations: no report; a report on every beat

#[test]
fn end_rows_replaces_and_keeps_or_clears_the_mark() { // §14, B2, S5, R1, R6
    let d = tempfile::tempdir().unwrap();
    let local = local_store(&d);
    local.insert_requests(&[row("A1", "alice")]).unwrap(); // a partial earlier sync; nothing marked yet
    assert_eq!(end_rows(&local, "A1", Ok(Rows { rows: vec![row("A1", "alice"), row("A1", "alice")], complete: true })), Ok(()));
    assert_eq!(local.requests_for_session("A1").unwrap().len(), 2); // replaced, not skipped
    assert_eq!(end_rows(&local, "B1", Ok(Rows { rows: vec![row("B1", "alice")], complete: false })), Err(RowsEnd::Incomplete));
    assert_eq!(local.requests_for_session("B1").unwrap().len(), 1);
    let gone = ClientError::Reply { status: 401, code: "zygo_session_unknown".into(), message: String::new() };
    assert_eq!(end_rows(&local, "C1", Err(gone)), Ok(()));
    assert!(matches!(end_rows(&local, "D1", Err(ClientError::Unreachable("x".into()))), Err(RowsEnd::NotFetched(_))));
    assert_eq!(local.sync_pending().unwrap(), ["B1", "D1"]); // marked again although never marked before
    assert_eq!(RowsEnd::Incomplete.to_string(), "usage rows incomplete");
    assert_eq!(RowsEnd::NotFetched("e".into()).to_string(), "usage rows not fetched (e)");
} // mutations: insert_requests instead of replace (→ 1 row); no mark on a non-clearing path; incomplete cleared; unknown kept

#[test]
fn sync_pending_retries_exactly_the_pending_sessions() { // §14, B2, S5, R1
    let s = server("");
    let remote = Store::open(&s.root.join("data/usage.db")).unwrap();
    for r in [row("A1", "alice"), row("A1", "alice"), row("B1", "bob"), row("E1", "alice"), row("L1", "alice")] {
        remote.insert_request(&r).unwrap();
    }
    let d = tempfile::tempdir().unwrap();
    let local = local_store(&d);
    let sd = d.path().join("sessions");
    std::fs::create_dir_all(sd.join("L1")).unwrap();
    std::fs::write(sd.join("L1/token"), "x").unwrap(); // L1 is live: its `pid` is locked (here by the test)
    let _held = locked_pid(&sd.join("L1"));
    local.insert_requests(&[row("A1", "alice")]).unwrap();
    for id in ["A1", "B1", "L1"] {
        local.mark_sync_pending(id).unwrap();
    }
    let r = sync_pending(&client(&s, &s.alice), &local, &sd).unwrap();
    assert_eq!((r.synced, r.tried, r.live, r.unreachable), (1, 2, 1, false));
    assert!(r.lines.is_empty(), "{:?}", r.lines);
    let a = local.requests_for_session("A1").unwrap();
    assert_eq!(a.len(), 2); // replaced
    assert!(a.iter().all(|r| r.provider.is_empty() && r.estimated_usd == 0.0 && r.token_label.as_deref() == Some("alice")));
    assert!(local.requests_for_session("B1").unwrap().is_empty()); // other label: unknown → cleared
    assert!(local.requests_for_session("E1").unwrap().is_empty()); // not pending
    assert!(local.requests_for_session("L1").unwrap().is_empty()); // live: skipped
    assert_eq!(local.sync_pending().unwrap(), ["L1"]);
    s.h.shutdown();
} // mutations: insert instead of replace; unknown kept; non-pending fetched; a live session fetched

/// `<dir>/pid` with this process id, locked while the returned file lives (as the launcher holds it).
fn locked_pid(dir: &Path) -> std::fs::File {
    let mut f = std::fs::File::create(dir.join(PID_FILE)).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    // Through the locked handle: on Windows, where locks are mandatory, a second handle cannot write it.
    write!(f, "{}", std::process::id()).unwrap();
    f
}

#[test]
fn sync_pending_counts_a_session_live_only_while_its_pid_is_locked() { // U3 (round 3), S1, R1
    let s = server("");
    let d = tempfile::tempdir().unwrap();
    let local = local_store(&d);
    let sd = d.path().join("sessions");
    // L1: `pid` locked; U1: `pid` names a running process (this one) but nobody holds its lock; N1: no `pid`.
    let mut held = Vec::new();
    for (id, pid, lock) in [("L1", true, true), ("U1", true, false), ("N1", false, false)] {
        std::fs::create_dir_all(sd.join(id)).unwrap();
        std::fs::write(sd.join(id).join("token"), "x").unwrap();
        if lock {
            held.push(locked_pid(&sd.join(id)));
        } else if pid {
            std::fs::write(sd.join(id).join(PID_FILE), std::process::id().to_string()).unwrap();
        }
        local.mark_sync_pending(id).unwrap();
    }
    let r = sync_pending(&client(&s, &s.alice), &local, &sd).unwrap();
    assert_eq!((r.tried, r.live), (2, 1), "{:?}", r.lines);
    assert_eq!(local.sync_pending().unwrap(), ["L1"]); // tried ones: unknown to the server → cleared
    assert_eq!(PID_FILE, "pid");
    drop(held);
    s.h.shutdown();
} // mutation: `live()` ignores the lock → U1 skipped as live

#[test]
fn sync_pending_goes_on_after_an_unreachable_session() { // §7 note, R5
    let b1 = serde_json::json!({ "requests": [row("B1", "alice")], "complete": true }).to_string();
    let (url, _rec) = recorder(move |r| {
        if r.path.ends_with("/A1/requests") { None } else { Some((200, b1.clone())) }
    });
    // A call bound well under the 20 s hold, with room for B1's reply under machine load.
    let c = connect_with(&url, &Secret::new(new_token().0), Bounds { connect: Duration::from_secs(10), call: Duration::from_secs(2) })
        .unwrap();
    let d = tempfile::tempdir().unwrap();
    let local = local_store(&d);
    local.mark_sync_pending("A1").unwrap();
    local.mark_sync_pending("B1").unwrap();
    let r = sync_pending(&c, &local, &d.path().join("sessions")).unwrap();
    assert_eq!((r.synced, r.tried, r.unreachable), (1, 2, true));
    assert_eq!(r.lines.len(), 1);
    assert!(r.lines[0].starts_with("session A1: server unreachable: "), "{:?}", r.lines);
    assert_eq!(local.sync_pending().unwrap(), ["A1"]);
    assert_eq!(local.requests_for_session("B1").unwrap().len(), 1);
} // mutation: stop at the first unreachable session (B1 not synced)

#[test]
fn denylist_removes_the_server_token_variable() { // C3, §8.4 (R3)
    let fx = fx();
    fx.config("[server]\ntoken = \"env:N37_TEAM_SECRET\"\n");
    let m = fx.merged();
    let rows = [("N37_TEAM_SECRET", "v"), ("GH_TOKEN", "v")].map(|(k, v)| (OsString::from(k), OsString::from(v)));
    let (child, removed) = child_env_with(&Env(rows.to_vec()), &m, &[], false);
    assert_eq!(child.0.iter().map(|(k, _)| k.to_str().unwrap()).collect::<Vec<_>>(), ["GH_TOKEN"]);
    assert_eq!(removed, ["N37_TEAM_SECRET"]);
    let folded = [(OsString::from("n37_team_secret"), OsString::from("v"))];
    assert_eq!(child_env_with(&Env(folded.to_vec()), &m, &[], true).1, ["n37_team_secret"]); // Windows rule
} // mutation: the server token name not in the denylist

#[test]
fn team_file_is_0600_and_round_trips() {
    let d = tempfile::tempdir().unwrap();
    assert!(read_team(d.path()).unwrap().is_none());
    let t: toml::Table = "main_role = \"lead\"\n".parse().unwrap();
    write_team(d.path(), &t).unwrap();
    assert_eq!(read_team(d.path()).unwrap(), Some(t));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(d.path().join(TEAM_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(TEAM_FILE, "team.toml");
} // mutations: default mode; another file name

#[test]
fn doctor_reachability_lines() { // §17
    let s = server("");
    let u = s.h.url().to_string();
    assert_eq!(reachability(&u), format!("{u} reachable"));
    let c = closed_url();
    assert!(reachability(&c).starts_with(&format!("{c} unreachable (")), "{}", reachability(&c));
    assert_eq!(reachability("http://10.0.0.5:8443"), "http://10.0.0.5:8443 not allowed: not https and not a loopback host (C8)");
    let userinfo = ["n37user", "n37pass"].join(":");
    for host in ["127.0.0.1:1", "10.0.0.5:8443"] {
        assert_eq!(
            reachability(&format!("https://{userinfo}@{host}/")),
            format!("https://{host}/ not allowed: the URL must not hold a user name or password (C8)")
        );
    }
    assert_eq!(reachability("not a url"), "[server] url is not a URL");
    s.h.shutdown();
} // mutations: always "reachable"; no C8 check

// ---- launcher fixtures ----

const STORE_VALUE: &str = "n37-store-value-1";
const ROWS_OK: &str = "{\"requests\":[],\"complete\":true}";
const FORGED: &str = "one\ntwo\u{1b}[31m";
const FORGED_ESC: &str = "one\\ntwo\\u{1b}[31m";
fn forged_error() -> String {
    serde_json::json!({ "error": { "type": "bad", "message": FORGED } }).to_string()
}
const STUB_ARGS: [&str; 5] = ["--exact", "stub_child", "--ignored", "--nocapture", "--test-threads=1"];

/// The stub agent: this test binary. Writes `env.json`, `argv.json`, `token.txt`, `view.json`, `session.json` (every
/// file under `ZYGO_SESSION_DIR`), `url` and (unix) `team_mode` into `ZYGO_TEST_OUT`, then runs `ZYGO_TEST_MODE`:
/// `dump`; `request` (one Messages request, `reply`, then `wait`); `wait` (`ready` = the session id, then `go`);
/// `wait_request` (`wait`, then the request); `linger` (starts `linger_child`, then `wait`).
#[test]
#[ignore]
fn stub_child() {
    let Some(out) = std::env::var_os("ZYGO_TEST_OUT").map(PathBuf::from) else { return };
    let mode = std::env::var("ZYGO_TEST_MODE").unwrap_or_default();
    let vars: BTreeMap<String, String> =
        std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    std::fs::write(out.join("env.json"), serde_json::to_vec(&vars).unwrap()).unwrap();
    std::fs::write(out.join("argv.json"), serde_json::to_vec(&std::env::args().collect::<Vec<_>>()).unwrap()).unwrap();
    let dir = PathBuf::from(std::env::var_os("ZYGO_SESSION_DIR").unwrap());
    let sd = SessionDir::new(dir.clone());
    let token = sd.read_token().unwrap();
    std::fs::write(out.join("token.txt"), &token).unwrap();
    let _ = std::fs::copy(sd.view_json(), out.join("view.json"));
    let _ = std::fs::copy(sd.url(), out.join("url"));
    std::fs::write(out.join("session.json"), serde_json::to_vec(&files_under(&dir)).unwrap()).unwrap();
    #[cfg(unix)]
    if let Ok(m) = std::fs::metadata(dir.join(TEAM_FILE)) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(out.join("team_mode"), format!("{:o}", m.permissions().mode() & 0o777)).unwrap();
    }
    match mode.as_str() {
        "request" => {
            child_request(&out, &token);
            child_wait(&out);
        }
        "wait" => child_wait(&out),
        "linger" => {
            // The child outlives the stub on purpose; nothing waits for it.
            #[allow(clippy::zombie_processes)]
            let _lingering = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "linger_child", "--ignored", "--test-threads=1"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            child_wait(&out);
        }
        "wait_request" => {
            child_wait(&out);
            child_request(&out, &token);
        }
        _ => {}
    }
}

/// The stub's own child in mode `linger` (it gets every open file the stub inherited): writes `lingering`, then
/// waits up to 30 s for `release`.
#[test]
#[ignore]
fn linger_child() {
    let Some(out) = std::env::var_os("ZYGO_TEST_OUT").map(PathBuf::from) else { return };
    if std::env::var("ZYGO_TEST_MODE").as_deref() != Ok("linger") {
        return;
    }
    std::fs::write(out.join("lingering"), "").unwrap();
    until(Duration::from_secs(30), || out.join("release").exists());
}

/// Writes `ready` (the session id) by rename, so a reader never sees it empty, then waits up to 30 s for `go`.
fn child_wait(out: &Path) {
    std::fs::write(out.join("ready.tmp"), std::env::var("ZYGO_SESSION").unwrap()).unwrap();
    std::fs::rename(out.join("ready.tmp"), out.join("ready")).unwrap();
    until(Duration::from_secs(30), || out.join("go").exists());
}

fn child_request(out: &Path, token: &str) {
    let url = Url::parse(&std::env::var("ZYGO_URL").unwrap()).unwrap();
    let body = r#"{"model":"test-up","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#;
    let mut s = std::net::TcpStream::connect((url.host_str().unwrap(), url.port().unwrap())).unwrap();
    let session = std::env::var("ZYGO_SESSION").unwrap();
    write!(
        s,
        "POST {}anthropic/v1/messages HTTP/1.1\r\nhost: {}\r\ncontent-type: application/json\r\nanthropic-version: 2023-06-01\r\nx-api-key: {token}\r\nx-zygo-session: {session}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        url.path(),
        url.host_str().unwrap(),
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
    std::fs::write(out.join("reply"), reply.lines().next().unwrap_or_default()).unwrap();
}

/// Every file under `dir`, keyed by its `/`-separated relative path, its bytes read lossily as UTF-8.
fn files_under(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.is_file()
                && let Ok(b) = std::fs::read(&p)
            {
                // A locked file (`pid`) cannot be read on Windows, where locks are mandatory.
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                files.insert(rel, String::from_utf8_lossy(&b).into_owned());
            }
        }
    }
    files
}

/// The files under `root` (not under `skip`, when it is not empty) whose bytes hold `needle`.
fn files_with(root: &Path, needle: &[u8], skip: &Path) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if !skip.as_os_str().is_empty() && p.starts_with(skip) {
                continue;
            }
            if p.is_dir() {
                todo.push(p);
            } else if std::fs::read(&p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle)) {
                hits.push(p);
            }
        }
    }
    hits
}

struct Stub {
    agent: Agent,
    program: String,
    refusal: Option<String>,
}

fn stub() -> Stub {
    Stub { agent: Agent::ClaudeCode, program: std::env::current_exe().unwrap().display().to_string(), refusal: None }
}

fn refusing_stub() -> Stub {
    Stub { refusal: Some("stub refusal".into()), ..stub() }
}

impl AgentAdapter for Stub {
    fn agent(&self) -> Agent {
        self.agent
    }
    fn program(&self) -> &str {
        &self.program
    }
    fn install_hint(&self) -> &str {
        "install the stub agent: TEST-HINT"
    }
    fn picked_model(&self, _args: &[String]) -> Option<String> {
        None
    }
    fn build(&self, _ctx: &LaunchCtx) -> anyhow::Result<ChildSpec> {
        Ok(ChildSpec { args: STUB_ARGS.map(String::from).to_vec(), env: vec![], lines: vec![], log: vec![] })
    }
    fn preflight(&self, _ctx: &LaunchCtx) -> Result<(), Preflight> {
        Ok(())
    }
    fn refusal(&self, _ctx: &LaunchCtx) -> Option<String> {
        self.refusal.clone()
    }
}

fn opts(fx: &Fx, mode: &str, server: Option<String>, extra: &[(&str, &str)]) -> LaunchOpts {
    let mut env = vec![
        (OsString::from("PATH"), OsString::from(&fx.bin)),
        (OsString::from("ACT_CONFIG_DIR"), OsString::from(fx.root.join("act"))),
        (OsString::from("ZYGO_TEST_OUT"), OsString::from(&fx.out)),
        (OsString::from("ZYGO_TEST_MODE"), OsString::from(mode)),
    ];
    env.extend(extra.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
    if cfg!(windows)
        && let Some(v) = std::env::var_os("SystemRoot")
    {
        env.push(("SystemRoot".into(), v));
    }
    let mut o = LaunchOpts::new(Env(env), fx.cwd.clone(), fx.paths.clone());
    o.builtin = Some(builtin());
    o.stderr = Some(Arc::default());
    o.server = server;
    o
}

/// `[server] url` and, when `token`, `token = "env:ZYGO_TEAM_TOKEN"`.
fn serve_config(fx: &Fx, url: &Url, token: bool) {
    let t = if token { "token = \"env:ZYGO_TEAM_TOKEN\"\n" } else { "" };
    fx.config(&format!("[server]\nurl = \"{url}\"\n{t}"));
}

/// Exit code and captured stderr; the out directory is emptied first.
fn run_with(adapter: &dyn AgentAdapter, o: LaunchOpts) -> (i32, String) {
    let out = PathBuf::from(o.env.get("ZYGO_TEST_OUT").unwrap());
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let buf = o.stderr.clone().unwrap();
    let code = Launcher::run(adapter, o).code;
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    (code, err)
}

fn run(o: LaunchOpts) -> (i32, String) {
    run_with(&stub(), o)
}

/// The launch on its own thread; its stderr buffer.
fn spawn_launch(o: LaunchOpts) -> (std::thread::JoinHandle<i32>, Arc<Mutex<Vec<u8>>>) {
    let out = PathBuf::from(o.env.get("ZYGO_TEST_OUT").unwrap());
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let buf = o.stderr.clone().unwrap();
    (std::thread::spawn(move || Launcher::run(&stub(), o).code), buf)
}

fn json<T: serde::de::DeserializeOwned>(p: PathBuf) -> T {
    serde_json::from_slice(&std::fs::read(p).expect("the child ran")).unwrap()
}
fn env_json(fx: &Fx) -> BTreeMap<String, String> {
    json(fx.out.join("env.json"))
}
fn view(fx: &Fx) -> SessionView {
    json(fx.out.join("view.json"))
}
fn log_of(session: &BTreeMap<String, String>) -> &str {
    session.get("session.log").map(String::as_str).unwrap_or_default()
}

fn store_value(fx: &Fx) {
    let dir = fx.root.join("act");
    let lookup = Lookup {
        env: Box::new(move |k| (k == "ACT_CONFIG_DIR").then(|| dir.clone().into_os_string())),
        home: None,
        known_config: None,
        windows: cfg!(windows),
    };
    let p = store_path(&fx.cwd, &lookup);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, format!("KEY={STORE_VALUE}\n")).unwrap();
}

/// One request on 127.0.0.1, answered with a minimal Messages response.
fn mock_upstream() -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = l.accept() else { return };
        if read_request(&mut s).is_none() {
            return;
        }
        let reply = r#"{"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
    });
    port
}

/// The server's `[[model]] test-up`, served by the mock upstream.
fn up_model(port: u16) -> String {
    format!(
        "[[model]]\nid = \"test-up\"\n[[model.provider]]\nname = \"mock\"\nbase_url = \"http://127.0.0.1:{port}\"\ndialects = [\"anthropic\"]\nupstream_model = \"test-up\"\n"
    )
}

/// A real session grant (from a local proxy) for the recorder to hand out.
fn grant_json(fx: &Fx) -> (String, String) {
    let p = Proxy::start_local_with(Arc::new(fx.merged()), StartOpts::default()).unwrap();
    let g = p.create_session(req()).unwrap();
    p.shutdown();
    (g.session.clone(), serde_json::to_string(&g).unwrap())
}

/// The recorder routes of a whole serve launch; `rows` answers `GET …/requests`.
fn launch_routes(fx: &Fx, grant: String, rows: (u16, String)) -> impl Fn(&Rec) -> Option<(u16, String)> + Send + Sync + 'static {
    let config = serde_json::to_string(&TeamConfigReply::from_merged(&fx.merged())).unwrap();
    move |r| {
        Some(match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/v1/zygo/config") => (200, config.clone()),
            ("GET", "/v1/zygo/plugins") => (200, "{\"marketplace\":[]}".into()),
            ("POST", "/v1/zygo/sessions") => (200, grant.clone()),
            ("GET", p) if p.ends_with("/requests") => rows.clone(),
            _ => (204, String::new()),
        })
    }
}

// ---- launcher ----

#[test]
fn plain_http_to_a_remote_server_exits_2() { // C8, §20 #13 client part
    let fx = fx();
    let (code, err) = run(opts(&fx, "dump", Some("http://10.0.0.5:8443".into()), &[]));
    assert_eq!(code, 2);
    assert_eq!(err, format!("zygo: {URL_RULE}\n"));
    assert!(!fx.out.join("env.json").exists());
    assert!(!fx.paths.sessions_dir.exists());
    let s = server("");
    serve_config(&fx, &s.h.url(), true);
    assert_eq!(run(opts(&fx, "dump", Some("http://10.0.0.5:8443".into()), &[("ZYGO_TEAM_TOKEN", s.alice.expose())])).0, 2);
    fx.config("[server]\nurl = \"http://10.0.0.5:8443\"\n");
    assert_eq!(run(opts(&fx, "dump", None, &[])).0, 2);
    s.h.shutdown();
} // mutations: no C8 check; config url preferred over the flag

/// The C8 refusal text.
const URL_RULE: &str = "--server/[server] url must be https://, or http:// to a loopback host, with no user name or password (C8)";

#[test]
fn a_server_url_with_userinfo_exits_2_and_sends_nothing() { // U1 (round 3), C8
    let fx = fx();
    let (url, rec) = recorder(|_| Some((204, String::new())));
    let (user, pass) = ("n37user", ["n37", "pass"].concat());
    let with_info = format!("http://{user}:{pass}@127.0.0.1:{}/", url.port().unwrap());
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(with_info), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 2, "{err}");
    assert_eq!(err, format!("zygo: {URL_RULE}\n"));
    assert!(!err.contains(user) && !err.contains(&pass), "{err}");
    std::thread::sleep(Duration::from_millis(200));
    assert!(rec.lock().unwrap().is_empty(), "{:?}", rec.lock().unwrap());
    assert!(!fx.paths.sessions_dir.exists());
} // mutation: userinfo allowed by `transport_allowed` → the server records `GET /v1/zygo/config`

#[test]
fn unreachable_server_exits_12() { // §8.2, §7 note
    let fx = fx();
    fx.config(&format!("[server]\nurl = \"{}\"\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n", closed_url()));
    let t = new_token().0;
    let (code, err) = run(opts(&fx, "dump", None, &[("ZYGO_TEAM_TOKEN", &t)]));
    assert_eq!(code, 12, "{err}");
    assert!(err.starts_with("zygo: server unreachable: "), "{err}");
    assert!(!err.contains(&t));
    assert!(!fx.out.join("env.json").exists());
    assert!(!fx.paths.sessions_dir.exists());
} // mutation: unreachable → 1

#[test]
fn an_unreachable_server_has_spawned_false() { // AU60
    let fx = fx();
    serve_config(&fx, &Url::parse(&closed_url()).unwrap(), true);
    let t = new_token().0;
    let o = Launcher::run(&stub(), opts(&fx, "dump", None, &[("ZYGO_TEAM_TOKEN", &t)]));
    assert_eq!(o, zygo_launch::launcher::LaunchOutcome { code: 12, signal: None, spawned: false, received: false });
}

#[test]
fn a_server_that_never_answers_exits_12_at_the_call_bound() { // §7 note: "a timeout is server unreachable (exit 12 at launch)"
    let fx = fx();
    let url = silent_server(Duration::from_secs(20));
    let t = new_token().0;
    let mut o = opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &t)]);
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    o.serve_bounds = Some(Bounds { connect: Duration::from_secs(10), call: Duration::from_millis(300) });
    let t0 = Instant::now();
    let (code, err) = run(o);
    assert_eq!(code, 12, "{err}");
    assert!(err.starts_with("zygo: server unreachable: "), "{err}");
    assert!(t0.elapsed() < Duration::from_secs(10));
} // mutations: the launcher ignores `serve_bounds` or the timeout maps to 1

#[test]
fn missing_or_wrong_team_token_exits_1() {
    let fx = fx();
    let s = server("");
    serve_config(&fx, &s.h.url(), false);
    assert_eq!(
        run(opts(&fx, "dump", None, &[])),
        (1, "zygo: serve mode needs a team token: [server] token is not set or names an empty value\n".into())
    );
    serve_config(&fx, &s.h.url(), true);
    assert_eq!(run(opts(&fx, "dump", None, &[])).0, 1);
    let (code, err) = run(opts(&fx, "dump", None, &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 1);
    assert!(err.starts_with("zygo: team config refused: zygo_token_invalid"), "{err}");
    s.h.shutdown();
} // mutation: no token check

#[test]
fn a_refusal_without_an_error_body_names_the_http_status() { // U4 (round 3)
    let fx = fx();
    let (url, _rec) = recorder(|_| Some((502, "bad gateway".into())));
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!((code, err.as_str()), (1, "zygo: team config refused: HTTP 502\n"));
} // mutation: the empty `{code}: {message}` kept (→ "team config refused: : ")

#[test]
fn a_refusal_message_cannot_forge_a_line_or_escape_the_terminal() { // N64 G4
    let fx = fx();
    let body = r#"{"error":{"type":"bad","message":"one\ntwo\u001b[31m"}}"#;
    let (url, _rec) = recorder(move |_| Some((400, body.into())));
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 1);
    assert_eq!(err, "zygo: team config refused: bad: one\\ntwo\\u{1b}[31m\n");
    assert!(!err.contains('\u{1b}'));
} // mutation: `esc` removed (→ two lines, one with ESC)

#[test]
fn an_unparsable_reply_reads_invalid_reply() { // V3 (round 4)
    let (url, _rec) = recorder(|_| Some((200, "not json".into())));
    let c = ServeClient::connect(&url, &Secret::new(new_token().0)).unwrap();
    assert_eq!(c.team_config().unwrap_err().to_string(), "invalid reply");
} // mutation: `{code}: {message}` with an empty code (→ ": invalid reply")

#[test]
fn a_session_refusal_without_an_error_body_names_the_http_status() { // V3 (round 4)
    let fx = fx();
    let routes = launch_routes(&fx, String::new(), (200, "{\"requests\":[],\"complete\":true}".into()));
    let (url, _rec) = recorder(move |r| if r.method == "POST" && r.path == "/v1/zygo/sessions" { Some((502, "bad gateway".into())) } else { routes(r) });
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!((code, err.as_str()), (1, "zygo: session refused: HTTP 502\n"));
} // mutation: the empty message printed (→ "session refused: ")

#[test]
fn team_config_is_merged_as_layer_2() { // §6.1, §7.2
    let fx = fx();
    let s = server("[results]\ntrim_threshold_bytes = 20000\n");
    serve_config(&fx, &s.h.url(), true);
    let (code, err) = run(opts(&fx, "dump", None, &[("ZYGO_TEAM_TOKEN", s.alice.expose())]));
    assert_eq!(code, 0, "{err}");
    assert_eq!(view(&fx).trim_threshold_bytes, 20000);
    let session: BTreeMap<String, String> = json(fx.out.join("session.json"));
    assert_eq!(session["team.toml"].parse::<toml::Table>().unwrap()["results"]["trim_threshold_bytes"].as_integer(), Some(20000));
    assert_eq!(env_json(&fx)["ZYGO_URL"], s.h.url().to_string());
    assert_eq!(std::fs::read_to_string(fx.out.join("url")).unwrap(), s.h.url().to_string());
    s.h.shutdown();
} // mutations: the second load without the team table; the local proxy URL kept; no team file

#[test]
fn serve_launch_order_and_no_stored_value() { // §7.2, §14, §15.3, §20 #29d
    let fx = fx();
    let (sid, grant) = grant_json(&fx);
    let (url, rec) = recorder(launch_routes(&fx, grant, (200, "{\"requests\":[],\"complete\":true}".into())));
    store_value(&fx);
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let t = new_token().0;
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &t)]));
    assert_eq!(code, 0, "{err}");
    let r = rec.lock().unwrap();
    let seen: Vec<String> = r.iter().filter(|x| !x.path.ends_with("/heartbeat")).map(|x| format!("{} {}", x.method, x.path)).collect();
    assert_eq!(
        seen,
        [
            "GET /v1/zygo/config".to_string(),
            "GET /v1/zygo/plugins".into(),
            "POST /v1/zygo/sessions".into(),
            format!("GET /v1/zygo/sessions/{sid}/requests"),
            format!("DELETE /v1/zygo/sessions/{sid}"),
        ]
    );
    assert!(r.iter().all(|x| x.headers["authorization"] == format!("Bearer {t}")));
    let body: serde_json::Value = serde_json::from_slice(&r[2].body).unwrap();
    let mut keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["agent", "agent_version", "passthrough", "profile", "repo", "work"]);
    assert!(r.iter().all(|x| !x.body.windows(STORE_VALUE.len()).any(|w| w == STORE_VALUE.as_bytes())));
    assert!(Store::open(&fx.paths.usage_db).unwrap().sync_pending().unwrap().is_empty()); // complete → cleared
} // mutations: DELETE before the rows fetch; no GET /plugins; `#[serde(skip)]` dropped from `SessionRequest.stored_values`
  // (the body gains `stored_values`); a route without the team token. The launcher's serve arm passes no stored value
  // (`req(Vec::new())`), and the skip hides any it passed, so the STORE_VALUE scan guards both together.

#[test]
fn session_is_pending_while_it_runs_and_rows_are_fetched_before_delete() { // §14, S5
    let fx = fx();
    let s = server(&up_model(mock_upstream()));
    serve_config(&fx, &s.h.url(), true);
    let mut o = opts(&fx, "request", None, &[("ZYGO_TEAM_TOKEN", s.alice.expose())]);
    o.heartbeat = Some(Duration::from_millis(20));
    let (j, buf) = spawn_launch(o);
    assert!(until(Duration::from_secs(30), || fx.out.join("ready").exists()));
    let sid = std::fs::read_to_string(fx.out.join("ready")).unwrap();
    assert_eq!(Store::open(&fx.paths.usage_db).unwrap().sync_pending().unwrap(), [sid.as_str()]); // marked at creation
    std::fs::write(fx.out.join("go"), "").unwrap();
    assert_eq!(j.join().unwrap(), 0);
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(std::fs::read_to_string(fx.out.join("reply")).unwrap().starts_with("HTTP/1.1 200"), "{err}");
    let local = Store::open(&fx.paths.usage_db).unwrap();
    let rows = local.requests_for_session(&sid).unwrap();
    assert_eq!(rows.len(), 1, "{err}"); // the server waited for the row (step 3b)
    assert_eq!(
        (rows[0].provider.as_str(), rows[0].estimated_usd, rows[0].token_label.as_deref(), rows[0].status),
        ("", 0.0, Some("alice"), 200)
    );
    assert!(local.sync_pending().unwrap().is_empty());
    assert!(matches!(client(&s, &s.alice).delete_session(&sid), Err(e) if unknown(&e)));
    assert!(!err.contains(RESTARTED) && !err.contains("usage rows"), "{err}");
    s.h.shutdown();
} // mutations: no mark at creation; no fetch; no clear when complete; no DELETE; a relaunch line at a normal exit

#[test]
fn failed_or_incomplete_rows_keep_the_session_pending() { // §14, B2, S5
    for incomplete in [false, true] {
        let fx = fx();
        let (sid, grant) = grant_json(&fx);
        let rows = if incomplete {
            (200, serde_json::json!({ "requests": [row(&sid, "alice")], "complete": false }).to_string())
        } else {
            (503, "{\"error\":{\"type\":\"zygo_usage_unavailable\",\"message\":\"usage database unavailable\"}}".into())
        };
        let (url, rec) = recorder(launch_routes(&fx, grant, rows));
        fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
        let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
        assert_eq!(code, 0, "{err}");
        let local = Store::open(&fx.paths.usage_db).unwrap();
        assert_eq!(local.sync_pending().unwrap(), [sid.as_str()]);
        if incomplete {
            assert!(err.lines().any(|l| l == "zygo: usage rows incomplete; zygo usage --sync retries"), "{err}");
            assert_eq!(local.requests_for_session(&sid).unwrap().len(), 1);
        } else {
            assert!(
                err.lines().any(|l| l.starts_with("zygo: usage rows not fetched (") && l.ends_with("); zygo usage --sync retries")),
                "{err}"
            );
            assert!(rec.lock().unwrap().iter().any(|x| x.method == "DELETE"));
        }
    }
} // mutations: cleared on a failed fetch; cleared or not inserted when incomplete

#[test]
fn a_server_that_stalls_after_creation_costs_one_call_bound_at_exit() { // S3 (round 2), §7.2 suspend, §7 note
    let fx = fx();
    let (sid, grant) = grant_json(&fx);
    let launch = launch_routes(&fx, grant, (200, String::new()));
    // The rows fetch and DELETE get no reply (held 20 s); the launch calls are answered.
    let (url, rec) = recorder(move |r| if r.path.ends_with("/requests") || r.method == "DELETE" { None } else { launch(r) });
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let mut o = opts(&fx, "wait", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]);
    let call = Duration::from_secs(4);
    o.serve_bounds = Some(Bounds { connect: Duration::from_secs(10), call });
    let (j, buf) = spawn_launch(o);
    assert!(until(Duration::from_secs(30), || fx.out.join("ready").exists()));
    let t0 = Instant::now();
    std::fs::write(fx.out.join("go"), "").unwrap();
    assert_eq!(j.join().unwrap(), 0);
    let took = t0.elapsed();
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    // One call bound for the rows fetch; a DELETE after it would add a second (2 × call or more).
    assert!(took < 2 * call, "{took:?}: {err}");
    assert!(err.lines().any(|l| l.starts_with("zygo: usage rows not fetched (")), "{err}");
    assert!(!rec.lock().unwrap().iter().any(|x| x.method == "DELETE"));
    assert_eq!(Store::open(&fx.paths.usage_db).unwrap().sync_pending().unwrap(), [sid.as_str()]);
} // mutation: DELETE sent after an unreachable rows fetch → a DELETE recorded and `took` ≥ 2 × call

#[test]
fn lost_session_prints_the_relaunch_message_once_and_clears_the_mark() { // §7.2, S5
    let fx = fx();
    let s = server("");
    serve_config(&fx, &s.h.url(), true);
    let mut o = opts(&fx, "wait", None, &[("ZYGO_TEAM_TOKEN", s.alice.expose())]);
    o.heartbeat = Some(Duration::from_millis(20));
    let (j, buf) = spawn_launch(o);
    assert!(until(Duration::from_secs(30), || fx.out.join("ready").exists()));
    let sid = std::fs::read_to_string(fx.out.join("ready")).unwrap();
    client(&s, &s.alice).delete_session(&sid).unwrap();
    let text = || String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(until(Duration::from_secs(30), || text().contains(RESTARTED)));
    std::fs::write(fx.out.join("go"), "").unwrap();
    assert_eq!(j.join().unwrap(), 0);
    assert_eq!(text().lines().filter(|l| *l == RESTARTED).count(), 1);
    assert!(Store::open(&fx.paths.usage_db).unwrap().sync_pending().unwrap().is_empty()); // unknown at exit → cleared
    s.h.shutdown();
} // mutations: no heartbeat; unknown keeps the mark

#[test]
fn team_and_session_tokens_stay_where_the_spec_puts_them() { // C3, §8.4
    let fx = fx();
    std::fs::copy(std::env::current_exe().unwrap(), fx.bin.join(format!("opencode{}", std::env::consts::EXE_SUFFIX))).unwrap();
    let s = server("");
    fx.config(&format!("[server]\nurl = \"{}\"\ntoken = \"env:N37_TEAM_SECRET\"\n", s.h.url()));
    let t = s.alice.expose();
    let mut o = opts(&fx, "dump", None, &[("N37_TEAM_SECRET", t)]);
    o.args = STUB_ARGS.map(String::from).to_vec();
    let (code, err) = run_with(&OpenCode::default(), o);
    assert_eq!(code, 0, "{err}");
    let st = std::fs::read_to_string(fx.out.join("token.txt")).unwrap();
    let env = env_json(&fx);
    assert!(!env.contains_key("N37_TEAM_SECRET"));
    assert!(env.values().all(|v| !v.contains(t)));
    let session: BTreeMap<String, String> = json(fx.out.join("session.json"));
    assert!(log_of(&session).contains("env: removed N37_TEAM_SECRET"), "{}", log_of(&session)); // names only
    assert!(env.iter().all(|(k, v)| k == "OPENCODE_CONFIG_CONTENT" || !v.contains(&st))); // §8.4 accepted limit
    let argv: Vec<String> = json(fx.out.join("argv.json"));
    assert!(argv.iter().all(|a| !a.contains(t) && !a.contains(&st)));
    assert!(!err.contains(t) && !err.contains(&st));
    assert!(session.contains_key("token"));
    let team: toml::Table = session["team.toml"].parse().unwrap();
    assert!(team["model"].as_array().is_some_and(|m| !m.is_empty()));
    #[cfg(unix)]
    assert_eq!(std::fs::read_to_string(fx.out.join("team_mode")).unwrap(), "600");
    assert!(session.iter().all(|(k, v)| !v.contains(t) && (k == "token" || !v.contains(&st))));
    assert!(files_with(&fx.root, t.as_bytes(), &fx.out).is_empty());
    assert!(files_with(&fx.root, st.as_bytes(), &fx.out).is_empty());
    assert!(files_with(&s.root, t.as_bytes(), Path::new("")).is_empty());
    assert!(files_with(&s.root, st.as_bytes(), Path::new("")).is_empty());
    assert!(s.lines.lock().unwrap().iter().all(|l| !l.contains(t) && !l.contains(&st)));
    s.h.shutdown();
} // mutations: the `[server] token` env name not denylisted (the token reaches the agent's env); team.toml missing or not 0600

#[test]
fn team_file_holds_no_probe_value() { // B3, R2 end to end
    let fx = fx();
    let s = server(
        "[role.code]\nmodel = \"claude-sonnet-5-5\"\nkey = \"env:SECRET_NAME\"\n[role.code.provider]\nbase_url = \"https://internal.example\"\n[profile.x.features]\nkey = \"env:SECRET_NAME\"\n",
    );
    serve_config(&fx, &s.h.url(), true);
    let (code, err) = run(opts(&fx, "dump", None, &[("ZYGO_TEAM_TOKEN", s.alice.expose())]));
    assert_eq!(code, 0, "{err}");
    let session: BTreeMap<String, String> = json(fx.out.join("session.json"));
    let text = &session["team.toml"];
    for x in ["SECRET_NAME", "internal.example", "provider"] {
        assert!(!text.contains(x), "{x}: {text}");
    }
    let t: toml::Table = text.parse().unwrap();
    assert_eq!(t["role"]["code"].as_table().unwrap().keys().collect::<Vec<_>>(), ["model"]);
    s.h.shutdown();
} // mutation: raw file values sent

#[test]
fn a_launch_refused_after_creation_still_deletes_the_session() { // R4, §8.2 step 6
    let fx = fx();
    let (sid, grant) = grant_json(&fx);
    let (url, rec) = recorder(launch_routes(&fx, grant, (200, "{\"requests\":[],\"complete\":true}".into())));
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run_with(&refusing_stub(), opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 2);
    assert!(err.contains("zygo: stub refusal"), "{err}");
    let r = rec.lock().unwrap();
    assert!(r.iter().any(|x| x.method == "DELETE" && x.path == format!("/v1/zygo/sessions/{sid}")));
    assert!(Store::open(&fx.paths.usage_db).unwrap().sync_pending().unwrap().is_empty());
} // mutations: the refusal path returns before the Finish guard; no DELETE on that path

#[test]
fn a_grant_that_is_unusable_or_names_another_session_is_refused_before_any_file() { // N70 B1, B-1, §8.2 step 2 note 2026-11-19
    const INVALID: &str = "zygo: session refused: invalid reply\n";
    const TAKEN: &str = "zygo: session refused: the session directory already exists\n";
    let mut failed = Vec::new();
    for case in 0..9 {
        let fx = fx();
        let live = fx.paths.sessions_dir.join("LIVE1");
        std::fs::create_dir_all(&live).unwrap();
        let _lock = locked_pid(&live);
        std::fs::write(live.join("token"), "other-session").unwrap();
        let store = Store::open(&fx.paths.usage_db).unwrap();
        store.mark_sync_pending("LIVE1").unwrap();
        let keep = [fx.paths.cache_dir.join("x/keep"), fx.paths.sessions_dir.join("other/keep"), fx.root.join("victim/keep")];
        for k in &keep {
            std::fs::create_dir_all(k.parent().unwrap()).unwrap();
            std::fs::write(k, "").unwrap();
        }
        let abs = fx.root.join("victim").display().to_string();
        let long = "A".repeat(27);
        let (field, bad, want) = [
            ("session", "../x", INVALID),
            ("session", "", INVALID),
            ("session", "/abs", INVALID),
            ("session", "a\nb", INVALID),
            ("session", abs.as_str(), INVALID),
            ("session", long.as_str(), INVALID),
            ("token", "t\r\nx-other: 1", INVALID),
            ("session", "LIVE1", TAKEN),
            ("session", "live1", TAKEN),
        ][case];
        if bad == "live1" && !fx.paths.sessions_dir.join("live1").exists() {
            continue; // a case-sensitive file system: `live1` is another directory, nothing is at risk
        }
        let mut g: serde_json::Value = serde_json::from_str(&grant_json(&fx).1).unwrap();
        g[field] = bad.into();
        let (url, rec) = recorder(launch_routes(&fx, g.to_string(), (200, ROWS_OK.into())));
        fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
        let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
        // A missing sessions directory (the red run's `""` case removes it) lists as empty.
        let mut names: Vec<String> = std::fs::read_dir(&fx.paths.sessions_dir)
            .into_iter()
            .flatten()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let calls: Vec<String> = rec.lock().unwrap().iter().map(|r| format!("{} {}", r.method, r.path)).collect();
        let checks = [
            ("exit and text", (code, err.as_str()) == (1, want)),
            ("kept files", keep.iter().all(|k| k.exists()) && live.join("token").exists()),
            ("session dirs", names == ["LIVE1", "other"]),
            ("calls", calls == ["GET /v1/zygo/config", "GET /v1/zygo/plugins", "POST /v1/zygo/sessions"]),
            ("pending marks", store.sync_pending().unwrap() == ["LIVE1"]),
            ("agent", !fx.out.join("env.json").exists()),
        ];
        for (what, ok) in checks {
            if !ok {
                failed.push(format!("case {case} ({field} {bad:?}): {what}; code {code}, err {err:?}, calls {calls:?}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
} // mutations: no grant check (cases 0–6 launch); no length limit (case 5 launches); token unchecked (case 6 launches);
  // recursive create (case 7: LIVE1 is wiped and DELETE sent); mark before open (pending marks gain the id)

#[cfg(unix)]
#[test]
fn the_first_launch_makes_the_cache_and_sessions_dirs_0700() { // N70 N-1; as core_basics.rs session_dir_modes
    use std::os::unix::fs::PermissionsExt;
    let fx = fx();
    let (url, _rec) = recorder(launch_routes(&fx, grant_json(&fx).1, (200, ROWS_OK.into())));
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    assert!(!fx.paths.cache_dir.exists());
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 0, "{err}");
    let m = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!((m(&fx.paths.cache_dir), m(&fx.paths.sessions_dir)), (0o700, 0o700));
} // mutation: `create_dir_all` for the parents (0o755 under the usual umask 022)

#[test]
fn a_sessions_path_that_is_a_file_is_not_called_another_session() { // N70 N-3
    let fx = fx();
    let (url, _rec) = recorder(launch_routes(&fx, grant_json(&fx).1, (200, ROWS_OK.into())));
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    std::fs::create_dir_all(&fx.paths.cache_dir).unwrap();
    std::fs::write(&fx.paths.sessions_dir, "").unwrap();
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 1, "{err}");
    assert!(err.starts_with("zygo: cannot create the session directory: "), "{err}");
    assert!(fx.paths.sessions_dir.is_file());
} // mutation: any `AlreadyExists` taken as `Taken` ("zygo: session refused: the session directory already exists")

#[test]
fn server_text_cannot_forge_a_line_at_session_creation() { // N70 S2: launcher.rs:272
    let fx = fx();
    let routes = launch_routes(&fx, String::new(), (200, ROWS_OK.into()));
    let (url, _rec) =
        recorder(move |r| if r.method == "POST" && r.path == "/v1/zygo/sessions" { Some((400, forged_error())) } else { routes(r) });
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!((code, err), (1, format!("zygo: session refused: {FORGED_ESC}\n")));
} // mutation: `esc` removed at :272

#[test]
fn server_text_cannot_forge_a_line_after_session_creation() { // N70 S2: launcher.rs:294, :297, :350, :592
    let fx = fx();
    let mut g: serde_json::Value = serde_json::from_str(&grant_json(&fx).1).unwrap();
    g["profile_warning"] = FORGED.into();
    g["usage_note"] = FORGED.into();
    let routes = launch_routes(&fx, g.to_string(), (400, forged_error()));
    let (url, rec) = recorder(move |r| if r.path.ends_with("/compact") { Some((400, forged_error())) } else { routes(r) });
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let mut o = opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]);
    o.compact = Some(WindowArg::Tokens(600_000));
    let (code, err) = run(o);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!(
            "zygo: {FORGED_ESC}; launching without the profile\nzygo: {FORGED_ESC}\nzygo: bad: {FORGED_ESC}\n\
             zygo: usage rows not fetched (bad: {FORGED_ESC}); zygo usage --sync retries\n"
        )
    );
    assert!(rec.lock().unwrap().iter().any(|r| r.method == "DELETE"));
} // mutations: `esc` removed at any one of the four sites (its line splits and holds ESC)

#[test]
fn a_team_config_error_cannot_escape_the_terminal() { // N70: launcher.rs:175-176
    let fx = fx();
    let config = r#"{"role":{"x\n\u001b[31m":{"model":5}},"model":[]}"#;
    let (url, _rec) = recorder(move |r| {
        Some(match r.path.as_str() {
            "/v1/zygo/config" => (200, config.into()),
            "/v1/zygo/plugins" => (200, "{\"marketplace\":[]}".into()),
            _ => (204, String::new()),
        })
    });
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(
        (code, err.as_str()),
        (11, "zygo: team config (team): role.\"x\\n\\u{1b}[31m\".model: expected a string, found an integer\n")
    );
} // the text is escaped where `describe` makes it (zygo-config typed.rs); the launcher's own `esc` at :175 is a second guard this test does not reach

#[test]
fn a_redaction_note_cannot_forge_a_session_log_line() { // N70: launcher.rs:317-319
    let fx = fx();
    let mut g: serde_json::Value = serde_json::from_str(&grant_json(&fx).1).unwrap();
    g["redaction_note"] = FORGED.into();
    let (url, _rec) = recorder(launch_routes(&fx, g.to_string(), (200, ROWS_OK.into())));
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let (code, err) = run(opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]));
    assert_eq!(code, 0, "{err}");
    let session: BTreeMap<String, String> = json(fx.out.join("session.json"));
    let log = log_of(&session);
    assert!(log.lines().any(|l| l == FORGED_ESC), "{log}");
    assert!(!log.contains('\u{1b}'), "{log}");
} // mutation: `esc` removed at :318 (the note splits into `one` and `two<ESC>[31m`)

#[test]
fn a_child_of_the_launcher_does_not_keep_the_session_live() { // U3 (round 3): files open close-on-exec
    let fx = fx();
    let s = server("");
    serve_config(&fx, &s.h.url(), true);
    let (j, buf) = spawn_launch(opts(&fx, "linger", None, &[("ZYGO_TEAM_TOKEN", s.alice.expose())]));
    assert!(until(Duration::from_secs(30), || fx.out.join("ready").exists() && fx.out.join("lingering").exists()));
    let sid = std::fs::read_to_string(fx.out.join("ready")).unwrap();
    let pid = std::fs::File::open(fx.paths.sessions_dir.join(&sid).join(PID_FILE)).unwrap();
    let during = fs4::FileExt::try_lock(&pid);
    assert!(matches!(during, Err(fs4::TryLockError::WouldBlock)), "the launcher holds the lock: {during:?}");
    std::fs::write(fx.out.join("go"), "").unwrap();
    let code = j.join().unwrap();
    // The launcher's handle is gone; the stub's own child still runs with the files the stub inherited. A process
    // that another test forks holds a copy of the fd until its exec, so the lock may come free a moment later (V1).
    let after = until(Duration::from_secs(5), || fs4::FileExt::try_lock(&pid).is_ok());
    std::fs::write(fx.out.join("release"), "").unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&buf.lock().unwrap()));
    assert!(after, "a child holds the lock");
    s.h.shutdown();
} // mutations: no lock (→ `during` is Ok); the lock file inherited by the agent (→ the lock stays held past the 5 s wait)

#[test]
fn sync_during_a_live_session_is_skipped_and_exit_brings_the_rows() { // R1
    let fx = fx();
    let s = server(&up_model(mock_upstream()));
    serve_config(&fx, &s.h.url(), true);
    let (j, _buf) = spawn_launch(opts(&fx, "wait_request", None, &[("ZYGO_TEAM_TOKEN", s.alice.expose())]));
    assert!(until(Duration::from_secs(30), || fx.out.join("ready").exists()));
    let sid = std::fs::read_to_string(fx.out.join("ready")).unwrap();
    let local = Store::open(&fx.paths.usage_db).unwrap();
    let r = sync_pending(&client(&s, &s.alice), &local, &fx.paths.sessions_dir).unwrap();
    assert_eq!((r.tried, r.live), (0, 1));
    assert_eq!(local.sync_pending().unwrap(), [sid.as_str()]); // still marked
    std::fs::write(fx.out.join("go"), "").unwrap();
    assert_eq!(j.join().unwrap(), 0);
    assert_eq!(local.requests_for_session(&sid).unwrap().len(), 1); // the request made after the sync
    assert!(local.sync_pending().unwrap().is_empty());
    s.h.shutdown();
} // mutations: a live session synced (its mark cleared, then exit's insert skips); end_rows with insert_requests

// ---- signals before and after the agent (N70) ----

/// The N70 signal launch in its own process; runs only when `ZYGO_TEST_SIGNAL_ROOT` is set. `ZYGO_TEST_SIGNAL` names
/// the held call: `compact` (with `--compact 600000`) or `requests` (the exit's rows fetch, call bound 5 s). Writes
/// `held` when the server gets that call; after the launch: `sid`, `launcher-exit`, `launcher-stderr`, `agent`
/// (whether the stub ran), `left` (the files under the sessions directory, JSON) and `recs` (`<method> <path>\n` each).
#[cfg(unix)]
#[test]
#[ignore]
fn signal_launch() {
    let Some(root) = std::env::var_os("ZYGO_TEST_SIGNAL_ROOT").map(PathBuf::from) else { return };
    let held = std::env::var("ZYGO_TEST_SIGNAL").unwrap();
    let fx = fx();
    let (sid, grant) = grant_json(&fx);
    let routes = launch_routes(&fx, grant, (200, ROWS_OK.into()));
    let (mark, leaf) = (root.join("held"), format!("/{held}"));
    let (url, rec) = recorder(move |r| {
        if r.path.ends_with(&leaf) {
            std::fs::write(&mark, "").unwrap();
            None
        } else {
            routes(r)
        }
    });
    fx.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let mut o = opts(&fx, "dump", Some(url.to_string()), &[("ZYGO_TEAM_TOKEN", &new_token().0)]);
    if held == "compact" {
        o.compact = Some(WindowArg::Tokens(600_000));
    }
    o.serve_bounds = Some(Bounds { connect: Duration::from_secs(10), call: Duration::from_secs(5) });
    let (code, err) = run(o);
    let w = |n: &str, t: String| std::fs::write(root.join(n), t).unwrap();
    w("sid", sid);
    w("launcher-exit", code.to_string());
    w("launcher-stderr", err);
    w("agent", fx.out.join("env.json").exists().to_string());
    w("left", serde_json::to_string(&files_under(&fx.paths.sessions_dir)).unwrap());
    w("recs", rec.lock().unwrap().iter().map(|r| format!("{} {}\n", r.method, r.path)).collect());
}

/// Starts `signal_launch` with the launcher's signals at their default action (N57 F1); returns once the server holds
/// the call.
#[cfg(unix)]
fn spawn_held(root: &Path, held: &str) -> std::process::Child {
    use std::os::unix::process::CommandExt;
    let tmp = root.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "signal_launch", "--ignored", "--nocapture", "--test-threads=1"])
        .env("ZYGO_TEST_SIGNAL_ROOT", root)
        .env("ZYGO_TEST_SIGNAL", held)
        // The harness's temp dirs (its `fx`, with a session `token`) live under `root`, which this test owns and
        // removes, also when the harness is killed (N-4).
        .env("TMPDIR", &tmp)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(root.join("harness-stderr")).unwrap());
    // SAFETY: between fork and exec the closure calls only `signal`, which is async-signal-safe; this test process is
    // not changed.
    unsafe {
        c.pre_exec(|| {
            for s in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP] {
                libc::signal(s, libc::SIG_DFL);
            }
            Ok(())
        });
    }
    let mut h = c.spawn().unwrap();
    if !until(Duration::from_secs(60), || root.join("held").exists()) {
        let _ = h.kill();
        let _ = h.wait();
        panic!("the server never held {held}; {}", std::fs::read_to_string(root.join("harness-stderr")).unwrap_or_default());
    }
    h
}

#[cfg(unix)]
fn send(h: &std::process::Child, sig: libc::c_int) {
    assert_eq!(unsafe { libc::kill(i32::try_from(h.id()).unwrap(), sig) }, 0);
}

#[cfg(unix)]
fn term(h: &std::process::Child) {
    send(h, libc::SIGTERM);
}

/// The harness's exit status within `bound`; else it is killed and the test fails.
#[cfg(unix)]
fn wait_exit(h: &mut std::process::Child, bound: Duration, root: &Path) -> std::process::ExitStatus {
    let mut st = None;
    if !until(bound, || {
        st = h.try_wait().unwrap();
        st.is_some()
    }) {
        let _ = h.kill();
        let _ = h.wait();
        panic!("the harness did not end; {}", std::fs::read_to_string(root.join("harness-stderr")).unwrap_or_default());
    }
    st.unwrap()
}

#[cfg(unix)]
fn term_while_held(root: &Path, held: &str) -> std::process::ExitStatus {
    let mut h = spawn_held(root, held);
    term(&h);
    wait_exit(&mut h, Duration::from_secs(90), root)
}

#[cfg(unix)]
#[test]
fn sigterm_during_a_slow_compact_runs_the_cleanup() { // N70 S1, §8.2 step 2 note 2026-11-19 (acceptance)
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let st = term_while_held(&root, "compact");
    let r = |n: &str| std::fs::read_to_string(root.join(n)).unwrap_or_default();
    assert!(st.success(), "harness: {st}; {}", r("harness-stderr"));
    assert_eq!(r("launcher-exit"), "143", "{}", r("launcher-stderr"));
    assert_eq!(r("launcher-stderr"), "");
    assert_eq!(r("agent"), "false");
    assert_eq!(r("left"), "{}");
    let sid = r("sid");
    assert_eq!(
        r("recs"),
        format!(
            "GET /v1/zygo/config\nGET /v1/zygo/plugins\nPOST /v1/zygo/sessions\nPOST /v1/zygo/sessions/{sid}/compact\n\
             GET /v1/zygo/sessions/{sid}/requests\nDELETE /v1/zygo/sessions/{sid}\n"
        )
    );
} // mutations: no handling before the spawn (the harness ends by SIGTERM); no check after the compact call (the
  // compact error is printed and the exit is 12)

#[cfg(unix)]
#[test]
fn sigterm_during_the_exit_finishes_the_cleanup() { // N70 S1, §8.2 step 6
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let st = term_while_held(&root, "requests");
    let r = |n: &str| std::fs::read_to_string(root.join(n)).unwrap_or_default();
    assert!(st.success(), "harness: {st}; {}", r("harness-stderr"));
    assert_eq!(r("launcher-exit"), "0", "{}", r("launcher-stderr"));
    assert_eq!(r("agent"), "true");
    assert_eq!(r("left"), "{}");
    let err = r("launcher-stderr");
    assert!(err.starts_with("zygo: usage rows not fetched (zygo proxy unreachable: "), "{err}");
    assert!(err.ends_with("); zygo usage --sync retries\n"), "{err}");
    let sid = r("sid");
    // S3: no DELETE after an unreachable rows fetch.
    assert_eq!(r("recs"), format!("GET /v1/zygo/config\nGET /v1/zygo/plugins\nPOST /v1/zygo/sessions\nGET /v1/zygo/sessions/{sid}/requests\n"));
} // mutations: the handling dropped before `finish` (the harness ends by SIGTERM); a step-6 signal that changes the code

#[cfg(unix)]
#[test]
fn a_second_sigint_before_the_agent_ends_the_launch_at_once() { // N70 M-2 (D5), round 2 M1
    use std::os::unix::process::ExitStatusExt;
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let mut h = spawn_held(&root, "compact");
    send(&h, libc::SIGINT);
    std::thread::sleep(Duration::from_secs(1));
    let early = h.try_wait().unwrap();
    assert!(early.is_none(), "the first SIGINT ended the launch: {early:?}");
    send(&h, libc::SIGINT);
    // Within 5 s, before the 10 s compact bound: the second signal, not the call's end, stopped it.
    let st = wait_exit(&mut h, Duration::from_secs(5), &root);
    assert_eq!(st.signal(), Some(libc::SIGINT), "{st}");
} // mutation: a second SIGINT only recorded (the launch ends at the compact bound with exit 130)

#[cfg(unix)]
#[test]
fn two_sighups_from_a_hangup_still_run_the_cleanup() { // N70 round 2 M1, §8.2 step 2 note 2026-11-19
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let mut h = spawn_held(&root, "compact");
    // A terminal hangup sends SIGHUP twice.
    send(&h, libc::SIGHUP);
    std::thread::sleep(Duration::from_millis(50));
    send(&h, libc::SIGHUP);
    let st = wait_exit(&mut h, Duration::from_secs(90), &root);
    let r = |n: &str| std::fs::read_to_string(root.join(n)).unwrap_or_default();
    assert!(st.success(), "harness: {st}; {}", r("harness-stderr"));
    assert_eq!(r("launcher-exit"), "129", "{}", r("launcher-stderr"));
    assert_eq!(r("launcher-stderr"), "");
    assert_eq!(r("agent"), "false");
    assert_eq!(r("left"), "{}");
    let sid = r("sid");
    assert_eq!(
        r("recs"),
        format!(
            "GET /v1/zygo/config\nGET /v1/zygo/plugins\nPOST /v1/zygo/sessions\nPOST /v1/zygo/sessions/{sid}/compact\n\
             GET /v1/zygo/sessions/{sid}/requests\nDELETE /v1/zygo/sessions/{sid}\n"
        )
    );
} // mutation: a second SIGHUP escalates (the harness ends by SIGHUP, session files left)
