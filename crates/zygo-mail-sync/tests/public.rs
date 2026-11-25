//! MS-8: the public check (mail sync design S§8.2, S§13 limit 7, S§18, S§19 #6). No test depends on wall-clock time.
use zygo_mail_sync::env::Verdict;
use zygo_mail_sync::public::*;
use zygo_work::repo::Whole;

fn fail(err: &str) -> Whole { Whole { ok: false, out: vec![], err: err.as_bytes().to_vec() } }

#[test]
fn map_url_table() {
    use Mapped::*;
    let h = |s: &str| Https(s.to_string());
    for (url, want) in [
        ("https://u:p@h/p", h("https://h/p")), ("https://h/p", h("https://h/p")),
        ("HTTPS://h/p", h("https://h/p")), ("https://h:8443/p", h("https://h:8443/p")),
        ("ssh://user@h:22/p", h("https://h/p")), ("ssh://h/p", h("https://h/p")),
        ("user@h:p", h("https://h/p")), ("h:p", h("https://h/p")),
        ("git@h:/srv/p.git", h("https://h/srv/p.git")),
        ("/srv/r.git", Local), ("./r", Local), ("../r", Local), ("r", Local), ("./a:b", Local),
        ("C:/r", Local), ("file:///srv/r.git", Local),
        ("http://h/p", Unknown), ("git://h/p", Unknown), ("ext::sh -c x", Unknown), ("https:///p", Unknown),
    ] {
        assert_eq!(map_url(url), want, "{url}");
    }
}

#[test]
fn classify_exit_0_is_public() {
    assert_eq!(classify(&Whole { ok: true, out: b"abc\trefs/heads/main\n".to_vec(), err: vec![] }), Verdict::Public);
}

#[test]
fn classify_other_failure_is_unknown_with_a_scrubbed_reason() {
    assert_eq!(classify(&fail("\nfatal: unable to access 'https://u:secret@h/p/': Could not resolve host: h\n")),
        Verdict::Unknown("git ls-remote failed: fatal: unable to access 'https://h/p/': Could not resolve host: h".into()));
    assert_eq!(classify(&fail("")), Verdict::Unknown("git ls-remote failed".into()));
    assert!(matches!(classify(&fail("fatal: unable to access 'https://h/p/': SSL certificate problem")), Verdict::Unknown(_)));
    assert!(matches!(classify(&fail("The requested URL returned error: 403")), Verdict::Unknown(_)));
}

#[test]
fn private_text_is_this_list() {
    assert_eq!(PRIVATE_TEXT, &["could not read Username", "could not read Password", "Authentication failed",
        "Repository not found", "' not found", "returned error: 401", "returned error: 404",
        "HTTP Basic: Access denied", "could not be found or you don't have permission"]);
}

/// `line` as git prints it, upper-cased and lower-cased, is `Private`.
fn assert_private(line: &str) {
    for l in [line.to_string(), line.to_uppercase(), line.to_lowercase()] {
        assert_eq!(classify(&fail(&l)), Verdict::Private, "{l}");
    }
}

#[test]
fn private_could_not_read_username() {
    assert_private("fatal: could not read Username for 'https://h': terminal prompts disabled");
}

#[test]
fn private_could_not_read_password() {
    assert_private("fatal: could not read Password for 'https://u@h': terminal prompts disabled");
}

#[test]
fn private_authentication_failed() {
    assert_private("fatal: Authentication failed for 'https://h/p/'");
}

#[test]
fn private_repository_not_found() {
    assert_private("remote: Repository not found.");
}

#[test]
fn private_quoted_not_found() {
    assert_private("fatal: repository 'https://h/p/' not found");
}

#[test]
fn private_error_401() {
    assert_private("fatal: unable to access 'https://h/p/': The requested URL returned error: 401");
}

#[test]
fn private_error_404() {
    assert_private("fatal: unable to access 'https://h/p/': The requested URL returned error: 404");
}

#[test]
fn private_http_basic_access_denied() {
    assert_private("remote: HTTP Basic: Access denied. If a password was provided for Git authentication, it was incorrect");
}

#[test]
fn private_gitlab_not_found_or_no_permission() {
    assert_private("remote: The project you were looking for could not be found or you don't have permission to view it.");
}

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

#[test]
fn probe_command_is_anonymous() {
    use std::ffi::OsStr;
    let home = tempfile::TempDir::new().unwrap();
    let c = probe_command("https://h/p", home.path());
    let args: Vec<&OsStr> = c.get_args().collect();
    let tail: Vec<&OsStr> = ["-c", "credential.helper=", "-c", "core.askPass=", "ls-remote", "--heads", "https://h/p"]
        .iter().map(OsStr::new).collect();
    assert!(args.ends_with(&tail), "{args:?}");
    for kv in ["gc.auto=0", "maintenance.auto=false", "fetch.writeCommitGraph=false"] {
        assert!(args.contains(&OsStr::new(kv)), "{kv}");
    }
    let envs: std::collections::BTreeMap<&OsStr, Option<&OsStr>> = c.get_envs().collect();
    let e = |k: &str| envs.get(OsStr::new(k)).copied();
    assert_eq!(e("GIT_CONFIG_GLOBAL"), Some(Some(OsStr::new(NULL))));
    assert_eq!(e("GIT_CONFIG_SYSTEM"), Some(Some(OsStr::new(NULL))));
    assert_eq!(e("GIT_CONFIG_NOSYSTEM"), Some(Some(OsStr::new("1"))));
    assert_eq!(e("HOME"), Some(Some(home.path().as_os_str())));
    assert_eq!(e("GIT_TERMINAL_PROMPT"), Some(Some(OsStr::new("0"))));
    assert_eq!(e("GIT_ASKPASS"), Some(None));
    assert_eq!(e("SSH_ASKPASS"), Some(None));
    for k in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"] {
        assert_eq!(e(k), None, "{k} stays as the environment has it");
    }
    assert_eq!(c.get_current_dir(), Some(home.path()));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::fs;

    fn far() -> std::time::Instant { std::time::Instant::now() + std::time::Duration::from_secs(3600) }

    /// Runs `git <args>` in `dir` and asserts it succeeds.
    fn git(dir: &std::path::Path, args: &[&str]) {
        let s = std::process::Command::new("git").current_dir(dir).args(args).status().unwrap();
        assert!(s.success(), "git {args:?}");
    }

    #[test]
    fn ls_remote_closed_port_is_unknown() {
        assert!(matches!(LsRemote.probe("https://127.0.0.1:1/p"), Verdict::Unknown(_)));
    }

    #[test]
    fn remote_urls_gives_fetch_then_push() {
        let t = tempfile::TempDir::new().unwrap();
        let d = t.path();
        git(d, &["init", "-q"]);
        git(d, &["remote", "add", "origin", "https://u:p@h/p"]);
        git(d, &["remote", "set-url", "--push", "origin", "ssh://git@h/p"]);
        assert_eq!(remote_urls(d, "origin", far()), Ok(vec!["https://u:p@h/p".to_string(), "ssh://git@h/p".to_string()]));
    }

    #[test]
    fn remote_urls_gives_one_url_when_push_is_fetch() {
        let t = tempfile::TempDir::new().unwrap();
        let d = t.path();
        git(d, &["init", "-q"]);
        git(d, &["remote", "add", "origin", "https://h/p"]);
        assert_eq!(remote_urls(d, "origin", far()), Ok(vec!["https://h/p".to_string()]));
    }

    #[test]
    fn remote_urls_missing_remote_fails() {
        let t = tempfile::TempDir::new().unwrap();
        let d = t.path();
        git(d, &["init", "-q"]);
        assert_eq!(remote_urls(d, "nope", far()), Err(zygo_work::repo::RunError::Failed));
    }

    use sha2::{Digest, Sha256};
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use zygo_core::paths::Paths;
    use zygo_mail_sync::env::{Clock, Probe};
    use zygo_mail_sync::state::{load, save, State, SyncPaths};

    struct At(SystemTime);
    impl Clock for At { fn now(&self) -> SystemTime { self.0 } }
    /// Minutes after T0 (negative: before).
    fn at(min: i64) -> At {
        let t0 = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        At(if min >= 0 { t0 + Duration::from_secs(min as u64 * 60) } else { t0 - Duration::from_secs((-min) as u64 * 60) })
    }
    /// Answers `Private` unless `by` names the URL; records each URL it is asked.
    struct Fake { by: Vec<(&'static str, Verdict)>, seen: Mutex<Vec<String>> }
    impl Probe for Fake {
        fn probe(&self, u: &str) -> Verdict {
            self.seen.lock().unwrap().push(u.to_string());
            self.by.iter().find(|(k, _)| *k == u).map_or(Verdict::Private, |(_, v)| v.clone())
        }
    }
    fn fake(by: &[(&'static str, Verdict)]) -> Fake { Fake { by: by.to_vec(), seen: Mutex::new(vec![]) } }
    fn seen(f: &Fake) -> Vec<String> { f.seen.lock().unwrap().clone() }
    fn urls(u: &[&str]) -> Vec<String> { u.iter().map(|s| s.to_string()).collect() }
    fn sha(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }
    fn setup() -> (tempfile::TempDir, SyncPaths) {
        let t = tempfile::TempDir::new().unwrap();
        let p = t.path();
        let paths = Paths {
            config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
            sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
            plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"),
            usage_db: p.join("data/usage.db"),
        };
        let sp = SyncPaths::new(&paths, "0123456789abcdef").unwrap();
        (t, sp)
    }

    #[test]
    fn check_uses_the_cache_at_59_minutes() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        assert_eq!(check(&sp, &urls(&["https://h/p"]), &f, &at(0)), Verdict::Private);
        assert_eq!(seen(&f).len(), 1);
        assert_eq!(check(&sp, &urls(&["https://h/p"]), &f, &at(59)), Verdict::Private);
        assert_eq!(seen(&f).len(), 1);
    }

    #[test]
    fn check_probes_again_at_61_minutes() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        check(&sp, &urls(&["https://h/p"]), &f, &at(0));
        check(&sp, &urls(&["https://h/p"]), &f, &at(61));
        assert_eq!(seen(&f).len(), 2);
    }

    #[test]
    fn check_probes_again_after_a_url_change() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        check(&sp, &urls(&["https://h/p"]), &f, &at(0));
        check(&sp, &urls(&["https://h/q"]), &f, &at(1));
        assert_eq!(seen(&f), urls(&["https://h/p", "https://h/q"]));
    }

    #[test]
    fn check_probes_again_when_the_clock_is_before_the_cache() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        check(&sp, &urls(&["https://h/p"]), &f, &at(0));
        check(&sp, &urls(&["https://h/p"]), &f, &at(-1));
        assert_eq!(seen(&f).len(), 2);
    }

    #[test]
    fn cache_holds_sha256_hex_only() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        check(&sp, &urls(&["https://u:secret@h/p", "ssh://git@h:22/p"]), &f, &at(0));
        let text = fs::read_to_string(&sp.state).unwrap();
        for bad in ["secret", "h/p", "https", "ssh"] {
            assert!(!text.contains(bad), "{bad} in {text}");
        }
        let p = load(&sp).unwrap().public.unwrap();
        assert_eq!(p.url_sha256, { let mut v = vec![sha("https://h/p"), sha("ssh://h:22/p")]; v.sort(); v });
        assert_eq!(p.verdict, "private");
        assert_eq!(p.at, 1_800_000_000);
    }

    #[test]
    fn check_probes_each_distinct_mapped_url_once() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        check(&sp, &urls(&["https://u@h/p", "ssh://git@h:22/p", "git@h:p"]), &f, &at(0));
        assert_eq!(seen(&f), urls(&["https://h/p"]));
    }

    #[test]
    fn public_for_any_url_wins() {
        let (_t, sp) = setup();
        let f = fake(&[("https://h/b", Verdict::Public)]);
        assert_eq!(check(&sp, &urls(&["https://h/a", "https://h/b"]), &f, &at(0)), Verdict::Public);
    }

    #[test]
    fn local_and_unknown_need_no_probe() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        assert_eq!(check(&sp, &urls(&["/srv/r.git", "file:///srv/r.git"]), &f, &at(0)), Verdict::Local);
        assert_eq!(check(&sp, &urls(&["git://h/p"]), &f, &at(0)), Verdict::Unknown("the remote URL has no https form".into()));
        assert_eq!(check(&sp, &[], &f, &at(0)), Verdict::Unknown("the remote has no URL".into()));
        assert!(seen(&f).is_empty());
    }

    #[test]
    fn cached_unknown_keeps_unknown() {
        let (_t, sp) = setup();
        let f = fake(&[("https://h/p", Verdict::Unknown("x".into()))]);
        assert_eq!(check(&sp, &urls(&["https://h/p"]), &f, &at(0)), Verdict::Unknown("x".into()));
        assert_eq!(check(&sp, &urls(&["https://h/p"]), &f, &at(30)),
            Verdict::Unknown("the last check, under 1 h ago, could not tell".into()));
        assert_eq!(seen(&f).len(), 1);
    }

    #[test]
    fn unreadable_state_runs_the_probe_and_writes_nothing() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        fs::create_dir_all(&sp.sync).unwrap();
        fs::write(&sp.state, b"not json").unwrap();
        assert_eq!(check(&sp, &urls(&["https://h/p"]), &f, &at(0)), Verdict::Private);
        assert_eq!(seen(&f).len(), 1);
        assert_eq!(fs::read(&sp.state).unwrap(), b"not json");
    }

    #[test]
    fn check_keeps_the_other_state_fields() {
        let (_t, sp) = setup();
        let f = fake(&[]);
        save(&sp, &State { last_ok: Some(5), ci_note_shown: true, ..Default::default() }).unwrap();
        check(&sp, &urls(&["https://h/p"]), &f, &at(0));
        let s = load(&sp).unwrap();
        assert_eq!(s.last_ok, Some(5));
        assert!(s.ci_note_shown);
        assert!(s.public.is_some());
    }
}
