//! The operator outcomes `ratify` and `overturn` (D§11.3, D§6.6): byte-preserving writes of their keys (D§6.2).
use std::fs;
use std::io::Write;
use std::path::Path;

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
/// `promoted_to` absent. Err lines: `zygo decision: <id> is <status>` and
/// `zygo decision: <id> is promoted; remove the promotion by hand first` (the status test first).
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
    Ok(())
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

/// Re-reads `path` with `read_bytes`; `Changed` when the bytes differ from `shown`; else checks `parse_record` and
/// [`allowed`], sets the outcome keys with `toml_edit` (D§6.2: unknown keys, comments and the body stay byte-equal),
/// checks the result with `parse_record`, writes `<dir>/<id>.<ULID>.tmp` (create_new, write_all, sync_all) and renames
/// it to `path` (D§11.3). A failed write removes the temporary file (best effort).
pub fn apply(path: &Path, shown: &[u8], outcome: &Outcome, today: &str) -> Result<(), EditError> {
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
    let close = text[..front_end].trim_end_matches(['\n', '\r']).len() - "+++".len();
    let mut doc: DocumentMut = text[open..close].parse().map_err(|_| invalid())?;
    for (k, v) in keys(outcome, today) {
        set(&mut doc, k, v);
    }
    let mut rendered = doc.to_string();
    if &text[..open] == "+++\r\n" {
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
    write(path, &r.id, new.as_bytes())
}

/// Writes `bytes` to `<dir>/<id>.<ULID>.tmp` and renames it to `path`; removes the temporary file on failure.
fn write(path: &Path, id: &str, bytes: &[u8]) -> Result<(), EditError> {
    let fail = |p: &Path, e: std::io::Error| EditError::Io(format!("zygo decision: cannot write {}: {e}", p.display()));
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!("{id}.{}.tmp", zygo_core::ids::ulid()));
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| fail(&tmp, e))?;
    let done = f.write_all(bytes).and_then(|()| f.sync_all());
    drop(f);
    let done = done.map_err(|e| fail(&tmp, e)).and_then(|()| fs::rename(&tmp, path).map_err(|e| fail(path, e)));
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}
