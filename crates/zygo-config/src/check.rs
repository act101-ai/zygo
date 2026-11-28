use std::collections::BTreeSet;
use std::fmt;

use zygo_core::ids::{reserved_member_name, valid_member_name};

use crate::layers::{join, seg};
use crate::profile::valid_name;
use crate::schema::{parse_duration, parse_sync_gap, valid_mail_host, valid_sync_branch, BUILTIN_LOADOUT, Config, KeyRef, JOBS, LOADOUT_KINDS, Reasoning, REPOSITORY_LOADOUT, STATUSLINE_STYLES};

const MAIL_HOST_FORM: &str = "must match ^[a-z0-9][a-z0-9-]{0,31}$";
const MAIL_SYNC_FORM: &str = "must match ^zygo/[a-z0-9][a-z0-9-]{0,31}$";
const MAIL_WORKSPACE_NAME_FORM: &str = "must match ^[a-z0-9-]{1,32}$";
const MAIL_SYNC_GAP_FORM: &str = "must be <n>s, <n>m or <n>h";
/// Mail sync design S§7.1.
const MAIL_NO_HOST: &str = "set [mail] host to turn on mail sync; zygo does not publish the system host name";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub severity: Severity,
    pub path: String,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

const BAD_RATE: &str = "must be a finite number >= 0";

/// The keys of one price tier whose rate `config check` refuses: negative, NaN or infinite (AU32).
fn bad_rates(input: f64, output: f64, cache_read: f64, cache_write: f64, cache_write_5m: Option<f64>) -> impl Iterator<Item = &'static str> {
    let rates = [("input", Some(input)), ("output", Some(output)), ("cache_read", Some(cache_read)), ("cache_write", Some(cache_write)), ("cache_write_5m", cache_write_5m)];
    rates.into_iter().filter(|(_, r)| r.is_some_and(|r| !r.is_finite() || r < 0.0)).map(|(k, _)| k)
}

/// What the R§10 read of the workspace files gave for their `repository` loadout (workspace definition design R§10
/// `zygo config check` row; node FX-29). It decides `[workspace] default_loadout = "repository"` when the user config
/// has no `[loadout.repository]`, as `zygo up` decides it (`zygo-workspace` `up.rs` `choose`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryLoadout {
    /// The read found approved files that hold `.zygo/workspace.toml`: `zygo up` plans their loadout.
    Added,
    /// The read was made and adds no loadout (absent, not approved, invalid, or profiles only): `zygo up` refuses.
    NotAdded,
    /// No read: no checkout at the folder, or the read failed. The files that `zygo up` will find are not known.
    NotRead,
}

/// The warning for `default_loadout = "repository"` when no workspace files were read (FX-29 author's call).
const REPOSITORY_NOT_READ: &str =
    "no workspace files were read here; zygo up uses \"repository\" only from approved files that hold .zygo/workspace.toml";

/// `zygo config check` (§6.3, §7.5, §7.6, §16; prices: H§6.1 AU32), for a caller that makes no read of the workspace
/// files: [`check_with`] and [`RepositoryLoadout::NotAdded`].
pub fn check(cfg: &Config) -> Vec<Violation> {
    check_with(cfg, RepositoryLoadout::NotAdded)
}

/// `zygo config check` with what the read of the workspace files gave for their `repository` loadout (R§10).
pub fn check_with(cfg: &Config, repository: RepositoryLoadout) -> Vec<Violation> {
    let mut v = Vec::new();
    let mut e = |path: String, message: String| v.push((Severity::Error, path, message));
    for (mi, m) in cfg.catalog.0.iter().enumerate() {
        let mp = format!("model[{mi}]");
        if m.id.is_empty() {
            e(mp.clone(), "id must not be empty".into());
        }
        if m.provider.is_empty() && !m.from_team {
            e(mp.clone(), "has no provider".into());
        }
        for (pi, p) in m.provider.iter().enumerate() {
            let pp = format!("{mp}.provider[{pi}]");
            if p.name.is_empty() {
                e(pp.clone(), "name must not be empty".into());
            }
            if p.base_url.is_empty() {
                e(format!("{pp}.base_url"), "must not be empty".into());
            }
            if p.upstream_model.is_empty() {
                e(format!("{pp}.upstream_model"), "must not be empty".into());
            }
            if p.key.as_ref().is_some_and(KeyRef::name_is_empty) {
                e(format!("{pp}.key"), "name must not be empty".into());
            }
            if let Some(pr) = &p.price_usd_per_mtok {
                let rp = format!("{pp}.price_usd_per_mtok");
                for k in bad_rates(pr.input, pr.output, pr.cache_read, pr.cache_write, pr.cache_write_5m) {
                    e(format!("{rp}.{k}"), BAD_RATE.into());
                }
                if let Some(l) = &pr.long_context {
                    let lp = format!("{rp}.long_context");
                    if l.above == 0 {
                        e(format!("{lp}.above"), "must be > 0".into());
                    }
                    for k in bad_rates(l.input, l.output, l.cache_read, l.cache_write, l.cache_write_5m) {
                        e(format!("{lp}.{k}"), BAD_RATE.into());
                    }
                }
            }
        }
        if let Some(soft) = m.soft_context_window {
            let sp = format!("{mp}.soft_context_window");
            match &m.facts {
                _ if soft == 0 => e(sp, "must be > 0".into()),
                None => e(sp, "needs [model.facts]".into()),
                Some(f) if soft > f.context_window => e(sp, "exceeds facts.context_window".into()),
                Some(_) => {}
            }
        }
        if let Some(f) = &m.facts {
            let lp = format!("{mp}.facts.effort_levels");
            if f.effort_levels.is_empty() && f.reasoning != Reasoning::None {
                e(lp, "empty only with reasoning = \"none\"".into());
            } else if f.effort_levels.windows(2).any(|w| w[0] >= w[1]) {
                e(lp, "must follow the ladder order without repeats".into());
            }
            if f.reasoning_default_on.is_some() && f.reasoning != Reasoning::Switchable {
                e(format!("{mp}.facts.reasoning_default_on"), "only with reasoning = \"switchable\"".into());
            }
        }
    }
    if cfg.server.token.as_ref().is_some_and(KeyRef::name_is_empty) {
        e("server.token".into(), "name must not be empty".into());
    }
    for (name, r) in &cfg.role {
        if cfg.catalog.get(&r.model).is_none() {
            e(format!("{}.model", join(&["role", name.as_str()])), "unknown model".into());
        }
    }
    if !STATUSLINE_STYLES.contains(&cfg.statusline.style.as_str()) {
        let [compact, verbose] = STATUSLINE_STYLES;
        e("statusline.style".into(), format!("must be \"{compact}\" or \"{verbose}\""));
    }
    if !(1..=32).contains(&cfg.limits.max_members) {
        e("limits.max_members".into(), "must be 1-32".into());
    }
    for (key, text) in [("keep", &cfg.mail.keep), ("wake_gap", &cfg.mail.wake_gap)] {
        if parse_duration(text).is_none() {
            e(format!("mail.{key}"), "must be <n>s, <n>m, <n>h or <n>d".into());
        }
    }
    if cfg.mail.host.as_deref().is_some_and(|h| !valid_mail_host(h)) {
        e("mail.host".into(), MAIL_HOST_FORM.into());
    }
    if parse_sync_gap(&cfg.mail.sync_gap).is_none() {
        e("mail.sync_gap".into(), MAIL_SYNC_GAP_FORM.into());
    }
    let mut sync_without_host = false;
    for (name, ws) in &cfg.mail.workspace {
        let path = join(&["mail", "workspace", name.as_str()]);
        // The table key is a workspace name (workspaces design W§5); zygo-workspace depends on this crate, so the form
        // comes from `profile::valid_name`, the same `^[a-z0-9-]{1,32}$`.
        if !valid_name(name) {
            e(path.clone(), MAIL_WORKSPACE_NAME_FORM.into());
        }
        if ws.sync.as_deref().is_some_and(|b| !valid_sync_branch(b)) {
            e(format!("{path}.sync"), MAIL_SYNC_FORM.into());
        }
        if ws.host.as_deref().is_some_and(|h| !valid_mail_host(h)) {
            e(format!("{path}.host"), MAIL_HOST_FORM.into());
        }
        sync_without_host |= ws.sync.is_some() && ws.host.is_none();
    }
    if cfg.mail.host.is_none() && sync_without_host {
        e("mail.host".into(), MAIL_NO_HOST.into());
    }
    let code_ok = regex::Regex::new("^[a-z0-9_]{1,64}$").expect("static pattern");
    for (i, r) in cfg.schema_rule.iter().enumerate() {
        let rp = format!("schema_rule[{i}]");
        if !code_ok.is_match(&r.code) {
            e(format!("{rp}.code"), "must be 1-64 of [a-z0-9_]".into());
        }
        if r.limit == Some(0) {
            e(format!("{rp}.limit"), "must be > 0".into());
        }
    }
    v.extend(loadout_violations(cfg, repository));
    for (mi, m) in cfg.catalog.0.iter().enumerate() {
        for (pi, p) in m.provider.iter().enumerate() {
            for k in p.flags.other.keys() {
                let pp = format!("model[{mi}].provider[{pi}].flags.{}", seg(k));
                v.push((Severity::Warning, pp, "unknown flag".into()));
            }
        }
    }
    for (i, p) in cfg.redaction.pattern.iter().enumerate() {
        if let Err(err) = regex::Regex::new(&p.pattern) {
            let path = format!("redaction.pattern[{i}].pattern");
            v.push((Severity::Error, path, format!("does not compile: {}", regex_error(&p.pattern, &err))));
        }
    }
    let c = &cfg.redaction.content;
    if !(1..=64).contains(&c.replacement.len()) {
        v.push((Severity::Error, "redaction.content.replacement".into(), "must be 1-64 bytes".into()));
    }
    let word_ok = regex::Regex::new("^[a-z]{3,40}$").expect("static pattern");
    for (list, words) in [("allow", &c.allow), ("deny", &c.deny)] {
        for (i, _) in words.iter().enumerate().filter(|(_, w)| !word_ok.is_match(w)) {
            let path = format!("redaction.content.{list}[{i}]");
            v.push((Severity::Error, path, "must be 3-40 lower-case letters".into()));
        }
    }
    for (i, _) in c.allow.iter().enumerate().filter(|(_, w)| c.deny.contains(w)) {
        v.push((Severity::Error, format!("redaction.content.allow[{i}]"), "is also in redaction.content.deny".into()));
    }
    v.into_iter().map(|(severity, path, message)| Violation { severity, path, message }).collect()
}

/// The structural rules of `[workspace]` and `[loadout.<name>]` (workspaces design W§11). The rules that need the
/// launch-argument filter or the compact-window parser are not here: this crate depends on neither (AU103).
/// `job` only on an agent pane (`claude-code`, `opencode`) and a refused `job` takes no orchestrator slot (R§9.1).
/// `default_loadout = "repository"` with no user table follows `repository` (R§10 `zygo config check` row, FX-29).
fn loadout_violations(cfg: &Config, repository: RepositoryLoadout) -> Vec<(Severity, String, String)> {
    const CLAUDE_ONLY: &str = "only with kind = \"claude-code\"";
    const AGENT_ONLY: &str = "only with kind = \"claude-code\" or \"opencode\"";
    let mut v = Vec::new();
    let default = cfg.workspace.default_loadout.as_str();
    if default != BUILTIN_LOADOUT && !cfg.loadout.contains_key(default) {
        let path = "workspace.default_loadout".to_string();
        match repository {
            RepositoryLoadout::Added if default == REPOSITORY_LOADOUT => {}
            RepositoryLoadout::NotRead if default == REPOSITORY_LOADOUT => {
                v.push((Severity::Warning, path, REPOSITORY_NOT_READ.into()));
            }
            _ => v.push((Severity::Error, path, "unknown loadout".into())),
        }
    }
    if cfg.workspace.pool.as_deref().is_some_and(|p| !absolute_after_tilde(p)) {
        v.push((Severity::Error, "workspace.pool".into(), "must be absolute after ~ expansion".into()));
    }
    for (name, l) in &cfg.loadout {
        let lp = join(&["loadout", name.as_str()]);
        if l.window.is_empty() {
            v.push((Severity::Error, lp.clone(), "must have at least one window".into()));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut orchestrator = false;
        for (wi, w) in l.window.iter().enumerate() {
            let wp = format!("{lp}.window[{wi}]");
            if w.pane.is_empty() {
                v.push((Severity::Error, wp.clone(), "must have at least one pane".into()));
            }
            for (pi, p) in w.pane.iter().enumerate() {
                let pp = format!("{wp}.pane[{pi}]");
                let claude = p.kind == "claude-code";
                let agent = claude || p.kind == "opencode";
                if !LOADOUT_KINDS.contains(&p.kind.as_str()) {
                    let [claude_code, opencode, shell] = LOADOUT_KINDS;
                    v.push((Severity::Error, format!("{pp}.kind"), format!("must be \"{claude_code}\", \"{opencode}\" or \"{shell}\"")));
                }
                if p.passthrough && !claude {
                    v.push((Severity::Error, format!("{pp}.passthrough"), CLAUDE_ONLY.into()));
                }
                if p.config_dir.is_some() && !claude {
                    v.push((Severity::Error, format!("{pp}.config_dir"), CLAUDE_ONLY.into()));
                }
                if let Some(job) = p.job.as_deref() {
                    if !agent {
                        v.push((Severity::Error, format!("{pp}.job"), AGENT_ONLY.into()));
                    } else if !JOBS.contains(&job) {
                        let [orch, worker] = JOBS;
                        v.push((Severity::Error, format!("{pp}.job"), format!("must be \"{orch}\" or \"{worker}\"")));
                    } else if job == "orchestrator" {
                        if orchestrator {
                            v.push((Severity::Error, format!("{pp}.job"), "only one orchestrator per loadout".into()));
                        }
                        orchestrator = true;
                    }
                }
                if p.pool && !claude {
                    v.push((Severity::Error, format!("{pp}.pool"), CLAUDE_ONLY.into()));
                }
                if p.pool && p.config_dir.is_some() {
                    v.push((Severity::Error, format!("{pp}.pool"), "not with config_dir".into()));
                }
                if p.profile.as_ref().is_some_and(|name| !cfg.profile.contains_key(name)) {
                    v.push((Severity::Warning, format!("{pp}.profile"), "unknown profile".into()));
                }
                // M§6; the message never echoes the name (it is user text)
                if let Some(n) = &p.name {
                    // a shell pane has no member record (W AU53): its name has no effect
                    let msg = if p.kind == "shell" {
                        Some(AGENT_ONLY)
                    } else if reserved_member_name(n) {
                        Some("is reserved")
                    } else if !valid_member_name(n) {
                        Some("must be 1-32 of [a-z0-9-], starting with a letter")
                    } else if !seen.insert(n) {
                        Some("is used by another pane in the loadout")
                    } else {
                        None
                    };
                    if let Some(msg) = msg {
                        v.push((Severity::Error, format!("{pp}.name"), msg.into()));
                    }
                }
            }
        }
    }
    v
}

/// `[workspace] pool` is absolute once `~` and `~/` expand to the home directory, as `--config-dir` expands them (R§9.5).
/// It reads the text only: no home lookup, no file access.
fn absolute_after_tilde(p: &str) -> bool {
    p == "~" || p.starts_with("~/") || std::path::Path::new(p).is_absolute()
}

/// Why `pattern` does not compile, from the parser's error kind and position; `regex::Error`'s text quotes the
/// pattern, which can hold a literal secret (C3).
pub(crate) fn regex_error(pattern: &str, e: &regex::Error) -> String {
    if let regex::Error::CompiledTooBig(limit) = e {
        return format!("compiled size exceeds the limit of {limit} bytes");
    }
    let (kind, at) = match regex_syntax::Parser::new().parse(pattern) {
        Err(regex_syntax::Error::Parse(p)) => (p.kind().to_string(), p.span().start),
        Err(regex_syntax::Error::Translate(t)) => (t.kind().to_string(), t.span().start),
        _ => return "syntax error".into(),
    };
    format!("{kind} at pattern line {}, column {}", at.line, at.column)
}
