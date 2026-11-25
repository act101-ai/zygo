//! The record set: the directory reader and the set checks (D§6.8). Read at each call (D-C3).
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use super::graph::back_edges;
use super::record::{name_kind, read_record, NameKind, Record, Status, DIR};

/// At most this many directory entries (D§6.8).
const MAX_ENTRIES: usize = 10_000;

/// The records of `docs/decisions/`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordSet {
    /// The valid records, sorted by id.
    pub valid: Vec<Record>,
    /// `(file name, fault lines)` of each malformed record file, sorted by name.
    pub malformed: Vec<(String, Vec<String>)>,
}

/// Why `docs/decisions/` was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirError {
    /// A file or a link at `docs` or `docs/decisions`.
    NotDirectory,
    /// An `lstat` of `docs` or `docs/decisions` that fails other than `NotFound`, a listing that cannot be opened,
    /// or an entry error while the listing is read (D§6.8).
    Unreadable,
    /// More than 10,000 entries.
    TooMany,
}

/// Reads `top/docs/decisions` (`top` is the repository root). An absent directory is an empty set; other names and
/// subdirectories are ignored. All names are listed before any record file is opened. A link at `docs` or at
/// `docs/decisions` is `NotDirectory`. A name that is not UTF-8 is malformed if its bytes start with `DDR-` and end
/// with `.md`, and ignored otherwise. An `lstat` error other than `NotFound`, a listing error or an entry error is
/// `Unreadable`, and no part of the listing is used.
pub fn read_dir(top: &Path) -> Result<RecordSet, DirError> {
    let dir = top.join(DIR);
    for path in [dir.parent().unwrap_or(&dir), &dir] {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(RecordSet::default()),
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => return Err(DirError::NotDirectory),
            Err(_) => return Err(DirError::Unreadable),
        }
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|_| DirError::Unreadable)? {
        let entry = entry.map_err(|_| DirError::Unreadable)?;
        if names.len() == MAX_ENTRIES {
            return Err(DirError::TooMany);
        }
        names.push(entry.file_name());
    }
    let mut set = RecordSet::default();
    for os_name in &names {
        let Some(name) = os_name.to_str() else {
            // Lossy conversion keeps the valid bytes at both ends, so these tests read the raw name's ends.
            let lossy = os_name.to_string_lossy();
            if lossy.starts_with("DDR-") && lossy.ends_with(".md") {
                set.malformed.push((lossy.into_owned(), vec!["bad record name".to_string()]));
            }
            continue;
        };
        match name_kind(name) {
            NameKind::Record(_) => match read_record(&dir.join(name)) {
                Ok(r) => set.valid.push(r),
                Err(faults) => set.malformed.push((name.to_string(), faults)),
            },
            NameKind::Malformed => set.malformed.push((name.to_string(), vec!["bad record name".to_string()])),
            NameKind::Other => {}
        }
    }
    set.valid.sort_by(|a, b| a.id.cmp(&b.id));
    set.malformed.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(set)
}

/// The set fault lines of D§6.8 over `set.valid`: duplicate ids, then per record the `depends_on` and `supersedes`
/// targets that are not records (or a `supersedes` target not `superseded`), then one line per `depends_on` cycle.
/// Each distinct line is given once, at its first place (D§17 #3).
pub fn set_faults(set: &RecordSet) -> Vec<String> {
    let mut faults = Vec::new();
    let mut by_id: HashMap<&str, &Record> = HashMap::new();
    for r in &set.valid {
        if by_id.contains_key(r.id.as_str()) {
            faults.push(format!("duplicate id {}", r.id));
        } else {
            by_id.insert(r.id.as_str(), r);
        }
    }
    for r in &set.valid {
        for d in r.depends_on.iter().filter(|d| !by_id.contains_key(d.as_str())) {
            faults.push(format!("{}: depends_on {d} is not a record", r.id));
        }
        if let Some(t) = &r.supersedes {
            match by_id.get(t.as_str()) {
                None => faults.push(format!("{}: supersedes {t} is not a record", r.id)),
                Some(target) if target.status != Status::Superseded => {
                    faults.push(format!("{}: supersedes {t}, which is {}, not superseded", r.id, target.status.name()))
                }
                Some(_) => {}
            }
        }
    }
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in &set.valid {
        let children = next.entry(r.id.as_str()).or_default();
        children.extend(r.depends_on.iter().map(String::as_str).filter(|d| by_id.contains_key(d)));
    }
    let starts: Vec<&str> = set.valid.iter().map(|r| r.id.as_str()).collect();
    faults.extend(back_edges(&next, &starts).into_iter().map(|id| format!("depends_on cycle at {id}")));
    let mut seen = HashSet::new();
    faults.retain(|f| seen.insert(f.clone()));
    faults
}
