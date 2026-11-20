//! `zygo keys set|rm|list` (§6.2, §17). A secret is read from stdin, never from argv, and never printed.

use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use zygo_config::keys::{self, resolve_with, SecretStore};
use zygo_config::{load, KeyRef, LoadContext};
use zygo_core::exit::ExitCode;
use zygo_core::paths::Paths;

#[derive(Parser)]
#[command(name = "zygo keys", no_binary_name = true)]
struct KeysArgs {
    #[command(subcommand)]
    sub: Sub,
}

#[derive(Subcommand)]
enum Sub {
    /// Store a key read from stdin
    Set { provider: String },
    /// Remove a stored key
    Rm { provider: String },
    /// Show each key reference and whether it is set
    List { provider: Option<String> },
}

/// Reads one line and removes a trailing `\r\n` or `\n`.
pub(crate) fn read_line_secret(r: &mut dyn BufRead) -> io::Result<String> {
    let mut s = String::new();
    r.read_line(&mut s)?;
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    Ok(s)
}

pub fn run() -> i32 {
    let tokens: Vec<String> = std::env::args().skip(2).collect();
    let ctx = LoadContext { paths: Paths::from_env(), repo: std::env::current_dir().ok(), team: None };
    let store = keys::default_store(&ctx.paths);
    let mut read = || {
        if io::stdin().is_terminal() {
            super::print_err("key: ");
            rpassword::read_password()
        } else {
            read_line_secret(&mut io::stdin().lock())
        }
    };
    run_in(&tokens, &ctx, &store, &|n| std::env::var_os(n), &mut read, &mut io::stdout(), &mut io::stderr())
}

fn run_in(
    tokens: &[String],
    ctx: &LoadContext,
    s: &dyn SecretStore,
    env: &dyn Fn(&str) -> Option<OsString>,
    read: &mut dyn FnMut() -> io::Result<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let args = match KeysArgs::try_parse_from(tokens) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    let name = match &args.sub {
        Sub::Set { provider } | Sub::Rm { provider } | Sub::List { provider: Some(provider) } => Some(provider),
        Sub::List { provider: None } => None,
    };
    if name.is_some_and(|n| n.trim().is_empty()) {
        let _ = writeln!(err, "zygo keys: empty provider name");
        return ExitCode::Usage as i32;
    }
    match args.sub {
        Sub::Set { provider } => {
            let value = match read() {
                Ok(v) => v.trim().to_string(),
                Err(e) => {
                    let _ = writeln!(err, "zygo keys: cannot read key: {e}");
                    return 1;
                }
            };
            if value.is_empty() {
                let _ = writeln!(err, "zygo keys: empty key");
                return 1;
            }
            match s.set_reporting(&provider, &keys::Secret::new(value)) {
                Ok((b, fallback)) => {
                    if let Some(f) = fallback {
                        let _ = writeln!(
                            err,
                            "zygo keys: OS secret store refused ({}); stored in file {}",
                            f.reason,
                            f.file.display()
                        );
                    }
                    let _ = writeln!(out, "stored {provider} in {b}");
                    0
                }
                Err(e) => {
                    let _ = writeln!(err, "zygo keys: {e}");
                    1
                }
            }
        }
        Sub::Rm { provider } => match s.delete(&provider) {
            Ok(true) => {
                let _ = writeln!(out, "removed {provider}");
                0
            }
            Ok(false) => {
                let _ = writeln!(out, "no key {provider}");
                0
            }
            Err(e) => {
                let _ = writeln!(err, "zygo keys: {e}");
                1
            }
        },
        Sub::List { provider } => {
            let refs: Vec<KeyRef> = match provider {
                Some(n) => vec![KeyRef::Keychain(n)],
                None => {
                    let merged = match load(ctx) {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = writeln!(err, "zygo keys: {e}");
                            return ExitCode::ConfigInvalid as i32;
                        }
                    };
                    let cfg = &merged.config;
                    let mut all: BTreeMap<String, KeyRef> = BTreeMap::new();
                    let found = cfg.catalog.0.iter().flat_map(|m| &m.provider).filter_map(|p| p.key.as_ref())
                        .chain(cfg.server.token.as_ref());
                    for k in found {
                        all.insert(k.to_string(), k.clone());
                    }
                    all.into_values().collect()
                }
            };
            for r in refs {
                let state = match resolve_with(&r, s, env) {
                    Ok(Some(_)) => "<set>",
                    Ok(None) => "<unset>",
                    Err(e) => {
                        let _ = writeln!(err, "zygo keys: cannot read {r}: {e}");
                        "<error>"
                    }
                };
                let _ = writeln!(out, "{r} {state}");
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use std::io::Cursor;
    use std::sync::Arc;
    use zygo_config::keys::{Auto, Backend, FileStore, MemStore, Secret};
    
    const CFG: &str = r#"
    [[model]]
    id = "m1"
    [model.facts]
    reasoning = "always"
    effort_levels = ["low", "high"]
    context_window = 200000
    max_output_tokens = 64000
    [[model.provider]]
    name = "acme"
    base_url = "https://acme.example.invalid"
    upstream_model = "u"
    dialects = ["anthropic"]
    key = "keychain:acme"
    
    [[model]]
    id = "m2"
    [model.facts]
    reasoning = "always"
    effort_levels = ["low", "high"]
    context_window = 200000
    max_output_tokens = 64000
    [[model.provider]]
    name = "viaenv"
    base_url = "https://env.example.invalid"
    upstream_model = "u"
    dialects = ["anthropic"]
    key = "env:ZYGO_N5_LIST"
    
    [server]
    token = "keychain:zygo-team"
    "#;
    
    fn fixture() -> (tempfile::TempDir, LoadContext) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let config_file = root.join("config.toml");
        std::fs::write(&config_file, CFG).unwrap();
        let paths = Paths {
            config_file,
            cache_dir: root.join("cache"),
            data_dir: root.join("data"),
            sessions_dir: root.join("cache/sessions"),
            marketplaces_dir: root.join("cache/marketplaces"),
            plugin_store: root.join("data/plugins"),
            approvals_file: root.join("data/approvals.toml"),
            usage_db: root.join("data/usage.db"),
        };
        (d, LoadContext { paths, repo: None, team: None })
    }
    
    fn go(args: &[&str], s: &dyn SecretStore, env_val: Option<&str>, input: &str, ctx: &LoadContext) -> (i32, String, String) {
        let toks: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let env = |n: &str| if n == "ZYGO_N5_LIST" { env_val.map(OsString::from) } else { None };
        let mut stdin = Cursor::new(input.as_bytes().to_vec());
        let mut read = || read_line_secret(&mut stdin);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_in(&toks, ctx, s, &env, &mut read, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }
    
    /// A store whose delete fails.
    struct BadDelete;
    impl SecretStore for BadDelete {
        fn backend(&self) -> Backend { Backend::File }
        fn get(&self, _: &str) -> anyhow::Result<Option<Secret>> { Ok(None) }
        fn set(&self, _: &str, _: &Secret) -> anyhow::Result<Backend> { Ok(Backend::File) }
        fn delete(&self, _: &str) -> anyhow::Result<bool> { anyhow::bail!("os delete failed") }
    }
    
    /// An OS store over shared memory whose set can fail.
    struct Os(Arc<MemStore>, bool);
    impl SecretStore for Os {
        fn backend(&self) -> Backend { Backend::OsKeychain }
        fn get(&self, n: &str) -> anyhow::Result<Option<Secret>> { self.0.get(n) }
        fn set(&self, n: &str, v: &Secret) -> anyhow::Result<Backend> {
            if self.1 { anyhow::bail!("os set failed") }
            self.0.set(n, v)
        }
        fn delete(&self, n: &str) -> anyhow::Result<bool> { self.0.delete(n) }
    }

    /// The keys file under the fixture's data dir, holding invalid TOML that contains `secret`.
    fn corrupt_file(ctx: &LoadContext, secret: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(&ctx.paths.data_dir).unwrap();
        let f = ctx.paths.data_dir.join(keys::FILE_NAME);
        std::fs::write(&f, format!("acme = {secret}\n")).unwrap();
        f
    }

    #[test]
    fn set_trims_whitespace_and_rejects_a_blank_key() {
        let (_d, ctx) = fixture();
        let s = MemStore::new();
        let v = format!("tv-{}", ulid::Ulid::generate());
        assert_eq!(go(&["set", "acme"], &s, None, &format!(" \t{v} \r\n"), &ctx).0, 0);
        assert_eq!(s.get("acme").unwrap().unwrap().expose(), v);
        assert_eq!(go(&["set", "other"], &s, None, " \t\r\n", &ctx), (1, String::new(), "zygo keys: empty key\n".into()));
        assert!(s.get("other").unwrap().is_none());
    }

    #[test]
    fn rm_and_list_reject_an_empty_name() {
        let (_d, ctx) = fixture();
        let s = MemStore::new();
        assert_eq!(go(&["rm", ""], &s, None, "", &ctx).0, 2);
        assert_eq!(go(&["list", ""], &s, None, "", &ctx).0, 2);
    }

    #[test]
    fn list_counts_a_whitespace_env_value_as_unset() {
        let (_d, ctx) = fixture();
        let (_, out, _) = go(&["list"], &MemStore::new(), Some(" \t"), "", &ctx);
        assert!(out.lines().any(|l| l == "env:ZYGO_N5_LIST <unset>"), "{out}");
    }

    #[test]
    fn set_fallback_prints_one_warning_without_the_value() {
        let (_d, ctx) = fixture();
        let file = ctx.paths.data_dir.join(keys::FILE_NAME);
        let a = Auto::new(Some(Box::new(Os(Arc::new(MemStore::new()), true))), FileStore::new(file.clone()));
        let v = format!("fv-{}", ulid::Ulid::generate());
        let (code, out, err) = go(&["set", "acme"], &a, None, &format!("{v}\n"), &ctx);
        assert_eq!((code, out), (0, format!("stored acme in {}\n", Backend::File)));
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("zygo keys: OS secret store refused ("), "{err}");
        assert!(err.ends_with(&format!("); stored in file {}\n", file.display())), "{err}");
        assert!(!err.contains(&v));
    }

    #[test]
    fn rm_clears_the_os_entry_when_the_file_is_corrupt() {
        let (_d, ctx) = fixture();
        let mem = Arc::new(MemStore::new());
        mem.set("acme", &Secret::new(format!("o-{}", ulid::Ulid::generate()))).unwrap();
        let secret = format!("c-{}", ulid::Ulid::generate());
        let a = Auto::new(Some(Box::new(Os(mem.clone(), false))), FileStore::new(corrupt_file(&ctx, &secret)));
        let (code, _, err) = go(&["rm", "acme"], &a, None, "", &ctx);
        assert_eq!(code, 1);
        assert!(err.starts_with("zygo keys: ") && err.contains("not valid TOML") && !err.contains(&secret), "{err}");
        assert!(mem.get("acme").unwrap().is_none());
    }

    #[test]
    fn list_marks_a_store_error_and_reports_it_once() {
        let (_d, ctx) = fixture();
        let secret = format!("c-{}", ulid::Ulid::generate());
        let a = Auto::new(None, FileStore::new(corrupt_file(&ctx, &secret)));
        let (code, out, err) = go(&["list", "acme"], &a, None, "", &ctx);
        assert_eq!((code, out.as_str()), (0, "keychain:acme <error>\n"));
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("zygo keys: ") && !err.contains(&secret), "{err}");
    }

    #[test]
    fn set_reads_crlf_line_stores_it_and_never_prints_it() {
        let (_d, ctx) = fixture();
        let s = MemStore::new();
        let v = format!("sv-{}", ulid::Ulid::generate());
        let (code, out, err) = go(&["set", "acme"], &s, None, &format!("{v}\r\n"), &ctx);
        assert_eq!(code, 0);
        assert_eq!(out, format!("stored acme in {}\n", Backend::OsKeychain));
        assert!(!out.contains(&v) && !err.contains(&v));
        assert_eq!(s.get("acme").unwrap().unwrap().expose(), v);
    }
    
    #[test]
    fn set_rejects_empty_secret_and_empty_name_without_storing() {
        let (_d, ctx) = fixture();
        let s = MemStore::new();
        assert_eq!(go(&["set", "acme"], &s, None, "\n", &ctx).0, 1);
        assert_eq!(go(&["set", ""], &s, None, "x\n", &ctx).0, 2);
        assert!(s.get("acme").unwrap().is_none());
    }
    
    #[test]
    fn rm_removes_and_is_idempotent() {
        let (_d, ctx) = fixture();
        let s = MemStore::new();
        s.set("acme", &Secret::new(format!("r-{}", ulid::Ulid::generate()))).unwrap();
        assert_eq!(go(&["rm", "acme"], &s, None, "", &ctx), (0, "removed acme\n".into(), String::new()));
        assert_eq!(go(&["rm", "acme"], &s, None, "", &ctx), (0, "no key acme\n".into(), String::new()));
    }
    
    #[test]
    fn rm_exits_1_naming_a_store_error() {
        let (_d, ctx) = fixture();
        let (code, out, err) = go(&["rm", "acme"], &BadDelete, None, "", &ctx);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("os delete failed"), "{err}");
    }
    
    #[test]
    fn list_shows_refs_with_set_state_only_including_env_refs() {
        let (_d, ctx) = fixture();
        let s = MemStore::new();
        let v = format!("l-{}", ulid::Ulid::generate());
        s.set("acme", &Secret::new(v.clone())).unwrap();
        let (code, out, _) = go(&["list"], &s, None, "", &ctx);
        assert_eq!(code, 0);
        assert_eq!(out, "env:ANTHROPIC_API_KEY <unset>\nenv:OPENAI_API_KEY <unset>\nenv:ZYGO_N5_LIST <unset>\nkeychain:acme <set>\nkeychain:zygo-team <unset>\n");
        assert!(!out.contains(&v));
        let (_, out, _) = go(&["list"], &s, Some("present"), "", &ctx);
        assert!(out.lines().any(|l| l == "env:ZYGO_N5_LIST <set>"), "{out}");
        assert!(!out.contains("present"));
        assert_eq!(go(&["list", "acme"], &s, None, "", &ctx).1, "keychain:acme <set>\n");
        assert_eq!(go(&["list", "nope"], &s, None, "", &ctx).1, "keychain:nope <unset>\n");
    }
}
