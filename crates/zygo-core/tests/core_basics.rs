use zygo_core::ids::ulid;
use zygo_core::overrides::Overrides;
#[cfg(unix)]
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
