//! Node W3: the multiplexer trait, the tmux implementation and the tmux test gate (W§13, W§19).
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use zygo_workspace::mux::tmux::Tmux;
#[cfg(unix)]
use zygo_workspace::mux::{HookScope, Multiplexer, NewSession, NewWindow, SessionAnswer, Split};
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

/// The log stub of step 7: each call appends `LC_ALL=<value or unset> <arguments>` to `<dir>/log`.
#[cfg(unix)]
fn log_stub(dir: &Path) -> PathBuf {
    stub(dir, "tmux", &format!("printf '%s\\n' \"LC_ALL=${{LC_ALL-unset}} $*\" >> '{}'", dir.join("log").display()));
    dir.join("tmux")
}

#[cfg(unix)]
fn os(v: &[&str]) -> Vec<OsString> { v.iter().map(OsString::from).collect() }

#[cfg(unix)]
#[test]
fn every_call_passes_the_socket_the_c_locale_and_u() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let t = Tmux::new(log_stub(dir.path()), Some(PathBuf::from("/tmp/zygo-mux-x/s")));
    let argv = os(&["sleep", "600"]);
    let env = vec![(OsString::from("ZTEST"), OsString::from("1"))];
    let d = Path::new("/w/d#x");
    let _ = t.version();
    let _ = t.socket_path("zygo-t");
    let _ = t.session_exists("zygo-t");
    let _ = t.new_session(&NewSession { name: "zygo-t", start_dir: d, width: 173, height: 47, env: &env, argv: &argv });
    let _ = t.resize_window("zygo-t", 0, 173, 47);
    let _ = t.unset_window_option("zygo-t", 0, "window-size");
    let _ = t.set_window_option("=zygo-t:1", "remain-on-exit", "off");
    let _ = t.set_env("zygo-t", "ZYGO_WORKSPACE", "0123456789abcdef");
    let _ = t.hide_env("zygo-t", "ZP_HIDE");
    let _ = t.set_session_option("zygo-t", "@zygo-ws", "0123456789abcdef");
    let _ = t.show_session_option("zygo-t", "@zygo-ws");
    let _ = t.set_hook("zygo-t", HookScope::Session, "after-new-window", "run-shell -b \"true\"");
    let _ = t.set_hook("zygo-t", HookScope::Window(3), "window-unlinked", "run-shell -b \"true\"");
    let _ = t.new_window(&NewWindow { session: "zygo-t", name: Some("a#b"), start_dir: d, argv: &argv });
    let _ = t.split_window(&Split { target: "%1", start_dir: Some(d), argv: &argv });
    let _ = t.select_layout("zygo-t", 1, "tiled");
    let _ = t.select_window("zygo-t", 1);
    let _ = t.select_pane("%1");
    let _ = t.zoom("%1");
    let _ = t.list_panes("zygo-t");
    let _ = t.show_pane_option("%1", "@zygo-pane");
    let _ = t.set_pane_option("%1", "@zygo-pane", "kabcdefgh");
    let _ = t.display("=zygo-t:", "#{@zygo-ws}");
    let _ = t.kill_session("zygo-t");
    let _ = t.switch_client("zygo-t");
    assert_eq!(t.attach("zygo-t"), Ok(0));
    let lines: Vec<String> = read_to_string(dir.path().join("log")).unwrap().lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 26, "{lines:#?}");
    for (i, l) in lines.iter().enumerate() {
        let args = l.split_once(' ').unwrap().1;
        if i == 25 {
            assert_eq!(args, "-S /tmp/zygo-mux-x/s attach-session -t =zygo-t:");
            continue;
        }
        assert!(args.starts_with("-S /tmp/zygo-mux-x/s -u "), "{l}");
        if i != 3 {
            assert!(l.starts_with("LC_ALL=C "), "{l}");
        }
    }
    assert_eq!(lines[2], "LC_ALL=C -S /tmp/zygo-mux-x/s -u has-session -t =zygo-t:");
    assert_eq!(
        lines[3],
        "LC_ALL=unset -S /tmp/zygo-mux-x/s -u new-session -d -P -F #{window_index} -s zygo-t -c /w/d##x -x 173 -y 47 -- sleep 600"
    );
    assert_eq!(lines[4], "LC_ALL=C -S /tmp/zygo-mux-x/s -u resize-window -t =zygo-t:0 -x 173 -y 47");
    assert_eq!(lines[5], "LC_ALL=C -S /tmp/zygo-mux-x/s -u set-option -w -u -t =zygo-t:0 window-size");
    assert_eq!(lines[6], "LC_ALL=C -S /tmp/zygo-mux-x/s -u set-option -w -t =zygo-t:1 remain-on-exit off");
    assert_eq!(lines[8], "LC_ALL=C -S /tmp/zygo-mux-x/s -u set-environment -r -t =zygo-t: ZP_HIDE");
    assert_eq!(lines[10], "LC_ALL=C -S /tmp/zygo-mux-x/s -u show-options -qv -t =zygo-t: @zygo-ws");
    assert_eq!(lines[12], "LC_ALL=C -S /tmp/zygo-mux-x/s -u set-hook -w -t =zygo-t:3 window-unlinked run-shell -b \"true\"");
    assert_eq!(
        lines[13],
        "LC_ALL=C -S /tmp/zygo-mux-x/s -u new-window -d -P -F #{window_index} -t =zygo-t: -n a##b -c /w/d##x -- sleep 600"
    );
    assert_eq!(lines[14], "LC_ALL=C -S /tmp/zygo-mux-x/s -u split-window -d -P -F #{pane_id} -t %1 -c /w/d##x -- sleep 600");
    assert_eq!(lines[19], format!("LC_ALL=C -S /tmp/zygo-mux-x/s -u list-panes -s -t =zygo-t: -F {PANE_FORMAT}"));
    assert_eq!(lines[20], "LC_ALL=C -S /tmp/zygo-mux-x/s -u show-options -p -qv -t %1 @zygo-pane");
}

#[cfg(unix)]
#[test]
fn no_socket_means_no_s_flag() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let t = Tmux::new(log_stub(dir.path()), None);
    let _ = t.session_exists("zygo-t");
    let _ = t.set_env("zygo-t", "A", "1");
    let _ = t.list_panes("zygo-t");
    let lines: Vec<String> = read_to_string(dir.path().join("log")).unwrap().lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 3, "{lines:#?}");
    assert!(lines.iter().all(|l| !l.contains(" -S ")), "{lines:#?}");
    assert_eq!(lines[0], "LC_ALL=C -u has-session -t =zygo-t:");
}

#[cfg(unix)]
#[test]
fn stderr_maps_to_answers_and_errors() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let at = |name: &str, body: &str| {
        stub(dir.path(), name, body);
        Tmux::new(dir.path().join(name), None)
    };
    let argv = os(&["sleep", "600"]);
    let split = Split { target: "%1", start_dir: None, argv: &argv };

    assert_eq!(at("ok", "exit 0").session_exists("zygo-t"), SessionAnswer::Exists);
    let t = at("nosession", "echo \"can't find session: zygo-t\" >&2; exit 1");
    assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Gone);
    assert_eq!(t.list_panes("zygo-t"), Err(MuxError::Gone { message: "can't find session: zygo-t".into() }));
    assert_eq!(at("exited", "echo 'server exited unexpectedly' >&2; exit 1").session_exists("zygo-t"), SessionAnswer::Gone);
    assert_eq!(
        at("nosuch", "echo 'no such session: =zygo-t:' >&2; exit 1").set_env("zygo-t", "A", "1"),
        Err(MuxError::Gone { message: "no such session: =zygo-t:".into() })
    );
    assert_eq!(
        at("nowindow", "echo \"can't find window: 9\" >&2; exit 1").resize_window("zygo-t", 9, 10, 10),
        Err(MuxError::NoTarget { message: "can't find window: 9".into() })
    );
    assert_eq!(
        at("denied", "echo 'error connecting to /s (Permission denied)' >&2; exit 1").session_exists("zygo-t"),
        SessionAnswer::Error { message: "error connecting to /s (Permission denied)".into() }
    );
    assert!(at("nospace", "echo 'no space for new pane' >&2; exit 1").split_window(&split).unwrap_err().is_no_space());

    let t = at("slow", "exec sleep 10");
    let t0 = Instant::now();
    assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Error { message: "tmux timed out after 2 s".into() });
    assert!(t0.elapsed() < Duration::from_secs(4), "{:?}", t0.elapsed());
    assert_eq!(t.set_env("zygo-t", "A", "1"), Err(MuxError::TimedOut));

    let t = at("held", "(sleep 5 &) ; exit 0");
    let t0 = Instant::now();
    assert_eq!(t.set_env("zygo-t", "A", "1"), Ok(()));
    assert!(t0.elapsed() < Duration::from_secs(4), "{:?}", t0.elapsed());

    let t = Tmux::new(PathBuf::from("/nonexistent/tmux"), None);
    assert!(matches!(t.session_exists("zygo-t"), SessionAnswer::Error { .. }));
    assert!(matches!(t.version(), Err(MuxError::Spawn { .. })));
}

#[cfg(unix)]
#[test]
fn short_argument_lists_are_refused_before_any_spawn() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let t = Tmux::new(log_stub(dir.path()), None);
    let refused = Err(MuxError::Failed { message: "tmux needs an argument list of 2 or more items".into() });
    let d = Path::new("/w");
    for argv in [os(&["zygo"]), Vec::new()] {
        assert_eq!(t.new_session(&NewSession { name: "zygo-t", start_dir: d, width: 80, height: 24, env: &[], argv: &argv }), refused);
        assert_eq!(t.new_window(&NewWindow { session: "zygo-t", name: None, start_dir: d, argv: &argv }), refused);
        assert_eq!(
            t.split_window(&Split { target: "%1", start_dir: None, argv: &argv }),
            Err(MuxError::Failed { message: "tmux needs an argument list of 2 or more items".into() })
        );
    }
    assert!(!dir.path().join("log").exists());
}

#[cfg(unix)]
#[test]
fn created_indexes_and_ids_are_read() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let at = |name: &str, body: &str| {
        stub(dir.path(), name, body);
        Tmux::new(dir.path().join(name), None)
    };
    let argv = os(&["sleep", "600"]);
    let d = Path::new("/w");
    let ns = NewSession { name: "zygo-t", start_dir: d, width: 80, height: 24, env: &[], argv: &argv };
    let nw = NewWindow { session: "zygo-t", name: None, start_dir: d, argv: &argv };
    let sp = Split { target: "%1", start_dir: None, argv: &argv };

    let t = at("four", "printf '4\\n'");
    assert_eq!(t.new_session(&ns), Ok(4));
    assert_eq!(t.new_window(&nw), Ok(4));
    assert_eq!(at("pane", "printf '%%7\\n'").split_window(&sp), Ok("%7".into()));
    let t = at("junk", "printf 'x\\n'");
    assert!(matches!(t.new_session(&ns), Err(MuxError::Failed { .. })));
    assert!(matches!(t.split_window(&sp), Err(MuxError::Failed { .. })));
    let t = at("empty", "exit 0");
    assert_eq!(t.show_pane_option("%1", "@zygo-pane"), Ok(None));
    assert_eq!(t.show_session_option("zygo-t", "@zygo-ws"), Ok(None));
}
