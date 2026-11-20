use std::path::PathBuf;

use zygo_core::paths::Paths;
use zygo_launch::env::Env;
use zygo_launch::opencode::OpenCode;
use zygo_launch::{LaunchOpts, Launcher};

use super::compact::{parse_window, Change};
use super::{bad_compact, print_err, LaunchOpenCode};

/// `zygo opencode`: launches OpenCode (§8.2); returns the process exit code.
pub(crate) fn run(a: &LaunchOpenCode) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            print_err(&format!("zygo opencode: cannot read the current directory: {e}\n"));
            return 1;
        }
    };
    match launch_opts(a, Env::from_process(), cwd, Paths::from_env()) {
        Ok(opts) => Launcher::run(&OpenCode::default(), opts),
        Err(m) => {
            print_err(&format!("{m}\n"));
            2
        }
    }
}

fn launch_opts(a: &LaunchOpenCode, env: Env, cwd: PathBuf, paths: Paths) -> Result<LaunchOpts, String> {
    let compact = match a.compact.as_deref() {
        None => None,
        Some(v) => match parse_window(v) {
            Some(Change::Set(w)) => Some(w),
            _ => return Err(bad_compact("opencode", v)),
        },
    };
    let mut o = LaunchOpts::new(env, cwd, paths);
    o.args = a.rest.clone();
    o.profile = a.profile.clone();
    o.server = a.server.clone();
    o.compact = compact;
    o.yolo = a.yolo;
    Ok(o)
}

#[cfg(test)]
mod tests {
    use zygo_proxy::client::WindowArg;

    use super::*;

    fn args(compact: Option<&str>) -> LaunchOpenCode {
        LaunchOpenCode {
            profile: Some("p1".into()),
            server: Some("https://example.invalid".into()),
            compact: compact.map(String::from),
            yolo: true,
            rest: vec!["run".into(), "x".into()],
        }
    }

    fn opts(a: &LaunchOpenCode) -> Result<zygo_launch::LaunchOpts, String> {
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
        launch_opts(a, Env::default(), p("w"), paths)
    }

    #[test]
    fn launch_opts_maps_flags() {
        let o = opts(&args(Some("600k"))).unwrap();
        assert_eq!(o.profile.as_deref(), Some("p1"));
        assert_eq!(o.server.as_deref(), Some("https://example.invalid"));
        assert_eq!(o.args, ["run", "x"]);
        assert!(o.yolo);
        assert!(!o.passthrough);
        assert_eq!(o.compact, Some(WindowArg::Tokens(600_000)));
        assert_eq!(opts(&args(Some("off"))).unwrap().compact, Some(WindowArg::Off));
        assert_eq!(opts(&args(None)).unwrap().compact, None);
    }

    #[test]
    fn launch_opts_rejects_a_bad_compact() {
        for v in ["on", "12x"] {
            assert_eq!(opts(&args(Some(v))).err(), Some(bad_compact("opencode", v)));
        }
    }

    #[test]
    fn bad_compact_text() {
        assert_eq!(
            bad_compact("opencode", "on"),
            "zygo opencode: bad --compact value `on`: use <tokens>, <n>k or <n>m from 100k to 10m, or off"
        );
    }
}
