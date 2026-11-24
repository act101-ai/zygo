//! The operator outcomes `ratify` and `overturn` (D§11.3, D§6.6): byte-preserving writes of their keys (D§6.2).
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Value};

use super::record::{parse_record, read_bytes, Record, Status};

/// The promotion target of `ratify --promote` (D§11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    Adr,
    Spec,
    Rule,
}

impl Promotion {
    /// `adr`, `spec`, `rule`; exact, case-sensitive.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "adr" => Some(Self::Adr),
            "spec" => Some(Self::Spec),
            "rule" => Some(Self::Rule),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Adr => "adr",
            Self::Spec => "spec",
            Self::Rule => "rule",
        }
    }
}

/// One operator outcome (D§11.3, D§6.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ratify { note: String, promote: Option<(Promotion, String)> },
    Overturn { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The file's bytes differ from `shown`; nothing written.
    Changed,
    /// The transition is not allowed, or the file is not a valid record; the stderr line; nothing written.
    Refused(String),
    /// A read or write failed; the stderr line, naming the file.
    Io(String),
}

/// D§6.6 operator rows. Ratify: `status` must be `open`. Overturn: `status` must be `open` or `ratified`, and
/// `promoted_to` absent. D§11.3: the promotion text and the reason are not empty (no trim, as the CLI). Err lines:
/// `zygo decision: <id> is <status>`, `zygo decision: <id> is promoted; remove the promotion by hand first`,
/// `zygo decision: <id>: the promotion text is empty` and `zygo decision: <id>: the reason is empty`, in this order.
pub fn allowed(r: &Record, outcome: &Outcome) -> Result<(), String> {
    let ok = match outcome {
        Outcome::Ratify { .. } => r.status == Status::Open,
        Outcome::Overturn { .. } => matches!(r.status, Status::Open | Status::Ratified),
    };
    if !ok {
        return Err(format!("zygo decision: {} is {}", r.id, r.status.name()));
    }
    if matches!(outcome, Outcome::Overturn { .. }) && r.promoted_to.is_some() {
        return Err(format!("zygo decision: {} is promoted; remove the promotion by hand first", r.id));
    }
    match outcome {
        Outcome::Ratify { promote: Some((_, text)), .. } if text.is_empty() => {
            Err(format!("zygo decision: {}: the promotion text is empty", r.id))
        }
        Outcome::Overturn { reason } if reason.is_empty() => Err(format!("zygo decision: {}: the reason is empty", r.id)),
        _ => Ok(()),
    }
}

/// A one-line basic TOML string of `v`: `\`, `"`, LF, CR, TAB and the other control characters escaped.
fn basic(v: &str) -> Value {
    let mut s = String::from("\"");
    for c in v.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if c < '\u{20}' || c == '\u{7f}' => s.push_str(&format!("\\u{:04X}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
    s.parse().expect("an escaped basic string is a TOML value")
}

/// Sets root key `k` to `v`: an existing value is replaced in place with its decor kept (the comment lines above a key
/// are key decor, and `insert` would drop them); an absent key is appended after the last root key-value.
fn set(doc: &mut DocumentMut, k: &str, v: &str) {
    if let Some(old) = doc.get_mut(k).and_then(Item::as_value_mut) {
        let d = old.decor().clone();
        *old = basic(v);
        *old.decor_mut() = d;
    } else {
        doc.insert(k, Item::Value(basic(v)));
    }
}

/// The outcome keys, in the order they are written (D§11.3).
fn keys<'a>(outcome: &'a Outcome, today: &'a str) -> Vec<(&'static str, &'a str)> {
    match outcome {
        Outcome::Ratify { note, promote } => {
            let mut k = vec![("status", "ratified"), ("reviewed", today), ("review_note", note.as_str())];
            if let Some((p, text)) = promote {
                k.push(("promoted_to", p.name()));
                k.push(("promotion", text.as_str()));
            }
            k
        }
        Outcome::Overturn { reason } => {
            vec![("status", "overturned"), ("reviewed", today), ("review_note", reason.as_str())]
        }
    }
}

/// Takes an exclusive lock on `lock` (the caller gives [`lock_path`] of the repository's git common dir), then
/// re-reads `path` with `read_bytes`; `Changed` when the bytes differ from `shown`; else checks `parse_record` and
/// [`allowed`], refuses a front matter that mixes CRLF and LF lines, sets the outcome keys with `toml_edit` (D§6.2:
/// unknown keys, comments and the body stay byte-equal), checks the result with `parse_record`, writes
/// `<dir>/<id>.<ULID>.tmp` (create_new, write_all, sync_all) and renames it to `path` (D§11.3). A failed write removes
/// the temporary file (best effort). The lock is held until after the rename, so of two zygo processes only one
/// writes: the other waits, re-reads the renamed file and gets `Changed`.
pub fn apply(path: &Path, lock: &Path, shown: &[u8], outcome: &Outcome, today: &str) -> Result<(), EditError> {
    apply_with(path, lock, shown, outcome, today, &mut || {}, &mut || {})
}

/// The lock file of [`apply`] for a repository: `<common_dir>/zygo-decisions.lock`, a flat file in the git common dir
/// beside `zygo-work`, so it is outside the work tree (never untracked, never committed) and one file for all linked
/// worktrees. It is never written, renamed or removed, so every `apply` locks the same file; a lock on the record
/// itself would not do this, because the rename replaces the record, and on Windows a lock on the record blocks reads
/// of it (os error 33).
pub fn lock_path(common_dir: &Path) -> PathBuf {
    common_dir.join("zygo-decisions.lock")
}

/// Opens or creates `path` (no link followed, a FIFO does not block, never truncated; its parent is not created) and
/// locks it exclusively. A failure is `Io` with `zygo decision: cannot write <path>: <error>`.
fn lock(path: &Path) -> Result<fs::File, EditError> {
    let fail = |e: std::io::Error| EditError::Io(format!("zygo decision: cannot write {}: {e}", path.display()));
    let mut o = fs::OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let f = o.open(path).map_err(fail)?;
    f.lock().map_err(fail)?;
    Ok(f)
}

/// [`apply`] with test seams: `before_read` called after the lock is taken and before the re-read, `before_rename`
/// after the temporary file is written and before the rename.
pub(crate) fn apply_with(
    path: &Path,
    lock_file: &Path,
    shown: &[u8],
    outcome: &Outcome,
    today: &str,
    before_read: &mut dyn FnMut(),
    before_rename: &mut dyn FnMut(),
) -> Result<(), EditError> {
    let _lock = lock(lock_file)?;
    before_read();
    let bytes = read_bytes(path)
        .map_err(|f| EditError::Io(format!("zygo decision: cannot read {}: {}", path.display(), f.join("; "))))?;
    if bytes != shown {
        return Err(EditError::Changed);
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let r = parse_record(stem, &bytes)
        .map_err(|_| EditError::Refused(format!("zygo decision: {stem} is not a valid record")))?;
    allowed(&r, outcome).map_err(EditError::Refused)?;
    let invalid = || EditError::Refused(format!("zygo decision: {} is not a valid record", r.id));
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    // The front matter lies between the end of the opening `+++` line and the start of the closing one.
    let open = text.find('\n').map_or(text.len(), |i| i + 1);
    let front_end = text.len() - r.body.len();
    // D§6.2: an untouched line stays byte-equal, so a front matter whose lines mix CRLF and LF is not rewritten.
    let crlf = text[..front_end].matches("\r\n").count();
    if crlf != 0 && crlf != text[..front_end].matches('\n').count() {
        return Err(EditError::Refused(format!(
            "zygo decision: {}: the front matter mixes CRLF and LF line endings; make them one kind first",
            r.id
        )));
    }
    let close = text[..front_end].trim_end_matches(['\n', '\r']).len() - "+++".len();
    let mut doc: DocumentMut = text[open..close].parse().map_err(|_| invalid())?;
    for (k, v) in keys(outcome, today) {
        set(&mut doc, k, v);
    }
    let mut rendered = doc.to_string();
    if crlf != 0 {
        rendered = rendered.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    let new = format!("{}{rendered}{}", &text[..open], &text[close..]);
    if let Err(faults) = parse_record(stem, new.as_bytes()) {
        return Err(EditError::Refused(format!(
            "zygo decision: {}: the outcome would make the record malformed: {}",
            r.id,
            faults.join("; ")
        )));
    }
    write(path, &r.id, new.as_bytes(), before_rename)
}

/// Writes `bytes` to `<dir>/<id>.<ULID>.tmp` and renames it to `path`; removes the temporary file on failure.
fn write(path: &Path, id: &str, bytes: &[u8], before_rename: &mut dyn FnMut()) -> Result<(), EditError> {
    let fail = |p: &Path, e: std::io::Error| EditError::Io(format!("zygo decision: cannot write {}: {e}", p.display()));
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!("{id}.{}.tmp", zygo_core::ids::ulid()));
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| fail(&tmp, e))?;
    let done = f.write_all(bytes).and_then(|()| f.sync_all());
    drop(f);
    if done.is_ok() {
        before_rename();
    }
    let done = done.map_err(|e| fail(&tmp, e)).and_then(|()| fs::rename(&tmp, path).map_err(|e| fail(path, e)));
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, TryLockError};

    const ID: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00001";
    const OPEN: &str = "+++\nid = \"DDR-01JDQ2Y7Z8K4M5N6P7Q8R00001\"\nkind = \"decision\"\nstatus = \"open\"\nclass = \"none\"\n\
        title = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\n\
        commit = \"6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556\"\ndate = \"2026-11-24\"\ndepends_on = []\nreverse_cost = \"low\"\n\
        +++\n\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";

    /// The record in `<t>/docs/decisions` and the lock file of the git common dir `<t>/git`.
    fn put(t: &Path) -> (PathBuf, PathBuf) {
        let dir = t.join("docs/decisions");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{ID}.md"));
        fs::write(&p, OPEN).unwrap();
        fs::create_dir_all(t.join("git")).unwrap();
        (p, lock_path(&t.join("git")))
    }

    fn names(dir: &Path) -> Vec<String> {
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect()
    }

    /// True when a second handle's `try_lock` on `l` gets `WouldBlock`, so another `apply` would wait; false when `l`
    /// does not exist.
    fn held(l: &Path) -> bool {
        File::open(l).is_ok_and(|f| matches!(f.try_lock(), Err(TryLockError::WouldBlock)))
    }

    /// D§11.3 two reviews, review focus 39: the lock is taken before the re-read, so a second zygo process cannot read
    /// the old bytes, compare them equal to its `shown` and then write after the first one's rename.
    #[test]
    fn the_lock_is_held_before_the_re_read() {
        let t = tempfile::tempdir().unwrap();
        let (p, l) = put(t.path());
        let mut seen = None;
        let mut before_read = || seen = Some(held(&l));
        let o = Outcome::Overturn { reason: "x".into() };
        assert_eq!(apply_with(&p, &l, OPEN.as_bytes(), &o, "2026-12-01", &mut before_read, &mut || {}), Ok(()));
        assert_eq!(seen, Some(true));
    }

    /// D§11.3 two reviews, review focus 39: from the re-read to the rename the lock file is locked, so a second zygo
    /// process waits and then re-reads the renamed file (`Changed`). The record stays readable through another handle
    /// while the lock is held (on Windows a lock on the record itself failed this read with os error 33).
    #[test]
    fn the_record_is_locked_from_the_re_read_to_the_rename() {
        let t = tempfile::tempdir().unwrap();
        let (p, l) = put(t.path());
        assert_eq!(l, t.path().join("git/zygo-decisions.lock"));
        let mut seen = None;
        let mut hook = || seen = Some((held(&l), fs::read_to_string(&p).unwrap() == OPEN));
        let o = Outcome::Overturn { reason: "x".into() };
        assert_eq!(apply_with(&p, &l, OPEN.as_bytes(), &o, "2026-12-01", &mut || {}, &mut hook), Ok(()));
        assert_eq!(seen, Some((true, true)));
        assert!(File::open(&l).unwrap().try_lock().is_ok()); // released after the rename
        assert_eq!(fs::read(&l).unwrap(), b""); // never written
        assert_eq!(names(p.parent().unwrap()), vec![format!("{ID}.md")]); // nothing else in the records directory
    }

    /// D§14 "the write of an outcome fails": a failed rename is `Io`, the record is unchanged and no `.tmp` is left.
    #[test]
    fn a_failed_rename_is_io_and_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let (p, l) = put(t.path());
        let dir = p.parent().unwrap().to_path_buf();
        let mut hook = || {
            for n in names(&dir).into_iter().filter(|n| n.ends_with(".tmp")) {
                fs::remove_file(dir.join(n)).unwrap();
            }
        };
        let o = Outcome::Overturn { reason: "x".into() };
        let r = apply_with(&p, &l, OPEN.as_bytes(), &o, "2026-12-01", &mut || {}, &mut hook);
        assert!(matches!(&r, Err(EditError::Io(s)) if s.starts_with("zygo decision: cannot write ")), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), OPEN);
        assert_eq!(names(&dir), vec![format!("{ID}.md")]);
    }
}
