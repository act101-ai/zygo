use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::store::Stored;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Json,
    Jsonl,
    Diff,
    Log,
    Tree,
    Text,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Json => "json",
            Kind::Jsonl => "jsonl",
            Kind::Diff => "diff",
            Kind::Log => "log",
            Kind::Tree => "tree",
            Kind::Text => "text",
        }
    }
}

pub const SNIFF_BYTES: usize = 4 * 1024;

static PATH_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^(\./|\.\./|/)?[^\s:*?"<>|]+(/[^\s:*?"<>|]+)+/?$"#).unwrap());
static LOG_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\W?\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}").unwrap());
static LOG_LEVEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\W*(ERROR|WARN|WARNING|INFO|DEBUG|TRACE)\b").unwrap());

fn is_json_start(l: &str) -> bool {
    l.starts_with('{') || l.starts_with('[')
}

fn parses(l: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(l).is_ok()
}

fn mostly(lines: &[&str], f: impl Fn(&str) -> bool) -> bool {
    let live: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    live.iter().filter(|l| f(l)).count() * 2 > live.len()
}

pub fn sniff(data: &[u8]) -> Kind {
    let head = &data[..data.len().min(SNIFF_BYTES)];
    let valid = match std::str::from_utf8(head) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).unwrap_or(""),
    };
    let truncated = data.len() > SNIFF_BYTES;
    let mut lines: Vec<&str> = valid.lines().collect();
    if truncated {
        lines.pop();
    }
    if lines.len() >= 2 && lines.iter().all(|l| is_json_start(l) && parses(l)) {
        return Kind::Jsonl;
    }
    if is_json_start(valid.trim_start()) {
        match serde_json::from_str::<serde_json::Value>(valid) {
            Ok(_) => return Kind::Json,
            Err(e) if truncated && e.is_eof() => return Kind::Json,
            Err(_) => {}
        }
    }
    let diff = lines.iter().enumerate().any(|(i, l)| l.starts_with("diff --git ") || (l.starts_with("--- ") && lines.get(i + 1).is_some_and(|n| n.starts_with("+++ "))));
    if diff {
        return Kind::Diff;
    }
    if mostly(&lines, |l| l.contains("├── ") || l.contains("└── ") || PATH_LINE.is_match(l)) {
        return Kind::Tree;
    }
    if mostly(&lines, |l| LOG_TIME.is_match(l) || LOG_LEVEL.is_match(l)) {
        return Kind::Log;
    }
    Kind::Text
}

static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{1,6} ").unwrap());

const JSON_DEPTH: usize = 3;

pub fn tree(s: &Stored) -> Result<String, String> {
    match s.kind {
        Kind::Json => json_tree(&s.data),
        Kind::Diff => Ok(diff_files(&s.text())),
        Kind::Tree => Ok(dir_tree(&s.text(), s.data.len())),
        Kind::Text => Ok(s.text().lines().enumerate().filter(|(_, l)| HEADING.is_match(l)).map(|(i, l)| format!("{}: {l}", i + 1)).collect::<Vec<_>>().join("\n")),
        Kind::Log | Kind::Jsonl => Ok(String::new()),
    }
}

fn json_type(v: &serde_json::Value) -> String {
    use serde_json::Value::*;
    match v {
        Object(_) => "object".to_string(),
        Array(a) => format!("array[{}]", a.len()),
        String(_) => "string".to_string(),
        Number(_) => "number".to_string(),
        Bool(_) => "bool".to_string(),
        Null => "null".to_string(),
    }
}

fn json_entry(key: &str, v: &serde_json::Value, level: usize, out: &mut Vec<String>) {
    out.push(format!("{}{key}: {}", "  ".repeat(level - 1), json_type(v)));
    if level < JSON_DEPTH {
        json_children(v, level + 1, out);
    }
}

fn json_children(v: &serde_json::Value, level: usize, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(m) => m.iter().for_each(|(k, c)| json_entry(k, c, level, out)),
        serde_json::Value::Array(a) => {
            if let Some(first) = a.first().filter(|f| f.is_object() || f.is_array()) {
                json_entry("[]", first, level, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn json_tree(data: &[u8]) -> Result<String, String> {
    let root: serde_json::Value = serde_json::from_slice(data).map_err(|_| "the result is not valid JSON".to_string())?;
    let mut out = Vec::new();
    if root.is_array() {
        out.push(json_type(&root));
    }
    json_children(&root, 1, &mut out);
    Ok(out.join("\n"))
}

pub(crate) fn diff_path(line: &str, marker: &str, side: &str) -> String {
    let rest = line.strip_prefix(marker).unwrap_or(line);
    let path = rest.split('\t').next().unwrap_or(rest);
    path.strip_prefix(side).unwrap_or(path).to_string()
}

fn diff_files(text: &str) -> String {
    let mut files = Vec::new();
    let mut prev_old = "";
    for l in text.lines() {
        if l.starts_with("--- ") {
            prev_old = l;
        } else if l.starts_with("+++ ") {
            let new = diff_path(l, "+++ ", "b/");
            files.push(if new == "/dev/null" { diff_path(prev_old, "--- ", "a/") } else { new });
        }
    }
    files.join("\n")
}

pub const TREE_CUT: &str = "[tree cut: longer than the stored result]";

/// Lines of a tree outline whose total length (each line plus its `\n`) stays within `limit`, the stored result's size (C5).
/// The first line that would pass it is not written; `TREE_CUT` ends the output instead.
struct Bounded {
    out: String,
    limit: usize,
    cut: bool,
}

impl Bounded {
    fn new(limit: usize) -> Self {
        Bounded { out: String::new(), limit, cut: false }
    }
    /// Writes `indent` spaces, `name` and `suffix` as one line; false once the limit is reached.
    fn line(&mut self, indent: usize, name: &str, suffix: &str) -> bool {
        if self.cut {
            return false;
        }
        let used = if self.out.is_empty() { 0 } else { self.out.len() + 1 };
        if used + indent + name.len() + suffix.len() + 1 > self.limit {
            self.cut = true;
            return false;
        }
        if !self.out.is_empty() {
            self.out.push('\n');
        }
        self.out.extend(std::iter::repeat_n(' ', indent));
        self.out.push_str(name);
        self.out.push_str(suffix);
        true
    }
    fn finish(mut self) -> String {
        if self.cut {
            if !self.out.is_empty() {
                self.out.push('\n');
            }
            self.out.push_str(TREE_CUT);
        }
        self.out
    }
}

fn dir_tree(text: &str, limit: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let marker_at = |l: &str| l.chars().position(|c| c == '├' || c == '└');
    let mut out = Bounded::new(limit);
    if lines.iter().any(|l| l.contains("├── ") || l.contains("└── ")) {
        let entries: Vec<(usize, String)> = lines
            .iter()
            .map(|l| match marker_at(l) {
                Some(i) => (1 + i / 4, l.chars().skip(i + 4).collect()),
                None => (0, l.to_string()),
            })
            .collect();
        for (i, (d, name)) in entries.iter().enumerate() {
            if entries.get(i + 1).is_some_and(|n| n.0 > *d) && !out.line(2 * d, name, "/") {
                break;
            }
        }
    } else {
        // a trie of path components: each component is stored once; node 0 is the root
        let mut names: Vec<&str> = vec![""];
        let mut children: Vec<std::collections::BTreeMap<&str, usize>> = vec![Default::default()];
        for l in &lines {
            let parts: Vec<&str> = l.split('/').filter(|p| !p.is_empty()).collect();
            let mut at = 0;
            for p in parts.iter().take(parts.len().saturating_sub(1)) {
                at = match children[at].get(p) {
                    Some(&c) => c,
                    None => {
                        names.push(p);
                        children.push(Default::default());
                        let c = names.len() - 1;
                        children[at].insert(p, c);
                        c
                    }
                };
            }
        }
        // depth-first, children in name order, without recursion
        let mut stack: Vec<(usize, usize)> = children[0].values().rev().map(|&c| (c, 0)).collect();
        while let Some((n, depth)) = stack.pop() {
            if !out.line(2 * depth, names[n], "/") {
                break;
            }
            stack.extend(children[n].values().rev().map(|&c| (c, depth + 1)));
        }
    }
    out.finish()
}
