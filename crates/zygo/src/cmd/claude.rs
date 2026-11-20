use std::path::PathBuf;

use zygo_core::paths::Paths;
use zygo_launch::claude_code::ClaudeCode;
use zygo_launch::env::Env;
use zygo_launch::{LaunchOpts, Launcher};

use super::compact::{parse_window, Change};
use super::{bad_compact, print_err, Launch};

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
        Ok(opts) => Launcher::run(&ClaudeCode::default(), opts),
        Err(m) => {
            print_err(&format!("{m}\n"));
            2
        }
    }
}

fn launch_opts(l: &Launch, env: Env, cwd: PathBuf, paths: Paths) -> Result<LaunchOpts, String> {
    let compact = match l.compact.as_deref() {
        None => None,
        Some(v) => match parse_window(v) {
            Some(Change::Set(w)) => Some(w),
            _ => return Err(bad_compact("claude", v)),
        },
    };
    let mut o = LaunchOpts::new(env, cwd, paths);
    o.args = l.rest.clone();
    o.profile = l.profile.clone();
    o.passthrough = l.passthrough;
    o.server = l.server.clone();
    o.compact = compact;
    o.yolo = l.yolo;
    Ok(o)
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
            rest: vec!["-p".into(), "hi".into()],
        }
    }

    fn opts(l: &Launch) -> Result<zygo_launch::LaunchOpts, String> {
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
        launch_opts(l, Env::default(), p("w"), paths)
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
}
