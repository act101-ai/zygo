//! Node W3: the multiplexer trait, the tmux implementation and the tmux test gate (W§13, W§19).
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zygo_workspace::mux::{
    check_path, format_escape, hook_command, is_gone_message, is_no_target_message, parse_pane_line, parse_version,
    vis_decode, MuxError, PaneRow, ParseError, UnsafePath, WindowRow, MIN_VERSION, PANE_FIELDS, PANE_FORMAT,
    UNSAFE_PATH_MESSAGE,
};

/// Held by every test that forks a process, from its first stub write or fork until its last program has ended, so no
/// other test forks while a stub is open for writing (ETXTBSY).
#[cfg(unix)]
static STUBS: Mutex<()> = Mutex::new(());
#[cfg(unix)]
fn forks() -> std::sync::MutexGuard<'static, ()> { STUBS.lock().unwrap_or_else(|e| e.into_inner()) }

/// Writes an executable `#!/bin/sh` program; the caller holds `forks()`.
#[cfg(unix)]
fn stub(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let f = dir.join(name);
    std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn hook_command_quotes_the_path() {
    assert_eq!(
        hook_command(Path::new("/opt/zygo/bin/zygo"), "0123456789abcdef", "after-new-window").unwrap(),
        r#"run-shell -b "'/opt/zygo/bin/zygo' ws snapshot --ws-id 0123456789abcdef --event after-new-window""#
    );
    assert_eq!(
        hook_command(Path::new("/opt/a#b/zygo"), "0123456789abcdef", "pane-exited").unwrap(),
        r#"run-shell -b "'/opt/a##b/zygo' ws snapshot --ws-id 0123456789abcdef --event pane-exited""#
    );
}

#[test]
fn hook_command_refuses_unsafe_paths() {
    for p in ["/a'b/zygo", "/a\"b/zygo", "/a\\b/zygo", "/a$b/zygo", "/a\nb/zygo", "/a\tb/zygo", "/a\u{7f}b/zygo", "/a\u{85}b/zygo"] {
        assert_eq!(hook_command(Path::new(p), "0123456789abcdef", "after-new-window"), Err(UnsafePath), "{p:?}");
        assert_eq!(check_path(Path::new(p)), Err(UnsafePath));
    }
    assert_eq!(check_path(Path::new("/opt/a#b c/zygo")), Ok("/opt/a#b c/zygo"));
    assert_eq!(
        UnsafePath.to_string(),
        "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it"
    );
    assert_eq!(UNSAFE_PATH_MESSAGE, UnsafePath.to_string());
}

#[cfg(unix)]
#[test]
fn check_path_refuses_a_path_that_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt;
    assert_eq!(check_path(Path::new(std::ffi::OsStr::from_bytes(b"/a\xffb"))), Err(UnsafePath));
}

#[test]
fn format_escape_doubles_each_hash() {
    assert_eq!(format_escape("a#b"), "a##b");
    assert_eq!(format_escape("##"), "####");
    assert_eq!(format_escape("#{session_name}"), "##{session_name}");
    assert_eq!(format_escape("w#(touch pwned)"), "w##(touch pwned)");
    assert_eq!(format_escape("plain/dir"), "plain/dir");
}

const LINE: &str = r"1|a\|b\\\\c\#d\ e|0|b25d,80x24,0,0,2|80|24|1|0|%3|0|1|k\\x|/home/u/x|y \| z";

#[test]
fn pane_format_has_thirteen_fields_path_last() {
    assert_eq!(PANE_FIELDS, 13);
    assert_eq!(PANE_FORMAT.split('|').count(), 13);
    assert!(PANE_FORMAT.starts_with("#{window_index}|#{q:window_name}|#{automatic-rename}|"));
    assert!(PANE_FORMAT.contains("|#{pane_id}|") && PANE_FORMAT.contains("|#{q:@zygo-pane}|"));
    assert!(PANE_FORMAT.ends_with("|#{pane_current_path}"));
}

#[test]
fn parse_pane_line_last_field_takes_the_rest() {
    let (w, p) = parse_pane_line(LINE).unwrap();
    // name: q-unescape gives a|b\\c#d e (tmux's vis form), vis_decode gives a|b\c#d e; key: q-unescape only
    assert_eq!(
        w,
        WindowRow {
            index: 1,
            name: r"a|b\c#d e".into(),
            automatic_rename: false,
            layout: "b25d,80x24,0,0,2".into(),
            width: 80,
            height: 24,
            active: true,
            zoomed: false
        }
    );
    assert_eq!(
        p,
        PaneRow { id: "%3".into(), index: 0, active: true, key: r"k\x".into(), cwd: PathBuf::from(r"/home/u/x|y \| z") }
    );
    assert_eq!(parse_pane_line("0|zsh|1|c0de,80x24,0,0,0|80|24|1|0|%0|0|1||/tmp").unwrap().1.key, "");
}

#[test]
fn parse_pane_line_refuses_short_and_bad_lines() {
    assert_eq!(parse_pane_line("0|zsh|1|c0de,80x24,0,0,0|80|24|1|0|%0|0|1|k"), Err(ParseError::TooFewFields { found: 12 }));
    assert_eq!(parse_pane_line(""), Err(ParseError::TooFewFields { found: 1 }));
    assert_eq!(parse_pane_line("x|zsh|1|l|80|24|1|0|%0|0|1||/tmp"), Err(ParseError::BadField { field: "window_index" }));
    assert_eq!(parse_pane_line("0|zsh|2|l|80|24|1|0|%0|0|1||/tmp"), Err(ParseError::BadField { field: "automatic-rename" }));
    assert_eq!(parse_pane_line("0|zsh|1|l|80|24|1|0|0|0|1||/tmp"), Err(ParseError::BadField { field: "pane_id" }));
}

#[test]
fn vis_decode_gives_the_typed_name() {
    assert_eq!(vis_decode(r"x\\y"), r"x\y");
    assert_eq!(vis_decode(r"c\001d"), "c\u{1}d");
    assert_eq!(vis_decode(r"a\tb\nc"), "a\tb\nc");
    assert_eq!(vis_decode("u€v"), "u€v");
    assert_eq!(vis_decode(r"\303\251"), "é");
    assert_eq!(vis_decode(r"a\qb"), r"a\qb");
}

#[test]
fn gone_messages_are_the_au87_answers() {
    for m in [
        "can't find session: zygo-t",
        "no such session: =zygo-t:",
        "no server running on /tmp/x/s",
        "server exited unexpectedly",
        "error connecting to /tmp/x/s (No such file or directory)",
        "error connecting to /tmp/x/s (Connection refused)",
        "can't find session: zygo-t\nmore",
    ] {
        assert!(is_gone_message(m), "{m}");
    }
    for m in [
        "error connecting to /tmp/x/s (Permission denied)",
        "can't find window: 9",
        "no such window: =zygo-t:9",
        "invalid option: @zygo-pane",
        "",
    ] {
        assert!(!is_gone_message(m), "{m}");
    }
}

#[test]
fn no_target_messages() {
    for m in ["can't find window: 9", "no such window: =zygo-t:9", "can't find pane: %99", "no such pane: %99"] {
        assert!(is_no_target_message(m), "{m}");
    }
    for m in ["can't find session: zygo-t", "no such session: =zygo-t:", "no space for new pane"] {
        assert!(!is_no_target_message(m), "{m}");
    }
}

#[test]
fn parse_version_reads_tmux_v_output() {
    assert_eq!(parse_version("tmux 3.2a\n"), Some((3, 2)));
    assert_eq!(parse_version("tmux 3.7c"), Some((3, 7)));
    assert_eq!(parse_version("tmux next-3.6"), Some((3, 6)));
    assert_eq!(parse_version("tmux 3.1"), Some((3, 1)));
    assert_eq!(parse_version("tmux master"), None);
    assert_eq!(parse_version(""), None);
    assert_eq!(MIN_VERSION, (3, 2));
}

#[test]
fn mux_error_kinds() {
    assert!(MuxError::Gone { message: "can't find session: s".into() }.is_gone());
    assert!(!MuxError::NoTarget { message: "can't find window: 9".into() }.is_gone());
    assert!(!MuxError::TimedOut.is_gone());
    assert_eq!(MuxError::TimedOut.to_string(), "tmux timed out after 2 s");
    assert!(MuxError::Failed { message: "no space for new pane".into() }.is_no_space());
    assert!(MuxError::Failed { message: "size or position no space for a new pane".into() }.is_no_space());
    assert!(!MuxError::Failed { message: "other".into() }.is_no_space());
}
