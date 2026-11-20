//! act-lite store values (§16.1 A1): location, project id and `.env` parsing, as act-lite does them.

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// act-lite's own minimum length for a value it redacts.
pub const MIN_VALUE_BYTES: usize = 6;

/// One act-lite store value. No `Display`, no serde; `Debug` prints `StoredValue(<redacted>)`.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredValue(String);

impl StoredValue {
    pub fn new(v: impl Into<String>) -> StoredValue {
        StoredValue(v.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for StoredValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoredValue(<redacted>)")
    }
}

/// Reads one environment variable; the real process or a test double.
type EnvFn = Box<dyn Fn(&str) -> Option<OsString>>;

/// Where act-lite looks; `system()` is the real process, tests build their own.
pub struct Lookup {
    pub env: EnvFn,
    /// `dirs::home_dir()`
    pub home: Option<PathBuf>,
    /// `dirs::config_dir()` (Windows Known Folder RoamingAppData)
    pub known_config: Option<PathBuf>,
    /// `cfg!(windows)`
    pub windows: bool,
}

impl Lookup {
    pub fn system() -> Lookup {
        Lookup {
            env: Box::new(|k| std::env::var_os(k)),
            home: dirs::home_dir(),
            known_config: dirs::config_dir(),
            windows: cfg!(windows),
        }
    }

    /// act-lite reads variables with `std::env::var` (Unicode only).
    fn var(&self, k: &str) -> Option<String> {
        (self.env)(k).and_then(|v| v.into_string().ok())
    }
}

pub fn read(repo: &Path) -> Result<Vec<StoredValue>> {
    read_with(repo, &Lookup::system())
}

/// The store's values of `MIN_VALUE_BYTES` or more, first assignments in file order.
/// No error holds a value.
pub fn read_with(repo: &Path, l: &Lookup) -> Result<Vec<StoredValue>> {
    let path = store_path(repo, l);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow!("no act-lite store at {}", path.display()))
        }
        Err(e) => return Err(anyhow!("cannot read act-lite store {}: {}", path.display(), e.kind())),
    };
    match parse(&text) {
        Ok(v) => Ok(v.into_iter().filter(|v| v.len() >= MIN_VALUE_BYTES).map(StoredValue).collect()),
        Err(ParseErr::Line(n)) => Err(anyhow!("act-lite store {} line {n}: cannot parse", path.display())),
        Err(ParseErr::Duplicate { line, key }) => {
            Err(anyhow!("act-lite store {} line {line}: duplicate key {key}", path.display()))
        }
    }
}

pub fn store_path(repo: &Path, l: &Lookup) -> PathBuf {
    config_dir(l).join("secrets").join(format!("{}.env", project_id(&resolve_root(repo))))
}

/// act-lite `act_dir`.
pub fn config_dir(l: &Lookup) -> PathBuf {
    if let Some(dir) = l.var("ACT_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    if l.windows {
        return l.known_config.clone().unwrap_or_else(|| PathBuf::from(".config")).join("act");
    }
    if let Some(dir) = l.var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("act");
    }
    l.home.clone().unwrap_or_else(|| PathBuf::from(".")).join(".config").join("act")
}

/// act-lite `project_id`: first 8 bytes of SHA-256 of the root path, lower-case hex.
pub fn project_id(root: &Path) -> String {
    hex::encode(&Sha256::digest(root.to_string_lossy().as_bytes())[..8])
}

/// act-lite `resolve_root`.
fn resolve_root(start: &Path) -> PathBuf {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    for ancestor in start.ancestors() {
        let dot_git = ancestor.join(".git");
        if dot_git.is_dir() {
            return ancestor.to_path_buf();
        }
        if dot_git.is_file() {
            return worktree_main_root(&dot_git).unwrap_or_else(|| ancestor.to_path_buf());
        }
    }
    start
}

/// act-lite `worktree_main_root`: a `gitdir:` under `<main>/.git/worktrees/<name>` gives `<main>`.
fn worktree_main_root(dot_git_file: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(dot_git_file).ok()?;
    let gitdir = content.lines().next()?.strip_prefix("gitdir:")?.trim();
    let gitdir = Path::new(gitdir);
    let gitdir = if gitdir.is_absolute() { gitdir.to_path_buf() } else { dot_git_file.parent()?.join(gitdir) };
    let gitdir = gitdir.canonicalize().unwrap_or(gitdir);
    let worktrees = gitdir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    let git_dir = worktrees.parent()?;
    if git_dir.file_name()? != ".git" {
        return None;
    }
    git_dir.parent().map(Path::to_path_buf)
}

enum ParseErr {
    Line(usize),
    Duplicate { line: usize, key: String },
}

/// act-lite `parse_line` + `parse_env` + `unquote`: unquoted first-assignment values in file order.
fn parse(text: &str) -> std::result::Result<Vec<String>, ParseErr> {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    let mut out = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let n = idx + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let eq = line.find('=').ok_or(ParseErr::Line(n))?;
        let lhs = line[..eq].trim();
        let key = match lhs.strip_prefix("export") {
            Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim(),
            _ => lhs,
        };
        if key.is_empty() || key.contains(char::is_whitespace) || key.starts_with('#') {
            return Err(ParseErr::Line(n));
        }
        let rest = line[eq + 1..].trim_start();
        let token = match rest.as_bytes().first() {
            Some(&q @ (b'"' | b'\'')) => {
                let close = rest[1..].find(q as char).ok_or(ParseErr::Line(n))? + 2;
                let tail = rest[close..].trim_start();
                if !tail.is_empty() && !tail.starts_with('#') {
                    return Err(ParseErr::Line(n));
                }
                &rest[..close]
            }
            _ => {
                let b = rest.as_bytes();
                match (1..b.len()).find(|&i| b[i] == b'#' && b[i - 1].is_ascii_whitespace()) {
                    Some(i) => rest[..i].trim_end(),
                    None => rest.trim_end(),
                }
            }
        };
        if let Some(prev) = seen.get(key) {
            if *prev != token {
                return Err(ParseErr::Duplicate { line: n, key: key.to_string() });
            }
            continue;
        }
        seen.insert(key, token);
        out.push(unquote(token).to_string());
    }
    Ok(out)
}

fn unquote(token: &str) -> &str {
    for q in ['"', '\''] {
        if token.len() >= 2 && token.starts_with(q) && token.ends_with(q) {
            return &token[1..token.len() - 1];
        }
    }
    token
}
