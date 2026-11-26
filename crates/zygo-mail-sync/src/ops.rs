//! The operator commands of `zygo mail sync` (mail sync design S§7.6): `--take-host`, `--new-identity` and
//! `--drop-host`. Another host's files go only by its ack or by `--drop-host` (S-C8); a drop counts and asks (S-G5).
use std::time::Instant;

use crate::branch::{self, Change, Fetched, Pushed};
use crate::env::Env;
use crate::identity::{read_meta, TipMeta};
use crate::registry::{Ack, Registry, MEMBERS_MAX};
use crate::state;
use crate::sync::{self, own_blobs, own_names, stop, Ctx, Fail, Mode, Opts, Report, SyncError, MARKER_LINE,
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

/// S§8.3 steps 13-15 (D2): the changes `build` gives for the fetched tip in one commit on it, pushed without force.
/// The own blobs of the commit are `pending` before the push and `pushed` after it. A moved branch fetches again,
/// at most [`MAX_ATTEMPTS`] times, then [`MOVED_LINE`]. No branch or no change writes nothing.
fn write(c: &Ctx, identity: &str, deadline: Instant,
    build: &mut dyn FnMut(&str, &TipMeta) -> Result<Vec<Change>, Fail>) -> Result<(), Fail> {
    let wt = &c.sp.worktree;
    for _ in 0..MAX_ATTEMPTS {
        let Some((tip, meta)) = fetch_tip(c, deadline)? else { return Ok(()) };
        let changes = build(&tip, &meta)?;
        if changes.is_empty() {
            return Ok(());
        }
        let commit = branch::commit(wt, &tip, Some(&tip), &changes, deadline)?;
        let mut st = state::load(&c.sp).map_err(|_| stop("state"))?;
        st.pending = own_blobs(&read_meta(wt, &commit, deadline, &mut |_| {})?, identity);
        state::save(&c.sp, &st).map_err(|_| stop("state"))?;
        match branch::push(wt, &c.s.remote, &c.s.branch, &commit, None, deadline)? {
            Pushed::Accepted => {
                st.pushed.extend(std::mem::take(&mut st.pending));
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
/// host, whatever its identity (D1). Then the sync. The old owner's next pass stops with the collision line.
pub fn take_host(env: &Env, ws_id: &str) -> Result<Report, SyncError> {
    sync::locked(env, ws_id, |c| {
        let deadline = Instant::now() + PASS_DEADLINE;
        sync::public_check(c, &mut Report::default(), deadline)?;
        let identity = state::make_identity(&c.sp).map_err(|_| stop("identity"))?;
        let host = c.s.host.as_str();
        write(c, &identity, deadline, &mut |_, meta| {
            let reg = meta.registries.get(host).and_then(|r| r.value.as_ref().ok());
            let ack = meta.acks.get(host).and_then(|r| r.value.as_ref().ok());
            if reg.is_some_and(|r| r.identity == identity) && ack.is_some_and(|a| a.identity == identity) {
                return Ok(Vec::new());
            }
            let mut out = Vec::new();
            let members = own_names(&c.ws);
            if members.len() > MEMBERS_MAX {
                c.log("mail sync: own file over limit: members");
            } else {
                let bytes = Registry { host: host.into(), identity: identity.clone(), members }.to_bytes();
                out.push(Change::Add { path: format!("registry/{host}.json"), bytes });
            }
            let imported = ack.map(|a| a.imported.clone()).unwrap_or_default();
            let bytes = Ack { host: host.into(), identity: identity.clone(), imported }.to_bytes();
            out.push(Change::Add { path: format!("ack/{host}.json"), bytes });
            Ok(out)
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
