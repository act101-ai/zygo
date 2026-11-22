//! Multiplexer trait and the tmux test gate. Owner: node W3 (W§13).

pub mod tmux;

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The oldest tmux that workspaces support (AU7).
pub const MIN_VERSION: (u32, u32) = (3, 2);

/// The bound of every tmux call except `attach` (W§13).
pub const BOUND: Duration = Duration::from_secs(2);

/// The answer to "does this tmux session exist" (AU87 (1)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAnswer {
    Exists,
    /// tmux said explicitly that the session or its server is gone ([`is_gone_message`]).
    Gone,
    /// Any other tmux error: a timeout, a permission error, a spawn error or another message.
    Error { message: String },
}

/// The error of a tmux call (W3 D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxError {
    /// The session or its server is gone ([`is_gone_message`]).
    Gone { message: String },
    /// The window or pane does not exist ([`is_no_target_message`]); the session can still exist.
    NoTarget { message: String },
    /// Any other failure, with tmux's stderr (trimmed) or the refusal text.
    Failed { message: String },
    /// The call did not end within [`BOUND`].
    TimedOut,
    /// tmux could not be started.
    Spawn { message: String },
}

impl MuxError {
    pub fn is_gone(&self) -> bool { matches!(self, MuxError::Gone { .. }) }

    /// A `Failed` error whose message holds `no space for` (a split that does not fit).
    pub fn is_no_space(&self) -> bool { matches!(self, MuxError::Failed { message } if message.contains("no space for")) }
}

impl std::fmt::Display for MuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MuxError::Gone { message }
            | MuxError::NoTarget { message }
            | MuxError::Failed { message }
            | MuxError::Spawn { message } => f.write_str(message),
            MuxError::TimedOut => write!(f, "tmux timed out after {} s", BOUND.as_secs()),
        }
    }
}

impl std::error::Error for MuxError {}

/// Whether the first line of tmux's stderr says that the session or its server is gone (AU87 (1), W3 D2).
pub fn is_gone_message(stderr: &str) -> bool {
    let l = stderr.lines().next().unwrap_or("").trim_end();
    l.starts_with("can't find session")
        || l.starts_with("no such session:")
        || l.starts_with("no server running on ")
        || l == "server exited unexpectedly"
        || (l.starts_with("error connecting to ")
            && (l.ends_with(" (No such file or directory)") || l.ends_with(" (Connection refused)")))
}

/// Whether the first line of tmux's stderr says that a window or pane does not exist (W3 D1).
pub fn is_no_target_message(stderr: &str) -> bool {
    let l = stderr.lines().next().unwrap_or("");
    ["can't find window:", "no such window:", "can't find pane:", "no such pane:"].iter().any(|p| l.starts_with(p))
}

/// The first `<digits>.<digits>` after `tmux ` in `tmux -V` output: `tmux 3.2a` gives (3, 2), `tmux next-3.6` gives
/// (3, 6), and `tmux master` gives none (W3 D13).
pub fn parse_version(out: &str) -> Option<(u32, u32)> {
    let rest = out.lines().next()?.strip_prefix("tmux ")?;
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let major_end = i + b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if b.get(major_end) == Some(&b'.') {
            let minor_len = b[major_end + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if minor_len > 0 {
                let major = rest[i..major_end].parse().ok()?;
                let minor = rest[major_end + 1..major_end + 1 + minor_len].parse().ok()?;
                return Some((major, minor));
            }
        }
        i = major_end;
    }
    None
}

/// The `list-panes -F` format (W§19, AU99): 13 fields split by `|`. The window name and the pane key are read through
/// `q:`, so a `|` in them arrives as `\|`. The pane path is last and raw: it takes the rest of the line.
pub const PANE_FORMAT: &str = "#{window_index}|#{q:window_name}|#{automatic-rename}|#{window_layout}|#{window_width}|#{window_height}|#{window_active}|#{window_zoomed_flag}|#{pane_id}|#{pane_index}|#{pane_active}|#{q:@zygo-pane}|#{pane_current_path}";

/// The number of fields of [`PANE_FORMAT`].
pub const PANE_FIELDS: usize = 13;

/// One window, from a line of [`PANE_FORMAT`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRow {
    pub index: u32,
    /// The name as the user typed it (`q:` escapes removed, then [`vis_decode`]).
    pub name: String,
    pub automatic_rename: bool,
    pub layout: String,
    pub width: u32,
    pub height: u32,
    pub active: bool,
    pub zoomed: bool,
}

/// One pane, from a line of [`PANE_FORMAT`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    /// The pane id, `%N`.
    pub id: String,
    pub index: u32,
    pub active: bool,
    /// The `@zygo-pane` value (`q:` escapes removed); empty when unset.
    pub key: String,
    pub cwd: PathBuf,
}

/// Why a `list-panes` line does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooFewFields { found: usize },
    BadField { field: &'static str },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::TooFewFields { found } => write!(f, "list-panes line has {found} of {PANE_FIELDS} fields"),
            ParseError::BadField { field } => write!(f, "list-panes field {field} is not valid"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Reverses tmux's vis encoding of a window name (`utf8_stravis` with `VIS_OCTAL|VIS_CSTYLE|VIS_TAB|VIS_NL`): `\\`
/// gives `\`, `\` and three octal digits give that byte, and `\a \b \t \n \v \f \r \0` give their control bytes.
/// Every other character stays unchanged. The bytes are made into text with `from_utf8_lossy`.
pub fn vis_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let octal = b.get(i + 1..i + 4).filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)));
        if let Some(d) = octal {
            let v = d.iter().fold(0u32, |a, c| a * 8 + u32::from(c - b'0'));
            if let Ok(byte) = u8::try_from(v) {
                out.push(byte);
                i += 4;
                continue;
            }
        }
        let control = match b[i + 1] {
            b'\\' => Some(b'\\'),
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b't' => Some(b'\t'),
            b'n' => Some(b'\n'),
            b'v' => Some(0x0b),
            b'f' => Some(0x0c),
            b'r' => Some(b'\r'),
            b'0' => Some(0),
            _ => None,
        };
        match control {
            Some(c) => {
                out.push(c);
                i += 2;
            }
            None => {
                out.push(b'\\');
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Removes the `q:` escapes: `\X` gives `X`.
fn q_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Splits a line into at most [`PANE_FIELDS`] fields. Each of the first 12 ends at the first `|` that no `\` escapes;
/// the last field is the rest of the line, raw.
fn split_fields(line: &str) -> Vec<&str> {
    let b = line.as_bytes();
    let mut fields = Vec::with_capacity(PANE_FIELDS);
    let mut start = 0;
    let mut i = 0;
    while fields.len() < PANE_FIELDS - 1 {
        if i >= b.len() {
            fields.push(&line[start..]);
            return fields;
        }
        match b[i] {
            b'\\' => i += 2,
            b'|' => {
                fields.push(&line[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    fields.push(&line[start..]);
    fields
}

fn number(s: &str, field: &'static str) -> Result<u32, ParseError> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(ParseError::BadField { field });
    }
    s.parse().map_err(|_| ParseError::BadField { field })
}

fn flag(s: &str, field: &'static str) -> Result<bool, ParseError> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(ParseError::BadField { field }),
    }
}

/// Parses one line of `list-panes -F` [`PANE_FORMAT`] (AU99). Fields 1-12 are read from the left, and field 13, the
/// pane path, is the rest of the line. A line with fewer than 13 fields, or a typed field that does not parse, is
/// refused.
pub fn parse_pane_line(line: &str) -> Result<(WindowRow, PaneRow), ParseError> {
    let f = split_fields(line);
    if f.len() < PANE_FIELDS {
        return Err(ParseError::TooFewFields { found: f.len() });
    }
    let window = WindowRow {
        index: number(f[0], "window_index")?,
        name: vis_decode(&q_unescape(f[1])),
        automatic_rename: flag(f[2], "automatic-rename")?,
        layout: f[3].to_string(),
        width: number(f[4], "window_width")?,
        height: number(f[5], "window_height")?,
        active: flag(f[6], "window_active")?,
        zoomed: flag(f[7], "window_zoomed_flag")?,
    };
    if !f[8].starts_with('%') {
        return Err(ParseError::BadField { field: "pane_id" });
    }
    let pane = PaneRow {
        id: f[8].to_string(),
        index: number(f[9], "pane_index")?,
        active: flag(f[10], "pane_active")?,
        key: q_unescape(f[11]),
        cwd: PathBuf::from(f[12]),
    };
    Ok((window, pane))
}

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
