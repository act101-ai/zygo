use std::collections::BTreeSet;
use std::fmt;

use zygo_core::ids::{reserved_member_name, valid_member_name};

use crate::layers::{join, seg};
use crate::schema::{parse_duration, BUILTIN_LOADOUT, Config, KeyRef, LOADOUT_KINDS, Reasoning, STATUSLINE_STYLES};

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

/// `zygo config check` (§6.3, §7.5, §7.6, §16; prices: H§6.1 AU32).
pub fn check(cfg: &Config) -> Vec<Violation> {
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
    for (key, text) in [("keep", &cfg.mail.keep), ("wake_gap", &cfg.mail.wake_gap)] {
        if parse_duration(text).is_none() {
            e(format!("mail.{key}"), "must be <n>s, <n>m, <n>h or <n>d".into());
        }
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
    v.extend(loadout_violations(cfg));
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
fn loadout_violations(cfg: &Config) -> Vec<(Severity, String, String)> {
    const CLAUDE_ONLY: &str = "only with kind = \"claude-code\"";
    let mut v = Vec::new();
    let default = cfg.workspace.default_loadout.as_str();
    if default != BUILTIN_LOADOUT && !cfg.loadout.contains_key(default) {
        v.push((Severity::Error, "workspace.default_loadout".into(), "unknown loadout".into()));
    }
    for (name, l) in &cfg.loadout {
        let lp = join(&["loadout", name.as_str()]);
        if l.window.is_empty() {
            v.push((Severity::Error, lp.clone(), "must have at least one window".into()));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (wi, w) in l.window.iter().enumerate() {
            let wp = format!("{lp}.window[{wi}]");
            if w.pane.is_empty() {
                v.push((Severity::Error, wp.clone(), "must have at least one pane".into()));
            }
            for (pi, p) in w.pane.iter().enumerate() {
                let pp = format!("{wp}.pane[{pi}]");
                let claude = p.kind == "claude-code";
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
                if p.profile.as_ref().is_some_and(|name| !cfg.profile.contains_key(name)) {
                    v.push((Severity::Warning, format!("{pp}.profile"), "unknown profile".into()));
                }
                // M§6; the message never echoes the name (it is user text)
                if let Some(n) = &p.name {
                    // a shell pane has no member record (W AU53): its name has no effect
                    let msg = if p.kind == "shell" {
                        Some("only with kind = \"claude-code\" or \"opencode\"")
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
