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
    let ev = parse_evidence(e)?;
    let rel = match ev {
        Ev::Test { path, .. } | Ev::Fixture { path } => path,
        Ev::Premise { file, .. } => file,
    };
    let full = root.join(rel);
    let canon = full.canonicalize().map_err(|err| format!("evidence `{e}`: {rel}: {err}"))?;
    let canon_root = root.canonicalize().map_err(|err| format!("evidence `{e}`: {}: {err}", root.display()))?;
    if !canon.starts_with(&canon_root) {
        return Err(format!("evidence `{e}`: {rel} leaves the repository"));
    }
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|err| format!("evidence `{e}`: {rel}: {err}"));
    match ev {
        Ev::Test { func, .. } => {
            let file = syn::parse_file(&read(&canon)?).map_err(|err| format!("evidence `{e}`: {rel}: {err}"))?;
            let mut found = Vec::new();
            find_tests(&file.items, func, &mut Vec::new(), &mut found);
            let (attrs, platforms) = match found.len() {
                0 => return Err(format!("evidence `{e}`: no test fn `{func}` in {rel}")),
                1 => found.remove(0),
                _ => return Err(format!("evidence `{e}`: ambiguous, {rel} has {} test fns `{func}`", found.len())),
            };
            if attrs.iter().any(is_ignore) {
                return Err(format!("evidence `{e}`: test fn `{func}` is ignored"));
            }
            Ok(Evidence { platforms })
        }
        Ev::Fixture { .. } => {
            let meta = std::fs::symlink_metadata(&full).map_err(|err| format!("evidence `{e}`: {rel}: {err}"))?;
            if meta.file_type().is_symlink() {
                return Err(format!("evidence `{e}`: {rel} is a symbolic link"));
            }
            if !meta.is_file() || meta.len() <= 2 {
                return Err(format!("evidence `{e}`: {rel} is not a regular file of more than 2 bytes"));
            }
            let bytes = std::fs::read(&full).map_err(|err| format!("evidence `{e}`: {rel}: {err}"))?;
            let squeezed: Vec<u8> = bytes.into_iter().filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')).collect();
            if squeezed == b"{}" || squeezed == b"[]" {
                return Err(format!("evidence `{e}`: {rel} is trivial"));
            }
            Ok(Evidence { platforms: vec![] })
        }
        Ev::Premise { file, id } => {
            let text = read(&canon)?;
            let head = format!("| {id} |");
            let rows: Vec<&str> = text.lines().filter(|l| l.trim_start_matches(' ').starts_with(&head)).collect();
            let [row] = rows[..] else {
                return Err(format!("evidence `{e}`: {file} has {} rows `{id}`, not one", rows.len()));
            };
            let want = if file == "docs/premises.md" { "verified" } else { "pass" };
            let cells = split_row(row);
            let got = cells.get(6).map(|c| c.trim()).unwrap_or("");
            if got != want {
                return Err(format!("evidence `{e}`: the result of `{id}` is `{got}`, not `{want}`"));
            }
            Ok(Evidence { platforms: vec![] })
        }
    }
}

/// Split a table row on each `|` not preceded by `\`.
fn split_row(row: &str) -> Vec<&str> {
    let mut cells = Vec::new();
    let (mut start, mut prev) = (0, '\0');
    for (i, c) in row.char_indices() {
        if c == '|' && prev != '\\' {
            cells.push(&row[start..i]);
            start = i + 1;
        }
        prev = c;
    }
    cells.push(&row[start..]);
    cells
}

/// The text of each `#[cfg(…)]` in `attrs`, leaving out a bare `test`.
fn cfgs(attrs: &[syn::Attribute], out: &mut Vec<String>) {
    for a in attrs {
        if let syn::Meta::List(l) = &a.meta {
            if l.path.is_ident("cfg") {
                let t = l.tokens.to_string();
                if t != "test" {
                    out.push(t);
                }
            }
        }
    }
}

fn path_is(p: &syn::Path, want: &[&str]) -> bool {
    p.segments.len() == want.len() && p.segments.iter().zip(want).all(|(s, w)| s.ident == w)
}

/// Push each test fn named `func` in `items` and in inline modules, with its attributes and platforms.
fn find_tests(items: &[syn::Item], func: &str, stack: &mut Vec<String>, found: &mut Vec<(Vec<syn::Attribute>, Vec<String>)>) {
    for item in items {
        match item {
            syn::Item::Fn(f) if f.sig.ident == func => {
                let is_test = f.attrs.iter().any(|a| path_is(a.path(), &["test"]) || path_is(a.path(), &["tokio", "test"]));
                if is_test {
                    let mut platforms = stack.clone();
                    cfgs(&f.attrs, &mut platforms);
                    found.push((f.attrs.clone(), platforms));
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    let depth = stack.len();
                    cfgs(&m.attrs, stack);
                    find_tests(inner, func, stack, found);
                    stack.truncate(depth);
                }
            }
            _ => {}
        }
    }
}

/// `#[ignore]` in any form, or a `cfg_attr(…)` whose tokens hold the ident `ignore`.
fn is_ignore(a: &syn::Attribute) -> bool {
    if path_is(a.path(), &["ignore"]) {
        return true;
    }
    let syn::Meta::List(l) = &a.meta else { return false };
    if !path_is(&l.path, &["cfg_attr"]) {
        return false;
    }
    // The idents of the token text, string literals left out.
    let text = l.tokens.to_string();
    let mut plain = String::new();
    let (mut in_str, mut esc) = (false, false);
    for c in text.chars() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            plain.push(' ');
        } else if c == '"' {
            in_str = true;
            plain.push(' ');
        } else {
            plain.push(c);
        }
    }
    plain.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == "ignore")
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|s| s.trim().is_empty())
}

/// The header lines of a premise file: each line before the first that starts with `##`, a line-end `\r` removed.
fn header_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text.lines().take_while(|l| !l.starts_with("##")).map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect())
}

pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client]) -> Vec<Fault> {
    let mut faults = Vec::new();
    let mut fault = |item: u8, text: String| faults.push(Fault { item, text });
    let files_of = |id: &str| grades.iter().filter(|g| g.client == id).collect::<Vec<_>>();
    let criteria: Vec<(&Capability, &Criterion)> =
        reg.capability.iter().flat_map(|cap| cap.criterion.iter().map(move |c| (cap, c))).collect();
    let core_criterion = |id: &str| criteria.iter().any(|(cap, c)| c.id == id && cap.tier == Tier::Core);

    // 1. one grade file per client, and no grade file for another client
    for c in clients {
        let n = files_of(&c.id).len();
        if n != 1 {
            fault(1, format!("client `{}` has {n} grade files, not one", c.id));
        }
    }
    for g in grades {
        if !clients.iter().any(|c| c.id == g.client) {
            fault(1, format!("grade file for `{}` names no client", g.client));
        }
    }
    // 2. each registry criterion graded exactly once, and no other
    for g in grades {
        for (_, c) in &criteria {
            let n = g.grade.iter().filter(|e| e.criterion == c.id).count();
            if n == 0 {
                fault(2, format!("client `{}`: criterion `{}` has no grade", g.client, c.id));
            } else if n > 1 {
                fault(2, format!("client `{}`: criterion `{}` is graded {n} times", g.client, c.id));
            }
        }
        for e in &g.grade {
            if !criteria.iter().any(|(_, c)| c.id == e.criterion) {
                fault(2, format!("client `{}`: criterion `{}` is not in the registry", g.client, e.criterion));
            }
        }
    }
    // 3. each met grade has an item of evidence that counts
    for g in grades {
        for e in g.grade.iter().filter(|e| e.status == Status::Met) {
            let errs: Vec<String> = e.evidence.iter().map(|x| evidence(root, x)).filter_map(Result::err).collect();
            if errs.len() == e.evidence.len() {
                let why = if errs.is_empty() { "no evidence".to_string() } else { errs.join("; ") };
                fault(3, format!("client `{}`: met criterion `{}` has no evidence that counts: {why}", g.client, e.criterion));
            }
        }
    }
    // 4. a reason for each not-met or n/a grade; no n/a on a core criterion
    for g in grades {
        for e in &g.grade {
            if e.status != Status::Met && blank(&e.reason) {
                fault(4, format!("client `{}`: criterion `{}` has no reason", g.client, e.criterion));
            }
            if e.status == Status::Na && core_criterion(&e.criterion) {
                fault(4, format!("client `{}`: core criterion `{}` is n/a", g.client, e.criterion));
            }
        }
    }
    // 5. an https cell url for each capability
    for g in grades {
        for cap in &reg.capability {
            if !g.cell.iter().any(|x| x.capability == cap.id && x.url.starts_with("https://")) {
                fault(5, format!("client `{}`: capability `{}` has no https cell url", g.client, cap.id));
            }
        }
    }
    // 6. each bar exception: a known client, a core criterion not met, an approval
    for b in &reg.bar_exception {
        let who = format!("bar exception `{}` `{}`", b.client, b.criterion);
        if !clients.iter().any(|c| c.id == b.client) {
            fault(6, format!("{who}: client `{}` is not a client", b.client));
        }
        let met = files_of(&b.client)
            .iter()
            .any(|g| g.grade.iter().any(|e| e.criterion == b.criterion && e.status == Status::Met));
        if met {
            fault(6, format!("{who}: criterion `{}` is met", b.criterion));
        }
        if !core_criterion(&b.criterion) {
            fault(6, format!("{who}: criterion `{}` is not a core criterion", b.criterion));
        }
        if blank(&b.approved) {
            fault(6, format!("{who}: not approved"));
        }
    }
    // 7. the client version is the graded version and the pinned premise version
    let premises = header_lines(&root.join("docs/premises.md"));
    let live = header_lines(&root.join("docs/live-checks.md"));
    for c in clients {
        let [g] = files_of(&c.id)[..] else { continue };
        if g.version != c.version {
            fault(7, format!("client `{}`: graded version {} is not the client version {}", c.id, g.version, c.version));
        }
        for (file, lines, want) in [
            ("docs/premises.md", &premises, format!("{}: {}", g.name, g.version)),
            ("docs/live-checks.md", &live, format!("{}: {} or later", g.name, g.version)),
        ] {
            match lines {
                Err(e) => fault(7, format!("client `{}`: {e}", c.id)),
                Ok(lines) if !lines.contains(&want) => fault(7, format!("client `{}`: {file} has no line `{want}`", c.id)),
                Ok(_) => {}
            }
        }
    }
    // 8. a supported client meets the bar
    for c in clients.iter().filter(|c| c.supported) {
        let [g] = files_of(&c.id)[..] else { continue };
        let failing = failing_core(reg, g);
        if !failing.is_empty() {
            fault(8, format!("client `{}` is supported and below the bar: {}", c.id, failing.join(", ")));
        }
    }
    // 9. a fixtures folder with an entry for each client
    for c in clients {
        let dir = root.join("fixtures").join(&c.id);
        let has_entry = std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some());
        if !has_entry {
            fault(9, format!("client `{}`: fixtures/{}/ is missing or empty", c.id, c.id));
        }
    }
    faults
}

/// CF-2 replaces this with derive::bar(reg, g).failing
fn failing_core(reg: &Registry, g: &GradeFile) -> Vec<String> {
    let ok = |criterion: &str| {
        g.grade.iter().any(|e| e.criterion == criterion && e.status == Status::Met)
            || reg.bar_exception.iter().any(|b| b.client == g.client && b.criterion == criterion)
    };
    reg.capability
        .iter()
        .filter(|cap| cap.tier == Tier::Core && cap.criterion.iter().any(|c| !ok(&c.id)))
        .map(|cap| cap.id.clone())
        .collect()
}

pub fn clients(root: &Path) -> Result<Vec<Client>, String> {
    todo!()
}
