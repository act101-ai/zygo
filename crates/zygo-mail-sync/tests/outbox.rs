//! MS-9: the outbox (mail sync design S§5, S§7.2, S§8.6, S§9 last paragraph, S§15; S§19 #4 send part, #21).
//! No test depends on wall-clock time: `now` is fixed, file times are set by the test, and the secret pass gets
//! an hour of budget.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use ulid::Ulid;
use zygo_config::{builtin_patterns, parse_duration, Action, MailConfig, RedactionConfig, RedactionPattern, SyncOff, SyncSettings};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::{resolve_to, AddrError, Address};
use zygo_mail_sync::outbox::{count, pending, refuse, refused_count, send_remote, send_remote_with, sweep, NoticeError, SendError};
use zygo_mail_sync::registry::{write_cache, MemberCache};
use zygo_mail_sync::state::{save, PublicCache, State, SyncPaths};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::mail::store::{deliver, list, FILE_MAX};
use zygo_workspace::mail::{Message, BODY_MAX};

const ID: &str = "0123456789abcdef";
const KEY: &str = "kaaaaaaaa";
const NO_NAME: &str = "zygo: remote mail needs a sender name; give this member a name in the loadout or with --name";

fn paths(p: &Path) -> Paths {
    Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    }
}

fn now() -> SystemTime { UNIX_EPOCH + Duration::from_secs(1_800_000_000) } // 2027-01-15T08:00:00Z
fn far() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }
fn aws() -> String { ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat() }
fn builtin() -> RedactionConfig { RedactionConfig { pattern: builtin_patterns(), ..Default::default() } }
fn on() -> SyncSettings {
    SyncSettings { branch: "zygo/mail".into(), remote: "origin".into(), host: "a".into(), sync_gap: Duration::from_secs(60) }
}
fn addr(name: &str, host: &str) -> Address { Address { name: name.into(), host: host.into() } }

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

/// A workspace on host `a` whose member cache holds `bob@b` and `carol@c`, and a main worktree with no
/// act-lite store (regex patterns only).
struct Fx { _t: TempDir, sp: SyncPaths, root: PathBuf }

fn fx() -> Fx {
    let t = TempDir::new().unwrap();
    let sp = SyncPaths::new(&paths(t.path()), ID).unwrap();
    fs::create_dir_all(sp.mail.parent().unwrap()).unwrap(); // the workspace exists (MS-21 review R2-1)
    let mut c = MemberCache::default();
    c.hosts.insert("b".into(), vec!["bob".into()]);
    c.hosts.insert("c".into(), vec!["carol".into()]);
    write_cache(&sp, &c).unwrap();
    let root = t.path().join("repo");
    fs::create_dir_all(root.join(".git")).unwrap();
    Fx { _t: t, sp, root }
}

fn send(f: &Fx, from: Option<&str>, to: &Address, body: &str) -> Result<Ulid, SendError> {
    send_remote_with(&f.sp, Ok(&on()), from, to, "status", body, &builtin(), &f.root, now(), &far())
}

/// One message file in `outbox/`, as `send_remote` writes it.
fn outbox_file(f: &Fx, from: &str) -> Ulid {
    let m = Message::new(from, "bob@b", "status", "hello", now()).unwrap();
    fs::create_dir_all(&f.sp.outbox).unwrap();
    fs::write(f.sp.outbox.join(format!("{}.json", m.id)), serde_json::to_vec(&m).unwrap()).unwrap();
    m.id.parse().unwrap()
}

fn put(p: &Path, age_secs: u64) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
    fs::File::options().write(true).open(p).unwrap().set_modified(now() - Duration::from_secs(age_secs)).unwrap();
}

#[test] // S§8.6 "At send", S§19 #4 send part
fn aws_key_is_refused_with_its_pattern_id_and_nothing_is_written() {
    let f = fx();
    let e = send(&f, Some("alice"), &addr("bob", "b"), &format!("the key is {}", aws())).unwrap_err();
    assert_eq!(e, SendError::Secret(vec!["aws_access_key_id".to_string()]));
    assert_eq!(e.line(), "zygo: message not sent: it holds a secret (aws_access_key_id)");
    assert!(!e.line().contains(&aws()));
    assert!(!f.sp.outbox.exists());
}

#[test] // S§8.6: ids joined with ", "
fn two_findings_name_both_ids() {
    let f = fx();
    let mut cfg = builtin();
    cfg.pattern.push(RedactionPattern { id: "ship_word".into(), pattern: "zz-forbidden-zz".into(), action: Action::Block, description: "d".into() });
    let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status",
        &format!("say zz-forbidden-zz with {}", aws()), &cfg, &f.root, now(), &far()).unwrap_err();
    assert_eq!(e.line(), "zygo: message not sent: it holds a secret (aws_access_key_id, ship_word)");
    assert!(!f.sp.outbox.exists());
}

#[test] // S§8.6 "Could not run"
fn check_that_cannot_run_is_refused_with_its_check_name() {
    let f = fx();
    let bad = RedactionConfig {
        pattern: vec![RedactionPattern { id: "bad".into(), pattern: "(".into(), action: Action::Redact, description: "d".into() }],
        ..Default::default()
    };
    let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status", "hi", &bad, &f.root, now(), &far()).unwrap_err();
    assert_eq!(e, SendError::CheckFailed("redaction_patterns"));
    assert_eq!(e.line(), "zygo: message not sent: the secret check could not run (redaction_patterns); send it again");
    let expire = DeadlinePolicy { expire_after_scans: Some(0), ..far() };
    let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now(), &expire).unwrap_err();
    assert_eq!(e.line(), "zygo: message not sent: the secret check could not run (secret_timeout); send it again");
    assert!(!f.sp.outbox.exists());
}

#[test] // S§7.2 refusal 1, S§19 #21
fn sender_with_no_name_is_refused() {
    let f = fx();
    for from in [None, Some("cli"), Some(KEY)] {
        let e = send_remote(&f.sp, Ok(&on()), from, &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now()).unwrap_err();
        assert_eq!(e, SendError::NoSenderName);
        assert_eq!(e.line(), NO_NAME);
    }
    assert!(!f.sp.outbox.exists());
}

#[test] // S§7.2 refusal 2
fn sync_off_is_refused() {
    let f = fx();
    for off in [SyncOff::NotOn, SyncOff::NoHost, SyncOff::BadHost, SyncOff::BadBranch, SyncOff::BadGap] {
        let e = send_remote(&f.sp, Err(&off), Some("alice"), &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now()).unwrap_err();
        assert_eq!(e, SendError::SyncOff);
        assert_eq!(e.line(), "zygo: mail sync is off for this workspace");
    }
    assert!(!f.sp.outbox.exists());
}

fn verdict(f: &Fx, v: &str) {
    save(&f.sp, &State { public: Some(PublicCache { url_sha256: vec!["0".repeat(64)], verdict: v.into(), at: 0 }), ..Default::default() }).unwrap();
}

#[test] // S§7.2 refusal 3, S§8.2: the last check, at any age
fn last_public_verdict_is_refused_and_the_others_send() {
    let f = fx();
    verdict(&f, "public");
    let e = send_remote(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), "status", "hi", &builtin(), &f.root, now()).unwrap_err();
    assert_eq!(e, SendError::Public);
    assert_eq!(e.line(), "zygo: the remote can be read without a login; mail sync refused");
    assert!(!f.sp.outbox.exists());
    for v in ["private", "local", "unknown"] {
        verdict(&f, v);
        send(&f, Some("alice"), &addr("bob", "b"), "hi").unwrap();
    }
    assert_eq!(count(&f.sp).unwrap(), 3);
}

#[test] // S§7.2 refusal 4 (as M§10), S§19 #21
fn unknown_address_is_refused_with_the_known_addresses() {
    let f = fx();
    let e = send(&f, Some("alice"), &addr("dave", "b"), "hi").unwrap_err();
    assert_eq!(e, SendError::UnknownAddress(vec!["bob@b".to_string(), "carol@c".to_string()]));
    assert_eq!(e.line(), "zygo: unknown address; known addresses: bob@b, carol@c");
    assert_eq!(resolve_to("cli@b", &["a".to_string()]), Err(AddrError::Reserved));
    assert!(matches!(send(&f, Some("alice"), &addr("cli", "b"), "hi"), Err(SendError::UnknownAddress(_))));
    assert!(matches!(send(&f, Some("alice"), &addr("zygo", "b"), "hi"), Err(SendError::UnknownAddress(_))));
    fs::remove_file(&f.sp.members).unwrap();
    assert_eq!(send(&f, Some("alice"), &addr("bob", "b"), "hi").unwrap_err().line(), "zygo: unknown address; known addresses: none");
    assert!(!f.sp.outbox.exists());
}

#[test] // M§5 limits stay; nothing is written
fn subject_over_the_limit_is_refused() {
    let f = fx();
    let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), &"s".repeat(201), "hi", &builtin(), &f.root, now(), &far()).unwrap_err();
    assert_eq!(e, SendError::Io("zygo: subject is longer than 200 bytes".into()));
    assert_eq!(e.line(), "zygo: subject is longer than 200 bytes");
    assert!(!f.sp.outbox.exists());
}

#[test] // S§5 Outbox, S§6.2, M§5 send through tmp/
fn clear_send_writes_one_0600_file_with_full_addresses() {
    let f = fx();
    let id = send(&f, Some("alice"), &addr("bob", "b"), "the build is green").unwrap();
    let file = f.sp.outbox.join(format!("{id}.json"));
    assert_eq!(pending(&f.sp).unwrap(), vec![(id, file.clone())]);
    let bytes = fs::read(&file).unwrap();
    let m: Message = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&m).unwrap());
    assert_eq!((m.version, m.id.clone(), m.from.as_str(), m.to.as_str()), (1, id.to_string(), "alice@a", "bob@b"));
    assert_eq!((m.sent_at.as_str(), m.subject.as_str(), m.body.as_str()), ("2027-01-15T08:00:00Z", "status", "the build is green"));
    assert_eq!(fs::read_dir(&f.sp.outbox_tmp).unwrap().count(), 0);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (1, 0));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!((mode(&file), mode(&f.sp.outbox), mode(&f.sp.outbox_tmp)), (0o600, 0o700, 0o700));
        assert_eq!(fs::metadata(&file).unwrap().nlink(), 1);
    }
}

#[test] // a write that fails partway leaves nothing in outbox/
fn failed_write_leaves_no_outbox_file() {
    let f = fx();
    fs::create_dir_all(&f.sp.outbox).unwrap();
    fs::write(&f.sp.outbox_tmp, b"").unwrap(); // tmp/ is a file: the folder check refuses it
    let e = send(&f, Some("alice"), &addr("bob", "b"), "hi").unwrap_err();
    assert!(matches!(&e, SendError::Io(l) if l.starts_with("zygo: mail: ")), "{e:?}");
    assert_eq!(pending(&f.sp).unwrap(), vec![]);
    assert_eq!(count(&f.sp).unwrap(), 0);
}

#[test]
fn pending_is_ulid_ordered_regular_files_only() {
    let f = fx();
    let (a, b, c) = (Ulid::from_parts(1, 1), Ulid::from_parts(2, 1), Ulid::from_parts(3, 1));
    for u in [b, a] { put(&f.sp.outbox.join(format!("{u}.json")), 0); }
    put(&f.sp.outbox.join("notes.txt"), 0);
    put(&f.sp.outbox.join(format!("{}.json", a.to_string().to_lowercase())), 0);
    put(&f.sp.outbox_tmp.join(format!("{c}.json")), 0);
    put(&f.sp.refused.join(format!("{c}.json")), 0);
    #[cfg(unix)]
    std::os::unix::fs::symlink(f.sp.refused.join(format!("{c}.json")), f.sp.outbox.join(format!("{c}.json"))).unwrap();
    assert_eq!(pending(&f.sp).unwrap(), vec![(a, f.sp.outbox.join(format!("{a}.json"))), (b, f.sp.outbox.join(format!("{b}.json")))]);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (2, 1));
}

#[test]
fn no_outbox_is_empty() {
    let f = fx();
    assert_eq!(pending(&f.sp).unwrap(), vec![]);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 0));
}

fn aws_ids() -> Vec<String> { vec!["aws_access_key_id".to_string()] }

#[test] // S§8.6 "At commit" finding
fn refuse_moves_the_file_and_notifies_the_sender_once() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    let before = fs::read(f.sp.outbox.join(format!("{id}.json"))).unwrap();
    let mut calls: Vec<(String, String, String)> = Vec::new();
    refuse(&f.sp, id, &aws_ids(), &mut |to: &str, subject: &str, body: &str| {
        calls.push((to.into(), subject.into(), body.into()));
        Ok(())
    }, now()).unwrap();
    assert_eq!(calls, vec![(
        "alice".to_string(),
        format!("not sent: {id}"),
        format!("zygo did not send message {id} to bob@b: it holds a secret (aws_access_key_id). The message is kept on this machine in mail/outbox/refused/."),
    )]);
    assert_eq!(fs::read(f.sp.refused.join(format!("{id}.json"))).unwrap(), before);
    assert_eq!(pending(&f.sp).unwrap(), vec![]);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
    #[cfg(unix)]
    assert_eq!(mode(&f.sp.refused), 0o700);
}

#[test] // S§8.6: the CLI as `cli` has no inbox
fn refuse_for_cli_sends_no_notice() {
    let f = fx();
    let id = outbox_file(&f, "cli@a");
    let mut n = 0;
    refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }, now()).unwrap();
    assert_eq!(n, 0);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
}

#[test] // the notice is a local M§5 message from `zygo`
fn notify_can_deliver_the_notice_from_zygo() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    refuse(&f.sp, id, &aws_ids(), &mut |to: &str, subject: &str, body: &str| {
        let m = Message::new("zygo", to, subject, body, now()).map_err(std::io::Error::other)?;
        Ok(deliver(&f.sp.mail, KEY, to, &m, 500).map_err(std::io::Error::other)?)
    }, now()).unwrap();
    let l = list(&f.sp.mail, KEY).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!((l[0].from.as_str(), l[0].subject.clone()), ("zygo", format!("not sent: {id}")));
}

#[test] // crash after the notice and the link, before the outbox name was removed: one more notice (S§8.6 accepted cost)
fn refuse_again_after_a_partial_move_notifies_once() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    fs::create_dir_all(&f.sp.refused).unwrap();
    fs::hard_link(f.sp.outbox.join(format!("{id}.json")), f.sp.refused.join(format!("{id}.json"))).unwrap();
    let mut n = 0;
    refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }, now()).unwrap();
    assert_eq!(n, 1);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
}

#[test]
fn refuse_of_a_missing_file_changes_nothing() {
    let f = fx();
    let mut n = 0;
    let e = refuse(&f.sp, Ulid::from_parts(1, 1), &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }, now()).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    assert_eq!((n, refused_count(&f.sp).unwrap()), (0, 0));
}

#[test] // S§9 last paragraph, S§15 `mail/outbox/` row
fn sweep_deletes_old_refused_and_tmp_files_only() {
    let f = fx();
    let keep = parse_duration(&MailConfig::default().keep).unwrap(); // 30d
    let k = keep.as_secs();
    let old_refused = f.sp.refused.join("a.json");
    let kept_refused = f.sp.refused.join("b.json");
    let old_tmp = f.sp.outbox_tmp.join("a.json");
    let kept_tmp = f.sp.outbox_tmp.join("b.json");
    let queued = f.sp.outbox.join(format!("{}.json", Ulid::from_parts(1, 1)));
    let incoming = f.sp.incoming.join("a.json");
    put(&old_refused, k + 1);
    put(&kept_refused, k);
    put(&old_tmp, 3601);
    put(&kept_tmp, 3600);
    put(&queued, 10 * 365 * 86400);
    put(&incoming, 10 * 365 * 86400);
    sweep(&f.sp, keep, now());
    assert!(!old_refused.exists() && !old_tmp.exists());
    assert!(kept_refused.exists() && kept_tmp.exists() && queued.exists() && incoming.exists());
}

#[cfg(unix)]
#[test] // a refused/ that is a link is skipped; its target is never touched
fn sweep_skips_a_linked_folder() {
    let f = fx();
    let outside = f._t.path().join("outside/a.json");
    put(&outside, 10 * 365 * 86400);
    fs::create_dir_all(&f.sp.outbox).unwrap();
    std::os::unix::fs::symlink(outside.parent().unwrap(), &f.sp.refused).unwrap();
    sweep(&f.sp, Duration::from_secs(1), now());
    assert!(outside.exists());
}

#[test]
fn sweep_with_no_outbox_does_nothing() {
    let f = fx();
    sweep(&f.sp, Duration::from_secs(1), now());
    assert!(!f.sp.outbox.exists());
}

fn no_notice(n: &mut u32) -> impl FnMut(&str, &str, &str) -> Result<(), NoticeError> + '_ {
    move |_: &str, _: &str, _: &str| { *n += 1; Ok(()) }
}

fn set_age(p: &Path, age_secs: u64) {
    fs::File::options().write(true).open(p).unwrap().set_modified(now() - Duration::from_secs(age_secs)).unwrap();
}

#[cfg(unix)]
#[test] // S§19 #20: an outbox that cannot be read is no count of 0
fn unreadable_outbox_count_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: the test runs as root, which reads a mode-0000 folder");
        return;
    }
    let f = fx();
    outbox_file(&f, "alice@a");
    outbox_file(&f, "alice@a");
    fs::create_dir_all(&f.sp.refused).unwrap();
    fs::set_permissions(&f.sp.refused, fs::Permissions::from_mode(0o000)).unwrap();
    let refused = refused_count(&f.sp);
    fs::set_permissions(&f.sp.refused, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&f.sp.outbox, fs::Permissions::from_mode(0o000)).unwrap();
    let (c, p) = (count(&f.sp), pending(&f.sp));
    fs::set_permissions(&f.sp.outbox, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(c.is_err() && p.is_err() && refused.is_err(), "{c:?} {p:?} {refused:?}");
    assert_eq!(count(&f.sp).unwrap(), 2);
}

#[test] // S§8.6 "kept on this machine": refused/ age counts from the refusal
fn refused_file_ages_from_the_refusal() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    set_age(&f.sp.outbox.join(format!("{id}.json")), 40 * 86400);
    let mut n = 0;
    refuse(&f.sp, id, &aws_ids(), &mut no_notice(&mut n), now()).unwrap();
    sweep(&f.sp, Duration::from_secs(30 * 86400), now());
    assert_eq!(refused_count(&f.sp).unwrap(), 1);
}

#[test] // a refused/ file with other bytes is never taken for the outbox file
fn refuse_onto_a_different_refused_file_keeps_the_outbox_file() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    let src = f.sp.outbox.join(format!("{id}.json"));
    let dst = f.sp.refused.join(format!("{id}.json"));
    let before = fs::read(&src).unwrap();
    fs::create_dir_all(&f.sp.refused).unwrap();
    fs::write(&dst, b"other").unwrap();
    let mut n = 0;
    assert!(refuse(&f.sp, id, &aws_ids(), &mut no_notice(&mut n), now()).is_err());
    assert_eq!(n, 0);
    assert_eq!(fs::read(&src).unwrap(), before);
    assert_eq!(fs::read(&dst).unwrap(), b"other");
}

#[cfg(unix)]
#[test] // the folder check of send_remote holds for pending and sweep too
fn linked_outbox_is_refused_by_pending_and_not_swept() {
    let f = fx();
    let outside = f._t.path().join("outside");
    let (old_refused, old_tmp) = (outside.join("refused/a.json"), outside.join("tmp/a.json"));
    put(&old_refused, 10 * 365 * 86400);
    put(&old_tmp, 10 * 365 * 86400);
    put(&outside.join(format!("{}.json", Ulid::from_parts(1, 1))), 0);
    std::os::unix::fs::symlink(&outside, &f.sp.outbox).unwrap();
    assert!(pending(&f.sp).is_err());
    assert!(count(&f.sp).is_err() && refused_count(&f.sp).is_err());
    sweep(&f.sp, Duration::from_secs(1), now());
    assert!(old_refused.exists() && old_tmp.exists());
}

#[test] // M§5 body limit stays; nothing is written
fn body_over_the_limit_is_refused() {
    let f = fx();
    let e = send(&f, Some("alice"), &addr("bob", "b"), &"b".repeat(BODY_MAX + 1)).unwrap_err();
    assert_eq!(e, SendError::Io("zygo: body is larger than 1 MiB".into()));
    assert!(!f.sp.outbox.exists());
}

#[test] // S§8.6: the pass covers every field, the subject too
fn secret_in_the_subject_is_refused() {
    let f = fx();
    let subject = format!("key {}", aws());
    let e = send_remote_with(&f.sp, Ok(&on()), Some("alice"), &addr("bob", "b"), &subject, "hi", &builtin(), &f.root, now(), &far()).unwrap_err();
    assert_eq!(e, SendError::Secret(aws_ids()));
    assert!(!f.sp.outbox.exists());
}

#[test] // a file that does not parse is moved and gets no notice
fn refuse_of_an_unparsable_file_moves_it_without_notice() {
    let f = fx();
    let id = Ulid::from_parts(1, 1);
    put(&f.sp.outbox.join(format!("{id}.json")), 0);
    let mut n = 0;
    refuse(&f.sp, id, &aws_ids(), &mut no_notice(&mut n), now()).unwrap();
    assert_eq!(n, 0);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
}

#[test] // S§8.6 (FX-24 review F5): the notice comes before the move; a notice not delivered leaves the message queued
fn notify_error_keeps_the_message_in_the_outbox_for_the_next_pass() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    let before = fs::read(f.sp.outbox.join(format!("{id}.json"))).unwrap();
    let e = refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| Err(std::io::Error::other("busy").into()), now()).unwrap_err();
    assert_eq!(e.to_string(), "busy");
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (1, 0));
    assert_eq!(fs::read(f.sp.outbox.join(format!("{id}.json"))).unwrap(), before);
    let mut n = 0;
    assert_eq!(refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| { n += 1; Ok(()) }, now()).unwrap(), None);
    assert_eq!(n, 1);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
}

#[test] // S§8.6 (FX-24 review R2 F1): a notice that no pass can deliver moves the message, and its reason comes back
fn lasting_notice_error_moves_the_message_with_its_reason() {
    let f = fx();
    let id = outbox_file(&f, "alice@a");
    let before = fs::read(f.sp.outbox.join(format!("{id}.json"))).unwrap();
    let why = refuse(&f.sp, id, &aws_ids(), &mut |_: &str, _: &str, _: &str| Err(NoticeError::Never("full")), now()).unwrap();
    assert_eq!(why, Some("full"));
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
    assert_eq!(fs::read(f.sp.refused.join(format!("{id}.json"))).unwrap(), before);
}

#[test] // M§5 FILE_MAX: a file over it is not read whole; it is moved with no notice, as a file that does not parse
fn refuse_of_a_file_over_the_read_limit_moves_it_without_notice() {
    let f = fx();
    let mut m = Message::new("alice@a", "bob@b", "status", "hello", now()).unwrap();
    m.body = "b".repeat(FILE_MAX as usize);
    let id: Ulid = m.id.parse().unwrap();
    fs::create_dir_all(&f.sp.outbox).unwrap();
    fs::write(f.sp.outbox.join(format!("{id}.json")), serde_json::to_vec(&m).unwrap()).unwrap();
    let mut n = 0;
    refuse(&f.sp, id, &aws_ids(), &mut no_notice(&mut n), now()).unwrap();
    assert_eq!(n, 0);
    assert_eq!((count(&f.sp).unwrap(), refused_count(&f.sp).unwrap()), (0, 1));
}
