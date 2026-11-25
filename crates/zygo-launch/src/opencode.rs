//! The OpenCode adapter (§8.3, §8.7, §15.3): inline config, generated plugin, delivered plugins and preflight.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use serde_json::{json, Map, Value};
use zygo_core::Agent;
use zygo_hooks::session_text::mechanics;
use zygo_plugins::snapshot::command_found;
use zygo_plugins::{embedded, store};
use zygo_proxy::client::WindowArg;

use crate::adapter::{AgentAdapter, ChildSpec, LaunchCtx, Plugins, Preflight};

/// OpenCode's built-in agents: name, the role it runs, and whether it is a primary agent.
const BUILTIN: [(&str, &str, bool); 4] = [("build", "lead", true), ("plan", "plan", true), ("general", "code", false), ("explore", "read", false)];

/// The window when the catalog model states none (§10.2.5).
const DEFAULT_WINDOW: u64 = 200_000;

/// The generated plugin; `__ZYGO_PATH__` is replaced by the zygo path as a JSON string.
const PLUGIN: &str = include_str!("../assets/opencode-plugin.js");

pub struct OpenCode {
    pub zygo: PathBuf,
}

impl OpenCode {
    /// An OpenCode adapter whose plugin and hooks run the zygo binary `zygo` (as `ClaudeCode::new` takes it).
    pub fn new(zygo: PathBuf) -> OpenCode {
        OpenCode { zygo }
    }
}

impl Default for OpenCode {
    fn default() -> Self {
        OpenCode::new(std::env::current_exe().unwrap_or_default())
    }
}

/// What delivered plugins add to the inline config and the launch output.
#[derive(Default)]
struct Delivery {
    agents: Map<String, Value>,
    commands: Map<String, Value>,
    mcp: Map<String, Value>,
    lines: Vec<String>,
    log: Vec<String>,
    partial: bool,
}

/// The agent names zygo writes, and those that get `permission.edit = "deny"`.
struct AgentNames {
    all: BTreeSet<String>,
    edit_deny: BTreeSet<String>,
}

fn agent_names(ctx: &LaunchCtx) -> AgentNames {
    let main = &ctx.config.main_role;
    let mut n = AgentNames { all: BTreeSet::new(), edit_deny: BTreeSet::new() };
    for (name, role, _) in BUILTIN {
        if ctx.config.roles.contains_key(role) {
            n.all.insert(name.into());
            if role == main && ctx.effective_role.as_deref() == Some(main.as_str()) {
                n.edit_deny.insert(name.into());
            }
        }
    }
    let writes = ctx.config.roles.iter().map(|(k, r)| (k, r.writes)).chain(ctx.config.custom_roles.iter().map(|(k, r)| (k, r.writes)));
    for (role, writes) in writes.filter(|(k, _)| *k != main) {
        n.all.insert(format!("zygo-{role}"));
        if !writes {
            n.edit_deny.insert(format!("zygo-{role}"));
        }
    }
    if let Plugins::Delivered(Some(d)) = &ctx.plugins {
        for p in &d.plugins {
            for rel in p.entry.files.keys() {
                if let Some(a) = rel.strip_prefix("agents/").and_then(|f| f.strip_suffix(".md")).filter(|a| !a.contains('/')) {
                    n.all.insert(format!("{}-{a}", p.id.plugin));
                }
            }
        }
    }
    n
}

/// The built-in agent mapped to `main_role` and whether it is primary.
fn main_agent(ctx: &LaunchCtx) -> Option<(&'static str, bool)> {
    BUILTIN.iter().find(|(_, role, _)| *role == ctx.config.main_role && ctx.config.roles.contains_key(*role)).map(|(n, _, p)| (*n, *p))
}

fn limit(ctx: &LaunchCtx, model: &str) -> Value {
    let row = ctx.cfg.config.catalog.get(model);
    let facts = row.and_then(|r| r.facts.as_ref());
    let window = facts.map(|f| f.context_window).filter(|w| *w > 0);
    let soft = row.and_then(|r| r.soft_context_window).filter(|w| *w > 0);
    let context = match ctx.compact {
        Some(WindowArg::Tokens(n)) => window.map_or(n, |w| n.min(w)),
        Some(WindowArg::Off) => window.unwrap_or(DEFAULT_WINDOW),
        None => soft.or(window).unwrap_or(DEFAULT_WINDOW),
    };
    json!({ "context": context, "output": facts.map_or(0, |f| f.max_output_tokens) })
}

/// The provider key as a reference to the session's `token` file (§8.7, premise PO1): OpenCode
/// reads the file when it loads its config, so the token is in no environment row. OpenCode matches
/// `{file:…}` on the raw config text, so the path uses `/` (Windows `\` becomes `/`; elsewhere a `\` is refused)
/// and may hold no `{`, `}`, `"` or control character. A path that is not absolute (on Windows: no `X:/` drive and
/// not UNC `//`) is refused, so OpenCode's own resolution (`~/`, relative to the project or the drive) never applies.
fn token_ref(t: &Path) -> anyhow::Result<String> {
    let raw = t.to_str().ok_or_else(|| anyhow!("cannot use the session directory path: it is not valid UTF-8"))?;
    let p = if cfg!(windows) { raw.replace('\\', "/") } else { raw.to_string() };
    let shown = p.escape_debug();
    if p.contains(['{', '}', '"', '\\']) || p.chars().any(|c| c < ' ' || c == '\u{7f}') {
        bail!("cannot reference the session token file {shown}: its path holds {{, }}, \", \\ or a control character");
    }
    let b = p.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/';
    let absolute = if cfg!(windows) { drive || p.starts_with("//") } else { p.starts_with('/') };
    if !absolute {
        bail!("cannot reference the session token file {shown}: its path is not absolute");
    }
    Ok(format!("{{file:{p}}}"))
}

fn inline_config(ctx: &LaunchCtx, zygo: &str, d: &Delivery, key: &str) -> Value {
    let c = &ctx.config;
    let mut headers = Map::new();
    headers.insert("X-Zygo-Session".into(), json!(ctx.session));
    if !ctx.agent_version.is_empty() {
        headers.insert("X-Zygo-Agent-Version".into(), json!(ctx.agent_version));
    }
    if let Some(r) = &ctx.repo {
        headers.insert("X-Zygo-Repo".into(), json!(r));
    }
    if let Some(w) = &ctx.work {
        headers.insert("X-Zygo-Work".into(), json!(format!("{}:{}", w.source, w.id)));
    }
    let models: Map<String, Value> = c
        .roles
        .iter()
        .map(|(k, r)| (k, &r.model))
        .chain(c.custom_roles.iter().map(|(k, r)| (k, &r.model)))
        .map(|(k, m)| (k.clone(), json!({ "limit": limit(ctx, m) })))
        .collect();
    let names = agent_names(ctx);
    let mut agent = Map::new();
    for (name, role, _) in BUILTIN {
        if c.roles.contains_key(role) {
            let mut a = json!({ "model": format!("zygo/{role}") });
            if names.edit_deny.contains(name) {
                a["permission"] = json!({ "edit": "deny" });
            }
            agent.insert(name.into(), a);
        }
    }
    let mech = mechanics(&ctx.view);
    let roles = c.roles.iter().map(|(k, r)| (k, &r.description, &r.prompt)).chain(c.custom_roles.iter().map(|(k, r)| (k, &r.description, &r.prompt)));
    for (role, description, prompt) in roles.filter(|(k, _, _)| **k != c.main_role) {
        let key = format!("zygo-{role}");
        let mut a = json!({ "mode": "subagent", "description": description, "prompt": format!("{prompt} {mech}"), "model": format!("zygo/{role}") });
        if names.edit_deny.contains(&key) {
            a["permission"] = json!({ "edit": "deny" });
        }
        agent.insert(key, a);
    }
    agent.extend(d.agents.clone());
    let mut mcp = Map::new();
    mcp.insert("zygo".into(), json!({ "type": "local", "command": [zygo, "mcp", "--agent", "opencode"] }));
    mcp.extend(d.mcp.clone());
    let small = if c.roles.contains_key("read") { "read" } else { &c.main_role };
    let mut cfg = json!({
        "$schema": "https://opencode.ai/config.json",
        "model": format!("zygo/{}", c.main_role),
        "small_model": format!("zygo/{small}"),
        "enabled_providers": ["zygo"],
        "disabled_providers": [],
        "provider": { "zygo": {
            "npm": "@ai-sdk/anthropic",
            "options": {
                "baseURL": format!("{}/anthropic/v1", ctx.proxy_url.as_str().trim_end_matches('/')),
                "apiKey": key,
                "headers": headers,
            },
            "models": models,
        } },
        "agent": agent,
        "mcp": mcp,
    });
    if let Some((name, true)) = main_agent(ctx) {
        cfg["default_agent"] = json!(name);
    }
    if !d.commands.is_empty() {
        cfg["command"] = Value::Object(d.commands.clone());
    }
    cfg
}

const ROOT_VAR: &str = "${CLAUDE_PLUGIN_ROOT}";

/// A plugin's files as `(path, bytes, exec)`.
type Files = Vec<(String, Vec<u8>, bool)>;

/// The `---` frontmatter block at the start of `text`: where its inner text starts and ends, and where the body starts.
fn frontmatter(text: &str) -> Option<(usize, usize, usize)> {
    let line = |at: usize| text[at..].find('\n').map_or(text.len(), |i| at + i + 1);
    let is_fence = |a: usize, b: usize| text[a..b].trim_end_matches(['\r', '\n']) == "---";
    let first = line(0);
    if first == text.len() || !is_fence(0, first) {
        return None;
    }
    let mut at = first;
    while at < text.len() {
        let end = line(at);
        if is_fence(at, end) {
            return Some((first, at, end));
        }
        at = end;
    }
    None
}

fn unquote(v: &str) -> String {
    match (v.strip_prefix('"').and_then(|r| r.strip_suffix('"')), v.strip_prefix('\'').and_then(|r| r.strip_suffix('\''))) {
        (Some(d), _) => d.replace("\\\"", "\"").replace("\\\\", "\\"),
        (_, Some(s)) => s.to_string(),
        _ => v.to_string(),
    }
}

/// The frontmatter `description` and the text after the closing `---` line.
fn markdown(text: &str) -> (Option<String>, &str) {
    let Some((a, b, end)) = frontmatter(text) else { return (None, text) };
    let description = text[a..b].lines().find_map(|l| l.strip_prefix("description:")).map(|v| unquote(v.trim()));
    (description, &text[end..])
}

/// `text` with the top-level `name:` line of its frontmatter set to `name` (inserted when absent).
fn rewrite_skill_name(text: &str, name: &str) -> String {
    let Some((a, b, _)) = frontmatter(text) else { return format!("---\nname: {name}\n---\n{text}") };
    let mut out = text[..a].to_string();
    let eol = if text[..a].ends_with("\r\n") { "\r\n" } else { "\n" };
    let mut done = false;
    for l in text[a..b].split_inclusive('\n') {
        if !done && l.starts_with("name:") {
            out.push_str(&format!("name: {name}{}", &l[l.trim_end_matches(['\r', '\n']).len()..]));
            done = true;
        } else {
            out.push_str(l);
        }
    }
    if !done {
        out.insert_str(a, &format!("name: {name}{eol}"));
    }
    out.push_str(&text[b..]);
    out
}

/// The plugin files that OpenCode delivery uses, read from the store; `Err` = a file differs from its manifest.
fn read_plugin(ctx: &LaunchCtx, p: &zygo_plugins::snapshot::DeliveredPlugin) -> anyhow::Result<Files> {
    let used = |rel: &str| {
        ["skills/", "agents/", "commands/", "bin/", "scripts/"].iter().any(|d| rel.starts_with(d))
            || matches!(rel, ".mcp.json" | "opencode/plugin.js" | ".claude-plugin/plugin.json")
    };
    p.entry
        .files
        .iter()
        .filter(|(rel, _)| used(rel))
        .map(|(rel, fe)| Ok((rel.clone(), store::read_verified(&ctx.paths.plugin_store, &p.entry, rel)?, fe.exec)))
        .collect()
}

fn homepage(files: &Files) -> Option<String> {
    let (_, b, _) = files.iter().find(|(r, _, _)| r == ".claude-plugin/plugin.json")?;
    serde_json::from_slice::<Value>(b).ok()?.get("homepage")?.as_str().map(str::to_string)
}

/// The skills, agents, commands and MCP servers of the embedded plugin and the delivered plugins in the OpenCode
/// forms of §15.3, written under `dir`; the first plugin to hold a key keeps it.
fn deliver(ctx: &LaunchCtx, dir: &Path) -> anyhow::Result<Delivery> {
    let mut d = Delivery::default();
    let delivered = match &ctx.plugins {
        Plugins::Delivered(Some(x)) => Some(x),
        _ => None,
    };
    let Some(x) = delivered else {
        d.partial = true;
        install_embedded(dir, &mut d)?;
        return Ok(d);
    };
    d.lines.extend(x.lines.iter().cloned());
    d.log.extend(x.details.iter().cloned());
    d.partial = !x.details.is_empty();
    let mut claimed: BTreeMap<(&'static str, String), String> = BTreeMap::new();
    install_embedded_claiming(dir, &mut d, &mut claimed)?;
    let search = ctx.env.get("PATH");
    for p in &x.plugins {
        let Ok(files) = read_plugin(ctx, p) else {
            d.log.push(format!("plugin {}: store copy differs from its manifest; run zygo plugins sync", p.id));
            d.partial = true;
            continue;
        };
        install_plugin(dir, &p.id.to_string(), &p.id.plugin, &files, search, &mut claimed, &mut d)?;
    }
    Ok(d)
}

fn install_embedded(dir: &Path, d: &mut Delivery) -> anyhow::Result<()> {
    install_embedded_claiming(dir, d, &mut BTreeMap::new())
}

fn install_embedded_claiming(dir: &Path, d: &mut Delivery, claimed: &mut BTreeMap<(&'static str, String), String>) -> anyhow::Result<()> {
    let files: Files = embedded::files().into_iter().filter(|(r, _)| r.starts_with("skills/")).map(|(r, b)| (r, b, false)).collect();
    install_skills(dir, "embedded:zygo", embedded::NAME, &files, claimed, d)
}

/// Takes `key` of `kind` for `id`; `false` (and a launch line) when another plugin holds it.
fn claim(claimed: &mut BTreeMap<(&'static str, String), String>, d: &mut Delivery, kind: &'static str, key: &str, id: &str) -> bool {
    match claimed.get(&(kind, key.to_string())) {
        Some(from) => {
            d.lines.push(format!("plugin {id}: {kind} {} already delivered from {from}", zygo_core::esc(key)));
            false
        }
        None => {
            claimed.insert((kind, key.to_string()), id.to_string());
            true
        }
    }
}

fn install_skills(dir: &Path, id: &str, plugin: &str, files: &Files, claimed: &mut BTreeMap<(&'static str, String), String>, d: &mut Delivery) -> anyhow::Result<()> {
    let skills: BTreeSet<&str> = files.iter().filter_map(|(r, _, _)| r.strip_prefix("skills/")?.strip_suffix("/SKILL.md")).filter(|s| !s.contains('/')).collect();
    for skill in skills {
        let key = format!("{plugin}-{skill}");
        if !claim(claimed, d, "skill", &key, id) {
            continue;
        }
        let prefix = format!("skills/{skill}/");
        for (rel, bytes, exec) in files.iter().filter(|(r, _, _)| r.starts_with(&prefix)) {
            let rest = &rel[prefix.len()..];
            let dest = format!("skills/{key}/{rest}");
            let bytes = if rest == "SKILL.md" { rewrite_skill_name(&String::from_utf8_lossy(bytes), &key).into_bytes() } else { bytes.clone() };
            write_file(dir, &dest, &bytes, *exec)?;
        }
    }
    Ok(())
}

fn install_plugin(
    dir: &Path,
    id: &str,
    plugin: &str,
    files: &Files,
    search: Option<&std::ffi::OsStr>,
    claimed: &mut BTreeMap<(&'static str, String), String>,
    d: &mut Delivery,
) -> anyhow::Result<()> {
    install_skills(dir, id, plugin, files, claimed, d)?;
    for (kind, folder) in [("agent", "agents"), ("command", "commands")] {
        for (rel, bytes, _) in files {
            let Some(name) = rel.strip_prefix(folder).and_then(|r| r.strip_prefix('/')).and_then(|f| f.strip_suffix(".md")).filter(|n| !n.contains('/')) else { continue };
            let key = format!("{plugin}-{name}");
            if !claim(claimed, d, kind, &key, id) {
                continue;
            }
            let text = String::from_utf8_lossy(bytes);
            let (description, body) = markdown(&text);
            let mut v = Map::new();
            if kind == "agent" {
                v.insert("mode".into(), json!("subagent"));
            }
            if let Some(x) = description {
                v.insert("description".into(), json!(x));
            }
            v.insert(if kind == "agent" { "prompt" } else { "template" }.into(), json!(body));
            if kind == "agent" { &mut d.agents } else { &mut d.commands }.insert(key, Value::Object(v));
        }
    }
    if let Some((_, bytes, exec)) = files.iter().find(|(r, _, _)| r == "opencode/plugin.js") {
        write_file(dir, &format!("plugins/plugin-{plugin}.js"), bytes, *exec)?;
    }
    let root = dir.join("plugin-root").join(plugin);
    for (rel, bytes, exec) in files.iter().filter(|(r, _, _)| r.starts_with("bin/") || r.starts_with("scripts/")) {
        write_file(&root, rel, bytes, *exec)?;
    }
    let Some((_, mcp, _)) = files.iter().find(|(r, _, _)| r == ".mcp.json") else { return Ok(()) };
    let Ok(v) = serde_json::from_slice::<Value>(mcp) else { return Ok(()) };
    let servers = v.get("mcpServers").filter(|s| s.is_object()).unwrap_or(&v);
    let home = homepage(files);
    // A verbatim root (`\\?\C:\…`) does not read `/` as a separator: hand out the plain form.
    let here = zygo_core::paths::plain(&root).to_string_lossy().into_owned();
    let sub = |s: &str| s.replace(ROOT_VAR, &here);
    let mut missing: Vec<&str> = vec![];
    for (name, s) in servers.as_object().into_iter().flatten() {
        let Some(command) = s.get("command").and_then(Value::as_str) else { continue };
        if !command_found(command, &root, search) {
            if !missing.contains(&command) {
                missing.push(command);
                d.lines.push(match &home {
                    Some(h) => format!("plugin {id}: {} not found ({})", zygo_core::esc(command), zygo_core::esc(h)),
                    None => format!("plugin {id}: {} not found", zygo_core::esc(command)),
                });
            }
            continue;
        }
        if !claim(claimed, d, "mcp", &format!("{plugin}-{name}"), id) {
            continue;
        }
        let args = s.get("args").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        let mut e = Map::from_iter([("type".to_string(), json!("local")), ("command".to_string(), json!(std::iter::once(command).chain(args).map(&sub).collect::<Vec<_>>()))]);
        let env: Map<String, Value> = s.get("env").and_then(Value::as_object).into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), json!(sub(v.as_str()?))))).collect();
        if !env.is_empty() {
            e.insert("environment".into(), Value::Object(env));
        }
        d.mcp.insert(format!("{plugin}-{name}"), Value::Object(e));
    }
    Ok(())
}

/// The role an OpenCode agent name stands for.
fn role_of_agent(name: &str) -> &str {
    BUILTIN.iter().find(|(n, _, _)| *n == name).map_or_else(|| name.strip_prefix("zygo-").unwrap_or(name), |(_, role, _)| role)
}

impl AgentAdapter for OpenCode {
    fn agent(&self) -> Agent {
        Agent::OpenCode
    }
    fn program(&self) -> &str {
        "opencode"
    }
    fn install_hint(&self) -> &str {
        "zygo: opencode was not found on PATH; install OpenCode: https://opencode.ai/docs/"
    }
    fn picked_model(&self, args: &[String]) -> Option<String> {
        let (mut model, mut agent): (Option<&str>, Option<&str>) = (None, None);
        let mut it = args.iter().map(String::as_str);
        while let Some(a) = it.next() {
            match a {
                "--" => break,
                "-m" | "--model" => model = model.or(it.next()),
                "--agent" => agent = agent.or(it.next()),
                _ => {
                    if let Some(v) = a.strip_prefix("--model=") {
                        model = model.or(Some(v));
                    } else if let Some(v) = a.strip_prefix("--agent=") {
                        agent = agent.or(Some(v));
                    }
                }
            }
        }
        match (model, agent) {
            (Some(m), _) => Some(m.strip_prefix("zygo/").unwrap_or(m).to_string()),
            (None, Some(a)) => Some(role_of_agent(a).to_string()),
            _ => None,
        }
    }
    fn build(&self, ctx: &LaunchCtx) -> anyhow::Result<ChildSpec> {
        let zygo = self.zygo.to_str().filter(|z| !z.is_empty()).ok_or_else(|| anyhow!("cannot find the path of the running zygo binary"))?;
        ctx.session_dir.root.to_str().ok_or_else(|| anyhow!("cannot use the session directory path: it is not valid UTF-8"))?;
        let key = token_ref(&ctx.session_dir.token())?;
        let dir = ctx.session_dir.root.join("opencode");
        std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
        let path = serde_json::to_string(zygo)?;
        let plugins = dir.join("plugins");
        write_file(&plugins, "zygo.js", PLUGIN.replacen("__ZYGO_PATH__", &path, 1).as_bytes(), false)?;
        if matches!(ctx.plugins, Plugins::Snapshot(_)) {
            bail!("OpenCode takes delivered plugins, not a snapshot");
        }
        let mut d = deliver(ctx, &dir)?;
        let mut log = std::mem::take(&mut d.log);
        if main_agent(ctx).is_none_or(|(_, primary)| !primary) {
            log.push(format!("lead guard: no primary OpenCode agent maps to main_role {}", zygo_plugins::check::esc(&ctx.config.main_role)));
        }
        let mut lines = std::mem::take(&mut d.lines);
        if d.partial {
            lines.push("plugins: partial".into());
        }
        let cfg = inline_config(ctx, zygo, &d, &key);
        let mut args = ctx.args.clone();
        if ctx.yolo
            && let Some(name) = place_auto(&mut args)
        {
            let (why, line) = match name {
                None => ("after --".to_string(), "zygo: --yolo has no effect for opencode (args start with --)".to_string()),
                Some(n) => (format!("opencode {n} has no --auto"), format!("zygo: --yolo has no effect for opencode {n}")),
            };
            log.push(format!("degraded: --yolo ({why})"));
            lines.push(line);
        }
        Ok(ChildSpec {
            args,
            env: vec![("OPENCODE_CONFIG_CONTENT".into(), cfg.to_string()), ("OPENCODE_CONFIG_DIR".into(), dir.to_string_lossy().into_owned())],
            lines,
            log,
        })
    }
    fn preflight(&self, ctx: &LaunchCtx) -> Result<(), Preflight> {
        let names = agent_names(ctx);
        for file in config_files(ctx) {
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let Ok(v) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else { continue };
            if let Some(key) = conflict(&v, &names) {
                return Err(Preflight { file, key });
            }
        }
        Ok(())
    }
    fn has_session_arg(&self, args: &[String]) -> bool {
        zygo_workspace::args::has_session_arg(Agent::OpenCode, args)
    }
    /// `--session=<id>`: one token, so an id that starts with `-` stays the value; OpenCode 1.19.6 binds the `=` form
    /// to the session option (probe 2026-11-22, AU3).
    fn resume_args(&self, id: &str) -> Vec<String> {
        vec![format!("--session={id}")]
    }
}

/// OpenCode 1.18.16 subcommands other than `run` and their aliases (`opencode --help`); none accepts `--auto`.
const NO_AUTO: [&str; 22] = [
    "completion", "acp", "mcp", "attach", "debug", "providers", "auth", "agent", "upgrade", "uninstall", "serve", "web", "models", "stats",
    "export", "import", "github", "pr", "session", "plugin", "plug", "db",
];

/// Puts `--auto` where OpenCode 1.18.16 accepts it: after `run`; first for the TUI; nowhere for any other subcommand or
/// when a `--` comes before any subcommand. Returns `Some(subcommand)` when it is left out (`None` for the `--` case).
fn place_auto(args: &mut Vec<String>) -> Option<Option<String>> {
    let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
    match args[..end].iter().position(|a| a == "run" || NO_AUTO.contains(&a.as_str())) {
        Some(i) if args[i] == "run" => args.insert(i + 1, "--auto".into()),
        Some(i) => return Some(Some(args[i].clone())),
        None if end < args.len() => return Some(None),
        None => args.insert(0, "--auto".into()),
    }
    None
}

/// Writes `rel` (`/`-separated) under `dest`, creating parents; mode 0755 or 0644 on unix.
fn write_file(dest: &Path, rel: &str, bytes: &[u8], exec: bool) -> anyhow::Result<()> {
    let p = rel.split('/').fold(dest.to_path_buf(), |p, s| p.join(s));
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).with_context(|| d.display().to_string())?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, if exec { 0o755 } else { 0o644 });
    let mut f = o.open(&p).with_context(|| p.display().to_string())?;
    f.write_all(bytes).with_context(|| p.display().to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 })).with_context(|| p.display().to_string())?;
    }
    #[cfg(not(unix))]
    let _ = exec;
    Ok(())
}

/// The OpenCode config files that can set what zygo sets: the user's files, then every directory from `cwd` up to
/// the first one with a `.git` entry.
fn config_files(ctx: &LaunchCtx) -> Vec<PathBuf> {
    let var = |n: &str| ctx.env.get(n).filter(|v| !v.is_empty()).map(PathBuf::from);
    let user = var("XDG_CONFIG_HOME").or_else(|| var("HOME").or_else(|| var("USERPROFILE")).map(|h| h.join(".config"))).map(|c| c.join("opencode"));
    let mut files: Vec<PathBuf> = user.into_iter().flat_map(|d| ["opencode.json", "opencode.jsonc"].map(|f| d.join(f))).collect();
    let dirs: Vec<&Path> = match ctx.cwd.ancestors().position(|d| d.join(".git").exists()) {
        Some(i) => ctx.cwd.ancestors().take(i + 1).collect(),
        None => vec![ctx.cwd.as_path()],
    };
    for dir in dirs {
        files.extend(["opencode.json", "opencode.jsonc", ".opencode/opencode.json", ".opencode/opencode.jsonc"].map(|f| dir.join(f)));
    }
    files
}

/// The first key of `v` that zygo sets (§8.7 preflight).
fn conflict(v: &Value, names: &AgentNames) -> Option<String> {
    if v.pointer("/provider/zygo").is_some() {
        return Some("provider.zygo".into());
    }
    for k in ["model", "small_model"] {
        if v.get(k).is_some() {
            return Some(k.into());
        }
    }
    let agent = |n: &String, tail: &str| v.pointer(&format!("/agent/{n}{tail}")).is_some();
    names.all.iter().find(|n| agent(n, "/model")).map(|n| format!("agent.{n}.model")).or_else(|| {
        names.edit_deny.iter().find(|n| agent(n, "/permission/edit")).map(|n| format!("agent.{n}.permission.edit"))
    })
}

/// `text` without `//` and `/* */` comments and trailing commas, both outside strings.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut in_str, mut esc) = (false, false);
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while chars.next_if(|n| *n != '\n').is_some() {}
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            '}' | ']' => {
                let t = out.trim_end().len();
                if out[..t].ends_with(',') {
                    out.remove(t - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_windows_token_path_uses_slashes() {
        assert_eq!(token_ref(Path::new(r"C:\h\a b\S1\token")).unwrap(), "{file:C:/h/a b/S1/token}");
        assert_eq!(token_ref(Path::new(r"\\srv\share\S1\token")).unwrap(), "{file://srv/share/S1/token}");
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_token_path_without_a_drive_is_refused() {
        for p in [r"\h\S1\token", "/h/S1/token", r"C:h\S1\token"] {
            let e = token_ref(Path::new(p)).unwrap_err().to_string();
            assert!(e.contains("not absolute"), "{p}: {e}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_unix_token_path_is_kept_and_a_backslash_is_refused() {
        assert_eq!(token_ref(Path::new("/h/a b/S1/token")).unwrap(), "{file:/h/a b/S1/token}");
        let e = token_ref(Path::new(r"/h/a\b/token")).unwrap_err().to_string();
        assert!(e.contains("cannot reference the session token file") && e.contains("its path holds"), "{e}");
    }
}
