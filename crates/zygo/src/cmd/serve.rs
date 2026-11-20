//! `zygo serve`, `zygo serve token new <label>`, `zygo serve reload` (§7.1, §7.2, §17).

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use zygo_config::{load, LoadContext};
use zygo_core::exit::ExitCode;
use zygo_core::paths::Paths;
use zygo_proxy::serve::{self, ConfigInvalid, ServeOpts, TlsFiles, TlsRequired};
use zygo_proxy::server::StartOpts;
use zygo_proxy::team;

#[derive(Parser)]
#[command(name = "zygo serve", no_binary_name = true)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8443")]
    listen: String,
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,
    #[command(subcommand)]
    sub: Option<Sub>,
}

#[derive(Subcommand)]
enum Sub {
    /// Manage serve tokens
    Token {
        #[command(subcommand)]
        sub: TokenSub,
    },
    /// Reload the server config
    Reload,
}

#[derive(Subcommand)]
enum TokenSub {
    /// Create a token
    New { label: String },
}

/// Error output; the server's report lines go there too, so it outlives this call.
type Sink = Arc<dyn Fn(&str) + Send + Sync>;

pub fn run() -> i32 {
    let tokens: Vec<String> = std::env::args().skip(2).collect();
    run_in(&tokens, &Paths::from_env(), &|s| super::print_out(s), Arc::new(|s: &str| super::print_err(s)))
}

fn run_in(tokens: &[String], paths: &Paths, out: &dyn Fn(&str), err: Sink) -> i32 {
    let a = match ServeArgs::try_parse_from(tokens) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    match a.sub {
        Some(Sub::Token { sub: TokenSub::New { label } }) => {
            let (t, h) = team::new_token();
            match team::append_token(&paths.config_file, &label, &h) {
                Ok(()) => {
                    out(&format!("{t}\n"));
                    let f = paths.config_file.display();
                    err(&format!("zygo serve: token for {label} added to {f}; it is not shown again\n"));
                    0
                }
                Err(e) => {
                    err(&format!("zygo serve: {e:#}\n"));
                    1
                }
            }
        }
        Some(Sub::Reload) => match serve::reload_running(&paths.data_dir.join("serve.pid")) {
            Ok(pid) => {
                err(&format!("zygo serve: reload sent to process {pid}\n"));
                0
            }
            Err(e) => {
                err(&format!("zygo serve reload: {e:#}\n"));
                1
            }
        },
        None => {
            let ctx = LoadContext { paths: paths.clone(), repo: None, team: None };
            let report = err.clone();
            let opts = ServeOpts {
                listen: a.listen,
                tls: a.tls_cert.zip(a.tls_key).map(|(cert, key)| TlsFiles { cert, key }),
                config_file: paths.config_file.clone(),
                load: Arc::new(move || Ok(load(&ctx)?)),
                report: Arc::new(move |l: &str| report(&format!("{l}\n"))),
                pid_file: Some(paths.data_dir.join("serve.pid")),
                signals: true,
                start: StartOpts { usage_db: Some(paths.usage_db.clone()), ..Default::default() },
            };
            match serve::run(opts) {
                Ok(()) => 0,
                Err(e) => {
                    err(&format!("zygo serve: {e:#}\n"));
                    if e.downcast_ref::<TlsRequired>().is_some() {
                        ExitCode::Usage as i32
                    } else if e.downcast_ref::<ConfigInvalid>().is_some() {
                        ExitCode::ConfigInvalid as i32
                    } else {
                        1
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::Mutex;

    fn paths(r: &std::path::Path) -> Paths {
        Paths {
            config_file: r.join("config.toml"),
            cache_dir: r.join("cache"),
            data_dir: r.join("data"),
            sessions_dir: r.join("cache/sessions"),
            marketplaces_dir: r.join("cache/marketplaces"),
            plugin_store: r.join("data/plugins"),
            approvals_file: r.join("data/approvals.toml"),
            usage_db: r.join("data/usage.db"),
        }
    }

    /// (exit code, stdout, stderr)
    fn call(args: &[&str], p: &Paths) -> (i32, String, String) {
        let out = std::cell::RefCell::new(String::new());
        let err = Arc::new(Mutex::new(String::new()));
        let e2 = err.clone();
        let tokens: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let code = run_in(&tokens, p, &|s| out.borrow_mut().push_str(s), Arc::new(move |s: &str| e2.lock().unwrap().push_str(s)));
        let e = err.lock().unwrap().clone();
        (code, out.into_inner(), e)
    }

    #[test]
    fn serve_token_new_prints_and_appends() {
        let d = tempfile::tempdir().unwrap();
        let p = paths(d.path());
        let (code, out, _) = call(&["token", "new", "alice"], &p);
        assert_eq!(code, 0);
        let t = out.strip_suffix('\n').unwrap();
        assert!(!t.contains('\n') && t.starts_with("zyg_t_") && t.len() == 49, "{out}");
        let m = load(&LoadContext { paths: p, repo: None, team: None }).unwrap();
        let tok = &m.config.serve.tokens;
        assert_eq!(tok.len(), 1);
        assert_eq!(tok[0].label, "alice");
        assert_eq!(tok[0].sha256, hex::encode(Sha256::digest(t.as_bytes())));
    }

    #[test]
    fn serve_token_new_existing_label_exit_1() {
        let d = tempfile::tempdir().unwrap();
        let p = paths(d.path());
        assert_eq!(call(&["token", "new", "alice"], &p).0, 0);
        let (code, out, err) = call(&["token", "new", "alice"], &p);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(err, "zygo serve: label alice exists; remove it from the config first\n");
    }

    #[test]
    fn serve_reload_no_server_exit_1() {
        let d = tempfile::tempdir().unwrap();
        let (code, _, err) = call(&["reload"], &paths(d.path()));
        assert_eq!(code, 1);
        assert_eq!(err, "zygo serve reload: no running zygo serve\n");
    }

    #[test]
    fn serve_c8_refusal_exit_2() {
        let d = tempfile::tempdir().unwrap();
        let p = paths(d.path());
        let (code, _, err) = call(&["--listen", "10.0.0.5:8443"], &p);
        assert_eq!(code, 2);
        assert!(err.contains("(C8)"), "{err}");
        assert!(!p.data_dir.join("serve.pid").exists());
    }

    #[test]
    fn serve_config_error_exit_11() {
        let d = tempfile::tempdir().unwrap();
        let p = paths(d.path());
        std::fs::write(&p.config_file, "[[ not toml").unwrap();
        let (code, _, err) = call(&["--listen", "127.0.0.1:0"], &p);
        assert_eq!(code, 11);
        assert_eq!(err, format!("zygo serve: config invalid ({}); run zygo config check\n", p.config_file.display()));
    }
}
