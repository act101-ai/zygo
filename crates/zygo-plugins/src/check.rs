//! `zygo plugin check` (spec §15.6, §15.7): structure rules, code items and the code digest.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    Malformed, BadName, NameNotDir, BadDescription, BadVersion, VersionMismatch,
    FileTooLarge, PluginTooLarge, Symlink, PathOutside,
}
impl Rule {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rule::Malformed => "malformed",
            Rule::BadName => "bad-name",
            Rule::NameNotDir => "name-not-dir",
            Rule::BadDescription => "bad-description",
            Rule::BadVersion => "bad-version",
            Rule::VersionMismatch => "version-mismatch",
            Rule::FileTooLarge => "file-too-large",
            Rule::PluginTooLarge => "plugin-too-large",
            Rule::Symlink => "symlink",
            Rule::PathOutside => "path-outside",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation { pub rule: Rule, pub path: String, pub detail: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodeKind {
    McpServer, LspServer, HookCommand, File, InlineShell, ShellBlock,
    FrontmatterHooks, FrontmatterAllowedTools,
}
impl CodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeKind::McpServer => "mcp-server",
            CodeKind::LspServer => "lsp-server",
            CodeKind::HookCommand => "hook-command",
            CodeKind::File => "file",
            CodeKind::InlineShell => "inline-shell",
            CodeKind::ShellBlock => "shell-block",
            CodeKind::FrontmatterHooks => "frontmatter-hooks",
            CodeKind::FrontmatterAllowedTools => "frontmatter-allowed-tools",
        }
    }
}

/// Field order = sort order. `line` is display only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CodeItem { pub path: String, pub kind: CodeKind, pub name: String, pub data: Vec<u8>, pub line: Option<u32> }

#[derive(Debug, Clone)]
pub struct PluginReport {
    pub name: String,
    pub version: String,
    pub license: Option<String>,
    pub dir: PathBuf,
    pub violations: Vec<Violation>,
    pub code: Vec<CodeItem>,
    pub digest: Option<String>,
    pub copied: Vec<String>,
    pub total_bytes: u64,
}
impl PluginReport {
    pub fn ok(&self) -> bool { self.violations.is_empty() }
}

#[derive(Debug, Clone)]
pub struct CheckReport {
    pub marketplace: Option<String>,
    pub violations: Vec<Violation>,
    pub plugins: Vec<PluginReport>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}
impl CheckReport {
    pub fn ok(&self) -> bool { self.violations.is_empty() && self.plugins.iter().all(|p| p.ok()) }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(m) = &self.marketplace { out.push_str(&format!("marketplace {}\n", esc(m))); }
        for w in &self.warnings { out.push_str(&format!("warning {}\n", esc(w))); }
        for v in &self.violations { out.push_str(&format!("{}\n", violation_line(v))); }
        for s in &self.skipped { out.push_str(&format!("skipped {}: source is not a relative path\n", esc(s))); }
        for p in &self.plugins {
            let n = p.violations.len();
            let status = match n { 0 => "ok".to_string(), 1 => "1 violation".to_string(), n => format!("{n} violations") };
            out.push_str(&format!("plugin {} {}: {status}\n", esc(&p.name), esc(&p.version)));
            for v in &p.violations { out.push_str(&format!("  {}\n", violation_line(v))); }
            for c in &p.code {
                let mut head = format!("  code {} {}", c.kind.as_str(), esc(&c.path));
                if !c.name.is_empty() { head.push_str(&format!(" {}", esc(&c.name))); }
                if let Some(l) = c.line { head.push_str(&format!(":{l}")); }
                let shown = shown(c);
                let mut lines = shown.split('\n');
                out.push_str(&format!("{head}: {}\n", esc(lines.next().unwrap_or(""))));
                for l in lines { out.push_str(&format!("      {}\n", esc(l))); }
            }
            match &p.digest {
                Some(d) => out.push_str(&format!("  digest {d}\n")),
                None => out.push_str("  code: none\n"),
            }
        }
        out
    }
}

fn violation_line(v: &Violation) -> String { format!("violation {} {}: {}", v.rule.as_str(), esc(&v.path), esc(&v.detail)) }

/// The text `render()` shows for an item: a file's size, a server without its env values, else the data.
fn shown(c: &CodeItem) -> String {
    match c.kind {
        CodeKind::File => format!("{} bytes", c.data.len()),
        CodeKind::McpServer | CodeKind::LspServer => {
            let mut v: Value = serde_json::from_slice(&c.data).unwrap_or_default();
            if let Some(env) = v.get_mut(5) {
                *env = Value::Array(env.as_array().map(|a| a.iter().filter_map(|kv| kv.get(0).cloned()).collect()).unwrap_or_default());
            }
            v.to_string()
        }
        _ => String::from_utf8_lossy(&c.data).into_owned(),
    }
}

/// Displayed text with `\n`, `\r`, `\t` and every other control or bidi character escaped, so it cannot forge a line.
pub use zygo_core::esc;

fn viol(rule: Rule, path: &str, detail: impl Into<String>) -> Violation {
    Violation { rule, path: path.to_string(), detail: detail.into() }
}

const MARKETPLACE_JSON: &str = ".claude-plugin/marketplace.json";
const PLUGIN_JSON: &str = ".claude-plugin/plugin.json";
/// The plugin parts §15.1 copies to the store.
const PARTS: &[&str] = &[
    PLUGIN_JSON, "skills", "agents", "commands", "hooks/hooks.json", ".mcp.json", ".lsp.json", "scripts", "bin",
    "opencode/plugin.js",
];

/// Check a marketplace or single-plugin directory.
pub fn check_dir(dir: &Path) -> CheckReport { check_dir_by(dir, Instant::now() + Duration::from_secs(2)) }

/// `check_dir` with its git call bounded by `min(2 s, deadline)`.
pub fn check_dir_by(dir: &Path, deadline: Instant) -> CheckReport { check_dir_modes(dir, deadline, None) }

/// `check_dir_by` with `Some(modes)` (`dir`-relative path → git index mode) used in place of the git call.
pub fn check_dir_modes(dir: &Path, deadline: Instant, modes: Option<HashMap<String, String>>) -> CheckReport {
    let mut r = CheckReport { marketplace: None, violations: vec![], plugins: vec![], skipped: vec![], warnings: vec![] };
    let index = modes.or_else(|| git_modes(dir, deadline)).unwrap_or_else(|| {
        r.warnings.push("git index unavailable: exec bits and symlinks read from disk only".to_string());
        HashMap::new()
    });
    let ctx = Ctx { root: dir.to_path_buf(), index };
    if dir.join(MARKETPLACE_JSON).is_file() {
        check_marketplace(&ctx, &mut r);
    } else if dir.join(PLUGIN_JSON).is_file() {
        r.plugins.push(check_plugin(&ctx, String::new()));
    } else {
        r.violations.push(viol(Rule::Malformed, ".", "no .claude-plugin/marketplace.json or .claude-plugin/plugin.json"));
    }
    r
}

/// The check root and its git index (D6).
struct Ctx { root: PathBuf, index: HashMap<String, String> }
impl Ctx {
    /// D7: `key` (root-relative, `/`-separated) is a symlink on disk or in the git index.
    fn is_link(&self, key: &str) -> bool {
        std::fs::symlink_metadata(self.root.join(key)).is_ok_and(|m| m.file_type().is_symlink())
            || self.index.get(key).is_some_and(|m| m == "120000")
    }
}

/// `rel` (`/`-separated) exists under `base` with each segment's name byte-equal to a `read_dir` entry
/// (a case-insensitive file system would otherwise match `Scripts` for `scripts`).
fn exists_exact(base: &Path, rel: &str) -> bool {
    let mut dir = base.to_path_buf();
    for seg in rel.split('/') {
        let Ok(rd) = std::fs::read_dir(&dir) else { return false };
        if !rd.flatten().any(|e| e.file_name() == std::ffi::OsStr::new(seg)) { return false; }
        dir.push(seg);
    }
    true
}

/// D6: `git ls-files -s -z` as root-relative path → mode; empty when `dir` is not in a work tree; `None` when
/// git is absent, fails or runs over `min(2 s, deadline)`. Runs through the sync builder (`sync::git_command`):
/// nothing from the checked repository runs (no fsmonitor, no hooks), no inherited repository or config
/// variables, the process group is killed at the deadline. Round 3: without the builder's
/// `GIT_CEILING_DIRECTORIES`, so `dir` may be a subdirectory of a work tree (sync keeps the ceiling).
pub fn git_modes(dir: &Path, deadline: Instant) -> Option<HashMap<String, String>> {
    let mut c = crate::sync::git_command(dir);
    c.env_remove("GIT_CEILING_DIRECTORIES");
    git_modes_by(c, deadline)
}

/// `git_modes` with a prepared git command (sync passes its program and global config).
pub(crate) fn git_modes_by(mut c: std::process::Command, deadline: Instant) -> Option<HashMap<String, String>> {
    let deadline = deadline.min(Instant::now() + Duration::from_secs(2));
    if Instant::now() >= deadline { return None; }
    c.args(["ls-files", "-s", "-z"]);
    let stdout = match crate::sync::run_bounded(c, deadline) {
        Ok(b) => b,
        Err(e) => return e.to_string().contains("not a git repository").then(HashMap::new),
    };
    Some(String::from_utf8_lossy(&stdout).split('\0').filter_map(|rec| {
        let (meta, path) = rec.split_once('\t')?;
        Some((path.to_string(), meta.split(' ').next()?.to_string()))
    }).collect())
}

/// `prefix/rel`, or `rel` when `prefix` is empty.
fn join_rel(prefix: &str, rel: &str) -> String { if prefix.is_empty() { rel.to_string() } else { format!("{prefix}/{rel}") } }

/// The first symlink among the cumulative prefixes of `rel` (`a`, `a/b`, …) under `prefix`.
fn first_link(ctx: &Ctx, prefix: &str, rel: &str) -> Option<String> {
    let mut cur = String::new();
    for seg in rel.split('/') {
        cur = join_rel(&cur, seg);
        if ctx.is_link(&join_rel(prefix, &cur)) { return Some(cur); }
    }
    None
}

fn check_marketplace(ctx: &Ctx, r: &mut CheckReport) {
    if let Some(l) = first_link(ctx, "", MARKETPLACE_JSON) {
        return r.violations.push(viol(Rule::Symlink, &l, "symlinks are refused"));
    }
    let dir = &ctx.root;
    let m = match read_json(&dir.join(MARKETPLACE_JSON)) {
        Ok(m) => m,
        Err(e) => return r.violations.push(viol(Rule::Malformed, MARKETPLACE_JSON, e)),
    };
    let mname = m.get("name").and_then(Value::as_str).unwrap_or("");
    if !valid_name(mname) { r.violations.push(viol(Rule::BadName, MARKETPLACE_JSON, format!("marketplace name {mname:?}"))); }
    r.marketplace = Some(mname.to_string());
    let Some(entries) = m.get("plugins").and_then(Value::as_array) else {
        return r.violations.push(viol(Rule::Malformed, MARKETPLACE_JSON, "plugins is not an array"));
    };
    let entry_name = |e: &Value| e.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for e in entries { *seen.entry(entry_name(e)).or_default() += 1; }
    for e in entries {
        let name = entry_name(e);
        let source = match e.get("source").and_then(Value::as_str) {
            Some(s) if outside(s) => {
                r.violations.push(viol(Rule::PathOutside, MARKETPLACE_JSON, format!("plugin {name}: source {s} is outside the marketplace")));
                continue;
            }
            Some(s) if s.starts_with("./") => s,
            _ => {
                r.skipped.push(name);
                continue;
            }
        };
        let prefix = segments(source).collect::<Vec<_>>().join("/");
        if !prefix.is_empty() && let Some(l) = first_link(ctx, "", &prefix) {
            r.violations.push(viol(Rule::Symlink, &l, format!("plugin {name}: symlinks are refused")));
            continue;
        }
        let mut p = check_plugin(ctx, prefix);
        if !valid_name(&name) { p.violations.push(viol(Rule::BadName, MARKETPLACE_JSON, format!("plugin entry name {name:?}"))); }
        if name != p.name {
            p.violations.push(viol(Rule::BadName, MARKETPLACE_JSON, format!("plugin entry name {name:?} is not plugin.json name {:?}", p.name)));
        }
        if seen[&name] > 1 { p.violations.push(viol(Rule::BadName, MARKETPLACE_JSON, format!("plugin entry name {name:?} is listed more than once"))); }
        if e.get("description").is_some_and(|d| !description_ok(d)) {
            p.violations.push(viol(Rule::BadDescription, MARKETPLACE_JSON, format!("plugin {name}: description is not 1-1024 characters")));
        }
        if let Some(v) = e.get("version") {
            match v.as_str().map(semver::Version::parse) {
                Some(Ok(ev)) => {
                    if semver::Version::parse(&p.version).is_ok_and(|pv| pv != ev) {
                        p.violations.push(viol(Rule::VersionMismatch, MARKETPLACE_JSON,
                            format!("plugin {name}: marketplace version {ev} is not plugin.json version {}", p.version)));
                    }
                }
                _ => p.violations.push(viol(Rule::BadVersion, MARKETPLACE_JSON, format!("plugin {name}: version is not semver"))),
            }
        }
        r.plugins.push(p);
    }
}

/// 1–64 of `[a-z0-9-]`, no leading, trailing or double `-`.
pub fn valid_name(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--")
}

fn description_ok(v: &Value) -> bool { v.as_str().is_some_and(|d| (1..=1024).contains(&d.chars().count())) }

/// One top-level frontmatter key and its scalar value.
struct FmEntry { key: String, value: String }

/// The lines between a first `---` line (after an optional UTF-8 BOM) and the next `---` line; the first
/// returned line is line 2 of the file. `None` without a block.
fn frontmatter_lines(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.strip_prefix('\u{FEFF}').unwrap_or(text).lines();
    if lines.next().map(str::trim_end) != Some("---") { return None; }
    let body: Vec<&str> = lines.collect();
    let end = body.iter().position(|l| l.trim_end() == "---")?;
    Some(body[..end].to_vec())
}

/// The top-level `key: value` entries of the frontmatter block.
fn frontmatter(text: &str) -> Option<Vec<FmEntry>> {
    let mut lines = frontmatter_lines(text)?;
    lines.insert(0, "---");
    let end = lines.len();
    let mut out = vec![];
    let mut i = 1;
    while i < end {
        let l = lines[i];
        let Some((k, rest)) = l.split_once(':').filter(|_| !l.starts_with([' ', '\t'])) else { i += 1; continue };
        let mut j = i + 1;
        while j < end && lines[j].starts_with([' ', '\t']) { j += 1; }
        let cont: Vec<&str> = lines[i + 1..j].iter().map(|c| c.trim()).collect();
        let rest = rest.trim();
        let value = if rest.starts_with('>') { cont.join(" ") } else if rest.starts_with('|') { cont.join("\n") } else {
            let q = rest.len() >= 2 && ((rest.starts_with('"') && rest.ends_with('"')) || (rest.starts_with('\'') && rest.ends_with('\'')));
            if q { rest[1..rest.len() - 1].to_string() } else { rest.to_string() }
        };
        out.push(FmEntry { key: k.trim().to_string(), value });
        i = j;
    }
    Some(out)
}

/// D8: a manifest path string that leaves the plugin, tested on the string on every OS. A `.git` segment
/// (any ASCII case) counts as outside: it names a repository's own data, never plugin content.
fn outside(s: &str) -> bool {
    let b = s.as_bytes();
    s.contains('\\') || s.starts_with('/') || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        || s.split('/').any(|seg| seg == ".." || is_dot_git(seg))
        || s.split('/').any(windows_unsafe)
}

/// A path segment `.git` in any ASCII case.
pub(crate) fn is_dot_git(seg: &str) -> bool { seg.eq_ignore_ascii_case(".git") }

/// A path segment Windows cannot use as a plain file name: the part before its first `.` is a reserved device
/// name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`, any ASCII case), or it contains `:`.
pub(crate) fn windows_unsafe(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or("");
    let b = stem.as_bytes();
    seg.contains(':')
        || ["con", "prn", "aux", "nul"].iter().any(|n| stem.eq_ignore_ascii_case(n))
        || (b.len() == 4 && (b[..3].eq_ignore_ascii_case(b"com") || b[..3].eq_ignore_ascii_case(b"lpt")) && matches!(b[3], b'1'..=b'9'))
}

/// The `/`-separated segments of an inside path string, without `.` and empty segments.
fn segments(s: &str) -> impl Iterator<Item = &str> { s.split('/').filter(|x| !x.is_empty() && *x != ".") }

fn read_json(p: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn check_plugin(ctx: &Ctx, prefix: String) -> PluginReport {
    let mut dir = ctx.root.clone();
    for seg in segments(&prefix) { dir.push(seg); }
    let mut p = PluginReport {
        name: String::new(), version: String::new(), license: None, dir, violations: vec![], code: vec![],
        digest: None, copied: vec![], total_bytes: 0,
    };
    if let Some(l) = first_link(ctx, &prefix, PLUGIN_JSON) {
        p.violations.push(viol(Rule::Symlink, &l, "symlinks are refused"));
        return p;
    }
    let pj = match read_json(&p.dir.join(PLUGIN_JSON)) {
        Ok(v) => v,
        Err(e) => {
            p.violations.push(viol(Rule::Malformed, PLUGIN_JSON, e));
            return p;
        }
    };
    p.name = pj.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    p.version = pj.get("version").and_then(Value::as_str).unwrap_or("").to_string();
    if !valid_name(&p.name) { p.violations.push(viol(Rule::BadName, PLUGIN_JSON, format!("plugin name {:?}", p.name))); }
    if semver::Version::parse(&p.version).is_err() {
        p.violations.push(viol(Rule::BadVersion, PLUGIN_JSON, format!("version {:?} is not semver", p.version)));
    }
    if pj.get("description").is_some_and(|d| !description_ok(d)) {
        p.violations.push(viol(Rule::BadDescription, PLUGIN_JSON, "description is not 1-1024 characters"));
    }
    p.license = pj.get("license").and_then(Value::as_str).map(str::to_string);
    let named = named_paths(&pj, &mut p.violations);
    let named_of = |key: &str, default: &str| -> Vec<String> {
        std::iter::once(default.to_string()).chain(named.iter().filter(|(k, _)| *k == key).map(|(_, r)| r.clone())).collect()
    };

    let mut copied = BTreeSet::new();
    let mut links = BTreeSet::new();
    let roots = PARTS.iter().map(|s| s.to_string()).chain(named.iter().map(|(_, r)| r.clone())).filter(|r| !r.is_empty());
    for part in roots {
        let part = part.as_str();
        if !ctx.index.contains_key(&join_rel(&prefix, part)) && !exists_exact(&p.dir, part) { continue; }
        if let Some(l) = first_link(ctx, &prefix, part) {
            links.insert(l);
            continue;
        }
        let walk = walkdir::WalkDir::new(p.dir.join(part)).follow_links(false).into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_str().is_some_and(is_dot_git));
        for e in walk {
            let e = match e {
                Ok(e) => e,
                Err(err) => {
                    p.violations.push(viol(Rule::Malformed, part, err.to_string()));
                    continue;
                }
            };
            let rel = rel_of(&p.dir, e.path());
            if e.file_type().is_symlink() || ctx.is_link(&join_rel(&prefix, &rel)) {
                links.insert(rel);
            } else if e.file_type().is_file() {
                if rel.split('/').any(windows_unsafe) {
                    p.violations.push(viol(Rule::PathOutside, &rel, "path is not usable on Windows"));
                    continue;
                }
                copied.insert(rel);
            }
        }
    }
    for l in links { p.violations.push(viol(Rule::Symlink, &l, "symlinks are refused")); }
    p.copied = copied.into_iter().collect();

    // D1, D9: limits count raw bytes as stored (§15.6 note); the copied files are read normalized.
    let mut files: Vec<(&str, Vec<u8>)> = vec![];
    let copied = p.copied.clone();
    for rel in &copied {
        let abs = p.dir.join(rel);
        let raw = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
        p.total_bytes += raw;
        if raw > FILE_LIMIT {
            p.violations.push(viol(Rule::FileTooLarge, rel, format!("{raw} bytes, limit {FILE_LIMIT}")));
            continue;
        }
        let bytes = match std::fs::read(&abs) {
            Ok(b) => normalize(b),
            Err(e) => {
                p.violations.push(viol(Rule::Malformed, rel, e.to_string()));
                continue;
            }
        };
        files.push((rel, bytes));
    }
    if p.total_bytes > PLUGIN_LIMIT {
        p.violations.push(viol(Rule::PluginTooLarge, ".", format!("{} bytes, limit {PLUGIN_LIMIT}", p.total_bytes)));
    }

    let skill_roots = named_of("skills", "skills");
    let hook_files = named_of("hooks", "hooks/hooks.json");
    let mcp_files = named_of("mcpServers", ".mcp.json");
    let lsp_files = named_of("lspServers", ".lsp.json");
    let under = |roots: &[String], rel: &str| roots.iter().any(|r| rel == r || rel.strip_prefix(r.as_str()).is_some_and(|x| x.starts_with('/')));
    for (rel, bytes) in &files {
        let rel = *rel;
        let text = String::from_utf8_lossy(bytes);
        if is_code_file(ctx, &join_rel(&prefix, rel), &p.dir.join(rel), rel) {
            p.code.push(CodeItem { path: rel.to_string(), kind: CodeKind::File, name: String::new(), data: bytes.clone(), line: None });
        }
        let json_kind = if under(&hook_files, rel) { Some(CodeKind::HookCommand) }
            else if under(&mcp_files, rel) { Some(CodeKind::McpServer) }
            else if under(&lsp_files, rel) { Some(CodeKind::LspServer) } else { None };
        if let Some(kind) = json_kind {
            match serde_json::from_slice::<Value>(bytes) {
                Ok(v) => {
                    let key = if kind == CodeKind::McpServer { "mcpServers" } else { "lspServers" };
                    let v = if kind == CodeKind::HookCommand { &v } else { v.get(key).filter(|m| m.is_object()).unwrap_or(&v) };
                    json_items(&mut p, rel, kind, v);
                }
                Err(e) => p.violations.push(viol(Rule::Malformed, rel, e.to_string())),
            }
        }
        if rel.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case("md")) { markdown_items(&mut p.code, rel, &text); }
        if let Some(skill) = skill_roots.iter().find_map(|root| skill_dir(root, rel)) {
            check_skill(&mut p.violations, rel, skill, &text);
        }
    }
    for (key, kind) in [("hooks", CodeKind::HookCommand), ("mcpServers", CodeKind::McpServer), ("lspServers", CodeKind::LspServer)] {
        if let Some(v) = pj.get(key).filter(|v| v.is_object()) { json_items(&mut p, PLUGIN_JSON, kind, v); }
    }
    // A copied file that a code item names by path is code too: one pass over all items, one over the files it adds.
    let by_path: HashMap<&str, &Vec<u8>> = files.iter().map(|(r, b)| (*r, b)).collect();
    let mut code_files: BTreeSet<String> = p.code.iter().filter(|c| c.kind == CodeKind::File).map(|c| c.path.clone()).collect();
    let mut scan: Vec<CodeItem> = p.code.clone();
    for _ in 0..2 {
        let mut added = vec![];
        for c in &scan {
            let base = if c.kind == CodeKind::File { c.path.rsplit_once('/').map_or("", |(d, _)| d) } else { "" };
            for t in ref_texts(c) {
                for r in named_refs(&t, base) {
                    if let Some(b) = by_path.get(r.as_str()) && code_files.insert(r.clone()) {
                        added.push(CodeItem { path: r, kind: CodeKind::File, name: String::new(), data: (*b).clone(), line: None });
                    }
                }
            }
        }
        p.code.extend(added.iter().cloned());
        scan = added;
    }
    p.code.sort();
    if !p.code.is_empty() { p.digest = Some(code_digest(&p.code)); }
    p
}

/// The `plugin.json` keys whose string values name plugin paths (D9).
const PATH_KEYS: &[&str] = &["hooks", "mcpServers", "lspServers", "commands", "agents", "skills"];

/// D8: each string path in a `PATH_KEYS` field, as `(key, plugin-relative path)`; outside strings are violations.
fn named_paths(pj: &Value, out: &mut Vec<Violation>) -> Vec<(&'static str, String)> {
    let mut named = vec![];
    for key in PATH_KEYS {
        let strs: Vec<&str> = match pj.get(*key) {
            Some(Value::String(s)) => vec![s],
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        for s in strs {
            if outside(s) {
                out.push(viol(Rule::PathOutside, PLUGIN_JSON, format!("{key}: {s} is outside the plugin")));
            } else {
                named.push((*key, segments(s).collect::<Vec<_>>().join("/")));
            }
        }
    }
    named
}

/// Hook commands in a hooks value, or servers in a name → server map (`kind` says which), as code items.
fn json_items(p: &mut PluginReport, rel: &str, kind: CodeKind, v: &Value) {
    let item = |name: &str, data: Vec<u8>| CodeItem { path: rel.to_string(), kind, name: name.to_string(), data, line: None };
    if kind == CodeKind::HookCommand {
        for (name, c) in hook_commands(v) {
            check_root_refs(&mut p.violations, rel, c);
            p.code.push(item(&name, c.as_bytes().to_vec()));
        }
        return;
    }
    let Some(map) = v.as_object() else { return };
    for (name, srv) in map {
        let Some(srv) = srv.as_object() else { continue };
        let g = |k: &str| srv.get(k).cloned().unwrap_or(Value::Null);
        let args: Vec<&str> = srv.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let command = srv.get("command").and_then(Value::as_str);
        for x in command.into_iter().chain(args.iter().copied()) { check_root_refs(&mut p.violations, rel, x); }
        let mut env: Vec<(&String, &Value)> = srv.get("env").and_then(Value::as_object).map(|m| m.iter().collect()).unwrap_or_default();
        env.sort_by_key(|kv| kv.0);
        let env: Vec<Value> = env.into_iter().map(|(k, v)| serde_json::json!([k, v])).collect();
        let mut headers: Vec<&String> = srv.get("headers").and_then(Value::as_object).map(|m| m.keys().collect()).unwrap_or_default();
        headers.sort();
        let data = serde_json::json!([g("type"), g("command"), g("args"), g("cwd"), g("url"), env, headers]);
        p.code.push(item(name, serde_json::to_vec(&data).unwrap_or_default()));
    }
}

/// Inline `` !`…` `` spans (across lines too), ```` ```! ```` blocks indented up to 3 spaces (to a closing fence
/// of at least as many backticks, or end of file), and the whole frontmatter block when it has a `hooks` or
/// `allowed-tools` key (quoted or not) at column 0.
fn markdown_items(code: &mut Vec<CodeItem>, rel: &str, text: &str) {
    static SPAN: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"!`([^`]+)`").unwrap());
    static OPEN: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^ {0,3}(`{3,})!").unwrap());
    static KEY: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r#"^["']?(hooks|allowed-tools)["']?\s*:"#).unwrap());
    let mut item = |kind, data: String, line: usize| {
        code.push(CodeItem { path: rel.to_string(), kind, name: String::new(), data: data.into_bytes(), line: Some(line as u32 + 1) });
    };
    let mut lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if let Some(n) = OPEN.captures(lines[i]).map(|c| c[1].len()) {
            let close = |l: &str| {
                let t = l.trim_start_matches(' ');
                let ticks = t.len() - t.trim_start_matches('`').len();
                l.len() - t.len() <= 3 && ticks >= n && t[ticks..].trim().is_empty()
            };
            let end = (i + 1..lines.len()).find(|&j| close(lines[j])).unwrap_or(lines.len());
            item(CodeKind::ShellBlock, lines[i + 1..end].join("\n"), i);
            // Block lines are not prose: blank them before looking for spans.
            let stop = (end + 1).min(lines.len());
            for l in &mut lines[i..stop] { *l = ""; }
            i = end + 1;
            continue;
        }
        i += 1;
    }
    let prose = lines.join("\n");
    for c in SPAN.captures_iter(&prose) {
        item(CodeKind::InlineShell, c[1].to_string(), prose[..c.get(0).unwrap().start()].matches('\n').count());
    }
    let Some(block) = frontmatter_lines(text) else { return };
    let keys: Vec<(usize, bool)> = block.iter().enumerate().filter_map(|(i, l)| KEY.captures(l).map(|c| (i, &c[1] == "hooks"))).collect();
    if let Some(&(first, _)) = keys.first() {
        let kind = if keys.iter().any(|k| k.1) { CodeKind::FrontmatterHooks } else { CodeKind::FrontmatterAllowedTools };
        item(kind, block.join("\n"), first + 1);
    }
}

/// Every `command` string in an object with `"type": "command"`, at any depth, with its name: the JSON
/// `[event, matcher]` (event = the nearest enclosing key other than `hooks` whose value is an array;
/// matcher = the nearest enclosing `matcher`, else null).
fn hook_commands(v: &Value) -> Vec<(String, &str)> {
    let mut out = vec![];
    fn walk<'a>(v: &'a Value, event: &'a str, matcher: &'a Value, out: &mut Vec<(String, &'a str)>) {
        match v {
            Value::Object(m) => {
                let matcher = m.get("matcher").unwrap_or(matcher);
                if m.get("type").and_then(Value::as_str) == Some("command")
                    && let Some(c) = m.get("command").and_then(Value::as_str) {
                    out.push((serde_json::json!([event, matcher]).to_string(), c));
                }
                for (k, x) in m { walk(x, if x.is_array() && k != "hooks" { k } else { event }, matcher, out); }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, event, matcher, out)),
            _ => {}
        }
    }
    walk(v, "", &Value::Null, &mut out);
    out
}

/// `${CLAUDE_PLUGIN_ROOT}/<path>` or `$CLAUDE_PLUGIN_ROOT/<path>`, quotes allowed around the variable and the
/// path; group 1 is the path up to whitespace, a quote or a shell operator.
static ROOT_REF: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\$(?:\{CLAUDE_PLUGIN_ROOT\}|CLAUDE_PLUGIN_ROOT\b)["']?/["']?([^\s"'`;|&<>()]*)"#).unwrap()
});
/// `./<path>` at the start of a word; group 1 is the path.
static DOT_REF: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?:^|[\s"'=(:;|&])\./([^\s"'`;|&<>()]+)"#).unwrap()
});

/// D8: the path after each plugin-root reference must stay inside.
fn check_root_refs(out: &mut Vec<Violation>, rel: &str, s: &str) {
    for c in ROOT_REF.captures_iter(s) {
        if outside(&c[1]) { out.push(viol(Rule::PathOutside, rel, format!("{} is outside the plugin", &c[0]))); }
    }
}

/// Plugin-relative paths `text` names by a plugin-root reference or `./` (from the plugin root, and from
/// `base`, the referencing file's directory, when it has one).
fn named_refs(text: &str, base: &str) -> Vec<String> {
    let mut out = vec![];
    let mut add = |s: &str| {
        for c in [s, s.trim_end_matches(['.', ',', ':'])] {
            if !outside(c) { out.push(segments(c).collect::<Vec<_>>().join("/")); }
        }
    };
    for c in ROOT_REF.captures_iter(text) { add(&c[1]); }
    for c in DOT_REF.captures_iter(text) {
        add(&c[1]);
        if !base.is_empty() { add(&format!("{base}/{}", &c[1])); }
    }
    out
}

/// The strings of an item that may name a file: a server's command and args, else the item's text.
fn ref_texts(c: &CodeItem) -> Vec<String> {
    if !matches!(c.kind, CodeKind::McpServer | CodeKind::LspServer) { return vec![String::from_utf8_lossy(&c.data).into_owned()]; }
    let v: Value = serde_json::from_slice(&c.data).unwrap_or_default();
    let args: Vec<&str> = v.get(2).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    v.get(1).and_then(Value::as_str).into_iter().chain(args).map(str::to_string).collect()
}

const SCRIPT_EXTS: &[&str] = &["sh", "bash", "zsh", "py", "js", "mjs", "ts", "rb", "pl", "ps1", "bat", "cmd"];

/// D4: `opencode/plugin.js`, a file under `scripts/` or `bin/`, an executable, or a script extension.
fn is_code_file(ctx: &Ctx, key: &str, abs: &Path, rel: &str) -> bool {
    let ext = rel.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e);
    rel == "opencode/plugin.js" || rel.starts_with("scripts/") || rel.starts_with("bin/")
        || ext.is_some_and(|e| SCRIPT_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        || executable(ctx, key, abs)
}

/// D5: the Unix mode bits; elsewhere the git index mode `100755`.
#[cfg(unix)]
fn executable(_ctx: &Ctx, _key: &str, abs: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(abs).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn executable(ctx: &Ctx, key: &str, _abs: &Path) -> bool { ctx.index.get(key).is_some_and(|m| m == "100755") }

/// D3: `SHA-256("zygo-code-digest-v1\n" ‖ f(kind) ‖ f(path) ‖ f(name) ‖ f(data) …)` over the sorted items,
/// `f(x) = u64_le(len x) ‖ x`; lower-case hex.
pub fn code_digest(items: &[CodeItem]) -> String {
    let mut sorted = items.to_vec();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(b"zygo-code-digest-v1\n");
    for i in &sorted {
        for x in [i.kind.as_str().as_bytes(), i.path.as_bytes(), i.name.as_bytes(), &i.data] {
            h.update((x.len() as u64).to_le_bytes());
            h.update(x);
        }
    }
    hex::encode(h.finalize())
}

/// `<skill>` when `rel` is `<root>/<skill>/SKILL.md`.
fn skill_dir<'a>(root: &str, rel: &'a str) -> Option<&'a str> {
    let rest = rel.strip_prefix(root)?.strip_prefix('/')?;
    let (skill, file) = rest.split_once('/')?;
    (file == "SKILL.md").then_some(skill)
}

fn check_skill(out: &mut Vec<Violation>, rel: &str, skill: &str, text: &str) {
    if !valid_name(skill) { out.push(viol(Rule::BadName, rel, format!("skill name {skill:?}"))); }
    let fm = frontmatter(text).unwrap_or_default();
    let get = |k: &str| fm.iter().find(|e| e.key == k).map(|e| e.value.as_str());
    if get("name") != Some(skill) {
        out.push(viol(Rule::NameNotDir, rel, format!("skill name {:?} is not its directory {skill:?}", get("name").unwrap_or(""))));
    }
    if !get("description").is_some_and(|d| (1..=1024).contains(&d.chars().count())) {
        out.push(viol(Rule::BadDescription, rel, "description is not 1-1024 characters"));
    }
}

const FILE_LIMIT: u64 = 1_048_576;
const PLUGIN_LIMIT: u64 = 4_194_304;

/// D1: a text file (no NUL in its first 8000 bytes) is read with each CRLF as LF; any other file raw.
fn normalize(b: Vec<u8>) -> Vec<u8> {
    if b[..b.len().min(8000)].contains(&0) { return b; }
    let mut out = Vec::with_capacity(b.len());
    for (i, &c) in b.iter().enumerate() {
        if !(c == b'\r' && b.get(i + 1) == Some(&b'\n')) { out.push(c); }
    }
    out
}

/// `/`-separated path of `abs` relative to `base`.
fn rel_of(base: &Path, abs: &Path) -> String {
    let rel = abs.strip_prefix(base).unwrap_or(abs);
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}
