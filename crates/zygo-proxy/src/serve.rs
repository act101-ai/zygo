//! Serve mode (§7.1, §7.2, C8, §14): the team server. No log line carries a config value (C3).

use std::collections::BTreeSet;
use std::fs::File;
use std::future::Future;
use std::io::{self, Read, Seek, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::Context as _;
use axum_server::tls_rustls::RustlsAcceptor;
use axum_server::AddrListener;
use fs4::TryLockError;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zygo_config::check::{check_with, RepositoryLoadout};
use zygo_config::{Merged, Severity};
use zygo_usage::Store;

use crate::routes;
use crate::server::{self, App, Served, StartOpts};
use crate::team::{self, Tokens};
use crate::Url;

/// The request-header read bound, the nginx `client_header_timeout` default. Spec §7 note 2026-11-19 (owner: follow
/// sensible standards).
pub const HEADER_READ: Duration = Duration::from_secs(60);
/// The bound between request body reads, the nginx `client_body_timeout` default, so a stalled upload does not hold a
/// connection slot forever. Spec §7 note 2026-11-19 (N60 plan review).
pub const BODY_READ_IDLE: Duration = Duration::from_secs(60);
/// The open client connection limit, the nginx `worker_connections` default; more connections wait in the accept
/// queue. Spec §7 note 2026-11-19 (owner: follow sensible standards).
pub const MAX_CONNECTIONS: usize = 512;

/// macOS refuses an `RLIM_INFINITY` soft open-file limit; `OPEN_MAX` from `<sys/syslimits.h>` (not in `libc`).
#[cfg(target_os = "macos")]
const OPEN_MAX: libc::rlim_t = 10240;

pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

pub struct ServeOpts {
    pub listen: String,
    pub tls: Option<TlsFiles>,
    /// Named in messages; `load` reads it.
    pub config_file: PathBuf,
    pub load: Arc<dyn Fn() -> anyhow::Result<Merged> + Send + Sync>,
    /// One line, no "\n".
    pub report: Arc<dyn Fn(&str) + Send + Sync>,
    pub pid_file: Option<PathBuf>,
    /// SIGHUP → reload, SIGINT and SIGTERM → stop (Windows: Ctrl+C → stop).
    pub signals: bool,
    /// `team` and `known_plugins` are not read: the config gives them.
    pub start: StartOpts,
}

#[derive(Debug, thiserror::Error)]
#[error("a non-loopback listen address needs --tls-cert and --tls-key (C8)")]
pub struct TlsRequired;

/// No error text from the config: it can quote a value (C3).
#[derive(Debug, thiserror::Error)]
#[error("config invalid ({}); run zygo config check", file.display())]
pub struct ConfigInvalid {
    pub file: PathBuf,
}

#[derive(Debug, thiserror::Error)]
#[error("zygo serve is already running")]
pub struct AlreadyRunning;

#[derive(Debug, thiserror::Error)]
#[error("no running zygo serve")]
pub struct NoServer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeEvent {
    Reload,
    Stop,
}

pub struct ServeHandle {
    url: Url,
    app: Arc<App>,
    stop: tokio::sync::oneshot::Sender<()>,
    thread: std::thread::JoinHandle<()>,
    tx: mpsc::Sender<ServeEvent>,
    rx: Mutex<mpsc::Receiver<ServeEvent>>,
    config_file: PathBuf,
    load: Arc<dyn Fn() -> anyhow::Result<Merged> + Send + Sync>,
    report: Arc<dyn Fn(&str) + Send + Sync>,
    /// The locked pid file, held for the server's life (D6).
    pid: Option<(PathBuf, File)>,
    /// Reloads tried (done or failed).
    reloads: (Mutex<u64>, Condvar),
}

/// The config from `load`, refused on a load error or a `config check` error. Serve reads no workspace files, so it
/// checks as `zygo config check` does with no read ([`RepositoryLoadout::NotRead`]; workspace definition design R§10,
/// FX-29): at start and at reload, a serve agrees with the `config check` that its refusal names.
fn checked(load: &(dyn Fn() -> anyhow::Result<Merged> + Send + Sync), file: &Path) -> Result<Merged, ConfigInvalid> {
    let invalid = || ConfigInvalid { file: file.to_path_buf() };
    let m = load().map_err(|_| invalid())?;
    if check_with(&m.config, RepositoryLoadout::NotRead).iter().any(|v| v.severity == Severity::Error) {
        return Err(invalid());
    }
    Ok(m)
}

fn served(m: Merged) -> Served {
    let team: Arc<dyn crate::session::TeamAuth> = Arc::new(Tokens::new(&m.config.serve.tokens));
    Served { known_plugins: team::known_plugins(&m.config), team: Some(team), cfg: Arc::new(m) }
}

/// Usage rows past `retain_days` (§14); an error is ignored (the usage writer fails open).
fn prune(db: Option<&Path>, retain_days: u32) {
    if let Some(Ok(s)) = db.map(Store::open) {
        let _ = s.prune(retain_days);
    }
}

/// Starts the server: C8 on every resolved address, config and check, pid lock, then the steps that change state
/// (prune, usage sink, signals, bind), so a refused second instance changes nothing (N16 round 2).
pub fn start(opts: ServeOpts) -> anyhow::Result<ServeHandle> {
    let ServeOpts { listen, tls, config_file, load, report, pid_file, signals, start } = opts;
    let addrs: Vec<SocketAddr> = listen.to_socket_addrs().with_context(|| format!("listen address {listen}"))?.collect();
    if tls.is_none() && addrs.iter().any(|a| !a.ip().to_canonical().is_loopback()) {
        return Err(TlsRequired.into());
    }
    let m = checked(&*load, &config_file)?;
    let pid = pid_file.map(|p| lock_pid(&p).map(|f| (p, f))).transpose()?;
    prune(start.usage_db.as_deref(), m.config.usage.retain_days);
    #[cfg(feature = "test-hooks")]
    let (header_read, body_read_idle, max_connections) = (start.header_read, start.body_read_idle, start.max_connections);
    #[cfg(not(feature = "test-hooks"))]
    let (header_read, body_read_idle, max_connections) = (HEADER_READ, BODY_READ_IDLE, MAX_CONNECTIONS);
    let app = server::app(served(m), start, Some(body_read_idle))?;

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().context("serve runtime")?;
    let tls = match tls {
        Some(t) => {
            let c = rt.block_on(axum_server::tls_rustls::RustlsConfig::from_pem_file(&t.cert, &t.key)).context("TLS certificate or key")?;
            // HTTP/1.1 only (§7 note 2026-11-19, N60 review): ALPN offers no `h2`.
            let mut sc = (*c.get_inner()).clone();
            sc.alpn_protocols = vec![b"http/1.1".to_vec()];
            c.reload_from_config(Arc::new(sc));
            Some(c)
        }
        None => None,
    };
    let (tx, rx) = mpsc::channel();
    if signals {
        install_signals(&rt, &tx).context("signal handlers")?;
    }

    #[cfg(unix)]
    if let Err(e) = raise_open_files() {
        report(&format!("zygo serve: open-file limit not raised ({e})"));
    }
    let std_listener = std::net::TcpListener::bind(&addrs[..]).with_context(|| format!("bind {listen}"))?;
    std_listener.set_nonblocking(true)?;
    let bound = std_listener.local_addr()?;
    let url = Url::parse(&format!("{}://{bound}/", if tls.is_some() { "https" } else { "http" }))?;
    let router = routes::router(app.clone());
    let server: Pin<Box<dyn Future<Output = std::io::Result<()>> + Send>> = {
        let _guard = rt.enter();
        let limited = Limited { inner: tokio::net::TcpListener::from_std(std_listener)?, permits: Arc::new(Semaphore::new(max_connections)) };
        // HTTP/1.1 only (§7 note 2026-11-19, N60 review), so the http1 bound covers each later request head and an idle
        // kept-open connection; the first-request bound covers the first.
        let mut srv = axum_server::Server::<Peer>::from_listener(limited).http1_only().first_request_timeout(header_read);
        srv.http_builder().http1().header_read_timeout(header_read);
        let mk = router.into_make_service();
        match tls {
            Some(c) => Box::pin(srv.acceptor(RustlsAcceptor::new(c)).serve(mk)),
            None => Box::pin(srv.serve(mk)),
        }
    };
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::Builder::new().name("zygo-serve".into()).spawn(move || {
        rt.block_on(async move {
            tokio::select! {
                // An accept error ends the server; requests then fail as unreachable.
                _ = server => {}
                _ = stop_rx => {}
            }
        });
        // No graceful drain (N10 review): open connections close at once.
        drop(rt);
    })?;
    report(&format!("zygo serve: listening on {url}"));
    let reloads = (Mutex::new(0), Condvar::new());
    Ok(ServeHandle { url, app, stop, thread, tx, rx: Mutex::new(rx), config_file, load, report, pid, reloads })
}

/// Raises the soft open-file limit to the hard limit, as Go's runtime does, so `MAX_CONNECTIONS` clients and their
/// upstream sockets fit (spec §7 note 2026-11-19, N60 review). It never lowers the limit.
#[cfg(unix)]
fn raise_open_files() -> io::Result<()> {
    let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `l` is a valid rlimit for the call to write.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(target_os = "macos")]
    let max = if l.rlim_max == libc::RLIM_INFINITY { OPEN_MAX } else { l.rlim_max };
    #[cfg(not(target_os = "macos"))]
    let max = l.rlim_max;
    if l.rlim_cur >= max {
        return Ok(());
    }
    l.rlim_cur = max;
    // SAFETY: `l` is a valid rlimit.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &l) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The `axum_server::Address` of the limited listener; no route reads the peer address, so it is not kept.
#[derive(Clone)]
struct Peer;

/// A listener that takes a connection permit before each accept, so a connection over the limit waits in the
/// kernel accept queue (§7 note: `MAX_CONNECTIONS`).
struct Limited {
    inner: tokio::net::TcpListener,
    permits: Arc<Semaphore>,
}

/// An accepted stream; dropping it releases its permit.
struct Held {
    io: tokio::net::TcpStream,
    _permit: OwnedSemaphorePermit,
}

impl axum_server::Address for Peer {
    type Stream = Held;
    type Listener = Limited;
}

impl AddrListener<Held, Peer> for Limited {
    async fn bind_to(_: Peer) -> io::Result<Self> {
        Err(io::Error::other("bound by serve::start"))
    }

    async fn accept_stream(&self) -> io::Result<(Held, Peer)> {
        let permit = self.permits.clone().acquire_owned().await.map_err(io::Error::other)?;
        let (io, _) = self.inner.accept().await?;
        Ok((Held { io, _permit: permit }, Peer))
    }

    fn get_local_addr(&self) -> io::Result<Peer> {
        self.inner.local_addr().map(|_| Peer)
    }
}

impl AsyncRead for Held {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for Held {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_write_vectored(mut self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[io::IoSlice<'_>]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

/// Registers each handler before it returns, then forwards the signals into `tx`.
#[cfg(unix)]
fn install_signals(rt: &tokio::runtime::Runtime, tx: &mpsc::Sender<ServeEvent>) -> std::io::Result<()> {
    use tokio::signal::unix::{signal, SignalKind};
    let _guard = rt.enter();
    for (kind, ev) in [
        (SignalKind::hangup(), ServeEvent::Reload),
        (SignalKind::interrupt(), ServeEvent::Stop),
        (SignalKind::terminate(), ServeEvent::Stop),
    ] {
        let mut s = signal(kind)?;
        let tx = tx.clone();
        rt.spawn(async move {
            while s.recv().await.is_some() {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn install_signals(rt: &tokio::runtime::Runtime, tx: &mpsc::Sender<ServeEvent>) -> std::io::Result<()> {
    let _guard = rt.enter();
    let mut s = tokio::signal::windows::ctrl_c()?;
    let tx = tx.clone();
    rt.spawn(async move {
        while s.recv().await.is_some() {
            if tx.send(ServeEvent::Stop).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// D6: opens `p` (create, no truncate; unix: no symlink followed), refuses anything but a regular file, takes its
/// lock or refuses with `AlreadyRunning` (the file untouched), then writes `"{pid}\n"`.
fn lock_pid(p: &Path) -> anyhow::Result<File> {
    if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).with_context(|| d.display().to_string())?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW);
    let mut f = o.open(p).with_context(|| format!("pid file {}", p.display()))?;
    if !f.metadata().with_context(|| p.display().to_string())?.is_file() {
        anyhow::bail!("pid file {}: not a regular file", p.display());
    }
    match fs4::FileExt::try_lock(&f) {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(AlreadyRunning.into()),
        Err(TryLockError::Error(e)) => return Err(e).with_context(|| p.display().to_string()),
    }
    f.set_len(0).and_then(|()| f.write_all(format!("{}\n", std::process::id()).as_bytes())).with_context(|| p.display().to_string())?;
    Ok(f)
}

impl ServeHandle {
    /// `http://` or `https://` and the bound address.
    pub fn url(&self) -> Url {
        self.url.clone()
    }

    /// Reads the config again and runs `config check` (§7.2 N16 note). On an error the running config stays. On
    /// success one swap replaces the tokens, the config and the known plugins; the sessions of a removed label end;
    /// old usage rows are pruned.
    pub fn reload(&self) -> anyhow::Result<()> {
        let r = match checked(&*self.load, &self.config_file) {
            Err(e) => {
                (self.report)(&format!("zygo serve: reload failed ({}); run zygo config check", self.config_file.display()));
                Err(e.into())
            }
            Ok(m) => {
                let keep: BTreeSet<String> = m.config.serve.tokens.iter().map(|t| t.label.clone()).collect();
                let retain_days = m.config.usage.retain_days;
                self.app.swap(served(m));
                self.app.sessions.retain_labels(&keep);
                prune(self.app.usage_db.as_deref(), retain_days);
                (self.report)("zygo serve: config reloaded");
                Ok(())
            }
        };
        *self.reloads.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.reloads.1.notify_all();
        r
    }

    pub fn sender(&self) -> mpsc::Sender<ServeEvent> {
        self.tx.clone()
    }

    /// Handles the events: `Reload` → `reload`; `Stop` or no sender left → returns.
    pub fn events(&self) {
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        while let Ok(ServeEvent::Reload) = rx.recv() {
            let _ = self.reload();
        }
    }

    /// Stops at once (open connections close), writes the queued usage rows, then removes the pid file when it
    /// still holds this process's pid.
    pub fn shutdown(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
        self.app.usage.close();
        if let Some((p, mut f)) = self.pid {
            let mut s = String::new();
            if f.rewind().and_then(|()| f.read_to_string(&mut s)).is_ok() && s == format!("{}\n", std::process::id()) {
                let _ = std::fs::remove_file(&p);
            }
            drop(f);
        }
    }

    /// True once `n` reloads were tried, false after `limit`.
    #[cfg(feature = "test-hooks")]
    pub fn wait_reloads(&self, n: u64, limit: Duration) -> bool {
        let g = self.reloads.0.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self.reloads.1.wait_timeout_while(g, limit, |c| *c < n).unwrap_or_else(|e| e.into_inner());
        *g >= n
    }

    /// True once `n` usage rows were handled (inserted or dropped), false after `limit`.
    #[cfg(feature = "test-hooks")]
    pub fn usage_written(&self, n: u64, limit: Duration) -> bool {
        self.app.usage.wait(n, limit)
    }
}

/// `zygo serve`: start, handle the events, stop.
pub fn run(opts: ServeOpts) -> anyhow::Result<()> {
    let h = start(opts)?;
    h.events();
    h.shutdown();
    Ok(())
}

/// `zygo serve reload`: signals the server only when one holds the pid-file lock (D6); else `NoServer`.
pub fn reload_running(pid_file: &Path) -> anyhow::Result<u32> {
    let mut f = match File::open(pid_file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(NoServer.into()),
        Err(e) => return Err(e).with_context(|| pid_file.display().to_string()),
    };
    match fs4::FileExt::try_lock(&f) {
        // Dropping `f` unlocks.
        Ok(()) => Err(NoServer.into()),
        Err(TryLockError::WouldBlock) => {
            // Windows: the locked file cannot be read, and `signal_reload` refuses anyway.
            let pid = if cfg!(unix) { read_pid(&mut f).with_context(|| pid_file.display().to_string())? } else { 0 };
            signal_reload(pid)?;
            Ok(pid)
        }
        Err(TryLockError::Error(e)) => Err(e).with_context(|| pid_file.display().to_string()),
    }
}

/// A positive pid (0 would signal the whole process group).
fn read_pid(f: &mut File) -> anyhow::Result<u32> {
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    s.trim().parse::<std::num::NonZeroU32>().map(|p| p.get()).map_err(|_| anyhow::anyhow!("no pid in the pid file"))
}

/// SIGHUP to `pid` (unix); Windows has no SIGHUP.
pub fn signal_reload(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid = i32::try_from(pid).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "pid out of range"))?;
        // SAFETY: kill takes no pointer.
        if unsafe { libc::kill(pid, libc::SIGHUP) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "not available on Windows; restart zygo serve"))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn nofile() -> libc::rlimit {
        let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: `l` is a valid rlimit for the call to write.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) }, 0);
        l
    }

    fn set_nofile(l: &libc::rlimit) {
        // SAFETY: `l` is a valid rlimit.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, l) }, 0);
    }

    #[test]
    fn open_file_soft_limit_is_raised_to_the_hard_limit() { // §7 note 2026-11-19 (N60 review)
        let before = nofile();
        // A low soft limit first, so the raise shows on a host whose soft limit is already high.
        let low = libc::rlimit { rlim_cur: before.rlim_cur.min(256), ..before };
        set_nofile(&low);
        raise_open_files().unwrap();
        let after = nofile();
        set_nofile(&before);
        #[cfg(target_os = "macos")]
        let want = if before.rlim_max == libc::RLIM_INFINITY { OPEN_MAX } else { before.rlim_max };
        #[cfg(not(target_os = "macos"))]
        let want = before.rlim_max;
        assert_eq!(after.rlim_cur, want.max(low.rlim_cur));
    } // mutation: no `setrlimit` call (the soft limit stays at 256)
}
