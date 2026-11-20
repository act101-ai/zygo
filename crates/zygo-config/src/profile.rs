//! Model profiles (§11): shape, selection, validation, default-profile writer.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::check::{Severity, Violation};
use crate::layers::{ConfigError, Merged};
use crate::schema::{CatalogView, CustomRole, Dialect, Role};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub overrides: BTreeMap<String, String>,
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub plugins: BTreeMap<String, bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<toml::Table>,
    pub roles: BTreeMap<String, CustomRole>,
}

/// The profile name asked for: the flag, else `user.default_profile`; `none` means no profile.
pub fn requested(cfg: &Merged, flag: Option<&str>) -> Option<String> {
    let name = flag.or(cfg.config.user.default_profile.as_deref())?;
    (name != "none").then(|| name.to_string())
}

/// The requested profile when it is defined; no validation.
pub fn select(cfg: &Merged, flag: Option<&str>) -> Option<Profile> {
    let name = requested(cfg, flag)?;
    let c = cfg.config.profile.get(&name)?;
    Some(Profile {
        name,
        overrides: c.overrides.clone(),
        disabled: c.disabled.clone(),
        prompt: c.prompt.clone(),
        plugins: c.plugins.clone(),
        features: c.features.clone(),
        roles: c.roles.clone(),
    })
}

/// 1-32 characters of `[a-z0-9-]`.
pub fn valid_name(s: &str) -> bool {
    (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `{:?}`-style escaping without quotes: an untrusted string in a message stays one line with no control characters.
fn esc(s: &str) -> String {
    s.escape_debug().to_string()
}

fn viol(path: String, message: impl Into<String>) -> Violation {
    Violation { severity: Severity::Error, path, message: message.into() }
}

/// §11.3. The first violation wins; the order of checks is fixed.
pub fn validate(
    p: &Profile,
    roles: &BTreeMap<String, Role>,
    main_role: &str,
    catalog: &CatalogView,
    dialect: Dialect,
    known_plugins: &BTreeSet<String>,
) -> Result<(), Violation> {
    let base = format!("profile.{}", esc(&p.name));
    if !valid_name(&p.name) {
        return Err(viol(base, "name must be 1-32 characters of a-z, 0-9 and -"));
    }
    if p.name == "none" {
        return Err(viol(base, "name is reserved; none disables the profile"));
    }
    let any = !p.overrides.is_empty()
        || !p.disabled.is_empty()
        || !p.plugins.is_empty()
        || !p.roles.is_empty()
        || p.prompt.as_deref().is_some_and(|s| !s.trim().is_empty())
        || p.features.as_ref().is_some_and(|f| !f.is_empty());
    if !any {
        return Err(viol(base, "sets no part"));
    }
    if let Some(t) = &p.prompt {
        text_ok(&format!("{base}.prompt"), t, 2048, Text::Free)?;
    }
    for (key, id) in &p.overrides {
        let path = format!("{base}.overrides.{}", esc(key));
        if !roles.contains_key(key) {
            return Err(viol(path, "not a bundle role"));
        }
        model_ok(path, id, catalog, dialect)?;
    }
    if p.roles.len() > 8 {
        return Err(viol(format!("{base}.roles"), format!("{} custom roles; the limit is 8", p.roles.len())));
    }
    for (name, r) in &p.roles {
        let rp = format!("{base}.roles.{}", esc(name));
        if !valid_name(name) {
            return Err(viol(rp, "name must be 1-32 characters of a-z, 0-9 and -"));
        }
        if roles.contains_key(name) {
            return Err(viol(rp, "name is a bundle role"));
        }
        if catalog.models.contains_key(name) {
            return Err(viol(rp, "name is a catalog model id"));
        }
        if RESERVED.contains(&name.as_str()) {
            return Err(viol(rp, "name is reserved"));
        }
        text_ok(&format!("{rp}.description"), &r.description, 256, Text::OneLine)?;
        text_ok(&format!("{rp}.prompt"), &r.prompt, 2048, Text::NoBraces)?;
        model_ok(format!("{rp}.model"), &r.model, catalog, dialect)?;
    }
    let mut seen = BTreeSet::new();
    for (i, r) in p.disabled.iter().enumerate() {
        let path = format!("{base}.disabled[{i}]");
        if !roles.contains_key(r) {
            return Err(viol(path, "not a bundle role"));
        }
        if !seen.insert(r) {
            return Err(viol(path, "listed twice"));
        }
        if r == main_role {
            return Err(viol(path, "is the main role and cannot be disabled"));
        }
    }
    if p.plugins.len() > 32 {
        return Err(viol(format!("{base}.plugins"), format!("{} plugins; the limit is 32", p.plugins.len())));
    }
    for key in p.plugins.keys() {
        let path = format!("{base}.plugins.{}", esc(key));
        let form = key.split(':').count() == 2
            && key.split(':').all(valid_part);
        if !form {
            return Err(viol(path, "not of the form <marketplace>:<plugin>"));
        }
        if !known_plugins.contains(key) {
            return Err(viol(path, "not in a configured marketplace"));
        }
    }
    if let Some(f) = &p.features {
        features_ok(&base, f)?;
    }
    Ok(())
}

/// The §15.6 name rule: 1-64 of `[a-z0-9-]`, no leading, trailing or double `-`.
fn valid_part(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

fn features_ok(base: &str, f: &toml::Table) -> Result<(), Violation> {
    // Same text as `Merged::apply_profile` (layers.rs).
    if f.get("redaction").and_then(toml::Value::as_table).is_some_and(|r| r.contains_key("secrets")) {
        return Err(viol(format!("{base}.features.redaction.secrets"), "features: redaction.secrets cannot be set by a profile"));
    }
    let flag = |path: String, v: &toml::Value| {
        if v.is_bool() { Ok(()) } else { Err(viol(path, "must be true or false")) }
    };
    for (k, v) in f {
        let path = format!("{base}.features.{}", esc(k));
        match k.as_str() {
            "results" | "work_units" | "compact_tool" => flag(path, v)?,
            "redaction" => {
                let Some(t) = v.as_table() else {
                    return Err(viol(path, "must be a table"));
                };
                for (rk, rv) in t {
                    let rpath = format!("{path}.{}", esc(rk));
                    if rk != "content" {
                        return Err(viol(rpath, "not a feature key"));
                    }
                    flag(rpath, rv)?;
                }
            }
            _ => return Err(viol(path, "not a feature key")),
        }
    }
    Ok(())
}

const RESERVED: [&str; 8] = ["direct", "subscription", "fable", "best", "opus", "opusplan", "sonnet", "haiku"];

fn dialect_name(d: Dialect) -> &'static str {
    match d {
        Dialect::Anthropic => "anthropic",
        Dialect::Openai => "openai",
        Dialect::OpenaiChat => "openai-chat",
    }
}

fn model_ok(path: String, id: &str, catalog: &CatalogView, dialect: Dialect) -> Result<(), Violation> {
    let Some(m) = catalog.models.get(id) else {
        return Err(viol(path, "model is not in the catalog"));
    };
    if !m.dialects.contains(&dialect) {
        return Err(viol(path, format!("model cannot be served on the {} dialect", dialect_name(dialect))));
    }
    Ok(())
}

#[derive(PartialEq)]
enum Text {
    /// The profile prompt: control and size checks only.
    Free,
    /// A role prompt: also non-empty (not only whitespace), no braces.
    NoBraces,
    /// A role description: also one line.
    OneLine,
}

fn text_ok(path: &str, s: &str, limit: usize, kind: Text) -> Result<(), Violation> {
    if kind != Text::Free && s.trim().is_empty() {
        return Err(viol(path.into(), "must not be empty"));
    }
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(viol(path.into(), "contains a control character"));
    }
    if s.len() > limit {
        return Err(viol(path.into(), format!("{} bytes; the limit is {limit}", s.len())));
    }
    if kind == Text::OneLine && s.contains(['\n', '\u{2028}', '\u{2029}']) {
        return Err(viol(path.into(), "must be one line"));
    }
    if kind != Text::Free && s.contains(['{', '}']) {
        return Err(viol(path.into(), "must not contain { or }"));
    }
    Ok(())
}

/// The file a config path names: a symlink (even a dangling one) resolves to its target,
/// a relative target against the link's directory.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    let mut p = path.to_path_buf();
    for _ in 0..40 {
        match std::fs::read_link(&p) {
            Ok(t) if t.is_absolute() => p = t,
            Ok(t) => p = p.parent().unwrap_or_else(|| Path::new("")).join(t),
            Err(_) => break,
        }
    }
    p
}

/// Sets `[user] default_profile` in the user config file, keeping every other byte (§11.2).
/// A symlinked file stays a symlink; its target is rewritten atomically with the target's mode.
/// The read-modify-write holds an exclusive lock on `<file>.lock` beside the target.
pub fn set_default_profile(path: &Path, name: &str) -> Result<(), ConfigError> {
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| ConfigError::Io { path: p, source }
    };
    let target = resolve(path);
    let parse = |message: String| ConfigError::Parse { layer: "user", path: target.display().to_string(), message };
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    let file_name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lock_path = dir.join(format!("{file_name}.lock"));
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(io(&lock_path))?;
    fs4::FileExt::lock(&lock).map_err(io(&lock_path))?;

    let text = match std::fs::read_to_string(&target) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io(&target)(e)),
    };
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| parse(crate::repo_file::syntax_error(&text, e.message(), e.span())))?;
    if doc.get("user").is_none() {
        doc["user"] = toml_edit::table();
    }
    let mut v = toml_edit::value(name);
    if let (Some(t), Some(new)) = (doc["user"].as_inline_table_mut(), v.as_value_mut()) {
        // A new last key in `{ … }` takes the old last value's suffix (the space before `}`).
        let fresh = !t.contains_key("default_profile");
        if let (true, Some((_, last))) = (fresh, t.iter_mut().last()) {
            let suffix = last.decor().suffix().cloned();
            last.decor_mut().set_suffix("");
            new.decor_mut().set_prefix(" ");
            if let Some(sfx) = suffix {
                new.decor_mut().set_suffix(sfx);
            }
        }
    }
    let user = doc["user"].as_table_like_mut().ok_or_else(|| parse("user: not a table".into()))?;
    if let (Some(old), Some(new)) = (user.get("default_profile").and_then(toml_edit::Item::as_value), v.as_value_mut()) {
        *new.decor_mut() = old.decor().clone();
    }
    user.insert("default_profile", v);

    let tmp = dir.join(format!(".{file_name}.{}.tmp", ulid::Ulid::generate()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(doc.to_string().as_bytes())?;
        f.sync_all()?;
        if let Ok(m) = std::fs::metadata(&target) {
            std::fs::set_permissions(&tmp, m.permissions())?;
        }
        std::fs::rename(&tmp, &target)?;
        #[cfg(unix)]
        std::fs::File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(&target)(e));
    }
    drop(lock);
    Ok(())
}
