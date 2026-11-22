//! Multiplexer trait and the tmux test gate. Owner: node W3 (W§13).

pub mod tmux;

use std::path::{Path, PathBuf};

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
