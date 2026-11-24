use std::path::{Path, PathBuf};

use zygo_core::paths::Paths;
use zygo_launch::claude_code::ClaudeCode;
use zygo_launch::env::Env;
use zygo_launch::{AgentAdapter, LaunchOpts};

use super::compact::{parse_window, Change};
use super::{bad_compact, member, print_err, Launch};

/// `zygo claude`: launches Claude Code (§8.2); returns the process exit code.
pub(super) fn run(l: &Launch) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            print_err(&format!("zygo claude: cannot read the current directory: {e}\n"));
            return 1;
        }
    };
    match launch_opts(l, Env::from_process(), cwd, Paths::from_env()) {
        Ok(opts) => member::launch_hand(&|z| -> Box<dyn AgentAdapter> { Box::new(ClaudeCode::for_zygo(z)) }, opts, saved_launch(l)),
        Err(m) => {
            print_err(&format!("{m}\n"));
            2
        }
    }
}

pub(crate) fn launch_opts(l: &Launch, mut env: Env, cwd: PathBuf, paths: Paths) -> Result<LaunchOpts, String> {
    let compact = match l.compact.as_deref() {
        None => None,
        Some(v) => match parse_window(v) {
            Some(Change::Set(w)) => Some(w),
            _ => return Err(bad_compact("claude", v)),
        },
    };
    if let Some(d) = l.config_dir.as_deref() {
        set_config_dir(&mut env, d, &cwd)?;
    }
    let mut o = LaunchOpts::new(env, cwd, paths);
    o.args = l.rest.clone();
    o.profile = l.profile.clone();
    o.passthrough = l.passthrough;
    o.server = l.server.clone();
    o.compact = compact;
    o.yolo = l.yolo;
    Ok(o)
}

/// The saved launch of a hand-started member (W§8.4, AU45): every flag as typed, the arguments after `--`.
fn saved_launch(l: &Launch) -> zygo_workspace::state::Launch {
    zygo_workspace::state::Launch {
        profile: l.profile.clone(),
        passthrough: l.passthrough,
        server: l.server.clone(),
        compact: l.compact.clone(),
        yolo: l.yolo,
        config_dir: l.config_dir.clone(),
        args: l.rest.clone(),
        ..zygo_workspace::state::Launch::new(zygo_core::Agent::ClaudeCode)
    }
}

/// `--config-dir`: expands a leading `~`, makes a relative path absolute against `cwd` (not canonicalized), checks for a directory, and
/// sets `CLAUDE_CONFIG_DIR` in `env` (the flag wins).
fn set_config_dir(env: &mut Env, d: &str, cwd: &Path) -> Result<(), String> {
    const VAR: &str = "CLAUDE_CONFIG_DIR";
    let rest = if d == "~" { Some("") } else { d.strip_prefix("~/") };
    let dir = match rest {
        Some(r) => {
            let home = env.get(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|h| h.is_absolute());
            match home.or_else(std::env::home_dir) {
                Some(h) if r.is_empty() => h,
                Some(h) => h.join(r),
                None => return Err(format!("zygo claude: --config-dir {d}: no home directory")),
            }
        }
        None => cwd.join(d),
    };
    if !dir.is_dir() {
        return Err(format!("zygo claude: --config-dir {d}: not a directory"));
    }
    let dir = dir.into_os_string();
    if let Some(old) = env.get(VAR).filter(|v| !v.is_empty() && *v != dir) {
        print_err(&format!("zygo claude: --config-dir {} overrides {VAR}={}\n", dir.to_string_lossy(), old.to_string_lossy()));
    }
    let same = |n: &str| if cfg!(windows) { n.eq_ignore_ascii_case(VAR) } else { n == VAR };
    env.0.retain(|(k, _)| !k.to_str().is_some_and(same));
    env.0.push((VAR.into(), dir));
    Ok(())
}

#[cfg(test)]
mod tests {
    use zygo_proxy::client::WindowArg;

    use super::*;

    fn args(compact: Option<&str>) -> Launch {
        Launch {
            profile: Some("p".into()),
            passthrough: true,
            server: Some("https://s.invalid".into()),
            compact: compact.map(String::from),
            yolo: true,
            config_dir: None,
            rest: vec!["-p".into(), "hi".into()],
        }
    }

    fn opts(l: &Launch) -> Result<zygo_launch::LaunchOpts, String> {
        opts_env(l, Env::default())
    }

    fn opts_env(l: &Launch, env: Env) -> Result<zygo_launch::LaunchOpts, String> {
        opts_cwd(l, env, PathBuf::from("w"))
    }

    fn opts_cwd(l: &Launch, env: Env, cwd: PathBuf) -> Result<zygo_launch::LaunchOpts, String> {
        let p = |s: &str| PathBuf::from(s);
        let paths = Paths {
            config_file: p("c"),
            cache_dir: p("k"),
            data_dir: p("d"),
            sessions_dir: p("s"),
            marketplaces_dir: p("m"),
            plugin_store: p("ps"),
            approvals_file: p("a"),
            usage_db: p("u"),
        };
        launch_opts(l, env, cwd, paths)
    }

    fn with_dir(dir: &str) -> Launch {
        Launch { config_dir: Some(dir.into()), ..args(None) }
    }

    fn env(rows: &[(&str, &str)]) -> Env {
        Env(rows.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect())
    }

    fn config_dir(o: &zygo_launch::LaunchOpts) -> Option<String> {
        o.env.get("CLAUDE_CONFIG_DIR").map(|v| v.to_string_lossy().into_owned())
    }

    #[test]
    fn config_dir_sets_env() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().to_string_lossy().into_owned();
        assert_eq!(config_dir(&opts(&with_dir(&d)).unwrap()).as_deref(), Some(d.as_str()));
        assert_eq!(config_dir(&opts(&args(None)).unwrap()), None);
    }

    #[test]
    fn config_dir_expands_tilde() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("maya")).unwrap();
        let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let e = env(&[(home, &t.path().to_string_lossy())]);
        let o = opts_env(&with_dir("~/maya"), e).unwrap();
        assert_eq!(config_dir(&o), Some(t.path().join("maya").to_string_lossy().into_owned()));
    }

    #[test]
    fn config_dir_bare_tilde_is_home() {
        let t = tempfile::tempdir().unwrap();
        let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let o = opts_env(&with_dir("~"), env(&[(home, &t.path().to_string_lossy())])).unwrap();
        assert_eq!(config_dir(&o), Some(t.path().to_string_lossy().into_owned()));
    }

    #[test]
    fn config_dir_relative_is_absolute_against_cwd() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join(".claude-x")).unwrap();
        let o = opts_cwd(&with_dir(".claude-x"), Env::default(), t.path().to_path_buf()).unwrap();
        assert_eq!(config_dir(&o), Some(t.path().join(".claude-x").to_string_lossy().into_owned()));
    }

    #[test]
    fn config_dir_ignores_relative_home() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("rel-home")).unwrap();
        let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let r = opts_cwd(&with_dir("~"), env(&[(home, "rel-home")]), t.path().to_path_buf());
        match r {
            Ok(o) => {
                let d = PathBuf::from(config_dir(&o).unwrap());
                assert!(d.is_absolute() && !d.starts_with(t.path().join("rel-home")) && !d.starts_with("rel-home"), "{d:?}");
            }
            Err(e) => assert_eq!(e, "zygo claude: --config-dir ~: no home directory"),
        }
    }

    #[test]
    fn config_dir_missing_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("absent").to_string_lossy().into_owned();
        let e = opts(&with_dir(&d)).err().unwrap();
        assert_eq!(e, format!("zygo claude: --config-dir {d}: not a directory"));
        let f = t.path().join("file");
        std::fs::write(&f, "").unwrap();
        assert!(opts(&with_dir(&f.to_string_lossy())).is_err());
    }

    #[test]
    fn config_dir_overrides_inherited() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().to_string_lossy().into_owned();
        let o = opts_env(&with_dir(&d), env(&[("CLAUDE_CONFIG_DIR", "/elsewhere"), ("PATH", "/bin")])).unwrap();
        assert_eq!(config_dir(&o).as_deref(), Some(d.as_str()));
        assert_eq!(o.env.0.iter().filter(|(k, _)| k == "CLAUDE_CONFIG_DIR").count(), 1);
        assert_eq!(o.env.get("PATH").map(|v| v.to_string_lossy().into_owned()).as_deref(), Some("/bin"));
    }

    #[test]
    fn launch_opts_maps_flags() {
        let o = opts(&args(Some("600k"))).unwrap();
        assert_eq!(o.profile.as_deref(), Some("p"));
        assert!(o.passthrough);
        assert_eq!(o.server.as_deref(), Some("https://s.invalid"));
        assert_eq!(o.compact, Some(WindowArg::Tokens(600_000)));
        assert!(o.yolo);
        assert_eq!(o.args, ["-p", "hi"]);
        assert_eq!(opts(&args(Some("off"))).unwrap().compact, Some(WindowArg::Off));
        assert_eq!(opts(&args(None)).unwrap().compact, None);
        let off = Launch { passthrough: false, yolo: false, ..args(None) };
        let o = opts(&off).unwrap();
        assert!(!o.passthrough);
        assert!(!o.yolo);
    }

    #[test]
    fn saved_launch_keeps_the_flags_as_typed() {
        let l = Launch { compact: Some("600k".into()), config_dir: Some("~/cc".into()), ..args(None) };
        let s = saved_launch(&l);
        assert_eq!(s.kind, zygo_core::Agent::ClaudeCode);
        assert_eq!((s.profile.as_deref(), s.passthrough, s.server.as_deref()), (Some("p"), true, Some("https://s.invalid")));
        assert_eq!((s.compact.as_deref(), s.yolo, s.config_dir.as_deref()), (Some("600k"), true, Some("~/cc")));
        assert_eq!(s.args, ["-p", "hi"]);
    }
}
