use zygo_core::ids::{ulid, valid_member_name, RESERVED_MEMBER_NAMES};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;

#[cfg(unix)]
#[test]
fn paths_unix() {
    unsafe { std::env::set_var("HOME", "/h") };
    unsafe { std::env::remove_var("ZYGO_CONFIG") };
    let p = Paths::from_env();
    assert_eq!(p.config_file, std::path::PathBuf::from("/h/.config/zygo/config.toml"));
    assert_eq!(p.usage_db, std::path::PathBuf::from("/h/.local/share/zygo/usage.db"));
    assert_eq!(p.sessions_dir, std::path::PathBuf::from("/h/.cache/zygo/sessions"));
    assert_eq!(p.state_dir(), std::path::PathBuf::from("/h/.local/state/zygo"));
    assert_eq!(p.workspaces_dir(), std::path::PathBuf::from("/h/.local/state/zygo/workspaces"));
    assert_eq!(p.workspace_cache_dir(), std::path::PathBuf::from("/h/.cache/zygo/workspaces"));
    unsafe { std::env::set_var("ZYGO_CONFIG", "/x.toml") };
    assert_eq!(Paths::from_env().config_file, std::path::PathBuf::from("/x.toml"));
    unsafe { std::env::remove_var("ZYGO_CONFIG") };
    // HOME empty or unset: the OS home, never the current directory.
    for set in [Some(""), None] {
        match set {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        let home = std::env::home_dir().expect("home_dir");
        let p = Paths::from_env();
        assert!(p.config_file.is_absolute(), "{:?}", p.config_file);
        assert_eq!(p.config_file, home.join(".config/zygo/config.toml"));
        assert_eq!(p.cache_dir, home.join(".cache/zygo"));
        assert_eq!(p.data_dir, home.join(".local/share/zygo"));
        assert_eq!(p.state_dir(), home.join(".local/state/zygo"));
    }
    unsafe { std::env::set_var("HOME", "/h") };
}

#[test]
fn ulid_shape() {
    let a = ulid();
    assert_eq!(a.len(), 26);
    assert_ne!(a, ulid());
}

#[test]
fn token_file() {
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().join("s"));
    d.create().unwrap();
    d.write_token("tok123").unwrap();
    assert_eq!(d.read_token().unwrap(), "tok123");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(m(&d.token()), 0o600);
        assert_eq!(m(&d.root), 0o700);
    }
}

#[cfg(unix)]
#[test]
fn session_dir_modes() {
    use std::os::unix::fs::PermissionsExt;
    let m = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let t = tempfile::tempdir().unwrap();
    // Every directory `create` makes is 0700, not only the last one.
    let d = SessionDir::new(t.path().join("a").join("b"));
    d.create().unwrap();
    assert_eq!(m(&t.path().join("a")), 0o700);
    assert_eq!(m(&d.root), 0o700);
    // An existing token file with a wide mode becomes 0600 and holds only the new token.
    std::fs::write(d.token(), "old-longer-token").unwrap();
    std::fs::set_permissions(d.token(), std::fs::Permissions::from_mode(0o644)).unwrap();
    d.write_token("new").unwrap();
    assert_eq!(m(&d.token()), 0o600);
    assert_eq!(std::fs::read_to_string(d.token()).unwrap(), "new");
}

#[test]
fn saves_are_atomic_and_private() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    // A leftover at the old fixed temp name must not break a save.
    std::fs::create_dir(d.root.join("overrides.json.tmp")).unwrap();
    let o = Overrides { work_units: Some(false), ..Overrides::default() };
    o.save(&d).unwrap();
    assert_eq!(Overrides::load(&d), o);
    let v = SessionView {
        session: "s".into(),
        agent: zygo_core::Agent::ClaudeCode,
        main_role: "main".into(),
        effective_role: None,
        role_prompt: None,
        user_prompt: None,
        profile: None,
        profile_prompt: None,
        work_item: None,
        legend: vec![],
        disabled_roles: vec![],
        work_units: true,
        results: false,
        compact_tool: true,
        navigation: false,
        trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20,
        ..Default::default()
    };
    v.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), v);
    // Only the targets (and the leftover) remain: no temp files.
    let mut names: Vec<String> = std::fs::read_dir(&d.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["overrides.json", "overrides.json.tmp", "view.json"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(m(&d.overrides_json()), 0o600);
        assert_eq!(m(&d.view_json()), 0o600);
    }
}

#[test]
fn overrides_roundtrip() {
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let mut o = Overrides { results: Some(true), results_threshold: Some(99), ..Overrides::default() };
    o.guards.insert("g".into(), "warn".into());
    o.save(&d).unwrap();
    assert_eq!(Overrides::load(&d), o);
    std::fs::write(d.overrides_json(), "not json").unwrap();
    assert_eq!(Overrides::load(&d), Overrides::default());
}

#[test]
fn slash_dispatch_seam() {
    use zygo_core::hook::{set_slash_dispatch, slash_dispatch};
    assert!(slash_dispatch().is_none());
    set_slash_dispatch(|_, _| Ok(String::new()));
    assert!(slash_dispatch().is_some());
}

#[test]
fn session_view_compact_tool_defaults_true_and_round_trips() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    // a view.json written before the field existed
    std::fs::write(d.view_json(), br#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":true,"results":true,"navigation":false}"#).unwrap();
    assert!(SessionView::load(&d).unwrap().compact_tool);
    let mut v = SessionView::load(&d).unwrap();
    v.compact_tool = false;
    v.save(&d).unwrap();
    assert!(!SessionView::load(&d).unwrap().compact_tool);
}

#[test]
fn plain_strips_the_verbatim_prefix_only_when_the_plain_path_is_the_same() {
    use std::path::{Path, PathBuf};
    use zygo_core::paths::plain;
    let p = |s: &str| plain(Path::new(s));
    assert_eq!(p(r"\\?\C:\Users\a\repo"), PathBuf::from(r"C:\Users\a\repo"));
    assert_eq!(p(r"\\?\UNC\host\share\x"), PathBuf::from(r"\\host\share\x"));
    // A plain path would read these differently: the prefix stays.
    assert_eq!(p(r"\\?\C:\a\b."), PathBuf::from(r"\\?\C:\a\b."));
    assert_eq!(p(r"\\?\C:\a\..\b"), PathBuf::from(r"\\?\C:\a\..\b"));
    assert_eq!(p(r"\\?\C:\a/b"), PathBuf::from(r"\\?\C:\a/b"));
    assert_eq!(p(r"\\?\Volume{1}\x"), PathBuf::from(r"\\?\Volume{1}\x"));
    assert_eq!(p("/home/a"), PathBuf::from("/home/a"));
    assert_eq!(p(r"C:\a"), PathBuf::from(r"C:\a"));
}

// ---- HUD-7: statusline style in the view and in the overrides (statusline HUD design H§8, AU1) ----

#[test]
fn session_view_without_statusline_style_reads_as_compact() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    // a view.json written before the field existed
    std::fs::write(d.view_json(), br#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":true,"results":true,"compact_tool":true,"navigation":false,"trim_threshold_bytes":16384,"head_lines":60,"tail_lines":20}"#).unwrap();
    let mut v = SessionView::load(&d).unwrap();
    assert_eq!(v.statusline_style, "compact");
    v.statusline_style = "verbose".into();
    v.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap().statusline_style, "verbose");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(json["statusline_style"], "verbose");
}

#[test]
fn session_view_default_equals_the_serde_defaults() {
    use zygo_core::session_view::SessionView;
    let v = SessionView { statusline_style: "verbose".into(), ..Default::default() };
    assert_eq!(v.statusline_style, "verbose");
    let d = SessionView::default();
    assert_eq!(d.statusline_style, "compact");
    assert_eq!((d.session.as_str(), d.agent, d.main_role.as_str()), ("", zygo_core::Agent::ClaudeCode, ""));
    assert!(d.compact_tool && !d.work_units && !d.results && !d.navigation);
    assert_eq!((d.trim_threshold_bytes, d.head_lines, d.tail_lines), (16384, 60, 20));
    assert!(d.legend.is_empty() && d.disabled_roles.is_empty() && d.work_item.is_none());
    // A file that holds only the fields without a serde default reads as the default of every other field.
    let min: SessionView = serde_json::from_str(
        r#"{"session":"","agent":"claude-code","main_role":"","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":false,"results":false,"navigation":false}"#,
    )
    .unwrap();
    assert_eq!(min, d);
}

#[test]
fn overrides_statusline_round_trips_and_is_absent_when_unset() {
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let o = Overrides { statusline: Some("verbose".into()), ..Overrides::default() };
    o.save(&d).unwrap();
    assert_eq!(Overrides::load(&d), o);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(d.overrides_json()).unwrap()).unwrap();
    assert_eq!(json["statusline"], "verbose");
    // An overrides.json written before the field existed reads as no choice.
    std::fs::write(d.overrides_json(), br#"{"results":true,"guards":{}}"#).unwrap();
    assert_eq!(Overrides::load(&d).statusline, None);
    Overrides::default().save(&d).unwrap();
    assert!(!std::fs::read_to_string(d.overrides_json()).unwrap().contains("statusline"));
}

// ---- C1: workspace paths, session-dir accessors, view workspace field (workspaces design W§7.1, W§7.3, AU58, AU72) ----

fn paths_under(root: &std::path::Path) -> Paths {
    let (cache_dir, data_dir) = (root.join("cache"), root.join("data"));
    Paths {
        config_file: root.join("config.toml"),
        sessions_dir: cache_dir.join("sessions"),
        marketplaces_dir: cache_dir.join("marketplaces"),
        plugin_store: data_dir.join("plugins"),
        approvals_file: data_dir.join("approvals.toml"),
        usage_db: data_dir.join("usage.db"),
        cache_dir,
        data_dir,
    }
}

#[test]
fn state_dir_in_follows_the_data_dir_rule() {
    use std::path::Path;
    let home = Path::new("/h");
    // Branch 1: data_dir is $HOME/.local/share/zygo, as Paths::from_env sets it on unix.
    let mut p = paths_under(Path::new("/t"));
    p.data_dir = home.join(".local/share/zygo");
    assert_eq!(p.state_dir_in(Some(home)), home.join(".local/state/zygo"));
    // Branch 2: another home, no home, or another data_dir.
    assert_eq!(p.state_dir_in(Some(Path::new("/other"))), home.join(".local/share/zygo").join("state"));
    assert_eq!(p.state_dir_in(None), home.join(".local/share/zygo").join("state"));
    let q = paths_under(Path::new("/t"));
    assert_eq!(q.state_dir_in(Some(home)), Path::new("/t").join("data").join("state"));
}

#[test]
fn workspace_dirs_of_a_test_built_paths_stay_under_its_directory() {
    let t = tempfile::tempdir().unwrap();
    let p = paths_under(t.path());
    assert_eq!(p.state_dir(), t.path().join("data").join("state"));
    assert_eq!(p.workspaces_dir(), t.path().join("data").join("state").join("workspaces"));
    assert_eq!(p.workspace_cache_dir(), t.path().join("cache").join("workspaces"));
}

#[test]
fn write_atomic_is_public_sets_0600_and_leaves_no_temp_file() {
    use zygo_core::session_dir::write_atomic;
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("workspace.json");
    write_atomic(&f, b"one").unwrap();
    write_atomic(&f, b"two").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"two");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // A failed rename (the target is a directory) is an error; the temporary file is deleted.
    let taken = t.path().join("taken");
    std::fs::create_dir(&taken).unwrap();
    assert!(write_atomic(&taken, b"x").is_err());
    let mut names: Vec<String> = std::fs::read_dir(t.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["taken", "workspace.json"]);
}

#[test]
fn session_dir_activity_accessors() {
    let d = SessionDir::new(std::path::PathBuf::from("/c/sessions/01ABC"));
    assert_eq!(d.activity_json(), d.root.join("activity.json"));
    assert_eq!(d.activity_lock(), d.root.join("activity.lock"));
}

#[test]
fn session_view_workspace_round_trips_and_is_none_for_an_old_file() {
    use zygo_core::session_view::{Member, ResumeNote, SessionView, SubagentRef, ViewWorkspace};
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    assert_eq!(SessionView::default().workspace, None);
    // a view.json written before the field existed
    std::fs::write(d.view_json(), br#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":true,"results":true,"compact_tool":true,"navigation":false,"trim_threshold_bytes":16384,"head_lines":60,"tail_lines":20,"statusline_style":"verbose"}"#).unwrap();
    let old = SessionView::load(&d).unwrap();
    assert_eq!(old.workspace, None);
    // Without `workspace`: a save and a load give the same view.
    old.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), old);
    // A member with a resume note (AU58, AU72).
    let member = Member { ws_id: "w1".into(), pane: "p1".into(), ..Default::default() };
    let note = ResumeNote {
        clean_stop: true,
        interrupted: vec![SubagentRef { agent_id: "a1".into(), agent_type: "zygo-code".into(), ..Default::default() }],
        closed_units: vec!["C1".into()],
        agent_session: None,
        ..Default::default()
    };
    let note_clone = note.clone();
    let v = SessionView {
        workspace: Some(ViewWorkspace { member: member.clone(), resume_note: Some(note), ..Default::default() }),
        ..old.clone()
    };
    v.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), v);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(
        json["workspace"],
        serde_json::json!({
            "member": { "ws_id": "w1", "pane": "p1" },
            "resume_note": {
                "clean_stop": true,
                "interrupted": [{ "agent_id": "a1", "agent_type": "zygo-code" }],
                "closed_units": ["C1"]
            }
        })
    );
    // Inputs bound to an agent session (W§9.4): the id is written and read back; an older file reads as none.
    let bound = ResumeNote { agent_session: Some("ses_R".into()), ..note_clone };
    let vb = SessionView {
        workspace: Some(ViewWorkspace { member: member.clone(), resume_note: Some(bound), ..Default::default() }),
        ..old.clone()
    };
    vb.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), vb);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(json["workspace"]["resume_note"]["agent_session"], "ses_R");
    assert_eq!(
        serde_json::from_str::<ResumeNote>(r#"{"clean_stop":true,"interrupted":[],"closed_units":[]}"#).unwrap().agent_session,
        None
    );
    // A member with no note: `resume_note` is not in the file.
    let m: ViewWorkspace = serde_json::from_str(r#"{"member":{"ws_id":"w1","pane":"p1"}}"#).unwrap();
    assert_eq!(m, ViewWorkspace { member, resume_note: None, ..Default::default() });
}

#[test]
fn session_view_save_keeps_unknown_top_level_fields() { // H1 attempt-2 review round 1, F4 (AU78 pattern)
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    SessionView { session: "s".into(), main_role: "lead".into(), ..Default::default() }.save(&d).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    json["future_field"] = serde_json::json!({"a": [1, "b"]});
    json["future_flag"] = serde_json::json!(true);
    std::fs::write(d.view_json(), serde_json::to_vec(&json).unwrap()).unwrap();
    let mut v = SessionView::load(&d).unwrap();
    v.main_role = "code".into();
    v.save(&d).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(after["future_field"], serde_json::json!({"a": [1, "b"]}));
    assert_eq!(after["future_flag"], serde_json::json!(true));
    assert_eq!(after["main_role"], "code");
    // A view of this zygo writes no extra field.
    let plain = SessionView::default();
    let keys: Vec<String> = serde_json::to_value(&plain).unwrap().as_object().unwrap().keys().cloned().collect();
    assert!(!keys.iter().any(|k| k.starts_with("future_") || k == "extra"), "{keys:?}");
}

#[test]
fn session_view_save_keeps_unknown_nested_fields() { // H1 attempt-2 review round 2, R5 (AU78 pattern)
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let text = r#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":false,"results":false,"navigation":false,"workspace":{"member":{"ws_id":"w","pane":"p","future_m":1},"resume_note":{"clean_stop":true,"interrupted":[{"agent_id":"a1","agent_type":"x","future_s":[2]}],"closed_units":[],"future_n":{"k":"v"}},"future_w":true},"future_top":3}"#;
    std::fs::write(d.view_json(), text).unwrap();
    let mut v = SessionView::load(&d).unwrap();
    v.main_role = "code".into();
    v.save(&d).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(after["main_role"], "code");
    assert_eq!(after["future_top"], 3);
    assert_eq!(after["workspace"]["future_w"], true);
    assert_eq!(after["workspace"]["member"], serde_json::json!({"ws_id": "w", "pane": "p", "future_m": 1}));
    assert_eq!(after["workspace"]["resume_note"]["future_n"], serde_json::json!({"k": "v"}));
    assert_eq!(after["workspace"]["resume_note"]["interrupted"][0], serde_json::json!({"agent_id": "a1", "agent_type": "x", "future_s": [2]}));
    assert_eq!(SessionView::load(&d).unwrap(), v);
}

#[test]
#[ignore = "timing: the FIFO write must end inside a 5 s recv_timeout"]
fn session_log_appends_lines_and_refuses_links_and_fifos() { // H1 D21: the one session-log writer
    let t = tempfile::tempdir().unwrap();
    let sd = SessionDir::new(t.path().join("s"));
    sd.create().unwrap();
    sd.append_log("a");
    sd.append_log("b\nc\rd");
    assert_eq!(std::fs::read_to_string(sd.log()).unwrap(), "a\nb c d\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        assert_eq!(std::fs::metadata(sd.log()).unwrap().permissions().mode() & 0o777, 0o600);
        // A link: the target is not written and the link stays a link.
        let outside = t.path().join("outside");
        std::fs::write(&outside, "x").unwrap();
        std::fs::remove_file(sd.log()).unwrap();
        std::os::unix::fs::symlink(&outside, sd.log()).unwrap();
        sd.append_log("e");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "x");
        assert!(std::fs::symlink_metadata(sd.log()).unwrap().file_type().is_symlink());
        // A FIFO: the writer does not wait for a reader.
        std::fs::remove_file(sd.log()).unwrap();
        let c = std::ffi::CString::new(sd.log().as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let sd2 = sd.clone();
        std::thread::spawn(move || {
            sd2.append_log("f");
            let _ = tx.send(());
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).expect("append_log waited on a FIFO");
        assert!(std::fs::symlink_metadata(sd.log()).unwrap().file_type().is_fifo());
    }
}

#[test] // FIX-22: W-C1 (C6); only a FIFO is refused; no size cap; a link is still followed
#[ignore = "timing: the FIFO load must end inside a 5 s recv_timeout"]
fn session_view_load_does_not_wait_on_a_fifo() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().join("s"));
    d.create().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::FileTypeExt;
        // A FIFO: the error `<path>: is a FIFO` within 5 s; the FIFO stays.
        let c = std::ffi::CString::new(d.view_json().as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let d2 = d.clone();
        std::thread::spawn(move || {
            let _ = tx.send(SessionView::load(&d2).map_err(|e| format!("{e:#}")));
        });
        let r = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("SessionView::load waited on a FIFO");
        assert_eq!(r.unwrap_err(), format!("{}: is a FIFO", d.view_json().display()));
        assert!(std::fs::symlink_metadata(d.view_json()).unwrap().file_type().is_fifo());
        std::fs::remove_file(d.view_json()).unwrap();
        // A folder, and a link to a folder: today's error, the one of `std::fs::read`.
        std::fs::create_dir(t.path().join("dir")).unwrap();
        let today = std::fs::read(t.path().join("dir")).unwrap_err().to_string();
        #[cfg(target_os = "linux")]
        assert_eq!(today, "Is a directory (os error 21)");
        std::fs::create_dir(d.view_json()).unwrap();
        assert_eq!(format!("{:#}", SessionView::load(&d).unwrap_err()), today);
        std::fs::remove_dir(d.view_json()).unwrap();
        std::os::unix::fs::symlink(t.path().join("dir"), d.view_json()).unwrap();
        assert_eq!(format!("{:#}", SessionView::load(&d).unwrap_err()), today);
        std::fs::remove_file(d.view_json()).unwrap();
    }
    // A missing view keeps today's error: the bare io error, no path added.
    let e = SessionView::load(&d).unwrap_err();
    assert_eq!(e.downcast_ref::<std::io::Error>().map(|e| e.kind()), Some(std::io::ErrorKind::NotFound), "{e:#}");
    assert!(!format!("{e:#}").contains("view.json"), "{e:#}");
    let v = SessionView { session: "s".into(), main_role: "main".into(), ..Default::default() };
    v.save(&d).unwrap();
    // More than 1 MiB still loads: no cap.
    let mut b = std::fs::read(d.view_json()).unwrap();
    b.extend(std::iter::repeat_n(b' ', 2 << 20));
    std::fs::write(d.view_json(), &b).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), v);
    // A link to a regular view still loads.
    #[cfg(unix)]
    {
        std::fs::rename(d.view_json(), t.path().join("v.json")).unwrap();
        std::os::unix::fs::symlink(t.path().join("v.json"), d.view_json()).unwrap();
        assert_eq!(SessionView::load(&d).unwrap(), v);
    }
}

#[cfg(unix)]
#[test] // FIX-22 review round 1, F3: every entry but a FIFO gives today's result; a link to a FIFO is a FIFO
#[ignore = "timing: the linked-FIFO load must end inside a 5 s recv_timeout"]
fn session_view_load_keeps_todays_result_for_other_entries() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use zygo_core::session_view::SessionView;
    // Under /tmp, not TMPDIR: a long TMPDIR makes the socket path longer than `sun_path` (review round 2).
    let t = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
    let d = SessionDir::new(t.path().join("s"));
    d.create().unwrap();
    // Today's read of the same path: `std::fs::read`, then serde.
    let today = |p: &std::path::Path| -> Result<SessionView, String> {
        let b = std::fs::read(p).map_err(|e| e.to_string())?;
        serde_json::from_slice(&b).map_err(|e| e.to_string())
    };
    let now = |d: &SessionDir| SessionView::load(d).map_err(|e| format!("{e:#}"));
    // A link to /dev/null: the serde error of an empty read.
    std::os::unix::fs::symlink("/dev/null", d.view_json()).unwrap();
    let want = today(&d.view_json());
    assert!(want.as_ref().is_err_and(|e| e.starts_with("EOF while parsing")), "{want:?}");
    assert_eq!(now(&d), want);
    std::fs::remove_file(d.view_json()).unwrap();
    // A bound socket: today's open error (ENXIO on Linux).
    let sock = std::os::unix::net::UnixListener::bind(d.view_json()).unwrap();
    let want = today(&d.view_json());
    #[cfg(target_os = "linux")]
    assert_eq!(want.clone().unwrap_err(), "No such device or address (os error 6)");
    assert_eq!(now(&d), want);
    drop(sock);
    std::fs::remove_file(d.view_json()).unwrap();
    // A mode-0 regular file: today's permission error (when the test runs as root both reads succeed).
    let v = SessionView { session: "s".into(), main_role: "main".into(), ..Default::default() };
    v.save(&d).unwrap();
    std::fs::set_permissions(d.view_json(), std::fs::Permissions::from_mode(0o000)).unwrap();
    let want = today(&d.view_json());
    assert_eq!(now(&d), want);
    std::fs::set_permissions(d.view_json(), std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::remove_file(d.view_json()).unwrap();
    // A link to a FIFO: `<view.json path>: is a FIFO` within 5 s.
    let fifo = t.path().join("fifo");
    let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    std::os::unix::fs::symlink(&fifo, d.view_json()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let d2 = d.clone();
    std::thread::spawn(move || {
        let _ = tx.send(SessionView::load(&d2).map_err(|e| format!("{e:#}")));
    });
    let r = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("SessionView::load waited on a linked FIFO");
    assert_eq!(r.unwrap_err(), format!("{}: is a FIFO", d.view_json().display()));
}

// ---- DR-8: session view `repo_root` (decision records D§13 item 6) ----

#[test]
fn repo_root_absent_from_view_json_reads_as_none() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    std::fs::write(d.view_json(), br#"{"session":"s","agent":"claude-code","main_role":"m","effective_role":null,"role_prompt":null,"user_prompt":null,"profile":null,"profile_prompt":null,"work_item":null,"legend":[],"disabled_roles":[],"work_units":true,"results":true,"navigation":false}"#).unwrap();
    let v = SessionView::load(&d).unwrap();
    assert_eq!(v.repo_root, None);
    assert_eq!(v.extra, Default::default());
    assert_eq!(SessionView::default().repo_root, None);
}

#[test]
fn repo_root_round_trips_through_save_and_load() {
    use zygo_core::session_view::SessionView;
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().to_path_buf());
    let v = SessionView { session: "s".into(), repo_root: Some("/r/top".into()), ..Default::default() };
    v.save(&d).unwrap();
    assert_eq!(SessionView::load(&d).unwrap(), v);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(raw["repo_root"], "/r/top");
}

#[test]
fn valid_member_name_accepts_the_m6_grammar() {
    // M§6: ^[a-z][a-z0-9-]{0,31}$
    let max = format!("a{}", "b".repeat(31));
    assert_eq!(max.len(), 32);
    for ok in ["elena", "a", max.as_str(), "a-", "a--b", "x9-y", "alle", "clip"] {
        assert!(valid_member_name(ok), "{ok:?}");
    }
}

#[test]
fn valid_member_name_refuses_others_and_the_reserved() {
    assert_eq!(RESERVED_MEMBER_NAMES, ["all", "cli"]);
    let long = format!("a{}", "b".repeat(32));
    assert_eq!(long.len(), 33);
    for bad in ["Elena", "1a", "-a", long.as_str(), "all", "cli", "", "a_b", "a.b", "a b", "é", "a/b"] {
        assert!(!valid_member_name(bad), "{bad:?}");
    }
}

#[test] // ML-1 review round 1, finding 2: a name of pane-key form (W§5 `^k[a-z2-7]{8}$`) would make mail_send's `to` ambiguous
fn valid_member_name_refuses_the_pane_key_form() {
    for bad in ["kabcdefgh", "k22222222", "kzzzzzzz7"] {
        assert!(!valid_member_name(bad), "{bad:?}");
    }
    // near misses: 8 characters, and a `1` (not in base32 `a-z2-7`) in 9 characters
    for ok in ["kabcdefg", "kabcdefg1"] {
        assert!(valid_member_name(ok), "{ok:?}");
    }
}
