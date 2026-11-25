//! Import (mail sync design S§8.5): candidates, checks, crash-safe delivery, finish incoming. S-C5: every remote
//! name, mode, size and JSON value is checked before use; a file path holds only a local pane key and a ULID.
use serde::Deserialize;
use zygo_workspace::mail::{BODY_MAX, SUBJECT_MAX};

use crate::address;

/// S§8.5: the most candidates one import pass reads.
pub const CANDIDATE_CAP: usize = 500;

/// S§8.5: the largest message blob read, the 8 MiB limit of the maildir reader.
pub const MESSAGE_MAX: u64 = zygo_workspace::mail::store::FILE_MAX;

/// A local member that can receive mail: its name (the folder `<name>@<own host>`) and its pane key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub pane_key: String,
}

/// What one import pass did. `delivered` holds `(pane key, ULID)`; `acked` each ULID once, in reading order;
/// `left` the seen candidates not finished, plus 1 when the reading stopped early.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    pub delivered: Vec<(String, ulid::Ulid)>,
    pub acked: Vec<ulid::Ulid>,
    pub left: usize,
}

/// The seven message fields the checks read; unknown keys are ignored, a duplicate key is an error.
#[derive(Deserialize)]
struct Fields {
    version: u64,
    id: String,
    from: String,
    to: String,
    sent_at: String,
    subject: String,
    body: String,
}

/// S§8.5 "Checks" on one entry of folder `folder`: the name of the first failed check (`mode`, `name`, `size`,
/// `json`, `version`, `id`, `to`, `from`, `subject`, `body`, `sent-at`), else `Ok`.
pub fn check_message(folder: &str, file: &str, mode: &str, kind: &str, size: u64, bytes: &[u8])
    -> Result<(), &'static str> {
    if mode != "100644" || kind != "blob" {
        return Err("mode");
    }
    let id = address::ulid_file(file).ok_or("name")?;
    if size > MESSAGE_MAX {
        return Err("size");
    }
    // serde reads a struct from an array too: only an object is a message.
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err("json");
    }
    let m: Fields = serde_json::from_slice(bytes).map_err(|_| "json")?;
    if m.version != 1 {
        return Err("version");
    }
    if m.id != id.to_string() {
        return Err("id");
    }
    if m.to != folder {
        return Err("to");
    }
    if address::parse(&m.from).is_none() {
        return Err("from");
    }
    if m.subject.len() > SUBJECT_MAX {
        return Err("subject");
    }
    if m.body.len() > BODY_MAX {
        return Err("body");
    }
    if !rfc3339(&m.sent_at) {
        return Err("sent-at");
    }
    Ok(())
}

/// RFC 3339 §5.6 `date-time`, ASCII only, with the calendar and clock ranges checked; nothing before or after.
fn rfc3339(s: &str) -> bool {
    let b = s.as_bytes();
    let num = |i: usize, n: usize| -> Option<u32> {
        let d = b.get(i..i + n)?;
        d.iter().all(u8::is_ascii_digit).then(|| d.iter().fold(0, |a, c| a * 10 + u32::from(c - b'0')))
    };
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(sec)) =
        (num(0, 4), num(5, 2), num(8, 2), num(11, 2), num(14, 2), num(17, 2))
    else {
        return false;
    };
    if !(at(4, b'-') && at(7, b'-') && matches!(b.get(10), Some(b'T' | b't')) && at(13, b':') && at(16, b':')) {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let last = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    if !(1..=last).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return false;
    }
    let mut i = 19;
    if at(i, b'.') {
        let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return false;
        }
        i += 1 + digits;
    }
    match b.get(i) {
        Some(b'Z' | b'z') => i + 1 == b.len(),
        Some(b'+' | b'-') => {
            let (Some(oh), Some(om)) = (num(i + 1, 2), num(i + 4, 2)) else { return false };
            at(i + 3, b':') && oh <= 23 && om <= 59 && i + 6 == b.len()
        }
        _ => false,
    }
}
