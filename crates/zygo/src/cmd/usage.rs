use std::fmt::Write;

use anyhow::anyhow;
use zygo_hooks::statusline::pct;
use zygo_plugins::check::esc;
use zygo_usage::{Filter, Store, Summary};
use zygo_work::report::parse_since;

use zygo_config::keys::default_store;
use zygo_launch::serve_client::{from_config, sync_pending, Unreachable};

use super::verbs::{base, VerbCtx};
use super::UsageError;

const USAGE: &str = "zygo usage: usage: usage [--session <id> | --since <date>] [--sync]";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub session: Option<String>,
    pub since: Option<String>,
    pub sync: bool,
}

pub fn parse(args: &[String]) -> Result<Args, UsageError> {
    let bad = || UsageError(USAGE.into());
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        match flag {
            "--sync" if inline.is_none() => out.sync = true,
            "--session" | "--since" => {
                let value = inline.or_else(|| it.next().cloned()).ok_or_else(bad)?;
                let slot = if flag == "--session" { &mut out.session } else { &mut out.since };
                if slot.replace(value).is_some() {
                    return Err(bad());
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(out)
}

/// `zygo usage --sync` (§14, B2, R5): the pending serve sessions' rows from the server; exit 12 when any session was
/// unreachable.
fn sync(ctx: &VerbCtx) -> anyhow::Result<String> {
    let m = base(ctx, "usage", None, None)?;
    let env = |n: &str| std::env::var_os(n);
    let client = from_config(None, &m.config, &default_store(&ctx.paths), &env).map_err(|r| anyhow!("zygo usage --sync: {r}"))?;
    let Some(c) = client else { anyhow::bail!("zygo usage --sync: no server configured ([server] url)") };
    let fail = |e: anyhow::Error| anyhow!("zygo usage --sync: {}", esc(&e.to_string()));
    let r = Store::open(&ctx.paths.usage_db).and_then(|s| sync_pending(&c, &s, &ctx.paths.sessions_dir)).map_err(fail)?;
    let mut o = String::new();
    for l in &r.lines {
        let _ = writeln!(o, "zygo usage --sync: {}", esc(l));
    }
    let _ = write!(o, "synced {} of {} sessions", r.synced, r.tried);
    if r.live > 0 {
        let _ = write!(o, "; {} live, skipped", r.live);
    }
    if r.unreachable {
        return Err(Unreachable(o).into());
    }
    Ok(o)
}

pub fn run(ctx: &VerbCtx, args: &[String], env_session: Option<&str>) -> anyhow::Result<String> {
    let a = parse(args)?;
    let since = match &a.since {
        Some(s) => Some(parse_since(s).map_err(|_| {
            UsageError(format!("zygo usage: --since takes a date YYYY-MM-DD, not `{}`", esc(s)))
        })?),
        None => None,
    };
    if a.sync {
        return sync(ctx);
    }
    let session = a.session.clone().or_else(|| if a.since.is_none() { env_session.map(str::to_owned) } else { None });
    let scope = match (&session, &a.since) {
        (None, None) => "all sessions".to_string(),
        (Some(s), None) => format!("session {}", esc(s)),
        (None, Some(d)) => format!("since {}", esc(d)),
        (Some(s), Some(d)) => format!("session {} since {}", esc(s), esc(d)),
    };
    let fail = |e: anyhow::Error| anyhow!("zygo usage: {}", esc(&e.to_string()));
    let s = if ctx.paths.usage_db.exists() {
        Store::open_read_only(&ctx.paths.usage_db).and_then(|st| st.summary(&Filter { session, since })).map_err(fail)?
    } else {
        Summary::default()
    };
    let mut o = String::new();
    let share = if s.input_tokens + s.cache_read_tokens + s.cache_write_tokens > 0 { Some(s.cache_read_share) } else { None };
    let events: Vec<String> =
        s.guard_events.iter().flat_map(|(g, ls)| ls.iter().map(move |(l, n)| format!("{} {} {n}", esc(g), esc(l)))).collect();
    let _ = writeln!(o, "scope: {scope}\nrequests: {}", s.requests);
    let _ = writeln!(o, "tokens: {} in, {} out, {} cache read, {} cache write", s.input_tokens, s.output_tokens, s.cache_read_tokens, s.cache_write_tokens);
    let _ = writeln!(o, "cache read share: {}\nestimated cost: ${:.4}", pct(share), s.estimated_usd);
    let _ = writeln!(o, "bytes trimmed: {}\ncompaction signals: {}\ncompactions observed: {}", s.bytes_trimmed, s.compaction_signals, s.compactions_observed);
    let _ = writeln!(o, "guard events: {}", if events.is_empty() { "none".into() } else { events.join(", ") });
    if s.by_role_model.is_empty() {
        o.push_str("by role and model: none");
    } else {
        o.push_str("by role and model:");
        for r in &s.by_role_model {
            let role = if r.role.is_empty() { "-".to_string() } else { esc(&r.role) };
            let _ = write!(
                o,
                "\n  {role} {}: {} requests, {} in, {} out, {} cache read, {} cache write, ${:.4}",
                esc(&r.model), r.requests, r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens, r.estimated_usd
            );
        }
    }
    Ok(o)
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, std::env::var("ZYGO_SESSION").ok().filter(|s| !s.is_empty()).as_deref())
}
