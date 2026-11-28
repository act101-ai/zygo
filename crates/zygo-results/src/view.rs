use zygo_config::ResultsConfig;

use crate::shape::Step;
use crate::store::Stored;

pub const VIEW_CAP_BYTES: usize = 8 * 1024;
/// Bytes of the longest O§7.8 shaped marker line with its `\n`: the 139 fixed bytes of the `shaped_view` format,
/// every `Step` name once with its `, ` (162), two numbers of at most 7 digits (the shaped path needs the raw text
/// ≤ `SHAPE_MAX_BYTES`) and the 26-character id. A kind view gets `VIEW_CAP_BYTES` less this (O§7.7).
pub const SHAPED_MARKER_ROOM: usize = 341;
pub const LINE_CAP_CHARS: usize = 400;

pub(crate) fn cut(line: &str) -> String {
    match line.char_indices().nth(LINE_CAP_CHARS) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

fn marker(s: &Stored) -> String {
    format!(
        "[zygo trimmed this output: {} lines, {} bytes; only part of it is shown. The full output is saved as result {id}. To read the rest, call result_search, result_view or result_tree with id \"{id}\".]",
        s.lines,
        s.bytes,
        id = s.id
    )
}

pub fn view(s: &Stored, cfg: &ResultsConfig) -> String {
    size_view(&s.text(), s, cfg)
}

/// The O§7.8 shaped marker line, then `text`. The numbers and id are those of the stored raw text.
pub fn shaped_view(steps: &[Step], text: &str, s: &Stored) -> String {
    let names: Vec<&str> = steps.iter().map(Step::as_str).collect();
    format!(
        "[zygo shaped this output: {}. The full output ({} lines, {} bytes) is saved as result {}; read it with result_view, result_search or result_tree.]\n{text}",
        names.join(", "),
        s.lines,
        s.bytes,
        s.id
    )
}

/// The §10.1 head and tail of `text`, with the trim marker of `s` (the stored raw text).
pub fn size_view(text: &str, s: &Stored, cfg: &ResultsConfig) -> String {
    let all: Vec<&str> = text.lines().collect();
    let head_n = (cfg.head_lines as usize).min(all.len());
    let tail_n = if s.tool.eq_ignore_ascii_case("read") { 0 } else { (cfg.tail_lines as usize).min(all.len() - head_n) };
    let mut head: Vec<String> = all[..head_n].iter().map(|l| cut(l)).collect();
    let mut tail: Vec<String> = all[all.len() - tail_n..].iter().map(|l| cut(l)).collect();
    let marker = marker(s);
    loop {
        let joined = head.iter().chain(std::iter::once(&marker)).chain(tail.iter()).map(String::as_str).collect::<Vec<_>>().join("\n");
        if joined.len() <= VIEW_CAP_BYTES || (head.is_empty() && tail.is_empty()) {
            return joined;
        }
        if !tail.is_empty() {
            tail.remove(0);
        } else {
            head.pop();
        }
    }
}
