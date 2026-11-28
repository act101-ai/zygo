//! Clean step (O§7.4): ANSI escapes and carriage-return progress.
use super::Step;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// An OSC whose terminator does not start within this many body bytes has no terminator (O§7.4).
const OSC_MAX_BODY: usize = 4096;

/// `None`: no change. Otherwise the cleaned text and the steps that changed it,
/// `ColorCodesRemoved` before `ProgressLinesRemoved`. With `reuse` (O§7.3), only escapes go.
pub fn clean(text: &str, reuse: bool) -> Option<(String, Vec<Step>)> {
    let mut steps = Vec::new();
    let mut out = None;
    if let Some(t) = strip_escapes(text) {
        steps.push(Step::ColorCodesRemoved);
        out = Some(t);
    }
    if !reuse && let Some(t) = overwrite_cr(out.as_deref().unwrap_or(text), text) {
        steps.push(Step::ProgressLinesRemoved);
        out = Some(t);
    }
    out.map(|t| (t, steps))
}

/// O§7.4 bullet 1. An escape never takes `\n` or `\r`, so no line goes.
fn strip_escapes(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut kept_from = 0;
    let mut i = 0;
    // The last OSC stop found; reused while it is ahead, so each byte is scanned once.
    let mut osc_stop_cache = None;
    while i < b.len() {
        let cut = match b[i] {
            ESC => Some(esc_len(text, i, &mut osc_stop_cache)),
            // U+009B in UTF-8; with no valid tail it stays.
            0xC2 if b.get(i + 1) == Some(&0x9B) => csi_end(b, i + 2).map(|end| end - i),
            _ => None,
        };
        match cut {
            Some(n) => {
                out.push_str(&text[kept_from..i]);
                i += n;
                kept_from = i;
            }
            None => i += 1,
        }
    }
    if kept_from == 0 {
        return None;
    }
    out.push_str(&text[kept_from..]);
    Some(out)
}

/// The most intermediate bytes an nF escape takes (O§7.4). The longest ECMA-35 designation
/// (`ESC $ ( SP F`) has 3; a longer run of spaces and punctuation is more likely text.
const NF_MAX_INTERMEDIATES: usize = 4;

/// Bytes to remove for the `ESC` at `i`: at least the `ESC`.
/// A lone `ESC` (at the text end, or before `\n`, `\r` or another `ESC`) is 1 byte;
/// the next `ESC` then starts its own sequence.
fn esc_len(text: &str, i: usize, osc_stop_cache: &mut Option<usize>) -> usize {
    let b = text.as_bytes();
    match b.get(i + 1) {
        None | Some(b'\n' | b'\r' | &ESC) => 1,
        // nF escape (ECMA-35), designations included: `ESC`, 1 to 4 intermediate bytes 0x20-0x2F,
        // one final byte 0x30-0x7E. With no final byte only `ESC` goes and the rest stays (O-C9),
        // so a line end, an `ESC`, U+009B or real text is never taken.
        Some(0x20..=0x2F) => {
            // Counted to one past the bound: a longer run is not taken, whatever follows it.
            let run = b[i + 1..]
                .iter()
                .take(NF_MAX_INTERMEDIATES + 1)
                .take_while(|c| (0x20..=0x2F).contains(*c))
                .count();
            match b.get(i + 1 + run) {
                Some(0x30..=0x7E) if run <= NF_MAX_INTERMEDIATES => run + 2,
                _ => 1,
            }
        }
        Some(b'[') => csi_end(b, i + 2).map_or(2, |end| end - i),
        Some(b']') => {
            let body = i + 2;
            let stop = match *osc_stop_cache {
                Some(stop) if stop >= body => stop,
                _ => osc_stop(b, body),
            };
            *osc_stop_cache = Some(stop);
            match b.get(stop) {
                _ if stop - body > OSC_MAX_BODY => 2,
                Some(&BEL) => stop + 1 - i,
                Some(&ESC) => stop + 2 - i,
                _ => 2,
            }
        }
        Some(_) => 1 + text[i + 1..].chars().next().map_or(1, char::len_utf8),
    }
}

/// The index after a CSI tail that starts at `j`: parameter bytes, intermediate bytes, one final byte.
fn csi_end(b: &[u8], mut j: usize) -> Option<usize> {
    while j < b.len() && (0x30..=0x3F).contains(&b[j]) {
        j += 1;
    }
    while j < b.len() && (0x20..=0x2F).contains(&b[j]) {
        j += 1;
    }
    match b.get(j) {
        Some(0x40..=0x7E) => Some(j + 1),
        _ => None,
    }
}

/// The first index at or after `j` that holds `BEL`, `ESC \`, `\n` or `\r`, or the text end.
/// Only `BEL` and `ESC \` end an OSC; an OSC never takes `\n` or `\r`.
fn osc_stop(b: &[u8], j: usize) -> usize {
    let mut k = j;
    while k < b.len() {
        match b[k] {
            BEL | b'\n' | b'\r' => return k,
            ESC if b.get(k + 1) == Some(&b'\\') => return k,
            _ => k += 1,
        }
    }
    k
}

/// O§7.4 bullet 2: `\r\n` becomes `\n`, then each line goes through `overwrite_line`. `raw` is the text before
/// escape removal; escapes never take `\n` (`strip_escapes`), so its lines pair one to one with those of `text`.
fn overwrite_cr(text: &str, raw: &str) -> Option<String> {
    if !text.contains('\r') {
        return None;
    }
    let text = text.replace("\r\n", "\n");
    let mut out = String::with_capacity(text.len());
    for (k, (line, raw_line)) in text.split('\n').zip(raw.split('\n')).enumerate() {
        if k > 0 {
            out.push('\n');
        }
        // An `ESC` is always removed; a U+009B may be. Either way the columns are unsure.
        let escaped = raw_line.contains(['\x1b', '\u{9b}']);
        overwrite_line(line, escaped, &mut out);
    }
    Some(out)
}

/// One column for sure: printable ASCII, and the closed list of characters that progress bars and spinners
/// draw with, which their programs count as one column: box drawing U+2500-U+257F (pip, rich), block elements
/// U+2580-U+259F (tqdm), braille patterns U+2800-U+28FF (spinners).
fn one_column(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{2500}'..='\u{259f}' | '\u{2800}'..='\u{28ff}')
}

/// Appends the kept text of one line. `\r` at the line end overwrites nothing and goes. When the line still
/// holds a `\r`, its last segment is kept alone only when it surely covers every earlier segment: no escape was
/// removed from the raw line, every character is `one_column`, and the last segment is at least as wide as each
/// earlier one. Else the line is unsure: only its `\r` bytes go and all its text stays, in order (O-C9; O§7.3:
/// a step removes, never reorders). One pass over the line, one copy.
fn overwrite_line(line: &str, escaped: bool, out: &mut String) {
    let line = line.trim_end_matches('\r');
    let Some(last_cr) = line.rfind('\r') else {
        out.push_str(line);
        return;
    };
    let mut sure = !escaped;
    let (mut width, mut widest_earlier) = (0usize, 0usize);
    for c in line.chars() {
        if !sure {
            break;
        }
        if c == '\r' {
            widest_earlier = widest_earlier.max(width);
            width = 0;
        } else if one_column(c) {
            width += 1;
        } else {
            sure = false;
        }
    }
    if sure && width >= widest_earlier {
        out.push_str(&line[last_cr + 1..]);
    } else {
        out.extend(line.split('\r'));
    }
}
