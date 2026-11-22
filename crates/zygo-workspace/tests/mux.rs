//! Node W3: the multiplexer trait, the tmux implementation and the tmux test gate (W§13, W§19).
use std::path::Path;
use std::sync::Mutex;

use zygo_workspace::mux::{check_path, format_escape, hook_command, UnsafePath, UNSAFE_PATH_MESSAGE};

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
