//! N16: serve mode server (§7.1, §7.2, §7.3, C8, §14, §15.3, §20 #13, #26). Loopback only; TLS certificates come
//! from rcgen at run time; team tokens from `team::new_token()`. No test changes the process environment.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::response::Response;
use http::HeaderMap;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zygo_config::{check, load, Approval, Dialect, LoadContext, Marketplace, Merged, ServeToken, Severity};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_proxy::client::SessionStatus;
use zygo_proxy::serve::{
    self, AlreadyRunning, ConfigInvalid, NoServer, ServeEvent, ServeHandle, ServeOpts, TlsFiles, TlsRequired, BODY_READ_IDLE, HEADER_READ,
    MAX_CONNECTIONS,
};
use zygo_proxy::server::{StartOpts, UPSTREAM_CONNECT, UPSTREAM_READ_IDLE};
use zygo_proxy::session::{ManualClock, SessionGrant, TeamAuth, SUSPEND_AFTER};
use zygo_proxy::team::{append_token, known_plugins, new_token, PluginsReply, TeamConfigReply, Tokens};
use zygo_proxy::tee::{UsageHold, ROWS_WAIT};
use zygo_usage::{RedactionCount, RequestRow, Store};

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"), sessions_dir: p.join("cache/sessions"),
            marketplaces_dir: p.join("cache/marketplaces"), plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"),
            usage_db: p.join("data/usage.db") }
}
fn ctx(p: &Path) -> LoadContext { LoadContext { paths: paths(p), repo: None, team: None } }
fn load_in(p: &Path) -> Merged { load(&ctx(p)).unwrap() }
fn hex256(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }
fn labels(m: &Merged) -> Vec<(String, String)> { m.config.serve.tokens.iter().map(|t| (t.label.clone(), t.sha256.clone())).collect() }

const BASE: &str = r#"
[serve]
tokens = [{ label = "alice", sha256 = "{ALICE}" }, { label = "bob", sha256 = "{BOB}" }]

[role.code]
model = "test-code"

[[model]]
id = "test-code"
soft_context_window = 150000
[model.facts]
context_window = 200000
max_output_tokens = 8192
[[model.provider]]
name = "p-code"
base_url = "{URL}"
dialects = ["anthropic"]
upstream_model = "up-code"

[[model]]
id = "test-alt"
[[model.provider]]
name = "p-alt"
base_url = "{URL}"
dialects = ["anthropic"]
upstream_model = "up-alt"

[[model]]
id = "test-hidden"
[[model.provider]]
name = "p-sentinel-name"
base_url = "{URL}/sentinel-path"
dialects = ["openai"]
upstream_model = "up-sentinel-model"
key = "env:ZYGO_SENTINEL_KEY_NAME"
price_usd_per_mtok = { input = 1234.5, output = 1234.5, cache_read = 1234.5, cache_write = 1234.5 }
flags = { zdr = true, flag_sentinel = "flag-sentinel-value" }
forward_headers = ["x-sentinel-header"]

[[marketplace]]
name = "team"
source = "github:example/agent-plugins"
ref = "v1.4.0"
commit = "{COMMIT}"
default_on = ["fmt"]
required = ["lint"]
approvals = [{ plugin = "fmt", digest = "{DIGEST}" }]

[plugins]
enable = ["team:extra"]

[profile.teamp.plugins]
"team:prof" = true
"#;

fn commit() -> String { "4f2c".repeat(10) }
fn digest() -> String { "ab12".repeat(16) }
/// `BASE` with the placeholders filled.
fn base(alice_hash: &str, bob_hash: &str, url: &str) -> String {
    BASE.replace("{ALICE}", alice_hash).replace("{BOB}", bob_hash).replace("{URL}", url)
        .replace("{COMMIT}", &commit()).replace("{DIGEST}", &digest())
}

// ---- team tokens and known plugins ----

#[test]
fn token_new_form() {
    let (t, h) = new_token();
    assert_eq!(t.len(), 49);
    assert!(t.starts_with("zyg_t_"));
    assert!(t[6..].bytes().all(|b| b.is_ascii_alphanumeric()));
    assert_eq!(h, hex256(&t));
    assert_eq!(h.len(), 64);
    assert_ne!(new_token().0, t);
}

#[test]
fn token_append_keeps_inline_form() {
    let d = TempDir::new().unwrap();
    let f = d.path().join("config.toml");
    let (h1, h2) = (new_token().1, new_token().1);
    std::fs::write(&f, format!("# team server\n[serve]\ntokens = [{{ label = \"a\", sha256 = \"{h1}\" }}]\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    append_token(&f, "b", &h2).unwrap();
    assert!(std::fs::read_to_string(&f).unwrap().starts_with("# team server\n"));
    assert_eq!(labels(&load_in(d.path())), vec![("a".into(), h1), ("b".into(), h2)]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn token_append_creates_file() {
    let d = TempDir::new().unwrap();
    let f = d.path().join("x/config.toml");
    let h = new_token().1;
    append_token(&f, "a", &h).unwrap();
    assert_eq!(labels(&load_in(&d.path().join("x"))), vec![("a".into(), h)]);
}

/// F5 (round 2): the temp file is created 0600 (a new config keeps that mode); an existing file keeps its own mode.
#[cfg(unix)]
#[test]
fn token_append_temp_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |f: &Path| std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
    let d = TempDir::new().unwrap();
    let f = d.path().join("config.toml");
    append_token(&f, "a", &new_token().1).unwrap();
    assert_eq!(mode(&f), 0o600, "new config file wider than 0600");
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
    append_token(&f, "b", &new_token().1).unwrap();
    assert_eq!(mode(&f), 0o640);
    let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n.to_string_lossy().ends_with(".tmp")).collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn token_append_refuses_existing_label() {
    let d = TempDir::new().unwrap();
    let f = d.path().join("config.toml");
    append_token(&f, "a", &new_token().1).unwrap();
    let before = std::fs::read(&f).unwrap();
    let e = append_token(&f, "a", &new_token().1).unwrap_err();
    assert_eq!(e.to_string(), "label a exists; remove it from the config first");
    assert_eq!(std::fs::read(&f).unwrap(), before);
}

#[test]
fn team_tokens_label_for() {
    let (t, h) = new_token();
    let tokens = Tokens::new(&[ServeToken { label: "alice".into(), sha256: h.to_uppercase() }]);
    assert_eq!(tokens.label_for(&t).as_deref(), Some("alice"));
    assert_eq!(tokens.label_for(&new_token().0), None);
}

#[test]
fn team_known_plugins_formula() {
    let d = TempDir::new().unwrap();
    std::fs::write(d.path().join("config.toml"), base(&new_token().1, &new_token().1, "http://127.0.0.1:9")).unwrap();
    let k = known_plugins(&load_in(d.path()).config);
    for p in ["team:fmt", "team:lint", "team:extra", "team:prof"] {
        assert!(k.contains(p), "{p}");
    }
    assert!(!k.contains("team:other"));
}

// ---- server fixtures ----

type Got = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

struct Upstream { addr: SocketAddr, got: Got, _stop: tokio::sync::oneshot::Sender<()> }
impl Upstream {
    /// 127.0.0.1:0, own runtime thread; every request is recorded (path with query, body) and answered 200 JSON.
    fn start() -> Upstream {
        let got: Got = Arc::default();
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let g2 = got.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let l = tokio::net::TcpListener::from_std(l).unwrap();
                let app = axum::Router::new().fallback(move |req: Request| {
                    let g3 = g2.clone();
                    async move {
                        let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
                        let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
                        g3.lock().unwrap().push((path, body));
                        Response::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("openai-organization", "org-test")
                            .header("openai-project", "proj-test")
                            .header("anthropic-organization-id", "org-id-test")
                            .body(axum::body::Body::from(
                                r#"{"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#,
                            ))
                            .unwrap()
                    }
                });
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, got, _stop: tx }
    }
    fn url(&self) -> String { format!("http://127.0.0.1:{}", self.addr.port()) }
    fn bodies(&self) -> Vec<Vec<u8>> { self.got.lock().unwrap().iter().map(|g| g.1.clone()).collect() }
}

/// A self-signed certificate for 127.0.0.1, made at run time.
struct Tls { _d: TempDir, files: TlsFiles, pem: String }
fn tls() -> Tls {
    let ck = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let d = TempDir::new().unwrap();
    let (cert, key) = (d.path().join("cert.pem"), d.path().join("key.pem"));
    let pem = ck.cert.pem();
    std::fs::write(&cert, &pem).unwrap();
    std::fs::write(&key, ck.signing_key.serialize_pem()).unwrap();
    Tls { _d: d, files: TlsFiles { cert, key }, pem }
}
fn tls_files(t: &Tls) -> TlsFiles { TlsFiles { cert: t.files.cert.clone(), key: t.files.key.clone() } }
/// Trusts only `t`'s certificate (`None`: plain http).
fn client(t: Option<&Tls>) -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // On Windows a connect to a port just closed (a test after `shutdown`) can stay in SYN-SENT for minutes instead of
    // being refused; the bound turns that into the error the test expects.
    let mut b = reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(5));
    if let Some(t) = t {
        b = b.tls_certs_only([reqwest::Certificate::from_pem(t.pem.as_bytes()).unwrap()]);
    }
    b.build().unwrap()
}

type Lines = Arc<Mutex<Vec<String>>>;

struct Srv { d: TempDir, up: Upstream, h: ServeHandle, alice: String, bob: String, lines: Lines, clock: Arc<ManualClock>, tls: Option<Tls> }

fn cfg_path(d: &Path) -> PathBuf { d.join("config.toml") }
/// `BASE` with new tokens for alice and bob and the upstream URL; returns the tokens.
fn write_base(d: &Path, up: &Upstream) -> (String, String) {
    let (alice, ah) = new_token();
    let (bob, bh) = new_token();
    std::fs::write(cfg_path(d), base(&ah, &bh, &up.url())).unwrap();
    (alice, bob)
}
fn opts(d: &Path, listen: &str, tls: Option<TlsFiles>, lines: &Lines, clock: Arc<ManualClock>, pid: bool, signals: bool) -> ServeOpts {
    let c = ctx(d);
    let l = lines.clone();
    ServeOpts {
        listen: listen.into(),
        tls,
        config_file: cfg_path(d),
        load: Arc::new(move || Ok(load(&c)?)),
        report: Arc::new(move |s: &str| l.lock().unwrap().push(s.to_string())),
        pid_file: pid.then(|| d.join("data/serve.pid")),
        signals,
        start: StartOpts { clock, usage_db: Some(d.join("data/usage.db")), ..Default::default() },
    }
}
fn setup(tls_on: bool, pid: bool, signals: bool) -> Srv {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    let (alice, bob) = write_base(d.path(), &up);
    let t = tls_on.then(tls);
    let lines = Lines::default();
    let clock = ManualClock::new();
    let h = serve::start(opts(d.path(), "127.0.0.1:0", t.as_ref().map(tls_files), &lines, clock.clone(), pid, signals)).unwrap();
    Srv { d, up, h, alice, bob, lines, clock, tls: t }
}
/// As `setup(false, false, false)`, but the config file is `edit(<BASE filled>)` and `opts` adjusts the `StartOpts`.
fn setup_with(edit: impl Fn(String) -> String, opts_edit: impl FnOnce(&mut StartOpts)) -> Srv {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    let (alice, bob) = write_base(d.path(), &up);
    let p = cfg_path(d.path());
    std::fs::write(&p, edit(std::fs::read_to_string(&p).unwrap())).unwrap();
    let lines = Lines::default();
    let clock = ManualClock::new();
    let mut o = opts(d.path(), "127.0.0.1:0", None, &lines, clock.clone(), false, false);
    opts_edit(&mut o.start);
    let h = serve::start(o).unwrap();
    Srv { d, up, h, alice, bob, lines, clock, tls: None }
}
/// Replaces the config file with `edit(<current text>)`.
fn rewrite(s: &Srv, edit: impl Fn(String) -> String) {
    let p = cfg_path(s.d.path());
    std::fs::write(&p, edit(std::fs::read_to_string(&p).unwrap())).unwrap();
}
fn lines(l: &Lines) -> Vec<String> { l.lock().unwrap().clone() }

/// (status, headers, body), or the transport error text.
fn request(c: &reqwest::Client, url: reqwest::Url, method: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> Result<(u16, HeaderMap, Vec<u8>), String> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut rb = c.request(m, url);
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.header("content-type", "application/json").body(b);
        }
        let r = rb.send().await.map_err(|e| e.to_string())?;
        let st = r.status().as_u16();
        let hd = r.headers().clone();
        Ok((st, hd, r.bytes().await.map_err(|e| e.to_string())?.to_vec()))
    })
}
#[allow(dead_code)]
fn send(s: &Srv, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> (u16, HeaderMap, Vec<u8>) {
    request(&client(s.tls.as_ref()), s.h.url().join(path).unwrap(), method, headers, body).unwrap()
}
fn healthz(h: &ServeHandle, t: Option<&Tls>) -> Result<(u16, Vec<u8>), String> {
    request(&client(t), h.url().join("healthz").unwrap(), "GET", &[], None).map(|r| (r.0, r.2))
}

// ---- start, C8, TLS, pid file ----

#[test]
fn c8_non_loopback_without_tls_refused() {
    let d = TempDir::new().unwrap();
    for listen in ["0.0.0.0:0", "10.0.0.5:8443", "[::]:0", "[::ffff:10.0.0.5]:0"] {
        let mut o = opts(d.path(), listen, None, &Lines::default(), ManualClock::new(), false, false);
        o.load = Arc::new(|| panic!("loaded before the C8 check"));
        let e = serve::start(o).err().unwrap_or_else(|| panic!("{listen} served"));
        assert!(e.downcast_ref::<TlsRequired>().is_some(), "{listen}: {e}");
        assert_eq!(e.to_string(), TlsRequired.to_string());
    }
}

#[test]
fn c8_loopback_without_tls_serves_http() {
    for listen in ["127.0.0.1:0", "localhost:0", "[::1]:0"] {
        let d = TempDir::new().unwrap();
        let up = Upstream::start();
        write_base(d.path(), &up);
        let ls = Lines::default();
        let h = serve::start(opts(d.path(), listen, None, &ls, ManualClock::new(), false, false)).unwrap();
        assert_eq!(h.url().scheme(), "http");
        assert_eq!(healthz(&h, None).unwrap(), (200, b"ok".to_vec()));
        assert_eq!(lines(&ls)[0], format!("zygo serve: listening on {}", h.url()));
        h.shutdown();
    }
}

#[test]
fn c8_tls_serves_https() {
    let s = setup(true, false, false);
    assert_eq!(s.h.url().scheme(), "https");
    assert_eq!(healthz(&s.h, s.tls.as_ref()).unwrap().0, 200);
    let mut plain = s.h.url();
    plain.set_scheme("http").unwrap();
    let r = request(&client(None), plain.join("healthz").unwrap(), "GET", &[], None);
    assert!(!matches!(r, Ok((200, ..))), "{r:?}");
    s.h.shutdown();
}

#[test]
fn c8_unreadable_tls_files_fail() {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    write_base(d.path(), &up);
    let t = tls();
    let files = TlsFiles { cert: d.path().join("missing.pem"), key: t.files.key.clone() };
    let e = serve::start(opts(d.path(), "127.0.0.1:0", Some(files), &Lines::default(), ManualClock::new(), false, false)).err().unwrap();
    assert!(e.downcast_ref::<TlsRequired>().is_none(), "{e}");
}

#[test]
fn start_prunes_old_rows() {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    write_base(d.path(), &up);
    let db = d.path().join("data/usage.db");
    Store::open(&db).unwrap().insert_request(&RequestRow { ts: 0, session: "old-session".into(), ..Default::default() }).unwrap();
    let h = serve::start(opts(d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), false, false)).unwrap();
    assert!(Store::open(&db).unwrap().requests_for_session("old-session").unwrap().is_empty());
    h.shutdown();
}

#[test]
fn start_check_error_refused() {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    write_base(d.path(), &up);
    let p = cfg_path(d.path());
    let want = format!("config invalid ({}); run zygo config check", p.display());
    let text = std::fs::read_to_string(&p).unwrap().replace("model = \"test-code\"", "model = \"no-such-model\"");
    std::fs::write(&p, text).unwrap();
    let e = serve::start(opts(d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), false, false)).err().unwrap();
    assert!(e.downcast_ref::<ConfigInvalid>().is_some(), "{e}");
    assert_eq!(e.to_string(), want);
    std::fs::write(&p, "[[ not toml").unwrap();
    let e = serve::start(opts(d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), false, false)).err().unwrap();
    assert!(e.downcast_ref::<ConfigInvalid>().is_some(), "{e}");
    assert_eq!(e.to_string(), want);
    assert!(!format!("{e:#}").contains("not toml"));
}

#[test]
fn pid_second_instance_refused() {
    let a = setup(false, true, false);
    let pid_file = a.d.path().join("data/serve.pid");
    let mine = format!("{}\n", std::process::id());
    // Windows locks are mandatory: the running server's pid file cannot be read (os error 33).
    if cfg!(unix) { assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), mine); }
    let o = opts(a.d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), true, false);
    let e = serve::start(o).err().unwrap();
    assert!(e.downcast_ref::<AlreadyRunning>().is_some(), "{e}");
    assert_eq!(e.to_string(), "zygo serve is already running");
    if cfg!(unix) { assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), mine); }
    a.h.shutdown();
    assert!(!pid_file.exists());
}

#[test]
fn pid_stale_file_no_signal() {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    write_base(d.path(), &up);
    let pid_file = d.path().join("data/serve.pid");
    std::fs::create_dir_all(pid_file.parent().unwrap()).unwrap();
    std::fs::write(&pid_file, format!("{}\n", std::process::id())).unwrap();
    // A SIGHUP to this process would end the test binary.
    let e = serve::reload_running(&pid_file).unwrap_err();
    assert!(e.downcast_ref::<NoServer>().is_some(), "{e}");
    assert_eq!(e.to_string(), "no running zygo serve");
    let h = serve::start(opts(d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), true, false)).unwrap();
    // Windows locks are mandatory: the running server's pid file cannot be read (os error 33).
    if cfg!(unix) { assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), format!("{}\n", std::process::id())); }
    #[cfg(unix)]
    {
        std::fs::write(&pid_file, "1\n").unwrap();
        h.shutdown();
        assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "1\n");
    }
    #[cfg(not(unix))]
    h.shutdown();
}

/// F3 (round 2): a refused second instance changes nothing; the running server's usage rows stay.
#[test]
fn pid_second_instance_prunes_nothing() {
    let a = setup(false, true, false);
    let db = a.d.path().join("data/usage.db");
    let old = RequestRow { ts: 1_000, session: "old".into(), ..Default::default() };
    Store::open(&db).unwrap().insert_request(&old).unwrap();
    let o = opts(a.d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), true, false);
    let e = serve::start(o).err().unwrap();
    assert!(e.downcast_ref::<AlreadyRunning>().is_some());
    let rows = Store::open(&db).unwrap().requests_for_session("old").unwrap();
    a.h.shutdown();
    assert_eq!(rows.len(), 1, "refused second instance pruned the running server's rows");
}

/// F2 (round 2): a symlink at the pid path is refused; the error names the path; the link target is unchanged.
#[cfg(unix)]
#[test]
fn pid_symlink_refused() {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    write_base(d.path(), &up);
    let victim = d.path().join("victim.txt");
    std::fs::write(&victim, "precious\n").unwrap();
    std::fs::create_dir_all(d.path().join("data")).unwrap();
    let pid_file = d.path().join("data/serve.pid");
    std::os::unix::fs::symlink(&victim, &pid_file).unwrap();
    let r = serve::start(opts(d.path(), "127.0.0.1:0", None, &Lines::default(), ManualClock::new(), true, false));
    let got = std::fs::read_to_string(&victim).unwrap();
    let e = match r {
        Ok(h) => {
            h.shutdown();
            panic!("started with a symlinked pid file; target now {got:?}");
        }
        Err(e) => e,
    };
    assert_eq!(got, "precious\n");
    assert!(format!("{e:#}").contains(&pid_file.display().to_string()), "{e:#}");
    assert!(e.downcast_ref::<AlreadyRunning>().is_none());
}

// ---- team routes ----

fn bearer(t: &str) -> String { format!("Bearer {t}") }
/// (status, error.type); asserts the Anthropic error shape.
fn err(r: &(u16, HeaderMap, Vec<u8>)) -> (u16, String) {
    let v: Value = serde_json::from_slice(&r.2).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&r.2)));
    assert_eq!(v["type"], "error");
    assert!(v["error"]["message"].is_string());
    (r.0, v["error"]["type"].as_str().unwrap().to_string())
}
fn err_msg(r: &(u16, HeaderMap, Vec<u8>)) -> String {
    serde_json::from_slice::<Value>(&r.2).unwrap()["error"]["message"].as_str().unwrap().to_string()
}
fn json_of<T: serde::de::DeserializeOwned>(r: &(u16, HeaderMap, Vec<u8>)) -> T {
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    serde_json::from_slice(&r.2).unwrap()
}
fn team_get(s: &Srv, path: &str, token: &str) -> (u16, HeaderMap, Vec<u8>) {
    let b = bearer(token);
    send(s, "GET", path, &[("authorization", b.as_str())], None)
}
fn session_body(passthrough: bool, profile: Option<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({"agent":"claude-code","agent_version":"2.1.328","profile":profile,"passthrough":passthrough,"repo":null,"work":null})).unwrap()
}
fn create(s: &Srv, token: &str, body: Vec<u8>) -> (u16, HeaderMap, Vec<u8>) {
    let b = bearer(token);
    send(s, "POST", "v1/zygo/sessions", &[("authorization", b.as_str())], Some(body))
}
fn grant_with(s: &Srv, token: &str, profile: Option<Value>) -> SessionGrant { json_of(&create(s, token, session_body(false, profile))) }
fn grant(s: &Srv, token: &str) -> SessionGrant { grant_with(s, token, None) }
/// A model request on `g` for `model` with `content` as the user text.
fn msg_with(s: &Srv, g: &SessionGrant, model: &str, content: Value) -> (u16, HeaderMap, Vec<u8>) {
    let b = serde_json::to_vec(&json!({"model": model, "max_tokens": 64, "messages": [{"role": "user", "content": content}]})).unwrap();
    send(s, "POST", "anthropic/v1/messages", &[("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())], Some(b))
}
fn msg(s: &Srv, g: &SessionGrant, model: &str) -> (u16, HeaderMap, Vec<u8>) { msg_with(s, g, model, json!("hi")) }
fn hdr(r: &(u16, HeaderMap, Vec<u8>), name: &str) -> String {
    r.1.get(name).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}
fn requests(s: &Srv, id: &str, token: &str) -> (u16, HeaderMap, Vec<u8>) { team_get(s, &format!("v1/zygo/sessions/{id}/requests"), token) }
fn keys_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => o.iter().for_each(|(k, x)| {
            out.push(k.clone());
            keys_of(x, out);
        }),
        Value::Array(a) => a.iter().for_each(|x| keys_of(x, out)),
        _ => {}
    }
}

#[test]
fn team_bad_or_missing_auth_401() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    let unknown = bearer(&new_token().0);
    let session = bearer(&g.token);
    for h in [vec![], vec![("authorization", unknown.as_str())], vec![("authorization", session.as_str())]] {
        assert_eq!(err(&send(&s, "GET", "v1/zygo/config", &h, None)), (401, "zygo_token_invalid".into()));
    }
    assert_eq!(err(&create(&s, &new_token().0, session_body(false, None))), (401, "zygo_token_invalid".into()));
    assert_eq!(team_get(&s, "v1/zygo/config", &s.alice).0, 200);
    s.h.shutdown();
}

#[test]
fn config_reply_has_no_provider_data() {
    let s = setup(true, false, false);
    let r = team_get(&s, "v1/zygo/config", &s.alice);
    assert_eq!(r.0, 200);
    let v: Value = serde_json::from_slice(&r.2).unwrap();
    let mut keys = Vec::new();
    keys_of(&v, &mut keys);
    for k in ["provider", "base_url", "upstream_model", "key", "price_usd_per_mtok", "flags", "forward_headers", "zdr", "serve", "tokens", "sha256", "proxy", "server"] {
        assert!(!keys.iter().any(|x| x == k), "key {k}");
    }
    let text = String::from_utf8(r.2.clone()).unwrap();
    for x in ["p-sentinel-name", "sentinel-path", "up-sentinel-model", "ZYGO_SENTINEL_KEY_NAME", "1234.5", "flag-sentinel-value", "x-sentinel-header", &hex256(&s.alice)] {
        assert!(!text.contains(x), "{x}");
    }
    s.h.shutdown();
}

#[test]
fn config_reply_fields() {
    let s = setup(true, false, false);
    let c: TeamConfigReply = json_of(&team_get(&s, "v1/zygo/config", &s.alice));
    // Owner decision at N37 planning: the reply sends only keys the server's config file names (no `main_role`).
    assert_eq!(c.team["role"]["code"]["model"].as_str(), Some("test-code"));
    let code = c.model.iter().find(|m| m.id == "test-code").unwrap();
    assert_eq!(code.dialects, vec![Dialect::Anthropic]);
    assert_eq!(code.soft_context_window, Some(150000));
    assert_eq!(code.facts.as_ref().unwrap().context_window, 200000);
    assert!(c.model.iter().find(|m| m.id == "test-hidden").unwrap().dialects.contains(&Dialect::Anthropic));
    let names: Vec<&str> = c.team["marketplace"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["team"]);
    s.h.shutdown();
}

#[test]
fn config_reply_loads_as_team_layer() {
    let s = setup(true, false, false);
    let c: TeamConfigReply = json_of(&team_get(&s, "v1/zygo/config", &s.alice));
    let t = toml::Table::try_from(&c).unwrap();
    let fresh = TempDir::new().unwrap();
    let m = load(&LoadContext { paths: paths(fresh.path()), repo: None, team: Some(t) }).unwrap();
    assert!(m.config.catalog.get("test-code").unwrap().provider.is_empty());
    let bad: Vec<_> = check(&m.config)
        .into_iter()
        .filter(|v| v.severity == Severity::Error && (v.path.starts_with("model[") || v.path.starts_with("role.")))
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
    s.h.shutdown();
}

#[test]
fn plugins_reply_lists_marketplaces() {
    let s = setup(true, false, false);
    let p: PluginsReply = json_of(&team_get(&s, "v1/zygo/plugins", &s.alice));
    // Owner decision at N37 planning: only the marketplaces the server's config file names.
    assert_eq!(p.marketplace.len(), 1);
    let team = p.marketplace.iter().find(|m| m.name == "team").unwrap();
    assert_eq!(
        *team,
        Marketplace {
            name: "team".into(),
            source: "github:example/agent-plugins".into(),
            git_ref: "v1.4.0".into(),
            commit: Some(commit()),
            default_on: vec!["fmt".into()],
            required: vec!["lint".into()],
            approvals: vec![Approval { plugin: "fmt".into(), digest: digest() }],
        }
    );
    assert_eq!(err(&send(&s, "GET", "v1/zygo/plugins", &[], None)), (401, "zygo_token_invalid".into()));
    s.h.shutdown();
}

#[test]
fn config_reply_holds_only_file_keys() { // §6.1 layer 7 owner note, §7.2
    let s = setup_with(|b| b + "[guards]\nclaims = \"enforce\"\n", |_| {});
    let r = team_get(&s, "v1/zygo/config", &s.alice);
    let v: Value = json_of(&r);
    assert_eq!(v["guards"], json!({"claims": "enforce"}));
    for k in ["results", "features", "limits", "navigation", "redaction", "main_role", "serve", "proxy"] {
        assert!(v.get(k).is_none(), "{k}");
    }
    assert_eq!(v["role"], json!({"code": {"model": "test-code"}}));
    let names: Vec<&str> = v["marketplace"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["team"]); // the built-in act101 is a default, not sent
    assert!(v["model"].as_array().unwrap().iter().any(|m| m["id"] == "test-code"));
    let fresh = TempDir::new().unwrap();
    let t = toml::Table::try_from(json_of::<TeamConfigReply>(&r)).unwrap();
    let m = load(&LoadContext { paths: paths(fresh.path()), repo: None, team: Some(t) }).unwrap();
    assert!(m.team_keys.contains("guards.claims"));
    assert!(!m.team_keys.iter().any(|k| k.starts_with("features.") || k.starts_with("results.")));
    s.h.shutdown();
} // mutations: the full typed config sent; defaults included; act101 sent

#[test]
fn config_reply_drops_keys_the_schema_ignores() { // B3 probe
    let s = setup_with(
        |b| {
            b.replace(
                "[role.code]\nmodel = \"test-code\"\n",
                "[role.code]\nmodel = \"test-code\"\nkey = \"env:SECRET_NAME\"\n[role.code.dialect_models]\nopenai = \"test-alt\"\n[role.code.provider]\nbase_url = \"https://internal.example\"\n",
            ) + "[profile.x.features]\nresults = false\nkey = \"env:SECRET_NAME\"\n[profile.empty]\n"
        },
        |_| {},
    );
    let r = team_get(&s, "v1/zygo/config", &s.alice);
    let text = String::from_utf8(r.2.clone()).unwrap();
    for x in ["SECRET_NAME", "internal.example", "\"key\"", "\"provider\"", "base_url"] {
        assert!(!text.contains(x), "{x}");
    }
    let t = toml::Table::try_from(json_of::<TeamConfigReply>(&r)).unwrap();
    let fresh = TempDir::new().unwrap();
    let m = load(&LoadContext { paths: paths(fresh.path()), repo: None, team: Some(t.clone()) }).unwrap();
    assert!(!m.team_keys.iter().any(|k| k.contains("provider") || k.ends_with(".key")), "{:?}", m.team_keys);
    assert_eq!(t["role"]["code"]["model"].as_str(), Some("test-code"));
    assert!(t["role"]["code"].get("dialect_models").is_none());
    assert_eq!(t["profile"]["x"]["features"], toml::Value::Table(toml::toml! { results = false }));
    assert_eq!(t["profile"]["empty"], toml::Value::Table(toml::Table::new()));
    s.h.shutdown();
} // mutations: raw file values sent; free-form profile features passed whole; dialect_models sent; an empty preset dropped

#[test]
fn create_passthrough_refused_403() {
    let s = setup(true, false, false);
    let r = create(&s, &s.alice, session_body(true, None));
    assert_eq!(err(&r), (403, "zygo_passthrough_local_only".into()));
    assert_eq!(err_msg(&r), "passthrough is local mode only");
    assert_eq!(create(&s, &s.alice, session_body(false, None)).0, 200);
    s.h.shutdown();
}

#[test]
fn create_validates_profile_plugins() {
    let s = setup(true, false, false);
    let g = grant_with(&s, &s.alice, Some(json!({"name": "x", "plugins": {"team:prof": true}})));
    assert_eq!(g.profile_warning, None);
    let g = grant_with(&s, &s.alice, Some(json!({"name": "x", "plugins": {"team:other": true}})));
    assert!(g.profile_warning.is_some());
    s.h.shutdown();
}

#[test]
fn requests_own_label_rows() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    assert_eq!(msg(&s, &g, "code").0, 200);
    assert!(s.h.usage_written(1, std::time::Duration::from_secs(10)));
    let r = requests(&s, &g.session, &s.alice);
    assert!(!String::from_utf8_lossy(&r.2).contains("p-code"));
    let v: Value = json_of(&r);
    let rows: Vec<RequestRow> = serde_json::from_value(v["requests"].clone()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].token_label.as_deref(), Some("alice"));
    assert_eq!(rows[0].session, g.session);
    assert_eq!(rows[0].role_served.as_deref(), Some("code"));
    assert_eq!(rows[0].provider, "");
    assert_eq!(err(&requests(&s, &g.session, &s.bob)), (401, "zygo_session_unknown".into()));
    assert_eq!(err(&requests(&s, &g.session, &g.token)), (401, "zygo_token_invalid".into()));
    s.h.shutdown();
}

#[test]
fn requests_after_delete_and_unknown_id() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    assert_eq!(msg(&s, &g, "code").0, 200);
    assert!(s.h.usage_written(1, std::time::Duration::from_secs(10)));
    let b = bearer(&s.alice);
    assert_eq!(send(&s, "DELETE", &format!("v1/zygo/sessions/{}", g.session), &[("authorization", b.as_str())], None).0, 204);
    let v: Value = json_of(&requests(&s, &g.session, &s.alice));
    assert_eq!(v["requests"].as_array().unwrap().len(), 1);
    assert_eq!(err(&requests(&s, &ulid(), &s.alice)), (401, "zygo_session_unknown".into()));
    let fresh = grant(&s, &s.alice);
    let v: Value = json_of(&requests(&s, &fresh.session, &s.alice));
    assert_eq!(v, json!({"requests": [], "complete": true})); // N37 Q2: the reply says whether it is complete
    s.h.shutdown();
}

/// A session's request is answered; then a thread GETs its rows into a channel.
fn rows_after_one_request(s: &Srv) -> (String, std::sync::mpsc::Receiver<(u16, HeaderMap, Vec<u8>)>) {
    let g = grant(s, &s.alice);
    assert_eq!(msg(s, &g, "code").0, 200);
    let (tx, rx) = std::sync::mpsc::channel();
    let (url, alice, id) = (s.h.url(), s.alice.clone(), g.session.clone());
    std::thread::spawn(move || {
        let b = bearer(&alice);
        let r = request(&client(None), url.join(&format!("v1/zygo/sessions/{id}/requests")).unwrap(), "GET", &[("authorization", b.as_str())], None);
        let _ = tx.send(r.unwrap());
    });
    (g.session, rx)
}
fn rows_reply(r: &(u16, HeaderMap, Vec<u8>)) -> (u16, usize, Option<bool>) {
    let v: Value = json_of(r);
    (r.0, v["requests"].as_array().unwrap().len(), v["complete"].as_bool())
}

#[test]
fn requests_reply_waits_for_the_session_rows() { // §14, Q2
    let hold = UsageHold::new();
    let h2 = hold.clone();
    let s = setup_with(|b| b, |o| o.usage_hold = Some(h2));
    let (_, rx) = rows_after_one_request(&s);
    std::thread::sleep(Duration::from_millis(200));
    assert!(rx.try_recv().is_err()); // the reply waits for the paused writer
    hold.release();
    let r = rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(rows_reply(&r), (200, 1, Some(true)));
    s.h.shutdown();
} // mutation: no wait → the GET returns at once with 0 rows

#[test]
fn requests_reply_ends_at_the_rows_bound_incomplete() { // §7 note: 25 s; B2
    let hold = UsageHold::new();
    let h2 = hold.clone();
    let s = setup_with(|b| b, |o| {
        o.usage_hold = Some(h2);
        o.rows_wait = Duration::from_millis(300);
    });
    let (id, rx) = rows_after_one_request(&s);
    let r = rx.recv_timeout(Duration::from_secs(10)).unwrap(); // ends at the bound, the writer still paused
    assert_eq!(rows_reply(&r), (200, 0, Some(false)));
    hold.release();
    assert_eq!(rows_reply(&requests(&s, &id, &s.alice)), (200, 1, Some(true)));
    assert_eq!(ROWS_WAIT, Duration::from_secs(25));
    s.h.shutdown();
} // mutations: unbounded wait (recv_timeout fails); `complete` always true

#[test]
fn a_dying_writer_leaves_no_row_count() { // S6
    let s = setup_with(|b| b, |o| o.usage_panic = true); // the writer panics on its first row
    let g = grant(&s, &s.alice);
    assert_eq!(msg(&s, &g, "code").0, 200);
    let t0 = Instant::now();
    let v: Value = json_of(&requests(&s, &g.session, &s.alice));
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    assert_eq!(v["complete"].as_bool(), Some(true)); // the dropped tickets released the count
    s.h.shutdown();
} // mutation: the count released only after an insert (→ waits the full 25 s)

/// F7 (round 2): rows of another label's session stay hidden after that session is gone.
#[test]
fn requests_other_label_gone_session_401() {
    let s = setup(false, false, false);
    let g = grant(&s, &s.bob);
    assert_eq!(msg(&s, &g, "code").0, 200);
    assert!(s.h.usage_written(1, std::time::Duration::from_secs(10)));
    let b = bearer(&s.bob);
    assert_eq!(send(&s, "DELETE", &format!("v1/zygo/sessions/{}", g.session), &[("authorization", b.as_str())], None).0, 204);
    assert_eq!(json_of::<Value>(&requests(&s, &g.session, &s.bob))["requests"].as_array().unwrap().len(), 1);
    assert_eq!(err(&requests(&s, &g.session, &s.alice)), (401, "zygo_session_unknown".into()));
    s.h.shutdown();
}

/// F4 (round 2): §7.2 "prices never leave the server": a priced model's row goes out with `estimated_usd` 0.
#[test]
fn requests_estimated_usd_zero() {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    let (alice, bob) = write_base(d.path(), &up);
    let p = cfg_path(d.path());
    let t = std::fs::read_to_string(&p).unwrap().replace("upstream_model = \"up-code\"\n", "upstream_model = \"up-code\"\nprice_usd_per_mtok = { input = 777000.0, output = 777000.0, cache_read = 0.0, cache_write = 0.0 }\n");
    std::fs::write(&p, t).unwrap();
    let lines = Lines::default();
    let clock = ManualClock::new();
    let h = serve::start(opts(d.path(), "127.0.0.1:0", None, &lines, clock.clone(), false, false)).unwrap();
    let s = Srv { d, up, h, alice, bob, lines, clock, tls: None };
    let g = grant(&s, &s.alice);
    assert_eq!(msg(&s, &g, "code").0, 200);
    assert!(s.h.usage_written(1, std::time::Duration::from_secs(10)));
    assert!(row_of(&s, &g).estimated_usd > 0.0, "the stored row keeps its cost");
    let v: Value = json_of(&requests(&s, &g.session, &s.alice));
    assert_eq!(v["requests"][0]["estimated_usd"].as_f64(), Some(0.0), "price-derived cost served to client");
    s.h.shutdown();
}

// ---- liveness and redaction on the server ----

fn key_sample() -> String { ["sk-", "ant-api03-", &"Ab1_".repeat(24)].concat() } // built-in `anthropic_api_key`
fn session_hd(g: &SessionGrant) -> Vec<(&str, &str)> { vec![("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())] }
fn row_of(s: &Srv, g: &SessionGrant) -> RequestRow {
    let mut v = Store::open(&s.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap();
    assert_eq!(v.len(), 1);
    v.remove(0)
}

#[test]
fn live_suspend_and_resume_over_https() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    let id = &g.session;
    let r = send(&s, "POST", &format!("v1/zygo/sessions/{id}/compact"), &session_hd(&g), Some(br#"{"window":600000}"#.to_vec()));
    assert_eq!(r.0, 200);
    s.clock.advance(SUSPEND_AFTER + std::time::Duration::from_secs(1));
    assert_eq!(err(&send(&s, "GET", "anthropic/v1/models", &session_hd(&g), None)), (401, "zygo_session_suspended".into()));
    let b = bearer(&s.alice);
    assert_eq!(send(&s, "POST", &format!("v1/zygo/sessions/{id}/heartbeat"), &[("authorization", b.as_str())], None).0, 204);
    assert_eq!(send(&s, "GET", "anthropic/v1/models", &session_hd(&g), None).0, 200);
    let st: Value = json_of(&send(&s, "GET", &format!("v1/zygo/sessions/{id}/status"), &session_hd(&g), None));
    assert_eq!(st["compact"], 600000);
    s.h.shutdown();
}

#[test]
fn redact_secret_on_server() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    let r = msg_with(&s, &g, "code", json!([{"type": "tool_result", "tool_use_id": "t1", "content": key_sample()}]));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_eq!(hdr(&r, "x-zygo-redacted"), "1");
    let up = String::from_utf8(s.up.bodies()[0].clone()).unwrap();
    assert!(up.contains("[REDACTED:anthropic_api_key]") && !up.contains(&key_sample()));
    assert!(s.h.usage_written(1, std::time::Duration::from_secs(10)));
    let row = row_of(&s, &g);
    assert_eq!(row.redactions, vec![RedactionCount { id: "anthropic_api_key".into(), count: 1 }]);
    assert_eq!(row.token_label.as_deref(), Some("alice"));
    s.h.shutdown();
}

/// F7 (round 2): `count_tokens` is scanned on the server too (§16 intro).
#[test]
fn redact_count_tokens_on_server() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    let b = serde_json::to_vec(&json!({"model": "code", "messages": [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": key_sample()}]}]})).unwrap();
    let r = send(&s, "POST", "anthropic/v1/messages/count_tokens", &session_hd(&g), Some(b));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_eq!(hdr(&r, "x-zygo-redacted"), "1");
    let got = s.up.got.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "/v1/messages/count_tokens");
    let up = String::from_utf8(got[0].1.clone()).unwrap();
    assert!(up.contains("[REDACTED:anthropic_api_key]") && !up.contains(&key_sample()), "{up}");
    s.h.shutdown();
}

#[test] // §7.3 note (c), §7.2: provider-account headers do not reach a serve client
fn serve_drops_provider_account_headers() {
    let s = setup(false, false, false);
    let g = grant(&s, &s.alice);
    let r = msg(&s, &g, "code");
    assert_eq!((r.0, hdr(&r, "x-zygo-model").as_str()), (200, "test-code"));
    for n in ["openai-organization", "openai-project", "anthropic-organization-id"] {
        assert!(r.1.get(n).is_none(), "{n} reached the client");
    }
    s.h.shutdown();
}

// ---- reload, signals, stop ----

fn without_bob(s: &Srv) -> impl Fn(String) -> String {
    let entry = format!(", {{ label = \"bob\", sha256 = \"{}\" }}", hex256(&s.bob));
    move |t| {
        assert!(t.contains(&entry));
        t.replace(&entry, "")
    }
}
/// The text without the `[[model]]` block of `id` (up to the next `[[model]]` or `[[marketplace]]`).
fn without_model(t: String, id: &str) -> String {
    let start = t.find(&format!("[[model]]\nid = \"{id}\"\n")).unwrap();
    let rest = &t[start + 1..];
    let end = start + 1 + rest.find("\n[[model]]\n").or_else(|| rest.find("\n[[marketplace]]\n")).unwrap() + 1;
    format!("{}{}", &t[..start], &t[end..])
}
fn last_line(s: &Srv) -> String { lines(&s.lines).last().unwrap().clone() }
fn reload_failed_line(s: &Srv) -> String {
    format!("zygo serve: reload failed ({}); run zygo config check", cfg_path(s.d.path()).display())
}

#[test]
fn reload_revokes_removed_token() {
    let s = setup(true, false, false);
    let _bob_session = grant(&s, &s.bob);
    rewrite(&s, without_bob(&s));
    s.h.reload().unwrap();
    assert_eq!(err(&team_get(&s, "v1/zygo/config", &s.bob)), (401, "zygo_token_invalid".into()));
    assert_eq!(team_get(&s, "v1/zygo/config", &s.alice).0, 200);
    assert_eq!(last_line(&s), "zygo serve: config reloaded");
    s.h.shutdown();
}

#[test]
fn reload_drops_sessions_of_removed_label() {
    let s = setup(true, false, false);
    let (gb, ga) = (grant(&s, &s.bob), grant(&s, &s.alice));
    assert_eq!(msg(&s, &gb, "code").0, 200);
    rewrite(&s, without_bob(&s));
    s.h.reload().unwrap();
    assert_eq!(err(&msg(&s, &gb, "code")), (401, "zygo_session_unknown".into()));
    assert_eq!(msg(&s, &ga, "code").0, 200);
    s.h.shutdown();
}

/// F1 (round 2): a create whose label was checked before a reload that removes it, its body sent after, is
/// refused 401 `zygo_token_invalid`.
#[test]
fn reload_create_race_refused() {
    use std::io::{Read, Write};
    let s = setup(false, false, false);
    let body = session_body(false, None);
    let addr = s.h.url().socket_addrs(|| None).unwrap()[0];
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    write!(c, "POST /v1/zygo/sessions HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", s.bob, body.len()).unwrap();
    c.write_all(&body[..1]).unwrap();
    c.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    rewrite(&s, without_bob(&s));
    s.h.reload().unwrap();
    assert_eq!(err(&team_get(&s, "v1/zygo/config", &s.bob)).0, 401);
    c.write_all(&body[1..]).unwrap();
    let mut resp = String::new();
    c.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 401 "), "{resp}");
    assert!(resp.contains("zygo_token_invalid"), "{resp}");
    s.h.shutdown();
}

#[test]
fn reload_failure_keeps_config() {
    let s = setup(true, false, false);
    rewrite(&s, |_| "[[ not toml".into());
    assert!(s.h.reload().is_err());
    assert_eq!(last_line(&s), reload_failed_line(&s));
    assert_eq!(team_get(&s, "v1/zygo/config", &s.bob).0, 200);
    s.h.shutdown();
}

#[test]
fn reload_check_error_keeps_config() {
    let s = setup(true, false, false);
    let g = grant(&s, &s.alice);
    rewrite(&s, |t| t.replace("model = \"test-code\"", "model = \"no-such-model\""));
    assert!(s.h.reload().is_err());
    assert_eq!(last_line(&s), reload_failed_line(&s));
    assert_eq!(hdr(&msg(&s, &g, "code"), "x-zygo-model"), "test-code");
    s.h.shutdown();
}

#[test]
fn reload_override_stale_over_http() {
    let s = setup(true, false, false);
    let g = grant_with(&s, &s.alice, Some(json!({"name": "p1", "overrides": {"code": "test-alt"}})));
    assert_eq!(g.profile_warning, None);
    assert_eq!(hdr(&msg(&s, &g, "code"), "x-zygo-model"), "test-alt");
    rewrite(&s, |t| without_model(t, "test-alt"));
    s.h.reload().unwrap();
    let r = msg(&s, &g, "code");
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_eq!(hdr(&r, "x-zygo-model"), "test-code");
    assert!(hdr(&r, "x-zygo-adjustments").split(',').any(|a| a == "profile_override_stale"), "{}", hdr(&r, "x-zygo-adjustments"));
    s.h.shutdown();
}

#[test]
fn reload_new_sessions_see_new_catalog() {
    let s = setup(true, false, false);
    let url = s.up.url();
    rewrite(&s, |t| {
        t + &format!("\n[[model]]\nid = \"test-new\"\n[[model.provider]]\nname = \"p-new\"\nbase_url = \"{url}\"\ndialects = [\"anthropic\"]\nupstream_model = \"up-new\"\n")
    });
    s.h.reload().unwrap();
    assert!(grant(&s, &s.alice).config.models.contains(&"test-new".to_string()));
    s.h.shutdown();
}

#[test]
fn reload_prunes_old_rows() {
    let s = setup(true, false, false);
    let db = s.d.path().join("data/usage.db");
    Store::open(&db).unwrap().insert_request(&RequestRow { ts: 0, session: "old-session".into(), ..Default::default() }).unwrap();
    s.h.reload().unwrap();
    assert!(Store::open(&db).unwrap().requests_for_session("old-session").unwrap().is_empty());
    s.h.shutdown();
}

#[test]
fn reload_events_report_and_stop() {
    let s = setup(true, false, false);
    let tx = s.h.sender();
    tx.send(ServeEvent::Reload).unwrap();
    tx.send(ServeEvent::Stop).unwrap();
    s.h.events();
    assert_eq!(lines(&s.lines)[1], "zygo serve: config reloaded");
    let url = s.h.url();
    s.h.shutdown();
    assert!(request(&client(s.tls.as_ref()), url.join("healthz").unwrap(), "GET", &[], None).is_err());
}

#[cfg(unix)]
#[test]
fn signal_hup_reaches_server() {
    let s = setup(false, true, true);
    let pid_file = s.d.path().join("data/serve.pid");
    std::thread::scope(|sc| {
        let ev = sc.spawn(|| s.h.events());
        assert_eq!(serve::reload_running(&pid_file).unwrap(), std::process::id());
        assert!(s.h.wait_reloads(1, std::time::Duration::from_secs(5)));
        s.h.sender().send(ServeEvent::Stop).unwrap();
        ev.join().unwrap();
    });
    s.h.shutdown();
    assert!(!pid_file.exists());
}

// ---- timeouts and connection limit (§7 note 2026-11-19) ----

#[test]
fn bounds_are_the_spec_values() {
    assert_eq!((UPSTREAM_CONNECT, UPSTREAM_READ_IDLE), (Duration::from_secs(10), Duration::from_secs(600)));
    assert_eq!((HEADER_READ, BODY_READ_IDLE, MAX_CONNECTIONS), (Duration::from_secs(60), Duration::from_secs(60), 512));
    let o = StartOpts::default();
    assert_eq!(
        (o.upstream_connect, o.upstream_read_idle, o.header_read, o.body_read_idle, o.max_connections),
        (UPSTREAM_CONNECT, UPSTREAM_READ_IDLE, HEADER_READ, BODY_READ_IDLE, MAX_CONNECTIONS)
    );
} // mutations: another constant value; a default not wired to its constant

/// A raw connection to the server; a read waits at most 10 s.
fn raw(s: &Srv) -> TcpStream {
    let c = TcpStream::connect(s.h.url().socket_addrs(|| None).unwrap()[0]).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c
}
/// Reads until the server closes (EOF or reset); a read that times out fails the test.
fn wait_closed(c: &mut TcpStream) {
    let mut b = [0u8; 4096];
    loop {
        match c.read(&mut b) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return,
            Err(e) => panic!("not closed: {e}"),
        }
    }
}

#[test]
fn header_read_bound_drops_a_silent_client() {
    let s = setup_with(|t| t, |o| o.header_read = Duration::from_secs(1));
    let mut c = raw(&s);
    let t0 = Instant::now();
    wait_closed(&mut c);
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    s.h.shutdown();
} // mutation: no `first_request_timeout(..)` (protocol detection waits forever)

#[test]
fn header_read_bound_drops_a_stalled_next_request() {
    let s = setup_with(|t| t, |o| o.header_read = Duration::from_secs(1));
    let mut c = raw(&s);
    c.write_all(b"GET /healthz HTTP/1.1\r\nhost: x\r\n\r\n").unwrap();
    let mut got = Vec::new();
    let mut b = [0u8; 4096];
    while !got.ends_with(b"ok") {
        let k = c.read(&mut b).unwrap();
        assert!(k > 0, "closed after {}", String::from_utf8_lossy(&got));
        got.extend_from_slice(&b[..k]);
    }
    assert!(got.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&got));
    c.write_all(b"GET /healthz HTTP/1.1\r\nhost: x\r\n").unwrap();
    let t0 = Instant::now();
    wait_closed(&mut c);
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    s.h.shutdown();
} // mutation: no `.header_read_timeout(..)` (the first-request bound has already passed)

#[test]
fn http2_prior_knowledge_is_refused() { // §7 note (N60 review): HTTP/1.1 only, so the header bound covers an idle connection
    let s = setup_with(|t| t, |o| o.header_read = Duration::from_secs(1));
    let mut c = raw(&s);
    // The preface, an empty SETTINGS frame and one complete request (HEADERS, END_STREAM: GET http /), then idle.
    c.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0\0\0\x03\x01\x05\0\0\0\x01\x82\x86\x84").unwrap();
    let t0 = Instant::now();
    wait_closed(&mut c);
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    s.h.shutdown();
} // mutation: no `.http1_only()` (the h2 connection stays open)

#[test]
fn tls_alpn_offers_http1_only() { // §7 note (N60 review): ALPN `http/1.1`
    use rustls::pki_types::pem::PemObject;
    let s = setup(true, false, false);
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(rustls::pki_types::CertificateDer::from_pem_slice(s.tls.as_ref().unwrap().pem.as_bytes()).unwrap()).unwrap();
    let mut cfg = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let mut conn = rustls::ClientConnection::new(Arc::new(cfg), "127.0.0.1".try_into().unwrap()).unwrap();
    let mut c = raw(&s);
    while conn.is_handshaking() {
        conn.complete_io(&mut c).unwrap();
    }
    assert_eq!(conn.alpn_protocol(), Some(&b"http/1.1"[..]));
    s.h.shutdown();
} // mutation: the TLS config keeps axum-server's ALPN list (`h2` is chosen)

#[test]
fn connection_limit_queues_the_next_client() {
    let s = setup_with(|t| t, |o| o.max_connections = 2);
    let held: Vec<TcpStream> = (0..2).map(|_| raw(&s)).collect();
    let quick = reqwest::Client::builder().no_proxy().timeout(Duration::from_millis(500)).build().unwrap();
    assert!(request(&quick, s.h.url().join("healthz").unwrap(), "GET", &[], None).is_err()); // in the accept queue
    drop(held);
    let ten = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap();
    assert_eq!(request(&ten, s.h.url().join("healthz").unwrap(), "GET", &[], None).map(|r| (r.0, r.2)), Ok((200, b"ok".to_vec())));
    s.h.shutdown();
} // mutations: no limit, or a permit dropped at accept (the first request is served); a permit never released (the second
  // request times out). The correct code never serves the queued request, so load cannot fail the first assertion.

/// Sends `head` (ending in `content-length: 100` and the blank line) and 10 body bytes, then stalls; returns the reply
/// read until the server closes. A read that waits 10 s fails the test.
fn stalled_upload(s: &Srv, head: &str) -> Vec<u8> {
    let mut c = raw(s);
    c.write_all(head.as_bytes()).unwrap();
    c.write_all(&[b' '; 10]).unwrap();
    let t0 = Instant::now();
    let mut got = Vec::new();
    let mut b = [0u8; 4096];
    loop {
        match c.read(&mut b) {
            Ok(0) => break,
            Ok(k) => got.extend_from_slice(&b[..k]),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => panic!("no end after {:?}: {e}", t0.elapsed()),
        }
    }
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    got
}
/// The status line and the error message of a raw HTTP/1.1 reply.
fn raw_err(got: &[u8]) -> (String, String) {
    let t = String::from_utf8_lossy(got);
    let (head, body) = t.split_once("\r\n\r\n").unwrap_or_else(|| panic!("no reply: {t}"));
    let v: Value = serde_json::from_str(body).unwrap_or_else(|_| panic!("not JSON: {t}"));
    (head.lines().next().unwrap().to_string(), v["error"]["message"].as_str().unwrap_or_default().to_string())
}

#[test]
fn a_stalled_session_create_ends_at_the_body_bound() { // §7 note (N60 review): the body bound covers every route
    let s = setup_with(|t| t, |o| {
        o.body_read_idle = Duration::from_secs(1);
        o.max_connections = 1;
    });
    let head = format!(
        "POST /v1/zygo/sessions HTTP/1.1\r\nhost: x\r\nauthorization: {}\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\r\n",
        bearer(&s.alice)
    );
    let got = stalled_upload(&s, &head);
    assert_eq!(raw_err(&got), ("HTTP/1.1 400 Bad Request".to_string(), "unreadable body".to_string()));
    assert_eq!(healthz(&s.h, None), Ok((200, b"ok".to_vec()))); // the stalled client's slot is free
    s.h.shutdown();
} // mutation: no bound in `routes::read_body` (the read waits 10 s)

#[test]
fn a_stalled_upload_ends_at_the_body_bound() { // §7 note (N60 plan review): 60 s between body reads
    let s = setup_with(|t| t, |o| {
        o.body_read_idle = Duration::from_secs(1);
        o.max_connections = 1;
    });
    let g = grant(&s, &s.alice);
    let head = format!(
        "POST /anthropic/v1/messages HTTP/1.1\r\nhost: x\r\nx-api-key: {}\r\nx-zygo-session: {}\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\r\n",
        g.token, g.session
    );
    let got = stalled_upload(&s, &head);
    assert_eq!(raw_err(&got), ("HTTP/1.1 400 Bad Request".to_string(), "unreadable body".to_string()));
    assert_eq!(healthz(&s.h, None), Ok((200, b"ok".to_vec()))); // the stalled client's slot is free
    s.h.shutdown();
} // mutation: no body bound (the read times out at 10 s)

#[test]
fn status_windows_after_reload() { // AU29: snapshot before the first main-loop request; then the served row
    let s = setup_with(|t| format!("main_role = \"code\"\n{t}"), |_| {});
    let g = grant(&s, &s.alice);
    let status = |s: &Srv, g: &SessionGrant, rows: u64| {
        assert!(s.h.usage_written(rows, Duration::from_secs(10)), "usage rows not written");
        let r = send(s, "GET", &format!("v1/zygo/sessions/{}/status", g.session), &session_hd(g), None);
        assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
        let v: SessionStatus = json_of(&r);
        (v.model, v.context_window, v.effective_window, v.threshold)
    };
    // test-code: window 200k, soft 150k; agent window 1M (not a claude- id) → safety 180k.
    let snapshot = (Some("test-code".into()), Some(200000), Some(150000), Some(150000));
    assert_eq!(status(&s, &g, 0), snapshot);
    rewrite(&s, |t| {
        let old = "soft_context_window = 150000\n[model.facts]\ncontext_window = 200000\n";
        assert!(t.contains(old));
        t.replace(old, "soft_context_window = 120000\n[model.facts]\ncontext_window = 300000\n")
    });
    s.h.reload().unwrap();
    // No main-loop request yet: the main-role rule reads the session's snapshot.
    assert_eq!(status(&s, &g, 0), snapshot);
    assert_eq!(msg(&s, &g, "code").0, 200);
    // The served row of the new config: window 300k, soft 120k, safety 270k.
    assert_eq!(status(&s, &g, 1), (Some("test-code".into()), Some(300000), Some(120000), Some(120000)));
    s.h.shutdown();
}
