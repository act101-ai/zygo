//! The R§10 read of the workspace files for `zygo config show/check`, `zygo profile list/show` and `zygo loadout
//! list/show`, and the read of `zygo doctor` (WD-16 plan call 1). CLI only: `profile.rs` enters it only when
//! `!ctx.slash`, through [`here`], so no slash verb file names the read (R-C8, R§18 item 20).

use std::path::Path;

use zygo_config::repo_def::{check_default, short, GitRunner, SystemGit, Use};
use zygo_config::Merged;
use zygo_core::esc;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::up::{find_checkout, Exit, GIT_BOUND};

/// What one R§10 read found, for `zygo config`, `zygo profile` and `zygo loadout`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum State {
    /// No folder, or no checkout at it (`find_checkout` failed): no read.
    Outside,
    Absent,
    Unread,
    /// `why`: the R§6.4 violation as the read gives it (not escaped).
    Invalid { why: String },
    /// `workspace`: the read has `.zygo/workspace.toml`.
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

/// The R§10 view of `merged` at `cwd`; `None` or an `Err` of `read_here` gives `State::Outside` (plan call 7).
/// Only an approved read changes `merged` (R-C2); the other states show the full ref, the short digest and the
/// violation, each through `esc` (R-C7, R§6.4), and no key or value of the files.
pub(crate) fn view(merged: &Merged, cwd: Option<&Path>, approvals: &Path, git: &dyn GitRunner) -> View {
    let unchanged = |state: State, line: Option<String>| View { merged: merged.clone(), state, line };
    let Some(found) = cwd.and_then(|d| read_here(d, approvals, git).ok()) else {
        return unchanged(State::Outside, None);
    };
    match found {
        Use::Absent { .. } => unchanged(State::Absent, None),
        Use::Unread { why } => unchanged(
            State::Unread,
            Some(format!("zygo: this repository's workspace files were not read ({}); not shown", esc(&why))),
        ),
        Use::Invalid { full_ref, why, .. } => {
            let line = format!(
                "zygo: this repository's workspace files ({}) are invalid ({}); not shown",
                esc(full_ref.as_deref().unwrap_or_default()),
                esc(&why)
            );
            unchanged(State::Invalid { why }, Some(line))
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

/// `view` at `std::env::current_dir()` with `SystemGit`: the call of `profile.rs`, which may not name the read.
pub(crate) fn here(merged: &Merged, approvals: &Path) -> View {
    let cwd = std::env::current_dir().ok();
    view(merged, cwd.as_deref(), approvals, &SystemGit)
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
