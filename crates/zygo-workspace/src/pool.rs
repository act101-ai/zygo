//! Pool directories (R§9.5, R-C6): `~` expansion, the directory listing, the assignment of directories to panes and the shared lines.
//! Names and types only, no file opened.
use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

const NOT_ABSOLUTE: &str = "must be absolute after ~ expansion";

/// The home directory of the `~` rule: `HOME` (`USERPROFILE` on Windows) when it is absolute, else `std::env::home_dir`.
pub fn home() -> Option<PathBuf> { home_in(|k| std::env::var_os(k)) }

/// `home` with the variables read through `var` (`zygo claude` reads them from the environment it builds for the agent).
pub fn home_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|h| h.is_absolute()).or_else(std::env::home_dir)
}

/// The one `~` rule, shared with `zygo claude --config-dir`: `None` when `p` is neither `~` nor starts with `~/`; `home` for `~` and
/// `~/`; `home/<rest>` for `~/<rest>`, with every leading separator of `rest` removed; `Err` when there is no home directory or `rest`
/// still has a root or a prefix (`~/C:\x` on Windows).
pub fn tilde(p: &str, home: Option<&Path>) -> Option<Result<PathBuf, String>> {
    let rest = if p == "~" { "" } else { p.strip_prefix("~/")? };
    let rest = rest.trim_start_matches(|c| c == '/' || (cfg!(windows) && c == '\\'));
    let Some(h) = home else { return Some(Err("no home directory".into())) };
    if matches!(Path::new(rest).components().next(), Some(Component::Prefix(_) | Component::RootDir)) {
        return Some(Err(NOT_ABSOLUTE.into()));
    }
    Some(Ok(if rest.is_empty() { h.to_path_buf() } else { h.join(rest) }))
}

/// The pool directory of `pool`: `tilde`, else the text as a path; it must be absolute. Callers pass `home().as_deref()`, so `~` is
/// expanded as `--config-dir` does (R§9.5).
pub fn expand(pool: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let p = tilde(pool, home).unwrap_or_else(|| Ok(PathBuf::from(pool)))?;
    if !p.is_absolute() {
        return Err(NOT_ABSOLUTE.into());
    }
    Ok(p)
}

fn unreadable(e: io::Error) -> String { format!("not readable ({:?})", e.kind()) }

/// A link whose target is missing or that loops: skipped.
fn broken(e: &io::Error) -> bool {
    #[cfg(unix)]
    let looped = e.raw_os_error() == Some(libc::ELOOP);
    #[cfg(windows)]
    let looped = e.raw_os_error() == Some(1921); // ERROR_CANT_RESOLVE_FILENAME
    #[cfg(not(any(unix, windows)))]
    let looped = false;
    e.kind() == ErrorKind::NotFound || looped
}

/// The subdirectories of `dir` whose names do not start with `.` (links followed), in byte order of name. The `Err` is the reason of
/// `zygo: pool <dir>: <reason>; nothing started`.
pub fn list(dir: &Path) -> Result<Vec<PathBuf>, String> {
    match fs::metadata(dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err("not a directory".into()),
        Err(e) if e.kind() == ErrorKind::NotFound => return Err("not a directory".into()),
        Err(e) => return Err(unreadable(e)),
    }
    let mut out = Vec::new();
    for e in fs::read_dir(dir).map_err(unreadable)? {
        let e = e.map_err(unreadable)?;
        let name = e.file_name();
        if name.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        // The entry's type first (no handle on Windows); a directory is checked with the entry's own data (a stat on unix, which fails
        // when `dir` cannot be searched), and only a link is followed.
        let t = e.file_type().map_err(unreadable)?;
        let is_dir = if t.is_symlink() {
            match fs::metadata(e.path()) {
                Ok(m) => m.is_dir(),
                Err(err) if broken(&err) => false,
                Err(err) => return Err(unreadable(err)),
            }
        } else {
            t.is_dir() && e.metadata().map_err(unreadable)?.is_dir()
        };
        if is_dir {
            out.push((name, e.path()));
        }
    }
    if out.is_empty() {
        return Err("no subdirectory".into());
    }
    out.sort_by(|a, b| a.0.as_encoded_bytes().cmp(b.0.as_encoded_bytes()));
    Ok(out.into_iter().map(|(_, p)| p).collect())
}

/// Pane indices: named panes in byte order of name, then unnamed panes in pane order.
pub fn order(names: &[Option<String>]) -> Vec<usize> {
    let mut named: Vec<usize> = (0..names.len()).filter(|&i| names[i].is_some()).collect();
    named.sort_by(|&a, &b| names[a].cmp(&names[b]));
    named.extend((0..names.len()).filter(|&i| names[i].is_none()));
    named
}

/// The directory index of each pane: the i-th pane of `order` gets `i mod k`. `k >= 1`.
pub fn assign(names: &[Option<String>], k: usize) -> Vec<usize> {
    let mut out = vec![0; names.len()];
    for (i, p) in order(names).into_iter().enumerate() {
        out[p] = i % k;
    }
    out
}

/// One `pool: <dir> is shared by <name>, <name>…` line per directory that two or more members share, in order of first appearance.
/// Directories are the same when their canonical paths are (two links to one directory); `<dir>` is the first path as given.
pub fn shared_lines(dir_of: &[(String, PathBuf)]) -> Vec<String> {
    let mut groups: Vec<(PathBuf, &Path, Vec<&str>)> = Vec::new();
    for (name, dir) in dir_of {
        let key = fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        match groups.iter_mut().find(|(k, ..)| *k == key) {
            Some((.., n)) => n.push(name),
            None => groups.push((key, dir, vec![name])),
        }
    }
    groups
        .into_iter()
        .filter(|(.., n)| n.len() >= 2)
        .map(|(_, d, n)| {
            let n: Vec<String> = n.into_iter().map(zygo_core::esc).collect();
            format!("pool: {} is shared by {}", zygo_core::esc(&d.to_string_lossy()), n.join(", "))
        })
        .collect()
}
