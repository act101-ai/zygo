//! Import (mail sync design S§8.5): candidates, checks, crash-safe delivery, finish incoming. S-C5: every remote
//! name, mode, size and JSON value is checked before use; a file path holds only a local pane key and a ULID.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::time::Instant;

use ulid::Ulid;
use zygo_work::repo::{Flow, RunError};
use zygo_workspace::identity::valid_pane_key;
use zygo_workspace::mail::store::{unread, write_synced};
use zygo_workspace::mail::{ensure_mail_dir, Delivery, Message, BODY_MAX, MAIL_LOCK_WAIT, SUBJECT_MAX};
use zygo_workspace::state::Store;

use crate::address::{self, ulid_file};
use crate::branch::{self, BranchError, Entry};
use crate::env::{Crash, Seam};
use crate::state::{imported_append, imported_load, SyncPaths};

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

/// The `json` check: `bytes` parse as the maildir reader's message type, so every value, unknown keys included, is
/// parsed as the reader parses it (a number out of range, a lone surrogate or bytes that are not UTF-8 are refused).
/// A duplicate field is an error. Only an object is a message: serde reads a struct from an array too.
fn parse_message(bytes: &[u8]) -> Result<Message, &'static str> {
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err("json");
    }
    serde_json::from_slice(bytes).map_err(|_| "json")
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
    let m = parse_message(bytes)?;
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

/// S§8.5 step 5: finishes each staged delivery in `incoming/` (`<pane key>.<ULID>.json`): the ULID goes into the
/// imported log when it is not there, then the file moves into `new/` (the inbox check passed before it was
/// staged). A file whose ULID is not in the log may be torn by a crash during its write: unless its bytes parse as
/// a message whose `id` is the file's ULID, it is deleted and not logged, and the next pass imports the branch copy
/// again. A file whose ULID is in the log was synced before the append. Returns the count of files moved into `new/`. Other names and links are left alone; a file whose finish
/// fails stays for the next pass. Each file is one M§5 P1 delivery under `mail/lock` of `store`'s workspace held
/// shared: a file whose key has no record in the read under that lock is deleted with the log unchanged (the branch
/// copy stays); a lock not taken in [`MAIL_LOCK_WAIT`] ends the finish for this pass, and the file stays. `Err` only
/// when `incoming/` or the imported log cannot be read.
pub fn finish_incoming(sp: &SyncPaths, store: &Store) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(&sp.incoming) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut seen = imported_load(sp)?;
    let mut moved = 0;
    for e in entries {
        let e = e?;
        let name = e.file_name();
        let Some((key, rest)) = name.to_str().and_then(|n| n.split_once('.')) else { continue };
        let Some(id) = ulid_file(rest).filter(|_| valid_pane_key(key)) else { continue };
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Ok(d) = Delivery::begin(store, MAIL_LOCK_WAIT) else { break }; // S§8.5 step 5: the file stays
        if !d.workspace().members.contains_key(key) {
            let _ = std::fs::remove_file(e.path()); // author's call 3: M§5 deletes a member's mail with its record
            continue;
        }
        if !seen.contains(&id) {
            let mut bytes = Vec::new();
            let read = std::fs::File::open(e.path()).and_then(|f| f.take(MESSAGE_MAX + 1).read_to_end(&mut bytes));
            if read.is_err() {
                continue;
            }
            if !parse_message(&bytes).is_ok_and(|m| m.id == id.to_string()) {
                let _ = std::fs::remove_file(e.path());
                continue;
            }
            if imported_append(sp, id).is_err() {
                continue;
            }
            seen.insert(id);
        }
        if let Ok(true) = d.deliver_staged(key, &id.to_string(), &e.path(), u32::MAX) {
            moved += 1;
        }
    }
    Ok(moved)
}

/// One candidate of a pass: its ULID, the pane key it goes to, its folder and its tree entry.
struct Candidate {
    id: Ulid,
    key: String,
    folder: String,
    entry: Entry,
}

/// The candidates of S§8.5 for each own host and target, in that order: entries `<folder>/<ULID>.json` not in
/// `own_ack`, each ULID once, at most [`CANDIDATE_CAP`]. A folder's reading also stops at its target's inbox room
/// (`max_unread` less its unread count, over all own hosts); a candidate already in the imported log (`seen`) takes
/// no room. Adds 1 to `left` for each folder stopped at its room, and once when the whole reading stopped early. A
/// stop at the deadline is normal and writes no line.
#[allow(clippy::too_many_arguments)]
fn candidates(wt: &Path, tip: &str, own_hosts: &[String], targets: &[Target], own_ack: &BTreeSet<Ulid>,
    seen: &HashSet<Ulid>, mail_dir: &Path, max_unread: u32, deadline: Instant, left: &mut usize,
    log: &mut dyn FnMut(&str)) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut ids = HashSet::new();
    let mut rooms: HashMap<&str, usize> = HashMap::new();
    for host in own_hosts {
        for t in targets {
            let folder = format!("{}@{}", t.name, host);
            if !valid_pane_key(&t.pane_key) || address::parse(&folder).is_none() {
                continue;
            }
            let room = rooms.entry(t.pane_key.as_str()).or_insert_with(|| {
                unread(mail_dir, &t.pane_key).map_or(0, |n| (max_unread as usize).saturating_sub(n))
            });
            if *room == 0 {
                *left += 1;
                continue;
            }
            let prefix = format!("{folder}/");
            let (mut full, mut no_room) = (false, false);
            let r = branch::ls(wt, tip, Some(&prefix), deadline, &mut |e| {
                let Some(id) = e.path.strip_prefix(&prefix).filter(|n| !n.contains('/')).and_then(ulid_file) else {
                    return Flow::Go;
                };
                if own_ack.contains(&id) || ids.contains(&id) {
                    return Flow::Go;
                }
                if out.len() == CANDIDATE_CAP {
                    full = true;
                    return Flow::Stop;
                }
                if !seen.contains(&id) {
                    if *room == 0 {
                        no_room = true;
                        return Flow::Stop;
                    }
                    *room -= 1;
                }
                ids.insert(id);
                out.push(Candidate { id, key: t.pane_key.clone(), folder: folder.clone(), entry: e });
                Flow::Go
            });
            match r {
                Err(BranchError::Git(RunError::TimedOut, _)) => {}
                Err(_) => log("mail sync: import: ls-tree"),
                Ok(_) if no_room && !full => {
                    *left += 1;
                    continue;
                }
                Ok(_) => {}
            }
            if r.is_err() || full {
                *left += 1;
                return out;
            }
        }
    }
    out
}

/// S§8.5: one import pass over the side-branch tree `tip` in worktree `wt`. Each candidate is checked; a refused
/// one is logged (one line with its ULID and check name) and acknowledged, a passing one is delivered through
/// `incoming/` (steps 1-4, with the seams `after_incoming`, `after_log`, `after_new`, `after_ack`). A full inbox
/// leaves the message neither delivered nor acknowledged. Steps 1-3 of each candidate run under `mail/lock` of
/// `store`'s workspace held shared (M§5 P1), taken after the candidate's git reads and dropped before the next: a
/// candidate whose key has no record in the read under that lock is left on the branch (not staged, logged or
/// acknowledged; no line); a lock not taken in [`MAIL_LOCK_WAIT`] writes `mail sync: import <ULID>: lock` and stops
/// the run. `Err` only when a seam stops the run.
#[allow(clippy::too_many_arguments)]
pub fn run(sp: &SyncPaths, store: &Store, wt: &Path, tip: &str, own_hosts: &[String], targets: &[Target],
    own_ack: &BTreeSet<Ulid>, max_unread: u32, deadline: Instant, seam: &dyn Seam, log: &mut dyn FnMut(&str))
    -> Result<Imported, Crash> {
    let mail_dir = store.dir().join("mail");
    let mail_dir = mail_dir.as_path();
    let mut out = Imported::default();
    let Ok(mut seen) = imported_load(sp) else {
        log("mail sync: import: imported");
        out.left = 1;
        return Ok(out);
    };
    for c in candidates(wt, tip, own_hosts, targets, own_ack, &seen, mail_dir, max_unread, deadline, &mut out.left, log) {
        if Instant::now() >= deadline {
            out.left += 1;
            break;
        }
        let id = c.id;
        if seen.contains(&id) {
            out.acked.push(id);
            continue;
        }
        let e = &c.entry;
        let name = &e.path[c.folder.len() + 1..];
        let size = e.size.unwrap_or(u64::MAX);
        let checked = if e.mode != "100644" || e.kind != "blob" || size > MESSAGE_MAX {
            check_message(&c.folder, name, &e.mode, &e.kind, size, b"").map(|()| Vec::new())
        } else {
            match branch::cat(wt, &e.id, MESSAGE_MAX, deadline) {
                Ok(b) => check_message(&c.folder, name, &e.mode, &e.kind, size, &b).map(|()| b),
                Err(BranchError::Refused("too large")) => Err("size"),
                Err(_) => {
                    log(&format!("mail sync: import {id}: read"));
                    out.left += 1;
                    continue;
                }
            }
        };
        let bytes = match checked {
            Ok(b) => b,
            Err(check) => {
                if imported_append(sp, id).is_err() {
                    log(&format!("mail sync: import {id}: write"));
                    out.left += 1;
                    continue;
                }
                seen.insert(id);
                out.acked.push(id);
                log(&format!("mail sync: import {id}: {check}"));
                continue;
            }
        };
        if !unread(mail_dir, &c.key).is_ok_and(|n| n < max_unread as usize) {
            out.left += 1;
            continue;
        }
        // S§8.5 "Delivery, crash-safe": steps 1-3 of this one candidate under `mail/lock` shared (M§5 P1), taken after
        // every git call of the candidate and dropped before the next one (author's call 1).
        let Ok(d) = Delivery::begin(store, MAIL_LOCK_WAIT) else {
            log(&format!("mail sync: import {id}: lock"));
            out.left += 1;
            break;
        };
        if !d.workspace().members.contains_key(&c.key) {
            continue; // not staged, logged or acked: it waits on the branch (S§13 limit 2)
        }
        let staged = sp.incoming.join(format!("{}.{id}.json", c.key));
        let wrote = [&sp.mail, &sp.sync, &sp.incoming].into_iter().try_for_each(|d| ensure_mail_dir(d))
            .and_then(|()| write_synced(&staged, &bytes));
        if wrote.is_err() {
            log(&format!("mail sync: import {id}: write"));
            out.left += 1;
            continue;
        }
        seam.at("after_incoming")?;
        if imported_append(sp, id).is_err() {
            log(&format!("mail sync: import {id}: write"));
            out.left += 1;
            continue;
        }
        seen.insert(id);
        seam.at("after_log")?;
        let Ok(moved) = d.deliver_staged(&c.key, &id.to_string(), &staged, max_unread) else {
            out.left += 1;
            continue;
        };
        drop(d);
        seam.at("after_new")?;
        if moved {
            out.delivered.push((c.key.clone(), id));
        }
        out.acked.push(id);
        seam.at("after_ack")?;
    }
    Ok(out)
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
