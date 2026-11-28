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
    if !reuse && let Some(t) = overwrite_cr(out.as_deref().unwrap_or(text)) {
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

/// O§7.4 bullet 2: `\r\n` becomes `\n`; in each line only the text after the last `\r` stays.
/// A `\r` with nothing after it on the line overwrites nothing.
fn overwrite_cr(text: &str) -> Option<String> {
    if !text.contains('\r') {
        return None;
    }
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| {
            let l = l.trim_end_matches('\r');
            l.rsplit('\r').next().unwrap_or(l)
        })
        .collect();
    Some(lines.join("\n"))
}
