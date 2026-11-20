//! The `work` hook (§13.3): `begin` on `work_begin`, one line per tool call, the fold on `SubagentStop`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::anyhow;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zygo_config::{LoadContext, Merged, RepoFile};
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_usage::Store;

use crate::current::{self, WorkItem};
use crate::fold;
use crate::guards;
use crate::log::{self, AgentLock, Begin, CheckResult, Kind, LogLine};
use crate::policy::Guard;
use crate::repo;

/// Everything a `work` hook call reads besides its input; tests build it without the environment.
pub struct HookCtx<'a> {
    pub session_dir: &'a SessionDir,
    pub session: String,
    pub usage_db: PathBuf,
    /// Unix seconds.
    pub now: i64,
    /// The merged config for a cwd, repository layer included.
    pub config: &'a dyn Fn(&Path) -> Option<Merged>,
    /// The current work item for a cwd.
    pub item: &'a dyn Fn(&Path, &Merged) -> Option<WorkItem>,
    /// One stderr line (C6).
    pub warn: &'a dyn Fn(&str),
    /// Canonical temp and scratch roots the scope guard never judges (G9); production: [`guards::temp_roots`].
    pub temp_roots: Vec<PathBuf>,
}

/// Set when this process has written its config lines: one set per hook run (C6), however often the config loads.
static CONFIG_LINES_SAID: AtomicBool = AtomicBool::new(false);

/// Layers 1–4 for a hook (§6.1), the team layer from `sd`'s `team.toml` (§14 note 2026-11-19). No file: no team layer
/// (N37 limit N3). A `team.toml` that cannot be read, or with which the config does not load while it loads without
/// it, is left out as an unread `.zygo.toml` is (§13.5 note 2026-11-19, C6): the user levels apply and the second
/// value is the one stderr line.
pub fn session_config(paths: &Paths, sd: Option<&SessionDir>, repo: Option<PathBuf>) -> (Option<Merged>, Option<String>) {
    let ctx = LoadContext { paths: paths.clone(), repo, team: None };
    let local = || zygo_config::load(&ctx).ok();
    let team = match sd.map(|sd| zygo_config::read_team(&sd.root)) {
        None | Some(Ok(None)) => return (local(), None),
        Some(Err(e)) => return (local(), Some(format!("team.toml not read ({e}); user levels apply"))),
        Some(Ok(Some(t))) => t,
    };
    match zygo_config::load(&LoadContext { team: Some(team), ..ctx.clone() }) {
        Ok(mut m) => {
            drop_model_warning(&mut m.warnings);
            (Some(m), None)
        }
        // The team layer is the cause only when the config loads without it.
        Err(e) => match local() {
            Some(m) => (Some(m), Some(format!("team.toml not used ({e}); user levels apply"))),
            None => (None, None),
        },
    }
}

/// Hooks use no models: an ignored user `[[model]]` is not their error, so no line (C6).
fn drop_model_warning(warnings: &mut Vec<String>) {
    warnings.retain(|w| w != zygo_config::MODEL_IGNORED);
}

/// The merged config for `cwd`, with the session's team layer ([`session_config`]) and its repository's layer when
/// `cwd` is in a checkout. A git call for that layer that does not finish drops the layer (§13.5 note 2026-11-19); that
/// and each `Merged.warnings` entry (an invalid `.zygo.toml`, §13.5) are one `zygo hook: ` stderr line each, written
/// once per process.
fn env_config(cwd: &Path) -> Option<Merged> {
    let (top, mut unread) = match repo::find_checkout(cwd, Instant::now() + repo::BOUND) {
        // git says "not a git repository" although a `.git` entry is there: a broken repository (N53 round 2).
        Ok(None) if git_marker(cwd) => (None, Some("git rev-parse: failed".to_string())),
        Ok(c) => (c.map(|c| c.top), None),
        // No `.git` entry on the way up (no git, a cwd that is gone, a slow git outside any checkout): not a repository.
        Err(_) if !git_marker(cwd) => (None, None),
        Err((call, e)) => (None, Some(format!("git {call}: {}", e.reason()))),
    };
    let (m, team) = session_config(&Paths::from_env(), SessionDir::from_env().as_ref(), top);
    if let Some(RepoFile::Unread { why }) = m.as_ref().map(|m| &m.repo_file) {
        unread = Some(why.clone());
    }
    let mut lines: Vec<String> =
        unread.map(|why| format!(".zygo.toml not read ({why}); user and team levels apply")).into_iter().collect();
    lines.extend(team);
    lines.extend(m.iter().flat_map(|m| m.warnings.iter().cloned()));
    if !lines.is_empty() && !CONFIG_LINES_SAID.swap(true, Ordering::Relaxed) {
        for l in &lines {
            zygo_core::hook::error_line(l);
        }
    }
    m
}

fn env_item(cwd: &Path, m: &Merged) -> Option<WorkItem> {
    current::resolve(cwd, &m.config.work)
}

fn warn_stderr(s: &str) {
    let _ = writeln!(std::io::stderr(), "{s}");
}

pub(crate) fn env_ctx(sd: &SessionDir) -> HookCtx<'_> {
    HookCtx {
        session_dir: sd,
        session: session_id(sd),
        usage_db: Paths::from_env().usage_db,
        now: time::OffsetDateTime::now_utc().unix_timestamp(),
        config: &env_config,
        item: &env_item,
        warn: &warn_stderr,
        temp_roots: guards::temp_roots(&std::env::temp_dir()),
    }
}

/// `zygo hook work`; outside a session it records nothing.
pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(sd) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    run_with(input, &env_ctx(&sd))
}

pub fn post_tool(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(sd) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    let ctx = env_ctx(&sd);
    if !units_on(&ctx, &cwd_of(input)?) {
        return Ok(HookOutput::None);
    }
    post_tool_with(input, &ctx)
}

pub(crate) fn cwd_of(input: &HookInput) -> anyhow::Result<PathBuf> {
    match input.raw.get("cwd").and_then(Value::as_str) {
        Some(c) => Ok(PathBuf::from(c)),
        None => Ok(std::env::current_dir()?),
    }
}

/// `("main", None)` for the lead; the subagent's id and type otherwise. An `agent_id` key that is not a string is an error.
pub(crate) fn agent_of(input: &HookInput) -> anyhow::Result<(String, Option<String>)> {
    if !input.has_agent_id {
        return Ok(("main".into(), None));
    }
    let id = input.agent_id.clone().ok_or_else(|| anyhow!("agent_id is not a string"))?;
    Ok((id, input.raw.get("agent_type").and_then(Value::as_str).map(String::from)))
}

pub(crate) fn is_zygo(agent_type: Option<&str>) -> bool {
    agent_type.is_some_and(|t| t.starts_with("zygo-"))
}

/// The deny reason for `work_verdict` inside a subagent (§13.3: lead only).
pub const VERDICT_LEAD_ONLY: &str = "zygo: work_verdict is for the lead only. Report your result to the lead; the lead records the verdict.";

/// Chosen bound per `files` path: POSIX `PATH_MAX`, so `begin.json` stays bounded (64 × 4,096 + 1,024 bytes ≈ 265 KiB).
pub const MAX_PATH_BYTES: usize = 4096;
pub const PATH_OVER_4096: &str = "`files` has a path over 4,096 bytes";

fn deny(reason: &str) -> HookOutput {
    HookOutput::Json(json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}))
}

/// The open unit under `g`; a `begin.json` that does not parse is one stderr line and no open unit (C6).
fn open_unit(g: &AgentLock, agent: &str, ctx: &HookCtx) -> Option<Begin> {
    g.read_begin().unwrap_or_else(|_| {
        (ctx.warn)(&format!("zygo hook: work: {agent}.begin.json does not parse; treated as no open unit"));
        None
    })
}

pub fn run_with(input: &HookInput, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    // `PreToolUse` on `work_verdict` (`--event verdict`): an `agent_id` key, any value, is a subagent (§8.3 mechanism).
    if input.cli_event.as_deref() == Some("verdict") {
        return Ok(if input.has_agent_id { deny(VERDICT_LEAD_ONLY) } else { HookOutput::None });
    }
    let cwd = cwd_of(input)?;
    if !units_on(ctx, &cwd) {
        return Ok(HookOutput::None);
    }
    if input.cli_event.as_deref() == Some("begin") {
        begin(input, ctx, &cwd)?;
        return Ok(HookOutput::None);
    }
    match input.event.as_deref() {
        Some("PostToolUse" | "PostToolUseFailure") => post_tool_with(input, ctx),
        Some("SubagentStop") => subagent_stop_with(input, ctx),
        _ => Ok(HookOutput::None),
    }
}

/// The levels in force for `cwd` (§13.1), session overrides included.
fn levels(ctx: &HookCtx, m: &Merged, cwd: &Path) -> BTreeMap<String, String> {
    let p = crate::guards::policy(m, ctx.session_dir, &crate::guards::top(cwd));
    Guard::CONFIGURED.iter().map(|g| (g.name().to_string(), p.level(*g).to_string())).collect()
}

fn begin(input: &HookInput, ctx: &HookCtx, cwd: &Path) -> anyhow::Result<()> {
    let (agent, agent_type) = agent_of(input)?;
    if input.has_agent_id && !is_zygo(agent_type.as_deref()) {
        return Ok(());
    }
    let mut args = BeginArgs::parse(input.tool_input.as_ref().unwrap_or(&Value::Null)).map_err(|e| anyhow!("work_begin: {e}"))?;
    let merged = (ctx.config)(cwd);
    if let Some(m) = &merged
        && (args.check.is_empty() || args.files.is_empty())
        && let Some(item) = (ctx.item)(cwd, m)
    {
        let u = item.unit();
        if args.unit == u || args.unit.starts_with(&format!("{u}:")) {
            if args.check.is_empty() && let Some(h) = item.check_hint {
                args.check = h;
            }
            if args.files.is_empty() {
                args.files = item.files_hint;
            }
        }
    }
    // A source hint is held to the same limits.
    args.validate().map_err(|e| anyhow!("work_begin: {e}"))?;
    let levels = merged.as_ref().map(|m| levels(ctx, m, cwd));
    let wd = ctx.session_dir.work();
    let g = log::lock(&wd, &agent)?;
    let old = open_unit(&g, &agent, ctx);
    if let Some(o) = &old
        && o.unit == args.unit
        && o.round == args.round
    {
        if o.stopped_at.is_some() {
            g.write_begin(Some(&Begin { stopped_at: None, ..o.clone() }))?;
        }
        return Ok(());
    }
    let at = |kind| LogLine { n: 0, ts: ctx.now, kind };
    if old.is_some() {
        g.append(&at(Kind::End { reason: "superseded".into() }))?;
    }
    let result = match g.append(&at(Kind::Begin { unit: args.unit.clone(), round: args.round }))? {
        None => {
            g.write_begin(None)?;
            Err(anyhow!("work log full"))
        }
        Some(offset) => {
            if let Some(levels) = levels {
                g.append(&at(Kind::Levels { levels }))?;
            }
            g.write_begin(Some(&Begin {
                unit: args.unit,
                round: args.round,
                check: args.check,
                files: args.files,
                began_at: ctx.now,
                offset,
                session: ctx.session.clone(),
                agent_type,
                stopped_at: None,
                edit_root: None,
            }))?;
            Ok(())
        }
    };
    drop(g);
    if let Some(o) = old
        && let Some(mut row) = fold::fold_begin(&wd, &agent, &o)
    {
        // A stopped unit ended at its stop, not at the begin that supersedes it.
        row.ended_at = Some(o.stopped_at.unwrap_or(ctx.now));
        Store::open(&ctx.usage_db)?.insert_work_unit(&row)?;
    }
    result
}

const EDIT_TOOLS: [&str; 3] = ["Edit", "Write", "NotebookEdit"];
/// act-lite's navigation tools (§10.5).
const NAV_TOOLS: [&str; 4] = ["skeleton", "symbols", "symbols_batch", "repo_outline"];
const NAV_KEYS: [&str; 5] = ["path", "file_path", "file", "paths", "files"];

/// A copy of `v` with every object's keys in sorted order.
fn canon(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), canon(&m[k]))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        x => x.clone(),
    }
}

/// 16 hex of SHA-256 of `<tool>\n<input with sorted keys>`.
fn call_hash(tool: &str, input: &Value) -> String {
    hex::encode(Sha256::digest(format!("{tool}\n{}", canon(input)).as_bytes()))[..16].to_string()
}

/// The repository root holding `p`: the nearest ancestor with a `.git` entry, after resolving the nearest existing ancestor.
pub(crate) fn repo_root(p: &Path) -> Option<String> {
    let start = p.ancestors().find_map(|a| a.canonicalize().ok())?;
    start.ancestors().find(|a| a.join(".git").exists()).map(|a| a.to_string_lossy().into_owned())
}

/// A `.git` entry in `cwd` (or its nearest existing ancestor) or a parent git would search: [`repo_root`], but never at
/// or above a `GIT_CEILING_DIRECTORIES` entry, nor across a filesystem boundary unless `GIT_DISCOVERY_ACROSS_FILESYSTEM`
/// is true (git does not look there either).
fn git_marker(cwd: &Path) -> bool {
    let ceilings: Vec<PathBuf> = std::env::var_os("GIT_CEILING_DIRECTORIES")
        .map(|v| std::env::split_paths(&v).filter_map(|p| p.canonicalize().ok()).collect())
        .unwrap_or_default();
    let across = std::env::var("GIT_DISCOVERY_ACROSS_FILESYSTEM")
        .is_ok_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "" | "0" | "false" | "no" | "off"));
    git_marker_in(cwd, &ceilings, across, &device)
}

/// The device id of `p` (unix `st_dev`); none elsewhere, where no boundary is checked.
fn device(p: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).ok().map(|m| m.dev())
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        None
    }
}

/// [`git_marker`] with its inputs: the start is always searched; a parent only below every ceiling and, unless
/// `across_fs`, on the start's device (`dev` none for the start: no boundary).
fn git_marker_in(cwd: &Path, ceilings: &[PathBuf], across_fs: bool, dev: &dyn Fn(&Path) -> Option<u64>) -> bool {
    let Some(start) = cwd.ancestors().find_map(|a| a.canonicalize().ok()) else { return false };
    let home = dev(&start);
    start
        .ancestors()
        .enumerate()
        .take_while(|(i, a)| {
            *i == 0 || (!ceilings.iter().any(|c| c == a) && (across_fs || home.is_none() || dev(a) == home))
        })
        .any(|(_, a)| a.join(".git").exists())
}

/// Lexical normalisation: `.` dropped, `..` pops; no file-system access.
pub(crate) fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

fn nav_tool(name: &str, agent: Agent) -> bool {
    NAV_TOOLS.iter().any(|t| {
        let Some(server) = name.strip_suffix(t) else { return false };
        let server = match agent {
            Agent::ClaudeCode => server.strip_prefix("mcp__").and_then(|s| s.strip_suffix("__")),
            Agent::OpenCode => server.strip_suffix('_'),
        };
        server.is_some_and(|s| s.ends_with("act-lite"))
    })
}

fn nav_paths(input: &Value, cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for k in NAV_KEYS {
        let vals: Vec<&str> = match input.get(k) {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        out.extend(vals.into_iter().map(|p| normalize(&cwd.join(p)).to_string_lossy().into_owned()));
    }
    out
}

/// True when the line fits in `MAX_LINE_BYTES` at any line number.
fn fits(l: &LogLine) -> bool {
    serde_json::to_string(&LogLine { n: log::MAX_LINES, ..l.clone() }).is_ok_and(|s| s.len() < log::MAX_LINE_BYTES)
}

/// `(pushed, pr_opened)` of a Bash command (R10).
fn push_pr(cmd: &str) -> (bool, bool) {
    let (mut pushed, mut pr) = (false, false);
    for seg in cmd.split([';', '|', '&', '\n']) {
        let w: Vec<&str> = seg.split_whitespace().collect();
        pr |= w.starts_with(&["gh", "pr", "create"]);
        if w.first() == Some(&"git") {
            let mut i = 1;
            while let Some(x) = w.get(i)
                && x.starts_with('-')
            {
                i += if *x == "-C" || *x == "-c" { 2 } else { 1 };
            }
            pushed |= w.get(i) == Some(&"push");
        }
    }
    (pushed, pr)
}

/// The same hash among the last `REPEAT_WINDOW` calls, with no edit since.
fn repeated(tail: &[(u64, LogLine)], h: &str) -> bool {
    let mut calls = 0;
    for (_, l) in tail.iter().rev() {
        match &l.kind {
            Kind::Edit { .. } => return false,
            Kind::Call { h: x, .. } | Kind::Failed { h: x, .. } => {
                if x == h {
                    return true;
                }
                calls += 1;
                if calls == log::REPEAT_WINDOW {
                    return false;
                }
            }
            _ => {}
        }
    }
    false
}

/// `PostToolUse`/`PostToolUseFailure`: `call|failed`, `repeat`, `edit`, `navigated`, `check`, `pushed`, `pr_opened`.
pub fn post_tool_with(input: &HookInput, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    let cwd = cwd_of(input)?;
    let (agent, _) = agent_of(input)?;
    let tool = input.tool_name.clone().unwrap_or_default();
    let tin = input.tool_input.clone().unwrap_or(Value::Null);
    let h = call_hash(&tool, &tin);
    // A background command's result is not observed: it is no check run.
    let background = tin.get("run_in_background") == Some(&Value::Bool(true));
    let failed = input.event.as_deref() == Some("PostToolUseFailure")
        || input.raw.pointer("/tool_response/interrupted") == Some(&Value::Bool(true));
    let edit = if !failed && EDIT_TOOLS.contains(&tool.as_str()) {
        ["file_path", "notebook_path"].iter().find_map(|k| tin.get(k)?.as_str()).map(|p| {
            let p = cwd.join(p);
            Kind::Edit { root: p.parent().and_then(repo_root) }
        })
    } else {
        None
    };
    let mut nav = None;
    if !failed && nav_tool(&tool, input.agent) {
        let mut paths = nav_paths(&tin, &cwd);
        while !paths.is_empty() && !fits(&LogLine { n: 0, ts: ctx.now, kind: Kind::Navigated { paths: paths.clone() } }) {
            paths.pop();
        }
        nav = (!paths.is_empty()).then_some(Kind::Navigated { paths });
    }
    let command = if tool == "Bash" { tin.get("command").and_then(Value::as_str) } else { None };
    let (pushed, pr) = command.filter(|_| !failed).map_or((false, false), push_pr);

    let wd = ctx.session_dir.work();
    let g = log::lock(&wd, &agent)?;
    let tail = g.tail();
    let b = open_unit(&g, &agent, ctx);
    if let Some(b) = &b {
        // A call resumes a stopped unit; an edit sets the repository its check must run in (§13.3).
        let mut next = Begin { stopped_at: None, ..b.clone() };
        if let Some(Kind::Edit { root }) = &edit {
            next.edit_root = root.clone();
        }
        if next != *b {
            g.write_begin(Some(&next))?;
        }
    }
    let mut kinds = vec![if failed { Kind::Failed { tool: tool.clone(), h: h.clone() } } else { Kind::Call { tool: tool.clone(), h: h.clone() } }];
    if repeated(&tail, &h) {
        kinds.push(Kind::Repeat { tool });
        kinds.push(crate::guards::repeat_event());
    }
    kinds.extend(edit);
    kinds.extend(nav);
    if let (Some(b), Some(cmd)) = (&b, command)
        && !background
        && let Some(d) = check_dir(cmd, &b.check)
    {
        let dir = d.map_or_else(|| cwd.clone(), |d| cwd.join(d));
        if b.edit_root.as_deref().is_none_or(|r| repo_root(&dir).as_deref() == Some(r)) {
            kinds.push(Kind::Check { result: if failed { CheckResult::Failed } else { CheckResult::Passed } });
        }
    }
    if pushed {
        kinds.push(Kind::Pushed);
    }
    if pr {
        kinds.push(Kind::PrOpened);
    }
    for kind in kinds {
        let l = LogLine { n: 0, ts: ctx.now, kind };
        if !fits(&l) {
            let t = serde_json::to_value(&l.kind).ok().and_then(|v| v["t"].as_str().map(String::from)).unwrap_or_default();
            (ctx.warn)(&format!("zygo hook: work log line over 1 KiB dropped ({t})"));
            continue;
        }
        g.append(&l)?;
    }
    Ok(HookOutput::None)
}

/// `SubagentStop` of a `zygo-` subagent: fold the open unit into its row; the unit stays open, marked stopped (R13).
fn subagent_stop_with(input: &HookInput, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    let (agent, agent_type) = agent_of(input)?;
    if !is_zygo(agent_type.as_deref()) {
        return Ok(HookOutput::None);
    }
    let cwd = cwd_of(input)?;
    let p = (ctx.config)(&cwd).map(|m| guards::policy(&m, ctx.session_dir, &guards::top(&cwd)));
    let wd = ctx.session_dir.work();
    let g = log::lock(&wd, &agent)?;
    let Some(b) = open_unit(&g, &agent, ctx) else { return Ok(HookOutput::None) };
    // Claims (§13.4) under the lock, before `stopped_at` and the fold, so the folded row counts its line.
    let claims = match &p {
        Some(p) if !b.check.is_empty() => guards::claims(&g, &b, p, ctx),
        _ => Ok(HookOutput::None),
    };
    // C6 (Round 3 fix 4): any internal error prints nothing that changes a decision, and one stderr line in all. A fold
    // error after the claims line drops the claims output (known limit: the "once" is spent).
    match (claims, stop_fold(g, &b, &wd, &agent, ctx)) {
        (Ok(out), Ok(())) => Ok(out),
        (Err(e), Ok(())) => {
            (ctx.warn)(&format!("zygo hook: claims: {e:#}"));
            Ok(HookOutput::None)
        }
        (Ok(_), Err(e)) => Err(e),
        (Err(c), Err(e)) => Err(e.context(format!("claims: {c:#}"))),
    }
}

/// The stop mark under `g`, then the fold into the unit's row.
fn stop_fold(g: AgentLock, b: &Begin, wd: &Path, agent: &str, ctx: &HookCtx) -> anyhow::Result<()> {
    g.write_begin(Some(&Begin { stopped_at: Some(ctx.now), ..b.clone() }))?;
    drop(g);
    if let Some(mut row) = fold::fold_begin(wd, agent, b) {
        row.ended_at = Some(ctx.now);
        Store::open(&ctx.usage_db)?.insert_work_unit(&row)?;
    }
    Ok(())
}

/// `overrides.json`, else the session view, else the config for `cwd`, else on (§17.1).
pub fn units_on(ctx: &HookCtx, cwd: &Path) -> bool {
    if let Some(w) = Overrides::load(ctx.session_dir).work_units {
        return w;
    }
    if let Ok(v) = SessionView::load(ctx.session_dir) {
        return v.work_units;
    }
    (ctx.config)(cwd).is_none_or(|m| m.config.features.work_units)
}

/// [`units_on`] with the production config lookup, for callers without a `HookCtx` (N26).
pub fn units_on_session(sd: &SessionDir, cwd: &Path) -> bool {
    units_on(&env_ctx(sd), cwd)
}

/// `SessionView.session`, else the session directory's file name (§8.2 step 2).
pub fn session_id(sd: &SessionDir) -> String {
    match SessionView::load(sd) {
        Ok(v) => v.session,
        Err(_) => sd.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

fn valid_segment(s: &str) -> bool {
    (1..=48).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// §13.1: `<source>:<id>[:<part>]`, each segment 1–48 of `[A-Za-z0-9._-]` starting alphanumeric, ≤ 128 bytes.
pub fn valid_unit(s: &str) -> bool {
    let n = s.split(':').count();
    s.len() <= 128 && (2..=3).contains(&n) && s.split(':').all(valid_segment)
}

#[derive(Debug, Clone, PartialEq)]
pub struct BeginArgs {
    pub unit: String,
    pub round: u32,
    pub check: String,
    pub files: Vec<String>,
}

impl BeginArgs {
    /// `work_begin` arguments (§13.3); `check` and `files` optional, unknown keys ignored.
    pub fn parse(v: &Value) -> Result<BeginArgs, String> {
        let unit = v.get("unit").and_then(Value::as_str).ok_or("`unit` must be a string")?;
        if !valid_unit(unit) {
            return Err(format!("`unit` must be <source>:<id>[:<part>], each 1-48 of [A-Za-z0-9._-] starting alphanumeric, at most 128 bytes: `{unit}`"));
        }
        let round = v
            .get("round")
            .and_then(Value::as_u64)
            .filter(|r| *r >= 1)
            .and_then(|r| u32::try_from(r).ok())
            .ok_or("`round` must be an integer from 1")?;
        let check = match v.get("check") {
            None | Some(Value::Null) => String::new(),
            Some(c) => c.as_str().ok_or("`check` must be a string")?.to_string(),
        };
        let files: Vec<String> = match v.get("files") {
            None | Some(Value::Null) => Vec::new(),
            Some(f) => f
                .as_array()
                .ok_or("`files` must be an array of strings")?
                .iter()
                .map(|x| x.as_str().map(String::from).ok_or("`files` must be an array of strings"))
                .collect::<Result<_, _>>()?,
        };
        let a = BeginArgs { unit: unit.to_string(), round, check, files };
        // The limits first: the shape error below repeats the path.
        a.validate()?;
        for f in &a.files {
            let absolute = f.starts_with(['/', '\\']) || f.as_bytes().get(1) == Some(&b':') || Path::new(f).is_absolute();
            if f.is_empty() || absolute || f.split(['/', '\\']).any(|s| s == "..") {
                return Err(format!("`files` must hold repository-relative paths without `..`: `{f}`"));
            }
        }
        Ok(a)
    }

    /// §13.3: `check` ≤ 1,024 bytes, `files` ≤ 64 paths; each path ≤ `MAX_PATH_BYTES` (chosen).
    pub fn validate(&self) -> Result<(), String> {
        if self.check.len() > 1024 {
            return Err("`check` is over 1,024 bytes".into());
        }
        if self.files.len() > 64 {
            return Err("`files` has more than 64 paths".into());
        }
        if self.files.iter().any(|f| f.len() > MAX_PATH_BYTES) {
            return Err(PATH_OVER_4096.into());
        }
        Ok(())
    }
}

/// `None`: `command` does not run `check`; `Some(dir)`: it does, with the `cd` directory if any (§13.3).
pub fn check_dir(command: &str, check: &str) -> Option<Option<String>> {
    let want = check.split_whitespace().collect::<Vec<_>>().join(" ");
    if want.is_empty() {
        return None;
    }
    if command.split_whitespace().collect::<Vec<_>>().join(" ") == want {
        return Some(None);
    }
    let mut rest = command.trim_start();
    let mut dir = None;
    if let Some(after) = rest.strip_prefix("cd")
        && after.starts_with(char::is_whitespace)
    {
        let after = after.trim_start();
        let parsed = match after.chars().next() {
            Some(q @ ('"' | '\'')) => after[1..].find(q).map(|i| (&after[1..1 + i], &after[2 + i..])),
            _ => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                Some((&after[..end], &after[end..]))
            }
        };
        if let Some((d, tail)) = parsed
            && let Some(r) = tail.trim_start().strip_prefix("&&")
            && r.starts_with(char::is_whitespace)
        {
            // `~` and `$` expand in the shell, not here: such a directory matches no repository.
            if d.starts_with(['~', '$']) {
                return None;
            }
            dir = Some(d.to_string());
            rest = r;
        }
    }
    let mut w: Vec<&str> = rest.split_whitespace().collect();
    if w.last() == Some(&"2>&1") {
        w.pop();
    }
    loop {
        if w.join(" ") == want {
            return Some(dir);
        }
        // An environment word `NAME=value` (R18).
        let var = w.first().and_then(|x| x.split_once('=')).is_some_and(|(n, _)| {
            n.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        });
        if !var {
            return None;
        }
        w.remove(0);
    }
}

#[cfg(test)]
mod git_marker_tests {
    use super::*;

    #[test] // N53 round 3: the `.git` walk stops at a filesystem boundary, at a ceiling, as git does
    fn git_marker_stops_at_boundary_and_ceiling() {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().canonicalize().unwrap();
        let cwd = base.join("mnt/sub");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir(base.join(".git")).unwrap();
        let mnt = base.join("mnt");
        let one = |_: &Path| Some(1);
        let mounted = |p: &Path| Some(if p.starts_with(&mnt) { 2 } else { 1 });
        assert!(git_marker_in(&cwd, &[], false, &one));
        assert!(!git_marker_in(&cwd, &[], false, &mounted));
        assert!(git_marker_in(&cwd, &[], true, &mounted), "GIT_DISCOVERY_ACROSS_FILESYSTEM crosses it");
        assert!(git_marker_in(&cwd, &[], false, &|_: &Path| None), "no device ids (Windows): no boundary");
        assert!(!git_marker_in(&cwd, std::slice::from_ref(&mnt), false, &one));
        std::fs::create_dir(cwd.join(".git")).unwrap();
        assert!(git_marker_in(&cwd, std::slice::from_ref(&cwd), false, &mounted), "cwd itself is always searched");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // N66 J3: only the ignored user `[[model]]` warning is dropped, not another that starts the same way
    fn only_the_model_warning_is_dropped() {
        let other = "config: [[model]] m: some later warning".to_string();
        let mut w = vec![zygo_config::MODEL_IGNORED.to_string(), other.clone()];
        drop_model_warning(&mut w);
        assert_eq!(w, [other]);
    } // mutation: a prefix match (→ the other warning dropped too)
}
