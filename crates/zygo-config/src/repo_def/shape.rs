//! Typed shapes of `.zygo/` and the allow-list validation (R§6.2-R§6.4, R-C3, R-C7).

use std::collections::BTreeMap;

use toml::{Table, Value};

use super::Files;

/// The parsed, valid workspace definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub workspace: Option<WorkspaceFile>,
    pub profiles: BTreeMap<String, RepoProfile>,
}

/// `.zygo/workspace.toml` (R§6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub version: u32,
    pub layout: Option<String>,
    pub members: BTreeMap<String, MemberDef>,
}

/// One `[members.<name>]` table. `kind` and `job` hold the file's value; the defaults belong to the consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberDef {
    pub kind: Option<String>,
    pub job: Option<String>,
    pub profile: Option<String>,
    pub pool: bool,
    pub cwd: Option<String>,
}

/// `.zygo/profiles/<name>.toml` (R§6.3). `disabled` keeps the order of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoProfile {
    pub overrides: BTreeMap<String, String>,
    pub disabled: Vec<String>,
}

const NOT_ALLOWED: &str = "not allowed in a repository file";
const LAYOUTS: [&str; 5] = ["even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled"];
const LAYOUT_REASON: &str =
    "must be one of even-horizontal, even-vertical, main-horizontal, main-vertical, tiled";
const ROLE_REASON: &str = "a role name must be 1-32 of [a-z0-9-]";
const MEMBERS_MAX: usize = 32;
const ENTRIES_MAX: usize = 16;

/// A violation: the unescaped key path (empty for a whole-file violation) and its reason.
type Violation = (String, &'static str);

/// Parses and validates every file of `files`. The `Err` is the first violation, in R§6.4 order, as
/// `<path>: <key path>: <reason>` with the key path escaped.
pub fn parse(files: &Files) -> Result<Definition, String> {
    let report = |path: &str, (key, reason): Violation| {
        if key.is_empty() {
            format!("{path}: {reason}")
        } else {
            format!("{path}: {}: {reason}", zygo_core::esc(&key))
        }
    };
    let workspace = match &files.workspace {
        Some(bytes) => {
            let path = ".zygo/workspace.toml";
            let t = text_of(path, bytes)?;
            Some(workspace(&t).map_err(|v| report(path, v))?)
        }
        None => None,
    };
    let mut profiles = BTreeMap::new();
    for (name, bytes) in &files.profiles {
        let path = format!(".zygo/profiles/{}.toml", zygo_core::esc(name));
        let t = text_of(&path, bytes)?;
        profiles.insert(name.clone(), profile(&t).map_err(|v| report(&path, v))?);
    }
    Ok(Definition { workspace, profiles })
}

/// The file's bytes as a TOML table; a syntax error keeps only its message and position (R-C7).
fn text_of(path: &str, bytes: &[u8]) -> Result<Table, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8"))?;
    text.parse::<Table>()
        .map_err(|e| format!("{path}: {}", crate::repo_file::syntax_error(text, e.message(), e.span())))
}

fn workspace(t: &Table) -> Result<WorkspaceFile, Violation> {
    if t.get("version").and_then(Value::as_integer) != Some(1) {
        return Err(("version".into(), "must be the integer 1"));
    }
    let mut layout = None;
    let mut members = BTreeMap::new();
    for (key, v) in t {
        match key.as_str() {
            "version" => {}
            "layout" => match v.as_str() {
                Some(s) if LAYOUTS.contains(&s) => layout = Some(s.to_string()),
                _ => return Err((key.clone(), LAYOUT_REASON)),
            },
            "members" => members = members_of(v)?,
            _ => return Err((key.clone(), NOT_ALLOWED)),
        }
    }
    Ok(WorkspaceFile { version: 1, layout, members })
}

fn members_of(v: &Value) -> Result<BTreeMap<String, MemberDef>, Violation> {
    let Some(t) = v.as_table() else { return Err(("members".into(), "must be a table")) };
    if t.len() > MEMBERS_MAX {
        return Err(("members".into(), "at most 32 members"));
    }
    let mut out = BTreeMap::new();
    let mut orchestrator = false;
    for (name, v) in t {
        let at = |key: &str| format!("members.{name}.{key}");
        if !zygo_core::ids::valid_member_name(name) {
            return Err((format!("members.{name}"), "not a valid member name"));
        }
        let Some(m) = v.as_table() else { return Err((format!("members.{name}"), "must be a table")) };
        let mut def = MemberDef { kind: None, job: None, profile: None, pool: false, cwd: None };
        for (key, v) in m {
            match key.as_str() {
                "kind" => {
                    let kind = one_of(v, &["claude-code", "opencode"], "must be claude-code or opencode");
                    def.kind = Some(kind.map_err(|r| (at(key), r))?);
                }
                "job" => {
                    let job = one_of(v, &["orchestrator", "worker"], "must be orchestrator or worker");
                    def.job = Some(job.map_err(|r| (at(key), r))?);
                }
                "profile" => match v.as_str() {
                    Some(s) if crate::profile::valid_name(s) && s != "none" => def.profile = Some(s.to_string()),
                    _ => return Err((at(key), "must be 1-32 of [a-z0-9-] and not none")),
                },
                "pool" => def.pool = v.as_bool().ok_or_else(|| (at(key), "must be true or false"))?,
                "cwd" => def.cwd = Some(cwd_rule(v).map_err(|r| (at(key), r))?),
                _ => return Err((at(key), NOT_ALLOWED)),
            }
        }
        if def.pool && def.kind.as_deref().is_some_and(|k| k != "claude-code") {
            return Err((at("pool"), "only a claude-code member can take a pool directory"));
        }
        if def.job.as_deref() == Some("orchestrator") {
            if orchestrator {
                return Err((at("job"), "at most one member can be the orchestrator"));
            }
            orchestrator = true;
        }
        out.insert(name.clone(), def);
    }
    Ok(out)
}

/// `v` as a string that is one of `allowed`.
fn one_of(v: &Value, allowed: &[&str], reason: &'static str) -> Result<String, &'static str> {
    v.as_str().filter(|s| allowed.contains(s)).map(str::to_string).ok_or(reason)
}

/// R§6.2 `cwd`: printable ASCII; not `~`-led, not absolute; `branch:<1-255 bytes>` or a relative path of 1-255 byte
/// parts, none `.` or `..`, no `\`, at most 1024 bytes.
fn cwd_rule(v: &Value) -> Result<String, &'static str> {
    let s = v.as_str().ok_or("must be a string")?;
    if !s.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err("must be printable ASCII (bytes 0x20-0x7e)");
    }
    if s.starts_with('~') {
        return Err("must not start with ~");
    }
    if s.starts_with('/') || std::path::Path::new(s).is_absolute() {
        return Err("must not be an absolute path");
    }
    if let Some(branch) = s.strip_prefix("branch:") {
        if !(1..=255).contains(&branch.len()) {
            return Err("a branch name must be 1-255 bytes");
        }
        return Ok(s.to_string());
    }
    if s.len() > 1024 {
        return Err("must be at most 1024 bytes");
    }
    if s.contains('\\') {
        return Err("must not hold \\");
    }
    for part in s.split('/') {
        if !(1..=255).contains(&part.len()) {
            return Err("each part must be 1-255 bytes");
        }
        if part == "." || part == ".." {
            return Err("must not hold a . or .. part");
        }
    }
    Ok(s.to_string())
}

fn profile(t: &Table) -> Result<RepoProfile, Violation> {
    let mut p = RepoProfile { overrides: BTreeMap::new(), disabled: Vec::new() };
    for (key, v) in t {
        match key.as_str() {
            "disabled" => p.disabled = disabled_of(v)?,
            "overrides" => p.overrides = overrides_of(v)?,
            _ => return Err((key.clone(), NOT_ALLOWED)),
        }
    }
    if p.overrides.is_empty() && p.disabled.is_empty() {
        return Err((String::new(), "a profile must set overrides or disabled"));
    }
    Ok(p)
}

fn overrides_of(v: &Value) -> Result<BTreeMap<String, String>, Violation> {
    let Some(t) = v.as_table() else { return Err(("overrides".into(), "must be a table")) };
    if t.len() > ENTRIES_MAX {
        return Err(("overrides".into(), "at most 16 entries"));
    }
    let mut out = BTreeMap::new();
    for (role, v) in t {
        let at = format!("overrides.{role}");
        if !crate::profile::valid_name(role) {
            return Err((at, ROLE_REASON));
        }
        match v.as_str() {
            Some(m) if (1..=128).contains(&m.len()) && m.bytes().all(|b| (0x20..=0x7e).contains(&b)) => {
                out.insert(role.clone(), m.to_string());
            }
            _ => return Err((at, "must be 1-128 bytes of printable ASCII")),
        }
    }
    Ok(out)
}

fn disabled_of(v: &Value) -> Result<Vec<String>, Violation> {
    let Some(a) = v.as_array() else { return Err(("disabled".into(), "must be an array")) };
    if a.len() > ENTRIES_MAX {
        return Err(("disabled".into(), "at most 16 entries"));
    }
    a.iter()
        .enumerate()
        .map(|(i, v)| match v.as_str() {
            Some(s) if crate::profile::valid_name(s) => Ok(s.to_string()),
            _ => Err((format!("disabled[{i}]"), ROLE_REASON)),
        })
        .collect()
}
