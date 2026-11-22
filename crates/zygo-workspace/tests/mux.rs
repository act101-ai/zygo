//! Node W3: the multiplexer trait, the tmux implementation and the tmux test gate (W§13, W§19).
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zygo_workspace::mux::{
    check_path, format_escape, hook_command, parse_pane_line, vis_decode, PaneRow, ParseError, UnsafePath, WindowRow,
    PANE_FIELDS, PANE_FORMAT, UNSAFE_PATH_MESSAGE,
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
