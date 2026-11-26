//! ML-12, ML-13: zygo mail send|read|ls|watch and the command-start mail sweep (M§5, M§8, M§10, M§13; W OP5, AU71).
mod common;

#[cfg(unix)]
#[path = "common/e2e.rs"]
mod e2e;

#[cfg(unix)]
mod unix {
    use std::cell::{Cell, RefCell};
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::{Output, Stdio};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use zygo_config::MailConfig;
    use zygo_core::Agent;
    use zygo_workspace::mail::{mail_dir, store, BODY_MAX};
    use zygo_workspace::state::{MemberRecord, Workspace};

    use crate::common::feed_stdin;
    use crate::common::workspace::{Harness, WS_ID};
    use crate::e2e::set_mtime;

    const KEY: &str = "kaaaaaaaa";
    const KEY2: &str = "kbbbbbbbb";
    const DAY: Duration = Duration::from_secs(86400);

    fn t0() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// The workspace `WS_ID` with `elena` (`KEY`) and `bob` (`KEY2`).
    fn ws(h: &Harness) -> Workspace {
        h.seed(WS_ID, &mut |w| {
            for (k, n) in [(KEY, "elena"), (KEY2, "bob")] {
                w.members.insert(k.to_string(), MemberRecord { name: Some(n.to_string()), ..MemberRecord::new(Agent::ClaudeCode) });
            }
        })
    }

    fn dir(h: &Harness) -> PathBuf {
        mail_dir(&h.paths, WS_ID)
    }

    /// Fixture mail from `cli` at `T0 + n s`; returns the id.
    fn put(h: &Harness, w: &Workspace, to: &str, subject: &str, body: &str, n: u64) -> String {
        let r = zygo_workspace::mail::send(&h.paths, w, None, to, subject, body, &MailConfig::default(), t0() + Duration::from_secs(n))
            .unwrap();
        assert!(r.refused.is_empty());
        r.sent[0].id.clone()
    }

    /// `zygo <args>` in the workspace `WS_ID`, with `extra` set.
    fn run(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)]) -> Output {
        let mut env = vec![("ZYGO_WORKSPACE", OsStr::new(WS_ID))];
        env.extend_from_slice(extra);
        h.zygo(args, &env).output().unwrap()
    }

    /// `run` with `input` on stdin.
    fn run_in(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)], input: &[u8]) -> Output {
        let mut env = vec![("ZYGO_WORKSPACE", OsStr::new(WS_ID))];
        env.extend_from_slice(extra);
        let mut c = h.zygo(args, &env);
        c.stdin(Stdio::piped());
        let mut child = c.spawn().unwrap();
        feed_stdin(child.stdin.take().unwrap(), input);
        child.wait_with_output().unwrap()
    }

    fn out(o: &Output) -> String {
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn err(o: &Output) -> String {
        String::from_utf8_lossy(&o.stderr).into_owned()
    }

    #[test]
    fn send_file_stdin_and_subject_deliver_as_cli() {
        let h = Harness::no_tmux();
        ws(&h);
        let body = h.root().join("body.txt");
        std::fs::write(&body, "line1\nline2").unwrap();
        let o = run(&h, &["mail", "send", "elena", "--subject", "hi", "--file", body.to_str().unwrap()], &[]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        let m = store::read(&dir(&h), KEY, 10).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(
            (m[0].from.as_str(), m[0].to.as_str(), m[0].subject.as_str(), m[0].body.as_str()),
            ("cli", "elena", "hi", "line1\nline2")
        );
        assert_eq!(out(&o), format!("sent {} to elena\n", m[0].id));
        let o = run_in(&h, &["mail", "send", "elena", "-"], &[], b"from stdin");
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        let m = store::read(&dir(&h), KEY, 10).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].body.as_str(), m[0].subject.as_str()), ("from stdin", ""));
    }

    #[test]
    fn send_as_member_when_zygo_pane_is_set() {
        let h = Harness::no_tmux();
        ws(&h);
        let o = run_in(&h, &["mail", "send", "elena", "--subject", "s", "-"], &[("ZYGO_PANE", OsStr::new(KEY2))], b"b");
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        let m = store::read(&dir(&h), KEY, 10).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].from, "bob");
    }

    #[test]
    fn send_to_unknown_name_exits_1_naming_known_names() {
        let h = Harness::no_tmux();
        ws(&h);
        let o = run(&h, &["mail", "send", "nobody", "--subject", "s"], &[]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(err(&o), "zygo: unknown target; known names: bob, elena\n");
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 0);
        assert_eq!(store::unread(&dir(&h), KEY2).unwrap(), 0);
    }

    #[test]
    fn send_body_over_1_mib_is_refused() {
        let h = Harness::no_tmux();
        ws(&h);
        let big = h.root().join("big.txt");
        std::fs::write(&big, vec![b'a'; BODY_MAX + 1]).unwrap();
        let o = run(&h, &["mail", "send", "elena", "--file", big.to_str().unwrap()], &[]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(err(&o), "zygo: body is larger than 1 MiB\n");
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 0);
    }

    #[test]
    fn read_defaults_to_this_pane_and_takes_name_and_max() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        let id1 = put(&h, &w, "elena", "s1", "b1", 1);
        put(&h, &w, "elena", "s2", "b2", 2);
        put(&h, &w, "bob", "s3", "b3", 3);
        let pane = [("ZYGO_PANE", OsStr::new(KEY))];
        let o = run(&h, &["mail", "read", "--max", "1"], &pane);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        let text = out(&o);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].starts_with(&format!("{id1} cli ")), "{text}");
        assert!(lines[0].ends_with(" s1"), "{text}");
        assert_eq!(lines[1], "  b1");
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 1);
        let o = run(&h, &["mail", "read", "--name", "bob"], &pane);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(out(&o).contains(" s3"), "{}", out(&o));
        assert_eq!(store::unread(&dir(&h), KEY2).unwrap(), 0);
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 1);
        let o = run(&h, &["mail", "read"], &[]);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(err(&o), "zygo mail read: no member for this pane; pass --name\n");
        let o = run(&h, &["mail", "read", "--name", "bob"], &pane);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(out(&o), "no new messages\n");
    }

    #[test]
    fn ls_counts_per_member_or_subjects_for_one() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        let id1 = put(&h, &w, "elena", "s1", "b1", 1);
        let id2 = put(&h, &w, "elena", "s2", "b2", 2);
        for args in [&["mail", "ls"][..], &["mail"][..]] {
            let o = run(&h, args, &[]);
            assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
            assert_eq!(out(&o), "kaaaaaaaa elena 2\nkbbbbbbbb bob 0\n", "{args:?}");
        }
        let o = run(&h, &["mail", "ls", "elena"], &[]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(out(&o), format!("{id1} cli s1\n{id2} cli s2\n"));
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 2);
        let id = put(&h, &w, "bob", "a\u{1b}b", "x", 3);
        let o = run(&h, &["mail", "ls", "bob"], &[]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(out(&o), format!("{id} cli a\\u{{1b}}b\n"));
    }

    /// `dir/KEY/<sub>/<name>` with `{}`, its folders made with mode 0700.
    fn file_at(h: &Harness, sub: &str, name: &str) -> PathBuf {
        use std::os::unix::fs::DirBuilderExt;
        let d = dir(h).join(KEY).join(sub);
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, "{}").unwrap();
        p
    }

    fn zygo_plain(h: &Harness, args: &[&str]) -> Output {
        h.zygo(args, &[]).output().unwrap()
    }

    fn config(h: &Harness, body: &str) {
        std::fs::write(h.root().join("config.toml"), body).unwrap();
    }

    fn exists(p: &Path) -> bool {
        std::fs::symlink_metadata(p).is_ok()
    }

    #[test] // M§5, AU71
    fn sweep_at_command_start_deletes_old_cur_files() {
        let h = Harness::no_tmux();
        let old = file_at(&h, "cur", "01J0000000000000000000000A.json");
        let fresh = file_at(&h, "cur", "01J0000000000000000000000B.json");
        let new_old = file_at(&h, "new", "01J0000000000000000000000C.json");
        let now = SystemTime::now();
        set_mtime(&old, now - 31 * DAY);
        set_mtime(&new_old, now - 31 * DAY);
        set_mtime(&fresh, now - 29 * DAY);
        let o = zygo_plain(&h, &["hook", "bogus"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(exists(&old), "hook swept mail");
        let _ = zygo_plain(&h, &["ws", "name", KEY, "x", "--workspace", "nope"]);
        assert!(exists(&old), "ws swept mail");
        let o = zygo_plain(&h, &["help"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(!exists(&old), "help did not sweep");
        assert!(exists(&fresh) && exists(&new_old));
        config(&h, "[mail]\nkeep = \"1d\"\n");
        let o = zygo_plain(&h, &["help"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(!exists(&fresh), "keep 1d did not sweep");
        assert!(exists(&new_old), "the sweep touched new/");
    }

    /// Old `cur/` and `tmp/` files of `KEY` under the mail folder `m`, made with mode 0700; returns the files.
    fn old_mail_at(m: &Path) -> Vec<PathBuf> {
        use std::os::unix::fs::DirBuilderExt;
        let mut out = Vec::new();
        for (sub, name) in [("cur", "01J0000000000000000000000A.json"), ("tmp", "01J0000000000000000000000B.json")] {
            let d = m.join(KEY).join(sub);
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&d).unwrap();
            let p = d.join(name);
            std::fs::write(&p, "{}").unwrap();
            set_mtime(&p, SystemTime::now() - 400 * DAY);
            out.push(p);
        }
        out
    }

    #[test] // H1: a workspace folder or a `mail/` folder that is a link is never opened (admin.rs:410)
    fn sweep_does_not_follow_a_linked_workspace_or_mail_folder() {
        let h = Harness::no_tmux();
        let control = file_at(&h, "cur", "01J0000000000000000000000A.json");
        set_mtime(&control, SystemTime::now() - 400 * DAY);
        let wss = h.paths.workspaces_dir();
        let t1 = h.root().join("target-ws");
        let mut kept = old_mail_at(&t1.join("mail"));
        std::os::unix::fs::symlink(&t1, wss.join("1111111111111111")).unwrap();
        let t2 = h.root().join("target-mail");
        kept.extend(old_mail_at(&t2));
        std::fs::create_dir_all(wss.join("2222222222222222")).unwrap();
        std::os::unix::fs::symlink(&t2, wss.join("2222222222222222").join("mail")).unwrap();
        let o = zygo_plain(&h, &["help"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(!exists(&control), "help did not sweep");
        for p in &kept {
            assert!(exists(p), "the sweep followed a link to {}", p.display());
        }
    }

    #[test] // M1: the messages already moved to `cur/` are printed; the failed member is named
    fn read_over_members_prints_what_it_has_when_a_later_member_fails() {
        use std::os::unix::fs::PermissionsExt;
        let h = Harness::no_tmux();
        let w = ws(&h);
        let id1 = put(&h, &w, "elena", "s1", "b1", 1);
        put(&h, &w, "bob", "s2", "b2", 2);
        let new = dir(&h).join(KEY2).join("new");
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o000)).unwrap();
        let o = run(&h, &["mail", "read", "--name", "all"], &[]);
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(o.status.code(), Some(1), "{}", err(&o));
        let text = out(&o);
        assert!(text.starts_with(&format!("{id1} cli ")), "{text}");
        assert!(text.contains("  b1\n"), "{text}");
        assert!(err(&o).contains("mail of kbbbbbbbb (bob)"), "{}", err(&o));
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 0);
    }

    #[test] // L1: a refused send names the target key and member name
    fn refused_send_names_the_member() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        put(&h, &w, "elena", "s1", "b1", 1);
        config(&h, "[mail]\nmax_unread = 1\n");
        let o = run(&h, &["mail", "send", "elena", "--subject", "s"], &[]);
        assert_eq!(o.status.code(), Some(1), "{}", err(&o));
        assert_eq!(err(&o), "zygo: elena has 1 unread messages; to kaaaaaaaa (elena)\n");
    }

    /// The `/zygo:mail` reply through `cmd::mail::verb`.
    fn slash(h: &Harness, args: &[&str]) -> String {
        // Every caller sets the same value; the other tests run zygo with a cleared environment.
        unsafe { std::env::set_var("ZYGO_WORKSPACE", WS_ID) };
        let ctx = zygo::cmd::verbs::VerbCtx { slash: true, save: false, session_dir: None, paths: h.paths.clone() };
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        zygo::cmd::mail::verb(&ctx, &a).unwrap()
    }

    #[test] // M§8: `/zygo:mail ls` and `/zygo:mail ls <name>` reply as `zygo mail ls`
    fn slash_ls_replies_with_counts_and_subjects() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        let id1 = put(&h, &w, "elena", "s1", "b1", 1);
        assert_eq!(slash(&h, &[]), "kaaaaaaaa elena 1\nkbbbbbbbb bob 0");
        assert_eq!(slash(&h, &["ls"]), "kaaaaaaaa elena 1\nkbbbbbbbb bob 0");
        assert_eq!(slash(&h, &["ls", "elena"]), format!("{id1} cli s1"));
        assert_eq!(slash(&h, &["ls", "bob"]), "no new messages");
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 1);
    }

    #[test] // Open 2
    fn sweep_skips_a_keep_that_does_not_parse() {
        let h = Harness::no_tmux();
        let old = file_at(&h, "cur", "01J0000000000000000000000A.json");
        set_mtime(&old, SystemTime::now() - 400 * DAY);
        config(&h, "[mail]\nkeep = \"soon\"\n");
        let o = zygo_plain(&h, &["help"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(exists(&old));
    }
    /// A clock that returns `times[i]` at its i-th call (the last value after the end) and counts its calls.
    struct Clock {
        times: Vec<Instant>,
        calls: Cell<usize>,
    }

    impl Clock {
        fn now(&self) -> Instant {
            let i = self.calls.get().min(self.times.len() - 1);
            self.calls.set(self.calls.get() + 1);
            self.times[i]
        }
    }

    #[test] // M§8; esc
    fn watch_prints_one_line_per_new_message_and_keeps_them_in_new() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        let id1 = put(&h, &w, "elena", "s1", "b1", 1);
        let id2 = put(&h, &w, "elena", "a\u{1b}b", "b2", 2);
        let t = Instant::now();
        let c = Clock { times: vec![t], calls: Cell::new(0) };
        let mut o = Vec::new();
        let code = zygo::cmd::mail::watch(&dir(&h), KEY, Duration::from_secs(1500), Duration::ZERO, &|| c.now(), &mut o);
        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(o).unwrap(), format!("{id1} cli s1\n{id2} cli a\\u{{1b}}b\n"));
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 2);
    }

    #[test] // M-G5, M§13 #7 send part: the clock sends on its 2nd call
    fn watch_returns_when_mail_lands_during_the_watch() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        let sent = RefCell::new(None);
        let calls = Cell::new(0);
        let t = Instant::now();
        let now = || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                *sent.borrow_mut() = Some(put(&h, &w, "elena", "late", "b", 1));
            }
            t
        };
        let mut o = Vec::new();
        assert_eq!(zygo::cmd::mail::watch(&dir(&h), KEY, Duration::from_secs(1500), Duration::ZERO, &now, &mut o), 0);
        assert_eq!(String::from_utf8(o).unwrap(), format!("{} cli late\n", sent.borrow().as_ref().unwrap()));
        assert_eq!(calls.get(), 2);
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 1);
    }

    #[test] // M§8 exit 1; M§13 #7 timeout part
    fn watch_exits_1_at_the_timeout_with_no_line() {
        let h = Harness::no_tmux();
        let _w = ws(&h);
        let t = Instant::now();
        let to = Duration::from_secs(1500);
        let c = Clock { times: vec![t, t + to - Duration::from_secs(1), t + to], calls: Cell::new(0) };
        let mut o = Vec::new();
        assert_eq!(zygo::cmd::mail::watch(&dir(&h), KEY, to, Duration::ZERO, &|| c.now(), &mut o), 1);
        assert!(o.is_empty());
        assert_eq!(c.calls.get(), 3); // start, 24m59s (goes on), 25m (exits)
    }

    #[test] // M§8 default 25m
    fn watch_default_timeout_is_25m() {
        use clap::{Args, FromArgMatches};
        use zygo::cmd::mail::{MailArgs, MailCmd};
        let h = Harness::no_tmux();
        let _w = ws(&h);
        let parse = |argv: &[&str]| {
            MailArgs::from_arg_matches(&MailArgs::augment_args(clap::Command::new("mail")).try_get_matches_from(argv).unwrap())
                .unwrap()
        };
        match parse(&["mail", "watch"]).sub {
            Some(MailCmd::Watch { name: None, timeout }) => {
                assert_eq!(timeout, "25m");
                assert_eq!(zygo_config::parse_duration(&timeout), Some(Duration::from_secs(1500)));
            }
            _ => panic!("not watch"),
        }
        match parse(&["mail", "watch", "bob", "--timeout", "90s"]).sub {
            Some(MailCmd::Watch { name: Some(n), timeout }) => assert_eq!((n.as_str(), timeout.as_str()), ("bob", "90s")),
            _ => panic!("not watch"),
        }
    }

    #[test] // `--timeout` takes the `parse_duration` forms; no wait (A4)
    fn watch_cli_takes_duration_forms_a_name_or_this_pane() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        for t in ["0s", "0m", "0h", "0d"] {
            let o = run(&h, &["mail", "watch", "elena", "--timeout", t], &[]);
            assert_eq!(o.status.code(), Some(1), "{t}: {}", err(&o));
            assert_eq!(out(&o), "", "{t}");
        }
        let id = put(&h, &w, "bob", "s", "b", 1);
        let o = run(&h, &["mail", "watch", "bob", "--timeout", "0s"], &[]);
        assert_eq!((o.status.code(), out(&o)), (Some(0), format!("{id} cli s\n")), "{}", err(&o));
        let o = run(&h, &["mail", "watch", KEY2, "--timeout", "0s"], &[]);
        assert_eq!((o.status.code(), out(&o)), (Some(0), format!("{id} cli s\n")), "{}", err(&o));
        let o = run(&h, &["mail", "watch", "--timeout", "0s"], &[("ZYGO_PANE", OsStr::new(KEY2))]);
        assert_eq!((o.status.code(), out(&o)), (Some(0), format!("{id} cli s\n")), "{}", err(&o));
        assert_eq!(store::unread(&dir(&h), KEY2).unwrap(), 1);
    }

    #[test] // A1, A2, A3
    fn watch_cli_refuses_a_bad_timeout_no_member_and_an_unknown_name() {
        let h = Harness::no_tmux();
        let _w = ws(&h);
        let o = run(&h, &["mail", "watch", "elena", "--timeout", "soon"], &[]);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(err(&o), "zygo: bad --timeout value soon: use <n>s, <n>m, <n>h or <n>d\n");
        let o = run(&h, &["mail", "watch", "--timeout", "0s"], &[]);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(err(&o), "zygo mail watch: no member for this pane; pass a name\n");
        let o = run(&h, &["mail", "watch", "nobody", "--timeout", "0s"], &[]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(err(&o), "zygo: unknown target; known names: bob, elena\n");
        let o = run(&h, &["mail", "watch", "all", "--timeout", "0s"], &[]);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(err(&o), "zygo mail watch: all is not one member\n");
        assert_eq!(out(&o), "");
    }

    #[test] // A5
    fn watch_read_error_exits_1_with_one_stderr_line() {
        use std::os::unix::fs::PermissionsExt;
        let h = Harness::no_tmux();
        let w = ws(&h);
        put(&h, &w, "elena", "s1", "b1", 1);
        let new = dir(&h).join(KEY).join("new");
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o000)).unwrap();
        let o = run(&h, &["mail", "watch", "elena", "--timeout", "0s"], &[]);
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(out(&o), "");
        assert!(err(&o).starts_with("zygo: mail: ") && err(&o).ends_with('\n') && err(&o).lines().count() == 1, "{}", err(&o));
    }
    #[test] // R1: M§8 "until a file lands in new/": a file that does not parse ends the watch with no line
    fn watch_exits_0_with_no_line_on_a_malformed_file_alone() {
        let h = Harness::no_tmux();
        let _w = ws(&h);
        let bad = file_at(&h, "new", "01J0000000000000000000000A.json");
        std::fs::write(&bad, "not json").unwrap();
        let t = Instant::now();
        let to = Duration::from_secs(1500);
        // The second call is at the timeout, so a watch that skips the file ends with 1 and never loops on.
        let c = Clock { times: vec![t, t + to], calls: Cell::new(0) };
        let mut o = Vec::new();
        assert_eq!(zygo::cmd::mail::watch(&dir(&h), KEY, to, Duration::ZERO, &|| c.now(), &mut o), 0);
        assert!(o.is_empty(), "{}", String::from_utf8_lossy(&o));
        assert!(exists(&bad));
    }

    #[test] // R1: only the parsed message is printed
    fn watch_prints_only_the_valid_line_next_to_a_malformed_file() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        let id = put(&h, &w, "elena", "s1", "b1", 1);
        std::fs::write(file_at(&h, "new", "01J0000000000000000000000A.json"), "not json").unwrap();
        let c = Clock { times: vec![Instant::now()], calls: Cell::new(0) };
        let mut o = Vec::new();
        assert_eq!(zygo::cmd::mail::watch(&dir(&h), KEY, Duration::from_secs(1500), Duration::ZERO, &|| c.now(), &mut o), 0);
        assert_eq!(String::from_utf8(o).unwrap(), format!("{id} cli s1\n"));
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 2);
    }

    /// A writer whose every write fails with `BrokenPipe`.
    struct Broken;

    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test] // R2: a failed write is not a success
    fn watch_exits_1_when_the_write_fails() {
        let h = Harness::no_tmux();
        let w = ws(&h);
        put(&h, &w, "elena", "s1", "b1", 1);
        let c = Clock { times: vec![Instant::now()], calls: Cell::new(0) };
        assert_eq!(zygo::cmd::mail::watch(&dir(&h), KEY, Duration::from_secs(1500), Duration::ZERO, &|| c.now(), &mut Broken), 1);
        assert_eq!(store::unread(&dir(&h), KEY).unwrap(), 1);
    }

    #[test] // R6 (A2): two hand-started members share one name
    fn watch_cli_refuses_a_name_of_two_members() {
        let h = Harness::no_tmux();
        // Hand starts can give one name to two panes; the record holds nothing more than the name.
        h.seed(WS_ID, &mut |w| {
            for k in [KEY, KEY2] {
                w.members.insert(k.to_string(), MemberRecord { name: Some("twin".into()), ..MemberRecord::new(Agent::ClaudeCode) });
            }
        });
        let o = run(&h, &["mail", "watch", "twin", "--timeout", "0s"], &[]);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(err(&o), "zygo mail watch: twin is not one member\n");
        assert_eq!(out(&o), "");
    }

    #[test] // R6 (A2): `all` with no members resolves to no target
    fn watch_cli_refuses_all_in_a_workspace_with_no_members() {
        let h = Harness::no_tmux();
        h.seed(WS_ID, &mut |_| {});
        let o = run(&h, &["mail", "watch", "all", "--timeout", "0s"], &[]);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(err(&o), "zygo mail watch: all is not one member\n");
        assert_eq!(out(&o), "");
    }
}

#[cfg(windows)]
#[test]
fn op5_every_mail_verb() {
    let home = tempfile::tempdir().unwrap();
    for args in [&["mail"][..], &["mail", "ls"][..], &["mail", "read"][..], &["mail", "send", "x", "-"][..], &["mail", "watch"][..]] {
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut c, home.path());
        let o = c.args(args).stdin(std::process::Stdio::null()).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&o.stderr), "workspaces need tmux; run zygo in WSL\n", "{args:?}");
    }
}
