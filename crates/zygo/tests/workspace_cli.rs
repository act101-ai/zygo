//! M1: the workspace verbs of the CLI (W§12), their slash handlers, OP5 and the v1 launch outside a workspace. Every
//! run uses a scratch home; `TMUX_TMPDIR` is a scratch folder, so no test reaches a shared tmux server. tmux runs only
//! through `tmux -V`.
mod common;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[cfg(unix)]
const NO_TMUX: &str = "zygo: workspaces need tmux; install tmux 3.2 or later\n";

/// The `Paths` literal of `usage_doctor.rs:26` under `root`.
#[cfg(unix)]
fn paths(root: &Path) -> zygo_core::paths::Paths {
    zygo_core::paths::Paths {
        config_file: root.join("config.toml"),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"),
        marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"),
        approvals_file: root.join("data/approvals.toml"),
        usage_db: root.join("data/usage.db"),
    }
}

/// `zygo <args>` in the scratch home: `config.toml` = `config`, `PATH` = `path`, every `ZYGO_*`, `CLAUDE*`, `TMUX`
/// and `TMUX_PANE` variable of the test process removed, `TMUX_TMPDIR` = `<home>/tmux`, cwd = home, stdin null,
/// then `extra` set.
fn zygo_in(home: &Path, config: &str, path: &OsStr, args: &[&str], extra: &[(&str, &OsStr)]) -> Output {
    zygo_cmd(home, config, path, args, extra).output().unwrap()
}

/// The command of `zygo_in`, not yet run.
fn zygo_cmd(home: &Path, config: &str, path: &OsStr, args: &[&str], extra: &[(&str, &OsStr)]) -> Command {
    std::fs::write(home.join("config.toml"), config).unwrap();
    let tmux_tmp = home.join("tmux");
    std::fs::create_dir_all(&tmux_tmp).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, home);
    for (k, _) in std::env::vars_os() {
        let Some(k) = k.to_str() else { continue };
        if k.starts_with("ZYGO_") || k.starts_with("CLAUDE") || k == "TMUX" || k == "TMUX_PANE" {
            cmd.env_remove(k);
        }
    }
    cmd.env("ZYGO_CONFIG", home.join("config.toml"))
        .env("PATH", path)
        .env("TMUX_TMPDIR", &tmux_tmp)
        .args(args)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `<home>/empty-path`, created: a `PATH` with no program on it.
fn empty_path(home: &Path) -> PathBuf {
    let p = home.join("empty-path");
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[cfg(unix)]
fn set_mtime(p: &Path, t: std::time::SystemTime) {
    std::fs::OpenOptions::new().read(true).open(p).unwrap().set_modified(t).unwrap();
}

/// W§12: every row parses and reaches the library; without tmux on `PATH` the verbs that need it say so.
#[cfg(unix)]
#[test]
fn w12_rows_parse_and_reach_the_library() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let rows: &[(&[&str], i32, &str, &str)] = &[
        (&["up"], 2, "", NO_TMUX),
        (&["up", "app", "--loadout", "pair", "--reset", "--no-attach"], 2, "", NO_TMUX),
        (&["up", "--all"], 2, "", NO_TMUX),
        (&["up", "--all", "app"], 2, "", "zygo: up --all takes no workspace name, --loadout or --reset\n"),
        (&["down"], 2, "", NO_TMUX),
        (&["down", "app", "--now", "--timeout", "5m", "--wait"], 2, "", NO_TMUX),
        (&["down", "--all"], 2, "", NO_TMUX),
        (&["attach"], 2, "", NO_TMUX),
        (&["attach", "app"], 2, "", NO_TMUX),
        (&["ls"], 0, "no workspaces\n", ""),
        (&["workspace"], 1, "", "no workspace for this session or folder\n"),
        (&["loadout", "list"], 0, "solo  built-in\n", ""),
        (&["ws", "forget", "app"], 1, "", "zygo: no workspace named app\n"),
        (&["ws", "forget-pane", "kaaaaaaaa", "--name", "app"], 1, "", "zygo: no workspace named app\n"),
        (&["ws", "snapshot", "--ws-id", "0123456789abcdef", "--event", "window-layout-changed", "--force"], 0, "", ""),
    ];
    for (args, code, out, err) in rows {
        let o = zygo_in(h.path(), "", path.as_os_str(), args, &[]);
        assert_eq!(
            (o.status.code(), text(&o.stdout), text(&o.stderr)),
            (Some(*code), out.to_string(), err.to_string()),
            "zygo {}",
            args.join(" ")
        );
    }
}

/// D4: a bad `--timeout`, a name with `--all`, and a name that starts with `-` without `--`.
#[cfg(unix)]
#[test]
fn w12_down_refuses_a_bad_timeout_a_name_with_all_and_a_dash_name() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let o = zygo_in(h.path(), "", path.as_os_str(), &["down", "--timeout", "5x"], &[]);
    assert_eq!(
        (o.status.code(), text(&o.stderr)),
        (Some(2), "zygo: bad --timeout value 5x: use <n>s, <n>m, <n>h or <n>d\n".to_string())
    );
    for args in [&["down", "--all", "app"][..], &["down", "-x"]] {
        let o = zygo_in(h.path(), "", path.as_os_str(), args, &[]);
        assert_eq!(o.status.code(), Some(2), "zygo {}", args.join(" "));
        assert!(text(&o.stderr).starts_with("error:"), "zygo {}: {}", args.join(" "), text(&o.stderr));
    }
}

/// The name reaches the library, also after `--`.
#[cfg(unix)]
#[test]
#[ignore = "timing: real tmux probe has a 2 s bound"]
fn w12_names_reach_the_library_also_after_double_dash() {
    let Some(tmux) = zygo_workspace::mux::testing::require_tmux() else { return };
    let h = tempfile::tempdir().unwrap();
    let path = tmux.parent().unwrap().as_os_str().to_owned();
    let rows: [(&[&str], &str); 4] = [
        (&["down", "--wait", "--", "-x"], "-x"),
        (&["down", "app", "--now"], "app"),
        (&["up", "app", "--no-attach"], "app"),
        (&["attach", "app"], "app"),
    ];
    for (args, name) in rows {
        let o = zygo_in(h.path(), "", &path, args, &[]);
        assert_eq!(
            (o.status.code(), text(&o.stderr)),
            (Some(1), format!("zygo: no workspace named {name}\n")),
            "zygo {}",
            args.join(" ")
        );
    }
}

/// FIX-16: the parsers of `zygo down`, `zygo up` and `zygo ws forget` read a name after `--`, and that name reaches
/// the library, also for the names `--` and `-x`.
#[cfg(unix)]
#[test]
#[ignore = "timing: real tmux probe has a 2 s bound"]
fn fix16_double_dash_names_reach_the_library() {
    let Some(tmux) = zygo_workspace::mux::testing::require_tmux() else { return };
    let h = tempfile::tempdir().unwrap();
    let path = tmux.parent().unwrap().as_os_str().to_owned();
    for name in ["--", "-x"] {
        for args in [&["down", "--now", "--", name][..], &["up", "--no-attach", "--", name], &["ws", "forget", "--", name]] {
            let o = zygo_in(h.path(), "", &path, args, &[]);
            assert_eq!(
                (o.status.code(), text(&o.stderr)),
                (Some(1), format!("zygo: no workspace named {name}\n")),
                "zygo {}",
                args.join(" ")
            );
        }
    }
}

/// W§18 #59 (AU71): `zygo ws snapshot` does not sweep; the next other command does.
#[cfg(unix)]
#[test]
fn w18_59_ws_snapshot_does_not_sweep() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let old = h.path().join(".cache/zygo/sessions/old");
    std::fs::create_dir_all(old.join("results")).unwrap();
    std::fs::write(old.join("results/f0"), "x").unwrap();
    std::fs::write(old.join("token"), "t").unwrap();
    std::fs::write(old.join("pid"), "1").unwrap();
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 3600);
    for p in [old.join("results/f0"), old.join("token"), old.join("pid"), old.join("results"), old.clone()] {
        set_mtime(&p, t);
    }
    for _ in 0..10 {
        let o = zygo_in(h.path(), "", path.as_os_str(), &["ws", "snapshot", "--ws-id", "0123456789abcdef"], &[]);
        assert_eq!((o.status.code(), text(&o.stdout), text(&o.stderr)), (Some(0), String::new(), String::new()));
    }
    assert!(old.join("results/f0").exists(), "ws snapshot does not sweep");
    let o = zygo_in(h.path(), "", path.as_os_str(), &["status"], &[]);
    assert!(!old.exists(), "status sweeps: {}", text(&o.stderr));
}

/// Sets the mtime of the symlink `p` itself (`set_mtime` follows a link).
#[cfg(unix)]
fn set_link_mtime(p: &Path, t: std::time::SystemTime) {
    use std::os::unix::ffi::OsStrExt;
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // SAFETY: a zeroed `timespec` is valid; its two fields are set below
    let mut ts: [libc::timespec; 2] = unsafe { std::mem::zeroed() };
    for x in &mut ts {
        x.tv_sec = secs as libc::time_t;
    }
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a NUL-terminated path and `ts` holds two `timespec` values, as `utimensat` requires
    assert_eq!(unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), ts.as_ptr(), libc::AT_SYMLINK_NOFOLLOW) }, 0);
}

/// M1 review round 1 (M2): the sweep of a `zygo` command gets `Paths::workspace_cache_dir`, so an ended session linked
/// into the real cache layout is swept 24 h after its last change, and the workspace result stays (AU69 (a), AU16).
#[cfg(unix)]
#[test]
fn au69_a_a_command_sweeps_an_ended_session_linked_into_the_workspace_cache() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let cache = h.path().join(".cache/zygo");
    let target = cache.join("workspaces/0123456789abcdef/results/kaaaaaaaa");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("f0"), "x").unwrap();
    let old = cache.join("sessions/old");
    std::fs::create_dir_all(&old).unwrap();
    for f in ["token", "pid", "session.log"] {
        std::fs::write(old.join(f), "1").unwrap();
    }
    std::os::unix::fs::symlink(&target, old.join("results")).unwrap();
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 3600);
    for p in [old.join("token"), old.join("pid"), old.join("session.log"), old.clone()] {
        set_mtime(&p, t);
    }
    set_link_mtime(&old.join("results"), t);
    let o = zygo_in(h.path(), "", path.as_os_str(), &["status"], &[]);
    assert!(old.symlink_metadata().is_err(), "status sweeps the linked session: {}", text(&o.stderr));
    assert_eq!(std::fs::read_to_string(target.join("f0")).unwrap(), "x", "the workspace result stays");
}

/// AU84: an invalid id is one escaped line on stderr, exit 0.
#[cfg(unix)]
#[test]
fn ws_snapshot_with_an_invalid_id_prints_one_line_and_exits_0() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let o = zygo_in(h.path(), "", path.as_os_str(), &["ws", "snapshot", "--ws-id", "../x\u{1b}"], &[]);
    assert_eq!(
        (o.status.code(), text(&o.stdout), text(&o.stderr)),
        (Some(0), String::new(), "zygo: invalid workspace id ../x\\u{1b}\n".to_string())
    );
}

/// `zygo ws forget-pane <key> [--name <n>]` run in a folder that the child removes after its `chdir`, so
/// `current_dir()` fails in `zygo`.
#[cfg(unix)]
fn forget_pane_in_a_removed_folder(h: &Path, args: &[&str]) -> Output {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::CommandExt;
    let path = empty_path(h);
    let gone = h.join("gone");
    std::fs::create_dir(&gone).unwrap();
    let c = std::ffi::CString::new(gone.as_os_str().as_bytes()).unwrap();
    let mut cmd = zygo_cmd(h, "", path.as_os_str(), args, &[]);
    cmd.current_dir(&gone);
    // SAFETY: the closure runs in the child after its `chdir` and before `exec`; it only calls `rmdir`, which is
    // async-signal-safe, on a NUL-terminated path that was made before the fork.
    unsafe {
        cmd.pre_exec(move || if libc::rmdir(c.as_ptr()) == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) });
    }
    cmd.output().unwrap()
}

/// D10 (M1 review round 1, fix 6): `zygo ws forget-pane <key> --name <n>` does not need the current folder, so a
/// removed one does not fail it; without `--name` the folder names the workspace, and its error stays. Kills: a
/// `forget-pane` that reads `current_dir()` on every path.
#[cfg(unix)]
#[test]
fn ws_forget_pane_with_a_name_does_not_need_the_current_folder() {
    let h = tempfile::tempdir().unwrap();
    let o = forget_pane_in_a_removed_folder(h.path(), &["ws", "forget-pane", "kaaaaaaaa", "--name", "app"]);
    assert_eq!((o.status.code(), text(&o.stdout), text(&o.stderr)), (Some(1), String::new(), "zygo: no workspace named app\n".to_string()));
    let o = forget_pane_in_a_removed_folder(h.path(), &["ws", "forget-pane", "kaaaaaaaa"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).starts_with("zygo: cannot read the current folder: "), "{}", text(&o.stderr));
}

#[cfg(unix)]
const PAIR_CFG: &str = r#"[loadout.pair]
[[loadout.pair.window]]
name = "agents"
[[loadout.pair.window.pane]]
kind = "claude-code"
[loadout.solo]
[[loadout.solo.window]]
[[loadout.solo.window.pane]]
kind = "shell"
"#;

/// D7: `loadout list` names each loadout and its source; a user `solo` replaces the built-in one.
#[cfg(unix)]
#[test]
fn loadout_list_names_each_source() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let run = |config: &str| zygo_in(h.path(), config, path.as_os_str(), &["loadout", "list"], &[]);
    let o = run(PAIR_CFG);
    assert_eq!((o.status.code(), text(&o.stdout)), (Some(0), "pair  user\nsolo  user\n".to_string()), "{}", text(&o.stderr));
    let o = run("");
    assert_eq!((o.status.code(), text(&o.stdout)), (Some(0), "solo  built-in\n".to_string()), "{}", text(&o.stderr));
}

/// W§12, OP11 (D5, D6): the slash handlers take the membership marker, never the cwd, and refuse other arguments.
#[cfg(unix)]
#[test]
fn slash_verbs_take_the_marker_never_the_cwd() {
    use zygo::cmd::verbs::VerbCtx;
    use zygo::cmd::UsageError;
    use zygo_config::display::error_text;

    let usage = |r: anyhow::Result<String>| -> String {
        let e = r.unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some(), "not a UsageError: {e}");
        e.to_string()
    };
    let h = tempfile::tempdir().unwrap();
    let p = paths(h.path());
    std::fs::write(&p.config_file, "").unwrap();
    let ctx = VerbCtx { slash: true, save: false, session_dir: None, paths: p.clone() };
    let a = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
    for args in [a(&[]), a(&["--now"])] {
        let e = zygo::cmd::down::verb(&ctx, &args).unwrap_err();
        assert_eq!(error_text(&e), "this session is not in a workspace", "{args:?}");
    }
    assert_eq!(usage(zygo::cmd::down::verb(&ctx, &a(&["--bogus"]))), "usage: /zygo:down [--now]");
    assert_eq!(zygo::cmd::ls::verb(&ctx, &a(&[])).unwrap(), "no workspaces");
    assert_eq!(usage(zygo::cmd::ls::verb(&ctx, &a(&["x"]))), "ls takes no arguments");
    assert_eq!(usage(zygo::cmd::workspace::verb(&ctx, &a(&["x"]))), "workspace takes no arguments");
    let saved = VerbCtx { save: true, ..ctx };
    assert_eq!(usage(zygo::cmd::down::verb(&saved, &a(&[]))), "usage: /zygo:down [--now]");
    assert_eq!(usage(zygo::cmd::ls::verb(&saved, &a(&[]))), "ls takes no arguments");
    assert_eq!(usage(zygo::cmd::workspace::verb(&saved, &a(&[]))), "workspace takes no arguments");
}

/// W§18 #64 (W-C6, OP9): `zygo claude` outside a workspace is a v1 launch, on every platform.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn w18_64_zygo_claude_outside_a_workspace_is_a_v1_launch() {
    use std::collections::BTreeMap;
    use std::env::consts::EXE_SUFFIX;
    let h = tempfile::tempdir().unwrap();
    let bin = h.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(common::stub_agent(), bin.join(format!("claude{EXE_SUFFIX}"))).unwrap();
    let out = h.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let version = zygo_config::builtin::min_agent_version(zygo_core::Agent::ClaudeCode).to_string();
    let o = zygo_in(
        h.path(),
        "",
        bin.as_os_str(),
        &["claude"],
        &[("ZYGO_STUB_OUT", out.as_os_str()), ("ZYGO_STUB_VERSION", OsStr::new(&version))],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let argv: Vec<String> = serde_json::from_slice(&std::fs::read(out.join("argv.json")).unwrap()).unwrap();
    assert!(argv.iter().any(|a| a == "--settings"), "{argv:?}");
    let env: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(out.join("env.json")).unwrap()).unwrap();
    assert!(!env.contains_key("ZYGO_WORKSPACE") && !env.contains_key("ZYGO_PANE"), "{env:?}");
}

/// W§18 #64 (OP5): on native Windows every workspace verb exits 2 before any library call.
#[cfg(windows)]
#[test]
fn w18_64_workspace_verbs_exit_2_on_windows() {
    let h = tempfile::tempdir().unwrap();
    let path = empty_path(h.path());
    let rows: [&[&str]; 8] = [
        &["up"],
        &["down"],
        &["ls"],
        &["attach"],
        &["workspace"],
        &["loadout", "list"],
        &["ws", "forget", "app"],
        &["ws", "snapshot", "--ws-id", "0123456789abcdef"],
    ];
    for args in rows {
        let o = zygo_in(h.path(), "", path.as_os_str(), args, &[]);
        assert_eq!(
            (o.status.code(), text(&o.stdout), text(&o.stderr)),
            (Some(2), String::new(), "workspaces need tmux; run zygo in WSL\n".to_string()),
            "zygo {}",
            args.join(" ")
        );
    }
}
