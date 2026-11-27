//! The R§10 read of the workspace files for `zygo config show/check`, `zygo profile list/show` and `zygo loadout
//! list/show`, and the read of `zygo doctor` (WD-16 plan call 1). CLI only: `profile.rs` enters it only when
//! `!ctx.slash`, through [`here`], so no slash verb file names the read (R-C8, R§18 item 20); [`here`] takes the slash
//! flag too and reads nothing when it is set (review F2).

use std::path::{Path, PathBuf};

use zygo_config::repo_def::{check_default, short, GitRunner, SystemGit, Use};
use zygo_config::Merged;
use zygo_core::esc;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::up::{find_checkout, Exit, GIT_BOUND};

/// What one R§10 read found, for `zygo config`, `zygo profile` and `zygo loadout`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum State {
    /// No folder, or no checkout at it (`find_checkout` exited 2): no read.
    Outside,
    /// The slash form of a verb (hook process): no read, no folder lookup, no git (R-C8; review F2).
    Slash,
    Absent,
    Unread,
    /// `why`: the R§6.4 violation as the read gives it (not escaped).
    /// `workspace`, here and below: the read has `.zygo/workspace.toml` (review F4).
    Invalid { why: String, workspace: bool },
    NotApproved { workspace: bool },
    Approved { workspace: bool },
}

pub(crate) struct View {
    /// `merged.with_definition(def)` when approved; else `merged` unchanged.
    pub(crate) merged: Merged,
    pub(crate) state: State,
    /// Plan call 3: the not-approved, invalid or unread line; `None` otherwise.
    pub(crate) line: Option<String>,
}

/// R§7.3: one read of the default branch at the top of the checkout of `cwd`, keyed by the workspace id of its git
/// common dir (the read of `zygo workspace approve` and `zygo doctor`). `Err`: the lookup error of `find_checkout`.
pub(crate) fn read_here(cwd: &Path, approvals: &Path, git: &dyn GitRunner) -> Result<Use, Exit> {
    let (co, ..) = find_checkout(cwd, GIT_BOUND)?;
    Ok(check_default(&co.top, &workspace_id(&co.common_dir), approvals, git))
}

/// The read of `read_here` as the verbs and `zygo doctor` show it (WD-16 review F1). `None` only for the exit 2 of
/// `find_checkout`, `zygo up`'s "not a repository" (also a bare repository with no checkout): outside, no line. Any
/// other lookup error (a git that fails, times out or cannot start) is `Use::Unread` with its reason. Decided by the
/// exit code, not the message.
pub(crate) fn found(r: Result<Use, Exit>) -> Option<Use> {
    match r {
        Ok(u) => Some(u),
        Err(e) if e.code == 2 => None,
        Err(e) => Some(Use::Unread { why: e.message }),
    }
}

/// The R§10 view of `merged` at `cwd`: [`view_of`] of [`found`] of `read_here`; no folder gives `State::Outside`.
pub(crate) fn view(merged: &Merged, cwd: Option<&Path>, approvals: &Path, git: &dyn GitRunner) -> View {
    view_of(merged, cwd.and_then(|d| found(read_here(d, approvals, git))))
}

/// The R§10 view of `merged` for one read; `None` gives `State::Outside` (plan call 7).
/// Only an approved read changes `merged` (R-C2); the other states show the full ref, the short digest and the
/// violation, each through `esc` (R-C7, R§6.4), and no key or value of the files.
pub(crate) fn view_of(merged: &Merged, found: Option<Use>) -> View {
    let unchanged = |state: State, line: Option<String>| View { merged: merged.clone(), state, line };
    let Some(found) = found else {
        return unchanged(State::Outside, None);
    };
    match found {
        Use::Absent { .. } => unchanged(State::Absent, None),
        Use::Unread { why } => unchanged(
            State::Unread,
            Some(format!("zygo: this repository's workspace files were not read ({}); not shown", esc(&why))),
        ),
        Use::Invalid { full_ref, why, workspace, .. } => {
            let line = format!(
                "zygo: this repository's workspace files ({}) are invalid ({}); not shown",
                esc(full_ref.as_deref().unwrap_or_default()),
                esc(&why)
            );
            unchanged(State::Invalid { why, workspace }, Some(line))
        }
        Use::NotApproved { full_ref, digest, def, .. } => unchanged(
            State::NotApproved { workspace: def.workspace.is_some() },
            Some(format!(
                "zygo: this repository's workspace files ({}, digest {}) are not approved on this machine; not shown; \
                 see zygo workspace approve",
                esc(&full_ref),
                esc(short(&digest))
            )),
        ),
        Use::Approved { def, .. } => View {
            merged: merged.with_definition(&def),
            state: State::Approved { workspace: def.workspace.is_some() },
            line: None,
        },
    }
}

/// [`here_in`] at `std::env::current_dir()` with `SystemGit`: the call of `profile.rs`, which may not name the read,
/// and of `loadout.rs`. `slash`: the verb runs as a slash verb in the hook process.
pub(crate) fn here(merged: &Merged, approvals: &Path, slash: bool) -> View {
    here_in(merged, approvals, slash, &|| std::env::current_dir().ok(), &SystemGit)
}

/// `view` at `cwd()` with `git`. With `slash` set it reads nothing, finds no folder and calls no git: `State::Slash`,
/// `merged` unchanged, no line (R-C8, R§18 item 20; WD-16 review F2: this holds whatever guard the caller has).
pub(crate) fn here_in(
    merged: &Merged,
    approvals: &Path,
    slash: bool,
    cwd: &dyn Fn() -> Option<PathBuf>,
    git: &dyn GitRunner,
) -> View {
    if slash {
        return View { merged: merged.clone(), state: State::Slash, line: None };
    }
    view(merged, cwd().as_deref(), approvals, git)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::time::Instant;

    use zygo_config::{load, LoadContext, Merged};
    use zygo_core::paths::Paths;

    /// `Merged` of an empty user config, no repository and no team layer.
    fn merged(dir: &Path) -> Merged {
        let config_file = dir.join("config.toml");
        std::fs::write(&config_file, "").unwrap();
        let paths = Paths {
            config_file,
            cache_dir: dir.join("cache"),
            data_dir: dir.join("data"),
            sessions_dir: dir.join("cache/sessions"),
            marketplaces_dir: dir.join("cache/marketplaces"),
            plugin_store: dir.join("data/plugins"),
            approvals_file: dir.join("data/approvals.toml"),
            usage_db: dir.join("data/usage.db"),
        };
        load(&LoadContext { paths, repo: None, team: None }).unwrap()
    }

    /// `git init -q -b main <dir>` with no user or system config and no template.
    fn git_init(dir: &Path) {
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let o = Command::new("git")
            .env("GIT_CONFIG_GLOBAL", null)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TEMPLATE_DIR", "")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .args(["init", "-q", "-b", "main"])
            .arg(dir)
            .output()
            .expect("git on PATH");
        assert!(o.status.success(), "git init: {}", String::from_utf8_lossy(&o.stderr));
    }

    /// A runner that no read may call.
    struct Panics;
    impl GitRunner for Panics {
        fn git(&self, _: &Path, args: &[&str], _: Instant) -> Result<Output, String> {
            panic!("a read ran git {args:?}")
        }
    }

    /// A runner whose every call fails with `why`.
    struct Fails(&'static str);
    impl GitRunner for Fails {
        fn git(&self, _: &Path, _: &[&str], _: Instant) -> Result<Output, String> {
            Err(self.0.to_string())
        }
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let r = t.path().join("r");
        git_init(&r);
        (t, r)
    }

    #[test]
    fn view_with_no_folder_reads_nothing() {
        let t = tempfile::tempdir().unwrap();
        let m = merged(t.path());
        let v = view(&m, None, &t.path().join("approvals.toml"), &Panics);
        assert_eq!(v.state, State::Outside);
        assert_eq!(v.line, None);
        assert_eq!(v.merged.config.profile, m.config.profile);
    }

    #[test]
    fn view_of_a_failed_read_gives_the_unread_line() {
        let (t, r) = repo();
        let m = merged(t.path());
        let v = view(&m, Some(&r), &t.path().join("approvals.toml"), &Fails("timed out"));
        assert_eq!(v.state, State::Unread);
        let line = v.line.expect("an unread line");
        assert!(line.starts_with("zygo: this repository's workspace files were not read ("), "{line}");
        assert!(line.ends_with("); not shown"), "{line}");
    }

    /// WD-16 review F1: only `find_checkout`'s exit 2 (not a repository; a bare repository with no checkout) is
    /// outside; any other lookup error is an unread read with its reason, escaped, decided by the exit code alone.
    #[test]
    fn only_a_lookup_exit_2_is_outside() {
        let t = tempfile::tempdir().unwrap();
        let m = merged(t.path());
        let exit = |code: i32, message: &str| Exit { code, message: message.to_string() };
        assert_eq!(found(Err(exit(2, zygo_workspace::up::NOT_A_REPO))), None);
        assert_eq!(found(Err(exit(2, "git worktree list: failed"))), None);
        let v = view_of(&m, found(Err(exit(1, "git worktree list: failed\u{1b}[2J"))));
        assert_eq!(v.state, State::Unread);
        assert_eq!(
            v.line.as_deref(),
            Some("zygo: this repository's workspace files were not read (git worktree list: failed\\u{1b}[2J); not shown")
        );
        let v = view_of(&m, found(Err(exit(1, zygo_workspace::up::NOT_A_REPO))));
        assert_eq!(v.state, State::Unread, "decided by the exit code, not the message");
    }

    /// WD-16 review F2: with `slash` set, `here_in` looks up no folder and never calls the git runner.
    #[test]
    fn here_with_slash_set_reads_nothing() {
        let (t, r) = repo();
        let m = merged(t.path());
        let no_cwd = || -> Option<PathBuf> { panic!("a slash call looked up the folder") };
        let v = here_in(&m, &t.path().join("approvals.toml"), true, &no_cwd, &Panics);
        assert_eq!(v.state, State::Slash);
        assert_eq!(v.line, None);
        assert_eq!(v.merged.config.profile, m.config.profile);
        // Control: the same call with `slash` unset reaches the runner.
        let v = here_in(&m, &t.path().join("approvals.toml"), false, &|| Some(r.clone()), &Fails("ran"));
        assert_eq!(v.state, State::Unread);
    }

    #[test]
    fn view_escapes_the_reason() {
        let (t, r) = repo();
        let m = merged(t.path());
        let v = view(&m, Some(&r), &t.path().join("approvals.toml"), &Fails("x\u{1b}[2J\u{202e}"));
        let line = v.line.expect("an unread line");
        assert!(line.contains("\\u{1b}[2J\\u{202e}"), "{line}");
        assert!(!line.contains('\u{1b}') && !line.contains('\u{202e}'), "{line:?}");
    }
}
