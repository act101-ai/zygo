//! `zygo config show [--layer <name>] | check` (§6.1, §6.3, §17).

use clap::{Parser, Subcommand};
use zygo_config::keys::{self, resolve_with, SecretStore};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use zygo_config::layers::seg;
use zygo_config::{check, load, Config, KeyRef, Layer, LoadContext, Merged, Severity, Violation};
use zygo_core::exit::ExitCode;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_workspace::args::{has_user_info, removed_labels};

use super::compact::{parse_window, Change};
use super::loadout::{hide_secrets, print_folder_warning, PaneBase};
use super::verbs::VerbCtx;

#[derive(Parser)]
#[command(name = "zygo config", no_binary_name = true)]
struct ConfigArgs {
    #[command(subcommand)]
    sub: Sub,
}

#[derive(Subcommand)]
enum Sub {
    /// Print the merged config with each key's source layer
    Show {
        #[arg(long)]
        layer: Option<String>,
    },
    /// Validate the config
    Check,
}

fn parse(tokens: &[String]) -> Result<ConfigArgs, clap::Error> {
    ConfigArgs::try_parse_from(tokens)
}

/// The `show` text with each key as `<set>`, `<unset>` or `<error>`; one line on `err` per key the store could not read.
/// Each loadout is shown after `hide_secrets`, with each pane's folder taken against `base` (FIX-27 D7).
fn render_show(
    m: &Merged,
    layer: Option<Layer>,
    s: &dyn SecretStore,
    env: &dyn Fn(&str) -> Option<OsString>,
    err: &mut dyn Write,
    base: &PaneBase,
) -> String {
    let mut m = m.clone();
    for l in m.config.loadout.values_mut() {
        hide_secrets(l, base);
    }
    let failed: RefCell<BTreeMap<String, String>> = RefCell::default();
    let mut text = m.show(layer, &|k: &KeyRef| match resolve_with(k, s, env) {
        Ok(v) => v.is_some(),
        Err(e) => {
            failed.borrow_mut().insert(k.to_string(), e.to_string());
            false
        }
    });
    // `show` prints a failed key as `"<ref>" <unset>`; it is marked `<error>` here.
    for (r, e) in failed.into_inner() {
        text = text.replace(&zygo_core::esc(&format!("\"{r}\" <unset>")), &zygo_core::esc(&format!("\"{r}\" <error>")));
        let _ = writeln!(err, "zygo config: cannot read {}: {}", zygo_core::esc(&r), zygo_core::esc(&e));
    }
    text
}

/// The loadout checks that need the compact parser, AU30 or AU98 (W§11, AU103; FIX-27 D9), per `claude-code` and
/// `opencode` pane in loadout order: a `compact` that the launch refuses (`on` included) and a `server` with user info
/// are errors; arguments that AU30 removes, taken against the pane's folder, give one warning that names them.
fn loadout_checks(cfg: &Config, base: &PaneBase) -> Vec<Violation> {
    let mut out = Vec::new();
    for (name, l) in &cfg.loadout {
        for (wi, w) in l.window.iter().enumerate() {
            for (pi, p) in w.pane.iter().enumerate() {
                let (agent, word) = match p.kind.parse::<Agent>() {
                    Ok(Agent::ClaudeCode) => (Agent::ClaudeCode, "claude"),
                    Ok(Agent::OpenCode) => (Agent::OpenCode, "opencode"),
                    Err(_) => continue,
                };
                let path = format!("loadout.{}.window[{wi}].pane[{pi}]", seg(name));
                if let Some(v) = p.compact.as_deref()
                    && !matches!(parse_window(v), Some(Change::Set(_)))
                {
                    let full = super::bad_compact(word, v);
                    let prefix = format!("zygo {word}: ");
                    let message = full.strip_prefix(&prefix).unwrap_or(&full).to_string();
                    out.push(Violation { severity: Severity::Error, path: format!("{path}.compact"), message });
                }
                if p.server.as_deref().is_some_and(has_user_info) {
                    out.push(Violation {
                        severity: Severity::Error,
                        path: format!("{path}.server"),
                        message: "holds user info; zygo up starts this pane as a shell and does not save it (AU98)".into(),
                    });
                }
                let labels = removed_labels(agent, &p.args, &base.of(p));
                if !labels.is_empty() {
                    out.push(Violation {
                        severity: Severity::Warning,
                        path: format!("{path}.args"),
                        message: format!("removed from the saved arguments (AU30): {}", labels.join(" ")),
                    });
                }
            }
        }
    }
    out
}

pub fn run() -> i32 {
    let tokens: Vec<String> = std::env::args().skip(2).collect();
    let ctx = LoadContext { paths: Paths::from_env(), repo: std::env::current_dir().ok(), team: None };
    let store = keys::default_store(&ctx.paths);
    let verb = VerbCtx { slash: false, save: false, session_dir: SessionDir::from_env(), paths: ctx.paths.clone() };
    run_in(&tokens, &ctx, &store, &|| super::verbs::team(&verb, "config"))
}

fn run_in(tokens: &[String], ctx: &LoadContext, s: &dyn SecretStore, team: &dyn Fn() -> anyhow::Result<Option<toml::Table>>) -> i32 {
    let args = match parse(tokens) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    // `--layer` is checked before loading: a bad layer is a usage error even with a broken config.
    let layer = match &args.sub {
        Sub::Show { layer: Some(n) } => match n.parse::<Layer>() {
            Ok(l) => Some(l),
            Err(_) => {
                super::print_err(&format!("zygo config: unknown layer {} (builtin|team|user|repository|profile|flags|session)\n", zygo_core::esc(n)));
                return ExitCode::Usage as i32;
            }
        },
        _ => None,
    };
    let loaded = |c: &LoadContext| load(c).map_err(|e| {
        super::print_err(&format!("zygo config: {e}\n"));
        ExitCode::ConfigInvalid as i32
    });
    let local = match loaded(ctx) {
        Ok(m) => m,
        Err(code) => return code,
    };
    // `show` and `check` debug this config, so they work offline: a team layer that cannot be read is one line, and the
    // local config is reported with its own exit code.
    let merged = match team() {
        Ok(None) => local,
        Ok(Some(t)) => match loaded(&LoadContext { team: Some(t), ..ctx.clone() }) {
            Ok(m) => m,
            Err(code) => return code,
        },
        Err(e) => {
            let why = e.to_string();
            super::print_err(&format!("zygo config: team layer not read: {}\n", why.strip_prefix("zygo config: ").unwrap_or(&why)));
            local
        }
    };
    for w in &merged.warnings {
        super::print_err(&format!("{w}\n"));
    }
    // D6: the pane folders by `zygo up`'s own lookup, only when there is a loadout (no git run otherwise)
    let base = if merged.config.loadout.is_empty() {
        PaneBase { main_root: PathBuf::new(), worktrees: vec![] }
    } else {
        let (base, warning) = PaneBase::from_folder(ctx.repo.as_deref().unwrap_or(Path::new("")));
        if let Some(m) = warning {
            print_folder_warning(&m);
        }
        base
    };
    match args.sub {
        Sub::Show { .. } => {
            super::print_out(&render_show(&merged, layer, s, &|n| std::env::var_os(n), &mut std::io::stderr(), &base));
            0
        }
        Sub::Check => {
            let mut violations = check(&merged.config);
            violations.extend(loadout_checks(&merged.config, &base));
            if violations.is_empty() {
                super::print_out("config ok\n");
            }
            for v in &violations {
                let tag = if v.severity == Severity::Error { "error" } else { "warning" };
                super::print_out(&format!("{tag}: {}\n", zygo_core::esc(&v.to_string())));
            }
            if violations.iter().any(|v| v.severity == Severity::Error) { ExitCode::ConfigInvalid as i32 } else { 0 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zygo_config::keys::{MemStore, Secret};

    fn p(args: &[&str]) -> Result<ConfigArgs, clap::Error> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_config_args() {
        for args in [&["show", "--layer=user"][..], &["show", "--layer", "user"]] {
            assert!(matches!(p(args).unwrap().sub, Sub::Show { layer: Some(ref l) } if l == "user"));
        }
        assert!(matches!(p(&["check"]).unwrap().sub, Sub::Check));
        assert!(p(&["bogus"]).is_err());
    }

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
"#;

    fn merged_fixture() -> (tempfile::TempDir, Paths, Merged) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let config_file = r.join("config.toml");
        std::fs::write(&config_file, CFG).unwrap();
        let paths = Paths {
            config_file,
            cache_dir: r.join("cache"),
            data_dir: r.join("data"),
            sessions_dir: r.join("cache/sessions"),
            marketplaces_dir: r.join("cache/marketplaces"),
            plugin_store: r.join("data/plugins"),
            approvals_file: r.join("data/approvals.toml"),
            usage_db: r.join("data/usage.db"),
        };
        let m = load(&LoadContext { paths: paths.clone(), repo: None, team: None }).unwrap();
        (d, paths, m)
    }

    fn no_env(_: &str) -> Option<std::ffi::OsString> {
        None
    }

    #[test]
    fn key_set_state_comes_from_the_injected_store() {
        let (_d, _, m) = merged_fixture();
        let s = MemStore::new();
        let mut err = Vec::new();
        assert!(render_show(&m, None, &s, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] }).contains("\"keychain:acme\" <unset>"));
        s.set("acme", &Secret::new(format!("c-{}", ulid::Ulid::generate()))).unwrap();
        assert!(render_show(&m, None, &s, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] }).contains("\"keychain:acme\" <set>"));
        assert!(err.is_empty());
    }

    #[test]
    fn config_show_hides_source_userinfo() {
        let t = format!("tk{}", ulid::Ulid::generate());
        let (_d, paths, _) = merged_fixture();
        let extra = format!("[[marketplace]]\nname = \"a\"\nsource = \"git+https://bob:{t}@h.example/o/r.git\"\nref = \"v1.0.0\"\n[[marketplace]]\nname = \"b\"\nsource = \"https://{t}@h.example/o/r\"\nref = \"v1.0.0\"\n");
        std::fs::write(&paths.config_file, format!("{CFG}{extra}")).unwrap();
        let m = load(&LoadContext { paths, repo: None, team: None }).unwrap();
        let mut err = Vec::new();
        for layer in [None, Some(Layer::User)] {
            let out = render_show(&m, layer, &MemStore::new(), &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] });
            assert!(out.contains("marketplace.a.source = \"git+https://h.example/o/r.git\"  # user\n"), "{out}");
            assert!(out.contains("marketplace.b.source = \"https://h.example/o/r\"  # user\n"), "{out}");
            assert!(!out.contains(&t), "{out}");
        }
        assert!(err.is_empty());
    }

    #[test]
    fn show_marks_a_store_error_and_reports_it_once() {
        use zygo_config::keys::{Auto, FileStore};
        let (_d, paths, m) = merged_fixture();
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let file = paths.data_dir.join("keys.toml");
        let secret = format!("c-{}", ulid::Ulid::generate());
        std::fs::write(&file, format!("acme = {secret}\n")).unwrap();
        let a = Auto::new(None, FileStore::new(file));
        let mut err = Vec::new();
        let out = render_show(&m, None, &a, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] });
        assert!(out.contains("\"keychain:acme\" <error>"), "{out}");
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("zygo config: ") && !err.contains(&secret), "{err}");
    }

    #[test]
    fn bad_layer_exits_2_before_load() {
        let t = tempfile::Builder::new().prefix("zygo-n3-cli-").tempdir().unwrap();
        let d = t.path();
        let config_file = d.join("config.toml");
        std::fs::write(&config_file, "[results").unwrap();
        let paths = Paths {
            config_file,
            cache_dir: d.join("cache"),
            data_dir: d.join("data"),
            sessions_dir: d.join("cache/sessions"),
            marketplaces_dir: d.join("cache/marketplaces"),
            plugin_store: d.join("data/plugins"),
            approvals_file: d.join("data/approvals.toml"),
            usage_db: d.join("data/usage.db"),
        };
        let ctx = LoadContext { paths, repo: None, team: None };
        let toks = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(run_in(&toks(&["show", "--layer", "bogus"]), &ctx, &MemStore::new(), &|| unreachable!("the team is read after the layer check and the local load")), ExitCode::Usage as i32);
        assert_eq!(run_in(&toks(&["show"]), &ctx, &MemStore::new(), &|| unreachable!("the team is read after the layer check and the local load")), ExitCode::ConfigInvalid as i32);
    }


    struct Failing;
    impl SecretStore for Failing {
        fn backend(&self) -> zygo_config::keys::Backend { zygo_config::keys::Backend::File }
        fn get(&self, _: &str) -> anyhow::Result<Option<Secret>> { anyhow::bail!("boom\u{1b}[2K\nzygo config: forged") }
        fn set(&self, _: &str, _: &Secret) -> anyhow::Result<zygo_config::keys::Backend> { anyhow::bail!("unused") }
        fn delete(&self, _: &str) -> anyhow::Result<bool> { anyhow::bail!("unused") }
    }

    #[test]
    fn show_store_error_line_is_escaped() {
        let (_d, _, m) = merged_fixture();
        let mut err = Vec::new();
        let out = render_show(&m, None, &Failing, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] });
        assert!(out.contains("\"keychain:acme\" <error>"), "{out}");
        assert_eq!(String::from_utf8(err).unwrap(), "zygo config: cannot read keychain:acme: boom\\u{1b}[2K\\nzygo config: forged\n");
    }
}
