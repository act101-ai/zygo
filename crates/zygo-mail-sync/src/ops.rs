//! The operator commands of `zygo mail sync` (mail sync design S§7.6): `--take-host`, `--new-identity` and
//! `--drop-host`. Another host's files go only by its ack or by `--drop-host` (S-C8); a drop counts and asks (S-G5).
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use ulid::Ulid;
use zygo_work::repo::{Flow, RunError};

use crate::address;
use crate::branch::{self, BranchError, Change, Fetched, Pushed};
use crate::env::Env;
use crate::identity::{self, read_meta, TipMeta};
use crate::prune;
use crate::registry::{Ack, Registry, MEMBERS_MAX};
use crate::state;
use crate::sync::{self, forget_old_hosts, own_blobs, own_names, promote, stop, Ctx, Fail, Mode, Opts, Report, SyncError, MARKER_LINE,
    MAX_ATTEMPTS, MOVED_LINE, PASS_DEADLINE};

/// S§8.3 steps 6-7: the fetched tip, in the worktree, with its registries and acks; `None` when there is no branch.
/// A tip without the marker stops with [`MARKER_LINE`].
fn fetch_tip(c: &Ctx, deadline: Instant) -> Result<Option<(String, TipMeta)>, Fail> {
    let root = &c.ws.main_root;
    let tip = match branch::fetch(root, &c.s.remote, &c.s.branch, deadline)? {
        Fetched::NoBranch => return Ok(None),
        Fetched::Tip(t) => t,
    };
    branch::ensure_worktree(root, &c.sp.worktree, &tip, deadline)?;
    if !branch::has_marker(&c.sp.worktree, &tip, deadline)? {
        return Err(Fail::Stop { line: MARKER_LINE.into(), check: "marker" });
    }
    let meta = read_meta(&c.sp.worktree, &tip, deadline, &mut |l| c.log(l))?;
    Ok(Some((tip, meta)))
}

/// S§8.3 steps 13-15 (D2): one commit on the fetched tip, pushed without force. Before `build`, the S§7.5 twin check
/// ([`identity::twin`], S§8.3 step 7) stops a twin state with the twin line, so nothing is written (S§7.6); the
/// collision part does not apply, as `--take-host` exists to clear it. `build` gives the tree-ish to
/// start from (the tip, or a tree made from it) and the changes on it, or `None` for no write. Only the own blobs
/// that the changes add are `pending` before the push and `pushed` after it: a blob that another workspace with this
/// identity wrote is never recorded, so the next pass still stops with the twin line (S§7.5, S§19 #11). The state
/// write after the push keeps no key of a host that is not an own host at the commit ([`forget_old_hosts`]). A pending blob
/// at the fetched tip is recorded as pushed first, as the pass does. A moved branch fetches and builds again, at
/// most [`MAX_ATTEMPTS`] times, then [`MOVED_LINE`]. No branch writes nothing.
fn write(c: &Ctx, identity: &str, deadline: Instant,
    build: &mut dyn FnMut(&str, &TipMeta) -> Result<Option<(String, Vec<Change>)>, Fail>) -> Result<(), Fail> {
    let wt = &c.sp.worktree;
    for _ in 0..MAX_ATTEMPTS {
        let Some((tip, meta)) = fetch_tip(c, deadline)? else { return Ok(()) };
        let mut st = state::load(&c.sp).map_err(|_| stop("state"))?;
        identity::twin(&meta, identity, &st).map_err(|s| Fail::Stop { line: s.line(), check: s.check_name() })?;
        let Some((base, changes)) = build(&tip, &meta)? else { return Ok(()) };
        let commit = branch::commit(wt, &base, Some(&tip), &changes, deadline)?;
        let added: BTreeSet<&str> = changes.iter()
            .filter_map(|ch| if let Change::Add { path, .. } = ch { Some(path.as_str()) } else { None })
            .collect();
        promote(&mut st, &meta, identity);
        let commit_meta = read_meta(wt, &commit, deadline, &mut |_| {})?;
        st.pending = own_blobs(&commit_meta, identity).into_iter()
            .filter(|(path, _)| added.contains(path.as_str()))
            .collect();
        state::save(&c.sp, &st).map_err(|_| stop("state"))?;
        match branch::push(wt, &c.s.remote, &c.s.branch, &commit, None, deadline)? {
            Pushed::Accepted => {
                st.pushed.extend(std::mem::take(&mut st.pending));
                forget_old_hosts(&mut st, &commit_meta, identity, &c.s.host);
                state::save(&c.sp, &st).map_err(|_| stop("state"))?;
                branch::set_tracking(wt, &c.s.remote, &c.s.branch, &commit, deadline)?;
                return Ok(());
            }
            Pushed::Moved => {}
            Pushed::Other(_) => return Err(stop("push")),
        }
    }
    Err(Fail::Stop { line: MOVED_LINE.into(), check: "moved" })
}

/// S§7.6 `zygo mail sync --take-host`: after the public check (D7), writes `registry/<host>.json` and
/// `ack/<host>.json` with this identity, without a question. The ack keeps the entries of the tip's valid ack of the
/// host, whatever its identity (D1). A registry over its limit is not written, so neither file is (one log line,
/// O3). Then the sync. The old owner's next pass stops with the collision line.
pub fn take_host(env: &Env, ws_id: &str) -> Result<Report, SyncError> {
    sync::locked(env, ws_id, |c| {
        let deadline = Instant::now() + PASS_DEADLINE;
        sync::public_check(c, &mut Report::default(), deadline)?;
        let identity = state::make_identity(&c.sp).map_err(|_| stop("identity"))?;
        let host = c.s.host.as_str();
        write(c, &identity, deadline, &mut |tip, meta| {
            let reg = meta.registries.get(host).and_then(|r| r.value.as_ref().ok());
            let ack = meta.acks.get(host).and_then(|r| r.value.as_ref().ok());
            if reg.is_some_and(|r| r.identity == identity) && ack.is_some_and(|a| a.identity == identity) {
                return Ok(None);
            }
            let members = own_names(&c.ws);
            if members.len() > MEMBERS_MAX {
                c.log("mail sync: own file over limit: members");
                return Ok(None);
            }
            let reg = Registry { host: host.into(), identity: identity.clone(), members }.to_bytes();
            let imported = ack.map(|a| a.imported.clone()).unwrap_or_default();
            let ack = Ack { host: host.into(), identity: identity.clone(), imported }.to_bytes();
            Ok(Some((tip.to_string(), vec![
                Change::Add { path: format!("registry/{host}.json"), bytes: reg },
                Change::Add { path: format!("ack/{host}.json"), bytes: ack },
            ])))
        })
    })?;
    sync::run_with(env, ws_id, Mode::Cli, &Opts::default())
}

/// S§7.6 `zygo mail sync --new-identity` (D6): deletes `mail/sync/identity` and clears the recorded blob ids
/// (`pushed`, `pending`) under the lock, then the sync, whose pass makes the new identity. It deletes no branch file.
pub fn new_identity(env: &Env, ws_id: &str) -> Result<Report, SyncError> {
    sync::locked(env, ws_id, |c| {
        match std::fs::remove_file(&c.sp.identity) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(stop("identity")),
            _ => {}
        }
        let mut st = state::load(&c.sp).map_err(|_| stop("state"))?;
        st.pushed.clear();
        st.pending.clear();
        state::save(&c.sp, &st).map_err(|_| stop("state"))
    })?;
    sync::run_with(env, ws_id, Mode::Cli, &Opts::default())
}

/// The top-level folders `<addr>` of `tip` whose address host is `host`.
fn host_folders(wt: &Path, tip: &str, host: &str, deadline: Instant) -> Result<Vec<String>, BranchError> {
    let mut folders = Vec::new();
    branch::ls(wt, tip, None, deadline, &mut |e| {
        if e.kind == "tree" && address::parse(&e.path).is_some_and(|a| a.host == host) {
            folders.push(e.path);
        }
        Flow::Go
    })?;
    Ok(folders)
}

/// D3: the paths `<addr>/<ULID>.json` directly under the folders of `host` at `tip` that are message files (S§9
/// "Tree", [`prune::removable`]) and that the valid `ack/<host>.json` does not list (an invalid or missing ack lists
/// none). A link, a submodule or a tree is not counted: it is no message (FX-27 review F3).
fn waiting(wt: &Path, tip: &str, meta: &TipMeta, host: &str, deadline: Instant)
    -> Result<BTreeSet<String>, BranchError> {
    let acked: BTreeSet<Ulid> = meta.acks.get(host).and_then(|r| r.value.as_ref().ok())
        .map(|a| a.imported.clone()).unwrap_or_default();
    let mut out = BTreeSet::new();
    for folder in host_folders(wt, tip, host, deadline)? {
        let prefix = format!("{folder}/");
        branch::ls(wt, tip, Some(&prefix), deadline, &mut |e| {
            let unlisted = e.path.strip_prefix(&prefix).and_then(address::ulid_file)
                .is_some_and(|u| !acked.contains(&u));
            if prune::removable(&e) && unlisted {
                out.insert(e.path);
            }
            Flow::Go
        })?;
    }
    Ok(out)
}

/// `git <args>` in `wt` on a zero exit; else `Git(_, step)`.
fn git_ok(wt: &Path, args: &[&str], step: &'static str, deadline: Instant) -> Result<Vec<u8>, BranchError> {
    match crate::git::run(wt, args, deadline) {
        Ok(w) if w.ok => Ok(w.out),
        Ok(_) => Err(BranchError::Git(RunError::Failed, step)),
        Err(e) => Err(BranchError::Git(e, step)),
    }
}

/// The tree of `tip` without `paths` and everything under them, at any depth and whatever the bytes of the names
/// (`git rm -r --cached` on the worktree's index, literal pathspecs; one call per [`branch::path_chunks`] run).
fn tree_without(wt: &Path, tip: &str, paths: &[String], deadline: Instant) -> Result<String, BranchError> {
    git_ok(wt, &["read-tree", tip], "read-tree", deadline)?;
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    for chunk in branch::path_chunks(&paths) {
        let mut args = vec!["--literal-pathspecs", "rm", "-r", "-f", "-q", "--cached", "--ignore-unmatch", "--"];
        args.extend(chunk);
        git_ok(wt, &args, "rm", deadline)?;
    }
    let out = git_ok(wt, &["write-tree"], "write-tree", deadline)?;
    let tree = String::from_utf8_lossy(&out).trim_end().to_string();
    if tree.is_empty() || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(BranchError::Git(RunError::Failed, "write-tree"));
    }
    Ok(tree)
}

/// D4: `host` in the S§5 host form, else the `host` stop, before the gate and the lock, with no `errors.log` line.
fn check_host(host: &str) -> Result<(), SyncError> {
    if zygo_config::valid_mail_host(host) {
        return Ok(());
    }
    Err(SyncError::Stopped { line: "zygo: mail sync stopped (host); the outbox is kept".into(), check: "host" })
}

/// S§7.6 `--drop-host`: the question `drop_host` asks.
pub fn drop_prompt(n: usize, host: &str) -> String {
    format!("zygo: {n} messages wait for host {host}; they will be deleted. Delete them? [y/N]")
}

/// The [`waiting`] set of the fetched tip, after the public check (D7); empty when there is no branch.
fn waiting_set(env: &Env, ws_id: &str, host: &str) -> Result<BTreeSet<String>, SyncError> {
    check_host(host)?;
    sync::locked(env, ws_id, |c| {
        let deadline = Instant::now() + PASS_DEADLINE;
        sync::public_check(c, &mut Report::default(), deadline)?;
        let Some((tip, meta)) = fetch_tip(c, deadline)? else { return Ok(BTreeSet::new()) };
        Ok(waiting(&c.sp.worktree, &tip, &meta, host, deadline)?)
    })
}

/// S§7.6, S§7.4 (D3): after the public check (D7), the count of message files `<addr>/<ULID>.json` (S§9 "Tree") at
/// the tip of the folders of `host` that its valid ack does not list (an invalid or missing ack lists none); 0 when
/// there is no branch.
pub fn waiting_for(env: &Env, ws_id: &str, host: &str) -> Result<usize, SyncError> {
    Ok(waiting_set(env, ws_id, host)?.len())
}

/// S§7.6 `zygo mail sync --drop-host <host>` (D5): counts as [`waiting_for`], then asks [`drop_prompt`] with the
/// lock released. On yes, under the lock, when the waiting files at the tip it then fetches are the counted ones,
/// one commit deletes `registry/<host>.json`, `ack/<host>.json` and everything under the `*@<host>/` folders; when
/// they differ, nothing is deleted and it asks again with the new count (S-G5). `Ok(None)`: an answer was no and
/// nothing changed; `Ok(Some(n))`: deleted, `n` the count of the answer that deleted.
pub fn drop_host(env: &Env, ws_id: &str, host: &str, ask: &mut dyn FnMut(&str) -> bool)
    -> Result<Option<usize>, SyncError> {
    let mut counted = waiting_set(env, ws_id, host)?;
    loop {
        if !ask(&drop_prompt(counted.len(), host)) {
            return Ok(None);
        }
        let mut changed = None;
        sync::locked(env, ws_id, |c| {
            let deadline = Instant::now() + PASS_DEADLINE;
            sync::public_check(c, &mut Report::default(), deadline)?;
            let identity = state::make_identity(&c.sp).map_err(|_| stop("identity"))?;
            let wt = &c.sp.worktree;
            write(c, &identity, deadline, &mut |tip, meta| {
                let now = waiting(wt, tip, meta, host, deadline)?;
                if now != counted {
                    changed = Some(now);
                    return Ok(None);
                }
                let mut paths = host_folders(wt, tip, host, deadline)?;
                if paths.is_empty() && !meta.registries.contains_key(host) && !meta.acks.contains_key(host) {
                    return Ok(None);
                }
                paths.extend([format!("registry/{host}.json"), format!("ack/{host}.json")]);
                Ok(Some((tree_without(wt, tip, &paths, deadline)?, Vec::new())))
            })
        })?;
        match changed {
            Some(now) => counted = now,
            None => return Ok(Some(counted.len())),
        }
    }
}
