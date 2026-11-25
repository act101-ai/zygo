//! Clean step (O§7.4): ANSI escapes and carriage-return progress.
use super::Step;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

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
    // A failed OSC scan found no terminator before this index; do not scan again before it.
    let mut no_osc_end_before = 0;
    while i < b.len() {
        let cut = match b[i] {
            ESC => Some(esc_len(text, i, &mut no_osc_end_before)),
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

/// Bytes to remove for the `ESC` at `i`: at least the `ESC`.
fn esc_len(text: &str, i: usize, no_osc_end_before: &mut usize) -> usize {
    let b = text.as_bytes();
    match b.get(i + 1) {
        None | Some(b'\n' | b'\r') => 1,
        Some(b'[') => csi_end(b, i + 2).map_or(2, |end| end - i),
        Some(b']') if i < *no_osc_end_before => 2,
        Some(b']') => match osc_end(b, i + 2) {
            Ok(end) => end - i,
            Err(line_end) => {
                *no_osc_end_before = line_end;
                2
            }
        },
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

/// `Ok`: the index after the `BEL` or `ESC \` that ends an OSC body starting at `j`.
/// `Err`: the line end (`\n` or text end) reached first.
fn osc_end(b: &[u8], j: usize) -> Result<usize, usize> {
    let mut k = j;
    while k < b.len() {
        match b[k] {
            BEL => return Ok(k + 1),
            ESC if b.get(k + 1) == Some(&b'\\') => return Ok(k + 2),
            b'\n' => return Err(k),
            _ => k += 1,
        }
    }
    Err(k)
}

/// O§7.4 bullet 2: `\r\n` becomes `\n`; in each line only the text after the last `\r` stays.
fn overwrite_cr(text: &str) -> Option<String> {
    if !text.contains('\r') {
        return None;
    }
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.rsplit('\r').next().unwrap_or(l))
        .collect();
    Some(lines.join("\n"))
}
