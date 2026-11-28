//! Harness text the content pass skips (§16.2 and its 2026-11-18 N9 note; command forms per premises Q6).

use std::ops::Range;

/// Text that starts with one of these is written by the agent, not typed.
pub const PREFIXES: [&str; 6] = [
    "<system-reminder>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<system-update>",
    "Stop hook feedback:",
    "SubagentStop hook feedback:",
];

pub fn is_harness(s: &str) -> bool {
    PREFIXES.iter().any(|p| s.starts_with(p))
}

/// `Some(typed arguments)` when `s` is a `<command-message>` block (empty when it has no
/// `<command-args>`); `None` for any other text.
pub fn command_args(s: &str) -> Option<&str> {
    if !s.starts_with("<command-message>") {
        return None;
    }
    let open = "<command-args>";
    let Some(a) = s.find(open).map(|i| i + open.len()) else { return Some("") };
    Some(s[a..].find("</command-args>").map_or("", |b| &s[a..a + b]))
}

/// Sorted ranges merged where they overlap or touch.
fn merge(spans: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(spans.len());
    for r in spans {
        match merged.last_mut() {
            Some(m) if r.start <= m.end => m.end = m.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// Spans skipped inside user text, sorted and disjoint: each `<command-name>…</command-name>`
/// element of a `<command-message>` block, and each `<tag …>…</tag>` span of the client's skip
/// tags (`ClientSpec::redact_skip_tags`).
pub fn skipped_spans(s: &str, tags: &[&str]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    if s.starts_with("<command-message>") {
        tag_spans(s, "command-name", &mut spans);
    }
    for tag in tags {
        tag_spans(s, tag, &mut spans);
    }
    spans.sort_by_key(|r| r.start);
    merge(spans)
}

/// Start of the next `<name` followed by `>` or whitespace, at or after `from`.
fn next_open(s: &str, open: &str, mut from: usize) -> Option<usize> {
    while let Some(i) = s[from..].find(open) {
        let at = from + i;
        if s[at + open.len()..].chars().next().is_some_and(|c| c == '>' || c.is_whitespace()) {
            return Some(at);
        }
        from = at + open.len();
    }
    None
}

/// An open tag through its close tag, with no other open tag of the same name between; when
/// one comes first, the span restarts at it. A missing close tag opens no span. One forward pass
/// (the next `>` and the next close tag are cached).
fn tag_spans(s: &str, name: &str, out: &mut Vec<Range<usize>>) {
    let (open, close) = (format!("<{name}"), format!("</{name}>"));
    let (mut pos, mut gt_at, mut close_at) = (0, None::<usize>, None::<usize>);
    while let Some(o) = next_open(s, &open, pos) {
        let gt = match gt_at {
            Some(g) if g > o => g,
            _ => match s[o..].find('>') {
                Some(i) => o + i + 1,
                None => return,
            },
        };
        gt_at = Some(gt);
        let c = match close_at {
            Some(c) if c >= gt => c,
            _ => match s[gt..].find(&close) {
                Some(i) => gt + i,
                None => return,
            },
        };
        close_at = Some(c);
        match next_open(s, &open, o + open.len()) {
            Some(o2) if o2 < c => pos = o2,
            _ => {
                out.push(o..c + close.len());
                pos = c + close.len();
            }
        }
    }
}
