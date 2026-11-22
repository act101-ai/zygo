//! The assumed width of the HUD and the cell count of its text (H§6.7).

use std::ffi::OsString;

use unicode_width::UnicodeWidthChar;

/// The width when no source answers (H§6.7 step 3).
pub const DEFAULT_WIDTH: u16 = 120;
/// A value outside this range, from any source, is skipped (H§6.7).
const RANGE: std::ops::RangeInclusive<u16> = 20..=1000;
/// Claude Code draws the status line this many cells from the left edge of the terminal (P-HUD-1 PH4, Claude Code
/// 2.1.330, 2026-11-22: `fixtures/claude-code/hud/p-hud-1-fit.json` `"indent": 2`; `p-hud-1-compact-19.txt`).
pub const STATUS_INDENT: u16 = 2;
/// Claude Code cuts the status line with `…` this many cells before the right edge of the terminal (P-HUD-1 PH4,
/// 2026-11-22: `p-hud-1-fit.json` `"gap": 2` at 19 columns).
pub const STATUS_RIGHT_GAP: u16 = 2;

/// The terminal width: the terminal (`tty`), else `COLUMNS` (through `env`), else 120. A source that gives a value
/// outside 20 to 1000, or no number, is skipped. The width-source log names this value (P-HUD-1).
pub fn terminal_width(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> u16 {
    measured(env, tty).unwrap_or(DEFAULT_WIDTH)
}

/// The assumed width W of Claude Code's status area (H§6.7): the terminal width less `STATUS_INDENT` and
/// `STATUS_RIGHT_GAP` when a source answers; 120 when none does.
pub fn status_width(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> u16 {
    measured(env, tty).map_or(DEFAULT_WIDTH, |n| n - STATUS_INDENT - STATUS_RIGHT_GAP)
}

/// The first source with a value in 20 to 1000: the terminal, then `COLUMNS`.
fn measured(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> Option<u16> {
    let columns = || env("COLUMNS")?.to_str()?.trim().parse::<u16>().ok();
    let usable = |n: &u16| RANGE.contains(n);
    tty().filter(usable).or_else(|| columns().filter(usable))
}

/// The columns of the controlling terminal: `TIOCGWINSZ` on `/dev/tty`, opened for reading only. `None` when
/// `/dev/tty` cannot be opened or the call fails. Inside tmux this is the size of the pane.
#[cfg(unix)]
pub fn tty_columns() -> Option<u16> {
    use std::os::fd::AsRawFd;

    let tty = std::fs::File::open("/dev/tty").ok()?;
    let mut size = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: `tty` is an open descriptor for the whole call, and `TIOCGWINSZ` writes one `winsize` to the pointer,
    // which is a live, exclusive `winsize`.
    let rc = unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut size) };
    (rc == 0).then_some(size.ws_col)
}

/// Windows has no `/dev/tty`: this source never answers.
#[cfg(not(unix))]
pub fn tty_columns() -> Option<u16> {
    None
}

/// The terminal cells of `s`: Unicode East Asian width, and 2 for each private-use character (a Nerd Font glyph),
/// which is safe for the "Mono" and the proportional font variants. `s` holds no escape sequence.
pub fn cells(s: &str) -> usize {
    s.chars().map(char_cells).sum()
}

fn char_cells(c: char) -> usize {
    match c as u32 {
        0xE000..=0xF8FF | 0xF_0000..=0xF_FFFD | 0x10_0000..=0x10_FFFD => 2,
        _ => c.width().unwrap_or(0),
    }
}

/// `s` when it has at most `max` cells; else its first cells and `…`, `max` cells at most, cut on a character
/// boundary (AU19).
pub fn truncate(s: &str, max: usize) -> String {
    if cells(s) <= max {
        return s.to_string();
    }
    let (mut out, mut used) = (String::new(), 0);
    for c in s.chars() {
        let w = char_cells(c);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}
