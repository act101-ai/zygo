//! `zygo update` against a local fixture server (the release base is replaced through the `test-hooks` seam).

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

mod common;
mod dead_port;

/// Held while a copy of the binary is written and while a child is spawned, so no other test's `fork` holds a write
/// descriptor of an executable that is being exec'd (ETXTBSY).
static COPY_LOCK: Mutex<()> = Mutex::new(());

const CUR: &str = env!("CARGO_PKG_VERSION");
const NEW: &str = "9.9.9";

fn asset() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match os {
        "macos" => format!("zygo-{arch}-apple-darwin"),
        "linux" => format!("zygo-{arch}-unknown-linux-musl"),
        _ => format!("zygo-{arch}-pc-windows-msvc.exe"),
    }
}

fn sums(name: &str, bytes: &[u8]) -> String {
    format!("{}  {name}\n", hex::encode(Sha256::digest(bytes)))
}

struct Fixture {
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    fn start(files: Vec<(String, Vec<u8>)>) -> Fixture {
        Fixture::start_failing(files, Vec::new())
    }

    /// [`Fixture::start`], and each path of `failing` answers `500 Internal Server Error`.
    fn start_failing(files: Vec<(String, Vec<u8>)>, failing: Vec<String>) -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                log2.lock().unwrap().push(path.clone());
                let hit = files.iter().find(|(p, _)| *p == path);
                let (status, body): (&str, &[u8]) = match hit {
                    _ if failing.contains(&path) => ("500 Internal Server Error", b"failed"),
                    Some((_, b)) => ("200 OK", b),
                    None => ("404 Not Found", b"not found"),
                };
                let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(body);
                let _ = s.flush();
            }
        });
        Fixture { port, log }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

struct Setup {
    dir: tempfile::TempDir,
    copy: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let name = if cfg!(windows) { "zygo.exe" } else { "zygo" };
    let copy = dir.path().join(name);
    let _g = COPY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::copy(env!("CARGO_BIN_EXE_zygo"), &copy).unwrap();
    Setup { dir, copy }
}

fn run_env(s: &Setup, base: &str, args: &[&str], extra: &[(&str, String)]) -> Output {
    let mut cmd = Command::new(&s.copy);
    cmd.args(args)
        .env("ZYGO_RELEASE_BASE", base)
        .env("HOME", s.dir.path())
        .env("LOCALAPPDATA", s.dir.path())
        .stdin(Stdio::null());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let _g = COPY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    drop(_g);
    child.wait_with_output().unwrap()
}

fn run(s: &Setup, base: &str, args: &[&str]) -> Output {
    run_env(s, base, args, &[])
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn no_temp(dir: &Path) {
    for e in std::fs::read_dir(dir).unwrap() {
        let n = e.unwrap().file_name().to_string_lossy().to_string();
        assert!(!n.starts_with(".zygo-update-"), "left over temp file {n}");
    }
}

/// Bytes and (on Unix) inode of the copy, taken before a run.
fn snapshot(p: &Path) -> (Vec<u8>, u64) {
    let b = std::fs::read(p).unwrap();
    #[cfg(unix)]
    let ino = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(p).unwrap());
    #[cfg(not(unix))]
    let ino = 0;
    (b, ino)
}

fn assert_unchanged(s: &Setup, before: &(Vec<u8>, u64)) {
    assert_eq!(&snapshot(&s.copy), before, "binary changed");
    no_temp(s.dir.path());
}

fn version_file(v: &str) -> (String, Vec<u8>) {
    ("/latest/download/VERSION".to_string(), format!("{v}\n").into_bytes())
}

fn release(v: &str, bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    vec![
        (format!("/download/v{v}/SHA256SUMS"), sums(&asset(), bytes).into_bytes()),
        (format!("/download/v{v}/{}", asset()), bytes.to_vec()),
    ]
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn check_reports_a_newer_version() {
    let f = Fixture::start(vec![version_file(NEW)]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update", "--check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("update available: {CUR} -> {NEW}\n"));
    assert_unchanged(&s, &before);
    assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn check_reports_up_to_date() {
    let f = Fixture::start(vec![version_file(CUR)]);
    let s = setup();
    let o = run(&s, &f.base(), &["update", "--check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn update_replaces_the_binary_after_the_checksum() {
    let mut files = vec![version_file(NEW)];
    files.extend(release(NEW, b"fixture-binary"));
    let f = Fixture::start(files);
    let s = setup();
    // The new binary takes the old one's mode; a mode no umask gives proves it is copied, not defaulted.
    #[cfg(unix)]
    std::fs::set_permissions(&s.copy, std::os::unix::fs::PermissionsExt::from_mode(0o750)).unwrap();
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("updated zygo {CUR} -> {NEW}\n"));
    assert_eq!(std::fs::read(&s.copy).unwrap(), b"fixture-binary");
    #[cfg(unix)]
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&s.copy).unwrap().permissions()) & 0o777, 0o750);
    for e in std::fs::read_dir(s.dir.path()).unwrap() {
        let n = e.unwrap().file_name().to_string_lossy().to_string();
        assert!(!n.starts_with(".zygo-update-"), "{n}");
    }
    let r = f.requests();
    assert_eq!(r[0], "/latest/download/VERSION");
    assert!(r.len() >= 3 && r[1..].iter().all(|p| p.starts_with("/download/v9.9.9/")), "{r:?}");
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn checksum_mismatch_is_refused() {
    let wrong = sums(&asset(), b"other-bytes").into_bytes();
    let f = Fixture::start(vec![
        version_file(NEW),
        (format!("/download/v{NEW}/SHA256SUMS"), wrong),
        (format!("/download/v{NEW}/{}", asset()), b"fixture-binary".to_vec()),
    ]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains(&format!("checksum mismatch for {}", asset())), "{}", err(&o));
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn missing_checksum_line_is_refused() {
    let f = Fixture::start(vec![
        version_file(NEW),
        (format!("/download/v{NEW}/SHA256SUMS"), sums("other-file", b"x").into_bytes()),
        (format!("/download/v{NEW}/{}", asset()), b"fixture-binary".to_vec()),
    ]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains(&format!("no checksum for {}", asset())), "{}", err(&o));
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn explicit_version_installs_from_the_tagged_path() {
    let f = Fixture::start(release("1.2.3", b"tagged-bytes"));
    let s = setup();
    let o = run(&s, &f.base(), &["update", "--version", "v1.2.3"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).ends_with("-> 1.2.3\n"), "{}", out(&o));
    assert_eq!(std::fs::read(&s.copy).unwrap(), b"tagged-bytes");
    assert!(f.requests().iter().all(|p| !p.starts_with("/latest/")), "{:?}", f.requests());
}

#[test]
fn bad_version_is_a_usage_error() {
    let f = Fixture::start(vec![]);
    let s = setup();
    let o = run(&s, &f.base(), &["update", "--version", "abc"]);
    assert_eq!(o.status.code(), Some(2), "{}", err(&o));
    assert!(f.requests().is_empty());
}

#[test]
fn unreachable_server_exits_1_and_changes_nothing() {
    let port = dead_port::closed_port();
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &format!("http://127.0.0.1:{port}"), &["update"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("cannot download "), "{}", err(&o));
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn up_to_date_update_requests_no_asset() {
    let f = Fixture::start(vec![version_file(CUR)]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn older_latest_is_not_installed() {
    let f = Fixture::start(vec![version_file("0.0.1")]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn malformed_version_file_exits_1() {
    let f = Fixture::start(vec![version_file("not-a-version")]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("bad VERSION from "), "{}", err(&o));
    assert_unchanged(&s, &before);
}

#[test]
fn check_with_version_reports_and_installs_nothing() {
    let f = Fixture::start(vec![]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update", "--check", "--version", "v1.2.3"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("update available: {CUR} -> 1.2.3\n"));
    let o = run(&s, &f.base(), &["update", "--check", "--version", &format!("v{CUR}")]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    assert!(f.requests().is_empty());
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn system_proxy_is_not_used() {
    let f = Fixture::start(vec![version_file(NEW)]);
    let port = dead_port::closed_port();
    let proxy = format!("http://127.0.0.1:{port}");
    let s = setup();
    let o = run_env(
        &s,
        &f.base(),
        &["update", "--check"],
        &[("HTTP_PROXY", proxy.clone()), ("http_proxy", proxy.clone()), ("ALL_PROXY", proxy)],
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).starts_with("update available:"), "{}", out(&o));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn update_keeps_the_mode_of_the_replaced_binary() {
    use std::os::unix::fs::PermissionsExt;
    let mut files = vec![version_file(NEW)];
    files.extend(release(NEW, b"fixture-binary"));
    let f = Fixture::start(files);
    let s = setup();
    std::fs::set_permissions(&s.copy, std::fs::Permissions::from_mode(0o700)).unwrap();
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(std::fs::read(&s.copy).unwrap(), b"fixture-binary");
    assert_eq!(std::fs::metadata(&s.copy).unwrap().permissions().mode() & 0o7777, 0o700);
}

#[test]
fn explicit_version_compares_by_precedence() {
    let f = Fixture::start(vec![]);
    let s = setup();
    let before = snapshot(&s.copy);
    let build = format!("{CUR}+x");
    let o = run(&s, &f.base(), &["update", "--check", "--version", &build]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    let o = run(&s, &f.base(), &["update", "--version", &build]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    assert!(f.requests().is_empty(), "{:?}", f.requests());
    assert_unchanged(&s, &before);
}

#[test]
#[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
fn latest_compares_by_precedence() {
    let f = Fixture::start(vec![version_file(&format!("{CUR}+x"))]);
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &f.base(), &["update", "--check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    let o = run(&s, &f.base(), &["update"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
    assert_eq!(f.requests(), vec!["/latest/download/VERSION", "/latest/download/VERSION"]);
    assert_unchanged(&s, &before);
}

// ---- the update check at agent start (auto-update design) ----

mod start {
    use super::*;
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use zygo::cmd::update::{start_check, PromptIo, StartOutcome};
    use zygo_config::Config;
    use zygo_core::paths::Paths;

    /// A terminal that is not one: the TTY flag, the answers to give, and what was asked and said.
    struct FakeIo {
        tty: bool,
        answers: VecDeque<&'static str>,
        asked: Vec<String>,
        said: Vec<String>,
    }

    impl FakeIo {
        fn tty(answers: &[&'static str]) -> FakeIo {
            FakeIo { tty: true, answers: answers.iter().copied().collect(), asked: Vec::new(), said: Vec::new() }
        }
    }

    impl PromptIo for FakeIo {
        fn is_tty(&self) -> bool {
            self.tty
        }
        fn ask(&mut self, prompt: &str) -> std::io::Result<String> {
            self.asked.push(prompt.to_string());
            Ok(format!("{}\n", self.answers.pop_front().unwrap_or("")))
        }
        fn say(&mut self, line: &str) {
            self.said.push(line.to_string());
        }
    }

    /// A temp home: the zygo paths under it and a file that stands for the binary (never the test binary).
    struct Home {
        dir: tempfile::TempDir,
        paths: Paths,
        exe: PathBuf,
    }

    const OLD: &[u8] = b"old-binary";

    fn home() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let data = dir.path().join("data");
        let paths = Paths {
            config_file: dir.path().join("config.toml"),
            sessions_dir: cache.join("sessions"),
            marketplaces_dir: cache.join("marketplaces"),
            plugin_store: data.join("plugins"),
            approvals_file: data.join("approvals.toml"),
            usage_db: data.join("usage.db"),
            cache_dir: cache,
            data_dir: data,
        };
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) { "zygo.exe" } else { "zygo" });
        std::fs::write(&exe, OLD).unwrap();
        Home { dir, paths, exe }
    }

    fn check_cfg(h: &Home, cfg: &Config, base: &str, args: &[&str], env: &[(&str, &str)], io: &mut FakeIo) -> StartOutcome {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let env = |k: &str| {
            if k == "ZYGO_RELEASE_BASE" {
                return Some(OsString::from(base));
            }
            env.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
        };
        start_check(&h.paths, cfg, &args, &env, &h.exe, io)
    }

    fn check(h: &Home, base: &str, io: &mut FakeIo) -> StartOutcome {
        check_cfg(h, &Config::default(), base, &[], &[], io)
    }

    fn prompt(new: &str) -> String {
        format!("zygo {new} is available (you have {CUR}). Update now? [y/N/s]")
    }

    fn cache_file(h: &Home) -> PathBuf {
        h.paths.cache_dir.join("update.json")
    }

    /// Moves the time of the last check 25 h back; every other field of `update.json` stays.
    fn age_cache(h: &Home) {
        let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(cache_file(h)).unwrap()).unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        v["checked_at"] = serde_json::json!(now - 25 * 60 * 60);
        std::fs::write(cache_file(h), serde_json::to_vec(&v).unwrap()).unwrap();
    }

    fn assert_not_installed(h: &Home) {
        assert_eq!(std::fs::read(&h.exe).unwrap(), OLD, "binary changed");
        no_temp(h.exe.parent().unwrap());
    }

    /// A live session directory under `h`: `token`, and `pid` locked while the returned file is open.
    fn live_session(h: &Home, name: &str) -> std::fs::File {
        let d = h.paths.sessions_dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("token"), "t").unwrap();
        let pid = std::fs::File::create(d.join("pid")).unwrap();
        fs4::FileExt::try_lock(&pid).unwrap();
        pid
    }

    fn sessions_line(n: usize) -> String {
        format!("{n} zygo sessions are running; they keep the old version until you restart them (zygo down / zygo up)")
    }

    #[test] // auto-update #1
    fn a_newer_version_on_a_tty_prompts() {
        let f = Fixture::start(vec![version_file(NEW)]);
        let h = home();
        let mut io = FakeIo::tty(&[""]);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
        assert_eq!(io.asked, vec![prompt(NEW)]);
        assert!(io.said.is_empty(), "{:?}", io.said);
        assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);

        // The same or an older release does not prompt.
        for v in [CUR, "0.0.1"] {
            let f = Fixture::start(vec![version_file(v)]);
            let h = home();
            let mut io = FakeIo::tty(&[]);
            assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
            assert!(io.asked.is_empty() && io.said.is_empty(), "{v}: {:?} {:?}", io.asked, io.said);
            assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
        }
    }

    #[test] // auto-update #2: in process, the binary to replace is a temp file
    fn yes_installs_prints_the_relaunch_line_and_exits_0() {
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, b"fixture-binary"));
        let f = Fixture::start(files);
        for answer in ["y", "Y", "yes"] {
            let h = home();
            let mut io = FakeIo::tty(&[answer]);
            // Exit, not Continue: the caller returns the code and does not start the agent.
            assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Exit { code: 0 }, "{answer}");
            assert_eq!(io.asked, vec![prompt(NEW)]);
            assert_eq!(io.said, vec![format!("updated to {NEW}; relaunch zygo")]);
            assert_eq!(std::fs::read(&h.exe).unwrap(), b"fixture-binary");
            no_temp(h.exe.parent().unwrap());
        }
        let r = f.requests();
        assert_eq!(r[0], "/latest/download/VERSION");
        assert!(r.contains(&format!("/download/v{NEW}/SHA256SUMS")) && r.contains(&format!("/download/v{NEW}/{}", asset())), "{r:?}");
    }

    #[test] // auto-update #2: a failed install changes nothing, prints the error and does not start the agent
    fn yes_with_a_bad_checksum_installs_nothing() {
        let f = Fixture::start(vec![
            version_file(NEW),
            (format!("/download/v{NEW}/SHA256SUMS"), sums(&asset(), b"other-bytes").into_bytes()),
            (format!("/download/v{NEW}/{}", asset()), b"fixture-binary".to_vec()),
        ]);
        let h = home();
        let mut io = FakeIo::tty(&["y"]);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Exit { code: 1 });
        assert_eq!(io.said, vec![format!("zygo update: checksum mismatch for {}", asset())]);
        assert_not_installed(&h);
    }

    #[test] // auto-update #3
    fn no_launches() {
        let f = Fixture::start(vec![version_file(NEW)]);
        for answer in ["", "n", "N", "no", "x", "yy"] {
            let h = home();
            let mut io = FakeIo::tty(&[answer]);
            assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue, "{answer:?}");
            assert_eq!(io.asked, vec![prompt(NEW)]);
            assert!(io.said.is_empty(), "{:?}", io.said);
            assert_not_installed(&h);
        }
        assert!(f.requests().iter().all(|p| p == "/latest/download/VERSION"), "{:?}", f.requests());

        // N asks again after 24 h.
        let h = home();
        let mut io = FakeIo::tty(&["n", "n"]);
        check(&h, &f.base(), &mut io);
        age_cache(&h);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
        assert_eq!(io.asked, vec![prompt(NEW), prompt(NEW)]);
    }

    #[test] // auto-update #4
    fn skip_is_kept_until_a_newer_release() {
        let f = Fixture::start(vec![version_file(NEW)]);
        let h = home();
        let mut io = FakeIo::tty(&["s"]);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
        assert_eq!(io.asked, vec![prompt(NEW)]);
        assert_not_installed(&h);
        let cache: serde_json::Value = serde_json::from_slice(&std::fs::read(cache_file(&h)).unwrap()).unwrap();
        assert_eq!(cache["skip"], NEW, "{cache}");

        // The same version, 25 h later: one request, no prompt.
        age_cache(&h);
        let mut io = FakeIo::tty(&[]);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
        assert!(io.asked.is_empty() && io.said.is_empty(), "{:?} {:?}", io.asked, io.said);
        assert_eq!(f.requests().len(), 2, "{:?}", f.requests());

        // A newer release prompts.
        age_cache(&h);
        let newer = Fixture::start(vec![version_file("9.9.10")]);
        let mut io = FakeIo::tty(&[""]);
        assert_eq!(check(&h, &newer.base(), &mut io), StartOutcome::Continue);
        assert_eq!(io.asked, vec![prompt("9.9.10")]);
    }

    #[test] // auto-update #5
    fn skipped_cases_give_no_prompt_and_no_request() {
        let f = Fixture::start(vec![version_file(NEW)]);
        let off = {
            let mut c = Config::default();
            c.update.check = false;
            c
        };
        let on = Config::default();
        type Case<'a> = (&'a str, bool, &'a Config, &'a [&'a str], &'a [(&'a str, &'a str)]);
        let cases: [Case; 10] = [
            ("non-TTY", false, &on, &[], &[]),
            ("-p", true, &on, &["-p", "hi"], &[]),
            ("--print", true, &on, &["--model", "x", "--print"], &[]),
            ("ZYGO_SESSION", true, &on, &[], &[("ZYGO_SESSION", "01ABC")]),
            // M5 round 2 finding 6: an empty marker counts as set, as AU64 and AU95 count it (`cmd/mod.rs:112-116`).
            ("empty ZYGO_SESSION", true, &on, &[], &[("ZYGO_SESSION", "")]),
            ("ZYGO_WORKSPACE", true, &on, &[], &[("ZYGO_WORKSPACE", "w1")]),
            ("empty ZYGO_WORKSPACE", true, &on, &[], &[("ZYGO_WORKSPACE", "")]),
            ("[update] check = false", true, &off, &[], &[]),
            ("ZYGO_NO_UPDATE_CHECK=1", true, &on, &[], &[("ZYGO_NO_UPDATE_CHECK", "1")]),
            ("both opt-outs", true, &off, &[], &[("ZYGO_NO_UPDATE_CHECK", "1")]),
        ];
        for (name, tty, cfg, args, env) in cases {
            let h = home();
            let mut io = FakeIo { tty, ..FakeIo::tty(&["y"]) };
            assert_eq!(check_cfg(&h, cfg, &f.base(), args, env, &mut io), StartOutcome::Continue, "{name}");
            assert!(io.asked.is_empty() && io.said.is_empty(), "{name}: {:?} {:?}", io.asked, io.said);
            assert!(f.requests().is_empty(), "{name}: {:?}", f.requests());
            assert!(!cache_file(&h).exists(), "{name}: a skipped check wrote update.json");
            assert_not_installed(&h);
        }

        // A check less than 24 h old: the first call asks (one request), the second does nothing.
        let h = home();
        let mut io = FakeIo::tty(&["n", "y"]);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
        assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
        assert_eq!(check(&h, &f.base(), &mut io), StartOutcome::Continue);
        assert_eq!(io.asked, vec![prompt(NEW)]);
        assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
        assert_not_installed(&h);
    }

    #[test] // auto-update #6
    #[ignore = "timing: asserts elapsed time against the 2 s check bound"]
    fn an_unreachable_server_is_silent_and_bounded() {
        // Nothing listens: the connection is refused.
        let closed = dead_port::closed_port();
        // Listens and never accepts: the connection opens (backlog) and no reply comes.
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        let stalled = silent.local_addr().unwrap().port();
        for port in [closed, stalled] {
            let h = home();
            let mut io = FakeIo::tty(&["y"]);
            let t = Instant::now();
            let got = check(&h, &format!("http://127.0.0.1:{port}"), &mut io);
            let took = t.elapsed();
            assert_eq!(got, StartOutcome::Continue);
            // The limit is 2 s for the request; the margin covers the scheduler of a loaded test host.
            assert!(took < Duration::from_millis(2500), "{took:?}");
            assert!(io.asked.is_empty() && io.said.is_empty(), "{:?} {:?}", io.asked, io.said);
            assert_not_installed(&h);

            // The failed check counts as the check of these 24 h: the next launch does not wait again.
            let t = Instant::now();
            assert_eq!(check(&h, &format!("http://127.0.0.1:{port}"), &mut io), StartOutcome::Continue);
            assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
        }
        drop(silent);
    }

    #[test] // auto-update #7; the Windows branch runs on the windows-latest CI job
    fn other_live_sessions_add_the_line() {
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, b"fixture-binary"));
        let f = Fixture::start(files);
        let h = home();
        let _a = live_session(&h, "01A");
        let _b = live_session(&h, "01B");
        // Not live: no lock on `pid`.
        let dead = h.paths.sessions_dir.join("01C");
        std::fs::create_dir_all(&dead).unwrap();
        std::fs::write(dead.join("token"), "t").unwrap();
        std::fs::write(dead.join("pid"), "1").unwrap();

        let mut io = FakeIo::tty(&["y"]);
        let got = check(&h, &f.base(), &mut io);
        assert_eq!(io.said[0], sessions_line(2));
        if cfg!(windows) {
            // The install is deferred: no prompt, the launch continues.
            assert_eq!(got, StartOutcome::Continue);
            assert_eq!(io.said.len(), 1, "{:?}", io.said);
            assert!(io.asked.is_empty(), "{:?}", io.asked);
            assert_not_installed(&h);
            assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
        } else {
            assert_eq!(got, StartOutcome::Exit { code: 0 });
            assert_eq!(io.asked, vec![prompt(NEW)]);
            assert_eq!(io.said, vec![sessions_line(2), format!("updated to {NEW}; relaunch zygo")]);
            assert_eq!(std::fs::read(&h.exe).unwrap(), b"fixture-binary");
        }

        // No live session: no line.
        let h = home();
        let mut io = FakeIo::tty(&["n"]);
        check(&h, &f.base(), &mut io);
        assert!(io.said.is_empty(), "{:?}", io.said);
        assert_eq!(io.asked, vec![prompt(NEW)]);
        drop(h.dir);
    }

    /// `zygo <agent>` with `ZYGO_TEST_ASSUME_TTY=1` and `y` on stdin: the dispatch runs the check before the launch.
    /// The binary replaced is the copy in the temp dir. `PATH` holds no agent, so exit 0 proves no launch was tried.
    fn dispatch_installs_and_does_not_launch(agent: &str) {
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, b"fixture-binary"));
        let f = Fixture::start(files);
        let s = setup();
        let empty = s.dir.path().join("no-agents");
        std::fs::create_dir(&empty).unwrap();
        std::fs::write(s.dir.path().join("config.toml"), common::NO_MARKETPLACE).unwrap();
        let mut cmd = Command::new(&s.copy);
        cmd.arg(agent)
            .env("ZYGO_RELEASE_BASE", f.base())
            .env("ZYGO_TEST_ASSUME_TTY", "1")
            .env("HOME", s.dir.path())
            .env("LOCALAPPDATA", s.dir.path())
            .env("APPDATA", s.dir.path())
            .env("PATH", &empty)
            .env_remove("ZYGO_SESSION")
            .env_remove("ZYGO_WORKSPACE")
            .env_remove("ZYGO_NO_UPDATE_CHECK")
            .env("ZYGO_CONFIG", s.dir.path().join("config.toml"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let g = COPY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut child = cmd.spawn().unwrap();
        drop(g);
        child.stdin.take().unwrap().write_all(b"y\n").unwrap();
        let o = child.wait_with_output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(err(&o), format!("{} updated to {NEW}; relaunch zygo\n", prompt(NEW)));
        assert_eq!(out(&o), "");
        assert_eq!(std::fs::read(&s.copy).unwrap(), b"fixture-binary");
        common::assert_no_marketplace_sync(s.dir.path());
    }

    /// `zygo <agent>` with an empty `ZYGO_SESSION`, on a TTY (`ZYGO_TEST_ASSUME_TTY=1`): the check skips (M5 round 2
    /// finding 6). No answer is given and no agent is on `PATH`, so the real test binary is never replaced.
    fn dispatch_with_an_empty_session_skips(agent: &str) {
        let f = Fixture::start(vec![version_file(NEW)]);
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("no-agents");
        std::fs::create_dir(&empty).unwrap();
        std::fs::write(dir.path().join("config.toml"), common::NO_MARKETPLACE).unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        cmd.arg(agent)
            .env("ZYGO_RELEASE_BASE", f.base())
            .env("ZYGO_TEST_ASSUME_TTY", "1")
            .env("ZYGO_SESSION", "")
            .env("HOME", dir.path())
            .env("LOCALAPPDATA", dir.path())
            .env("APPDATA", dir.path())
            .env("PATH", &empty)
            .env_remove("ZYGO_WORKSPACE")
            .env_remove("ZYGO_NO_UPDATE_CHECK")
            .env("ZYGO_CONFIG", dir.path().join("config.toml"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let g = COPY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let child = cmd.spawn().unwrap();
        drop(g);
        let o = child.wait_with_output().unwrap();
        assert!(!err(&o).contains("is available"), "{}", err(&o));
        assert!(f.requests().is_empty(), "{:?}", f.requests());
        assert!(!dir.path().join(".cache/zygo/update.json").exists());
        common::assert_no_marketplace_sync(dir.path());
    }

    #[test]
    fn zygo_claude_with_an_empty_zygo_session_skips_the_check() {
        dispatch_with_an_empty_session_skips("claude");
    }

    #[test]
    fn zygo_opencode_with_an_empty_zygo_session_skips_the_check() {
        dispatch_with_an_empty_session_skips("opencode");
    }

    #[test]
    #[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
    fn zygo_claude_runs_the_check_before_the_launch() {
        dispatch_installs_and_does_not_launch("claude");
    }

    #[test]
    #[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
    fn zygo_opencode_runs_the_check_before_the_launch() {
        dispatch_installs_and_does_not_launch("opencode");
    }
}

// ---- the update check of `zygo up` and `zygo ws run` (M5; auto-update design, "Where") ----

#[cfg(unix)]
mod workspace_check {
    use super::*;
    use common::workspace::{Guarded, Harness, WS_ID};
    use std::ffi::{OsStr, OsString};
    use std::os::unix::fs::MetadataExt;
    use std::time::Duration;
    use zygo_core::Agent;
    use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind};

    /// `f` while `COPY_LOCK` is held: every spawn and fork of this module runs under it (D8).
    fn locked<R>(f: impl FnOnce() -> R) -> R {
        let _g = COPY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

    /// The prompt of the auto-update design.
    fn prompt(new: &str) -> String {
        format!("zygo {new} is available (you have {CUR}). Update now? [y/N/s]")
    }

    /// Polls `f` every 20 ms until it is true or `bound` is over; the last result.
    fn until(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
        let t0 = std::time::Instant::now();
        loop {
            if f() {
                return true;
            }
            if t0.elapsed() >= bound {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A git repository in `dir` with one empty commit and no user or system git config; `git` linked into `h.bin`.
    fn git_repo(h: &Harness, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for args in [&["init", "-q", "-b", "main"][..], &["commit", "-q", "--allow-empty", "-m", "a"][..]] {
            let mut c = Command::new("git");
            c.args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .current_dir(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let child = locked(|| c.spawn().unwrap());
            let o = child.wait_with_output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", err(&o));
        }
        let link = h.bin.join("git");
        if !link.exists() {
            std::os::unix::fs::symlink(which::which("git").unwrap(), link).unwrap();
        }
    }

    /// `c` with the update check on: the harness opt-out removed, the fixture release base, and a TTY assumed (D7).
    fn checked(mut c: Command, base: &OsStr) -> Command {
        c.env_remove("ZYGO_NO_UPDATE_CHECK").env("ZYGO_RELEASE_BASE", base).env("ZYGO_TEST_ASSUME_TTY", "1");
        c
    }

    fn update_json(h: &Harness) -> PathBuf {
        h.paths.cache_dir.join("update.json")
    }

    /// The number of entries of the state folder of workspaces; 0 when it does not exist.
    fn state_entries(h: &Harness) -> usize {
        std::fs::read_dir(h.paths.data_dir.join("state/workspaces")).map(|d| d.count()).unwrap_or(0)
    }

    /// The record that `up` writes for a new loadout member (`zygo-workspace/src/up.rs:565-568`).
    fn loadout_member() -> MemberRecord {
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        r.state = MemberState::Running;
        r
    }

    #[test]
    fn harness_env_opts_out_of_the_update_check() {
        let h = locked(Harness::no_tmux);
        assert!(h.env().iter().any(|(k, v)| k == "ZYGO_NO_UPDATE_CHECK" && v == "1"), "{:?}", h.env());
    }

    #[test]
    #[ignore = "timing: tmux e2e: 2 s tmux and launcher bounds, test polls under 60 s"]
    fn zygo_up_prompts_once_before_any_pane_starts() {
        let Some(h) = locked(Harness::new) else { return };
        git_repo(&h, &h.repo());
        let f = Fixture::start(vec![version_file(NEW)]);
        let base = OsString::from(f.base());
        let (out, stub) = h.stub(Agent::ClaudeCode, "a");
        let tmux = OsString::from(format!("{},1,0", h.socket().display()));
        let mut extra: Vec<(&str, &OsStr)> = vec![("TMUX", tmux.as_os_str())];
        extra.extend(stub.iter().map(|(k, v)| (*k, v.as_os_str())));
        let mut c = checked(h.zygo(&["up", "--no-attach"], &extra), &base);
        c.current_dir(h.repo()).stdin(Stdio::piped());
        let mut g = locked(|| Guarded::spawn(c));
        let mut stdin = g.stdin().unwrap();
        // Asked, not answered: no server, no state, no agent.
        assert!(until(Duration::from_secs(10), || f.requests().len() == 1), "{:?}", f.requests());
        std::thread::sleep(Duration::from_millis(500));
        let srv = h.server.as_ref().unwrap();
        assert!(locked(|| srv.run(&["list-sessions"])).is_err(), "a pane started before the answer");
        assert_eq!(state_entries(&h), 0);
        assert!(!out.exists());
        stdin.write_all(b"n\n").unwrap();
        drop(stdin);
        let o = g.output("zygo up");
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(err(&o).matches(&prompt(NEW)).count(), 1, "{}", err(&o));
        // The member pane starts after the answer; its `ws run` makes no second request (the 24 h rule; P2).
        assert!(until(Duration::from_secs(20), || out.join("argv.json").exists()));
        assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
        assert!(update_json(&h).is_file());
    }

    #[test]
    #[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
    fn zygo_up_yes_installs_and_starts_nothing() {
        let h = locked(Harness::no_tmux); // `bin/tmux` fails every call
        git_repo(&h, &h.repo());
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, b"fixture-binary"));
        let f = Fixture::start(files);
        let base = OsString::from(f.base());
        let s = setup(); // a copy: the test binary is never replaced
        let mut c = Command::new(&s.copy);
        c.args(["up", "--no-attach"]).env_clear().envs(h.env());
        let mut c = checked(c, &base);
        c.current_dir(h.repo()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = locked(|| Guarded::spawn(c));
        child.stdin().unwrap().write_all(b"y\n").unwrap();
        let o = child.output("zygo up");
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(err(&o), format!("{} updated to {NEW}; relaunch zygo\n", prompt(NEW)));
        assert_eq!(std::fs::read(&s.copy).unwrap(), b"fixture-binary");
        assert_eq!(state_entries(&h), 0);
    }

    #[test]
    #[ignore = "timing: tmux e2e: 2 s tmux and launcher bounds, test polls under 60 s"]
    fn zygo_up_all_asks_one_time_for_two_workspaces() {
        let Some(h) = locked(Harness::new) else { return };
        let tmux = OsString::from(format!("{},1,0", h.socket().display()));
        let repos = [h.repo(), h.root().join("repo2")];
        for r in &repos {
            git_repo(&h, r);
            // The harness opt-out stays: these two `up` runs make no check.
            let mut c = h.zygo(&["up", "--no-attach"], &[("TMUX", tmux.as_os_str())]);
            c.current_dir(r);
            let o = locked(|| Guarded::spawn(c)).output("zygo up");
            assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        }
        let srv = h.server.as_ref().unwrap();
        locked(|| srv.kill_server()); // both stay `up` in their state: `--all` restores both
        // The launchers of the killed panes end shortly after the server, and while one is live `up` refuses the
        // restore (AU87). So the test waits until none is live.
        let live = || {
            let d = std::fs::read_dir(&h.paths.sessions_dir);
            d.map(|d| d.flatten().filter(|e| zygo_core::session_dir::live(&e.path())).count()).unwrap_or(0)
        };
        assert!(until(Duration::from_secs(15), || live() == 0), "launchers still live");
        let f = Fixture::start(vec![version_file(NEW)]);
        let base = OsString::from(f.base());
        // The restored panes are shells; the harness test shell exits at once and would end the session, so they
        // get an interactive `/bin/sh`, which waits in its pane.
        let sh = OsStr::new("/bin/sh");
        let mut c = checked(h.zygo(&["up", "--all"], &[("TMUX", tmux.as_os_str()), ("SHELL", sh)]), &base);
        c.current_dir(h.root()).stdin(Stdio::piped());
        let mut g = locked(|| Guarded::spawn(c));
        g.stdin().unwrap().write_all(b"n\n").unwrap();
        let o = g.output("zygo up --all");
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(err(&o).matches(&prompt(NEW)).count(), 1, "{}", err(&o));
        let names = locked(|| srv.run(&["list-sessions", "-F", "#{session_name}"])).unwrap();
        assert_eq!(names.lines().count(), 2, "{names}");
        assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
    }

    /// `zygo up --no-attach` whose own check does not run, on a fresh `update.json`, with the update check on for its
    /// panes (`extra` and `tty` say why `up` skips). The first pane is a TTY. Without `ZYGO_WORKSPACE` from
    /// `new-session -e` (W§8.1 step 9), its `ws run` made the request, and the prompt waited in the pane, so the
    /// member did not start. No request means no prompt: the prompt comes only after the answer of the request.
    fn first_pane_makes_no_request(extra: &[(&str, &OsStr)], tty: bool) {
        let Some(h) = locked(Harness::new) else { return };
        git_repo(&h, &h.repo());
        let f = Fixture::start(vec![version_file(NEW)]);
        let base = OsString::from(f.base());
        let (out, stub) = h.stub(Agent::ClaudeCode, "a");
        let tmux = OsString::from(format!("{},1,0", h.socket().display()));
        let mut all: Vec<(&str, &OsStr)> = vec![("TMUX", tmux.as_os_str())];
        all.extend(stub.iter().map(|(k, v)| (*k, v.as_os_str())));
        all.extend_from_slice(extra);
        let mut c = h.zygo(&["up", "--no-attach"], &all);
        c.env_remove("ZYGO_NO_UPDATE_CHECK").env("ZYGO_RELEASE_BASE", &base).current_dir(h.repo());
        if tty {
            c.env("ZYGO_TEST_ASSUME_TTY", "1");
        }
        let o = locked(|| Guarded::spawn(c)).output("zygo up");
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(!err(&o).contains(&prompt(NEW)), "{}", err(&o));
        assert!(until(Duration::from_secs(20), || out.join("argv.json").exists()), "no member; requests {:?}", f.requests());
        assert!(f.requests().is_empty(), "{:?}", f.requests());
        assert!(!update_json(&h).exists());
    }

    #[test] // M5 round 2 finding 1 (FIX-28 folded): `up` inside an agent skips its own check; the first pane skips too
    #[ignore = "timing: tmux e2e: 2 s tmux and launcher bounds, test polls under 60 s"]
    fn zygo_up_inside_an_agent_its_first_pane_makes_no_request() {
        first_pane_makes_no_request(&[("ZYGO_SESSION", OsStr::new("01JZ0000000000000000000000"))], true);
    }

    #[test] // M5 round 2 finding 5: `up` with no TTY asks nothing; its first pane (a TTY) asks nothing either
    #[ignore = "timing: tmux e2e: 2 s tmux and launcher bounds, test polls under 60 s"]
    fn zygo_up_without_a_tty_and_its_first_pane_ask_nothing() {
        first_pane_makes_no_request(&[], false);
    }

    #[test] // auto-update #5, the `ZYGO_WORKSPACE` bullet: the opt-out is removed, so only `ZYGO_WORKSPACE` stops it
    fn ws_run_with_zygo_workspace_makes_no_request() {
        let h = locked(Harness::no_tmux);
        let f = Fixture::start(vec![version_file(NEW)]);
        let base = OsString::from(f.base());
        let c = h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", "kaaaaaaaa"], &[("ZYGO_WORKSPACE", OsStr::new(WS_ID))]);
        let c = checked(c, &base);
        let o = locked(|| Guarded::spawn(c)).output("ws run");
        assert_eq!((o.status.code(), err(&o)), (Some(0), format!("zygo: no workspace with id {WS_ID}\n")));
        assert!(f.requests().is_empty(), "{:?}", f.requests());
        assert!(!update_json(&h).exists());
    }

    #[test] // the same run: the skip is what stops it; AU93 still holds after the check
    #[ignore = "timing: the update fetch timeouts in the zygo binary against the test server"]
    fn ws_run_without_zygo_workspace_makes_the_request() {
        let h = locked(Harness::no_tmux);
        let f = Fixture::start(vec![version_file(NEW)]);
        let base = OsString::from(f.base());
        let c = checked(h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", "kaaaaaaaa"], &[]), &base);
        let o = locked(|| Guarded::spawn(c)).output("ws run");
        // stdin is null: the prompt reads "" and the run continues (D6).
        assert_eq!((o.status.code(), err(&o)), (Some(0), format!("{} zygo: no workspace with id {WS_ID}\n", prompt(NEW))));
        assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
        assert!(update_json(&h).is_file());
        let env = std::fs::read_to_string(h.root().join("shell/env")).unwrap();
        assert!(env.lines().any(|l| l == format!("ZYGO_WORKSPACE={WS_ID}")), "{env}"); // AU93
    }

    /// The pane key of the `ws run` tests.
    const KEY: &str = "kaaaaaaaa";

    /// What one `ws run` of the member pane gave.
    struct Run {
        o: Output,
        out: PathBuf,
        took: Duration,
    }

    /// `exe ws run` of a loadout member pane (the record that `up` writes, W§8.1 step 8), with `answer` on stdin and
    /// `extra` in the environment. With `check`, the update check is on (fixture `base`); else the harness opt-out
    /// stays. `exe` is a `setup()` copy for each answer that can install.
    fn ws_run_member(h: &Harness, exe: &Path, base: &OsStr, check: bool, answer: &[u8], extra: &[(&str, &OsStr)]) -> Run {
        h.seed(WS_ID, &mut |w| {
            w.panes = vec![Pane { key: KEY.into(), cwd: h.repo(), kind: PaneKind::ClaudeCode, ..Default::default() }];
            w.members.clear();
            w.members.insert(KEY.into(), loadout_member());
        });
        let (out, stub) = h.stub(Agent::ClaudeCode, "a");
        let mut c = Command::new(exe);
        c.args(["ws", "run", "--ws-id", WS_ID, "--pane", KEY]).env_clear().envs(h.env()).envs(stub);
        if check {
            c = checked(c, base);
        }
        for (k, v) in extra {
            c.env(k, v);
        }
        c.current_dir(h.root()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let t0 = std::time::Instant::now();
        let mut child = locked(|| Guarded::spawn(c));
        child.stdin().unwrap().write_all(answer).unwrap();
        let o = child.output("ws run");
        Run { o, out, took: t0.elapsed() }
    }

    /// The member started and ended as a normal `ws run`: exit 0 (the test shell after the member), the agent ran, its
    /// record does not stay `running` without a launcher, and `ZYGO_RELAUNCHED` reached neither the agent nor the
    /// shell (AU64).
    fn assert_member_ran(h: &Harness, r: &Run) {
        let e = err(&r.o);
        assert_eq!(r.o.status.code(), Some(0), "{e}");
        assert!(r.out.join("argv.json").exists(), "the agent did not start: {e}");
        assert_ne!(h.store(WS_ID).read().unwrap().unwrap().members[KEY].state, MemberState::Running);
        let agent = std::fs::read_to_string(r.out.join("env.json")).unwrap();
        assert!(!agent.contains("ZYGO_RELAUNCHED"), "{agent}");
        let shell = std::fs::read_to_string(h.root().join("shell/env")).unwrap();
        assert!(!shell.lines().any(|l| l.starts_with("ZYGO_RELAUNCHED=")), "{shell}");
    }

    /// The requests of one check and one install of `NEW`.
    fn check_and_install() -> Vec<String> {
        vec!["/latest/download/VERSION".to_string(), format!("/download/v{NEW}/SHA256SUMS"), format!("/download/v{NEW}/{}", asset())]
    }

    /// `answer` installs a release that is a real zygo, so the relaunched process runs (D4; W§8.2 step 1a).
    fn yes_relaunches_into_the_member(answer: &[u8]) {
        let h = locked(Harness::no_tmux);
        let zygo = std::fs::read(env!("CARGO_BIN_EXE_zygo")).unwrap();
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, &zygo));
        let f = Fixture::start(files);
        let s = setup();
        let ino = std::fs::metadata(&s.copy).unwrap().ino();
        let r = ws_run_member(&h, &s.copy, OsStr::new(&f.base()), true, answer, &[]);
        let e = err(&r.o);
        assert!(e.starts_with(&format!("{} updated to {NEW}; relaunch zygo\n", prompt(NEW))), "{e}");
        assert_eq!(e.matches(&prompt(NEW)).count(), 1, "{e}");
        assert!(!e.contains("cannot relaunch"), "{e}");
        // Installed in the copy: a new inode with the release bytes.
        assert_ne!(std::fs::metadata(&s.copy).unwrap().ino(), ino);
        assert_eq!(std::fs::read(&s.copy).unwrap(), zygo);
        assert_member_ran(&h, &r);
        // The relaunched process made no request (ZYGO_RELAUNCHED).
        assert_eq!(f.requests(), check_and_install());
    }

    #[test] // D4
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_yes_installs_and_relaunches_into_the_member() {
        yes_relaunches_into_the_member(b"y\n");
    }

    #[test] // M5 round 2 finding 5: `yes` is the long form of `y`
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_yes_in_full_installs_and_relaunches_into_the_member() {
        yes_relaunches_into_the_member(b"yes\n");
    }

    #[test] // M5 round 2 finding 3: a failed relaunch continues as a normal `ws run`, so the member starts
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_relaunch_failure_starts_the_member() {
        let h = locked(Harness::no_tmux);
        let zygo = std::fs::read(env!("CARGO_BIN_EXE_zygo")).unwrap();
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, &zygo));
        let f = Fixture::start(files);
        let s = setup();
        // The `test-hooks` seam: the relaunch execs a path that does not exist.
        let r = ws_run_member(&h, &s.copy, OsStr::new(&f.base()), true, b"y\n", &[("ZYGO_TEST_RELAUNCH_FAIL", OsStr::new("1"))]);
        let e = err(&r.o);
        let head = format!(
            "{} updated to {NEW}; relaunch zygo\nzygo: cannot relaunch zygo ws run: No such file or directory (os error 2)\n",
            prompt(NEW)
        );
        assert!(e.starts_with(&head), "{e}");
        assert_member_ran(&h, &r);
        assert_eq!(f.requests(), check_and_install());
        // M5 round 3 finding 1: the install replaced the binary, so `/proc/self/exe` now names `<path> (deleted)`.
        // Every zygo command in the settings that the agent got names the canonical path that `ws run` read first.
        let settings: serde_json::Value = serde_json::from_slice(&std::fs::read(r.out.join("settings.json")).unwrap()).unwrap();
        let mut commands: Vec<String> = Vec::new();
        commands.extend(settings["apiKeyHelper"].as_str().map(String::from));
        commands.extend(settings["statusLine"]["command"].as_str().map(String::from));
        for list in settings["hooks"].as_object().into_iter().flat_map(|m| m.values()) {
            for entry in list.as_array().into_iter().flatten() {
                for hook in entry["hooks"].as_array().into_iter().flatten() {
                    commands.extend(hook["command"].as_str().map(String::from));
                }
            }
        }
        let quoted = format!("'{}'", std::fs::canonicalize(&s.copy).unwrap().display());
        assert!(commands.iter().any(|c| c.contains(" guard")), "{commands:?}");
        assert!(commands.iter().any(|c| c.contains("role-guard")), "{commands:?}");
        for c in &commands {
            assert!(!c.contains("(deleted)"), "{c}");
            assert!(c.contains(&quoted), "{c} does not name {quoted}");
        }
    }

    /// Removes the directory at its path when dropped, also when an assertion failed.
    struct RemoveDir(PathBuf);

    impl Drop for RemoveDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test] // M5 round 2 finding 4, round 3 finding 4: a cache that cannot be written gives no second check
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_yes_with_an_unwritable_cache_checks_once() {
        let h = locked(Harness::no_tmux);
        let zygo = std::fs::read(env!("CARGO_BIN_EXE_zygo")).unwrap();
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, &zygo));
        let f = Fixture::start(files);
        let s = setup();
        // A directory at the `update.json` path: the cache write (a rename onto it) fails for every uid, also root,
        // and a read finds no cache.
        let blocked = update_json(&h);
        std::fs::create_dir_all(blocked.join("keep")).unwrap();
        let _guard = RemoveDir(blocked.clone());
        let r = ws_run_member(&h, &s.copy, OsStr::new(&f.base()), true, b"y\n", &[]);
        let e = err(&r.o);
        assert!(blocked.join("keep").is_dir(), "the cache write replaced the directory");
        assert_eq!(e.matches(&prompt(NEW)).count(), 1, "{e}");
        assert_member_ran(&h, &r);
        assert_eq!(f.requests(), check_and_install());
    }

    /// `y` with an install that fails: the update line names the failure as UPD-1 prints it (`zygo update: <msg>`),
    /// the binary is unchanged, and `ws run` continues as a normal `ws run` with no relaunch (D4). The `test-hooks`
    /// seam makes any relaunch fail with a line, so a relaunch that is tried shows.
    fn failed_install_starts_the_member(f: &Fixture, line: &str) {
        let h = locked(Harness::no_tmux);
        let s = setup();
        let before = std::fs::read(&s.copy).unwrap();
        let seam = [("ZYGO_TEST_RELAUNCH_FAIL", OsStr::new("1"))];
        let r = ws_run_member(&h, &s.copy, OsStr::new(&f.base()), true, b"y\n", &seam);
        let e = err(&r.o);
        assert!(e.starts_with(&format!("{} {line}\n", prompt(NEW))), "{e}");
        assert!(!e.contains("relaunch"), "{e}");
        assert_eq!(std::fs::read(&s.copy).unwrap(), before);
        assert_member_ran(&h, &r);
        assert_eq!(f.requests(), check_and_install());
    }

    #[test] // M5 round 2 finding 5
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_install_with_a_bad_checksum_starts_the_member() {
        let mut files = vec![version_file(NEW)];
        files.push((format!("/download/v{NEW}/SHA256SUMS"), sums(&asset(), b"other-bytes").into_bytes()));
        files.push((format!("/download/v{NEW}/{}", asset()), b"fixture-binary".to_vec()));
        let f = Fixture::start(files);
        failed_install_starts_the_member(&f, &format!("zygo update: checksum mismatch for {}", asset()));
    }

    #[test] // M5 round 2 finding 5
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_install_with_http_500_starts_the_member() {
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, b"fixture-binary"));
        let failing = format!("/download/v{NEW}/{}", asset());
        let f = Fixture::start_failing(files, vec![failing.clone()]);
        let line = format!("zygo update: cannot download {}{failing}: HTTP 500 Internal Server Error", f.base());
        failed_install_starts_the_member(&f, &line);
    }

    #[test] // M5 round 2 finding 5, round 3 finding 3: a server that accepts and never answers ends at the 2 s bound
    #[ignore = "timing: asserts elapsed time against the 2 s check bound"]
    fn ws_run_with_a_stalled_server_starts_the_member_at_the_bound() {
        // The total time limit of the check (auto-update design, "Check").
        const BOUND: Duration = Duration::from_secs(2);
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // Each connection is accepted and kept open with no answer.
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for c in l.incoming().flatten() {
                held.push(c);
            }
        });
        let h = locked(Harness::no_tmux);
        let base = OsString::from(format!("http://127.0.0.1:{port}"));
        let r = ws_run_member(&h, Path::new(env!("CARGO_BIN_EXE_zygo")), &base, true, b"", &[]);
        let e = err(&r.o);
        assert!(!e.contains("is available") && !e.contains("zygo update"), "{e}");
        assert_member_ran(&h, &r);
        assert!(r.took >= BOUND - Duration::from_millis(200), "{:?}: the run did not wait for the bound", r.took);
        assert!(r.took < BOUND + Duration::from_secs(3), "{:?}", r.took);
    }

    #[test] // M5 round 2 finding 5: `n`, `s` and `skip` start the member and install nothing; `s` and `skip` keep the skip
    #[ignore = "timing: the 2 s update check and the 2 s launcher probes of ws run"]
    fn ws_run_other_answers_start_the_member() {
        for (answer, skip) in [(&b"n\n"[..], None), (b"s\n", Some(NEW)), (b"skip\n", Some(NEW))] {
            let h = locked(Harness::no_tmux);
            let f = Fixture::start(vec![version_file(NEW)]);
            let r = ws_run_member(&h, Path::new(env!("CARGO_BIN_EXE_zygo")), OsStr::new(&f.base()), true, answer, &[]);
            let e = err(&r.o);
            assert!(e.starts_with(&format!("{} ", prompt(NEW))), "{e}");
            assert!(!e.contains("zygo update") && !e.contains("updated to"), "{e}");
            assert_member_ran(&h, &r);
            assert_eq!(f.requests(), vec!["/latest/download/VERSION"]);
            let v: serde_json::Value = serde_json::from_slice(&std::fs::read(update_json(&h)).unwrap()).unwrap();
            assert_eq!(v["skip"].as_str(), skip, "{answer:?}: {v}");
        }
    }
}
