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
