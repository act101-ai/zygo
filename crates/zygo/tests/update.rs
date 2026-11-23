//! `zygo update` against a local fixture server (the release base is replaced through the `test-hooks` seam).

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

mod common;

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
fn check_reports_up_to_date() {
    let f = Fixture::start(vec![version_file(CUR)]);
    let s = setup();
    let o = run(&s, &f.base(), &["update", "--check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("zygo {CUR} is up to date\n"));
}

#[test]
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
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let s = setup();
    let before = snapshot(&s.copy);
    let o = run(&s, &format!("http://127.0.0.1:{port}"), &["update"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("cannot download "), "{}", err(&o));
    assert_unchanged(&s, &before);
}

#[test]
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
fn system_proxy_is_not_used() {
    let f = Fixture::start(vec![version_file(NEW)]);
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
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
        let cases: [Case; 8] = [
            ("non-TTY", false, &on, &[], &[]),
            ("-p", true, &on, &["-p", "hi"], &[]),
            ("--print", true, &on, &["--model", "x", "--print"], &[]),
            ("ZYGO_SESSION", true, &on, &[], &[("ZYGO_SESSION", "01ABC")]),
            ("ZYGO_WORKSPACE", true, &on, &[], &[("ZYGO_WORKSPACE", "w1")]),
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
    fn an_unreachable_server_is_silent_and_bounded() {
        // Nothing listens: the connection is refused.
        let closed = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
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
            .env_remove("ZYGO_CONFIG")
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
    }

    #[test]
    fn zygo_claude_runs_the_check_before_the_launch() {
        dispatch_installs_and_does_not_launch("claude");
    }

    #[test]
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
        // A restore waits until no launcher of the killed panes is live (AU87); they end shortly after the server.
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

    #[test] // D4
    fn ws_run_yes_installs_and_relaunches_into_the_member() {
        let h = locked(Harness::no_tmux);
        let key = "kaaaaaaaa";
        h.seed(WS_ID, &mut |w| {
            w.panes = vec![Pane { key: key.into(), cwd: h.repo(), kind: PaneKind::ClaudeCode, ..Default::default() }];
            w.members.clear();
            w.members.insert(key.into(), loadout_member());
        });
        // The release is a real zygo, so the relaunched process runs; the copy gets a new inode on the install.
        let zygo = std::fs::read(env!("CARGO_BIN_EXE_zygo")).unwrap();
        let mut files = vec![version_file(NEW)];
        files.extend(release(NEW, &zygo));
        let f = Fixture::start(files);
        let base = OsString::from(f.base());
        let (out, stub) = h.stub(Agent::ClaudeCode, "a");
        let s = setup();
        let ino = std::fs::metadata(&s.copy).unwrap().ino();
        let mut c = Command::new(&s.copy);
        c.args(["ws", "run", "--ws-id", WS_ID, "--pane", key]).env_clear().envs(h.env()).envs(stub);
        let mut c = checked(c, &base);
        c.current_dir(h.root()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = locked(|| Guarded::spawn(c));
        child.stdin().unwrap().write_all(b"y\n").unwrap();
        let o = child.output("ws run");
        let e = err(&o);
        assert_eq!(o.status.code(), Some(0), "{e}"); // the exit of the test shell after the member
        assert!(e.starts_with(&format!("{} updated to {NEW}; relaunch zygo\n", prompt(NEW))), "{e}");
        assert_eq!(e.matches(&prompt(NEW)).count(), 1, "{e}");
        assert!(!e.contains("cannot relaunch"), "{e}");
        // Installed in the copy.
        assert_ne!(std::fs::metadata(&s.copy).unwrap().ino(), ino);
        assert_eq!(std::fs::read(&s.copy).unwrap(), zygo);
        // The relaunch ran: the member started, and its record does not stay `Running` without a launcher.
        assert!(out.join("argv.json").exists(), "{e}");
        assert_ne!(h.store(WS_ID).read().unwrap().unwrap().members[key].state, MemberState::Running);
        // One check and one install; the relaunched process made no request (no loop).
        assert_eq!(f.requests(), vec![
            "/latest/download/VERSION".to_string(),
            format!("/download/v{NEW}/SHA256SUMS"),
            format!("/download/v{NEW}/{}", asset()),
        ]);
    }
}
