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
use zygo_workspace::mail::{ensure_mail_dir, Delivery, MailError, Message, BODY_MAX, MAIL_LOCK_WAIT, SUBJECT_MAX};
use zygo_workspace::mail::names;
use zygo_workspace::state::{Store, Workspace};

use crate::address::{self, ulid_file};
use crate::branch::{self, BranchError, Entry};
use crate::env::{Crash, Seam};
use crate::state::{imported_append, imported_load, pending_wake_add, SyncPaths};

/// S§8.5: the most candidates one import pass reads.
pub const CANDIDATE_CAP: usize = 500;

/// S§8.5: the largest message blob read, the 8 MiB limit of the maildir reader.
pub const MESSAGE_MAX: u64 = zygo_workspace::mail::store::FILE_MAX;

/// A local member that can receive mail: its name (the folder `<name>@<own host>`) and its pane key. The pane key is
/// that of the pass's first read; it sets the folder's inbox room only, and each delivery maps the name to a key
/// again on the read under `mail/lock` (FX-24 review F2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub pane_key: String,
}

/// What one import pass did. `delivered` holds `(pane key, ULID)`; `acked` each ULID once, in reading order;
/// `left` the seen candidates not finished, plus 1 when the reading stopped early; `read` the state read under
/// `mail/lock` at the last delivery, which holds the record of each key in `delivered` that still has one (the wake
/// uses it, S§10.1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    pub delivered: Vec<(String, ulid::Ulid)>,
    pub acked: Vec<ulid::Ulid>,
    pub left: usize,
    pub read: Option<Workspace>,
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
/// again. A file whose ULID is in the log was synced before the append. Returns the pane key and ULID of each file
/// moved into `new/`, so that S§8.3 step 10 counts and wakes it (FX-30).
/// Other names and links are left alone; a file whose finish fails stays for the next pass. Each file is one M§5 P1
/// delivery under `mail/lock` of `store`'s workspace held shared. A file whose key has no record in the read under
/// that lock stays while `mail/<key>/` exists (the record can come back, as at the undo write of `up`), and is
/// deleted with the log unchanged when `mail/<key>/` is missing (M§5 removed the member's mail; the branch copy
/// stays). A lock not taken in [`MAIL_LOCK_WAIT`] ends the finish for this pass, and the file stays. `Err` only when
/// `incoming/` or the imported log cannot be read.
pub fn finish_incoming(sp: &SyncPaths, store: &Store) -> std::io::Result<Vec<(String, Ulid)>> {
    let entries = match std::fs::read_dir(&sp.incoming) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut seen = imported_load(sp)?;
    let mut moved = Vec::new();
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
            // Author's call 3 (FX-24 review F1): a record can be out for a short time (`up` step 8 to its undo write);
            // M§5 removes the maildir only after the record went for good, so only a missing maildir deletes.
            if std::fs::symlink_metadata(sp.mail.join(key)).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
                let _ = std::fs::remove_file(e.path());
            }
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
            moved.push((key.to_string(), id));
        }
    }
    Ok(moved)
}

/// S§8.5 (FX-24 review F6): the check name of the log line of a delivery that could not begin: `busy` (`mail/lock`
/// held for the whole bound), `no-workspace` (no state folder or no `workspace.json`), `state` (a state folder that
/// is a link or not a folder, or a state that does not read as valid), `mail` (FX-24 review R2 F2: `mail/` that
/// cannot be made or checked, such as a link or an entry that is not a folder), `io` (any other error). The kind
/// comes from the `MailError` variant of [`Delivery::begin`], not from an `io::ErrorKind`.
fn begin_error(e: &MailError) -> &'static str {
    match e {
        MailError::Busy => "busy",
        MailError::NoWorkspace => "no-workspace",
        MailError::BadState(_) => "state",
        MailError::BadMail(_) => "mail",
        _ => "io",
    }
}

/// The ULIDs of the staged files in `incoming/` (`<pane key>.<ULID>.json`, regular files; links and other names are
/// left out); none when `incoming/` is missing.
fn staged_ids(sp: &SyncPaths) -> std::io::Result<HashSet<Ulid>> {
    let entries = match std::fs::read_dir(&sp.incoming) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(e) => return Err(e),
    };
    let mut out = HashSet::new();
    for e in entries {
        let e = e?;
        let name = e.file_name();
        let Some((key, rest)) = name.to_str().and_then(|n| n.split_once('.')) else { continue };
        if let Some(id) = ulid_file(rest).filter(|_| valid_pane_key(key))
            && e.file_type().is_ok_and(|t| t.is_file())
        {
            out.insert(id);
        }
    }
    Ok(out)
}

/// One candidate of a pass: its ULID, the member name of its folder, its folder and its tree entry.
struct Candidate {
    id: Ulid,
    name: String,
    folder: String,
    entry: Entry,
}

/// The candidates of S§8.5 for each own host and target, in that order: entries `<folder>/<ULID>.json` not in
/// `own_ack` and not in `taken`, the ULIDs that this pass took already (in an earlier push attempt or an earlier
/// folder), each added to `taken`, while `taken` holds fewer than [`CANDIDATE_CAP`] (FX-40). A folder's reading also stops at its target's inbox room
/// (`max_unread` less its unread count, over all own hosts); a candidate already in the imported log (`seen`) takes
/// no room. Adds 1 to `left` for each folder stopped at its room, and once when the whole reading stopped early. A
/// stop at the deadline is normal and writes no line.
#[allow(clippy::too_many_arguments)]
fn candidates(wt: &Path, tip: &str, own_hosts: &[String], targets: &[Target], own_ack: &BTreeSet<Ulid>,
    taken: &mut BTreeSet<Ulid>, seen: &HashSet<Ulid>, mail_dir: &Path, max_unread: u32, deadline: Instant, left: &mut usize,
    log: &mut dyn FnMut(&str)) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
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
                if own_ack.contains(&id) || taken.contains(&id) {
                    return Flow::Go;
                }
                if taken.len() >= CANDIDATE_CAP {
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
                taken.insert(id);
                out.push(Candidate { id, name: t.name.clone(), folder: folder.clone(), entry: e });
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

/// S§8.5: one import over the side-branch tree `tip` in worktree `wt`, as one push attempt of a pass; `taken` is the
/// pass's set of ULIDs taken as candidates, kept by the caller across the attempts, so the pass reads at most
/// [`CANDIDATE_CAP`] (FX-40). Each candidate is checked; a refused
/// one is logged (one line with its ULID and check name) and acknowledged, a passing one is delivered through
/// `incoming/` (steps 1-4, with the seams `after_incoming`, `after_log`, `after_new`, `after_ack`). A full inbox
/// leaves the message neither delivered nor acknowledged. Steps 1-3 of each candidate run under `mail/lock` of
/// `store`'s workspace held shared (M§5 P1), taken after the candidate's git reads and dropped before the next. The
/// folder's member name maps to its pane key, and the inbox check runs, on the read under that lock (`targets` gives
/// the names and each folder's inbox room only); a name with no record in that read is left on the branch (not
/// staged, logged or acknowledged; no line), and [`Imported::read`] keeps the read of the last delivery. A delivery
/// that cannot begin (a lock not taken in [`MAIL_LOCK_WAIT`], no workspace, an invalid state, an I/O error) writes
/// `mail sync: import <ULID>: <kind>` ([`begin_error`]) and stops the run. Before step 1 the delivery is recorded in
/// `pending_wake` ([`pending_wake_add`], FX-30); a record that cannot be written logs `mail sync: import <ULID>:
/// state` and leaves the candidate for a later pass. Step 1 makes the target's maildir before it writes the staged file. A candidate whose ULID is in the
/// imported log is acknowledged, unless its staged file is still in `incoming/`: then it is left until step 5 of a
/// later pass moves that file (S§8.5). `incoming/` that cannot be read writes `mail sync: import: incoming` and
/// imports nothing. `Err` only when a seam stops the run.
#[allow(clippy::too_many_arguments)]
pub fn run(sp: &SyncPaths, store: &Store, wt: &Path, tip: &str, own_hosts: &[String], targets: &[Target],
    own_ack: &BTreeSet<Ulid>, taken: &mut BTreeSet<Ulid>, max_unread: u32, deadline: Instant, seam: &dyn Seam,
    log: &mut dyn FnMut(&str)) -> Result<Imported, Crash> {
    let mail_dir = store.dir().join("mail");
    let mail_dir = mail_dir.as_path();
    let mut out = Imported::default();
    let Ok(mut seen) = imported_load(sp) else {
        log("mail sync: import: imported");
        out.left = 1;
        return Ok(out);
    };
    let Ok(staged_ids) = staged_ids(sp) else {
        log("mail sync: import: incoming");
        out.left = 1;
        return Ok(out);
    };
    for c in candidates(wt, tip, own_hosts, targets, own_ack, taken, &seen, mail_dir, max_unread, deadline,
        &mut out.left, log) {
        if Instant::now() >= deadline {
            out.left += 1;
            break;
        }
        let id = c.id;
        if seen.contains(&id) {
            // FX-24 review F1: a logged ULID whose staged file is still in `incoming/` is not delivered yet; step 5 of
            // a later pass moves it, and only then is it acknowledged.
            if staged_ids.contains(&id) {
                out.left += 1;
            } else {
                out.acked.push(id);
            }
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
        // S§8.5 "Delivery, crash-safe": steps 1-3 of this one candidate under `mail/lock` shared (M§5 P1), taken after
        // every git call of the candidate and dropped before the next one (author's call 1).
        let d = match Delivery::begin(store, MAIL_LOCK_WAIT) {
            Ok(d) => d,
            Err(e) => {
                log(&format!("mail sync: import {id}: {}", begin_error(&e)));
                out.left += 1;
                break;
            }
        };
        // FX-24 review F2: the folder name maps to a pane key on the read under the lock, not on the pass's first
        // read. A name with no record there is not staged, logged or acked: it waits on the branch (S§13 limit 2).
        let Some(key) = names::members(d.workspace()).into_iter()
            .find(|t| t.name.as_deref() == Some(c.name.as_str()) && valid_pane_key(&t.key))
            .map(|t| t.key)
        else {
            continue;
        };
        if !unread(mail_dir, &key).is_ok_and(|n| n < max_unread as usize) {
            out.left += 1;
            continue;
        }
        // FX-30: the pending wake is recorded before the first write of this delivery, so a crash at any later point
        // leaves a record by which the next pass counts and wakes the message it finds in `new/` (S§8.3 step 10).
        if pending_wake_add(sp, &key, id).is_err() {
            log(&format!("mail sync: import {id}: state"));
            out.left += 1;
            continue;
        }
        let staged = sp.incoming.join(format!("{key}.{id}.json"));
        // Step 1 makes the maildir first (FX-24 review F1): a staged file is kept while its maildir exists.
        let wrote = d.make_maildir(&key).map_err(|e| std::io::Error::other(e.to_string()))
            .and_then(|()| [&sp.mail, &sp.sync, &sp.incoming].into_iter().try_for_each(|d| ensure_mail_dir(d)))
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
        let Ok(moved) = d.deliver_staged(&key, &id.to_string(), &staged, max_unread) else {
            out.left += 1;
            continue;
        };
        let read = d.into_workspace(); // the lock is released here
        seam.at("after_new")?;
        if moved {
            out.delivered.push((key, id));
            out.read = Some(read);
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
