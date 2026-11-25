//! HUD-10: the HUD of a team session (H§14 #15, AU14, review focus 8).
//!
//! A real team server (`zygo_proxy::serve`) whose configured prices differ from the built-in catalog serves a team
//! session; `collect` runs in process against it. The test passes its environment as a closure and its own `Paths`:
//! it changes no process environment. The clock is the `test-hooks` seam `ZYGO_TEST_NOW`. The requests go over
//! plain loopback TCP, so the test needs no HTTP client crate.

use std::ffi::OsString;
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::{load, LoadContext, Prices, TokenUsage};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_hooks::statusline::data::collect;
use zygo_hooks::statusline::render::{render, run_time_text, spend_text, ColorMode, HudData, IconSet, RenderEnv, Style};
use zygo_proxy::client::{SessionClient, SessionStatus};
use zygo_proxy::serve::{self, ServeHandle, ServeOpts};
use zygo_proxy::server::{Proxy, StartOpts};
use zygo_proxy::session::SessionRequest;
use zygo_proxy::team::new_token;

/// The clock of the test: `ZYGO_TEST_NOW`.
const T: u64 = 1_800_000_000;
const FAR: Duration = Duration::from_secs(10);

// ---- plain HTTP/1.1 over loopback ----

/// Reads one request or response: the head, then `content-length` bytes of body.
fn read_message(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok());
    let mut body = Vec::new();
    match length {
        Some(n) => {
            body.resize(n, 0);
            stream.read_exact(&mut body).unwrap();
        }
        // A response with no length (a streamed body): all bytes until the server closes.
        None if head.starts_with("HTTP/") => {
            let _ = stream.read_to_end(&mut body);
        }
        None => {}
    }
    (head, body)
}

/// One request on its own connection: the status and the body bytes.
fn http(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&Value>) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut request = format!("{method} /{path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\ncontent-type: application/json\r\ncontent-length: {}\r\n", body.len());
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(&body);
    stream.write_all(request.as_bytes()).unwrap();
    let (head, body) = read_message(&mut stream);
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("no status line in {head:?}"));
    (status, body)
}

/// A fake Anthropic upstream: every request gets a Messages reply with the `usage` object that `set_usage` gave.
struct Upstream {
    url: String,
    usage: Arc<Mutex<Value>>,
}

impl Upstream {
    fn start() -> Upstream {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let usage = Arc::new(Mutex::new(json!({"input_tokens": 1, "output_tokens": 1})));
        let u = usage.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
                let _ = read_message(&mut stream);
                let usage = u.lock().unwrap().clone();
                let body = json!({"id": "m", "type": "message", "role": "assistant", "content": [], "usage": usage}).to_string();
                let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            }
        });
        Upstream { url, usage }
    }

    fn set_usage(&self, u: &TokenUsage) {
        *self.usage.lock().unwrap() = json!({
            "input_tokens": u.input,
            "output_tokens": u.output,
            "cache_read_input_tokens": u.cache_read,
            "cache_creation_input_tokens": u.cache_write,
        });
    }
}

// ---- the servers ----

fn paths(root: &Path) -> Paths {
    let (cache_dir, data_dir) = (root.join("cache"), root.join("data"));
    Paths {
        config_file: root.join("config.toml"),
        sessions_dir: root.join("sessions"),
        marketplaces_dir: cache_dir.join("marketplaces"),
        plugin_store: data_dir.join("plugins"),
        approvals_file: data_dir.join("approvals.toml"),
        usage_db: data_dir.join("usage.db"),
        cache_dir,
        data_dir,
    }
}

/// The server's configured rates: none is a rate of the built-in catalog.
fn team_prices() -> Prices {
    Prices { input: 7.25, output: 11.5, cache_read: 3.75, cache_write: 13.5, ..Default::default() }
}

/// The server's config: the configured rates on `test-code` (a model the built-in catalog does not have) and on a
/// `claude-haiku-5-5` row that replaces the built-in one.
fn server_config(token_sha256: &str, upstream: &str) -> String {
    let price = "price_usd_per_mtok = { input = 7.25, output = 11.5, cache_read = 3.75, cache_write = 13.5 }";
    let model = |id: &str, window: u64, provider: &str, served: &str| {
        format!(
            "[[model]]\nid = \"{id}\"\n[model.facts]\ncontext_window = {window}\nmax_output_tokens = 8192\n[[model.provider]]\nname = \"{provider}\"\nbase_url = \"{upstream}\"\ndialects = [\"anthropic\"]\nupstream_model = \"{served}\"\n{price}\n\n"
        )
    };
    format!(
        "[serve]\ntokens = [{{ label = \"alice\", sha256 = \"{token_sha256}\" }}]\n\n{}{}",
        model("test-code", 200_000, "p-code", "up-code"),
        model("claude-haiku-5-5", 1_000_000, "p-haiku", "up-haiku"),
    )
}

/// The first price of the row `id` in the built-in catalog of this binary.
fn list_prices(id: &str) -> Option<Prices> {
    zygo_config::builtin::defaults().catalog.get(id)?.provider.iter().find_map(|p| p.price_usd_per_mtok.clone())
}

fn start_team_server(root: &Path) -> ServeHandle {
    let ctx = LoadContext { paths: paths(root), repo: None, team: None };
    serve::start(ServeOpts {
        listen: "127.0.0.1:0".into(),
        tls: None,
        config_file: root.join("config.toml"),
        load: Arc::new(move || Ok(load(&ctx)?)),
        report: Arc::new(|_: &str| {}),
        pid_file: None,
        signals: false,
        start: StartOpts::default(),
    })
    .unwrap()
}

fn addr_of(url: &zygo_proxy::Url) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], url.port().unwrap()))
}

// ---- the session of the HUD ----

/// A session directory `<tmp>/sessions/<id>` with the token, the `url` file and a `view.json`, and `<tmp>/plain`, a
/// directory outside every repository.
struct Fx {
    t: TempDir,
    dir: PathBuf,
    plain: PathBuf,
    runs: u64,
}

impl Fx {
    fn new(session: &str, token: &str, url: &str) -> Fx {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("sessions").join(session);
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token(token).unwrap();
        std::fs::write(sd.url(), url).unwrap();
        SessionView { session: session.into(), ..Default::default() }.save(&sd).unwrap();
        let plain = t.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        Fx { t, dir, plain, runs: 0 }
    }

    fn client(&self) -> SessionClient {
        SessionClient::from_session_dir(&self.dir).unwrap()
    }

    /// The status reply once the session has `requests` counted requests: the usage of a request is added when its
    /// response body ends, which can be after the client has read it.
    fn status_after(&self, requests: u64) -> SessionStatus {
        let end = Instant::now() + FAR;
        loop {
            let s = self.client().status().unwrap();
            if s.requests_total == Some(requests) {
                return s;
            }
            assert!(Instant::now() < end, "requests_total {:?}, not {requests}", s.requests_total);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// One `collect` with `ZYGO_URL` = `zygo_url`. Each run is 31 s after the one before, so the status part of
    /// the cache is due and the run fetches.
    fn run(&mut self, zygo_url: &str) -> HudData {
        let now = T + 31 * self.runs;
        self.runs += 1;
        let vars = [("ZYGO_TEST_NOW".to_string(), now.to_string()), ("ZYGO_URL".to_string(), zygo_url.to_string())];
        let env = move |key: &str| vars.iter().find(|(k, _)| k == key).map(|(_, v)| OsString::from(v));
        let stdin = json!({ "cwd": self.plain }).to_string().into_bytes();
        collect(Box::new(Cursor::new(stdin)), &self.dir, &paths(self.t.path()), &env, Instant::now() + FAR)
    }
}

fn renv(style: Style, color: ColorMode) -> RenderEnv {
    RenderEnv { style, icons: IconSet::Nerd, color, width: 200, force_panic: false }
}

fn text(d: &HudData, style: Style) -> String {
    render(d, &renv(style, ColorMode::Off))
}

/// The run-time segment that ends verbose row 1 (session run time design T§2.4): the session id is a real ULID, so
/// the run time is `ZYGO_TEST_NOW` less its ULID time.
fn run_time(d: &HudData) -> String {
    format!("\u{f0150} {}", run_time_text(d.run_time.expect("the run time of a ULID session")))
}

fn ansi(d: &HudData, style: Style) -> String {
    render(d, &renv(style, ColorMode::Ansi256))
}

/// The last SGR sequence before the first `needle` in `s`.
fn sgr_before<'a>(s: &'a str, needle: &str) -> &'a str {
    let head = &s[..s.find(needle).unwrap_or_else(|| panic!("no {needle:?} in {s:?}"))];
    let start = head.rfind('\x1b').unwrap_or_else(|| panic!("no escape before {needle:?} in {s:?}"));
    &head[start..start + head[start..].find('m').unwrap() + 1]
}

fn message(addr: SocketAddr, session: &str, token: &str, model: &str) -> u16 {
    let body = json!({"model": model, "max_tokens": 64, "messages": [{"role": "user", "content": "hi"}]});
    http(addr, "POST", "anthropic/v1/messages", &[("x-api-key", token), ("x-zygo-session", session)], Some(&body)).0
}

/// H§14 #15 and review focus 8, the HUD part: a team session's spend is `≈$X` at the built-in list prices, with
/// `≈` in SGR `2`; a served model that the built-in catalog does not have gives `≈$X+`; through a loopback tunnel
/// the mode is `serve 127.0.0.1`. The session of a local proxy on the same config shows `local` and no `≈`.
#[test]
#[ignore = "timing: a fetch 31 s after a cached one must answer within its 1 s cap"]
fn h14_15_team_session_hud_shows_notional_spend_at_list_prices() {
    let up = Upstream::start();
    let server_dir = tempfile::tempdir().unwrap();
    let (team_token, hash) = new_token();
    std::fs::write(server_dir.path().join("config.toml"), server_config(&hash, &up.url)).unwrap();
    let server = start_team_server(server_dir.path());
    let addr = addr_of(&server.url());

    // A team session, made as the launcher makes it: `POST /v1/zygo/sessions` with the team token.
    let create = json!({"agent": "claude-code", "agent_version": "2.1.328", "profile": null, "passthrough": false, "repo": null, "work": null});
    let (status, body) = http(addr, "POST", "v1/zygo/sessions", &[("authorization", &format!("Bearer {team_token}"))], Some(&create));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let grant: Value = serde_json::from_slice(&body).unwrap();
    let (session, token) = (grant["session"].as_str().unwrap(), grant["token"].as_str().unwrap());
    let mut fx = Fx::new(session, token, server.url().as_str());
    // The loopback tunnel: the agent's `ZYGO_URL` is an https URL on 127.0.0.1.
    let tunnel = format!("https://127.0.0.1:{}", addr.port());

    let (list, own) = (list_prices("claude-haiku-5-5").expect("the built-in catalog prices claude-haiku-5-5"), team_prices());
    assert_eq!(list_prices("test-code"), None);
    let above = list.long_context.as_ref().expect("the built-in haiku row has a long-context tier").above;
    // Request 1: a prompt below the tier. Request 2: a prompt above it.
    let short = TokenUsage { input: above - 10_000, output: 20_000, ..Default::default() };
    let long = TokenUsage { input: above + 500_000, output: 100_000, cache_read: 300_000, ..Default::default() };
    for u in [&short, &long] {
        up.set_usage(u);
        assert_eq!(message(addr, session, token, "claude-haiku-5-5"), 200);
    }
    let reply = fx.status_after(2);
    let list_sum = list.cost(&short).unwrap() + list.cost(&long).unwrap();
    let own_sum = own.cost(&short).unwrap() + own.cost(&long).unwrap();
    let want = spend_text(Some(list_sum), Some(true), Some(2), Some(2)).unwrap();
    let configured = spend_text(Some(own_sum), Some(true), Some(2), Some(2)).unwrap();
    // The form is `≈$N.NN`, and the configured prices would give another text.
    assert!((0.01..100.0).contains(&list_sum) && want.starts_with("≈$") && !want.ends_with('+'), "{want}");
    assert_ne!(want, configured);
    assert_eq!((reply.team_session, reply.spend_notional, reply.requests_priced), (Some(true), Some(true), Some(2)));
    assert!((reply.spend_usd.unwrap() - list_sum).abs() < 1e-9, "{:?} != {list_sum}", reply.spend_usd);

    let d = fx.run(&tunnel);
    let s = d.status.as_ref().expect("the status of the team server");
    assert_eq!((s.team_session, s.spend_notional, s.passthrough), (Some(true), Some(true), Some(false)));
    assert_eq!(d.team_host.as_deref(), Some("127.0.0.1"));
    for style in [Style::Compact, Style::Verbose] {
        let (plain, colored) = (text(&d, style), ansi(&d, style));
        assert!(plain.contains(&format!("{want}  ")), "{want}: {plain}");
        assert!(!plain.contains(&configured["≈".len()..]), "{configured}: {plain}");
        assert_eq!(sgr_before(&colored, "≈"), "\x1b[2m", "{colored:?}");
        assert!(colored.contains(&format!("≈\x1b[0m\x1b[32m{}\x1b[0m", &want["≈".len()..])), "{colored:?}");
    }
    // Mode (verbose only): `serve` and the host of `ZYGO_URL`, then the run time.
    assert!(text(&d, Style::Verbose).contains(&format!("  serve 127.0.0.1   {}\n", run_time(&d))), "{}", text(&d, Style::Verbose));

    // Request 3: a model that the built-in catalog does not have. The server prices it in its own row only.
    up.set_usage(&TokenUsage { input: 40_000, output: 8_000, ..Default::default() });
    assert_eq!(message(addr, session, token, "test-code"), 200);
    let reply = fx.status_after(3);
    assert_eq!((reply.requests_total, reply.requests_priced), (Some(3), Some(2)));
    let d = fx.run(&tunnel);
    let plain = text(&d, Style::Compact);
    assert!(plain.contains(&format!("{want}+  ")), "{want}+: {plain}");

    // A local proxy on the same config: `team_session = false`, mode `local`, no `≈`.
    let local = Proxy::start_local(Arc::new(load(&LoadContext { paths: paths(server_dir.path()), repo: None, team: None }).unwrap())).unwrap();
    let lg = local
        .create_session(SessionRequest {
            agent: Agent::ClaudeCode,
            agent_version: "2.1.328".into(),
            profile: None,
            passthrough: false,
            repo: None,
            work: None,
            stored_values: Vec::new(),
            known_plugins: Default::default(),
        })
        .unwrap();
    let mut lfx = Fx::new(&lg.session, &lg.token, local.url().as_str());
    let d = lfx.run(local.url().as_str());
    assert_eq!(d.status.as_ref().and_then(|s| s.team_session), Some(false));
    let plain = text(&d, Style::Verbose);
    assert!(plain.contains(&format!("  local   {}\n", run_time(&d))) && plain.contains("$0.00") && !plain.contains('≈') && !plain.contains("serve"), "{plain}");

    local.shutdown();
    server.shutdown();
}
