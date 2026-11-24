use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use zygo_config::Config;
use zygo_core::paths::Paths;

/// The release repository.
const RELEASE_REPO: &str = "act101-ai/zygo";

/// Version rule shared with `scripts/install.sh` and `scripts/install.ps1`.
const VERSION_RULE: &str = r"^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$";

/// `zygo update [--version <v>] [--check]` (§17). Exit 0, 1 or 2.
pub fn run(version: Option<&str>, check: bool) -> i32 {
    match run_inner(version, check) {
        Ok(()) => 0,
        Err((code, msg)) => {
            super::print_err(&format!("zygo update: {msg}\n"));
            code
        }
    }
}

/// An exit code and the error line (printed after `zygo update: `).
pub type Failure = (i32, String);

fn fail<T>(msg: String) -> Result<T, Failure> {
    Err((1, msg))
}

/// This binary's version.
const CUR: &str = env!("CARGO_PKG_VERSION");

/// A runtime and a client for the release downloads. The runtime ends in the background: a name lookup that hangs
/// in its blocking thread does not hold the caller past the request's time limit.
struct Http {
    rt: Option<tokio::runtime::Runtime>,
    client: reqwest::Client,
}

impl Http {
    /// `total` bounds a whole request (connect, send and body); `None` leaves only `connect` and `read`.
    fn new(connect: Duration, read: Duration, total: Option<Duration>) -> Result<Http, Failure> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| (1, format!("cannot start runtime: {e}")))?;
        let client = match total {
            None => build_client(connect, read),
            Some(t) => build(client_builder(connect, read).timeout(t)),
        }
        .map_err(|e| (1, e))?;
        Ok(Http { rt: Some(rt), client })
    }

    fn get(&self, url: &str) -> Result<Vec<u8>, Failure> {
        let rt = self.rt.as_ref().ok_or_else(|| (1, "the runtime has ended".to_string()))?;
        fetch(&self.client, rt, url).map_err(|e| (1, e))
    }
}

impl Drop for Http {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

/// `v` without a leading `v`, and parsed; `None` when it breaks the version rule.
fn parsed(v: &str) -> Option<(String, semver::Version)> {
    parse_version(v).and_then(|s| semver::Version::parse(&s).ok().map(|p| (s, p)))
}

/// The version in the release's public VERSION file: the one request of a check.
fn latest(http: &Http, base: &str) -> Result<(String, semver::Version), Failure> {
    let url = format!("{base}/latest/download/VERSION");
    let body = http.get(&url)?;
    let text = String::from_utf8_lossy(&body);
    match parsed(text.trim()) {
        Some(x) => Ok(x),
        None => fail(format!("bad VERSION from {url}")),
    }
}

fn run_inner(version: Option<&str>, check: bool) -> Result<(), Failure> {
    let cur_str = CUR;
    let cur = semver::Version::parse(cur_str).map_err(|e| (1, format!("bad own version {cur_str}: {e}")))?;
    let explicit = match version {
        Some(v) => match parsed(v) {
            Some(x) => Some(x),
            None => return Err((2, format!("bad --version {v}"))),
        },
        None => None,
    };

    let base = release_base(&|k| std::env::var_os(k));
    let (target_str, target) = match explicit {
        Some(x) => x,
        None => latest(&Http::new(CONNECT_TIMEOUT, READ_TIMEOUT, None)?, &base)?,
    };
    // Build metadata does not count (semver precedence): `0.1.0+x` against `0.1.0` is up to date.
    let order = target.cmp_precedence(&cur);
    let newer = if version.is_some() { order.is_ne() } else { order.is_gt() };

    if check {
        if newer {
            super::print_out(&format!("update available: {cur_str} -> {target_str}\n"));
        } else {
            super::print_out(&format!("zygo {cur_str} is up to date\n"));
        }
        return Ok(());
    }
    if !newer {
        super::print_out(&format!("zygo {cur_str} is up to date\n"));
        return Ok(());
    }

    let exe = std::env::current_exe().and_then(std::fs::canonicalize).map_err(|e| (1, format!("cannot locate the running binary: {e}")))?;
    let installed = install(&exe, &base, Some(&target_str))?;
    super::print_out(&format!("updated zygo {cur_str} -> {installed}\n"));
    Ok(())
}

/// Downloads the release `version` (`None`: the latest release) of this platform from `release_base`, verifies its
/// SHA-256 checksum and replaces `target` with it atomically. Returns the installed version, without a leading `v`.
/// `target` is unchanged on every error. The system proxy is not used (N36).
pub fn install(target: &Path, release_base: &str, version: Option<&str>) -> Result<String, Failure> {
    let http = Http::new(CONNECT_TIMEOUT, READ_TIMEOUT, None)?;
    let version = match version {
        Some(v) => match parsed(v) {
            Some((s, _)) => s,
            None => return Err((2, format!("bad version {v}"))),
        },
        None => latest(&http, release_base)?.0,
    };
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let Some(name) = asset_name(os, arch) else {
        return fail(format!("unsupported platform {os}/{arch}"));
    };
    let dir = format!("{release_base}/download/v{version}");
    let sums = http.get(&format!("{dir}/SHA256SUMS"))?;
    let sums = String::from_utf8_lossy(&sums).to_string();
    let Some(expected) = find_sum(&sums, &name) else {
        return fail(format!("no checksum for {name}"));
    };
    let bytes = http.get(&format!("{dir}/{name}"))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if !sum_matches(expected, &actual) {
        return fail(format!("checksum mismatch for {name}"));
    }
    replace_binary(&bytes, target).map_err(|e| (1, format!("cannot replace {}: {e}", target.display())))?;
    Ok(version)
}

// ---- the update check at agent start (auto-update design) ----

/// The terminal of the start check: tests give one that is not a terminal.
pub trait PromptIo {
    /// stdin and stderr are both a terminal.
    fn is_tty(&self) -> bool;
    /// Shows `prompt` and returns the line the user typed.
    fn ask(&mut self, prompt: &str) -> std::io::Result<String>;
    /// Shows one line.
    fn say(&mut self, line: &str);
}

/// The process's terminal: prompts and lines go to stderr, the answer comes from stdin.
pub struct Terminal {
    assume_tty: bool,
}

impl Terminal {
    /// `assume_tty` counts stdin and stderr as a terminal whatever they are (`ZYGO_TEST_ASSUME_TTY=1`, read by the
    /// entry point under `test-hooks`).
    pub fn new(assume_tty: bool) -> Terminal {
        Terminal { assume_tty }
    }
}

impl PromptIo for Terminal {
    fn is_tty(&self) -> bool {
        use std::io::IsTerminal;
        self.assume_tty || (std::io::stdin().is_terminal() && std::io::stderr().is_terminal())
    }

    fn ask(&mut self, prompt: &str) -> std::io::Result<String> {
        super::print_err(&format!("{prompt} "));
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line)
    }

    fn say(&mut self, line: &str) {
        super::print_err(&format!("{line}\n"));
    }
}

/// What the caller of [`start_check`] does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// Start the agent.
    Continue,
    /// Do not start the agent; exit with `code`.
    Exit { code: i32 },
}

/// The least time between two checks, in seconds.
const CHECK_EVERY: u64 = 24 * 60 * 60;

/// The total time limit of the check.
const CHECK_LIMIT: Duration = Duration::from_secs(2);

/// The latest release at `base`, or `None` on any failure or when [`CHECK_LIMIT`] ends first. The request runs on its
/// own thread, so the limit also covers the client set-up and the name lookup; the request has the same limit, so
/// the thread ends by itself.
fn check_latest(base: String) -> Option<(String, semver::Version)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let found = Http::new(CHECK_LIMIT, CHECK_LIMIT, Some(CHECK_LIMIT)).and_then(|h| latest(&h, &base));
            let _ = tx.send(found.ok());
        })
        .ok()?;
    rx.recv_timeout(CHECK_LIMIT).ok().flatten()
}

/// `update.json` in the cache directory: `checked_at` (Unix seconds of the last check, failed or not), `latest` (the
/// version that check found, if it found one) and `skip` (the version the user answered `s` to).
#[derive(Default)]
struct Cache {
    checked_at: Option<u64>,
    latest: Option<String>,
    skip: Option<String>,
}

impl Cache {
    /// A missing or unreadable file is an empty cache.
    fn read(file: &Path) -> Cache {
        let Some(v) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
            return Cache::default();
        };
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        Cache { checked_at: v.get("checked_at").and_then(|x| x.as_u64()), latest: text("latest"), skip: text("skip") }
    }

    /// Atomic: a temporary file next to `file`, then a rename. Every error is ignored (the check stays silent).
    fn write(&self, file: &Path) {
        use std::io::Write;
        let Some(dir) = file.parent() else { return };
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        if b.create(dir).is_err() {
            return;
        }
        let body = serde_json::json!({ "checked_at": self.checked_at, "latest": self.latest, "skip": self.skip });
        let tmp = dir.join(format!(".update.json.{}", std::process::id()));
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let written = o.open(&tmp).and_then(|mut f| {
            f.write_all(body.to_string().as_bytes())?;
            f.sync_all()
        });
        if written.and_then(|()| std::fs::rename(&tmp, file)).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// The number of live sessions under `sessions_dir` ([`zygo_core::session_dir::live`]).
fn live_sessions(sessions_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else { return 0 };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && zygo_core::session_dir::live(&e.path()))
        .count()
}

/// The update check before an agent launch (auto-update design). `args` are the arguments passed to the agent, `env`
/// reads the environment, `exe` is the binary an install replaces.
///
/// No request and no prompt when: `io` is not a terminal; `-p` or `--print` is in `args`; `ZYGO_SESSION` or
/// `ZYGO_WORKSPACE` is set, an empty value too (the AU64 and AU95 meaning, `cmd/mod.rs:112-116`);
/// `ZYGO_NO_UPDATE_CHECK` is set and not empty; `[update] check = false`; or the last check is less than 24 h old.
/// Else one GET for the release's public VERSION file, bounded by 2 s, the system proxy not used; a failure is silent. A newer version that was not skipped is asked about: `y` installs it and returns `Exit`, `s`
/// records the skip, any other answer continues.
pub fn start_check(
    paths: &Paths,
    cfg: &Config,
    args: &[OsString],
    env: &dyn Fn(&str) -> Option<OsString>,
    exe: &Path,
    io: &mut dyn PromptIo,
) -> StartOutcome {
    // A marker of a zygo session or a workspace pane counts as set with any value, an empty one too; the opt-out
    // needs a value.
    let marked = |k: &str| env(k).is_some();
    let opted_out = env("ZYGO_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty());
    if !io.is_tty()
        || args.iter().any(|a| a.as_os_str() == "-p" || a.as_os_str() == "--print")
        || marked("ZYGO_SESSION")
        || marked("ZYGO_WORKSPACE")
        || opted_out
        || !cfg.update.check
    {
        return StartOutcome::Continue;
    }
    let file = paths.cache_dir.join("update.json");
    let mut cache = Cache::read(&file);
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()) else {
        return StartOutcome::Continue;
    };
    // A time in the future (the clock went back) does not count as a check.
    if cache.checked_at.is_some_and(|t| t <= now && now - t < CHECK_EVERY) {
        return StartOutcome::Continue;
    }

    let base = release_base(env);
    let found = check_latest(base.clone());
    cache.checked_at = Some(now);
    if let Some((v, _)) = &found {
        cache.latest = Some(v.clone());
    }
    cache.write(&file);
    let Some((new, new_v)) = found else { return StartOutcome::Continue };
    let Ok(cur) = semver::Version::parse(CUR) else { return StartOutcome::Continue };
    if !new_v.cmp_precedence(&cur).is_gt() {
        return StartOutcome::Continue;
    }
    let skipped = cache.skip.as_deref().and_then(|s| semver::Version::parse(s).ok());
    if skipped.is_some_and(|s| !new_v.cmp_precedence(&s).is_gt()) {
        return StartOutcome::Continue;
    }

    let others = live_sessions(&paths.sessions_dir);
    if others > 0 {
        io.say(&format!(
            "{others} zygo sessions are running; they keep the old version until you restart them (zygo down / zygo up)"
        ));
        // The locked `.exe.old` of a running session makes a second update fail: the update is deferred.
        if cfg!(windows) {
            return StartOutcome::Continue;
        }
    }
    let answer = io.ask(&format!("zygo {new} is available (you have {CUR}). Update now? [y/N/s]")).unwrap_or_default();
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => match install(exe, &base, Some(&new)) {
            Ok(v) => {
                io.say(&format!("updated to {v}; relaunch zygo"));
                StartOutcome::Exit { code: 0 }
            }
            Err((code, msg)) => {
                io.say(&format!("zygo update: {msg}"));
                StartOutcome::Exit { code }
            }
        },
        "s" | "skip" => {
            cache.skip = Some(new);
            cache.write(&file);
            StartOutcome::Continue
        }
        _ => StartOutcome::Continue,
    }
}

fn asset_name(os: &str, arch: &str) -> Option<String> {
    let triple = match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("windows", "x86_64") => return Some("zygo-x86_64-pc-windows-msvc.exe".to_string()),
        ("windows", "aarch64") => return Some("zygo-aarch64-pc-windows-msvc.exe".to_string()),
        _ => return None,
    };
    Some(format!("zygo-{triple}"))
}

/// The digest of the first line whose name is `name` (a leading `*` on the name is ignored).
fn find_sum<'a>(sums: &'a str, name: &str) -> Option<&'a str> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let digest = it.next()?;
        let file = it.next()?;
        (file.strip_prefix('*').unwrap_or(file) == name).then_some(digest)
    })
}

fn sum_matches(expected: &str, actual: &str) -> bool {
    expected.eq_ignore_ascii_case(actual)
}

/// The version without a leading `v`, or `None` when it breaks the version rule.
fn parse_version(s: &str) -> Option<String> {
    let re = regex::Regex::new(VERSION_RULE).ok()?;
    re.is_match(s).then(|| s.strip_prefix('v').unwrap_or(s).to_string())
}

/// The release base URL. Under `test-hooks`, `ZYGO_RELEASE_BASE` (read through `env`) replaces it.
pub fn release_base(env: &dyn Fn(&str) -> Option<OsString>) -> String {
    #[cfg(feature = "test-hooks")]
    if let Some(b) = env("ZYGO_RELEASE_BASE").and_then(|b| b.into_string().ok()) {
        return b;
    }
    #[cfg(not(feature = "test-hooks"))]
    let _ = env;
    format!("https://github.com/{RELEASE_REPO}/releases")
}

/// Spec §7 note 2026-11-19 (owner: follow sensible standards); connect as `serve_client::CONNECT_TIMEOUT`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Idle time between bytes; equals `serve_client::CALL_TIMEOUT`, which bounds a whole call there and each read here.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

fn client_builder(connect: Duration, read: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder().no_proxy().connect_timeout(connect).read_timeout(read)
}

/// Installs the rustls `ring` provider (once; an earlier install is not an error), so every caller is safe.
fn build_client(connect: Duration, read: Duration) -> Result<reqwest::Client, String> {
    build(client_builder(connect, read))
}

fn build(b: reqwest::ClientBuilder) -> Result<reqwest::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    b.build().map_err(|e| format!("http client: {e}"))
}

fn fetch(client: &reqwest::Client, rt: &tokio::runtime::Runtime, url: &str) -> Result<Vec<u8>, String> {
    rt.block_on(async {
        let resp = client.get(url).send().await.map_err(|e| format!("cannot download {url}: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("cannot download {url}: HTTP {status}"));
        }
        resp.bytes().await.map(|b| b.to_vec()).map_err(|e| format!("cannot download {url}: {e}"))
    })
}

fn temp_path(target: &Path, pid: u32) -> PathBuf {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!(".zygo-update-{pid}"))
}

/// Writes `new` next to `target` (never following a planted file or link), then renames it over `target`. On Windows
/// a running `.exe` cannot be replaced, so it is renamed to `.old` first and renamed back if the second rename fails.
fn replace_binary(new: &[u8], target: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = temp_path(target, std::process::id());
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let written = (|| -> std::io::Result<()> {
        f.write_all(new)?;
        f.sync_all()?;
        // The new binary takes the mode of the binary it replaces.
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::fs::metadata(target)?.permissions())?;
        Ok(())
    })();
    drop(f);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let swapped = swap_in(&tmp, target);
    if swapped.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    swapped
}

#[cfg(not(windows))]
fn swap_in(tmp: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(tmp, target)
}

/// Windows error 32 (`ERROR_SHARING_VIOLATION`).
#[cfg(windows)]
const SHARING_VIOLATION: i32 = 32;

/// How long a rename retries a sharing violation. A real-time scanner opens a freshly started or written `.exe` for
/// about a second (measured on Windows 11 with Defender: 35 MB binary, renames refused from 22 ms to 1062 ms after
/// launch), so one attempt is not enough.
#[cfg(windows)]
const SHARING_RETRY: std::time::Duration = std::time::Duration::from_secs(10);

/// `std::fs::rename`, retried while another process holds `from` without delete sharing; the error names `what`.
#[cfg(windows)]
fn rename_retrying(from: &Path, to: &Path, what: &str) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + SHARING_RETRY;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.raw_os_error() == Some(SHARING_VIOLATION) && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(std::io::Error::new(e.kind(), format!("{what}: {e}"))),
        }
    }
}

#[cfg(windows)]
fn swap_in(tmp: &Path, target: &Path) -> std::io::Result<()> {
    let mut old = target.as_os_str().to_owned();
    old.push(".old");
    let old = PathBuf::from(old);
    let _ = std::fs::remove_file(&old);
    rename_retrying(target, &old, "moving it aside")?;
    if let Err(e) = rename_retrying(tmp, target, "moving the new binary in") {
        let _ = rename_retrying(&old, target, "restoring it");
        return Err(e);
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    /// Serves one connection: writes `head`, then `body`, then holds the socket `hold_ms`. Returns the URL.
    fn serve_once(head: &'static str, body: &'static [u8], hold_ms: u64) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = l.accept() else { return };
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(body);
            let _ = s.flush();
            std::thread::sleep(Duration::from_millis(hold_ms));
        });
        format!("http://127.0.0.1:{port}/f")
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    #[ignore = "timing: asserts elapsed time of a 300 ms read timeout"]
    fn fetch_ends_a_stalled_read_after_the_read_timeout() {
        let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n", b"abc", 4000);
        let c = build_client(Duration::from_secs(5), Duration::from_millis(300)).unwrap();
        let t = Instant::now();
        let e = fetch(&c, &rt(), &url).unwrap_err();
        assert!(e.starts_with(&format!("cannot download {url}: ")), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[test]
    #[ignore = "timing: the server thread must answer inside a 300 ms read timeout"]
    fn fetch_still_reads_a_moving_body_whole() {
        let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n", b"abcd", 100);
        let c = build_client(Duration::from_secs(5), Duration::from_millis(300)).unwrap();
        assert_eq!(fetch(&c, &rt(), &url).unwrap(), b"abcd");
    }

    #[test] // reqwest prints `connect_timeout` in the Debug of a ClientBuilder (async_impl/client.rs, `Config::fmt_fields`)
    fn client_builder_carries_the_connect_timeout() {
        let b = client_builder(Duration::from_millis(1234), Duration::from_millis(5678));
        let d = format!("{b:?}");
        assert!(d.contains("connect_timeout: 1.234s"), "{d}");
    }

    #[test]
    fn timeouts_are_the_decided_values() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(READ_TIMEOUT, Duration::from_secs(30));
    }


    #[test]
    fn asset_name_covers_the_six_targets() {
        let cases = [
            ("macos", "aarch64", Some("zygo-aarch64-apple-darwin")),
            ("macos", "x86_64", Some("zygo-x86_64-apple-darwin")),
            ("linux", "aarch64", Some("zygo-aarch64-unknown-linux-musl")),
            ("linux", "x86_64", Some("zygo-x86_64-unknown-linux-musl")),
            ("windows", "x86_64", Some("zygo-x86_64-pc-windows-msvc.exe")),
            ("windows", "aarch64", Some("zygo-aarch64-pc-windows-msvc.exe")),
            ("freebsd", "x86_64", None),
        ];
        for (os, arch, want) in cases {
            assert_eq!(asset_name(os, arch).as_deref(), want, "{os}/{arch}");
        }
    }

    #[test]
    fn find_sum_first_match_wins_and_ignores_case() {
        assert_eq!(find_sum("AA  zygo-a\nbb  zygo-a\n", "zygo-a"), Some("AA"));
        assert_eq!(find_sum("aa *zygo-a\n", "zygo-a"), Some("aa"));
        assert_eq!(find_sum("aa  zygo-a.exe\n", "zygo-a"), None);
        assert!(sum_matches("AA", "aa"));
        assert!(!sum_matches("aa", "ab"));
    }

    #[test]
    fn version_rule() {
        for ok in ["1.2.3", "v1.2.3", "1.2.3-rc.1", "1.2.3+b.1"] {
            assert_eq!(parse_version(ok).as_deref(), Some(ok.trim_start_matches('v')), "{ok}");
        }
        for bad in ["1.2", "abc", "1.2.3.4", "1.2.3-", "../x", ""] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn temp_file_is_created_with_create_new() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let victim = dir.path().join("victim");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&victim, b"victim-bytes").unwrap();
        let tmp = temp_path(&target, std::process::id());

        // A planted regular file.
        std::fs::write(&tmp, b"planted").unwrap();
        assert!(replace_binary(b"new", &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read(&tmp).unwrap(), b"planted");
        std::fs::remove_file(&tmp).unwrap();

        // A planted symlink to another file.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&victim, &tmp).unwrap();
            assert!(replace_binary(b"new", &target).is_err());
            assert_eq!(std::fs::read(&victim).unwrap(), b"victim-bytes");
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
        }
    }

    /// A file held open without delete sharing (as a scanner holds a new `.exe`) is renamed once it is released.
    #[cfg(windows)]
    #[test]
    #[ignore = "timing: a 500 ms sleep races the retry deadline; asserts elapsed time"]
    fn rename_waits_out_a_sharing_violation() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tempfile::tempdir().unwrap();
        let (from, to) = (d.path().join("zygo.exe"), d.path().join("zygo.exe.old"));
        std::fs::write(&from, b"bin").unwrap();
        // FILE_SHARE_READ only: a rename needs delete sharing.
        let held = std::fs::OpenOptions::new().read(true).share_mode(0x1).open(&from).unwrap();
        assert_eq!(std::fs::rename(&from, &to).unwrap_err().raw_os_error(), Some(SHARING_VIOLATION));
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            drop(held);
        });
        let t0 = Instant::now();
        rename_retrying(&from, &to, "moving it aside").unwrap();
        assert!(t0.elapsed() >= Duration::from_millis(300), "{:?}", t0.elapsed());
        release.join().unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"bin");
    }
}
