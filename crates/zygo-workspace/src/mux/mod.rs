//! Multiplexer trait and the tmux test gate. Owner: node W3 (W§13).

pub mod tmux;

use std::path::Path;

/// The AU54 refusal text for a zygo path or a tmux socket path that a tmux command string cannot quote.
pub const UNSAFE_PATH_MESSAGE: &str =
    "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";

/// A path that holds `'`, `"`, `\`, `$` or a control character, or is not UTF-8 (AU54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsafePath;

impl std::fmt::Display for UnsafePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(UNSAFE_PATH_MESSAGE) }
}

impl std::error::Error for UnsafePath {}

/// The path as text, when it is UTF-8 and holds no `'`, `"`, `\`, `$` and no control character (AU54).
pub fn check_path(path: &Path) -> Result<&str, UnsafePath> {
    let s = path.to_str().ok_or(UnsafePath)?;
    if s.chars().any(|c| matches!(c, '\'' | '"' | '\\' | '$') || c.is_control()) {
        return Err(UnsafePath);
    }
    Ok(s)
}

/// Doubles each `#`, so tmux reads the value as literal text and expands no format in it (AU55, W-C8).
pub fn format_escape(s: &str) -> String { s.replace('#', "##") }

/// The tmux hook command of AU54: `run-shell -b "'<zygo>' ws snapshot --ws-id <id> --event <event>"`, with each `#`
/// of the path doubled. `ws_id` must pass `identity::valid_ws_id`, and `event` is a tmux hook name.
pub fn hook_command(zygo: &Path, ws_id: &str, event: &str) -> Result<String, UnsafePath> {
    debug_assert!(
        crate::identity::valid_ws_id(ws_id)
            && !event.is_empty()
            && event.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    );
    let path = check_path(zygo)?;
    Ok(format!("run-shell -b \"'{}' ws snapshot --ws-id {ws_id} --event {event}\"", format_escape(path)))
}
