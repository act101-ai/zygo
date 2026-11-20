use zygo_config::display::{display_source, error_text, scrub_userinfo, EscapedLines};

fn tok() -> String { format!("tk{}", zygo_core::ids::ulid()) }

fn same(cases: &[(String, String)], t: &str) {
    for (i, o) in cases {
        assert_eq!(&display_source(i), o, "input {i}");
        assert!(!display_source(i).contains(t), "token in output for {i}");
    }
}

#[test]
fn display_source_drops_userinfo_of_every_url_form() {
    let t = tok();
    let c = |i: String, o: &str| (i, o.to_string());
    same(&[
        c(format!("git+https://bob:{t}@h.example/o/r.git"), "git+https://h.example/o/r.git"),
        c(format!("https://{t}@h.example/o/r"), "https://h.example/o/r"),
        c(format!("http://bob:{t}@h.example:8080/o"), "http://h.example:8080/o"),
        c(format!("git+ssh://git:{t}@h.example/o/r.git"), "git+ssh://h.example/o/r.git"),
        c("ssh://git@h.example/o/r".into(), "ssh://h.example/o/r"),
        c(format!("HTTPS://bob:{t}@h/x"), "HTTPS://h/x"),
        c(format!("https://bob:{t}@h"), "https://h"),
    ], &t);
}

#[test]
fn display_source_fails_closed_on_odd_userinfo() {
    let t = tok();
    let c = |i: String, o: &str| (i, o.to_string());
    same(&[
        c("https://bob:pa/ss@h/x".into(), "https://h/x"),
        c("https://bob:p#q@h/x".into(), "https://h/x"),
        c("https://bob:p?q@h/x".into(), "https://h/x"),
        c(format!("https://bob:p@{t}@h/o"), "https://h/o"),
        c("https://h?t=a@b/x".into(), "https://b/x"),
        c("https://h/o/r@v1".into(), "https://v1"),
        c(format!("ssh://git:{t}@[::1]:22/o"), "ssh://[::1]:22/o"),
        c("https://[::1]:8080/o".into(), "https://[::1]:8080/o"),
    ], &t);
}

#[test]
fn display_source_scp_form_drops_the_user() {
    let t = tok();
    let c = |i: String, o: &str| (i, o.to_string());
    same(&[
        c("git@github.com:o/r.git".into(), "github.com:o/r.git"),
        c(format!("bob:{t}@h.example:o/r"), "h.example:o/r"),
    ], &t);
}

#[test]
fn display_source_fails_closed_on_a_bad_scheme() {
    let t = tok();
    let c = |i: String, o: &str| (i, o.to_string());
    same(&[
        c(format!(" git+https://bob:{t}@h/o"), "h/o"),
        c(format!("x y://u:{t}@h"), "h"),
        c(format!("\tgit+https://bob:{t}@h/o"), "h/o"),
        c(format!("x y://u:{t} k@h"), "h"),
        c(format!("bob:{t}@h:path://x"), "h:path://x"),
        c(format!("{t}@x://h"), "x://h"),
        c(format!("u:{t}@h https://x"), "h https://x"),
        c(format!("u:{t}@h:o/r\nhttps://x"), "h:o/r\nhttps://x"),
        c(format!("g:{t}@h:o/r"), "h:o/r"),
        c(format!("u:{t}@h"), "h"),
        c(format!("C:{t}@h"), "h"),
    ], &t);
}

#[test]
fn display_source_fails_closed_on_a_non_local_at() {
    let t = tok();
    let c = |i: String, o: &str| (i, o.to_string());
    same(&[
        c(format!("bob:p/{t}@h:o/r"), "h:o/r"),
        c("bob:p/ss@h:o/r".into(), "h:o/r"),
        c("rel/a@b:c".into(), "b:c"),
        c(format!("{t}@h"), "h"),
    ], &t);
}

const PLAIN: &[&str] = &["/tmp/a@b", "./x@y", "../x@y", ".\\x@y", "..\\x@y", "~/x@y", "\\\\host\\share@x", "C:\\a@b", "C:/a@b",
    "/a://b@c", "C:\\a://b@c", "github:o/r", "https://h.example/o/r", ""];

#[test]
fn display_source_keeps_local_paths_and_plain_forms() {
    for s in PLAIN { assert_eq!(display_source(s), *s); }
}

#[test]
fn display_source_is_idempotent() {
    let t = tok();
    let mut all: Vec<String> = PLAIN.iter().map(|s| s.to_string()).collect();
    all.extend([
        format!("git+https://bob:{t}@h.example/o/r.git"), format!("https://{t}@h.example/o/r"), "ssh://git@h.example/o/r".into(),
        format!("HTTPS://bob:{t}@h/x"), "https://bob:pa/ss@h/x".into(), "https://h?t=a@b/x".into(), "https://h/o/r@v1".into(),
        format!("ssh://git:{t}@[::1]:22/o"), "https://[::1]:8080/o".into(), "git@github.com:o/r.git".into(),
        format!("bob:{t}@h.example:o/r"), format!("https://bob:p@{t}@h/o"),
        format!(" git+https://bob:{t}@h/o"), format!("x y://u:{t}@h"), format!("\tgit+https://bob:{t}@h/o"),
        format!("x y://u:{t} k@h"), format!("bob:p/{t}@h:o/r"), "rel/a@b:c".into(), "a@b".into(),
    ]);
    for s in all {
        let once = display_source(&s);
        assert_eq!(display_source(&once), once, "input {s}");
    }
}

#[test]
fn scrub_userinfo_cleans_urls_in_git_text() {
    let t = tok();
    assert_eq!(scrub_userinfo(&format!("fatal: unable to access 'https://bob:{t}@h.example/o/r.git/': x")),
        "fatal: unable to access 'https://h.example/o/r.git/': x");
    assert_eq!(scrub_userinfo("clone ssh://git@h/o failed"), "clone ssh://h/o failed");
}

#[test]
fn scrub_userinfo_follows_the_token_rule() {
    let t = tok();
    let cases = [
        ("'https://bob:pa/ss@h/x'".to_string(), "'https://h/x'"),
        ("https://bob:p#q@h/x".into(), "https://h/x"),
        ("https://bob:p?q@h/x".into(), "https://h/x"),
        ("https://h?t=a@b/x".into(), "https://b/x"),
        ("https://h/o/r@v1".into(), "https://v1"),
        (format!("ssh://git:{t}@[::1]:22/o"), "ssh://[::1]:22/o"),
        (format!("see https://a:{t}@h/x and https://b:{t}@h2/y."), "see https://h/x and https://h2/y."),
        (format!("https://a:{t}@h/x https://h2/o"), "https://h/x https://h2/o"),
    ];
    for (i, o) in cases {
        assert_eq!(scrub_userinfo(&i), o, "input {i}");
        assert!(!scrub_userinfo(&i).contains(&t));
    }
}

#[test]
fn scrub_userinfo_leaves_other_text() {
    for s in ["mail a@b.example", "git timed out", "git failed: ssh: Could not resolve hostname h", "git@github.com: Permission denied (publickey)."] {
        assert_eq!(scrub_userinfo(s), s);
    }
}

/// The local rule of `display_source`, restated for the fuzz test.
fn local(s: &str) -> bool {
    let b = s.as_bytes();
    ["/", "./", "../", ".\\", "..\\", "~/", "\\\\"].iter().any(|p| s.starts_with(p))
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
}

/// Ported from the round 2 review: no token survives for any non-local input, and a local input is unchanged.
#[test]
fn display_source_fuzz_never_shows_a_token() {
    let t = tok();
    let shapes: Vec<String> = [
        "https://u:T@h/o", " git+https://u:T@h/o", "\tgit+https://u:T@h", "\r\nhttps://u:T@h", "x y://u:T@h", "x y://u:T k@h",
        "https://h/?r=git+https://u:T@h2", "a://b://u:T@h", "git+https://u:T@h/x://y", "https://u:T\r\n@h", "'https://u:T@h'",
        "\"https://u:T@h\"", "`https://u:T@h`", "https://u:T'@h", "https://u:T\"x@h", "x'y://u:T'z@h", "ü://u:T@h", "https://ü:T@h",
        "file://u:T@/x", "FILE://u:T@localhost/x", "C:\\a://u:T@h", "\\\\host\\share://u:T@h", "u:T@h:o/r", "g:T@h:o/r",
        "u:T@h:o/r?x=https://h", "bob:T@h:path://x", "git@h:o/r#https://x", "T@x://h", "u:T@h https://x", "x:T@h:a/b://c",
        "z:p/T@h:o", "\\\\host\\T@x", "~/T@h", ".\\T@h", "u:T@h", "a b:T@h", "https:/u:T@h", "https:\\\\u:T@h", "https//u:T@h",
        "git+ssh://git:T@[::1]:22/o", "ssh:/ /u:T@h", "u:T@h\n", "u:T@h:o/r\nhttps://x", "https://u:T@h\nhttps://x",
        ":://u:T@h", "://u:T@h", "1x://u:T@h", "-x://u:T@h", "https://u%40:T@h", "https://u:T%40@h", "https://u:T＠h@h2", "C:T@h",
    ].iter().map(|s| s.replace('T', &t)).collect();
    let mut all = shapes;
    for b in ["https://u:p@h/o", "git+https://u:p@h/o", " git+https://u:p@h/o", "bob:p@h:o/r", "x y://a b@h", "a://b://c@d", "\\\\h\\s@x"] {
        // userinfo positions only: a token typed into the scheme name itself is not userinfo, and the kept scheme would show it.
        let (from, at) = (b.find("://").map_or(0, |p| p + 3), b.rfind('@').unwrap());
        all.extend((from..=at).filter(|&i| b.is_char_boundary(i)).map(|i| format!("{}{t}{}", &b[..i], &b[i..])));
    }
    let mut leaks = vec![];
    for s in &all {
        let d = display_source(s);
        if local(s) { if &d != s { leaks.push(format!("local changed {s:?} -> {d:?}")); } }
        else if d.contains(&t) { leaks.push(format!("display_source({s:?}) = {d:?}")); }
    }
    for l in ["/tmp/a@b", "/", "./x@y", "../x@y", "C:\\a@b", "C:/a@b", "c:\\x\\y@z", "/tmp/a b@c", "/tmp/ü@x", "./a:b@c", "/a://b@c",
        "C:\\a://b@c", "\\\\host\\share@x", ".\\x@y", "~/x@y"] {
        if display_source(l) != l { leaks.push(format!("local changed {l:?}")); }
    }
    for s in ["see https://u:T@h/x.", "x\thttps://u:T@h", "a://b://u:T@h", "(https://u:T@h)", "<https://u:T@h>"] {
        let s = s.replace('T', &t);
        if scrub_userinfo(&s).contains(&t) { leaks.push(format!("scrub_userinfo({s:?})")); }
    }
    assert!(leaks.is_empty(), "{} leaks:\n{}", leaks.len(), leaks.join("\n"));
}

#[test]
fn verb_errors_are_escaped_unless_escaped_lines() {
    assert_eq!(error_text(&anyhow::anyhow!("a\u{1b}[2K\nb")), "a\\u{1b}[2K\\nb");
    assert_eq!(error_text(&EscapedLines::new(["a", "b\u{1b}[2K"]).into()), "a\nb\\u{1b}[2K");
}
