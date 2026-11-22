//! Local-mode proxy (§7.1): 127.0.0.1, OS-assigned port, own runtime thread.

use std::collections::BTreeSet;
#[cfg(feature = "test-hooks")]
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use tokio::sync::Semaphore;
use zygo_config::Merged;

use crate::errors::ZygoError;
use crate::forward::BUSY_WAIT;
use crate::pipeline::Pipeline;
use crate::provider::{KeyResolver, SystemKeys};
#[cfg(feature = "test-hooks")]
use crate::redact_stage::RedactLimits;
use crate::routes;
#[cfg(feature = "test-hooks")]
use crate::serve::{BODY_READ_IDLE, HEADER_READ, MAX_CONNECTIONS};
use crate::session::{Clock, SessionGrant, SessionRequest, SessionState, Sessions, SystemClock, TeamAuth};
#[cfg(feature = "test-hooks")]
use crate::tee::UsageHold;
use crate::tee::{UsageSink, WriterHooks, ROWS_WAIT};
use crate::Url;

/// The upstream connect bound. Spec §7 note 2026-11-19 (owner: follow sensible standards).
pub const UPSTREAM_CONNECT: Duration = Duration::from_secs(10);
/// The upstream read bound, the default request timeout of the Anthropic and OpenAI SDKs. Spec §7 note 2026-11-19
/// (owner: follow sensible standards). Before the response head it is one deadline from the send, covering connect,
/// the upload and the time to the first byte; after the head it restarts on each body read. No total limit, so a
/// long stream is not cut.
pub const UPSTREAM_READ_IDLE: Duration = Duration::from_secs(600);

pub struct Proxy;

impl Proxy {
    /// Binds 127.0.0.1:0 (§7.1), serves on its own multi-thread tokio runtime in one thread.
    pub fn start_local(cfg: Arc<Merged>) -> anyhow::Result<ProxyHandle> {
        Proxy::start_local_with(cfg, StartOpts::default())
    }

    /// `start_local` with options: the key resolver, the usage database (`None`: no usage rows; a database that does
    /// not open leaves the proxy running without rows, `SessionGrant::usage_note` says so), the in-flight wait.
    pub fn start_local_with(cfg: Arc<Merged>, opts: StartOpts) -> anyhow::Result<ProxyHandle> {
        let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0)).context("bind 127.0.0.1")?;
        std_listener.set_nonblocking(true)?;
        let port = std_listener.local_addr()?.port();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/"))?;
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().context("proxy runtime")?;
        let listener = {
            let _guard = rt.enter();
            tokio::net::TcpListener::from_std(std_listener)?
        };
        let served = Served { cfg, known_plugins: opts.known_plugins.clone(), team: opts.team.clone() };
        let app = app(served, opts, None)?;
        let router = routes::router(app.clone());
        let (stop, rx) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::Builder::new().name("zygo-proxy".into()).spawn(move || {
            rt.block_on(async move {
                tokio::select! {
                    // An accept error ends the server; requests then fail as unreachable.
                    _ = async { axum::serve(listener, router).await } => {}
                    _ = rx => {}
                }
            });
            // No graceful drain (N10 review): dropping the runtime drops every connection task, so open connections
            // close at once.
            drop(rt);
        })?;
        Ok(ProxyHandle { url, app, stop, thread })
    }
}

/// The serving state from `opts`: usage sink, upstream clients, pipeline, sessions, in-flight budget.
/// `opts.team` and `opts.known_plugins` are not read: `served` carries them. `body_idle`: the bound between request
/// body reads (serve mode); `None` (local mode, a loopback agent): no bound.
pub(crate) fn app(served: Served, opts: StartOpts, body_idle: Option<Duration>) -> anyhow::Result<Arc<App>> {
    let cfg = served.cfg.clone();
    let hooks = WriterHooks {
        #[cfg(feature = "test-hooks")]
        hold: opts.usage_hold.clone(),
        #[cfg(feature = "test-hooks")]
        panic: opts.usage_panic,
    };
    // The secret deadline of stage 6 for the upstream_errors fields (§14, FIX-10).
    #[cfg(feature = "test-hooks")]
    let secret = opts.redact_limits.as_ref().map(|l| l.secret.clone()).unwrap_or_default();
    #[cfg(not(feature = "test-hooks"))]
    let secret = zygo_redact::DeadlinePolicy::default();
    let (usage, usage_note) = UsageSink::open(opts.usage_db.as_deref(), hooks, secret);
    #[cfg(feature = "test-hooks")]
    let rows_wait = opts.rows_wait;
    #[cfg(not(feature = "test-hooks"))]
    let rows_wait = ROWS_WAIT;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = upstream_client(&opts, true)?;
    let direct = upstream_client(&opts, false)?;
    #[cfg(feature = "test-hooks")]
    let test_hosts: Vec<String> = opts.upstream_test.iter().flatten().map(|(h, _)| h.to_ascii_lowercase()).collect();
    #[cfg(feature = "test-hooks")]
    let pipeline = match opts.pipeline {
        Some(p) => p,
        None => Pipeline::with_redact_limits(opts.keys.clone(), test_hosts.clone(), opts.redact_limits.clone().unwrap_or_default()),
    };
    #[cfg(not(feature = "test-hooks"))]
    let pipeline = Pipeline::new(opts.keys.clone());
    let inflight_bytes = usize::try_from(cfg.config.proxy.max_inflight_mib)
        .unwrap_or(usize::MAX)
        .saturating_mul(1 << 20)
        .min(Semaphore::MAX_PERMITS);
    Ok(Arc::new(App {
        sessions: Sessions::new(cfg.config.proxy.redaction_cache_mib, opts.clock.clone(), usage_note),
        served: Arc::new(RwLock::new(Arc::new(served))),
        clock: opts.clock,
        pipeline: Arc::new(pipeline),
        http,
        direct,
        inflight: Arc::new(Semaphore::new(inflight_bytes)),
        busy_wait: opts.busy_wait,
        body_idle,
        usage: Arc::new(usage),
        usage_db: opts.usage_db,
        rows_wait,
        #[cfg(feature = "test-hooks")]
        test_hosts,
    }))
}

/// The upstream client; no redirect is followed (§7.3 note). `system_proxy`: the system proxy settings (an HTTPS
/// CONNECT tunnel keeps the key encrypted); else no proxy, for a loopback upstream (C3: a proxy would see the key of
/// an `http://` request in clear text).
fn upstream_client(#[allow(unused_variables)] opts: &StartOpts, system_proxy: bool) -> anyhow::Result<reqwest::Client> {
    #[cfg(feature = "test-hooks")]
    let (connect, idle) = (opts.upstream_connect, opts.upstream_read_idle);
    #[cfg(not(feature = "test-hooks"))]
    let (connect, idle) = (UPSTREAM_CONNECT, UPSTREAM_READ_IDLE);
    // No `.timeout`: a total limit would cut a long stream.
    let mut b = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).connect_timeout(connect).read_timeout(idle);
    if !system_proxy {
        b = b.no_proxy();
    }
    #[cfg(feature = "test-hooks")]
    if let (true, Some(p)) = (system_proxy, &opts.upstream_proxy) {
        b = b.proxy(reqwest::Proxy::all(p.as_str()).context("upstream proxy")?);
    }
    #[cfg(feature = "test-hooks")]
    if let Some(hosts) = &opts.upstream_test {
        b = b.no_proxy();
        for (h, a) in hosts {
            b = b.resolve(h, *a);
        }
    }
    b.build().context("upstream http client")
}

pub struct StartOpts {
    pub team: Option<Arc<dyn TeamAuth>>,
    pub clock: Arc<dyn Clock>,
    /// `<marketplace>:<plugin>` names the server knows; HTTP session creation validates profiles against them.
    pub known_plugins: BTreeSet<String>,
    /// Provider keys (§6.2); default `SystemKeys`.
    pub keys: Arc<dyn KeyResolver>,
    /// The usage database (§14); `None` (default): no usage rows.
    pub usage_db: Option<PathBuf>,
    /// The in-flight memory wait before `503 zygo_busy` (§7.3); default `BUSY_WAIT`.
    pub busy_wait: Duration,
    /// Upstream host names resolved to these addresses; they count as loopback and the client uses no proxy.
    #[cfg(feature = "test-hooks")]
    pub upstream_test: Option<Vec<(String, SocketAddr)>>,
    /// A proxy URL used in place of the system proxy settings (the system-proxy seam).
    #[cfg(feature = "test-hooks")]
    pub upstream_proxy: Option<String>,
    /// Redaction deadlines and the panic hook (stage 6); `None`: the spec values.
    #[cfg(feature = "test-hooks")]
    pub redact_limits: Option<RedactLimits>,
    /// A stage list used in place of the default one.
    #[cfg(feature = "test-hooks")]
    pub pipeline: Option<Pipeline>,
    /// The rows reply's wait for a session's queued rows; default `ROWS_WAIT`.
    #[cfg(feature = "test-hooks")]
    pub rows_wait: Duration,
    /// The upstream connect bound; default `UPSTREAM_CONNECT`.
    #[cfg(feature = "test-hooks")]
    pub upstream_connect: Duration,
    /// The upstream read bound; default `UPSTREAM_READ_IDLE`.
    #[cfg(feature = "test-hooks")]
    pub upstream_read_idle: Duration,
    /// Serve mode: the request-header read bound; default `HEADER_READ`.
    #[cfg(feature = "test-hooks")]
    pub header_read: Duration,
    /// Serve mode: the bound between request body reads; default `BODY_READ_IDLE`.
    #[cfg(feature = "test-hooks")]
    pub body_read_idle: Duration,
    /// Serve mode: the open client connection limit; default `MAX_CONNECTIONS`.
    #[cfg(feature = "test-hooks")]
    pub max_connections: usize,
    /// Pauses the usage writer before each insert until released.
    #[cfg(feature = "test-hooks")]
    pub usage_hold: Option<Arc<UsageHold>>,
    /// The usage writer panics on its first row.
    #[cfg(feature = "test-hooks")]
    pub usage_panic: bool,
}

impl Default for StartOpts {
    fn default() -> Self {
        StartOpts {
            team: None,
            clock: Arc::new(SystemClock),
            known_plugins: BTreeSet::new(),
            keys: Arc::new(SystemKeys),
            usage_db: None,
            busy_wait: BUSY_WAIT,
            #[cfg(feature = "test-hooks")]
            upstream_test: None,
            #[cfg(feature = "test-hooks")]
            upstream_proxy: None,
            #[cfg(feature = "test-hooks")]
            redact_limits: None,
            #[cfg(feature = "test-hooks")]
            pipeline: None,
            #[cfg(feature = "test-hooks")]
            rows_wait: ROWS_WAIT,
            #[cfg(feature = "test-hooks")]
            upstream_connect: UPSTREAM_CONNECT,
            #[cfg(feature = "test-hooks")]
            upstream_read_idle: UPSTREAM_READ_IDLE,
            #[cfg(feature = "test-hooks")]
            header_read: HEADER_READ,
            #[cfg(feature = "test-hooks")]
            body_read_idle: BODY_READ_IDLE,
            #[cfg(feature = "test-hooks")]
            max_connections: MAX_CONNECTIONS,
            #[cfg(feature = "test-hooks")]
            usage_hold: None,
            #[cfg(feature = "test-hooks")]
            usage_panic: false,
        }
    }
}

/// The serving config, its team tokens and its known plugins: one snapshot, swapped whole at a reload (§7.2).
pub struct Served {
    pub cfg: Arc<Merged>,
    pub known_plugins: BTreeSet<String>,
    pub team: Option<Arc<dyn TeamAuth>>,
}

pub(crate) type ServedCell = Arc<RwLock<Arc<Served>>>;

pub(crate) struct App {
    /// Read once per request with `served()`.
    pub served: ServedCell,
    pub sessions: Sessions,
    pub clock: Arc<dyn Clock>,
    pub pipeline: Arc<Pipeline>,
    /// Upstream client with the system proxy settings, for a non-loopback host.
    pub http: reqwest::Client,
    /// Upstream client without proxy, for a loopback host.
    pub direct: reqwest::Client,
    /// In-flight request memory in bytes (§7.3).
    pub inflight: Arc<Semaphore>,
    pub busy_wait: Duration,
    /// The bound between request body reads; `None`: no bound (§7 note 2026-11-19, serve mode only).
    pub body_idle: Option<Duration>,
    pub usage: Arc<UsageSink>,
    pub usage_db: Option<PathBuf>,
    /// The rows reply's wait for a session's queued rows (§14, Q2).
    pub rows_wait: Duration,
    /// `StartOpts.upstream_test` host names (the passthrough host seam).
    #[cfg(feature = "test-hooks")]
    pub test_hosts: Vec<String>,
}

impl App {
    /// The current snapshot; a poisoned lock is read through (the swap is one assignment).
    pub(crate) fn served(&self) -> Arc<Served> {
        self.served.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn swap(&self, s: Served) {
        *self.served.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
    }
}

pub struct ProxyHandle {
    url: Url,
    app: Arc<App>,
    stop: tokio::sync::oneshot::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl ProxyHandle {
    /// `http://127.0.0.1:<port>/`
    pub fn url(&self) -> Url {
        self.url.clone()
    }

    /// In-process session creation (§7.2 local mode); no team label.
    pub fn create_session(&self, req: SessionRequest) -> Result<SessionGrant, ZygoError> {
        self.app.sessions.create(req, None, &self.app.served().cfg, &|| true)
    }

    /// Stops at once: open connections close, no drain. Joins the server thread, then writes the queued usage rows.
    pub fn shutdown(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
        self.app.usage.close();
    }

    /// Takes `bytes` of the in-flight budget until the hold is dropped; panics when they are not free.
    #[cfg(feature = "test-hooks")]
    pub fn hold_inflight(&self, bytes: u32) -> InflightHold {
        InflightHold(self.app.inflight.clone().try_acquire_many_owned(bytes).expect("in-flight bytes free"))
    }

    /// True once `n` usage rows were handled (inserted or dropped), false after `limit`.
    #[cfg(feature = "test-hooks")]
    pub fn usage_written(&self, n: u64, limit: Duration) -> bool {
        self.app.usage.wait(n, limit)
    }

    #[doc(hidden)]
    pub fn session(&self, id: &str) -> Option<Arc<SessionState>> {
        self.app.sessions.get(id)
    }
}

#[cfg(feature = "test-hooks")]
pub struct InflightHold(#[allow(dead_code)] tokio::sync::OwnedSemaphorePermit);
