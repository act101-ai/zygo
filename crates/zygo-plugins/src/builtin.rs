//! The built-in act101 marketplace: the pre-approved code digests compiled into the binary at the pinned
//! commit (spec §15.7, §15.9), and the writers the pin script runs (`scripts/pin-builtin-marketplace.sh`).

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use serde_json::Value;

use crate::approve::PreApproval;
use crate::check::{check_dir, check_dir_modes, CheckReport, PluginReport};

const MARKETPLACE_JSON: &str = ".claude-plugin/marketplace.json";
const PERSON_KEYS: [&str; 5] = ["owner", "author", "authors", "maintainers", "contributors"];
const NEUTRAL: &str = "act101";
const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}";
/// The JSON keys of the kept files that are schema, not data; the name scan skips them and scans every other key.
const SCHEMA_KEYS: [&str; 22] = [
    "$schema", "args", "author", "authors", "category", "command", "contributors", "description", "email", "homepage",
    "keywords", "license", "maintainers", "mcpServers", "name", "owner", "plugins", "repository", "skills", "source",
    "url", "version",
];

#[derive(Deserialize)]
struct File { #[serde(default)] approval: Vec<Row> }
#[derive(Deserialize)]
struct Row { marketplace: String, plugin: String, digest: String }

/// The pre-approvals compiled into the binary, from `data/act101-approved.toml`.
pub fn approved() -> Vec<PreApproval> {
    approved_from(include_str!("../data/act101-approved.toml")).expect("built-in approvals are valid TOML")
}

/// The `[[approval]]` rows of `text` in file order; the `ref` and `commit` keys are ignored.
pub fn approved_from(text: &str) -> anyhow::Result<Vec<PreApproval>> {
    let f: File = toml::from_str(text)?;
    Ok(f.approval.into_iter().map(|r| PreApproval { marketplace: r.marketplace, plugin: r.plugin, digest: r.digest }).collect())
}

fn q(s: &str) -> String { toml::Value::String(s.into()).to_string() }

/// The checked plugin `name` of `r`; an error when it is missing or has a violation.
fn checked<'a>(r: &'a CheckReport, name: &str) -> anyhow::Result<&'a PluginReport> {
    let p = r.plugins.iter().find(|p| p.name == name).ok_or_else(|| anyhow!("{name} is not in the marketplace"))?;
    if let Some(v) = p.violations.first() { bail!("{name}: check failed: {} {}", v.rule.as_str(), v.path); }
    Ok(p)
}

/// `check_dir` with the exec bits read from disk, so no git runs.
fn check_on_disk(root: &Path) -> CheckReport {
    check_dir_modes(root, Instant::now() + Duration::from_secs(2), Some(disk_modes(root)))
}

/// The text of `data/act101-approved.toml` for the marketplace checkout at `root`: the header, then one
/// block per name of `plugins` whose digest is `Some`, in argument order, marketplace `act101`.
pub fn render_approved(root: &Path, git_ref: &str, commit: &str, plugins: &[String]) -> anyhow::Result<String> {
    let r = check_on_disk(root);
    let mut out = format!("# Written by scripts/pin-builtin-marketplace.sh. Do not edit.\nref = {}\ncommit = {}\n", q(git_ref), q(commit));
    for name in plugins {
        let Some(d) = &checked(&r, name)?.digest else { continue };
        out.push_str(&format!("\n[[approval]]\nmarketplace = \"act101\"\nplugin = {}\ndigest = {}\n", q(name), q(d)));
    }
    Ok(out)
}

/// D2: writes the trimmed, neutralized copy of the checkout at `clone` to `tree` (which must not exist),
/// keeping `.claude-plugin/marketplace.json` and the directories of `plugins`; stops when a digest changes.
pub fn write_fixture(clone: &Path, tree: &Path, plugins: &[String]) -> anyhow::Result<()> {
    if tree.symlink_metadata().is_ok() { bail!("{}: exists", tree.display()); }
    let r = write_fixture_in(clone, tree, plugins);
    if r.is_err() { let _ = std::fs::remove_dir_all(tree); }
    r
}

fn write_fixture_in(clone: &Path, tree: &Path, plugins: &[String]) -> anyhow::Result<()> {
    let src = check_dir(clone);
    let mut digests = Vec::new();
    for name in plugins { digests.push(checked(&src, name)?.digest.clone()); }
    let mut ids = history_ids(clone)?;

    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(clone.join(MARKETPLACE_JSON))?)
        .map_err(|_| anyhow!("{MARKETPLACE_JSON}: not JSON"))?;
    let mut dirs = Vec::new();
    for name in plugins {
        let entry = m["plugins"].as_array().into_iter().flatten().find(|e| e["name"].as_str() == Some(name.as_str()));
        let dir = entry.and_then(|e| e["source"].as_str()).and_then(|s| s.strip_prefix("./"))
            .ok_or_else(|| anyhow!("{name}: source is not a relative path"))?;
        dirs.push(dir.trim_end_matches('/').to_string());
    }
    if let Some(list) = m.get_mut("plugins").and_then(Value::as_array_mut) {
        list.retain(|e| e["name"].as_str().is_some_and(|n| plugins.iter().any(|p| p == n)));
    }
    neutralize(&mut m, &mut ids);
    let mp = tree.join(MARKETPLACE_JSON);
    std::fs::create_dir_all(mp.parent().unwrap())?;
    std::fs::write(&mp, serde_json::to_string_pretty(&m)? + "\n")?;
    std::fs::set_permissions(&mp, std::fs::metadata(clone.join(MARKETPLACE_JSON))?.permissions())?;

    for dir in &dirs {
        for e in walkdir::WalkDir::new(clone.join(dir)).follow_links(false).sort_by_file_name() {
            let e = e?;
            let rel = rel_of(clone, e.path());
            if rel.split('/').any(|s| s == ".git") { continue; }
            if e.path_is_symlink() { bail!("{rel}: symlink"); }
            let dst = tree.join(&rel);
            if e.file_type().is_dir() { std::fs::create_dir_all(&dst)?; continue; }
            if rel.ends_with(".json") {
                let mut v: Value = serde_json::from_slice(&std::fs::read(e.path())?).map_err(|_| anyhow!("{rel}: not JSON"))?;
                if neutralize(&mut v, &mut ids) {
                    std::fs::write(&dst, serde_json::to_string_pretty(&v)? + "\n")?;
                    std::fs::set_permissions(&dst, e.metadata()?.permissions())?;
                    continue;
                }
            }
            std::fs::copy(e.path(), &dst).with_context(|| rel.clone())?;
        }
    }

    if let Some(hit) = name_hit(tree, &ids) { bail!("{hit}"); }

    let out = check_on_disk(tree);
    for (name, d) in plugins.iter().zip(&digests) {
        let same = out.plugins.iter().find(|p| &p.name == name).is_some_and(|p| p.ok() && &p.digest == d);
        if !same { bail!("{name}: digest changed by the fixture rewrite"); }
    }
    Ok(())
}

/// `path` relative to `root`, `/`-separated.
fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// The replacement rule of D2: every person value under a person key becomes `act101`; the replaced names,
/// e-mails and person strings are pushed to `seen`. True when a value changed.
fn neutralize(v: &mut Value, seen: &mut Vec<String>) -> bool {
    match v {
        Value::Object(o) => {
            let mut changed = false;
            for (k, val) in o.iter_mut() {
                changed |= if PERSON_KEYS.contains(&k.as_str()) { person(val, seen) } else { neutralize(val, seen) };
            }
            changed
        }
        Value::Array(a) => a.iter_mut().fold(false, |c, x| neutralize(x, seen) | c),
        _ => false,
    }
}

fn person(v: &mut Value, seen: &mut Vec<String>) -> bool {
    match v {
        Value::Array(a) => a.iter_mut().fold(false, |c, x| person(x, seen) | c),
        Value::String(s) => {
            seen.push(s.clone());
            replace(v)
        }
        Value::Object(o) => {
            let mut changed = false;
            for k in ["name", "url", "email"] {
                let Some(x) = o.get_mut(k) else { continue };
                if k != "url" && let Some(s) = x.as_str() { seen.push(s.to_string()); }
                changed |= replace(x);
            }
            changed
        }
        _ => false,
    }
}

fn replace(v: &mut Value) -> bool {
    let changed = v.as_str() != Some(NEUTRAL);
    *v = Value::String(NEUTRAL.into());
    changed
}

/// D2: on unix, each regular file under `root` (links not followed, `.git` skipped) → `100755` or `100644`
/// from its exec bits; elsewhere empty.
fn disk_modes(root: &Path) -> HashMap<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        walkdir::WalkDir::new(root).follow_links(false).into_iter()
            .filter_entry(|e| e.file_name() != ".git").flatten()
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let mode = e.metadata().ok()?.permissions().mode();
                Some((rel_of(root, e.path()), if mode & 0o111 != 0 { "100755" } else { "100644" }.to_string()))
            })
            .collect()
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        HashMap::new()
    }
}

/// The author and committer names and e-mails of the history of `clone`, without the service accounts:
/// GitHub's web-merge committer (`noreply@github.com`) and any name that ends with `-bot` or `[bot]`.
fn history_ids(clone: &Path) -> anyhow::Result<Vec<String>> {
    let out = crate::sync::git_command(clone).args(["log", "--format=%an%n%ae%n%cn%n%ce"]).output()
        .map_err(|_| anyhow!("git log failed"))?;
    if !out.status.success() { bail!("git log failed"); }
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let mut ids = BTreeSet::new();
    for pair in lines.chunks(2) {
        if let [name, email] = pair {
            let n = name.to_lowercase();
            if email.eq_ignore_ascii_case("noreply@github.com") || n.ends_with("-bot") || n.ends_with("[bot]") { continue; }
            ids.insert(name.to_string());
            ids.insert(email.to_string());
        }
    }
    Ok(ids.into_iter().collect())
}

/// Split at each lower-to-upper case change, Unicode lower case, each run of whitespace made one space, trimmed.
fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut lower = false;
    for c in s.chars() {
        if lower && c.is_uppercase() { out.push(' '); }
        lower = c.is_lowercase();
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// True when `w` occurs in `text` with no letter or digit next to it.
fn has_word(text: &str, w: &str) -> bool {
    text.match_indices(w).any(|(i, _)| {
        let edge = |c: Option<char>| !c.is_some_and(char::is_alphanumeric);
        edge(text[..i].chars().next_back()) && edge(text[i + w.len()..].chars().next())
    })
}

/// The name-scan refusal for the first file under `tree` (path order), or `None`. A string hits when it holds
/// an e-mail address, one of `ids` (both sides through `norm`), or a word at word boundaries: a part of 3 or
/// more letters of a name in `ids` split at whitespace and hyphens, or the `<handle>` of a
/// `<digits>+<handle>@users.noreply.github.com` e-mail. Strings and words under 3 characters and `act101` are
/// skipped. Scanned per file: each directory and file name of its path (the refusal then names only the
/// directory above it), then the keys outside `SCHEMA_KEYS` and the string values of a `.json` file, or the
/// whole text (read lossily) of any other file.
fn name_hit(tree: &Path, ids: &[String]) -> Option<String> {
    let email = regex::Regex::new(EMAIL).unwrap();
    let handle = regex::Regex::new(r"^(?:[0-9]+\+)?([^@]+)@users\.noreply\.github\.com$").unwrap();
    let keep = |s: &String| s.chars().count() >= 3 && s != NEUTRAL;
    let ids: BTreeSet<String> = ids.iter().map(|s| norm(s)).filter(keep).collect();
    let mut words: BTreeSet<String> = ids.iter().filter(|s| !s.contains('@')).flat_map(|s| s.split([' ', '-']))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3 && keep(w)).collect();
    words.extend(ids.iter().filter_map(|s| handle.captures(s)).map(|c| c[1].to_string()).filter(keep));
    let hit = |s: &str| {
        let s = norm(s);
        email.is_match(&s) || ids.iter().any(|id| s.contains(id.as_str())) || words.iter().any(|w| has_word(&s, w))
    };
    let files: BTreeSet<String> = walkdir::WalkDir::new(tree).follow_links(false).into_iter().flatten()
        .filter(|e| e.file_type().is_file()).map(|e| rel_of(tree, e.path())).collect();
    files.into_iter().find_map(|rel| {
        let parts: Vec<&str> = rel.split('/').collect();
        if let Some(i) = parts.iter().position(|p| hit(p)) {
            let at = if i == 0 { ".".to_string() } else { parts[..i].join("/") };
            return Some(format!("{at}: a file or directory name holds a name or e-mail of the source checkout"));
        }
        let text = String::from_utf8_lossy(&std::fs::read(tree.join(&rel)).unwrap_or_default()).into_owned();
        let values = match rel.ends_with(".json").then(|| serde_json::from_str::<Value>(&text)) {
            Some(Ok(v)) => { let mut out = Vec::new(); strings(&v, &mut out); out }
            _ => vec![text.clone()],
        };
        let found = email.is_match(&text) || values.iter().any(|s| hit(s));
        found.then(|| format!("{rel}: holds a name or e-mail of the source checkout"))
    })
}

/// The string values and the non-schema keys of `v`.
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| {
            if !SCHEMA_KEYS.contains(&k.as_str()) { out.push(k.clone()); }
            strings(x, out);
        }),
        _ => {}
    }
}
