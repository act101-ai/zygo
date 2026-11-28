//! The sync of one workspace (mail sync design S§8.3): gate, mark, lock, passes, push retry, release and look again.
//! A pass imports (S§8.5), wakes (S§10.1), commits the outbox after the secret pass (S§8.6 "At commit") and the own
//! files (S§7.3, S§7.4), pushes, and prunes (S§9). Every `errors.log` line holds a ULID, pattern ids or a check name only (S-C2).
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ulid::Ulid;
use zygo_config::{sync_for, Config, LoadContext, SyncOff, SyncSettings};
use zygo_work::repo::Flow;
use zygo_workspace::mail::{names, store, Delivery, MailError, Message, MAIL_LOCK_WAIT};
use zygo_workspace::state::{lock_is_at, Store, Workspace};

use crate::address;
use crate::branch::{self, BranchError, Change, Fetched, Pushed};
use crate::env::{Clock, Crash, Env, Verdict};
use crate::identity::{self, own_hosts, read_meta, TipMeta};
use crate::import::{self, Target};
use crate::outbox::NoticeError;
use crate::prune::{self, Pruned};
use crate::public;
use crate::secret::{self, Outcome};
use crate::registry::{write_cache, Ack, MemberCache, Registry, ACK_ENTRIES_MAX, MEMBERS_MAX};
use crate::state::{self, State, SyncPaths};

/// S§8.3 step 4: the deadline of one pass.
pub const PASS_DEADLINE: Duration = Duration::from_secs(120);
/// S§8.3 step 15: the push attempts of one pass.
pub const MAX_ATTEMPTS: u32 = 5;
/// S§8.3 step 11: the outbox files one pass reads.
pub const OUTBOX_CAP: usize = 1000;
/// S§8.3 step 3: how long the CLI waits for the lock.
pub const LOCK_WAIT: Duration = Duration::from_secs(30);

/// Who runs the sync: the child a hook starts, or `zygo mail sync`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Child,
    Cli,
}

/// What one sync did: the passes run, the outbox files pushed, the messages delivered and the notes for the CLI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub passes: u32,
    pub pushed: usize,
    pub delivered: usize,
    pub notes: Vec<String>,
    /// S§6.6: this `Mode::Cli` sync is the first of its workspace that ends without a stop; the CLI prints the note.
    pub ci_note: bool,
}

/// What a caller asks of one sync beyond S§8.3.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Opts {
    /// `zygo mail sync --prune` (S§9 When): the first pass prunes at step 16 even when not due.
    pub prune: bool,
}

/// Why a sync did not run to its end. `Stopped.line` is the CLI line; only `check` goes to `errors.log` (S-C2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    Off(SyncOff),
    Busy,
    Stopped { line: String, check: &'static str },
}

impl SyncError {
    /// S§8.3 steps 1 and 3: the gate (sync off, or a workspace record or config that cannot be read: checks
    /// `workspace` and `config`) and the lock give exit 0 to a child and 1 to the CLI; every other stop gives 1
    /// (S§7.5, S§8.3 steps 7 and 15).
    pub fn exit_code(&self, mode: Mode) -> i32 {
        match (self, mode) {
            (SyncError::Off(_) | SyncError::Busy, Mode::Child) => 0,
            (SyncError::Stopped { check: "workspace" | "config", .. }, Mode::Child) => 0,
            _ => 1,
        }
    }
}

/// S§8.3 step 7: the line of a tip without the marker.
pub(crate) const MARKER_LINE: &str = "zygo: the side branch is not a zygo mail branch";
/// S§8.2: the line of a public remote.
const PUBLIC_LINE: &str = "zygo: the remote can be read without a login; mail sync refused";
/// S§11: the line of a `remote` that `git remote` does not list.
const REMOTE_LINE: &str = "zygo: no such remote";
/// S§8.3 step 1: the CLI lines of a gate failure (author's text; review F5).
const GATE_RECORD_LINE: &str = "zygo: mail sync cannot run: the workspace record cannot be read";
const GATE_CONFIG_LINE: &str = "zygo: mail sync cannot run: the config does not load";
/// S§8.3 step 3: the CLI line of a sync whose workspace was forgotten while it waited for the lock (MS-21 review F1).
const FORGOTTEN_LINE: &str = "zygo: the workspace was forgotten; nothing synced";
/// S§8.3 step 15: the line after the fifth move of the branch.
pub(crate) const MOVED_LINE: &str = "zygo: the side branch moved 5 times during the sync; try again";
/// S§9 Push: the note of a forced prune whose lease failed.
const PRUNE_MOVED_NOTE: &str = "prune skipped: the branch moved; run again";
/// The poll of a lock that another holder has, as the workspace lock helper.
const LOCK_POLL: Duration = Duration::from_millis(5);

/// One sync: the seams, the paths, the workspace record and the settings in force.
pub(crate) struct Ctx<'a> {
    pub(crate) env: &'a Env,
    pub(crate) ws_id: &'a str,
    pub(crate) sp: SyncPaths,
    pub(crate) store: Store,
    pub(crate) ws: Workspace,
    /// The state folder `<workspaces_dir>/<id>/` that the gate read the record in, held open so its inode is not
    /// used again while this sync runs (unix; `None` elsewhere): a forget and a new workspace of the same id make a
    /// new folder (MS-21 review R2-2).
    state_dir: Option<File>,
    pub(crate) cfg: Config,
    pub(crate) s: SyncSettings,
}

/// Why a pass stopped: a stop with its CLI line and check name, or a test seam.
pub(crate) enum Fail {
    Stop { line: String, check: &'static str },
    Crash(&'static str),
}

impl From<Crash> for Fail {
    fn from(c: Crash) -> Fail { Fail::Crash(c.0) }
}

impl From<BranchError> for Fail {
    fn from(e: BranchError) -> Fail { stop(branch_check(&e)) }
}

/// FX-40 review, S§8.3 step 15, S§9 Push: the `push` stop of a push or prune that one push URL took and another did
/// not. It first sets `push_split` in `state.json` (under the caller's sync lock, atomic write); a state that cannot be
/// read or written is the `state` stop.
pub(crate) fn split_stop(sp: &SyncPaths) -> Fail {
    let Ok(mut st) = state::load(sp) else { return stop("state") };
    st.push_split = true;
    if state::save(sp, &st).is_err() {
        return stop("state");
    }
    stop("push")
}

/// A stop with the author's line (Open O6): the check name and that the outbox is kept.
pub(crate) fn stop(check: &'static str) -> Fail {
    Fail::Stop { line: format!("zygo: mail sync stopped ({check}); the outbox is kept"), check }
}

/// The check name of a branch error: the git step with each space as `-`, `io` or `refused`.
fn branch_check(e: &BranchError) -> &'static str {
    match e {
        BranchError::Git(_, "worktree add") => "worktree-add",
        BranchError::Git(_, "worktree remove") => "worktree-remove",
        BranchError::Git(_, step) => step,
        BranchError::Io(_) => "io",
        BranchError::Refused(_) => "refused",
    }
}

/// S§8.3: one sync of workspace `ws_id`: gate (step 1), mark (step 2), then passes under the lock (steps 3-16)
/// until the look after a release finds no mark (step 17).
pub fn run(env: &Env, ws_id: &str, mode: Mode) -> Result<Report, SyncError> {
    run_with(env, ws_id, mode, &Opts::default())
}

/// [`run`] with `opts`: `prune` forces the prune of the first pass (S§9 When).
pub fn run_with(env: &Env, ws_id: &str, mode: Mode, opts: &Opts) -> Result<Report, SyncError> {
    sync_passes(env, ws_id, mode, opts, PASS_DEADLINE)
}

/// Test seam (S-C9; FX-26 review round 2 F-3, R3-5): [`run_with`] with `pass_deadline` in place of [`PASS_DEADLINE`] as
/// the deadline of each pass (S§8.3 step 4), on the `Instant` and on the injected clock, so that a test whose result
/// must not depend on how fast the machine runs git can give a far deadline, as the secret pass takes
/// `Env::secret_policy`. Only the `test-hooks` feature builds it; production code calls [`run_with`].
#[cfg(feature = "test-hooks")]
#[doc(hidden)]
pub fn run_with_deadline(env: &Env, ws_id: &str, mode: Mode, opts: &Opts, pass_deadline: Duration)
    -> Result<Report, SyncError> {
    sync_passes(env, ws_id, mode, opts, pass_deadline)
}

/// [`run_with`] with the deadline of each pass as an argument ([`PASS_DEADLINE`] in production).
fn sync_passes(env: &Env, ws_id: &str, mode: Mode, opts: &Opts, pass_deadline: Duration) -> Result<Report, SyncError> {
    let c = gate(env, ws_id)?;
    env.seam.at("after_gate").map_err(|e| c.stopped(e.into(), false))?;
    if state::mark_again(&c.sp).is_err() {
        // MS-21 review R2-1: the mark makes no state folder; with none, a forget ran after the gate.
        if c.state_gone() {
            return forgotten(mode, Report::default());
        }
        return Err(c.stopped(stop("again"), false));
    }
    let mut report = Report::default();
    let mut first = true;
    // S§6.6: a `Mode::Cli` sync has not yet looked at the CI note.
    let mut note_open = mode == Mode::Cli;
    loop {
        let lock = match take_lock(&c.sp, mode, &*env.clock) {
            Ok(Some(f)) => f,
            // A pass ran: the holder of the lock owns the mark and runs the next pass (S§8.3 step 17).
            Ok(None) if report.passes > 0 => return Ok(report),
            Ok(None) => return Err(SyncError::Busy),
            Err(_) if c.state_gone() => return forgotten(mode, report),
            Err(f) => return Err(c.stopped(f, false)),
        };
        match c.recheck(&lock) {
            Recheck::Same => {}
            Recheck::Forgotten => return forgotten(mode, report),
            Recheck::Unreadable => return Err(record_error()),
        }
        let held = (|| {
            state::take_again(&c.sp).map_err(|_| stop("again"))?;
            pass(&c, &mut report, first, first && opts.prune, pass_deadline)?;
            report.passes += 1;
            first = false;
            // S§6.6: the note goes with the last pass, the one that finds no mark under the lock.
            if note_open && !c.sp.again.exists() {
                note_open = false;
                ci_note(&c.sp, &mut report)?;
            }
            env.seam.at("before_release")?;
            Ok::<(), Fail>(())
        })();
        if let Err(f) = held {
            // A mark came after the look and this later pass stopped: the sync is not Ok, so the note is not used up.
            if report.ci_note {
                unsee_ci_note(&c.sp);
            }
            return Err(c.stopped(f, true));
        }
        drop(lock);
        env.seam.at("after_release").map_err(|e| c.stopped(e.into(), false))?;
        if !c.sp.again.exists() {
            return Ok(report);
        }
    }
}

/// S§6.6, S§15: after a pass of a `Mode::Cli` sync ends with no mark, under the lock (the only writer of
/// `state.json`): when `ci_note_shown` is false, it is set and saved, and `report.ci_note` is set, so the note prints
/// once per workspace. A child never calls this, so a child sync does not use the note up.
fn ci_note(sp: &SyncPaths, report: &mut Report) -> Result<(), Fail> {
    let mut st = state::load(sp).map_err(|_| stop("state"))?;
    if !st.ci_note_shown {
        st.ci_note_shown = true;
        state::save(sp, &st).map_err(|_| stop("state"))?;
        report.ci_note = true;
    }
    Ok(())
}

/// S§6.6, S§15: under the lock, a later pass of the sync that recorded the note stopped; `ci_note_shown` goes back
/// to false so the next sync that ends without a stop prints the note. A read or write error keeps the file as it is.
fn unsee_ci_note(sp: &SyncPaths) {
    if let Ok(mut st) = state::load(sp) {
        st.ci_note_shown = false;
        let _ = state::save(sp, &st);
    }
}

impl Ctx<'_> {
    /// The error of a stop. A stop writes `mail sync: stopped: <check>` to `errors.log`, never the line (S-C2), and,
    /// while the lock is held (the only writer of `state.json`, S§15), `last_error`. A seam stop writes nothing.
    fn stopped(&self, f: Fail, locked: bool) -> SyncError {
        match f {
            Fail::Crash(p) => SyncError::Stopped { line: format!("zygo: stopped at seam {p}"), check: "seam" },
            Fail::Stop { line, check } => {
                self.store.log_error(&format!("mail sync: stopped: {check}"));
                if locked && let Ok(mut st) = state::load(&self.sp) {
                    st.last_error = Some(check.to_string());
                    let _ = state::save(&self.sp, &st);
                }
                SyncError::Stopped { line, check }
            }
        }
    }

    pub(crate) fn log(&self, line: &str) { self.store.log_error(line) }

    /// S§8.3 step 3 (MS-21 review F1, R2-2, R2-3): under the lock `lock`, before any write. `Forgotten`: `zygo ws
    /// forget` ran while this sync waited: (unix) `lock` is no longer the file at `mail/sync/lock` (the forget
    /// unlinked the file this sync waited on), or the workspace record (read as the gate reads it) is gone or is not
    /// the gate's workspace (its id, name, common folder or main worktree differ, or, on unix, its state folder is a
    /// new one). `Unreadable`: the record cannot be read, as at the gate. A record change that keeps the workspace
    /// (its phase, its snapshot count, its members) is `Same`.
    fn recheck(&self, lock: &File) -> Recheck {
        if !lock_is_at(lock, &self.sp.lock) {
            return Recheck::Forgotten;
        }
        let same_dir = match &self.state_dir {
            Some(d) => file_id(d.metadata()).is_some_and(|id| file_id(std::fs::symlink_metadata(self.store.dir())) == Some(id)),
            None => true,
        };
        match self.store.read() {
            Err(_) => Recheck::Unreadable,
            Ok(Some(ws))
                if same_dir
                    && (&ws.id, &ws.name, &ws.common_dir, &ws.main_root)
                        == (&self.ws.id, &self.ws.name, &self.ws.common_dir, &self.ws.main_root) =>
            {
                Recheck::Same
            }
            Ok(_) => Recheck::Forgotten,
        }
    }

    /// S§8.3 steps 2-3 (MS-21 review R2-1): the state folder `<workspaces_dir>/<id>/` is gone, so `zygo ws forget`
    /// ran after the gate. The sync makes no mail folder in a missing state folder, so a failed mark or lock with
    /// this true is the forget, not a stop.
    fn state_gone(&self) -> bool {
        matches!(std::fs::symlink_metadata(self.store.dir()), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    }
}

/// S§8.3 step 3: what [`Ctx::recheck`] found under the lock.
enum Recheck {
    Same,
    Forgotten,
    Unreadable,
}

/// S§8.3 step 1: the stop of a workspace record that cannot be read (no write, exit 0 for a child).
fn record_error() -> SyncError { SyncError::Stopped { line: GATE_RECORD_LINE.into(), check: "workspace" } }

/// S§8.3 step 3 (MS-21 review F1): the stop of a CLI sync whose workspace was forgotten. It writes nothing: no
/// `errors.log` line and no `state.json` (the state folder is gone, and a write would make it again).
fn forgotten_error() -> SyncError { SyncError::Stopped { line: FORGOTTEN_LINE.into(), check: "forgotten" } }

/// S§8.3 step 3: the end of a sync whose workspace was forgotten: `report` with no line (child), or
/// [`forgotten_error`] (CLI).
fn forgotten(mode: Mode, report: Report) -> Result<Report, SyncError> {
    match mode {
        Mode::Child => Ok(report),
        Mode::Cli => Err(forgotten_error()),
    }
}

/// S§8.3 step 1: the workspace record, the config (layers 1-4, the main worktree as the repository) and the sync
/// settings. A gate failure takes no action, so nothing is written, not even an `errors.log` line: sync off is
/// `Off`; a workspace record or a config that cannot be read is `Stopped` with check `workspace` or `config`, which
/// [`SyncError::exit_code`] gives exit 0 for a child.
fn gate<'a>(env: &'a Env, ws_id: &'a str) -> Result<Ctx<'a>, SyncError> {
    let store = Store::open(&env.paths, ws_id);
    let record = record_error;
    let sp = SyncPaths::new(&env.paths, ws_id).map_err(|_| record())?;
    let Ok(Some(ws)) = store.read() else { return Err(record()) };
    let ctx = LoadContext { paths: env.paths.clone(), repo: Some(ws.main_root.clone()), team: None };
    let cfg = zygo_config::load(&ctx)
        .map_err(|_| SyncError::Stopped { line: GATE_CONFIG_LINE.into(), check: "config" })?
        .config;
    let s = sync_for(&cfg, &ws.name).map_err(SyncError::Off)?;
    let state_dir = open_state_dir(store.dir()).map_err(|_| record())?;
    Ok(Ctx { env, ws_id, sp, store, ws, state_dir, cfg, s })
}

/// MS-21 review R2-2: the state folder `dir` opened with no link followed (unix), or `None` (other platforms).
fn open_state_dir(dir: &Path) -> std::io::Result<Option<File>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut o = OpenOptions::new();
        o.read(true).custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC);
        o.open(dir).map(Some)
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(None)
    }
}

/// The device and inode of `meta` (unix); `None` for an error, and on every other platform.
fn file_id(meta: std::io::Result<std::fs::Metadata>) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

/// S§8.3 steps 1 and 3, S§15: `f` under the gate and the CLI lock. A lock held by another after the CLI wait is
/// `Busy`; a stop of `f` is logged and recorded as a stop of a pass. S§8.3 step 17: after the release, a mark that
/// a sync met the lock meanwhile runs the sync, as a child (a held lock: its holder sees the mark); its result is
/// in `errors.log` and `state.json` only.
pub(crate) fn locked<T>(env: &Env, ws_id: &str, f: impl FnOnce(&Ctx) -> Result<T, Fail>) -> Result<T, SyncError> {
    let c = gate(env, ws_id)?;
    env.seam.at("after_gate").map_err(|e| c.stopped(e.into(), false))?;
    let out = match take_lock(&c.sp, Mode::Cli, &*env.clock) {
        Ok(Some(lock)) => match c.recheck(&lock) {
            Recheck::Same => f(&c).map_err(|e| c.stopped(e, true)),
            Recheck::Forgotten => return Err(forgotten_error()),
            Recheck::Unreadable => return Err(record_error()),
        },
        Ok(None) => return Err(SyncError::Busy),
        Err(_) if c.state_gone() => return Err(forgotten_error()),
        Err(f) => return Err(c.stopped(f, false)),
    };
    if c.sp.again.exists() {
        let _ = run_with(env, ws_id, Mode::Child, &Opts::default());
    }
    out
}

/// S§8.3 step 3: the lock on `mail/sync/lock`. Held by another (`Ok(None)`): at once for a child; the CLI tries each
/// 5 ms until [`LOCK_WAIT`] has passed on the injected clock. A folder, open or lock error is the `lock` stop. A lock
/// file that is a link or not a regular file is never opened as the lock (no link followed, `O_NONBLOCK` so a FIFO
/// does not wait, the handle checked), so no sync holds it and `zygo ws forget` goes on without it (S§7.6 step 1,
/// FX-24 review R2 F4).
fn take_lock(sp: &SyncPaths, mode: Mode, clock: &dyn Clock) -> Result<Option<File>, Fail> {
    let fail = || stop("lock");
    for d in [&sp.mail, &sp.sync] {
        zygo_workspace::mail::ensure_mail_dir(d).map_err(|_| fail())?;
    }
    let mut o = OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let f = o.open(&sp.lock).map_err(|_| fail())?;
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return Err(fail());
    }
    let start = clock.now();
    loop {
        match fs4::FileExt::try_lock(&f) {
            Ok(()) => return Ok(Some(f)),
            Err(fs4::TryLockError::Error(_)) => return Err(fail()),
            Err(fs4::TryLockError::WouldBlock) => {
                if mode == Mode::Child || clock.now() >= start + LOCK_WAIT {
                    return Ok(None);
                }
                std::thread::sleep(LOCK_POLL);
            }
        }
    }
}

/// S§8.3 steps 5-16, with one deadline of `pass_deadline` ([`PASS_DEADLINE`] in production).
fn pass(c: &Ctx, report: &mut Report, first: bool, forced: bool, pass_deadline: Duration) -> Result<(), Fail> {
    let (env, sp, root, wt) = (c.env, &c.sp, &c.ws.main_root, &c.sp.worktree);
    let (remote, branch_name, host) = (c.s.remote.as_str(), c.s.branch.as_str(), c.s.host.as_str());
    let deadline = Instant::now() + pass_deadline;
    // The same deadline on the injected clock (S-C9), checked before each secret check of the outbox step.
    let pass_end = env.clock.now() + pass_deadline;
    let mut log = |l: &str| c.log(l);
    // FX-30: step 5's deliveries join the step 10 count and wake of the first round.
    let mut finished = import::finish_incoming(sp, &c.store).map_err(|_| stop("incoming"))?;
    // FX-30: each ULID that this pass counted, once across the rounds of the push loop.
    let mut counted = BTreeSet::<Ulid>::new();
    let mut carried: Option<BTreeMap<String, String>> = None;
    // FX-40, S§8.5: the ULIDs taken as candidates, one set over every push attempt of the pass (at most
    // `import::CANDIDATE_CAP`), and the ULIDs acknowledged by any attempt, which each later attempt's commit holds.
    let mut taken = BTreeSet::<Ulid>::new();
    let mut acked = BTreeSet::<Ulid>::new();
    if first {
        public_check(c, report, deadline)?;
    }
    let identity = state::make_identity(sp).map_err(|_| stop("identity"))?;
    let mut attempts = 0;
    let (base, base_meta) = loop {
        let (tip, fetched) = match branch::fetch(root, remote, branch_name, deadline)? {
            Fetched::Tip(t) => {
                branch::ensure_worktree(root, wt, &t, deadline)?;
                (t, true)
            }
            Fetched::NoBranch => (branch::bootstrap(root, wt, deadline)?, false),
        };
        env.seam.at("after_fetch")?;
        if !branch::has_marker(wt, &tip, deadline)? {
            return Err(Fail::Stop { line: MARKER_LINE.into(), check: "marker" });
        }
        let meta = read_meta(wt, &tip, deadline, &mut log)?;
        let mut st = state::load(sp).map_err(|_| stop("state"))?;
        // FX-30: the entries of an earlier run, read before this pass's first import records any.
        let first_round = carried.is_none();
        let carried = carried.get_or_insert_with(|| st.pending_wake.clone());
        identity::check(&meta, host, &identity, &st, &env.paths, c.ws_id)
            .map_err(|s| Fail::Stop { line: s.line(), check: s.check_name() })?;
        if promote(&mut st, &meta, &identity) {
            state::save(sp, &st).map_err(|_| stop("state"))?;
        }
        let mut cache = MemberCache::default();
        for (h, r) in &meta.registries {
            if let Ok(reg) = &r.value && reg.identity != identity {
                cache.hosts.insert(h.clone(), reg.members.clone());
            }
        }
        write_cache(sp, &cache).map_err(|_| stop("members"))?;
        let own = own_hosts(&meta, &identity, host);
        let own_ack: BTreeSet<Ulid> = own.iter()
            .filter_map(|h| own_ack(&meta, h, &identity))
            .flat_map(|a| a.imported.iter().copied())
            .collect();
        let targets: Vec<Target> = names::members(&c.ws).into_iter()
            .filter_map(|t| Some(Target { name: t.name?, pane_key: t.key }))
            .collect();
        let imp = import::run(sp, &c.store, wt, &tip, &own, &targets, &own_ack, &mut taken, c.cfg.mail.max_unread,
            deadline, &*env.seam, &mut log)?;
        acked.extend(imp.acked.iter().copied());
        // The import wrote its `pending_wake` entries to `state.json`; this round's later saves of `st` keep them.
        st.pending_wake = state::load(sp).map_err(|_| stop("state"))?.pending_wake;
        // S§8.3 step 10 (FX-30): the delivered set is step 5's result, the import's deliveries and, in the first round,
        // the carried entries whose message is in the member's maildir, each ULID once in the pass.
        let mut got = std::mem::take(&mut finished);
        got.extend(imp.delivered.iter().cloned());
        if first_round {
            got.extend(arrived(sp, carried));
        }
        got.retain(|(_, id)| counted.insert(*id));
        report.delivered += got.len();
        // S§10.1: the wake is for unread mail, so only a counted message whose file is in `new/` wakes its member.
        let mut keys: Vec<String> = got.into_iter().filter(|(k, id)| in_new(sp, k, *id)).map(|(k, _)| k).collect();
        keys.sort();
        keys.dedup();
        let lock_read = if imp.read.is_none() && !keys.is_empty() { wake_read(c) } else { None };
        wake(c, imp.read.as_ref().or(lock_read.as_ref()), &keys);
        clear_pending(sp, &mut st, &counted)?;
        let mut notified = Notified::default();
        // FX-40 review: while the push URLs may differ (`push_split`), a file at the tip is not taken as pushed.
        let split = st.push_split;
        let (mut changes, sent) = outbox(c, &tip, fetched.then_some(&meta), split, (deadline, pass_end),
            &mut notified)?;
        notified.keys.sort();
        notified.keys.dedup();
        wake(c, notified.read.as_ref(), &notified.keys);
        let files = own_files(wt, &tip, &own, deadline)?;
        let pass_acked: Vec<Ulid> = acked.iter().copied().collect();
        changes.extend(own_changes(c, &meta, &identity, &own, &files, &pass_acked, &own_ack));
        if changes.is_empty() && !split {
            break (tip, meta);
        }
        // FX-40 review, S§8.3 step 15: with the split flag set and no change, the tip itself goes to every push URL.
        let (commit, commit_meta) = if changes.is_empty() {
            (tip, meta)
        } else {
            let commit = branch::commit(wt, &tip, Some(&tip), &changes, deadline)?;
            let commit_meta = read_meta(wt, &commit, deadline, &mut |_| {})?;
            (commit, commit_meta)
        };
        st.pending = own_blobs(&commit_meta, &identity);
        state::save(sp, &st).map_err(|_| stop("state"))?;
        env.seam.at("before_push")?;
        match branch::push(wt, remote, branch_name, &commit, None, deadline)? {
            Pushed::Accepted => {
                env.seam.at("after_push")?;
                st.pushed.extend(std::mem::take(&mut st.pending));
                // Every push URL took this commit: they hold the same branch again.
                st.push_split = false;
                state::save(sp, &st).map_err(|_| stop("state"))?;
                branch::set_tracking(wt, remote, branch_name, &commit, deadline)?;
                env.seam.at("before_outbox_delete")?;
                for p in &sent {
                    let _ = std::fs::remove_file(p);
                }
                report.pushed += sent.len();
                break (commit, commit_meta);
            }
            // S§8.3 step 15: a new fetch, and steps 7-13 again on the new tip; no wait, no merge or rebase.
            Pushed::Moved => {
                attempts += 1;
                if attempts == MAX_ATTEMPTS {
                    return Err(Fail::Stop { line: MOVED_LINE.into(), check: "moved" });
                }
            }
            // FX-40 review F1: a push that some push URL took and another did not stops the pass as any other error,
            // and sets the split flag.
            Pushed::Split => return Err(split_stop(sp)),
            Pushed::Other(_) => return Err(stop("push")),
        }
    };
    prune_step(c, report, &base, &base_meta, &identity, forced, deadline)?;
    let mut st = state::load(sp).map_err(|_| stop("state"))?;
    forget_old_hosts(&mut st, &base_meta, &identity, host);
    // FX-30: `pending_wake` is as the step 10 writes of this pass left it (`clear_pending`); this write keeps it.
    st.last_ok = Some(env.clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    st.last_error = None;
    state::save(sp, &st).map_err(|_| stop("state"))
}

/// S§8.3 step 16, S§9 When and Base: the prune of `base` (this pass's push, else the fetched tip) when forced, when
/// the longest valid own ack of this identity has more than [`prune::OWN_ACK_DUE`] entries, or at
/// [`prune::COMMITS_DUE`] commits. A done prune adds the note `pruned <n> files`. A lost lease writes no line (the
/// next pass tries again, S§9), but a forced prune notes it. A failed count, commit or push skips the prune and adds
/// the note `prune skipped (<check>)`: the pass ends as without a prune (D11). A failed branch-name or `ZYGO-MAIL`
/// check stops the pass (S-C3). A root that one push URL took and another did not (`Split`) stops the pass with the
/// `push` stop (S§9 Push, FX-40 review F2).
fn prune_step(c: &Ctx, report: &mut Report, base: &str, meta: &TipMeta, identity: &str, forced: bool,
    deadline: Instant) -> Result<(), Fail> {
    // FX-40 review, S§9 Push: no prune while the push URLs may hold different branches.
    if state::load(&c.sp).map_err(|_| stop("state"))?.push_split {
        return Ok(());
    }
    let own_len = own_hosts(meta, identity, &c.s.host).iter()
        .filter_map(|h| own_ack(meta, h, identity))
        .map(|a| a.imported.len())
        .max()
        .unwrap_or(0);
    let pruned = match prune::due(&c.sp.worktree, base, own_len, forced, deadline) {
        Ok(false) => return Ok(()),
        Ok(true) => {
            c.env.seam.at("before_prune_push")?;
            prune::run(&c.sp.worktree, &c.s.remote, &c.s.branch, base, meta, deadline)
        }
        Err(e) => Err(e),
    };
    match pruned {
        Ok(Pruned::Done { removed }) => report.notes.push(format!("pruned {removed} files")),
        Ok(Pruned::LeaseLost) if forced => report.notes.push(PRUNE_MOVED_NOTE.into()),
        // FX-40 review F2: a root that some push URL took and another did not: the branch differs between the URLs.
        Ok(Pruned::Split) => return Err(split_stop(&c.sp)),
        Ok(_) => {}
        Err(e @ BranchError::Refused("branch" | "marker")) => return Err(e.into()),
        Err(e) => report.notes.push(format!("prune skipped ({})", branch_check(&e))),
    }
    Ok(())
}

/// S§8.3 step 5, S§8.2, S§11: the remote is one that `git remote` lists, then the public check of its URLs. `Public`
/// stops the sync before any fetch; `Unknown` is a note and the sync runs. It loads and saves `state.json` itself,
/// so the pass loads the state after it.
pub(crate) fn public_check(c: &Ctx, report: &mut Report, deadline: Instant) -> Result<(), Fail> {
    let root = &c.ws.main_root;
    let w = crate::git::run(root, &["remote"], deadline).map_err(|_| stop("remote"))?;
    if !w.ok {
        return Err(stop("remote"));
    }
    if !String::from_utf8_lossy(&w.out).lines().any(|l| l == c.s.remote) {
        return Err(Fail::Stop { line: REMOTE_LINE.into(), check: "remote" });
    }
    let urls = public::remote_urls(root, &c.s.remote, deadline).map_err(|_| stop("remote"))?;
    match public::check(&c.sp, &urls, &*c.env.probe, &*c.env.clock, deadline) {
        Verdict::Public => Err(Fail::Stop { line: PUBLIC_LINE.into(), check: "public" }),
        Verdict::Unknown(r) => {
            report.notes.push(format!("zygo: cannot check if the remote is public: {r}"));
            Ok(())
        }
        Verdict::Private | Verdict::Local => Ok(()),
    }
}

/// S§8.3 step 11, S§8.6 "At commit": the outbox files in ULID order, at most [`OUTBOX_CAP`] readable ones. A file
/// that cannot be read (a link, a FIFO, over `FILE_MAX`, not JSON, or a `to` that is no address) moves to
/// `refused/`, with one log line, and takes no cap slot. So does a file whose `to` host no machine reads at `meta`,
/// the fetched tip ([`read_host`]; review F1: a stale member cache after a rename): when its path is at the tip it
/// is deleted, else, while the pass deadline holds, it moves with the check `no-host` and the notice to the sender;
/// a bootstrap tip (`None`) holds no registry yet and refuses nothing (S§19 #8). A
/// file whose path is at the tip was pushed before and is deleted now, or, with `split` (the `push_split` flag,
/// FX-40 review), kept and put in the returned list, so it is deleted only after an accepted push. Else, while the pass deadline holds, the
/// secret pass: clear, an add of its bytes at `<to>/<ULID>.json` (and the file in the returned list, deleted after
/// the push); a finding, the move to `refused/` and the notice to the sender, whose keys go into `notified`; could
/// not run, the file stays.
fn outbox(c: &Ctx, tip: &str, meta: Option<&TipMeta>, split: bool, (deadline, pass_end): (Instant, SystemTime),
    notified: &mut Notified) -> Result<(Vec<Change>, Vec<PathBuf>), Fail> {
    let (sp, wt) = (&c.sp, &c.sp.worktree);
    let (mut items, mut no_host) = (Vec::new(), Vec::new());
    for (id, p) in outbox_entries(sp).map_err(|_| stop("outbox"))? {
        if items.len() == OUTBOX_CAP {
            break;
        }
        match read_message(&p) {
            Some((_, _, to)) if meta.is_some_and(|m| !read_host(m, host_of(&to))) => {
                no_host.push((id, p, format!("{to}/{id}.json"), to));
            }
            Some((bytes, value, to)) => items.push((id, p, bytes, value, format!("{to}/{id}.json"), to)),
            None => refuse_unread(c, id, &p),
        }
    }
    let wanted: BTreeSet<&String> = items.iter().map(|i| &i.4).chain(no_host.iter().map(|i| &i.2)).collect();
    let mut at_tip = BTreeSet::new();
    let folders: BTreeSet<&String> = items.iter().map(|i| &i.5).chain(no_host.iter().map(|i| &i.3)).collect();
    for to in folders {
        branch::ls(wt, tip, Some(&format!("{to}/")), deadline, &mut |e| {
            if wanted.contains(&e.path) {
                at_tip.insert(e.path);
            }
            Flow::Go
        })?;
    }
    let (mut changes, mut sent) = (Vec::new(), Vec::new());
    // A file pushed before (the tip check) is deleted before the host check, so it gets no wrong notice. With the
    // split flag set (FX-40 review), it is kept and deleted only after an accepted push of this pass.
    for (id, p, path, to) in no_host {
        if at_tip.contains(&path) {
            if split { sent.push(p) } else { let _ = std::fs::remove_file(&p); }
            continue;
        }
        if Instant::now() >= deadline || c.env.clock.now() >= pass_end {
            return Err(stop("deadline"));
        }
        c.log(&format!("mail sync: outbox {id}: no-host"));
        let h = host_of(&to);
        let body = format!("zygo did not send message {id} to {to}: host {h} has no registry on the side branch. The message is kept on this machine in mail/outbox/refused/.");
        refuse(c, id, &[], Some(&body), notified);
    }
    for (id, p, bytes, value, path, _) in items {
        if at_tip.contains(&path) {
            if split { sent.push(p) } else { let _ = std::fs::remove_file(&p); }
            continue;
        }
        if Instant::now() >= deadline || c.env.clock.now() >= pass_end {
            return Err(stop("deadline"));
        }
        match secret::check(&value, &c.cfg.redaction, &c.ws.main_root, &c.env.secret_policy) {
            Outcome::Clear => {
                changes.push(Change::Add { path, bytes });
                sent.push(p);
            }
            Outcome::Finding(ids) => {
                c.log(&format!("mail sync: outbox {id}: refused: {}", ids.join(", ")));
                refuse(c, id, &ids, None, notified);
            }
            Outcome::CouldNotRun(check) => {
                c.log(&format!("mail sync: outbox {id}: secret check could not run: {check}"));
            }
        }
    }
    Ok((changes, sent))
}

/// No-host review F1, F2: some machine reads `*@<h>/` at `meta`, the fetched tip: `h` has a valid registry (S§7.3),
/// or a valid ack, which keeps an old host an own host of its renamed workspace (S§7.6 "Rename", S§8.5).
fn read_host(meta: &TipMeta, h: &str) -> bool {
    meta.registries.get(h).is_some_and(|r| r.value.is_ok()) || meta.acks.get(h).is_some_and(|a| a.value.is_ok())
}

/// The host part of the address `to`.
fn host_of(to: &str) -> &str { to.split_once('@').map_or("", |(_, h)| h) }

/// S§8.6 "At commit": sends the notice to the sender of `outbox/<id>.json`, whose keys go into `notified`, then
/// moves the file to `refused/` (FX-24 review F5: in that order); `body` replaces the secret-pass notice body
/// (`ids`) for another reason. The notice is one M§5 P1 delivery (S§8.6): `mail/lock` held shared for at most
/// [`MAIL_LOCK_WAIT`], the target the sender's name resolved on the member records read under it (not the gate's
/// read), and the lock dropped before the next git call. A notice whose [`Delivery::begin`] fails (review R3 F3:
/// [`begin_error`]: a lock not taken, the lock I/O, a bad state or `mail/`, no workspace) writes the log line `notice`
/// and leaves the file in the outbox for the next pass. A notice that no pass can deliver (review R2 F1, R3 F3:
/// [`write_error`]) moves the file all the same, with the log line `notice: <reason>`: `full` (the sender's `new/` is
/// at `max_unread`), `no-record` (that read does not have the sender's name: no notice and no maildir, as `cli`),
/// `maildir` or `io` (the write into the sender's maildir failed) or `moved` (review R2 F5, R3 F1: the pass's first
/// read `c.ws` also has the name and maps it to other keys, so it was renamed or another member holds the name now;
/// the notice goes to no one). A name that `c.ws` does not have is notified at the keys of the read under the lock. A
/// failed move writes the line `notice` too.
fn refuse(c: &Ctx, id: Ulid, ids: &[String], body: Option<&str>, notified: &mut Notified) {
    let now = c.env.clock.now();
    let mut notify = |sender: &str, subject: &str, secret_body: &str| -> Result<(), NoticeError> {
        let d = Delivery::begin(&c.store, MAIL_LOCK_WAIT).map_err(begin_error)?;
        let targets = names::resolve(d.workspace(), sender, None).map_err(write_error)?;
        // FX-24 review R2 F5, R3 F1: the target is the sender's name resolved in the read under the lock. Only when
        // the pass's first read also has the name and maps it to other keys (renamed, or now held by another member)
        // is it not that member: no notice. A name the first read does not have (a member that renamed itself and
        // kept its key, or joined during the run) is notified at the key of the read under the lock.
        let keys = |t: &[names::Target]| t.iter().map(|t| t.key.clone()).collect::<Vec<_>>();
        if names::resolve(&c.ws, sender, None).is_ok_and(|first| keys(&first) != keys(&targets)) {
            return Err(NoticeError::Never("moved"));
        }
        let mut sent = false;
        let each = || -> Result<(), NoticeError> {
            for t in targets {
                let label = t.name.clone().unwrap_or_else(|| t.key.clone());
                let msg = Message::new("zygo", &label, subject, body.unwrap_or(secret_body), now).map_err(write_error)?;
                d.deliver(&t.key, &label, &msg, c.cfg.mail.max_unread).map_err(write_error)?;
                notified.keys.push(t.key);
                sent = true;
            }
            Ok(())
        };
        let r = each();
        if sent {
            notified.read = Some(d.into_workspace()); // FX-24 review F2: the wake uses this read
        }
        r
    };
    match crate::outbox::refuse(&c.sp, id, ids, &mut notify, now) {
        Ok(None) => {}
        Ok(Some(why)) => c.log(&format!("mail sync: outbox {id}: notice: {why}")),
        Err(_) => c.log(&format!("mail sync: outbox {id}: notice")),
    }
}

/// S§8.6 (FX-24 review R3 F3): an error of [`Delivery::begin`] for the notice (a busy lock, the lock I/O, a bad state,
/// a bad `mail/`, no workspace) is one that a later pass can clear: the message stays in the outbox.
fn begin_error(e: MailError) -> NoticeError { NoticeError::Later(std::io::Error::other(e.to_string())) }

/// S§8.6 (FX-24 review R2 F1, R3 F3): an error after [`Delivery::begin`] (the sender's name resolved in the read under
/// the lock, the notice made and written into the sender's maildir) is lasting, so the message moves with no notice,
/// with the reason: `full` (the sender's inbox is at `max_unread`, and only the sender's reads can clear it),
/// `no-record` (the read under the lock does not have the sender: `UnknownTarget` or `NoRecord`), `maildir` (the
/// sender's maildir is not made of real folders: the store's check of a link or an entry that is not a folder at
/// `mail/<key>/` or one of its folders, `InvalidInput`, or a path part that is not a folder, `NotADirectory`) and
/// `io` (every other error of the write).
fn write_error(e: MailError) -> NoticeError {
    match e {
        MailError::Full { .. } => NoticeError::Never("full"),
        MailError::UnknownTarget { .. } | MailError::NoRecord { .. } => NoticeError::Never("no-record"),
        MailError::Io(e) if matches!(e.kind(), std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotADirectory) => {
            NoticeError::Never("maildir")
        }
        _ => NoticeError::Never("io"),
    }
}

/// Each entry of `mail/outbox/` named `<ULID>.json` that is not a folder (regular files, links, FIFOs and others),
/// in ULID order. `mail/` and `outbox/` must be folders, not links; a missing one is no entry.
fn outbox_entries(sp: &SyncPaths) -> std::io::Result<Vec<(Ulid, PathBuf)>> {
    for d in [&sp.mail, &sp.outbox] {
        match std::fs::symlink_metadata(d) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        }
    }
    let mut v = Vec::new();
    for e in std::fs::read_dir(&sp.outbox)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            continue;
        }
        if let Some(u) = e.file_name().to_str().and_then(address::ulid_file) {
            v.push((u, e.path()));
        }
    }
    v.sort_by_key(|(u, _)| *u);
    Ok(v)
}

/// Review F2: moves the outbox entry `p` that cannot be read into `refused/` as it is (a rename: a link is moved,
/// never followed, and an existing `refused/` name is never replaced), with one log line, check `read`. A regular
/// file gets the time of the move, so the `refused/` sweep counts from it (S§9). A failed move leaves it in place.
fn refuse_unread(c: &Ctx, id: Ulid, p: &Path) {
    c.log(&format!("mail sync: outbox {id}: read"));
    let dst = c.sp.refused.join(format!("{id}.json"));
    if zygo_workspace::mail::ensure_mail_dir(&c.sp.refused).is_err() || std::fs::symlink_metadata(&dst).is_ok() {
        return;
    }
    if std::fs::rename(p, &dst).is_ok() && std::fs::symlink_metadata(&dst).is_ok_and(|m| m.is_file()) {
        let mut o = OpenOptions::new();
        o.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        let _ = o.open(&dst).and_then(|f| f.set_modified(c.env.clock.now()));
    }
}

/// The bytes of the outbox file `p` (a regular file, no link followed, at most `FILE_MAX` bytes), its JSON value and
/// its `to`, an address.
fn read_message(p: &Path) -> Option<(Vec<u8>, serde_json::Value, String)> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let f = o.open(p).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    f.take(store::FILE_MAX + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > store::FILE_MAX {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let to = value.get("to")?.as_str().filter(|t| address::parse(t).is_some())?.to_string();
    Some((bytes, value, to))
}

/// S§10.1: the wake of `keys`, when there are any, at the injected time, with the member records `read` that the
/// deliveries read under `mail/lock` (FX-24 review F2; not the gate's read). No read: nothing was delivered.
fn wake(c: &Ctx, read: Option<&Workspace>, keys: &[String]) {
    if let Some(ws) = read
        && !keys.is_empty()
    {
        c.env.wake.wake(&c.env.paths, ws, keys, &c.cfg.mail, c.env.clock.now());
    }
}

/// S§8.3 step 10 (FX-30): the `(pane key, ULID)` of each `pending_wake` entry of an earlier run whose message is in
/// the member's maildir, `mail/<key>/new/<ULID>.json` or `cur/<ULID>.json` (a regular file; the rule by which
/// `deliver_staged` takes a message as delivered). An entry whose message is in neither never arrived: it is not
/// counted. A message in `cur/` is counted once but wakes no one (FX-30 review F2, [`in_new`]). `check_state` made
/// each key a pane key and each ULID canonical.
fn arrived(sp: &SyncPaths, pending: &BTreeMap<String, String>) -> Vec<(String, Ulid)> {
    pending.iter().filter_map(|(id, key)| {
        let u = Ulid::from_string(id).ok()?;
        let md = sp.mail.join(key);
        let name = format!("{id}.json");
        ["new", "cur"].into_iter()
            .any(|d| std::fs::symlink_metadata(md.join(d).join(&name)).is_ok_and(|m| m.is_file()))
            .then(|| (key.clone(), u))
    }).collect()
}

/// S§10.1 (FX-30 review F2): the message `id` is unread mail of `key`: `mail/<key>/new/<ULID>.json` is a regular file.
fn in_new(sp: &SyncPaths, key: &str, id: Ulid) -> bool {
    std::fs::symlink_metadata(sp.mail.join(key).join("new").join(format!("{id}.json"))).is_ok_and(|m| m.is_file())
}

/// S§8.3 step 10 (FX-30 review F1, F3): right after the wake, one state write removes from `pending_wake` each entry
/// whose ULID this pass counted, and each entry whose message neither arrived nor is staged; an entry whose staged
/// file `incoming/<key>.<ULID>.json` is still there is kept, so the pass whose step 5 delivers it counts it. The write
/// loads `state.json`, changes only `pending_wake` and saves it, under the sync lock that the pass holds (S§15); the
/// pass's own copy `st` takes the result, so its later saves do not put the entries back. No entry to remove: no
/// write. A stop before the wake never gets here and keeps every entry. The recorded limit: a crash between the wake
/// and this write counts and wakes the message again in the next pass.
fn clear_pending(sp: &SyncPaths, st: &mut State, counted: &BTreeSet<Ulid>) -> Result<(), Fail> {
    let keep = |id: &String, key: &String| {
        if Ulid::from_string(id).is_ok_and(|u| counted.contains(&u)) {
            return false;
        }
        if std::fs::symlink_metadata(sp.incoming.join(format!("{key}.{id}.json"))).is_ok_and(|m| m.is_file()) {
            return true; // still staged
        }
        false
    };
    if st.pending_wake.iter().all(|(id, key)| keep(id, key)) {
        return Ok(());
    }
    let mut disk = state::load(sp).map_err(|_| stop("state"))?;
    disk.pending_wake.retain(|id, key| keep(id, key));
    state::save(sp, &disk).map_err(|_| stop("state"))?;
    st.pending_wake = disk.pending_wake;
    Ok(())
}

/// S§10.1 (FX-30): the member records read under `mail/lock` held shared (M§5; FX-24 review F2: not the gate's read)
/// for the wake of a recovered message when this round's import delivered nothing and so has no read. A read that
/// cannot begin writes the line `mail sync: wake: read` and wakes no one; the message stays counted.
fn wake_read(c: &Ctx) -> Option<Workspace> {
    match Delivery::begin(&c.store, MAIL_LOCK_WAIT) {
        Ok(d) => Some(d.into_workspace()),
        Err(_) => {
            c.log("mail sync: wake: read");
            None
        }
    }
}

/// The members that the notices of a pass went to, and the state read under `mail/lock` at the last notice that was
/// delivered (the wake uses it, S§10.1; FX-24 review F2).
#[derive(Default)]
struct Notified {
    keys: Vec<String>,
    read: Option<Workspace>,
}

/// The valid ack of host `h` with this identity at the tip.
fn own_ack<'m>(meta: &'m TipMeta, h: &str, identity: &str) -> Option<&'m Ack> {
    meta.acks.get(h).and_then(|r| r.value.as_ref().ok()).filter(|a| a.identity == identity)
}

/// S§7.5 "the blob id that it last pushed" (MS-11 F9): an own file at the tip whose blob is the pending one was
/// pushed by a run that stopped before it recorded it; it moves from `pending` to `pushed`. Whether one moved.
pub(crate) fn promote(st: &mut State, meta: &TipMeta, identity: &str) -> bool {
    let mut promoted = false;
    for (path, blob) in own_blobs(meta, identity) {
        if st.pending.get(&path) == Some(&blob) {
            st.pending.remove(&path);
            st.pushed.insert(path, blob);
            promoted = true;
        }
    }
    promoted
}

/// S§7.5: the branch path and blob id of each valid registry and ack file of this identity.
pub(crate) fn own_blobs(meta: &TipMeta, identity: &str) -> BTreeMap<String, String> {
    let regs = meta.registries.iter()
        .filter(|(_, r)| matches!(&r.value, Ok(v) if v.identity == identity))
        .map(|(h, r)| (format!("registry/{h}.json"), r.blob.clone()));
    let acks = meta.acks.iter()
        .filter(|(_, r)| matches!(&r.value, Ok(v) if v.identity == identity))
        .map(|(h, r)| (format!("ack/{h}.json"), r.blob.clone()));
    regs.chain(acks).collect()
}

/// S§7.5, S-C2 (MS-22 review F1): the keys of `st` for the own hosts of `meta` (`host` and each host whose valid
/// ack holds `identity`) stay, and so does the path of each file of this identity at `meta`, as the twin check
/// needs its blob id; every other key goes. Whether a key went.
pub(crate) fn forget_old_hosts(st: &mut State, meta: &TipMeta, identity: &str, host: &str) -> bool {
    let hosts: BTreeSet<String> = own_hosts(meta, identity, host).into_iter().collect();
    let files: BTreeSet<String> = own_blobs(meta, identity).into_keys().collect();
    state::forget_hosts(st, &hosts, &files)
}

/// The `<ULID>.json` entries of one own host's `*@<h>/` folders at the tip.
#[derive(Debug, Default)]
struct HostFiles {
    /// The ULID of every `<ULID>.json` entry, of any mode or kind.
    ulids: BTreeSet<Ulid>,
    /// Whether one of them is a message file ([`prune::removable`], S§9 Tree).
    message: bool,
}

/// S§7.4, S§9 Tree: for each own host `h`, the `<ULID>.json` entries of the `*@<h>/` folders at the tip
/// ([`HostFiles`]).
fn own_files(wt: &Path, tip: &str, own: &[String], deadline: Instant)
    -> Result<BTreeMap<String, HostFiles>, BranchError> {
    let mut folders = Vec::new();
    branch::ls(wt, tip, None, deadline, &mut |e| {
        if e.kind == "tree" && let Some(a) = address::parse(&e.path) && own.contains(&a.host) {
            folders.push((a.host, e.path));
        }
        Flow::Go
    })?;
    let mut out: BTreeMap<String, HostFiles> = own.iter().map(|h| (h.clone(), HostFiles::default())).collect();
    for (h, folder) in folders {
        let prefix = format!("{folder}/");
        let at = out.entry(h).or_default();
        branch::ls(wt, tip, Some(&prefix), deadline, &mut |e| {
            if let Some(u) = e.path.strip_prefix(&prefix).and_then(address::ulid_file) {
                at.ulids.insert(u);
                at.message |= prune::removable(&e);
            }
            Flow::Go
        })?;
    }
    Ok(out)
}

/// S§7.3: the valid member names of `ws`, sorted, without duplicates: the `members` of the own registry.
pub(crate) fn own_names(ws: &Workspace) -> Vec<String> {
    let mut names: Vec<String> = ws.members.values().filter_map(|m| m.valid_name().map(String::from)).collect();
    names.sort();
    names.dedup();
    names
}

/// S§8.3 step 12, S§7.3, S§7.4: the own registry when the tip's differs, and the ack of each own host when it
/// changes: (old ∪ the ULIDs acknowledged in this pass ∪ the own acks) ∩ the ULIDs of that host's folders at the tip
/// (S§7.4 "Owner"). The current host's ack is written when the tip has none of this identity (S§7.6). A file over
/// its limit is not written; one line is logged (O3). After a rename (S§7.6 "Rename") it deletes the registry of an
/// old own host, unless the new one is over its limit, and the ack of an old own host whose folders hold no message
/// file and whose ULIDs the new list all holds, when it deletes that host's registry or the host has none.
fn own_changes(c: &Ctx, meta: &TipMeta, identity: &str, own: &[String], files: &BTreeMap<String, HostFiles>,
    acked: &[Ulid], own_ack: &BTreeSet<Ulid>) -> Vec<Change> {
    let host = c.s.host.as_str();
    let names = own_names(&c.ws);
    let over = names.len() > MEMBERS_MAX;
    let mut out = Vec::new();
    let same = meta.registries.get(host).and_then(|r| r.value.as_ref().ok())
        .is_some_and(|r| r.identity == identity && r.members == names);
    if !same {
        if over {
            c.log("mail sync: own file over limit: members");
        } else {
            let bytes = Registry { host: host.into(), identity: identity.into(), members: names }.to_bytes();
            out.push(Change::Add { path: format!("registry/{host}.json"), bytes });
        }
    }
    for (h, r) in meta.registries.iter().filter(|_| !over) {
        if h != host && r.value.as_ref().is_ok_and(|v| v.identity == identity) {
            out.push(Change::Remove { path: format!("registry/{h}.json") });
        }
    }
    let add: BTreeSet<Ulid> = acked.iter().chain(own_ack).copied().collect();
    let none = HostFiles::default();
    for h in own {
        let tip_ack = self::own_ack(meta, h, identity); // the function; the parameter `own_ack` is the union
        let old = tip_ack.map(|a| a.imported.clone()).unwrap_or_default();
        let at = files.get(h).unwrap_or(&none);
        // The old ack goes only with or after the old registry: `own_hosts` finds an old host by its ack (S-G5).
        let reg_stays = over
            && meta.registries.get(h).is_some_and(|r| r.value.as_ref().is_ok_and(|v| v.identity == identity));
        let new: BTreeSet<Ulid> = old.union(&add).filter(|u| at.ulids.contains(u)).copied().collect();
        // S§7.6 "Rename": (a) no message file, (b) the new list holds every ULID, (c) the registry goes or is none.
        if h != host && !at.message && new == at.ulids && !reg_stays {
            out.push(Change::Remove { path: format!("ack/{h}.json") });
            continue;
        }
        if new == old && !(h == host && tip_ack.is_none()) {
            continue;
        }
        if new.len() > ACK_ENTRIES_MAX {
            c.log("mail sync: own file over limit: ack-entries");
            continue;
        }
        let bytes = Ack { host: h.clone(), identity: identity.into(), imported: new }.to_bytes();
        out.push(Change::Add { path: format!("ack/{h}.json"), bytes });
    }
    out
}
