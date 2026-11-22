//! Node W3: the multiplexer trait, the tmux implementation and the tmux test gate (W§13, W§19).
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::fs::{self, read_to_string};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use zygo_workspace::mux::testing::PrivateServer;
#[cfg(unix)]
use zygo_workspace::mux::testing::{require_tmux, require_tmux_at, tmux_gate, SKIP_PREFIX};
#[cfg(unix)]
use zygo_workspace::mux::tmux::Tmux;
#[cfg(unix)]
use zygo_workspace::mux::{window_target, HookScope, Multiplexer, NewSession, NewWindow, SessionAnswer, Split};
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

#[test]
fn hook_command_refuses_a_bad_ws_id_or_event() {
    // Round 2 M7: checked in release builds too, not only by debug_assert!
    let z = Path::new("/opt/zygo/bin/zygo");
    for id in ["", "0123456789abcde", "0123456789ABCDEF", "0123456789abcdef0", "0123456789abcdeg", "01234567 9abcdef"] {
        assert_eq!(hook_command(z, id, "after-new-window"), Err(UnsafePath), "{id:?}");
    }
    for event in ["", "after new-window", "after-new-window;kill-server", "After", "a\"b", "a_b"] {
        assert_eq!(hook_command(z, "0123456789abcdef", event), Err(UnsafePath), "{event:?}");
    }
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
    // Round 2 M1: the path is last; `\` is doubled and LF and CR become `\n` and `\r`, so a record is one line.
    assert!(PANE_FORMAT.ends_with("|#{s/\\\\/\\\\\\\\/;s/\n/\\\\n/;s/\r/\\\\r/:pane_current_path}"), "{PANE_FORMAT}");
    assert_eq!(PANE_FORMAT.matches('\n').count(), 1);
    assert_eq!(PANE_FORMAT.matches('\r').count(), 1);
}

#[test]
fn parse_pane_line_decodes_the_path_field() {
    // Round 2 M1
    let (_, p) = parse_pane_line("0|zsh|1|l|80|24|1|0|%0|0|1||/a\\\\b\\nc\\rd|e\\x").unwrap();
    assert_eq!(p.cwd, PathBuf::from("/a\\b\nc\rd|e\\x"));
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

/// The log stub of step 7: each call appends `LC_ALL=<value or unset> <arguments>` and the record separator 0x1e to
/// `<dir>/log` (an argument can hold a newline), then prints `1` (not for `attach-session`, whose stdout is the
/// terminal).
#[cfg(unix)]
fn log_stub(dir: &Path) -> PathBuf {
    let log = dir.join("log");
    let body = format!(
        "printf '%s\\036' \"LC_ALL=${{LC_ALL-unset}} $*\" >> '{}'\ncase \"$*\" in *attach-session*) ;; *) echo 1;; esac",
        log.display()
    );
    stub(dir, "tmux", &body);
    dir.join("tmux")
}

/// The records of the log stub.
#[cfg(unix)]
fn log_lines(dir: &Path) -> Vec<String> {
    let s = read_to_string(dir.join("log")).unwrap();
    s.split('\u{1e}').filter(|l| !l.is_empty()).map(str::to_string).collect()
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
    // Round 2: B1 adds rename-window after new-window; M2 adds `--` before positional arguments; M4 adds has-session
    // before socket_path's display-message.
    let list = format!("list-panes -s -t =zygo-t: -F {PANE_FORMAT}");
    let calls = [
        "-V",
        "has-session -t =zygo-t:",
        "display-message -p -t =zygo-t: -- #{socket_path}",
        "has-session -t =zygo-t:",
        "new-session -d -P -F #{window_index} -s zygo-t -c /w/d##x -x 173 -y 47 -- sleep 600",
        "resize-window -t =zygo-t:0 -x 173 -y 47",
        "set-option -w -u -t =zygo-t:0 -- window-size",
        "set-option -w -t =zygo-t:1 -- remain-on-exit off",
        "set-environment -t =zygo-t: -- ZYGO_WORKSPACE 0123456789abcdef",
        "set-environment -r -t =zygo-t: -- ZP_HIDE",
        "set-option -t =zygo-t: -- @zygo-ws 0123456789abcdef",
        "show-options -qv -t =zygo-t: -- @zygo-ws",
        "set-hook -t =zygo-t: -- after-new-window run-shell -b \"true\"",
        "set-hook -w -t =zygo-t:3 -- window-unlinked run-shell -b \"true\"",
        "new-window -d -P -F #{window_index} -t =zygo-t: -c /w/d##x -- sleep 600",
        "rename-window -t =zygo-t:1 -- a##b",
        "split-window -d -P -F #{pane_id} -t %1 -c /w/d##x -- sleep 600",
        "select-layout -t =zygo-t:1 -- tiled",
        "select-window -t =zygo-t:1",
        "select-pane -t %1",
        "resize-pane -Z -t %1",
        list.as_str(),
        "show-options -p -qv -t %1 -- @zygo-pane",
        "set-option -p -t %1 -- @zygo-pane kabcdefgh",
        "display-message -p -t =zygo-t: -- #{@zygo-ws}",
        "kill-session -t =zygo-t:",
        "switch-client -t =zygo-t:",
    ];
    let caller_lc_all = std::env::var("LC_ALL").unwrap_or_else(|_| "unset".into());
    let want: Vec<String> = calls
        .iter()
        .enumerate()
        .map(|(i, a)| format!("{} -S /tmp/zygo-mux-x/s -u {a}", if i == 4 { "LC_ALL=unset" } else { "LC_ALL=C" }))
        .chain([format!("LC_ALL={caller_lc_all} -S /tmp/zygo-mux-x/s attach-session -t =zygo-t:")])
        .collect();
    assert_eq!(log_lines(dir.path()), want);
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
    let lines = log_lines(dir.path());
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

    // A process of the group holds stdout after the exit: the group is killed (round 2 M6: the process is gone).
    let bg = dir.path().join("bg");
    let t = at("held", &format!("sleep 30 & echo $! > '{}'; exit 0", bg.display()));
    let t0 = Instant::now();
    assert_eq!(t.set_env("zygo-t", "A", "1"), Ok(()));
    assert!(t0.elapsed() < Duration::from_secs(4), "{:?}", t0.elapsed());
    assert!(gone_within(pid_in(&bg), Duration::from_secs(3)), "the background sleep still runs");

    // Round 2 M6: a process outside the group (setsid) holds stdout: the call fails within the bound.
    let out = dir.path().join("out");
    let t = at("outside", &format!("setsid sleep 30 & echo $! > '{}'; exit 0", out.display()));
    let t0 = Instant::now();
    let r = t.set_env("zygo-t", "A", "1");
    let took = t0.elapsed();
    let pid = pid_in(&out);
    unsafe { libc::kill(pid, libc::SIGKILL) };
    assert_eq!(r, Err(MuxError::Failed { message: "tmux exited, but a process outside its process group holds its output".into() }));
    assert!(took < Duration::from_secs(3), "{took:?}");

    let t = Tmux::new(PathBuf::from("/nonexistent/tmux"), None);
    assert!(matches!(t.session_exists("zygo-t"), SessionAnswer::Error { .. }));
    assert!(matches!(t.version(), Err(MuxError::Spawn { .. })));
}

/// The pid that a stub wrote to `file`; waits at most 2 s for the file.
#[cfg(unix)]
fn pid_in(file: &Path) -> i32 {
    let t0 = Instant::now();
    loop {
        if let Ok(s) = read_to_string(file)
            && let Ok(p) = s.trim().parse()
        {
            return p;
        }
        assert!(t0.elapsed() < Duration::from_secs(2), "no pid in {}", file.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether `pid` is gone (`kill(pid, 0)` fails) within `limit`.
#[cfg(unix)]
fn gone_within(pid: i32, limit: Duration) -> bool {
    let t0 = Instant::now();
    while unsafe { libc::kill(pid, 0) } == 0 {
        if t0.elapsed() > limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

#[cfg(unix)]
#[test]
fn missing_window_in_a_gone_session_is_gone() {
    // Round 2 M4: NoTarget, then has-session says the session is gone: the answer is Gone.
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    stub(
        dir.path(),
        "tmux",
        "case \"$*\" in *has-session*) echo \"can't find session: zygo-t\" >&2;; *) echo 'no such window: =zygo-t:0' >&2;; esac; exit 1",
    );
    let t = Tmux::new(dir.path().join("tmux"), None);
    let gone = Err(MuxError::Gone { message: "can't find session: zygo-t".into() });
    assert_eq!(t.set_window_option("=zygo-t:0", "remain-on-exit", "off"), gone);
    assert_eq!(t.unset_window_option("zygo-t", 0, "window-size"), gone);
    assert_eq!(t.resize_window("zygo-t", 0, 10, 10), gone);
    assert_eq!(t.select_layout("zygo-t", 0, "tiled"), gone);
    assert_eq!(t.select_window("zygo-t", 0), gone);
    assert_eq!(t.set_hook("zygo-t", HookScope::Window(0), "window-unlinked", "run-shell -b true"), gone);
    let argv = os(&["sleep", "600"]);
    assert_eq!(t.split_window(&Split { target: "=zygo-t:0", start_dir: None, argv: &argv }).map(|_| ()), gone);
    assert_eq!(t.socket_path("zygo-t").map(|_| ()), gone);
    // A pane id names no session: the answer stays NoTarget.
    assert_eq!(t.select_pane("%9"), Err(MuxError::NoTarget { message: "no such window: =zygo-t:0".into() }));
}

#[cfg(unix)]
#[test]
fn bad_option_environment_and_session_names_are_refused_before_any_spawn() {
    // Round 2 M2 and M3
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let t = Tmux::new(log_stub(dir.path()), None);
    for n in ["-g", "-gu", "", "a b", "a=b", "a;b", "a#b", "a\nb", "é"] {
        let bad = Err(MuxError::Failed { message: format!("invalid tmux option or environment name: {n:?}") });
        assert_eq!(t.set_session_option("zygo-t", n, "1"), bad, "{n:?}");
        assert_eq!(t.show_session_option("zygo-t", n), bad.clone().map(|()| None));
        assert_eq!(t.set_window_option("=zygo-t:0", n, "1"), bad);
        assert_eq!(t.unset_window_option("zygo-t", 0, n), bad);
        assert_eq!(t.set_pane_option("%1", n, "1"), bad);
        assert_eq!(t.show_pane_option("%1", n), bad.clone().map(|()| None));
        assert_eq!(t.set_env("zygo-t", n, "1"), bad);
        assert_eq!(t.hide_env("zygo-t", n), bad);
        assert_eq!(t.set_hook("zygo-t", HookScope::Session, n, "run-shell -b true"), bad);
    }
    let argv = os(&["sleep", "600"]);
    for s in ["a#{pid}b", "a:b", "a.b", "zygo-a:b", "zygo-a.b", "zygo-A", "zygo-", "zygo-a#{pid}b", "keep", "-t"] {
        let bad = format!("invalid tmux session name: {s:?}");
        assert_eq!(t.session_exists(s), SessionAnswer::Error { message: bad.clone() });
        assert_eq!(
            t.new_session(&NewSession { name: s, start_dir: Path::new("/w"), width: 80, height: 24, env: &[], argv: &argv }),
            Err(MuxError::Failed { message: bad.clone() })
        );
        assert_eq!(t.set_env(s, "A", "1"), Err(MuxError::Failed { message: bad.clone() }));
        assert_eq!(t.kill_session(s), Err(MuxError::Failed { message: bad.clone() }));
        assert_eq!(t.list_panes(s), Err(MuxError::Failed { message: bad.clone() }));
        assert_eq!(t.attach(s), Err(MuxError::Failed { message: bad.clone() }));
    }
    assert!(!dir.path().join("log").exists());
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
fn control_characters_in_window_names_are_refused_before_any_spawn() {
    // Round 2 B1
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let t = Tmux::new(log_stub(dir.path()), None);
    let argv = os(&["sleep", "600"]);
    for name in ["a\nb", "a\tb", "\u{1b}[31m", "del\u{7f}", "nul\u{0}"] {
        assert_eq!(
            t.new_window(&NewWindow { session: "zygo-t", name: Some(name), start_dir: Path::new("/w"), argv: &argv }),
            Err(MuxError::Failed { message: "window name holds a control character".into() }),
            "{name:?}"
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

#[cfg(unix)]
#[test]
fn gate_refuses_a_missing_program() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("no-tmux");
    assert!(tmux_gate(&p).unwrap_err().starts_with(&format!("{}: cannot run: ", p.display())));
    assert_eq!(require_tmux_at(&p), None);
}

#[cfg(unix)]
#[test]
fn gate_refuses_tmux_3_1() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    stub(dir.path(), "tmux", "echo 'tmux 3.1'");
    let p = dir.path().join("tmux");
    assert_eq!(tmux_gate(&p), Err(format!("{} is tmux 3.1; the minimum is 3.2", p.display())));
    assert_eq!(require_tmux_at(&p), None);
}

#[cfg(unix)]
#[test]
fn gate_refuses_an_unknown_version() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    stub(dir.path(), "tmux", "echo 'tmux master'");
    let p = dir.path().join("tmux");
    assert_eq!(tmux_gate(&p), Err(format!("{}: unknown tmux version: tmux master", p.display())));
}

#[cfg(unix)]
#[test]
fn gate_accepts_tmux_3_2a() {
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    stub(dir.path(), "tmux", "echo 'tmux 3.2a'");
    let p = dir.path().join("tmux");
    assert_eq!(tmux_gate(&p), Ok(p.clone()));
    assert_eq!(require_tmux_at(&p), Some(p));
    assert_eq!(SKIP_PREFIX, "tmux tests skipped: ");
}

// Live tests: each one holds `forks()`, runs only when tmux 3.2 or later is on PATH, and uses its own private server.

/// The environment of the test sessions: the process `PATH` and `HOME` only.
#[cfg(unix)]
fn env() -> Vec<(OsString, OsString)> {
    ["PATH", "HOME"].iter().filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v))).collect()
}

#[cfg(unix)]
fn argv() -> Vec<OsString> { os(&["sleep", "600"]) }

#[cfg(unix)]
fn sorted<S: Into<String>>(v: Vec<S>) -> Vec<String> {
    let mut v: Vec<String> = v.into_iter().map(Into::into).collect();
    v.sort();
    v
}

#[cfg(unix)]
fn session(t: &Tmux, dir: &Path, w: u32, h: u32) -> u32 {
    t.new_session(&NewSession { name: "zygo-t", start_dir: dir, width: w, height: h, env: &env(), argv: &argv() }).unwrap()
}

/// Window index and name, one entry per window of `zygo-t`.
#[cfg(unix)]
fn names(t: &Tmux) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = Vec::new();
    for l in t.list_panes("zygo-t").unwrap() {
        let (w, _) = parse_pane_line(&l).unwrap();
        if !out.iter().any(|(i, _)| *i == w.index) {
            out.push((w.index, w.name));
        }
    }
    out
}

#[cfg(unix)]
#[test]
fn private_server_uses_its_own_socket() {
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    assert!(srv.socket().as_os_str().len() <= 100);
    assert_eq!(fs::metadata(srv.socket().parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Gone); // no socket file
    assert_eq!(session(&t, dir.path(), 100, 30), 0);
    let real = fs::canonicalize(srv.socket()).unwrap();
    assert_eq!(fs::canonicalize(t.socket_path("zygo-t").unwrap()).unwrap(), real);
    assert_eq!(fs::canonicalize(Tmux::new(srv.wrapper().into(), None).socket_path("zygo-t").unwrap()).unwrap(), real); // M6
    assert_eq!(srv.run(&["show-options", "-g", "-v", "default-size"]).unwrap(), "80x24"); // -f /dev/null
}

#[cfg(unix)]
#[test]
fn session_answers_follow_au87() {
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    srv.run(&["new-session", "-d", "-s", "zygo-keep", "--", "sleep", "600"]).unwrap();
    assert_eq!(t.session_exists("zygo-keep"), SessionAnswer::Exists);
    assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Gone); // can't find session
    assert_eq!(t.set_session_option("zygo-t", "@x", "1"), Err(MuxError::Gone { message: "no such session: =zygo-t:".into() }));
    let pid: i32 = srv.run(&["display-message", "-p", "-t", "=zygo-keep:", "#{pid}"]).unwrap().parse().unwrap();
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let t0 = Instant::now();
    while unsafe { libc::kill(pid, 0) } == 0 && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "server {pid} still runs");
    assert!(srv.socket().exists());
    assert_eq!(t.session_exists("zygo-keep"), SessionAnswer::Gone); // no server running on (6.09)
    if unsafe { libc::geteuid() } != 0 {
        // root ignores folder modes
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let a = Tmux::new(srv.program().into(), Some(locked.join("s"))).session_exists("zygo-keep");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            matches!(&a, SessionAnswer::Error { message } if message.starts_with("error connecting to ") && message.ends_with("(Permission denied)")),
            "{a:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn killing_the_last_session_gives_gone() {
    // B2
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    for _ in 0..10 {
        session(&t, dir.path(), 80, 24);
        t.kill_session("zygo-t").unwrap();
        assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Gone);
    }
}

#[cfg(unix)]
#[test]
fn first_window_resize_and_window_size_unset() {
    // AU96, PW6 6.11
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let w = session(&t, dir.path(), 173, 47);
    let wt = window_target("zygo-t", w);
    let global = srv.run(&["show-options", "-g", "-w", "-v", "window-size"]).unwrap();
    t.resize_window("zygo-t", w, 173, 47).unwrap();
    assert_eq!(t.display(&wt, "#{window_width} #{window_height}").unwrap(), "173 47");
    assert_eq!(srv.run(&["show-options", "-w", "-qv", "-t", &wt, "window-size"]).unwrap(), "manual");
    t.unset_window_option("zygo-t", w, "window-size").unwrap();
    assert_eq!(t.display(&wt, "#{window_width} #{window_height}").unwrap(), "173 47");
    assert_eq!(srv.run(&["show-options", "-w", "-qv", "-t", &wt, "window-size"]).unwrap(), "");
    assert_eq!(srv.run(&["show-options", "-g", "-w", "-v", "window-size"]).unwrap(), global);
}

#[cfg(unix)]
#[test]
fn new_session_returns_the_base_index() {
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    srv.run(&["new-session", "-d", "-s", "zygo-keep", "--", "sleep", "600"]).unwrap();
    srv.run(&["set-option", "-g", "base-index", "1"]).unwrap();
    assert_eq!(session(&t, dir.path(), 100, 30), 1);
}

#[cfg(unix)]
#[test]
fn hash_in_names_and_start_dirs_stays_literal() {
    // AU55, W-C8, 6.26, 6.27
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let d = dir.path().join("d#x");
    fs::create_dir(&d).unwrap();
    let d = fs::canonicalize(&d).unwrap();
    let w0 = session(&t, &d, 120, 40);
    let w1 = t.new_window(&NewWindow { session: "zygo-t", name: Some("w#(touch pwned)"), start_dir: &d, argv: &argv() }).unwrap();
    let p = t.split_window(&Split { target: &window_target("zygo-t", w1), start_dir: Some(&d), argv: &argv() }).unwrap();
    assert_eq!(t.display(&window_target("zygo-t", w1), "#{window_name}").unwrap(), "w#(touch pwned)");
    for target in [window_target("zygo-t", w0), window_target("zygo-t", w1), p.clone()] {
        assert_eq!(PathBuf::from(t.display(&target, "#{pane_current_path}").unwrap()), d);
    }
    for place in [d.as_path(), dir.path(), srv.socket().parent().unwrap()] {
        assert!(!place.join("pwned").exists());
    }
}

#[cfg(unix)]
#[test]
fn window_names_and_paths_round_trip() {
    // B3, B5
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let d = dir.path().join("dé");
    fs::create_dir(&d).unwrap();
    let d = fs::canonicalize(&d).unwrap();
    session(&t, &d, 120, 40);
    // Round 2 B1: names that hold vis escapes (a literal backslash) round-trip on 3.2a and 3.7c.
    for name in [r"x\y", r"a\tb", r"x\\y\101", "a|b c#d", "w#(touch pwned)", "-dash", "u€v"] {
        let w = t.new_window(&NewWindow { session: "zygo-t", name: Some(name), start_dir: &d, argv: &argv() }).unwrap();
        let read = names(&t).into_iter().find(|(i, _)| *i == w).unwrap().1;
        assert_eq!(read, name);
        let w2 = t.new_window(&NewWindow { session: "zygo-t", name: Some(&read), start_dir: &d, argv: &argv() }).unwrap();
        assert_eq!(names(&t).into_iter().find(|(i, _)| *i == w2).unwrap().1, read);
        assert_eq!(t.display(&window_target("zygo-t", w2), "#{automatic-rename}").unwrap(), "0");
    }
    let before = names(&t).len();
    for name in ["c\u{1}d", "a\nb", "a\tb", "a\rb", "del\u{7f}"] {
        assert_eq!(
            t.new_window(&NewWindow { session: "zygo-t", name: Some(name), start_dir: &d, argv: &argv() }),
            Err(MuxError::Failed { message: "window name holds a control character".into() }),
            "{name:?}"
        );
    }
    assert_eq!(names(&t).len(), before);
    let rows: Vec<_> = t.list_panes("zygo-t").unwrap().iter().map(|l| parse_pane_line(l).unwrap()).collect();
    assert!(rows.iter().all(|(_, p)| p.cwd == d), "{rows:?}"); // dé, not d_
}

#[cfg(unix)]
#[test]
fn hooks_go_to_the_session_and_window_unlinked_follows_the_version() {
    // W§7.4, PW6 6.30
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let w = session(&t, dir.path(), 100, 30);
    let cmd = |e: &str| hook_command(Path::new("/usr/bin/true"), "0123456789abcdef", e).unwrap();
    for e in ["after-new-window", "after-split-window", "after-kill-pane", "client-detached"] {
        t.set_hook("zygo-t", HookScope::Session, e, &cmd(e)).unwrap();
    }
    t.set_hook("zygo-t", HookScope::Window(w), "window-unlinked", &cmd("window-unlinked")).unwrap();
    let s = srv.run(&["show-hooks", "-t", "=zygo-t:"]).unwrap();
    let wl = srv.run(&["show-hooks", "-w", "-t", &window_target("zygo-t", w)]).unwrap();
    let ev = |l: &str| -> Vec<String> { l.lines().map(|x| x.split('[').next().unwrap().to_string()).collect() };
    let new_scope = t.version().unwrap() >= (3, 3);
    let mut want = vec!["after-new-window", "after-split-window", "after-kill-pane", "client-detached"];
    if new_scope {
        want.push("window-unlinked")
    }
    assert_eq!(sorted(ev(&s)), sorted(want), "{s}");
    assert_eq!(ev(&wl), if new_scope { Vec::<String>::new() } else { vec!["window-unlinked".to_string()] }, "{wl}");
    for l in s.lines().chain(wl.lines()) {
        let e = l.split('[').next().unwrap();
        assert!(
            l.starts_with(&format!("{e}[0] run-shell -b ")) && l.contains("/usr/bin/true") && l.contains(&format!("--event {e}")),
            "{l}"
        );
    }
}

#[cfg(unix)]
#[test]
fn global_options_and_hooks_stay_unchanged() {
    // W-C3
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    srv.run(&["new-session", "-d", "-s", "zygo-keep", "--", "sleep", "600"]).unwrap();
    let g = || {
        ["show-hooks -g", "show-options -g", "show-options -g -w", "show-options -s"]
            .map(|a| srv.run(&a.split(' ').collect::<Vec<_>>()).unwrap())
            .join("\n")
    };
    let g0 = g();
    let w = session(&t, dir.path(), 120, 40);
    let wt = window_target("zygo-t", w);
    t.resize_window("zygo-t", w, 120, 40).unwrap();
    t.set_session_option("zygo-t", "destroy-unattached", "off").unwrap();
    t.set_session_option("zygo-t", "@zygo-ws", "0123456789abcdef").unwrap();
    t.set_window_option(&wt, "remain-on-exit", "off").unwrap();
    t.set_env("zygo-t", "ZYGO_WORKSPACE", "0123456789abcdef").unwrap();
    t.hide_env("zygo-t", "ZP_HIDE").unwrap();
    let cmd = |e: &str| hook_command(Path::new("/usr/bin/true"), "0123456789abcdef", e).unwrap();
    for e in ["after-new-window", "after-split-window", "after-kill-pane", "client-detached"] {
        t.set_hook("zygo-t", HookScope::Session, e, &cmd(e)).unwrap();
    }
    t.set_hook("zygo-t", HookScope::Window(w), "window-unlinked", &cmd("window-unlinked")).unwrap();
    let w1 = t.new_window(&NewWindow { session: "zygo-t", name: Some("w1"), start_dir: dir.path(), argv: &argv() }).unwrap();
    let p = t.split_window(&Split { target: &wt, start_dir: Some(dir.path()), argv: &argv() }).unwrap();
    t.select_layout("zygo-t", w, "tiled").unwrap();
    t.set_pane_option(&p, "@zygo-pane", "kabcdefgh").unwrap();
    t.select_window("zygo-t", w1).unwrap();
    t.select_pane(&p).unwrap();
    t.zoom(&p).unwrap();
    t.unset_window_option("zygo-t", w, "window-size").unwrap();
    assert_eq!(g(), g0);
}

#[cfg(unix)]
#[test]
fn options_environment_and_panes() {
    // AU64 hide, AU68, B1, B4, 6.17-6.25, 6.28
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let w0 = session(&t, dir.path(), 120, 40);
    let p = t.split_window(&Split { target: &window_target("zygo-t", w0), start_dir: Some(dir.path()), argv: &argv() }).unwrap();
    let w1 = t.new_window(&NewWindow { session: "zygo-t", name: Some("w1"), start_dir: dir.path(), argv: &argv() }).unwrap();
    assert_eq!(t.show_session_option("zygo-t", "@zygo-ws").unwrap(), None); // B1: unset is not an error
    t.set_session_option("zygo-t", "@zygo-ws", "0123456789abcdef").unwrap();
    assert_eq!(t.show_session_option("zygo-t", "@zygo-ws").unwrap(), Some("0123456789abcdef".into()));
    t.set_session_option("zygo-t", "destroy-unattached", "off").unwrap();
    assert_eq!(srv.run(&["show-options", "-qv", "-t", "=zygo-t:", "destroy-unattached"]).unwrap(), "off");
    t.set_window_option(&window_target("zygo-t", w1), "remain-on-exit", "off").unwrap(); // B4
    assert_eq!(srv.run(&["show-options", "-w", "-qv", "-t", &window_target("zygo-t", w1), "remain-on-exit"]).unwrap(), "off");
    assert_eq!(srv.run(&["show-options", "-w", "-qv", "-t", &window_target("zygo-t", w0), "remain-on-exit"]).unwrap(), "");
    t.set_env("zygo-t", "ZYGO_WORKSPACE", "0123456789abcdef").unwrap();
    t.hide_env("zygo-t", "ZP_HIDE").unwrap();
    let e = srv.run(&["show-environment", "-t", "=zygo-t:"]).unwrap();
    assert!(e.lines().any(|l| l == "ZYGO_WORKSPACE=0123456789abcdef") && e.lines().any(|l| l == "-ZP_HIDE"), "{e}");
    assert_eq!(t.show_pane_option(&p, "@zygo-pane").unwrap(), None); // B1
    t.set_pane_option(&p, "@zygo-pane", "kabcdefgh").unwrap();
    assert_eq!(t.show_pane_option(&p, "@zygo-pane").unwrap(), Some("kabcdefgh".into()));
    assert_eq!(t.show_pane_option(&window_target("zygo-t", w1), "@zygo-pane").unwrap(), None);
    let rows: Vec<_> = t.list_panes("zygo-t").unwrap().iter().map(|l| parse_pane_line(l).unwrap()).collect();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().any(|(_, r)| r.id == p && r.key == "kabcdefgh"));
    let real = fs::canonicalize(dir.path()).unwrap();
    assert!(rows.iter().all(|(_, r)| r.id.starts_with('%') && r.cwd == real), "{rows:?}");
    t.select_window("zygo-t", w0).unwrap();
    t.select_pane(&p).unwrap();
    t.zoom(&p).unwrap();
    assert_eq!(t.display(&p, "#{window_active} #{pane_active} #{window_zoomed_flag}").unwrap(), "1 1 1");
    let layout = t.display(&window_target("zygo-t", w0), "#{window_layout}").unwrap();
    t.zoom(&p).unwrap();
    t.select_layout("zygo-t", w0, "even-horizontal").unwrap();
    t.select_layout("zygo-t", w0, &layout).unwrap();
    assert_eq!(t.display(&window_target("zygo-t", w0), "#{window_layout}").unwrap(), layout);
    t.resize_window("zygo-t", w1, 20, 6).unwrap();
    let err = (0..20)
        .find_map(|_| t.split_window(&Split { target: &window_target("zygo-t", w1), start_dir: None, argv: &argv() }).err())
        .unwrap();
    assert!(err.is_no_space(), "{err}");
    t.kill_session("zygo-t").unwrap();
    assert_eq!(t.session_exists("zygo-t"), SessionAnswer::Gone);
}

#[cfg(unix)]
#[test]
fn missing_targets() {
    // M3, M4
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let w0 = session(&t, dir.path(), 100, 30);
    assert_eq!(t.resize_window("zygo-t", 9, 10, 10), Err(MuxError::NoTarget { message: "can't find window: 9".into() }));
    assert_eq!(
        t.set_window_option("=zygo-t:9", "remain-on-exit", "off"),
        Err(MuxError::NoTarget { message: "no such window: =zygo-t:9".into() })
    );
    assert_eq!(t.select_pane("%99"), Err(MuxError::NoTarget { message: "can't find pane: %99".into() }));
    assert_eq!(t.set_pane_option("%99", "@zygo-pane", "k"), Err(MuxError::NoTarget { message: "no such pane: %99".into() }));
    assert!(t.list_panes("zygo-nosuch").unwrap_err().is_gone());
    assert_eq!(t.display("=zygo-t:9", "#{window_index}").unwrap(), w0.to_string()); // current window's value, rc 0
    assert_eq!(t.display("%99", "#{pane_id}").unwrap(), ""); // empty, rc 0
    assert_eq!(t.show_pane_option("%99", "@zygo-pane").unwrap(), None);
}

#[cfg(unix)]
#[test]
fn operations_on_a_gone_session_give_gone() {
    // Round 2 M4: another session keeps the server alive, so tmux answers for the missing session itself.
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    srv.run(&["new-session", "-d", "-s", "zygo-keep", "--", "sleep", "600"]).unwrap();
    session(&t, dir.path(), 100, 30);
    t.kill_session("zygo-t").unwrap();
    let argv = argv();
    let results = [
        t.socket_path("zygo-t").map(|_| ()),
        t.resize_window("zygo-t", 0, 10, 10),
        t.unset_window_option("zygo-t", 0, "window-size"),
        t.set_window_option("=zygo-t:0", "remain-on-exit", "off"),
        t.set_env("zygo-t", "A", "1"),
        t.hide_env("zygo-t", "A"),
        t.set_session_option("zygo-t", "@x", "1"),
        t.set_hook("zygo-t", HookScope::Session, "after-new-window", "run-shell -b true"),
        t.set_hook("zygo-t", HookScope::Window(0), "window-unlinked", "run-shell -b true"),
        t.new_window(&NewWindow { session: "zygo-t", name: Some("w"), start_dir: dir.path(), argv: &argv }).map(|_| ()),
        t.split_window(&Split { target: "=zygo-t:0", start_dir: None, argv: &argv }).map(|_| ()),
        t.select_layout("zygo-t", 0, "tiled"),
        t.select_window("zygo-t", 0),
        t.list_panes("zygo-t").map(|_| ()),
        t.kill_session("zygo-t"),
    ];
    for (i, r) in results.iter().enumerate() {
        assert!(matches!(r, Err(MuxError::Gone { .. })), "{i}: {r:?}");
    }
    assert_eq!(t.session_exists("zygo-keep"), SessionAnswer::Exists);
}

#[cfg(unix)]
#[test]
fn bad_names_change_no_global_option_or_environment() {
    // Round 2 M2: `-g` and `-gu` as names are refused; the global `status` and `HOME` stay.
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    session(&t, dir.path(), 100, 30);
    let status = srv.run(&["show-options", "-g", "-v", "status"]).unwrap();
    let home = srv.run(&["show-environment", "-g", "HOME"]).unwrap();
    assert!(t.set_session_option("zygo-t", "-g", "status").is_err());
    assert!(t.set_session_option("zygo-t", "-gu", "status").is_err());
    assert!(t.set_window_option("=zygo-t:0", "-g", "status").is_err());
    assert!(t.unset_window_option("zygo-t", 0, "-gu").is_err());
    assert!(t.set_env("zygo-t", "-gu", "HOME").is_err());
    assert!(t.set_env("zygo-t", "-g", "HOME").is_err());
    assert!(t.hide_env("zygo-t", "-gu").is_err());
    assert_eq!(srv.run(&["show-options", "-g", "-v", "status"]).unwrap(), status);
    assert_eq!(srv.run(&["show-environment", "-g", "HOME"]).unwrap(), home);
    // A value that starts with `-` is a value, after `--`.
    t.set_session_option("zygo-t", "@x", "-g").unwrap();
    assert_eq!(t.show_session_option("zygo-t", "@x").unwrap(), Some("-g".into()));
    t.set_env("zygo-t", "ZA", "-u").unwrap();
    assert_eq!(srv.run(&["show-environment", "-t", "=zygo-t:", "ZA"]).unwrap(), "ZA=-u");
}

#[cfg(unix)]
#[test]
fn pane_paths_with_newline_and_cr_round_trip() {
    // Round 2 M1
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let srv = PrivateServer::start();
    let t = srv.tmux();
    let mk = |n: &str| {
        let d = dir.path().join(n);
        fs::create_dir(&d).unwrap();
        fs::canonicalize(&d).unwrap()
    };
    let (nl, cr, bs) = (mk("nl\nx"), mk("cr\r"), mk("b\\n"));
    let w = session(&t, &nl, 120, 40);
    let p_cr = t.split_window(&Split { target: &window_target("zygo-t", w), start_dir: Some(&cr), argv: &argv() }).unwrap();
    let p_bs = t.split_window(&Split { target: &window_target("zygo-t", w), start_dir: Some(&bs), argv: &argv() }).unwrap();
    let lines = t.list_panes("zygo-t").unwrap();
    assert_eq!(lines.len(), 3, "{lines:?}");
    let mut cwds: Vec<PathBuf> = lines.iter().map(|l| parse_pane_line(l).unwrap().1.cwd).collect();
    cwds.sort();
    let mut want = vec![nl.clone(), cr.clone(), bs.clone()];
    want.sort();
    assert_eq!(cwds, want);
    assert_eq!(PathBuf::from(t.display(&p_cr, "#{pane_current_path}").unwrap()), cr);
    assert_eq!(PathBuf::from(t.display(&p_bs, "#{pane_current_path}").unwrap()), bs);
    assert_eq!(PathBuf::from(t.display(&window_target("zygo-t", w), "#{pane_current_path}").unwrap()), nl);
}

#[cfg(unix)]
#[test]
fn au54_hook_runs_with_a_space_and_a_hash_in_the_zygo_path() {
    // Round 2 M6
    let _g = forks();
    let Some(_) = require_tmux() else { return };
    let dir = tempfile::tempdir().unwrap();
    let zdir = dir.path().join("z #dir");
    fs::create_dir(&zdir).unwrap();
    let log = dir.path().join("hooklog");
    stub(&zdir, "zygo", &format!("printf '%s\\n' \"$*\" >> '{}'", log.display()));
    let srv = PrivateServer::start();
    let t = srv.tmux();
    session(&t, dir.path(), 100, 30);
    let cmd = hook_command(&zdir.join("zygo"), "0123456789abcdef", "after-new-window").unwrap();
    t.set_hook("zygo-t", HookScope::Session, "after-new-window", &cmd).unwrap();
    t.new_window(&NewWindow { session: "zygo-t", name: None, start_dir: dir.path(), argv: &argv() }).unwrap();
    let t0 = Instant::now();
    while !log.exists() && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(read_to_string(&log).unwrap(), "ws snapshot --ws-id 0123456789abcdef --event after-new-window\n");
}

/// Runs the ignored test `name` of this binary in a child process with `env` set on that child only.
#[cfg(unix)]
fn run_child(name: &str, env: &[(&str, &std::ffi::OsStr)]) {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--ignored", "--test-threads=1", "--nocapture"])
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let (so, se) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && so.contains("1 passed"), "{name}\nstdout:\n{so}\nstderr:\n{se}");
}

#[cfg(unix)]
#[test]
fn held_pipe_is_killed_under_sigchld_ignored() {
    // Round 2 M5: the kernel reaps tmux, and the group is still killed; no reader thread stays.
    let _g = forks();
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg");
    stub(dir.path(), "tmux", &format!("sleep 30 & echo $! > '{}'; exit 0", bg.display()));
    let mark = dir.path().join("mark");
    run_child(
        "held_pipe_sigchld_child",
        &[("W3_HELD_TMUX", dir.path().join("tmux").as_os_str()), ("W3_HELD_BG", bg.as_os_str()), ("W3_HELD_MARK", mark.as_os_str())],
    );
    assert_eq!(read_to_string(&mark).unwrap(), "ok"); // the child ran the assertions
}

/// The number of threads of this process.
#[cfg(target_os = "linux")]
fn threads() -> usize { fs::read_dir("/proc/self/task").unwrap().count() }

/// Run only by `held_pipe_is_killed_under_sigchld_ignored`, in a child process; passes when run alone.
#[cfg(unix)]
#[test]
#[ignore = "run by held_pipe_is_killed_under_sigchld_ignored"]
fn held_pipe_sigchld_child() {
    let (Some(tmux), Some(bg), Some(mark)) =
        (std::env::var_os("W3_HELD_TMUX"), std::env::var_os("W3_HELD_BG"), std::env::var_os("W3_HELD_MARK"))
    else {
        return;
    };
    // SAFETY: sets this process's SIGCHLD disposition; the child process runs this one test alone.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
    #[cfg(target_os = "linux")]
    let before = threads();
    let t = Tmux::new(PathBuf::from(tmux), None);
    let t0 = Instant::now();
    assert_eq!(t.set_env("zygo-t", "A", "1"), Ok(()));
    assert!(t0.elapsed() < Duration::from_secs(4), "{:?}", t0.elapsed());
    assert!(gone_within(pid_in(Path::new(&bg)), Duration::from_secs(3)), "the background sleep still runs");
    #[cfg(target_os = "linux")]
    {
        let t1 = Instant::now();
        while threads() > before && t1.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(threads(), before, "a reader thread stays");
    }
    fs::write(mark, "ok").unwrap();
}
