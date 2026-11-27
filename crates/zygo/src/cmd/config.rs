//! `zygo config show [--layer <name>] | check` (§6.1, §6.3, §17; workspace definition design R§10).

use clap::{Parser, Subcommand};
use zygo_config::keys::{self, resolve_with, SecretStore};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use zygo_config::layers::seg;
use zygo_config::repo_def::SystemGit;
use zygo_config::check::{check_with, RepositoryLoadout};
use zygo_config::{load, Config, KeyRef, Layer, LoadContext, Merged, Severity, Violation};
use zygo_core::exit::ExitCode;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_workspace::args::{has_user_info, removed_labels};

use super::compact::{parse_window, Change};
use super::loadout::{hide_secrets, print_folder_warning, shown_field, PaneBase};
use super::verbs::VerbCtx;
use super::workspace_files::{view, State};

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

/// What `config show` prints (R§10): every key, one layer's keys, or the keys the workspace files added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    All,
    Layer(Layer),
    WorkspaceFiles,
}

/// `None` gives `All`; `"workspace-files"` gives `WorkspaceFiles` (a source, not a `Layer`: R§8 "Not a layer"); a
/// `Layer` name gives `Layer`; any other name gives `None` (unknown).
fn which(name: Option<&str>) -> Option<Which> {
    match name {
        None => Some(Which::All),
        Some("workspace-files") => Some(Which::WorkspaceFiles),
        Some(n) => n.parse::<Layer>().ok().map(Which::Layer),
    }
}

/// The `show` text with each key as `<set>`, `<unset>` or `<error>`; one line on `err` per key the store could not read.
/// Each loadout is shown after `hide_secrets`, with each pane's folder taken against `base` (FIX-27 D7).
fn render_show(
    m: &Merged,
    which: Which,
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
    let is_set = |k: &KeyRef| match resolve_with(k, s, env) {
        Ok(v) => v.is_some(),
        Err(e) => {
            failed.borrow_mut().insert(k.to_string(), e.to_string());
            false
        }
    };
    let mut text = match which {
        Which::All => m.show(None, &is_set),
        Which::Layer(l) => m.show(Some(l), &is_set),
        Which::WorkspaceFiles => m.show_workspace_files(),
    };
    // `show` prints a failed key as `"<ref>" <unset>`; it is marked `<error>` here, on its own key line only.
    let failed = failed.into_inner();
    if !failed.is_empty() {
        text = text.split_inclusive('\n').map(|l| mark_error(l, &failed)).collect();
    }
    for (r, e) in failed {
        let _ = writeln!(err, "zygo config: cannot read {}: {}", zygo_core::esc(&r), zygo_core::esc(&e));
    }
    text
}

/// `line` of `Merged::show` (`<key> = <value>  # <source>\n`) with `<unset>` made `<error>` when it is the line of a
/// key reference (`model.<id>.provider.<name>.key`, `server.token`) whose value is exactly `"<ref>" <unset>` for a ref
/// in `failed`; any other line is returned unchanged (WD-16 review F3). A repository value that holds the same text,
/// or text that looks like a source label inside its TOML quotes, is never a key-reference line: the repository
/// files add `profile.*` and `loadout.repository` keys only.
fn mark_error(line: &str, failed: &BTreeMap<String, String>) -> String {
    let Some((key, rest)) = line.split_once(" = ") else { return line.to_string() };
    let key_ref = key == "server.token" || (key.starts_with("model.") && key.contains(".provider.") && key.ends_with(".key"));
    if key_ref {
        for r in failed.keys() {
            let unset = zygo_core::esc(&format!("\"{r}\" <unset>"));
            if let Some(source) = rest.strip_prefix(unset.as_str()).and_then(|s| s.strip_prefix("  # ")) {
                return format!("{key} = {}  # {source}", zygo_core::esc(&format!("\"{r}\" <error>")));
            }
        }
    }
    line.to_string()
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
                    // review round 2, finding 3: the value as `config show` prints it
                    let full = super::bad_compact(word, &shown_field(v));
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

/// What the R§10 read gave for the `repository` loadout of the workspace files, as `zygo up` uses it (`up.rs`
/// `choose`; FX-29): approved files that hold `.zygo/workspace.toml` add it; with no read (no checkout, a failed read,
/// a slash call) the files are not known; every other state adds none.
fn repository_of(state: &State) -> RepositoryLoadout {
    match state {
        State::Approved { workspace: true } => RepositoryLoadout::Added,
        State::Outside | State::Unread | State::Slash => RepositoryLoadout::NotRead,
        State::Absent | State::Invalid { .. } | State::NotApproved { .. } | State::Approved { workspace: false } => {
            RepositoryLoadout::NotAdded
        }
    }
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
    let shown = match &args.sub {
        Sub::Show { layer } => match which(layer.as_deref()) {
            Some(w) => w,
            None => {
                super::print_err(&format!(
                    "zygo config: unknown layer {} (builtin|team|user|repository|profile|flags|session|workspace-files)\n",
                    zygo_core::esc(layer.as_deref().unwrap_or_default())
                ));
                return ExitCode::Usage as i32;
            }
        },
        Sub::Check => Which::All,
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
    // R§10 (WD-16 plan call 4): `show --layer <Layer>` reads no workspace files; `show`, `show --layer workspace-files`
    // and `check` read the default branch of the checkout of the current folder. Only an approved read adds keys (R-C2).
    let folder = match (&args.sub, shown) {
        (Sub::Show { .. }, Which::Layer(_)) => None,
        _ => ctx.repo.as_deref(),
    };
    let v = view(&merged, folder, &ctx.paths.approvals_file, &ctx.paths.workspace_cache_dir(), &SystemGit);
    // D6: the pane folders by `zygo up`'s own lookup, only when there is a loadout (no git run otherwise) and the output
    // can show one: only the user layer holds `[loadout]` (the team layer drops it, `layers.rs:86`; the repository file
    // may not set it, `layers.rs:499`), so `show --layer <other>` needs no lookup (review round 2, finding 6)
    // `--layer workspace-files` shows `loadout.repository` when the files add it.
    let (shows_loadouts, cfg) = match &args.sub {
        Sub::Show { .. } => (matches!(shown, Which::All | Which::Layer(Layer::User) | Which::WorkspaceFiles), &v.merged.config),
        Sub::Check => (true, &merged.config),
    };
    let base = if cfg.loadout.is_empty() || !shows_loadouts {
        PaneBase { main_root: PathBuf::new(), worktrees: vec![] }
    } else {
        let (base, warning) = PaneBase::from_folder(ctx.repo.as_deref().unwrap_or(Path::new("")), &ctx.paths.workspace_cache_dir());
        if let Some(m) = warning {
            print_folder_warning(&m);
        }
        base
    };
    match args.sub {
        Sub::Show { .. } => {
            if let Some(line) = &v.line {
                super::print_err(&format!("{line}\n"));
            }
            super::print_out(&render_show(&v.merged, shown, s, &|n| std::env::var_os(n), &mut std::io::stderr(), &base));
            0
        }
        Sub::Check => {
            let mut violations = check_with(&merged.config, repository_of(&v.state));
            violations.extend(loadout_checks(&merged.config, &base));
            // R§10 `config check`: an invalid definition is one warning, so the exit status stays that of the config;
            // an unread one is its line on stderr; the other states print nothing here.
            match &v.state {
                State::Invalid { why, .. } => violations.push(Violation {
                    severity: Severity::Warning,
                    path: "workspace files".into(),
                    message: why.clone(),
                }),
                State::Unread => {
                    if let Some(line) = &v.line {
                        super::print_err(&format!("{line}\n"));
                    }
                }
                _ => {}
            }
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
    fn which_parses_the_workspace_files_source() {
        assert_eq!(which(None), Some(Which::All));
        assert_eq!(which(Some("workspace-files")), Some(Which::WorkspaceFiles));
        assert_eq!(which(Some("user")), Some(Which::Layer(Layer::User)));
        assert_eq!(which(Some("bogus")), None);
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
        assert!(render_show(&m, Which::All, &s, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] }).contains("\"keychain:acme\" <unset>"));
        s.set("acme", &Secret::new(format!("c-{}", ulid::Ulid::generate()))).unwrap();
        assert!(render_show(&m, Which::All, &s, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] }).contains("\"keychain:acme\" <set>"));
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
        for layer in [Which::All, Which::Layer(Layer::User)] {
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
        let out = render_show(&m, Which::All, &a, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] });
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
        let out = render_show(&m, Which::All, &Failing, &no_env, &mut err, &PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] });
        assert!(out.contains("\"keychain:acme\" <error>"), "{out}");
        assert_eq!(String::from_utf8(err).unwrap(), "zygo config: cannot read keychain:acme: boom\\u{1b}[2K\\nzygo config: forged\n");
    }

    /// WD-16 review F3: only the key line of a failed key is marked `<error>`; a repository value that holds the same
    /// text, or text that looks like a source label inside its TOML quotes, is shown unchanged.
    #[test]
    fn show_marks_only_the_key_line_of_a_failed_key() {
        use std::collections::BTreeMap;
        use zygo_config::repo_def::{parse, Files};
        let (_d, _, m) = merged_fixture();
        let profile = "overrides = { review = '\"keychain:acme\" <unset>', plan = '\"keychain:acme\" <unset>  # user' }\n";
        let files = Files {
            full_ref: Some("refs/remotes/origin/main".into()),
            commit: "0".repeat(40),
            workspace: None,
            profiles: BTreeMap::from([("rust".to_string(), profile.as_bytes().to_vec())]),
            ignored: vec![],
        };
        let m = m.with_definition(&parse(&files).unwrap());
        let mut err = Vec::new();
        let base = PaneBase { main_root: _d.path().to_path_buf(), worktrees: vec![] };
        for which in [Which::All, Which::WorkspaceFiles] {
            let out = render_show(&m, which, &Failing, &no_env, &mut err, &base);
            let file = "  # workspace-files .zygo/profiles/rust.toml";
            let review = format!("profile.rust.overrides.review = '\"keychain:acme\" <unset>'{file}");
            let plan = format!("profile.rust.overrides.plan = '\"keychain:acme\" <unset>  # user'{file}");
            let shown: Vec<&str> = out.lines().filter(|l| l.starts_with("profile.") || l.contains("keychain:acme")).collect();
            assert!(out.lines().any(|l| l == review), "{which:?}: {shown:#?}");
            assert!(out.lines().any(|l| l == plan), "{which:?}: {shown:#?}");
            if which == Which::All {
                assert!(out.contains("\"keychain:acme\" <error>  # user\n"), "{shown:#?}");
            }
        }
    }
}
