//! MS-21: zygo ws forget with mail sync: the outbox question and the removal of the sync worktree (mail sync design
//! S§6.3, S§7.6, S§14, S§19 #20 forget part, #26). Machines are scratch homes on one `file://` bare repository, as
//! `tests/mail_sync_cli.rs`; no agent runs, no clock is read.
#![cfg(unix)]
mod common;

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime};

use zygo_config::{builtin_patterns, RedactionConfig, SyncSettings};
use zygo_core::Agent;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::outbox::{count, send_remote_with};
use zygo_mail_sync::state::SyncPaths;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::state::{MemberRecord, Phase};

use common::feed_stdin;
use common::workspace::{ready_exe, Harness, WS_ID};

const ALICE: &str = "kaaaaaaaa";
const BOB: &str = "kbbbbbbbb";
const PROMPT: &str = "zygo: 2 unsent messages will be deleted. Forget the workspace? [y/N]";
/// The `sent_at` of each test message: fixed, so no test reads a clock (review focus 75).
const SENT: u64 = 1_790_000_000;

/// `git -c user.name=t -c user.email=t@t.invalid -c commit.gpgSign=false <args>` in `dir`, hermetic (no global or
/// system config, no templates); panics on a non-zero exit; the stdout without its trailing newline.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TEMPLATE_DIR", "")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgSign=false"])
        .args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", err(&o));
    out(&o).trim_end().to_string()
}

/// A bare repository `bare.git` with one commit on `main`, and its `file://` URL.
struct Remote { _t: tempfile::TempDir, url: String }
fn remote() -> Remote {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    // `main` named here: the hermetic git reads no `init.defaultBranch`, and a clone needs a HEAD that exists.
    git(&p, &["init", "-q", "--bare", "--initial-branch=main", "bare.git"]);
    let url = format!("file://{}", p.join("bare.git").display());
    git(&p, &["clone", "-q", &url, "seed"]);
    std::fs::write(p.join("seed/README"), "x\n").unwrap();
    git(&p.join("seed"), &["add", "README"]);
    git(&p.join("seed"), &["commit", "-q", "-m", "seed"]);
    git(&p.join("seed"), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    Remote { _t: t, url }
}

/// The workspace `app` (`WS_ID`) of `h`, down, with a socket whose folder does not exist (so `forget` makes no tmux
/// call, W§12) and the members `(pane key, name)`.
fn seed_down(h: &Harness, members: &[(&str, &str)]) {
    let sock = h.root().join("no-socket/s");
    h.seed(WS_ID, &mut |w| {
        w.phase = Phase::Down;
        w.tmux_socket = Some(sock.clone());
        for (k, n) in members {
            w.members.insert(k.to_string(), MemberRecord { name: Some(n.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
        }
    });
}

/// One machine: a scratch home whose `repo` is a clone of `r`, `git` linked into `bin`, `[mail] host` and
/// `[mail.workspace.app] sync`, and the workspace of [`seed_down`].
fn machine(r: &Remote, host: &str, members: &[(&str, &str)]) -> Harness {
    let h = Harness::no_tmux();
    git(h.root(), &["clone", "-q", &r.url, "repo"]);
    std::os::unix::fs::symlink(which::which("git").unwrap(), h.bin.join("git")).unwrap();
    std::fs::write(h.root().join("config.toml"), format!("[mail]\nhost = \"{host}\"\n[mail.workspace.app]\nsync = \"zygo/mail\"\n")).unwrap();
    seed_down(&h, members);
    h
}

/// `h.zygo(args, …)`; `input` fed to stdin (none: the null device, the end of input at once).
fn zygo(h: &Harness, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut c = h.zygo(args, &[("ZYGO_WORKSPACE", OsStr::new(WS_ID))]);
    let Some(bytes) = input else { return c.output().unwrap() };
    c.stdin(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    feed_stdin(child.stdin.take().unwrap(), bytes);
    child.wait_with_output().unwrap()
}
fn out(o: &Output) -> String { String::from_utf8_lossy(&o.stdout).into_owned() }
fn err(o: &Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }
fn code_out_err(o: &Output) -> (Option<i32>, String, String) { (o.status.code(), out(o), err(o)) }

/// `zygo mail sync`, which must exit 0.
fn sync(h: &Harness) {
    let o = zygo(h, &["mail", "sync"], None);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
}

fn sp(h: &Harness) -> SyncPaths { SyncPaths::new(&h.paths, WS_ID).unwrap() }

/// The state folder as the binary names it (`HOME = root`: `~/.local/state/zygo`, a link to the test's folder).
fn state(h: &Harness) -> PathBuf { h.root().join(".local/state/zygo/workspaces").join(WS_ID) }

/// One message from `from@own` to `to` in the outbox of `h` (as `tests/mail_sync_cli.rs:105-111`, time fixed).
fn send(h: &Harness, own: &str, from: &str, to: (&str, &str)) {
    let s = SyncSettings { branch: "zygo/mail".into(), remote: "origin".into(), host: own.into(), sync_gap: Duration::from_secs(60) };
    let policy = DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() };
    let red = RedactionConfig { pattern: builtin_patterns(), ..Default::default() };
    let to = Address { name: to.0.into(), host: to.1.into() };
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(SENT);
    send_remote_with(&sp(h), Ok(&s), Some(from), &to, "s", "b", &red, &h.repo(), now, &policy).unwrap();
}

/// `git worktree list --porcelain` in `repo`, one item per entry.
fn worktrees(repo: &Path) -> Vec<String> {
    git(repo, &["worktree", "list", "--porcelain"]).split("\n\n").map(String::from).collect()
}

/// Whether `entry` is the sync worktree of `h`.
fn is_sync(entry: &str, h: &Harness) -> bool { entry.starts_with(&format!("worktree {}\n", sp(h).worktree.display())) }

#[test] // S§7.6 steps 1-4, S§19 #20 (forget part), #26, S§6.3 (never prune): 2 outbox files: count and question; n and
        // the end of input keep everything (exit 0); --yes forgets; the sync worktree leaves git's list; the other
        // worktrees stay, a prunable one included
fn forget_asks_for_two_unsent_messages_then_yes_removes_the_sync_worktree() {
    let r = remote();
    let (a, b) = (machine(&r, "a", &[(ALICE, "alice")]), machine(&r, "b", &[(BOB, "bob")]));
    sync(&a);
    sync(&b);
    sync(&a);
    for _ in 0..2 {
        send(&a, "a", "alice", ("bob", "b"));
    }
    let repo = a.repo();
    let (other, gone) = (a.root().join("other"), a.root().join("gone"));
    git(&repo, &["worktree", "add", "-q", "-b", "other", other.to_str().unwrap()]);
    git(&repo, &["worktree", "add", "-q", "--detach", gone.to_str().unwrap()]);
    std::fs::remove_dir_all(&gone).unwrap();
    let before = worktrees(&repo);
    assert_eq!((before.len(), before.iter().filter(|e| is_sync(e, &a)).count()), (4, 1), "{before:?}");
    let prunable = |list: &[String]| list.iter().any(|e| e.starts_with(&format!("worktree {}\n", gone.display())) && e.contains("\nprunable"));
    assert!(prunable(&before), "{before:?}");
    for input in [Some(&b"n\n"[..]), None] {
        let o = zygo(&a, &["ws", "forget", "app"], input);
        assert_eq!(code_out_err(&o), (Some(0), "zygo: app not forgotten\n".to_string(), format!("{PROMPT} ")));
        assert_eq!(count(&sp(&a)).unwrap(), 2);
        assert!(state(&a).join("workspace.json").exists());
        assert_eq!(worktrees(&repo), before);
    }
    let o = zygo(&a, &["ws", "forget", "app", "--yes"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), format!("{PROMPT} y (--yes)\n")));
    let after = worktrees(&repo);
    let want: Vec<String> = before.into_iter().filter(|e| !is_sync(e, &a)).collect();
    assert_eq!((after.len(), &after), (3, &want));
    assert!(prunable(&after), "{after:?}");
    assert!(std::fs::symlink_metadata(state(&a)).is_err());
    assert!(std::fs::symlink_metadata(a.paths.workspace_cache_dir().join(WS_ID)).is_err());
}

#[test] // S§7.6 step 2, #26: an empty outbox asks nothing; forget reads no [mail] config; the sync worktree goes
fn forget_with_an_empty_outbox_asks_nothing_and_needs_no_mail_config() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    assert_eq!(worktrees(&a.repo()).iter().filter(|e| is_sync(e, &a)).count(), 1);
    std::fs::write(a.root().join("config.toml"), "").unwrap();
    let o = zygo(&a, &["ws", "forget", "app"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    let after = worktrees(&a.repo());
    assert_eq!((after.len(), after.iter().filter(|e| is_sync(e, &a)).count()), (1, 0), "{after:?}");
}

#[test] // S§6.3 Remove: no worktree folder, no git call: forget succeeds with no git on PATH
fn forget_of_a_workspace_that_never_synced_runs_no_git() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    assert!(std::fs::symlink_metadata(h.bin.join("git")).is_err());
    let o = zygo(&h, &["ws", "forget", "app", "--yes"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    assert!(std::fs::symlink_metadata(state(&h)).is_err());
}

#[test] // S§6.3 Remove: a moved repository gets no git call; forget succeeds; the moved repository lists the old path as prunable
fn forget_after_the_repository_moved_runs_no_git() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    let moved = a.root().join("moved");
    std::fs::rename(a.repo(), &moved).unwrap();
    let o = zygo(&a, &["ws", "forget", "app"], None);
    assert_eq!(code_out_err(&o), (Some(0), "zygo: forgot app\n".to_string(), String::new()));
    assert!(std::fs::symlink_metadata(sp(&a).worktree).is_err());
    let left = worktrees(&moved);
    assert!(left.iter().any(|e| is_sync(e, &a) && e.contains("\nprunable")), "{left:?}");
}

#[test] // S§7.6 step 3: a removal that fails refuses with its reason; nothing deleted, the worktree still listed
fn forget_refuses_when_git_cannot_remove_the_worktree() {
    let r = remote();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    let g = a.bin.join("git");
    std::fs::remove_file(&g).unwrap();
    std::fs::write(&g, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o755)).unwrap();
    ready_exe(&g);
    let bytes = std::fs::read(state(&a).join("workspace.json")).unwrap();
    let o = zygo(&a, &["ws", "forget", "app", "--yes"], None);
    let line = format!("zygo: cannot forget app: the mail sync worktree {} was not removed: git failed\n", sp(&a).worktree.display());
    assert_eq!(code_out_err(&o), (Some(1), String::new(), line));
    assert_eq!(std::fs::read(state(&a).join("workspace.json")).unwrap(), bytes);
    assert!(sp(&a).worktree.join(".git").exists());
    assert_eq!(worktrees(&a.repo()).iter().filter(|e| is_sync(e, &a)).count(), 1);
}

#[test] // S§7.6 step 2, S§19 #20: an outbox that cannot be read refuses; unsent mail is never taken for none
fn forget_refuses_an_outbox_that_cannot_be_read() {
    let h = Harness::no_tmux();
    seed_down(&h, &[(ALICE, "alice")]);
    std::fs::create_dir_all(state(&h).join("mail")).unwrap();
    std::fs::write(state(&h).join("mail/outbox"), "").unwrap();
    let o = zygo(&h, &["ws", "forget", "app", "--yes"], None);
    let line = format!("zygo: cannot forget app: the outbox cannot be read: {} is not a folder\n", state(&h).join("mail/outbox").display());
    assert_eq!(code_out_err(&o), (Some(1), String::new(), line));
    assert!(state(&h).join("workspace.json").exists());
}
