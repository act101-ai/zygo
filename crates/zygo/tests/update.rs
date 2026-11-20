//! `zygo update` against a local fixture server (the release base is replaced through the `test-hooks` seam).

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

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
