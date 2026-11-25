//! Pool directories (R§9.5, R-C6): `~` expansion, the directory listing, the assignment of directories to panes and the shared lines.
//! Names and types only, no file opened.
use std::path::{Path, PathBuf};

/// The one `~` rule, shared with `zygo claude --config-dir`: `None` when `p` is neither `~` nor starts with `~/`; `home` for `~` and
/// `~/`; `home/<rest>` for `~/<rest>`; `Err` when there is no home directory.
pub fn tilde(p: &str, home: Option<&Path>) -> Option<Result<PathBuf, String>> {
    let rest = if p == "~" { "" } else { p.strip_prefix("~/")? };
    Some(match home {
        Some(h) if rest.is_empty() => Ok(h.to_path_buf()),
        Some(h) => Ok(h.join(rest)),
        None => Err("no home directory".into()),
    })
}

/// The pool directory of `pool`: `tilde`, else the text as a path; it must be absolute.
pub fn expand(pool: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let p = tilde(pool, home).unwrap_or_else(|| Ok(PathBuf::from(pool)))?;
    if !p.is_absolute() {
        return Err("must be absolute after ~ expansion".into());
    }
    Ok(p)
}

/// The subdirectories of `dir` whose names do not start with `.` (links followed), in byte order of name. The `Err` is the reason of
/// `zygo: pool <dir>: <reason>; nothing started`.
pub fn list(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Err("not a directory".into());
    }
    let unreadable = |e: std::io::Error| format!("not readable ({:?})", e.kind());
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(unreadable)? {
        let e = e.map_err(unreadable)?;
        let name = e.file_name();
        if !name.as_encoded_bytes().starts_with(b".") && e.path().is_dir() {
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
pub fn shared_lines(dir_of: &[(String, PathBuf)]) -> Vec<String> {
    let mut groups: Vec<(&Path, Vec<&str>)> = Vec::new();
    for (name, dir) in dir_of {
        match groups.iter_mut().find(|(d, _)| *d == dir.as_path()) {
            Some((_, n)) => n.push(name),
            None => groups.push((dir, vec![name])),
        }
    }
    groups
        .into_iter()
        .filter(|(_, n)| n.len() >= 2)
        .map(|(d, n)| {
            let n: Vec<String> = n.into_iter().map(zygo_core::esc).collect();
            format!("pool: {} is shared by {}", zygo_core::esc(&d.to_string_lossy()), n.join(", "))
        })
        .collect()
}
