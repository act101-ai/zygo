//! Text from outside zygo, made safe to print.

/// Displayed text with `\n`, `\r`, `\t` and every other control, bidi or line-separator character escaped, so it cannot forge a line.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') => {
                o.push_str(&format!("\\u{{{:02x}}}", c as u32));
            }
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::esc;

    #[test]
    fn line_and_paragraph_separators_are_escaped() {
        assert_eq!(esc("a\u{2028}b\u{2029}c"), "a\\u{2028}b\\u{2029}c");
    }

    #[test] // FIX-23 (D6): kills: a Bidi_Control character printed raw; the list is the 12 characters with the Unicode property Bidi_Control
    fn every_bidi_control_character_is_escaped() {
        let bidi = ['\u{61c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'];
        let raw: Vec<String> = bidi.into_iter().filter(|c| esc(&format!("a{c}b")) != format!("a\\u{{{:x}}}b", *c as u32)).map(|c| format!("U+{:04X}", c as u32)).collect();
        assert!(raw.is_empty(), "not escaped: {raw:?}");
    }
}
