use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

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

type Failure = (i32, String);

fn fail<T>(msg: String) -> Result<T, Failure> {
    Err((1, msg))
}

fn run_inner(version: Option<&str>, check: bool) -> Result<(), Failure> {
    let cur_str = env!("CARGO_PKG_VERSION");
    let cur = semver::Version::parse(cur_str).map_err(|e| (1, format!("bad own version {cur_str}: {e}")))?;
    let explicit = match version {
        Some(v) => match parse_version(v).and_then(|s| semver::Version::parse(&s).ok().map(|p| (s, p))) {
            Some(x) => Some(x),
            None => return Err((2, format!("bad --version {v}"))),
        },
        None => None,
    };

    let base = release_base();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| (1, format!("cannot start runtime: {e}")))?;
    let client = build_client(CONNECT_TIMEOUT, READ_TIMEOUT).map_err(|e| (1, e))?;

    let (target_str, target) = match explicit {
        Some(x) => x,
        None => {
            let url = format!("{base}/latest/download/VERSION");
            let body = fetch(&client, &rt, &url).map_err(|e| (1, e))?;
            let text = String::from_utf8_lossy(&body);
            match parse_version(text.trim()).and_then(|s| semver::Version::parse(&s).ok().map(|p| (s, p))) {
                Some(x) => x,
                None => return fail(format!("bad VERSION from {url}")),
            }
        }
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

    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let Some(name) = asset_name(os, arch) else {
        return fail(format!("unsupported platform {os}/{arch}"));
    };
    let dir = format!("{base}/download/v{target_str}");
    let sums = fetch(&client, &rt, &format!("{dir}/SHA256SUMS")).map_err(|e| (1, e))?;
    let sums = String::from_utf8_lossy(&sums).to_string();
    let Some(expected) = find_sum(&sums, &name) else {
        return fail(format!("no checksum for {name}"));
    };
    let bytes = fetch(&client, &rt, &format!("{dir}/{name}")).map_err(|e| (1, e))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if !sum_matches(expected, &actual) {
        return fail(format!("checksum mismatch for {name}"));
    }
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).map_err(|e| (1, format!("cannot locate the running binary: {e}")))?;
    replace_binary(&bytes, &exe).map_err(|e| (1, format!("cannot replace {}: {e}", exe.display())))?;
    super::print_out(&format!("updated zygo {cur_str} -> {target_str}\n"));
    Ok(())
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

fn release_base() -> String {
    #[cfg(feature = "test-hooks")]
    if let Ok(b) = std::env::var("ZYGO_RELEASE_BASE") {
        return b;
    }
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
    let _ = rustls::crypto::ring::default_provider().install_default();
    client_builder(connect, read).build().map_err(|e| format!("http client: {e}"))
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

#[cfg(windows)]
fn swap_in(tmp: &Path, target: &Path) -> std::io::Result<()> {
    let mut old = target.as_os_str().to_owned();
    old.push(".old");
    let old = PathBuf::from(old);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old)?;
    if let Err(e) = std::fs::rename(tmp, target) {
        let _ = std::fs::rename(&old, target);
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
    fn fetch_ends_a_stalled_read_after_the_read_timeout() {
        let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n", b"abc", 4000);
        let c = build_client(Duration::from_secs(5), Duration::from_millis(300)).unwrap();
        let t = Instant::now();
        let e = fetch(&c, &rt(), &url).unwrap_err();
        assert!(e.starts_with(&format!("cannot download {url}: ")), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[test]
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
}
