//! Work-item sources (§13.2): detection, lookup and argument normalization.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use regex::Regex;

use crate::repo::{self, run_bounded, Checkout, RunError};

#[derive(Debug, Default, Clone)]
pub struct Item {
    pub title: Option<String>,
    pub url: Option<String>,
    pub status: Option<String>,
    pub check_hint: Option<String>,
    pub files_hint: Vec<String>,
}

pub trait Source {
    fn name(&self) -> &'static str;
    /// False for sources with nothing to look up (`branch`).
    fn looks_up(&self) -> bool {
        true
    }
    fn detect(&self, co: &Checkout, branch: Option<&str>, notes: &mut Vec<String>) -> Option<String>;
    /// `Err` carries the reason for the failure.
    fn lookup(&self, co: &Checkout, id: &str) -> Result<Item, String>;
    fn normalize(&self, co: &Checkout, arg: &str) -> Option<String>;
}

pub fn by_name(name: &str) -> Option<Box<dyn Source>> {
    match name {
        "work-graph" => Some(Box::new(WorkGraph)),
        "beads" => Some(Box::new(Beads)),
        "github" => Some(Box::new(Github)),
        "branch" => Some(Box::new(Branch)),
        _ => None,
    }
}

fn some(s: Option<&str>) -> Option<String> {
    s.filter(|s| !s.is_empty()).map(String::from)
}

/// Runs `tool` (resolved on `PATH`, `.exe`/`.cmd` included) with `args` in the checkout, bounded by
/// its deadline, and parses its stdout as JSON; an array uses its first element.
fn run_json(tool: &str, args: &[&str], co: &Checkout) -> Result<serde_json::Value, String> {
    let path = which::which(tool).map_err(|_| RunError::NotRun.reason().to_string())?;
    let mut cmd = Command::new(path);
    cmd.args(args).current_dir(&co.top);
    let out = run_bounded(cmd, co.deadline).map_err(|e| e.reason().to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&out).map_err(|_| "unparseable output".to_string())?;
    let v = match v {
        serde_json::Value::Array(mut a) if !a.is_empty() => a.swap_remove(0),
        v => v,
    };
    if v.is_object() { Ok(v) } else { Err("unparseable output".into()) }
}

// ---- work-graph ----

struct Row {
    program: String,
    id: String,
    node: String,
    state: String,
    refs: String,
}

fn cells(line: &str) -> Vec<String> {
    line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect()
}

fn graph_rows(co: &Checkout) -> Vec<Row> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(co.main_root.join("docs/work"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    let mut rows = Vec::new();
    for d in dirs {
        let Ok(text) = std::fs::read_to_string(d.join("graph.md")) else { continue };
        if !text.contains("<!-- work-graph -->") {
            continue;
        }
        let program = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // only the first table under `## Nodes`, up to the next heading, the table's end or a second header
        let section = text
            .lines()
            .skip_while(|l| l.trim() != "## Nodes")
            .skip(1)
            .take_while(|l| !l.trim_start().starts_with('#'))
            .skip_while(|l| !l.trim_start().starts_with('|'))
            .take_while(|l| l.trim_start().starts_with('|'));
        let mut cols: Option<[usize; 4]> = None;
        for line in section {
            let c = cells(line);
            if c.first().map(String::as_str) == Some("ID") {
                if cols.is_some() {
                    break;
                }
                let pos = |n: &str| c.iter().position(|h| h == n);
                cols = (|| Some([pos("ID")?, pos("Node")?, pos("State")?, pos("Refs")?]))();
                if cols.is_none() {
                    break;
                }
                continue;
            }
            let Some([i, n, s, r]) = cols else { break };
            if let (Some(id), Some(node), Some(state), Some(refs)) = (c.get(i), c.get(n), c.get(s), c.get(r)) {
                if id.is_empty() || id.bytes().all(|b| b == b'-' || b == b':') {
                    continue;
                }
                rows.push(Row { program: program.clone(), id: id.clone(), node: node.clone(), state: state.clone(), refs: refs.clone() });
            }
        }
    }
    rows
}

/// True when a `wt <path>` entry of `refs` names `top`; the path ends at `;`, `,` or the cell end.
fn names_checkout(refs: &str, co: &Checkout) -> bool {
    refs.match_indices("wt ").any(|(i, _)| {
        if refs[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric()) {
            return false;
        }
        let p = refs[i + 3..].split([';', ',']).next().unwrap_or("").trim().replace('\\', "/");
        let mut p = p.as_str();
        while let Some(r) = p.strip_prefix("./") {
            p = r;
        }
        let p = p.trim_end_matches('/');
        let path = Path::new(p);
        let full = if path.is_absolute() { path.to_path_buf() } else { co.main_root.join(path) };
        !p.is_empty() && full.canonicalize().is_ok_and(|c| c == co.top)
    })
}

fn spans(line: &str) -> Vec<String> {
    line.split('`').skip(1).step_by(2).map(String::from).collect()
}

struct WorkGraph;

impl Source for WorkGraph {
    fn name(&self) -> &'static str {
        "work-graph"
    }
    fn detect(&self, co: &Checkout, _branch: Option<&str>, notes: &mut Vec<String>) -> Option<String> {
        let hits: Vec<String> = graph_rows(co)
            .into_iter()
            .filter(|r| r.state == "IN-PROGRESS" && names_checkout(&r.refs, co))
            .map(|r| format!("{}/{}", r.program, r.id))
            .collect();
        match hits.len() {
            0 => None,
            1 => hits.into_iter().next(),
            _ => {
                notes.push(format!("zygo: work graph names this checkout twice ({}); no item", hits.join(", ")));
                None
            }
        }
    }
    fn lookup(&self, co: &Checkout, id: &str) -> Result<Item, String> {
        let row = find_row(co, id).ok_or("not found")?;
        let mut item = Item { title: some(Some(&row.node)), status: some(Some(&row.state)), ..Item::default() };
        let safe = !row.id.is_empty() && row.id != ".." && row.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if safe {
            let path = co.main_root.join("docs/work").join(&row.program).join("nodes").join(format!("{}.md", row.id));
            if let Ok(text) = std::fs::read_to_string(path) {
                for line in text.lines() {
                    if let Some(rest) = line.strip_prefix("**Acceptance:**") {
                        item.check_hint = spans(rest).into_iter().next();
                    } else if let Some(rest) = line.strip_prefix("**Files:**") {
                        item.files_hint = spans(rest).into_iter().take(64).collect();
                    }
                }
            }
        }
        Ok(item)
    }
    fn normalize(&self, co: &Checkout, arg: &str) -> Option<String> {
        find_row(co, arg).map(|_| arg.to_string())
    }
}

fn find_row(co: &Checkout, id: &str) -> Option<Row> {
    let (prog, node) = id.split_once('/')?;
    graph_rows(co).into_iter().find(|r| r.program == prog && r.id == node)
}

// ---- beads ----

fn beads_prefix(co: &Checkout) -> Option<String> {
    if !co.top.join(".beads").is_dir() {
        return None;
    }
    let text = std::fs::read_to_string(co.top.join(".beads/config.yaml")).unwrap_or_default();
    let p = text
        .lines()
        .find_map(|l| l.strip_prefix("issue-prefix:"))
        .map(|v| v.split('#').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|v| {
            static RE: OnceLock<Regex> = OnceLock::new();
            RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap()).is_match(v)
        });
    Some(p.unwrap_or_else(|| "bd".into()))
}

/// A beads id at the start of `text`. From a branch (`from_branch`) the id may be followed by `-…`
/// and its hash needs a digit (so `bd-fix-it` is no id); an explicit argument is the whole id.
fn beads_match(prefix: &str, text: &str, from_branch: bool) -> Option<String> {
    let tail = if from_branch { "(?:-|$)" } else { "$" };
    let re = Regex::new(&format!(r"^{}-([a-z0-9]{{3,8}})((?:\.[0-9]+)*){tail}", regex::escape(prefix))).ok()?;
    let c = re.captures(text)?;
    let ok = !from_branch || c[1].bytes().any(|b| b.is_ascii_digit());
    ok.then(|| format!("{prefix}-{}{}", &c[1], &c[2]))
}

struct Beads;

impl Source for Beads {
    fn name(&self) -> &'static str {
        "beads"
    }
    fn detect(&self, co: &Checkout, branch: Option<&str>, _notes: &mut Vec<String>) -> Option<String> {
        beads_match(&beads_prefix(co)?, branch?, true)
    }
    fn lookup(&self, co: &Checkout, id: &str) -> Result<Item, String> {
        let v = run_json("bd", &["--readonly", "show", id, "--json"], co)?;
        Ok(Item {
            title: some(v["title"].as_str()),
            status: some(v["status"].as_str()),
            check_hint: some(v["acceptance_criteria"].as_str()),
            ..Item::default()
        })
    }
    fn normalize(&self, co: &Checkout, arg: &str) -> Option<String> {
        beads_match(&beads_prefix(co)?, arg, false)
    }
}

// ---- github ----

/// `owner/repo` when the repository id is exactly `github.com/owner/repo`.
fn github_slug(co: &Checkout) -> Option<String> {
    let id = repo::repository_id_by(&co.top, co.deadline)?;
    let mut parts = id.split('/');
    let (host, owner, name) = (parts.next()?, parts.next()?, parts.next()?);
    (host == "github.com" && parts.next().is_none()).then(|| format!("{owner}/{name}"))
}

struct Github;

impl Source for Github {
    fn name(&self) -> &'static str {
        "github"
    }
    fn detect(&self, co: &Checkout, branch: Option<&str>, _notes: &mut Vec<String>) -> Option<String> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^(?:(?:gh|issue)-([0-9]{1,6})(?:[^0-9]|$)|([0-9]{1,6})-)").unwrap());
        let c = re.captures(branch?)?;
        let n = c.get(1).or(c.get(2))?.as_str();
        Some(format!("{}#{n}", github_slug(co)?))
    }
    fn lookup(&self, co: &Checkout, id: &str) -> Result<Item, String> {
        let (slug, n) = id.split_once('#').ok_or("failed")?;
        let v = run_json("gh", &["issue", "view", n, "--json", "title,url,state", "-R", slug], co)?;
        Ok(Item { title: some(v["title"].as_str()), url: some(v["url"].as_str()), status: some(v["state"].as_str()), ..Item::default() })
    }
    fn normalize(&self, co: &Checkout, arg: &str) -> Option<String> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^(?:([A-Za-z0-9._-]+/[A-Za-z0-9._-]+)#|#|[Gg][Hh]-)([0-9]{1,6})$").unwrap());
        let c = re.captures(arg)?;
        let slug = github_slug(co)?;
        let slug = c.get(1).map(|m| m.as_str().to_ascii_lowercase()).unwrap_or(slug);
        Some(format!("{slug}#{}", &c[2]))
    }
}

// ---- branch ----

struct Branch;

impl Source for Branch {
    fn name(&self) -> &'static str {
        "branch"
    }
    fn looks_up(&self) -> bool {
        false
    }
    fn detect(&self, co: &Checkout, branch: Option<&str>, notes: &mut Vec<String>) -> Option<String> {
        let b = branch?;
        let id = self.normalize(co, b);
        if id.is_none() {
            notes.push("zygo: branch name is not a usable work item id; no item".into());
        }
        id
    }
    fn lookup(&self, _co: &Checkout, _id: &str) -> Result<Item, String> {
        Ok(Item::default())
    }
    fn normalize(&self, _co: &Checkout, arg: &str) -> Option<String> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9._/-]{1,128}$").unwrap());
        let ok = re.is_match(arg)
            && !arg.starts_with('-')
            && !arg.contains("..")
            && !arg.contains("//")
            && !arg.ends_with('/')
            && !arg.ends_with(".lock");
        ok.then(|| arg.to_string())
    }
}
