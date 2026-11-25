//! The conformance input model (K§6.2.1, K§6.2.3) and check items 1-9 (K§6.2.5).

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub capability: Vec<Capability>,
    #[serde(default)]
    pub bar_exception: Vec<BarException>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub tier: Tier,
    pub standard: String,
    pub criterion: Vec<Criterion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Core,
    Standard,
    Extra,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarException {
    pub client: String,
    pub criterion: String,
    pub reason: String,
    pub approved: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradeFile {
    pub client: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub cell: Vec<CellUrl>,
    #[serde(default)]
    pub grade: Vec<GradeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellUrl {
    pub capability: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradeEntry {
    pub criterion: String,
    pub status: Status,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Status {
    #[serde(rename = "met")]
    Met,
    #[serde(rename = "not-met")]
    NotMet,
    #[serde(rename = "n/a")]
    Na,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub version: String,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    pub item: u8,
    pub text: String,
}

/// Read `path`, then `parse`; an `Err` text starts with `<path>: `.
fn load_with<T>(path: &Path, parse: fn(&str) -> Result<T, String>) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// `^(A|WA)-[0-9]+(\.[0-9]+)?$`
fn valid_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("A-").or_else(|| id.strip_prefix("WA-")) else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match rest.split_once('.') {
        Some((a, b)) => digits(a) && digits(b),
        None => digits(rest),
    }
}

fn need_id(what: &str, id: &str) -> Result<(), String> {
    if valid_id(id) { Ok(()) } else { Err(format!("{what} `{id}` is not an id of the form A-n, A-n.m, WA-n or WA-n.m")) }
}

/// A one-line text of at most 300 characters.
fn need_line(what: &str, text: &str) -> Result<(), String> {
    if text.contains(['\n', '\r']) {
        return Err(format!("{what} `{}` holds a line break", text.escape_debug()));
    }
    if text.chars().count() > 300 {
        return Err(format!("{what} has more than 300 characters"));
    }
    Ok(())
}

impl Registry {
    pub fn load(path: &Path) -> Result<Registry, String> {
        load_with(path, Registry::parse)
    }

    pub fn parse(text: &str) -> Result<Registry, String> {
        let reg: Registry = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut seen = BTreeSet::new();
        for cap in &reg.capability {
            need_id("capability.id", &cap.id)?;
            if !seen.insert(cap.id.as_str()) {
                return Err(format!("capability id `{}` is given twice", cap.id));
            }
            for c in &cap.criterion {
                need_id("criterion.id", &c.id)?;
                if !seen.insert(c.id.as_str()) {
                    return Err(format!("criterion id `{}` is given twice", c.id));
                }
            }
        }
        for b in &reg.bar_exception {
            need_id("bar_exception.criterion", &b.criterion)?;
            need_line("bar_exception.reason", &b.reason)?;
        }
        Ok(reg)
    }
}

impl GradeFile {
    pub fn load(path: &Path) -> Result<GradeFile, String> {
        load_with(path, GradeFile::parse)
    }

    pub fn parse(text: &str) -> Result<GradeFile, String> {
        let g: GradeFile = toml::from_str(text).map_err(|e| e.to_string())?;
        for c in &g.cell {
            need_id("cell.capability", &c.capability)?;
            need_line("cell.url", &c.url)?;
        }
        for e in &g.grade {
            need_id("grade.criterion", &e.criterion)?;
            if let Some(r) = &e.reason {
                need_line("grade.reason", r)?;
            }
            for ev in &e.evidence {
                parse_evidence(ev)?;
            }
        }
        Ok(g)
    }
}

/// One evidence item, by kind (K§6.2.3).
enum Ev<'a> {
    Test { path: &'a str, func: &'a str },
    Fixture { path: &'a str },
    Premise { file: &'a str, id: &'a str },
}

/// A repository-relative path: not empty, not absolute, no `\`, `:` or `..` component.
fn need_rel(e: &str, path: &str) -> Result<(), String> {
    if path.is_empty()
        || Path::new(path).is_absolute()
        || path.starts_with('/')
        || path.contains(['\\', ':'])
        || path.split('/').any(|c| c == "..")
    {
        return Err(format!("evidence `{e}`: path `{path}` is not a plain repository-relative path"));
    }
    Ok(())
}

fn parse_evidence(e: &str) -> Result<Ev<'_>, String> {
    let bad = |why: &str| Err(format!("evidence `{e}`: {why}"));
    if let Some(rest) = e.strip_prefix("test:") {
        let Some((path, func)) = rest.rsplit_once("::") else { return bad("no `::<fn>`") };
        need_rel(e, path)?;
        if !path.starts_with("crates/") || !path.ends_with(".rs") {
            return bad("a test path starts with `crates/` and ends with `.rs`");
        }
        let mut cs = func.chars();
        let ok = cs.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && cs.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return bad("the function name is not an identifier");
        }
        Ok(Ev::Test { path, func })
    } else if let Some(path) = e.strip_prefix("fixture:") {
        need_rel(e, path)?;
        if !path.starts_with("fixtures/") {
            return bad("a fixture path starts with `fixtures/`");
        }
        Ok(Ev::Fixture { path })
    } else if let Some(rest) = e.strip_prefix("premise:") {
        let Some((file, id)) = rest.split_once('#') else { return bad("no `#<id>`") };
        if file != "docs/premises.md" && file != "docs/live-checks.md" {
            return bad("a premise file is docs/premises.md or docs/live-checks.md");
        }
        if id.is_empty() || id.contains(|c: char| c.is_whitespace() || c == '|') {
            return bad("the premise id is empty or holds a space or `|`");
        }
        Ok(Ev::Premise { file, id })
    } else {
        bad("the kind is not test:, fixture: or premise:")
    }
}

pub fn evidence(root: &Path, e: &str) -> Result<Evidence, String> {
    let _ev = parse_evidence(e)?;
    todo!()
}

pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client]) -> Vec<Fault> {
    todo!()
}

/// CF-2 replaces this with derive::bar(reg, g).failing
fn failing_core(reg: &Registry, g: &GradeFile) -> Vec<String> {
    todo!()
}

pub fn clients(root: &Path) -> Result<Vec<Client>, String> {
    todo!()
}
