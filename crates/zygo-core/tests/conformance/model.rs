//! The conformance input model (K§6.2.1, K§6.2.3) and check items 1-9 (K§6.2.5).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zygo_core::Agent;

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
    /// The file name in `docs/conformance/`, set by `grade_files`; check item 1 compares its stem with `client`.
    #[serde(skip)]
    pub file: Option<String>,
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

/// The evidence of each `met` grade, resolved against the repository once, so that the check (item 3)
/// and the generator read the same result with no file system.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// One entry per `met` grade, in grade-file order, then grade order.
    pub grade: Vec<ResolvedGrade>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGrade {
    /// The index of the grade file in the `grades` given to `resolve`.
    pub file: usize,
    pub client: String,
    pub criterion: String,
    /// Each evidence item of the grade, in file order.
    pub evidence: Vec<ResolvedEvidence>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEvidence {
    /// The evidence string as written in the grade file.
    pub text: String,
    /// `Ok` when the item counts by K§6.2.3, with the CI targets it runs on; else why it does not count.
    pub result: Result<Evidence, String>,
}

impl Resolved {
    /// The first resolved `met` grade of `client` for `criterion`.
    pub fn get(&self, client: &str, criterion: &str) -> Option<&ResolvedGrade> {
        self.grade.iter().find(|x| x.client == client && x.criterion == criterion)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    pub item: u8,
    pub text: String,
}

/// The largest input file the check reads: registry, grade file, premise file, test source.
const READ_MAX: u64 = 1 << 20;

/// Read a file of at most `READ_MAX` bytes; an `Err` text starts with `<shown>: `.
fn read_capped_bytes(path: &Path, shown: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(READ_MAX + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{shown}: {e}"))?;
    if bytes.len() as u64 > READ_MAX {
        return Err(format!("{shown}: the file is over 1 MiB"));
    }
    Ok(bytes)
}

/// Read a UTF-8 file of at most `READ_MAX` bytes; an `Err` text starts with `<path>: `.
fn read_capped(path: &Path) -> Result<String, String> {
    let shown = path.display().to_string();
    String::from_utf8(read_capped_bytes(path, &shown)?).map_err(|e| format!("{shown}: {e}"))
}

/// At most 300 characters of an error text from a parser, so that a long input line does not fill the report.
fn clip(text: String) -> String {
    match text.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text,
    }
}

/// The deepest nesting of `(`, `[` and `{` that `with_parsed` parses; deeper nesting overflows the parser's stack.
const NEST_MAX: usize = 256;

/// The stack of the thread that parses Rust source: `NEST_MAX` levels need more than a test thread's 2 MiB.
const PARSE_STACK: usize = 64 << 20;

/// The end of a quoted literal whose text starts at `start`: the index after the first `close` that no `\`
/// escapes, or the end of `b` when there is none.
fn quoted_end(b: &[u8], start: usize, close: u8) -> usize {
    let mut i = start;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            c if c == close => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// The end of a raw string whose `#` run starts at `start` (after `r`, `br` or `cr`), or `None` when no `"`
/// follows the run (a raw identifier such as `r#match`, or plain code).
fn raw_end(b: &[u8], start: usize) -> Option<usize> {
    let hashes = b[start..].iter().take_while(|&&c| c == b'#').count();
    let body = start + hashes;
    if b.get(body) != Some(&b'"') {
        return None;
    }
    let mut i = body + 1;
    while i < b.len() {
        if b[i] == b'"' && b.len() - (i + 1) >= hashes && b[i + 1..i + 1 + hashes].iter().all(|&c| c == b'#') {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(b.len())
}

/// The deepest nesting of `(`, `[` and `{` in the code of `text`. As Rust's lexer does, it skips the brackets in
/// comments (block comments nest) and in string, byte, C-string, raw and char literals; a `'` that starts no
/// char literal is a lifetime or a label. A literal or block comment with no end runs to the end of the text,
/// whose parse then fails.
fn code_nesting(text: &str) -> usize {
    let b = text.as_bytes();
    let ident = |c: u8| c == b'_' || c.is_ascii_alphanumeric() || c >= 0x80;
    let (mut i, mut depth, mut deepest) = (0usize, 0usize, 0usize);
    while i < b.len() {
        let next = b.get(i + 1).copied();
        match b[i] {
            b'/' if next == Some(b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if next == Some(b'*') => {
                let mut level = 0usize;
                while i < b.len() {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        level += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        level -= 1;
                        i += 2;
                        if level == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b'"' => i = quoted_end(b, i + 1, b'"'),
            b'r' | b'b' | b'c' if i == 0 || !ident(b[i - 1]) => {
                let raw_at = if b[i] == b'r' { Some(i + 1) } else if next == Some(b'r') { Some(i + 2) } else { None };
                if let Some(end) = raw_at.and_then(|at| raw_end(b, at)) {
                    i = end;
                } else if b[i] != b'r' && next == Some(b'"') {
                    i = quoted_end(b, i + 2, b'"');
                } else if b[i] == b'b' && next == Some(b'\'') {
                    i = quoted_end(b, i + 2, b'\'');
                } else {
                    i += 1;
                }
            }
            b'\'' if next == Some(b'\\') => i = quoted_end(b, i + 1, b'\''),
            b'\'' => {
                let len = text[i + 1..].chars().next().map_or(0, char::len_utf8);
                i += if len > 0 && b.get(i + 1 + len) == Some(&b'\'') { len + 2 } else { 1 };
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    deepest
}

/// Parse Rust source text and give the file to `f`, on a thread with a `PARSE_STACK` stack; refuse nesting
/// deeper than `NEST_MAX`, counted on the code of the text (`code_nesting`), before the parse. `syn` types are
/// not `Send`, so `f` returns what the caller needs.
fn with_parsed<T: Send>(rel: &str, text: &str, f: impl FnOnce(&syn::File) -> T + Send) -> Result<T, String> {
    let deepest = code_nesting(text);
    if deepest > NEST_MAX {
        return Err(format!("{rel}: brackets are nested {deepest} deep, more than {NEST_MAX}"));
    }
    std::thread::scope(|scope| {
        let parse = || syn::parse_file(text).map(|file| f(&file)).map_err(|err| format!("{rel}: {err}"));
        std::thread::Builder::new()
            .stack_size(PARSE_STACK)
            .spawn_scoped(scope, parse)
            .map_err(|err| format!("{rel}: no parse thread: {err}"))?
            .join()
            .map_err(|_| format!("{rel}: the parse failed"))?
    })
}

/// Read `path`, then `parse`; an `Err` text starts with `<path>: `.
fn load_with<T>(path: &Path, parse: fn(&str) -> Result<T, String>) -> Result<T, String> {
    let text = read_capped(path)?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// A text the generator prints: no control character, U+2028 or U+2029.
fn need_text(what: &str, text: &str) -> Result<(), String> {
    match text.chars().find(|&c| c.is_control() || c == '\u{2028}' || c == '\u{2029}') {
        Some(c) => Err(format!("{what} `{}` holds the character U+{:04X}", text.escape_debug(), c as u32)),
        None => Ok(()),
    }
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

/// A capability id: `A-n` or `WA-n`, no `.`.
fn need_cap_id(what: &str, id: &str) -> Result<(), String> {
    need_id(what, id)?;
    if id.contains('.') { Err(format!("{what} `{id}` is a criterion id, not a capability id")) } else { Ok(()) }
}

/// A criterion id: `A-n.m` or `WA-n.m`.
fn need_criterion_id(what: &str, id: &str) -> Result<(), String> {
    need_id(what, id)?;
    if id.contains('.') { Ok(()) } else { Err(format!("{what} `{id}` is a capability id, not a criterion id")) }
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
        let reg: Registry = toml::from_str(text).map_err(|e| clip(e.to_string()))?;
        if reg.capability.is_empty() {
            return Err("the registry has no capability".into());
        }
        let mut seen = BTreeSet::new();
        for cap in &reg.capability {
            need_cap_id("capability.id", &cap.id)?;
            need_text("capability.name", &cap.name)?;
            need_text("capability.standard", &cap.standard)?;
            if !seen.insert(cap.id.as_str()) {
                return Err(format!("capability id `{}` is given twice", cap.id));
            }
            if cap.criterion.is_empty() {
                return Err(format!("capability `{}` has no criterion", cap.id));
            }
            for c in &cap.criterion {
                need_criterion_id("criterion.id", &c.id)?;
                if c.id.rsplit_once('.').map(|(head, _)| head) != Some(cap.id.as_str()) {
                    return Err(format!("criterion id `{}` is not of the form `{}.<n>`", c.id, cap.id));
                }
                need_text("criterion.text", &c.text)?;
                if !seen.insert(c.id.as_str()) {
                    return Err(format!("criterion id `{}` is given twice", c.id));
                }
            }
        }
        for b in &reg.bar_exception {
            need_criterion_id("bar_exception.criterion", &b.criterion)?;
            need_text("bar_exception.client", &b.client)?;
            need_line("bar_exception.reason", &b.reason)?;
            need_text("bar_exception.reason", &b.reason)?;
            if let Some(a) = &b.approved {
                need_text("bar_exception.approved", a)?;
            }
        }
        Ok(reg)
    }
}

impl GradeFile {
    pub fn load(path: &Path) -> Result<GradeFile, String> {
        load_with(path, GradeFile::parse)
    }

    pub fn parse(text: &str) -> Result<GradeFile, String> {
        let g: GradeFile = toml::from_str(text).map_err(|e| clip(e.to_string()))?;
        need_text("client", &g.client)?;
        need_text("name", &g.name)?;
        need_text("version", &g.version)?;
        for c in &g.cell {
            need_cap_id("cell.capability", &c.capability)?;
            need_line("cell.url", &c.url)?;
            need_text("cell.url", &c.url)?;
        }
        for e in &g.grade {
            need_criterion_id("grade.criterion", &e.criterion)?;
            if let Some(r) = &e.reason {
                need_line("grade.reason", r)?;
                need_text("grade.reason", r)?;
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
    let bad = |why: &str| Err(format!("evidence `{}`: {why}", e.escape_debug()));
    need_text("evidence", e)?;
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

/// Resolve the evidence of each `met` grade of `grades` (K§6.2.3). A `premise:` row counts for the
/// grade file's `name`; check item 7 holds that name equal to the client's name.
pub fn resolve(root: &Path, grades: &[GradeFile]) -> Resolved {
    let mut cache = ReachCache::default();
    let mut grade = Vec::new();
    for (file, g) in grades.iter().enumerate() {
        for e in g.grade.iter().filter(|e| e.status == Status::Met) {
            let evidence = e
                .evidence
                .iter()
                .map(|text| ResolvedEvidence { text: text.clone(), result: evidence_in(root, text, &g.name, &mut cache) })
                .collect();
            grade.push(ResolvedGrade { file, client: g.client.clone(), criterion: e.criterion.clone(), evidence });
        }
    }
    Resolved { grade }
}

/// The CI targets, in canonical order: `Evidence.platforms` is the subset where a test compiles.
const TARGETS: [&str; 3] = ["linux", "macos", "windows"];

/// For each of `TARGETS`, whether a cfg predicate holds there.
type Targets = [bool; 3];

const EVERY: Targets = [true; 3];

fn names(t: Targets) -> Vec<String> {
    TARGETS.iter().zip(t).filter(|(_, on)| *on).map(|(n, _)| n.to_string()).collect()
}

fn and(a: Targets, b: Targets) -> Targets {
    [a[0] && b[0], a[1] && b[1], a[2] && b[2]]
}

fn or(a: Targets, b: Targets) -> Targets {
    [a[0] || b[0], a[1] || b[1], a[2] || b[2]]
}

/// Whether `e` counts as evidence for the client named `client_name`, and on which CI targets.
/// A `fixture:` or `premise:` item holds on every target.
pub fn evidence(root: &Path, e: &str, client_name: &str) -> Result<Evidence, String> {
    evidence_in(root, e, client_name, &mut ReachCache::default())
}

/// `evidence`, with the test targets of each crate walked once per `cache`.
fn evidence_in(root: &Path, e: &str, client_name: &str, cache: &mut ReachCache) -> Result<Evidence, String> {
    let ev = parse_evidence(e)?;
    let fail = |why: String| format!("evidence `{e}`: {why}");
    let rel = match ev {
        Ev::Test { path, .. } | Ev::Fixture { path } => path,
        Ev::Premise { file, .. } => file,
    };
    let full = root.join(rel);
    let canon = full.canonicalize().map_err(|err| fail(format!("{rel}: {err}")))?;
    let canon_root = root.canonicalize().map_err(|err| fail(format!("{}: {err}", root.display())))?;
    if !canon.starts_with(&canon_root) {
        return Err(fail(format!("{rel} leaves the repository")));
    }
    let every = Evidence { platforms: names(EVERY) };
    match ev {
        Ev::Test { func, .. } => {
            let crate_dir = test_crate_dir(rel).map_err(fail)?;
            crate_manifest(&canon_root, crate_dir, rel).map_err(fail)?;
            let text = read_capped(&canon).map_err(fail)?;
            let mut found = with_parsed(rel, &text, |file| {
                let mut found = Vec::new();
                // The inner attributes of the file (`#![cfg(…)]`) limit each item in it.
                find_tests(&file.items, func, &mut vec![cfg_targets(&file.attrs)], &mut found);
                found
            })
            .map_err(fail)?;
            let found = match found.len() {
                0 => return Err(fail(format!("no test fn `{func}` in {rel}"))),
                1 => found.remove(0),
                n => return Err(fail(format!("ambiguous, {rel} has {n} test fns `{func}`"))),
            };
            let (targets, ignored) = found.map_err(fail)?;
            if ignored {
                return Err(fail(format!("test fn `{func}` is ignored")));
            }
            let reach = cache.0.entry(crate_dir.to_string()).or_insert_with(|| reach(&canon_root.join(crate_dir)));
            let Some(compiled) = reach.get(&canon) else {
                return Err(fail(format!(
                    "no test target compiles {rel}: no {crate_dir}/tests/*.rs reaches it by `mod`, and it is not in the module tree of {crate_dir}/src/lib.rs or src/main.rs"
                )));
            };
            let targets = and(targets, compiled.clone().map_err(|err| fail(format!("a `mod` that reaches {rel}: {err}")))?);
            if targets == [false; 3] {
                return Err(fail(format!("test fn `{func}` compiles on no CI target, so it never runs in CI")));
            }
            Ok(Evidence { platforms: names(targets) })
        }
        Ev::Fixture { .. } => {
            let canon_fixtures = root.join("fixtures").canonicalize().map_err(|err| fail(format!("fixtures/: {err}")))?;
            if !canon.starts_with(&canon_fixtures) {
                return Err(fail(format!("{rel} leaves fixtures/ through a link")));
            }
            // A link in any component, not only the last, is refused.
            let mut at = root.to_path_buf();
            let mut shown = String::new();
            for c in rel.split('/') {
                at.push(c);
                shown = if shown.is_empty() { c.to_string() } else { format!("{shown}/{c}") };
                let meta = std::fs::symlink_metadata(&at).map_err(|err| fail(format!("{shown}: {err}")))?;
                if meta.file_type().is_symlink() {
                    return Err(fail(format!("{shown} is a symbolic link")));
                }
            }
            let meta = std::fs::metadata(&full).map_err(|err| fail(format!("{rel}: {err}")))?;
            if !meta.is_file() || meta.len() <= 2 {
                return Err(fail(format!("{rel} is not a regular file of more than 2 bytes")));
            }
            let bytes = read_capped_bytes(&full, rel).map_err(fail)?;
            // A leading UTF-8 byte order mark (U+FEFF) is not content.
            let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
            let squeezed: Vec<u8> = bytes.iter().copied().filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')).collect();
            if squeezed == b"{}" || squeezed == b"[]" {
                return Err(fail(format!("{rel} is trivial")));
            }
            Ok(every)
        }
        Ev::Premise { file, id } => {
            let text = read_capped(&canon).map_err(fail)?;
            let head = format!("| {id} |");
            let rows: Vec<&str> = text.lines().filter(|l| l.trim_start_matches(' ').starts_with(&head)).collect();
            let [row] = rows[..] else {
                return Err(fail(format!("{file} has {} rows `{id}`, not one", rows.len())));
            };
            let cells = split_row(row);
            let cell = |i: usize| cells.get(i).map(|c| c.trim()).unwrap_or("");
            // A joint row (`Claude Code + tmux`) counts for each agent it names.
            if !cell(3).split(" + ").any(|a| a.trim() == client_name) {
                return Err(fail(format!("row `{id}` is for `{}`, not `{client_name}`", cell(3))));
            }
            let want = if file == "docs/premises.md" { "verified" } else { "pass" };
            if cell(6) != want {
                return Err(fail(format!("the result of `{id}` is `{}`, not `{want}`", cell(6))));
            }
            Ok(every)
        }
    }
}

/// The crate folder of a `test:` path: the components before the first `tests` or `src` after `crates/<name>`.
/// A file under `tests/**/fixtures/` is refused: it is test data, not a test.
fn test_crate_dir(rel: &str) -> Result<&str, String> {
    let parts: Vec<&str> = rel.split('/').collect();
    let Some(i) = (2..parts.len() - 1).find(|&i| parts[i] == "tests" || parts[i] == "src") else {
        return Err(format!("{rel} is not under the tests/ or src/ folder of a crate"));
    };
    if parts[i] == "tests" && parts[i + 1..parts.len() - 1].contains(&"fixtures") {
        return Err(format!("{rel} is under a tests fixtures folder, which holds test data"));
    }
    Ok(&rel[..parts[..i].join("/").len()])
}

/// The crate folder of a `test:` file must hold a `Cargo.toml`, else no test target compiles the file. The
/// walk of `reach` knows only Cargo's default targets, so a manifest that changes the test targets is refused
/// (fail closed): any manifest key that changes which files Cargo compiles as test code. These are any
/// `[[test]]` table, `autotests`, `autobins`, and on `[lib]` or a `[[bin]]` a `harness`, `required-features`
/// or `path` key or `test = false`. Two forms change no target that the walk counts and are let through: a
/// `[[test]]` array whose each entry has a `path` that leaves the crate and a `name` that is no file stem of an
/// entry of `<crate>/tests/`; and a `[[bin]]` with `test = false` and a relative `path` with no `\`, `:` or `..`
/// that is no root of the walk.
fn crate_manifest(root: &Path, crate_dir: &str, rel: &str) -> Result<(), String> {
    let manifest = format!("{crate_dir}/Cargo.toml");
    let path = root.join(&manifest);
    if !path.is_file() {
        return Err(format!("no test target compiles {rel}: {manifest} is missing, so {crate_dir} is not a crate"));
    }
    let text = read_capped(&path)?;
    let table: toml::Table = text.parse().map_err(|e| clip(format!("{manifest}: {e}")))?;
    let unsupported = |key: &str| Err(format!("{manifest}: unsupported Cargo target setting `{key}`"));
    if let Some(tests) = table.get("test") {
        let stems: Vec<String> = std::fs::read_dir(root.join(crate_dir).join("tests"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Path::new(&e.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        let outside = |t: &toml::Value| {
            let path = t.get("path").and_then(|p| p.as_str());
            let name = t.get("name").and_then(|n| n.as_str());
            path.is_some_and(|p| leaves_crate(crate_dir, p)) && name.is_some_and(|n| !stems.iter().any(|s| s == n))
        };
        if !tests.as_array().is_some_and(|a| a.iter().all(outside)) {
            return unsupported("[[test]]");
        }
    }
    for key in ["autotests", "autobins"] {
        if table.get("package").and_then(|p| p.get(key)).is_some() {
            return unsupported(key);
        }
    }
    let lib = table.get("lib").into_iter();
    let bins = table.get("bin").and_then(|b| b.as_array()).into_iter().flatten().filter(|b| !bin_off_the_walk(b));
    for target in lib.chain(bins) {
        for key in ["harness", "required-features", "path"] {
            if target.get(key).is_some() {
                return unsupported(key);
            }
        }
        if target.get("test").and_then(|t| t.as_bool()) == Some(false) {
            return unsupported("test = false");
        }
    }
    Ok(())
}

/// A `[[bin]]` that Cargo never builds as a test and whose source is no root of the walk: `test = false` and a
/// relative `path` with no `\`, `:` or `..` component for which `walk_root` is false.
fn bin_off_the_walk(bin: &toml::Value) -> bool {
    let Some(p) = bin.get("path").and_then(|p| p.as_str()) else { return false };
    bin.get("test").and_then(|t| t.as_bool()) == Some(false)
        && !p.starts_with('/')
        && !p.contains(['\\', ':'])
        && !p.split('/').any(|c| c == "..")
        && !walk_root(p)
}

/// A crate-relative path that is a root of the walk of `reach` or a Cargo default binary: `src/lib.rs`,
/// `src/main.rs`, `src/bin/…`, `tests/<x>` or `tests/<x>/main.rs`. Empty and `.` components are dropped.
fn walk_root(p: &str) -> bool {
    let c: Vec<&str> = p.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    matches!(c.as_slice(), ["src", "lib.rs"] | ["src", "main.rs"] | ["src", "bin", _, ..] | ["tests", _] | ["tests", _, "main.rs"])
}

/// True when `p`, resolved lexically from the repository-relative `crate_dir`, is neither `crate_dir` nor under
/// it. False for an absolute path, a path with `\` or `:`, or one whose `..` pops past the repository root.
fn leaves_crate(crate_dir: &str, p: &str) -> bool {
    if p.starts_with('/') || p.contains(['\\', ':']) {
        return false;
    }
    let mut out: Vec<&str> = crate_dir.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if out.pop().is_none() {
                    return false;
                }
            }
            c => out.push(c),
        }
    }
    let base: Vec<&str> = crate_dir.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    !out.starts_with(&base)
}

/// The CI targets where each `.rs` file of a crate is compiled into a test target, by canonical path; an
/// `Err` when a `mod` on the way has a cfg that is not provable.
type Reach = HashMap<PathBuf, Result<Targets, String>>;

/// The `Reach` of each crate folder, so that `resolve` walks each crate once.
#[derive(Default)]
struct ReachCache(HashMap<String, Reach>);

/// A `mod <name>;` declaration: the files it loads, first one found wins, each with the folder of its own
/// `mod` declarations; and the CI targets where the declaration compiles.
struct ModDecl {
    candidates: Vec<(PathBuf, PathBuf)>,
    cfg: Result<Targets, String>,
}

/// The `#[path = "…"]` value of a `mod` on each CI target, `cfg_attr(<p>, path = "…")` included (the first
/// that applies wins), grouped: each distinct value (`None` for no path) with the targets where it applies.
fn path_choices(attrs: &[syn::Attribute]) -> Result<Vec<(Option<String>, Targets)>, String> {
    let mut per: [Option<String>; 3] = Default::default();
    for a in applied(attrs)? {
        let syn::Meta::NameValue(nv) = &a.meta else { continue };
        if !path_is(&nv.path, &["path"]) {
            continue;
        }
        let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value else { continue };
        let on = when(&a.conds)?;
        for (slot, on) in per.iter_mut().zip(on) {
            if on && slot.is_none() {
                *slot = Some(s.value());
            }
        }
    }
    let mut out: Vec<(Option<String>, Targets)> = Vec::new();
    for (i, p) in per.into_iter().enumerate() {
        match out.iter_mut().find(|(q, _)| *q == p) {
            Some((_, t)) => t[i] = true,
            None => {
                let mut t = [false; 3];
                t[i] = true;
                out.push((p, t));
            }
        }
    }
    Ok(out)
}

/// Push each `mod <name>;` of `items` and of their inline modules. `file_dir` is the folder of the file,
/// `dir` the folder of the modules of this scope; `top` is true at file level (a `#[path]` there is relative
/// to `file_dir`, in an inline module to `dir`).
fn mod_decls(items: &[syn::Item], file_dir: &Path, dir: &Path, top: bool, outer: &Result<Targets, String>, out: &mut Vec<ModDecl>) {
    use syn::ext::IdentExt;
    for item in items {
        let syn::Item::Mod(m) = item else { continue };
        let cfg = outer.clone().and_then(|o| Ok(and(o, cfg_targets(&m.attrs)?)));
        let name = m.ident.unraw().to_string();
        // Each file the `mod` can load is walked with the targets limited to where that path applies.
        let (choices, cfg) = match path_choices(&m.attrs) {
            Ok(c) => (c, cfg),
            Err(e) => (vec![(None, EVERY)], Err(e)),
        };
        for (path, on) in choices {
            let cfg = cfg.clone().map(|c| and(c, on));
            match (&m.content, path) {
                (Some((_, inner)), path) => {
                    let sub = dir.join(path.unwrap_or_else(|| name.clone()));
                    mod_decls(inner, file_dir, &sub, false, &cfg, out);
                }
                (None, Some(p)) => {
                    let f = if top { file_dir.join(&p) } else { dir.join(&p) };
                    let owned = f.parent().map(Path::to_path_buf).unwrap_or_default();
                    out.push(ModDecl { candidates: vec![(f, owned)], cfg });
                }
                (None, None) => {
                    let sub = dir.join(&name);
                    let candidates = vec![(dir.join(format!("{name}.rs")), sub.clone()), (sub.join("mod.rs"), sub)];
                    out.push(ModDecl { candidates, cfg });
                }
            }
        }
    }
}

/// A module folder in one spelling: canonical when it exists, else with `.` and `..` resolved on the text, so
/// that a walk through `..` or a link revisits a known folder and ends.
fn folder_key(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| {
        let mut out = PathBuf::new();
        for c in dir.components() {
            match c {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                c => out.push(c),
            }
        }
        out
    })
}

/// Walk a crate's test targets (`tests/*.rs`, `tests/*/main.rs`) and its crate roots (`src/lib.rs`,
/// `src/main.rs`) through each `mod` declaration. A file that cannot be read or parsed adds no module.
/// A visit is a file in one module folder: a file that two `mod`s load with different module folders is
/// walked once per folder, each with its own targets, so the modules under one folder get only its targets.
fn reach(crate_dir: &Path) -> Reach {
    let mut visits: HashMap<(PathBuf, PathBuf), Result<Targets, String>> = HashMap::new();
    let mut queue: Vec<(PathBuf, PathBuf, Result<Targets, String>)> = Vec::new();
    let mut roots = vec![crate_dir.join("src/lib.rs"), crate_dir.join("src/main.rs")];
    for e in std::fs::read_dir(crate_dir.join("tests")).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            roots.push(p.join("main.rs"));
        } else if p.extension().is_some_and(|x| x == "rs") {
            roots.push(p);
        }
    }
    // A visit in the same folder merges its targets by union; the walk ends when no visit grows.
    let mut merge = |file: PathBuf, dir: PathBuf, t: Result<Targets, String>, queue: &mut Vec<_>| {
        let key = (file, folder_key(&dir));
        let new = match (visits.get(&key), t) {
            (None, t) => t,
            (Some(Ok(old)), Ok(t)) if or(*old, t) != *old => Ok(or(*old, t)),
            (Some(Err(_)), Ok(t)) => Ok(t),
            _ => return,
        };
        queue.push((key.0.clone(), key.1.clone(), new.clone()));
        visits.insert(key, new);
    };
    for r in roots.into_iter().filter(|r| r.is_file()) {
        if let Ok(c) = r.canonicalize() {
            let dir = c.parent().map(Path::to_path_buf).unwrap_or_default();
            merge(c, dir, Ok(EVERY), &mut queue);
        }
    }
    while let Some((file, dir, t)) = queue.pop() {
        let Ok(text) = read_capped(&file) else { continue };
        let file_dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let shown = file.display().to_string();
        let decls = with_parsed(&shown, &text, |f| {
            let mut out = Vec::new();
            // The inner cfg of the file limits each `mod` in it.
            let t = t.and_then(|t| Ok(and(t, cfg_targets(&f.attrs)?)));
            mod_decls(&f.items, &file_dir, &dir, true, &t, &mut out);
            out
        });
        for d in decls.into_iter().flatten() {
            let Some((f, sub)) = d.candidates.into_iter().find(|(f, _)| f.is_file()) else { continue };
            if let Ok(c) = f.canonicalize() {
                merge(c, sub, d.cfg, &mut queue);
            }
        }
    }
    // A file compiles on the union of the targets of its visits; an `Ok` visit outweighs an `Err` one.
    let mut reach = Reach::new();
    for ((file, _), t) in visits {
        let new = match (reach.remove(&file), t) {
            (None, t) | (Some(Err(_)), t) => t,
            (Some(Ok(old)), Ok(t)) => Ok(or(old, t)),
            (Some(Ok(old)), Err(_)) => Ok(old),
        };
        reach.insert(file, new);
    }
    reach
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

fn path_is(p: &syn::Path, want: &[&str]) -> bool {
    p.segments.len() == want.len() && p.segments.iter().zip(want).all(|(s, w)| s.ident == w)
}

fn is_test_attr(p: &syn::Path) -> bool {
    path_is(p, &["test"]) || path_is(p, &["tokio", "test"])
}

/// The CI targets where a cfg predicate holds. Only `unix`, `windows`, `test` (true), `target_os`,
/// `target_family`, `not`, `all` and `any` are provable; any other predicate is refused.
fn eval_cfg(m: &syn::Meta) -> Result<Targets, String> {
    let name = |p: &syn::Path| p.get_ident().map(|i| i.to_string()).unwrap_or_else(|| "a path".into());
    let unprovable = |p: &syn::Path| Err(format!("the cfg predicate `{}` is not provable on the CI targets", name(p)));
    match m {
        syn::Meta::Path(p) => match name(p).as_str() {
            "unix" => Ok([true, true, false]),
            "windows" => Ok([false, false, true]),
            "test" => Ok(EVERY),
            _ => unprovable(p),
        },
        syn::Meta::NameValue(nv) => {
            let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value else { return unprovable(&nv.path) };
            let v = s.value();
            match name(&nv.path).as_str() {
                "target_os" => Ok(TARGETS.map(|t| t == v)),
                "target_family" => Ok([v == "unix", v == "unix", v == "windows"]),
                _ => unprovable(&nv.path),
            }
        }
        syn::Meta::List(l) => {
            let args = l
                .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                .map_err(|e| format!("the cfg predicate `{}(…)` is not readable: {e}", name(&l.path)))?;
            match name(&l.path).as_str() {
                "all" => args.iter().try_fold(EVERY, |acc, m| Ok(and(acc, eval_cfg(m)?))),
                "any" => args.iter().try_fold([false; 3], |acc, m| Ok(or(acc, eval_cfg(m)?))),
                "not" if args.len() == 1 => eval_cfg(&args[0]).map(|t| t.map(|b| !b)),
                _ => unprovable(&l.path),
            }
        }
    }
}

/// One attribute that applies where each of `conds` holds: the `cfg_attr` predicates around it, outermost first.
struct Applied {
    conds: Vec<syn::Meta>,
    meta: syn::Meta,
}

/// Each attribute of `attrs`, with each `cfg_attr(<pred>, <attr>, …)` unwrapped, nested ones included.
fn applied(attrs: &[syn::Attribute]) -> Result<Vec<Applied>, String> {
    fn unwrap(meta: syn::Meta, conds: Vec<syn::Meta>, out: &mut Vec<Applied>) -> Result<(), String> {
        let syn::Meta::List(l) = &meta else {
            out.push(Applied { conds, meta });
            return Ok(());
        };
        if !path_is(&l.path, &["cfg_attr"]) {
            out.push(Applied { conds, meta });
            return Ok(());
        }
        let list = l
            .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
            .map_err(|_| "a cfg_attr is not readable".to_string())?;
        let mut list = list.into_iter();
        let Some(pred) = list.next() else { return Ok(()) };
        for m in list {
            let mut c = conds.clone();
            c.push(pred.clone());
            unwrap(m, c, out)?;
        }
        Ok(())
    }
    let mut out = Vec::new();
    for a in attrs {
        unwrap(a.meta.clone(), Vec::new(), &mut out)?;
    }
    Ok(out)
}

/// The CI targets where each of `conds` holds.
fn when(conds: &[syn::Meta]) -> Result<Targets, String> {
    conds.iter().try_fold(EVERY, |acc, p| Ok(and(acc, eval_cfg(p)?)))
}

/// The CI targets where the item compiles: each `cfg(<q>)` holds where it applies. A
/// `cfg_attr(<p>, cfg(<q>))` applies where `p` holds, so it keeps the item where `p` does not hold or `q` holds.
fn cfg_of(attrs: &[Applied]) -> Result<Targets, String> {
    let mut t = EVERY;
    for a in attrs.iter().filter(|a| path_is(a.meta.path(), &["cfg"])) {
        let syn::Meta::List(l) = &a.meta else { return Err("a cfg is not readable".into()) };
        let pred: syn::Meta = l.parse_args().map_err(|e| format!("a cfg is not readable: {e}"))?;
        let on = when(&a.conds)?;
        let holds = eval_cfg(&pred)?;
        t = and(t, or(on.map(|b| !b), holds));
    }
    Ok(t)
}

/// The CI targets where an item with `attrs` compiles.
fn cfg_targets(attrs: &[syn::Attribute]) -> Result<Targets, String> {
    cfg_of(&applied(attrs)?)
}

/// A test fn found: the CI targets where it is a test and compiles, and whether it is ignored on one.
type Found = Result<(Targets, bool), String>;

/// `Some` when `f` is a test fn (`#[test]`, `#[tokio::test]`, or either inside `#[cfg_attr(<pred>, …)]`, nested or not).
fn test_fn(f: &syn::ItemFn, outer: Targets) -> Option<Found> {
    let attrs = match applied(&f.attrs) {
        Ok(a) => a,
        Err(e) => return Some(Err(e)),
    };
    let tests: Vec<&Applied> = attrs.iter().filter(|a| is_test_attr(a.meta.path())).collect();
    if tests.is_empty() {
        return None;
    }
    Some((|| {
        let mut test = [false; 3];
        for a in &tests {
            test = or(test, when(&a.conds)?);
        }
        let targets = and(and(test, outer), cfg_of(&attrs)?);
        let mut ignored = false;
        for a in attrs.iter().filter(|a| path_is(a.meta.path(), &["ignore"])) {
            ignored |= when(&a.conds)? != [false; 3];
        }
        Ok((targets, ignored))
    })())
}

/// Push each test fn named `func` in `items` and in inline modules; `stack` holds the targets of the enclosing modules.
fn find_tests(items: &[syn::Item], func: &str, stack: &mut Vec<Result<Targets, String>>, found: &mut Vec<Found>) {
    for item in items {
        match item {
            syn::Item::Fn(f) if f.sig.ident == func => {
                let outer = stack.iter().try_fold(EVERY, |acc, t| t.clone().map(|t| and(acc, t)));
                match outer {
                    Ok(outer) => found.extend(test_fn(f, outer)),
                    Err(e) => found.extend(test_fn(f, EVERY).map(|_| Err(e))),
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    stack.push(cfg_targets(&m.attrs));
                    find_tests(inner, func, stack, found);
                    stack.pop();
                }
            }
            _ => {}
        }
    }
}

/// True when the text is absent or only white space.
pub fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|s| s.trim().is_empty())
}

/// The header lines of a premise file: each line before the first that starts with `##`, a line-end `\r` removed.
fn header_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = read_capped(path)?;
    Ok(text.lines().take_while(|l| !l.starts_with("##")).map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect())
}

/// Check items 1-9 (K§6.2.5). Item 3 reads `resolved`, the result of `resolve(root, grades)`, so the
/// evidence is resolved once for the check and the generator; a `met` grade with no entry in `resolved` is an item 3 fault.
pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client], resolved: &Resolved) -> Vec<Fault> {
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
        if let Some(f) = &g.file {
            let stem = Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            if stem != g.client {
                fault(1, format!("grade file `{f}`: its stem `{stem}` is not its client `{}`", g.client));
            }
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
    // 3. each met grade has an item of evidence that counts; a met grade with no entry in `resolved` is a fault
    for g in grades {
        for e in g.grade.iter().filter(|e| e.status == Status::Met) {
            if resolved.get(&g.client, &e.criterion).is_none() {
                fault(3, format!("client `{}`: met criterion `{}`: evidence not resolved", g.client, e.criterion));
            }
        }
    }
    for x in &resolved.grade {
        if !x.evidence.iter().any(|e| e.result.is_ok()) {
            let errs: Vec<&str> = x.evidence.iter().filter_map(|e| e.result.as_ref().err().map(String::as_str)).collect();
            let why = if errs.is_empty() { "no evidence".to_string() } else { errs.join("; ") };
            fault(3, format!("client `{}`: met criterion `{}` has no evidence that counts: {why}", x.client, x.criterion));
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
    // 5. one https cell url for each capability, and no cell for another capability
    for g in grades {
        for cap in &reg.capability {
            let cells: Vec<&CellUrl> = g.cell.iter().filter(|x| x.capability == cap.id).collect();
            if cells.len() > 1 {
                fault(5, format!("client `{}`: capability `{}` has {} cells", g.client, cap.id, cells.len()));
            } else if !cells.iter().any(|x| x.url.starts_with("https://")) {
                fault(5, format!("client `{}`: capability `{}` has no https cell url", g.client, cap.id));
            }
        }
        for x in &g.cell {
            if !reg.capability.iter().any(|cap| cap.id == x.capability) {
                fault(5, format!("client `{}`: cell for capability `{}`, which is not in the registry", g.client, x.capability));
            }
        }
    }
    // 6. each bar exception: once per criterion, a reason, a known client, a core criterion not met, an approval
    for (i, b) in reg.bar_exception.iter().enumerate() {
        let who = format!("bar exception `{}` `{}`", b.client, b.criterion);
        if reg.bar_exception[..i].iter().any(|a| a.client == b.client && a.criterion == b.criterion) {
            fault(6, format!("{who}: given twice"));
        }
        if b.reason.trim().is_empty() {
            fault(6, format!("{who}: no reason"));
        }
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
    // 7. the client name and version are the graded ones and the pinned premise version
    let premises = header_lines(&root.join("docs/premises.md"));
    let live = header_lines(&root.join("docs/live-checks.md"));
    for c in clients {
        let [g] = files_of(&c.id)[..] else { continue };
        if g.name != c.name {
            fault(7, format!("client `{}`: graded name `{}` is not the client name `{}`", c.id, g.name, c.name));
        }
        if g.version != c.version {
            fault(7, format!("client `{}`: graded version {} is not the client version {}", c.id, g.version, c.version));
        }
        for (file, lines, want) in [
            ("docs/premises.md", &premises, format!("{}: {}", c.name, g.version)),
            ("docs/live-checks.md", &live, format!("{}: {} or later", c.name, g.version)),
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
        let failing = crate::derive::bar(reg, g).failing;
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

/// The display name of a client (P4-6 replaces this with the descriptor).
fn client_name(a: Agent) -> &'static str {
    match a {
        Agent::ClaudeCode => "Claude Code",
        Agent::OpenCode => "OpenCode",
    }
}

/// The `.toml` files in `docs/conformance/` that are not grade files: the registry and CC-15's exception list.
const NOT_GRADE_FILES: [&str; 2] = ["capabilities.toml", "boundary-exceptions.toml"];

/// The grade files in `dir`: each file whose extension is `toml` in any case, except `NOT_GRADE_FILES`, names
/// matched exactly as listed. Subfolders are not grade files (the flat layout of K§6.2.1).
/// `<client id>.toml` for each client in `Agent::ALL` comes first, in that order, then the others by name, so
/// that check item 1 sees a grade file of an unknown client or a second file of a client. A client with no
/// `<client id>.toml`, a grade file that is a symbolic link or not a regular file, and a name that is not UTF-8 are refused.
pub fn grade_files(dir: &Path) -> Result<Vec<GradeFile>, String> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = e.path();
        let name = e.file_name().into_string().map_err(|n| format!("{}: a name that is not UTF-8: {n:?}", dir.display()))?;
        let toml = Path::new(&name).extension().is_some_and(|x| x.eq_ignore_ascii_case("toml"));
        if !toml || NOT_GRADE_FILES.contains(&name.as_str()) {
            continue;
        }
        let kind = e.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("{}: the grade file is a symbolic link", path.display()));
        }
        if !kind.is_file() {
            return Err(format!("{}: the grade file is not a regular file", path.display()));
        }
        names.push(name);
    }
    names.sort();
    let mut ordered = Vec::new();
    for a in Agent::ALL {
        let want = format!("{}.toml", a.as_str());
        let Some(i) = names.iter().position(|n| *n == want) else {
            return Err(format!("{}: no grade file for client `{}`", dir.join(want).display(), a.as_str()));
        };
        ordered.push(names.remove(i));
    }
    ordered.extend(names);
    ordered
        .iter()
        .map(|n| {
            let path = dir.join(n);
            let mut g = GradeFile::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            g.file = Some(n.clone());
            Ok(g)
        })
        .collect()
}

/// One `Client` per `Agent::ALL`, its version the built-in minimum in `crates/zygo-config/data/agents.toml`.
pub fn clients(root: &Path) -> Result<Vec<Client>, String> {
    let path = root.join("crates/zygo-config/data/agents.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let table: toml::Table = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    Agent::ALL
        .iter()
        .map(|&a| {
            let id = a.as_str();
            let version = table
                .get(id)
                .and_then(|t| t.get("min_version"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("{}: no `[{id}] min_version`", path.display()))?;
            Ok(Client { id: id.into(), name: client_name(a).into(), version: version.into(), supported: true })
        })
        .collect()
}
