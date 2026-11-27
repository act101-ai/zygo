use std::collections::BTreeSet;

use anyhow::{bail, Context};
use zygo_config::profile::{set_default_profile, valid_name};
use zygo_config::layers::seg;
use zygo_config::Merged;

use super::verbs::VerbCtx;

fn loaded(ctx: &VerbCtx) -> anyhow::Result<Merged> {
    super::verbs::base(ctx, "profile", super::verbs::team(ctx, "profile")?, None)
}

/// The config of `list` and `show` (R§10): `loaded`, then, in the CLI form only, the approved workspace files of the
/// current folder's repository (`workspace_files::here`), with its not-approved, invalid or unread line on stderr.
/// The slash form runs in the hook process and reads no workspace files (R-C8, R§18 item 20): config profiles only.
fn listed(ctx: &VerbCtx) -> anyhow::Result<Merged> {
    let m = loaded(ctx)?;
    if ctx.slash {
        return Ok(m);
    }
    let v = super::workspace_files::here(&m, &ctx.paths.approvals_file);
    if let Some(line) = &v.line {
        super::print_err(&format!("{line}\n"));
    }
    Ok(v.merged)
}

/// The source text of profile `n`: `workspace-files` for a profile the workspace files added, else its layer.
fn source(m: &Merged, n: &str) -> String {
    let key = format!("profile.{}", seg(n));
    match m.definition_file_of(&key) {
        Some(_) => "workspace-files".to_string(),
        None => m.source_of(&key).name().to_string(),
    }
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => {
            let m = loaded(ctx)?;
            Ok(format!("default profile: {}", zygo_core::esc(m.config.user.default_profile.as_deref().unwrap_or("none"))))
        }
        ["list"] => {
            let m = listed(ctx)?;
            let cur = m.config.user.default_profile.as_deref();
            let shadowed = m.shadowed_profiles();
            // R§18 item 9: a repository profile that a user profile or a team preset shadows is the row after the row
            // of that name, with no `*` (it does not apply).
            let names: BTreeSet<&str> =
                m.config.profile.keys().map(String::as_str).chain(shadowed.iter().map(String::as_str)).collect();
            if names.is_empty() {
                return Ok("no profiles".into());
            }
            let mut rows = Vec::new();
            for n in names {
                if m.config.profile.contains_key(n) {
                    let mark = if Some(n) == cur { "*" } else { " " };
                    rows.push(format!("{mark} {} ({})", zygo_core::esc(n), source(&m, n)));
                }
                if shadowed.iter().any(|s| s == n) {
                    rows.push(format!("  {} (workspace-files, shadowed)", zygo_core::esc(n)));
                }
            }
            Ok(rows.join("\n"))
        }
        ["show", n] => {
            // The profile that applies: a shadowed repository profile is not shown.
            let m = listed(ctx)?;
            let Some(p) = m.config.profile.get(*n) else { bail!("profile {}: not found", zygo_core::esc(n)) };
            let body = toml::to_string(p).context("encoding profile")?;
            // Each line escaped; the line breaks stay (§11.3).
            let body: String = body.lines().map(|l| format!("{}\n", zygo_core::esc(l))).collect();
            Ok(format!("profile {} ({})\n{body}", zygo_core::esc(n), source(&m, n)))
        }
        // `help` is not a profile name: the verb's help, as `help profile` (spec §11 note, N42 review).
        ["help"] => super::help::verb(ctx, &["profile".into()]),
        ["use", n] => use_profile(ctx, n),
        [n] if !matches!(*n, "list" | "show" | "use") => use_profile(ctx, n),
        _ => bail!("usage: zygo profile [list | show <name> | use <name> | <name> | none]"),
    }
}

fn use_profile(ctx: &VerbCtx, n: &str) -> anyhow::Result<String> {
    if n != "none" && !valid_name(n) {
        bail!("invalid profile name");
    }
    let m = loaded(ctx)?;
    let defined = n == "none" || m.config.profile.contains_key(n);
    set_default_profile(&ctx.paths.config_file, n)?;
    let mut out = format!("default profile: {n} (applies at next launch)");
    if !defined {
        out.push_str(&format!("\nzygo profile: {n} is not defined in your config (it may be a team preset)"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use zygo_core::paths::Paths;

    fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::Builder::new().prefix("zygo-n6-verb-").tempdir().unwrap();
        let f = d.path().join("config.toml");
        std::fs::write(&f, text).unwrap();
        (d, f)
    }

    fn ctx(file: &Path) -> VerbCtx {
        let r = file.parent().unwrap();
        VerbCtx {
            slash: false,
            save: false,
            session_dir: None,
            paths: Paths {
                config_file: file.to_path_buf(),
                cache_dir: r.join("cache"),
                data_dir: r.join("data"),
                sessions_dir: r.join("cache/sessions"),
                marketplaces_dir: r.join("cache/marketplaces"),
                plugin_store: r.join("data/plugins"),
                approvals_file: r.join("data/approvals.toml"),
                usage_db: r.join("data/usage.db"),
            },
        }
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn run(cfg: &str, a: &[&str]) -> String {
        verb(&ctx(&write(cfg).1), &args(a)).unwrap()
    }

    fn run_err(cfg: &str, a: &[&str]) -> String {
        verb(&ctx(&write(cfg).1), &args(a)).unwrap_err().to_string()
    }

    fn default_of(file: &PathBuf) -> String {
        let t: toml::Table = std::fs::read_to_string(file).unwrap().parse().unwrap();
        t["user"]["default_profile"].as_str().unwrap().to_string()
    }

    const CFG: &str = "[user]\ndefault_profile = \"a\"\n[profile.a]\nprompt = \"A\"\n[profile.b]\nprompt = \"B\"\n";
    #[test] fn list_marks_default_and_layer() { assert_eq!(run(CFG, &["list"]), "* a (user)\n  b (user)"); }
    #[test] fn show_prints_toml_and_layer() {
        let o = run(CFG, &["show", "b"]); assert!(o.starts_with("profile b (user)\n") && o.contains("prompt = \"B\""));
        assert!(run_err(CFG, &["show", "zz"]).contains("profile zz: not found"));
    }
    #[test] fn use_and_bare_name_write_default_and_keep_other_content() {
        let (_d, f) = write(CFG);
        assert_eq!(verb(&ctx(&f), &["use".into(), "b".into()]).unwrap(), "default profile: b (applies at next launch)");
        assert_eq!(default_of(&f), "b");
        verb(&ctx(&f), &["a".into()]).unwrap(); assert_eq!(default_of(&f), "a");
        verb(&ctx(&f), &["none".into()]).unwrap(); assert_eq!(default_of(&f), "none");
        assert!(std::fs::read_to_string(&f).unwrap().contains("[profile.b]"));
    }
    #[test] fn undefined_name_is_written_with_one_note() {
        let (_d, f) = write(CFG);
        assert_eq!(verb(&ctx(&f), &["use".into(), "team-x".into()]).unwrap(),
            "default profile: team-x (applies at next launch)\nzygo profile: team-x is not defined in your config (it may be a team preset)");
        assert_eq!(default_of(&f), "team-x");
    }
    #[test] fn invalid_name_is_an_error_and_writes_nothing() {
        let (_d, f) = write(CFG);
        assert!(verb(&ctx(&f), &["use".into(), "Bad_Name".into()]).unwrap_err().to_string().contains("invalid profile name"));
        assert_eq!(default_of(&f), "a");
    }
    #[test] fn verb_words_win_and_use_sets_a_profile_with_such_a_name() {
        let (_d, f) = write("[profile.list]\nprompt = \"L\"\n");
        assert_eq!(verb(&ctx(&f), &["list".into()]).unwrap(), "  list (user)");   // the verb, not `use list`
        verb(&ctx(&f), &["use".into(), "list".into()]).unwrap(); assert_eq!(default_of(&f), "list");
    }
    #[test] fn list_without_profiles_says_so() { assert_eq!(run("[user]\n", &["list"]), "no profiles"); }
    #[test] fn use_refuses_when_config_does_not_load_and_writes_nothing() {
        let bad = "[user]\ndefault_profile = \"a\"\n[profile.a]\nprompt = 1\n";
        let (_d, f) = write(bad);
        assert!(verb(&ctx(&f), &["use".into(), "b".into()]).unwrap_err().to_string().starts_with("zygo profile: "));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), bad);
    }
    #[test] fn no_argument_prints_current_default() {
        assert_eq!(run(CFG, &[]), "default profile: a");
        assert_eq!(run("[profile.a]\nprompt = \"A\"\n", &[]), "default profile: none");
    }

    const ESC_CFG: &str = "[user]\ndefault_profile = \"x\\u001b[2K\"\n[profile.\"a.b\"]\nprompt = \"B\"\n[profile.\"x\\u001b[2K\\n* forged\"]\nprompt = \"A\"\n";
    #[test] fn list_escapes_names_and_finds_the_layer_of_a_quoted_name() {
        assert_eq!(run(ESC_CFG, &["list"]), "  a.b (user)\n  x\\u{1b}[2K\\n* forged (user)");
    }
    #[test] fn show_and_default_escape_config_text() {
        assert!(run(ESC_CFG, &["show", "x\u{1b}[2K\n* forged"]).starts_with("profile x\\u{1b}[2K\\n* forged (user)\n"));
        assert_eq!(run(ESC_CFG, &["show", "a.b"]).lines().next(), Some("profile a.b (user)"));
        assert_eq!(run(ESC_CFG, &[]), "default profile: x\\u{1b}[2K");
        assert_eq!(run_err(ESC_CFG, &["show", "z\u{1b}"]), "profile z\\u{1b}: not found");
    }
    /// Round 2 F1: the TOML body is escaped line by line; its line breaks stay (§11.3).
    #[test] fn show_escapes_the_toml_body() {
        let cfg = "[profile.a]\nprompt = \"x\\u202e\"\n[profile.a.plugins]\n\"m:p\\u202e\\u001b[2K\" = true\n";
        let o = run(cfg, &["show", "a"]);
        assert!(!o.contains('\u{202e}') && !o.contains('\u{1b}'), "{o:?}");
        assert!(o.lines().any(|l| l == "prompt = \"x\\u{202e}\""), "{o:?}");
        assert!(o.contains("\"m:p\\u{202e}\\u001B[2K\" = true"), "{o:?}");
        assert!(o.starts_with("profile a (user)\n") && o.lines().count() > 3, "{o:?}");
    }
}
