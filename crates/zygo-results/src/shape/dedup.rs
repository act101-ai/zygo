//! Dedup step (O§7.5): runs of equal consecutive lines, lossless.

/// `None`: no change (no run of 3 or more equal non-blank lines, or a line already counted).
pub fn dedup(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.iter().any(|l| counted(l).is_some()) {
        return None;
    }
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let n = lines[i..].iter().take_while(|l| **l == line).count();
        if n >= 3 && !line.trim().is_empty() {
            out.push(format!("{line} [repeated {n} times]"));
            changed = true;
        } else {
            out.extend(std::iter::repeat_n(line.to_string(), n));
        }
        i += n;
    }
    changed.then(|| out.join("\n"))
}

/// O§7.5 expansion: each line that ends in ` [repeated <n> times]` becomes `<n>` copies of its base.
pub fn expand(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        match counted(line).and_then(|(base, d)| Some((base, d.parse::<usize>().ok()?))) {
            Some((base, n)) => out.extend(std::iter::repeat_n(base, n)),
            None => out.push(line),
        }
    }
    out.join("\n")
}

/// `Some((base, digits))` when `line` ends in the counted form (Decision 3).
fn counted(line: &str) -> Option<(&str, &str)> {
    let (base, digits) = line.strip_suffix(" times]")?.rsplit_once(" [repeated ")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some((base, digits))
}
