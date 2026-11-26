//! Repair of a state that a dead `up` or `down` left. Owner: node W7 (W§8.1 step 5, AU67, AU75).
//!
//! `repair` runs no `snapshot.pending` check after its writes: it has no `Paths`, so its caller runs
//! `snapshot::after_write` right after it returns (W§7.3, finding 7).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, bail};

use crate::mail::{self, MAIL_LOCK_WAIT, not_pruned_line};
use crate::mux::{check_path, parse_pane_line, Multiplexer, MuxError, SessionAnswer, UnsafePath};
use crate::snapshot::OP_WAIT;
use crate::state::{Pane, PaneKind, Phase, StateError, Store, Workspace};
use crate::up::{
    member_live, no_build_plan_line, not_built_line, not_owned, socket_is_private, take_back_plan, unreachable, BuildPane,
    BuildWindow, Builder, LOADOUT_SIZE,
};

const WS_OPTION: &str = "@zygo-ws";

fn tmux(e: MuxError) -> anyhow::Error { anyhow!("tmux: {e}") }

/// `phase = up`; the write that sets it clears `build_plan` and `fresh_members` (Round 3 R3-2).
fn set_up(c: &mut Workspace) {
    c.phase = Phase::Up;
    c.build_plan = None;
    c.fresh_members.clear();
}

/// One write under `lock` with the wait of `up` and `down` (D20).
fn write(store: &Store, name: &str, f: &mut dyn FnMut(&mut Workspace)) -> anyhow::Result<Workspace> {
    store
        .update(OP_WAIT, &mut |ws| {
            f(ws);
            Ok(())
        })
        .map_err(|e| match e {
            StateError::Busy => anyhow!("workspace {name} is busy; try again"),
            e => anyhow!(e),
        })
}

/// The AU63 check before any change to a session that exists: `@zygo-ws` must be the id (B1).
fn owned(mux: &dyn Multiplexer, ws: &Workspace) -> anyhow::Result<()> {
    match mux.show_session_option(&ws.tmux_session, WS_OPTION).map_err(tmux)? {
        Some(v) if v == ws.id => Ok(()),
        _ => bail!(not_owned(&ws.tmux_session, &ws.name)),
    }
}

/// "Gone" needs no live launcher (AU87): a gone session with a live launcher is unreachable.
fn gone(store: &Store, mux: &dyn Multiplexer, ws: &Workspace) -> anyhow::Result<()> {
    match store.live_launcher(ws) {
        Some(key) => bail!(unreachable(&ws.name, &key, mux.socket())),
        None => Ok(()),
    }
}

/// FIX-29 rounds 2-3 (AU54): the saved `tmux_socket` of a state, in every phase, must pass [`check_path`], as the
/// read socket does in W§8.1 step 9, because the dead-pane lines and the AU100 attach line quote it. Every caller
/// runs it before any use of the socket (`socket_is_private`, the tmux calls, a printed line): `up` (step 5), `up
/// --all` (its session check), `zygo attach`, `down` and `repair`. FIX-30: also `zygo ls` (the row is `unreachable`),
/// the snapshot (W§7.4 step 1) and the hand start of the crate `zygo` (W§8.4), so it is `pub`.
pub fn check_saved_socket(ws: &Workspace) -> Result<(), UnsafePath> {
    match &ws.tmux_socket {
        Some(s) => check_path(s).map(|_| ()),
        None => Ok(()),
    }
}

/// FIX-29 round 3: the exit code and the line of a [`repair`] error, for `up` and `down`: [`UnsafePath`] gives exit 2
/// and its text once (it holds the `zygo: ` prefix, AU54); any other error gives exit 1 and `zygo: <error>` with its
/// causes.
pub(crate) fn repair_failure(e: &anyhow::Error) -> (i32, String) {
    match e.downcast_ref::<UnsafePath>() {
        Some(u) => (2, u.to_string()),
        None => (1, format!("zygo: {e:#}")),
    }
}

/// Repairs the state of a dead `up` or `down` (W§8.1 step 5, D20). The caller holds `op.lock`. No state, or phase
/// `up` or `down`, makes no tmux call. The AU96 and layout lines go to `err`. `Err` holds the text without the
/// `zygo: ` prefix, except a saved socket that fails AU54: that `Err` is [`UnsafePath`] itself, before any tmux call.
///
/// After a repair that wrote, the maildirs of the records it dropped are removed under `mail/lock` (M§5 P1): the
/// loadout take-back of a dead build and the panes that did not fit drop records. A removal that fails prints one line
/// to `err`; the next removal tries again. In phase `up` or `down` it runs the same removal, best effort (an error or a
/// held lock is ignored and leaves the next removal to try again), so a maildir that a failed removal left does not
/// stay.
pub fn repair(store: &Store, mux: &dyn Multiplexer, zygo: &Path, _out: &mut dyn Write, err: &mut dyn Write) -> anyhow::Result<()> {
    let Some(ws) = store.read()? else {
        return Ok(());
    };
    if matches!(ws.phase, Phase::Up | Phase::Down) {
        let _ = mail::prune(store, MAIL_LOCK_WAIT);
        return Ok(());
    }
    repair_phase(store, mux, zygo, ws, err)?;
    if let Err(e) = mail::prune(store, MAIL_LOCK_WAIT) {
        let _ = writeln!(err, "{}", not_pruned_line(&e));
    }
    Ok(())
}

/// The repair of a state in phase `building`, `draining` or `stopping` (W§8.1 step 5).
fn repair_phase(store: &Store, mux: &dyn Multiplexer, zygo: &Path, ws: Workspace, err: &mut dyn Write) -> anyhow::Result<()> {
    check_saved_socket(&ws)?;
    let answer = mux.session_exists(&ws.tmux_session);
    if let SessionAnswer::Error { message } = &answer {
        bail!("tmux: {message}");
    }
    let exists = answer == SessionAnswer::Exists;
    if !exists {
        gone(store, mux, &ws)?;
    }
    match ws.phase {
        Phase::Building if exists => {
            owned(mux, &ws)?;
            building(store, mux, zygo, &ws, err)
        }
        Phase::Draining if exists => {
            write(store, &ws.name, &mut set_up)?;
            Ok(())
        }
        Phase::Stopping if exists => {
            owned(mux, &ws)?;
            let live: BTreeSet<String> = ws.members.keys().filter(|k| member_live(store, &ws, k)).cloned().collect();
            if live.is_empty() {
                mux.kill_session(&ws.tmux_session).map_err(tmux)?;
                write(store, &ws.name, &mut |c| c.phase = Phase::Down)?;
            } else {
                write(store, &ws.name, &mut |c| {
                    set_up(c);
                    for k in &live {
                        if let Some(m) = c.members.get_mut(k) {
                            m.clean_stop = false;
                            m.closed_units.clear();
                            m.interrupted.clear();
                            m.saved_for = None; // AU75: no saved values, so no session they were saved for
                        }
                    }
                })?;
            }
            Ok(())
        }
        Phase::Building => {
            // Round 3 R3-2: the saved `build_plan` says what the dead build wrote: a loadout plan is taken back, so the
            // next `up` plans from the loadout again; a restore plan keeps the layout; no plan (an older zygo) keeps
            // everything, with one line.
            let mut older = false;
            write(store, &ws.name, &mut |c| {
                c.phase = Phase::Down;
                older = !take_back_plan(c);
            })?;
            if older {
                let _ = writeln!(err, "{}", no_build_plan_line(&ws.name));
            }
            Ok(())
        }
        _ => {
            write(store, &ws.name, &mut |c| c.phase = Phase::Down)?;
            Ok(())
        }
    }
}

/// AU67 on a session that exists and is ours: the session setup again, then each missing pane as in W§8.1 step 11,
/// with the build hold on every window (FIX-29 round 2); then, as `up` does in step 12, the release of the hold and
/// the read of the dead panes, `phase = up` with the panes that did not fit removed (M1), and one line per dead pane.
fn building(store: &Store, mux: &dyn Multiplexer, zygo: &Path, ws: &Workspace, err: &mut dyn Write) -> anyhow::Result<()> {
    let b =
        Builder { mux, session: &ws.tmux_session, zygo, id: &ws.id, main_root: &ws.main_root, dir: store.dir(), hold: true };
    b.session_setup(true).map_err(tmux)?;
    let mut present: BTreeMap<String, u32> = BTreeMap::new();
    let mut windows: BTreeSet<u32> = BTreeSet::new();
    for line in mux.list_panes(&ws.tmux_session).map_err(tmux)? {
        let (w, p) = parse_pane_line(&line).map_err(|e| anyhow!("{e}: {line:?}"))?;
        windows.insert(w.index);
        if crate::identity::valid_pane_key(&p.key) {
            present.insert(p.key, w.index);
        }
    }
    // FIX-29 round 3: from the first `window_setup` on, every exit path ends the hold. On an error, release (best
    // effort) each window that the hold may cover: the windows set so far, and each window of the session that was
    // not there before the repair (a window that `add_window` made before it failed); then return the error.
    let before = windows.clone();
    let mut held: BTreeSet<u32> = BTreeSet::new();
    match held_build(&b, store, mux, ws, &present, windows, &mut held, err) {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Ok(lines) = mux.list_panes(&ws.tmux_session) {
                held.extend(lines.iter().filter_map(|l| parse_pane_line(l).ok()).map(|(w, _)| w.index).filter(|w| !before.contains(w)));
            }
            b.release(&held.into_iter().collect::<Vec<_>>(), &ws.name, err);
            Err(e)
        }
    }
}

/// The part of [`building`] under the build hold. `held` gets each window before its hold is set.
#[allow(clippy::too_many_arguments)]
fn held_build(
    b: &Builder<'_>, store: &Store, mux: &dyn Multiplexer, ws: &Workspace, present: &BTreeMap<String, u32>,
    mut windows: BTreeSet<u32>, held: &mut BTreeSet<u32>, err: &mut dyn Write,
) -> anyhow::Result<()> {
    for w in &windows {
        held.insert(*w);
        b.window_setup(*w).map_err(tmux)?;
    }
    let mut groups: BTreeMap<u32, Vec<&Pane>> = BTreeMap::new();
    for p in ws.panes.iter().filter(|p| !present.contains_key(&p.key)) {
        groups.entry(p.window).or_default().push(p);
    }
    let bp = |p: &Pane| BuildPane { key: p.key.clone(), shell: p.kind == PaneKind::Shell, cwd: p.cwd.clone(), active: p.active };
    let mut not_built: Vec<String> = Vec::new();
    for (stored, group) in &groups {
        let saved = ws.windows.iter().find(|w| w.index == *stored);
        let real = ws.panes.iter().filter(|p| p.window == *stored).find_map(|p| present.get(&p.key)).copied();
        let (real, rest) = match real {
            Some(r) => (r, &group[..]),
            None => {
                let w = BuildWindow {
                    name: saved.and_then(|w| w.name.clone()).filter(|n| !n.is_empty()),
                    layout: None,
                    width: saved.map_or(LOADOUT_SIZE.0, |w| w.width),
                    height: saved.map_or(LOADOUT_SIZE.1, |w| w.height),
                    active: false,
                    zoomed: false,
                    panes: vec![bp(group[0])],
                };
                let (index, _) = b.add_window(&w).map_err(tmux)?;
                held.insert(index);
                windows.insert(index);
                (index, &group[1..])
            }
        };
        for p in rest {
            if b.split(real, &bp(p)).map_err(tmux)?.is_none() {
                not_built.push(p.key.clone());
                let _ = writeln!(err, "{}", not_built_line(&p.key, ws.members.get(&p.key)));
            }
        }
        b.layout(real, saved.map(|w| w.layout.as_str()), err).map_err(tmux)?;
    }
    // The socket of a session whose `up` died before it wrote `tmux_socket` (implementation decision I3).
    let socket = match &ws.tmux_socket {
        Some(_) => None,
        None => {
            let s = mux.socket_path(&ws.tmux_session).map_err(tmux)?;
            check_path(&s).map_err(|e| anyhow!("{}", e.to_string().trim_start_matches("zygo: ")))?;
            socket_is_private(&s, false).map_err(|t| anyhow!(t))?;
            Some(s)
        }
    };
    // FIX-29 round 2: as `up` in W§8.1 step 12: end the build hold on every window of the session (best effort), read
    // the dead panes (of the stopped build and of this repair), write `phase = up`, then print one line per dead pane.
    let windows: Vec<u32> = windows.into_iter().collect();
    b.release(&windows, &ws.name, err);
    let exited = match socket.clone().or_else(|| ws.tmux_socket.clone()) {
        Some(sock) => b.dead_lines(&sock, &ws.name, err),
        None => Vec::new(),
    };
    write(store, &ws.name, &mut |c| {
        set_up(c);
        if let Some(s) = &socket {
            c.tmux_socket = Some(s.clone());
        }
        if !not_built.is_empty() {
            c.panes.retain(|p| !not_built.contains(&p.key));
            c.snapshot_seq += 1;
        }
    })?;
    for line in &exited {
        let _ = writeln!(err, "{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::UNSAFE_PATH_MESSAGE;

    #[test] // FIX-29 diff review round 3, finding 4: an `UnsafePath` from repair is the AU54 line once, exit 2
    fn repair_failure_maps_unsafe_path_to_exit_2_and_other_errors_to_exit_1() {
        assert_eq!(repair_failure(&anyhow::Error::new(UnsafePath)), (2, UNSAFE_PATH_MESSAGE.to_string()));
        assert_eq!(repair_failure(&anyhow!("tmux: injected")), (1, "zygo: tmux: injected".to_string()));
        let chained = anyhow!("inner").context("outer");
        assert_eq!(repair_failure(&chained), (1, "zygo: outer: inner".to_string()));
    }
}
