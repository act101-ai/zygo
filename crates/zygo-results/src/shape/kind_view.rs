//! Step 4 of the pipeline, the kind views (O§7.7).

use std::sync::LazyLock;

use regex::Regex;

use super::{Shaper, Step};
use crate::kind::{diff_path, json_tree};
use crate::Kind;

/// Head and tail line counts of the log view (O§7.7).
const LOG_EDGE: usize = 20;

static LOG_KEEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(ERROR|WARN|WARNING|FATAL|CRITICAL)\b|^(panic|Traceback)").unwrap());

/// The kind view of `text`, at most `cap` bytes, with its step and shaper; `None` when the kind has no view
/// or the view cannot fit its first unit (O§7.7).
pub fn kind_view(text: &str, kind: Kind, cap: usize) -> Option<(String, Step, Shaper)> {
    match kind {
        Kind::Diff => diff_view(text, cap).map(|v| (v, Step::DiffFilesListed, Shaper::Diff)),
        Kind::Json => json_view(text, cap).map(|v| (v, Step::JsonOutlined, Shaper::Json)),
        Kind::Log => log_view(text, cap).map(|v| (v, Step::LogErrorsKept, Shaper::Log)),
        Kind::Jsonl | Kind::Tree | Kind::Text => None,
    }
}

/// A line as the view writes it: verbatim, with a `\n` added when it has none.
fn push_line(out: &mut String, l: &str) {
    out.push_str(l);
    if !l.ends_with('\n') {
        out.push('\n');
    }
}

fn line_len(l: &str) -> usize {
    l.len() + usize::from(!l.ends_with('\n'))
}

fn diff_marker(n: usize) -> String {
    format!("[zygo: {n} file sections not shown]\n")
}

/// `<path> +<added> -<removed>` of one file section.
fn diff_list_line(section: &[&str]) -> String {
    let bare = |l: &str| l.trim_end_matches(['\n', '\r']).to_string();
    let hunk = section.iter().position(|l| l.starts_with("@@")).unwrap_or(section.len());
    let header = &section[..hunk];
    let new = header.iter().find(|l| l.starts_with("+++ ")).map(|l| diff_path(&bare(l), "+++ ", "b/"));
    let path = match new {
        Some(p) if p == "/dev/null" => header.iter().find(|l| l.starts_with("--- ")).map_or(p, |l| diff_path(&bare(l), "--- ", "a/")),
        Some(p) => p,
        None => {
            let first = bare(section[0]);
            first.rsplit_once(" b/").map_or_else(|| first.trim_start_matches("diff --git ").to_string(), |(_, p)| p.to_string())
        }
    };
    let body = section.get(hunk + 1..).unwrap_or(&[]);
    let added = body.iter().filter(|l| l.starts_with('+')).count();
    let removed = body.iter().filter(|l| l.starts_with('-')).count();
    format!("{path} +{added} -{removed}\n")
}

fn diff_view(text: &str, cap: usize) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let git = lines.iter().any(|l| l.starts_with("diff --git "));
    let starts: Vec<usize> = (0..lines.len())
        .filter(|&i| if git { lines[i].starts_with("diff --git ") } else { lines[i].starts_with("--- ") && lines.get(i + 1).is_some_and(|n| n.starts_with("+++ ")) })
        .collect();
    let sections: Vec<&[&str]> = starts.iter().enumerate().map(|(k, &s)| &lines[s..starts.get(k + 1).copied().unwrap_or(lines.len())]).collect();
    let mut view: String = sections.iter().map(|s| diff_list_line(s)).collect();
    let mut shown = 0;
    for (i, s) in sections.iter().enumerate() {
        let size: usize = s.iter().map(|l| line_len(l)).sum();
        if view.len() + size + diff_marker(sections.len() - i - 1).len() > cap {
            break;
        }
        s.iter().for_each(|l| push_line(&mut view, l));
        shown = i + 1;
    }
    if shown == 0 {
        return None;
    }
    view.push_str(&diff_marker(sections.len() - shown));
    Some(view)
}

fn json_view(text: &str, cap: usize) -> Option<String> {
    let outline = json_tree(text.as_bytes()).ok()?;
    let mut view = String::new();
    for l in outline.lines() {
        if view.len() + l.len() + 1 > cap / 2 {
            break;
        }
        push_line(&mut view, l);
    }
    for l in text.split_inclusive('\n') {
        if view.len() + line_len(l) > cap {
            break;
        }
        push_line(&mut view, l);
    }
    (!view.is_empty()).then_some(view)
}

fn log_view(text: &str, cap: usize) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let n = lines.len();
    if n <= 2 * LOG_EDGE {
        return None;
    }
    let join = |ls: &[&str]| ls.iter().map(|l| format!("{l}\n")).collect::<String>();
    let head = join(&lines[..LOG_EDGE]);
    let tail = join(&lines[n - LOG_EDGE..]);
    if head.len() + tail.len() > cap {
        return None;
    }
    let mut view = head;
    for (i, l) in lines.iter().enumerate().take(n - LOG_EDGE).skip(LOG_EDGE) {
        if !LOG_KEEP.is_match(l) {
            continue;
        }
        let kept = format!("{}: {l}\n", i + 1);
        if view.len() + kept.len() + tail.len() > cap {
            break;
        }
        view.push_str(&kept);
    }
    view.push_str(&tail);
    Some(view)
}
