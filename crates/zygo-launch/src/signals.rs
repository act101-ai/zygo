//! The launcher's signal handling from session creation until its cleanup ends (§8.2 step 2 note 2026-11-19, steps
//! 5 and 6). Unix: one handler for SIGINT, SIGQUIT, SIGTERM and SIGHUP; a signal already ignored stays ignored. While
//! a child pid is stored (step 5), SIGINT and SIGQUIT do nothing (the agent, in the same process group, gets them from
//! the terminal and, since `exec` resets a caught handler, starts with the default handling), and SIGTERM and SIGHUP
//! are sent on to the child. With no child (steps 2-4 and 6), the first signal is recorded: the launcher stops at its
//! next check with `128 + signal` and runs the cleanup ([`stopped`], [`or_stopped`]); during the cleanup it changes
//! nothing. A second SIGINT or SIGQUIT with no child takes the default action at once, with no cleanup; a repeated
//! SIGTERM or SIGHUP (a terminal hangup sends SIGHUP twice) is only recorded. The handler and [`wait`]
//! each take the recorded signal with one swap, so a SIGTERM or SIGHUP that arrives while the pid is stored reaches
//! the child once. Windows: one console routine keeps the launcher alive on Ctrl+C and Ctrl+Break; with no child the
//! first is recorded as SIGINT and a second takes the default action; close, logoff and shutdown keep the default.
//! The first [`install`] sets the handling and the last [`Installed`] dropped puts back what it replaced; concurrent
//! launches in one process share the slots. Accepted limit: a signal sent on between the reap and the slot clear can
//! reach a reused pid. Unix: the handler also sets a flag on each SIGTERM or SIGHUP, with or without a child; only the
//! first [`install`] clears it, and [`received`] reads it (workspaces design AU34).

use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

struct State {
    count: usize,
    #[cfg(unix)]
    old: Vec<(libc::c_int, libc::sigaction)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    count: 0,
    #[cfg(unix)]
    old: Vec::new(),
});

/// Held from session creation until the cleanup ends (§8.2 steps 2-6); the last one dropped restores the handling it
/// replaced.
#[must_use = "the handling ends when this is dropped"]
pub(crate) struct Installed(());

/// Installs the launcher's signal handling before session creation (§8.2 step 2 note 2026-11-19).
pub(crate) fn install() -> Installed {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if s.count == 0 {
        PENDING.store(0, Ordering::SeqCst);
        RECEIVED.store(false, Ordering::SeqCst);
        set(&mut s);
    }
    s.count += 1;
    Installed(())
}

impl Drop for Installed {
    fn drop(&mut self) {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        s.count -= 1;
        if s.count == 0 {
            restore(&mut s);
        }
    }
}

static CHILD: AtomicI32 = AtomicI32::new(0);
/// The first signal recorded with no child stored; 0 = none.
static PENDING: AtomicI32 = AtomicI32::new(0);

/// SIGTERM or SIGHUP arrived since the first `install`.
static RECEIVED: AtomicBool = AtomicBool::new(false);

/// SIGTERM or SIGHUP arrived since the first `install` (workspaces design AU34, AU17). Only `install` clears it.
/// Windows: always `false`.
pub(crate) fn received() -> bool {
    RECEIVED.load(Ordering::SeqCst)
}

/// The signal recorded with no child (§8.2 step 2 note 2026-11-19), if any.
pub(crate) fn stopped() -> Option<i32> {
    match PENDING.load(Ordering::SeqCst) {
        0 => None,
        s => Some(s),
    }
}

/// `128 + signal` when a signal is recorded, else `code`.
pub(crate) fn or_stopped(code: i32) -> i32 {
    stopped().map_or(code, |s| 128 + s)
}

/// `child.wait()`; the child's pid is the forwarding target for the duration. A SIGTERM or SIGHUP recorded before the
/// pid was stored is sent to the child.
pub(crate) fn wait(child: &mut Child) -> std::io::Result<ExitStatus> {
    let pid = i32::try_from(child.id()).unwrap_or(0);
    CHILD.store(pid, Ordering::SeqCst);
    let p = PENDING.swap(0, Ordering::SeqCst);
    #[cfg(unix)]
    if pid > 0 && (p == libc::SIGTERM || p == libc::SIGHUP) {
        // SAFETY: `kill` takes plain integers; the child is not reaped yet.
        unsafe { libc::kill(pid, p) };
    }
    #[cfg(not(unix))]
    let _ = p;
    let r = child.wait();
    CHILD.store(0, Ordering::SeqCst);
    r
}

#[cfg(unix)]
extern "C" fn handle(sig: libc::c_int) {
    // SAFETY: atomics, `kill`, `signal` and `raise` are async-signal-safe; `errno` is put back.
    unsafe {
        let e = *errno();
        if sig == libc::SIGTERM || sig == libc::SIGHUP {
            RECEIVED.store(true, Ordering::SeqCst);
        }
        let first = PENDING.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst).is_ok();
        let p = CHILD.load(Ordering::SeqCst);
        if p > 0 {
            // Step 5: the slot goes to this handler or to `wait`, never both (D6).
            let taken = PENDING.swap(0, Ordering::SeqCst);
            for s in [taken, if first { 0 } else { sig }] {
                if s == libc::SIGTERM || s == libc::SIGHUP {
                    libc::kill(p, s);
                }
            }
        } else if !first && (sig == libc::SIGINT || sig == libc::SIGQUIT) {
            // A second SIGINT or SIGQUIT with no child: the default action at once (D5). A repeated SIGTERM or SIGHUP
            // (a terminal hangup sends SIGHUP twice) is only recorded, so the cleanup runs (round 2 M1).
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
        *errno() = e;
    }
}

#[cfg(target_os = "linux")]
fn errno() -> *mut libc::c_int {
    // SAFETY: returns the calling thread's errno location.
    unsafe { libc::__errno_location() }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn errno() -> *mut libc::c_int {
    // SAFETY: returns the calling thread's errno location.
    unsafe { libc::__error() }
}

#[cfg(unix)]
fn set(s: &mut State) {
    for sig in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `sigaction` on zeroed structs; the handler calls only async-signal-safe functions.
        unsafe {
            let mut old: libc::sigaction = std::mem::zeroed();
            libc::sigaction(sig, std::ptr::null(), &mut old);
            if old.sa_sigaction == libc::SIG_IGN {
                continue;
            }
            let mut new: libc::sigaction = std::mem::zeroed();
            new.sa_sigaction = handle as extern "C" fn(libc::c_int) as libc::sighandler_t;
            new.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut new.sa_mask);
            libc::sigaction(sig, &new, std::ptr::null_mut());
            s.old.push((sig, old));
        }
    }
}

#[cfg(unix)]
fn restore(s: &mut State) {
    for (sig, old) in s.old.drain(..) {
        // SAFETY: puts back the action `set` read for this signal.
        unsafe { libc::sigaction(sig, &old, std::ptr::null_mut()) };
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(routine: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

/// TRUE for `CTRL_C_EVENT` (0) and `CTRL_BREAK_EVENT` (1) while a child runs (the console gives the agent the same
/// event) or, with no child, for the first one (recorded as SIGINT). FALSE for a second one with no child and for the
/// close, logoff and shutdown events: the default (the process ends).
#[cfg(windows)]
extern "system" fn keep(ctrl: u32) -> i32 {
    if ctrl > 1 {
        return 0;
    }
    if CHILD.load(Ordering::SeqCst) == 0
        && PENDING.compare_exchange(0, libc::SIGINT, Ordering::SeqCst, Ordering::SeqCst).is_err()
    {
        return 0;
    }
    1
}

#[cfg(windows)]
fn set(_: &mut State) {
    // SAFETY: adds a routine that lives for the whole program.
    unsafe { SetConsoleCtrlHandler(Some(keep), 1) };
}

#[cfg(windows)]
fn restore(_: &mut State) {
    // SAFETY: removes the routine `set` added.
    unsafe { SetConsoleCtrlHandler(Some(keep), 0) };
}

#[cfg(all(test, unix))]
mod tests {
    use super::install;

    const SIGS: [libc::c_int; 4] = [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP];

    fn handler(sig: libc::c_int) -> libc::sighandler_t {
        let mut a: libc::sigaction = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::sigaction(sig, std::ptr::null(), &mut a) }, 0);
        a.sa_sigaction
    }

    /// One test, so no other test in this binary changes the handling meanwhile; it sends no signal and restores
    /// this process's handling before it returns.
    #[test]
    fn nested_installs_restore_and_ignored_stays_ignored() {
        let before = SIGS.map(handler);
        let a = install();
        let during = SIGS.map(handler);
        let b = install();
        drop(a);
        assert_eq!(SIGS.map(handler), during);
        drop(b);
        assert_eq!(SIGS.map(handler), before);
        assert!(during.iter().zip(before).all(|(d, b)| *d != b && *d != libc::SIG_IGN), "{during:?} {before:?}");

        let mut ign: libc::sigaction = unsafe { std::mem::zeroed() };
        ign.sa_sigaction = libc::SIG_IGN;
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::sigaction(libc::SIGHUP, &ign, &mut old) }, 0);
        let c = install();
        let hup = handler(libc::SIGHUP);
        drop(c);
        let hup_after = handler(libc::SIGHUP);
        unsafe { libc::sigaction(libc::SIGHUP, &old, std::ptr::null_mut()) };
        assert_eq!(hup, libc::SIG_IGN);
        assert_eq!(hup_after, libc::SIG_IGN);
    }
}
