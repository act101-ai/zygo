//! Text from outside zygo, made safe to print.

/// Displayed text with `\n`, `\r`, `\t` and every other control, bidi or line-separator character escaped, so it cannot forge a line.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') => {
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
}
