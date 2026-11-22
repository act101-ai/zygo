//! Claude Code adapter (§8.3-§8.5, §12.3).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};
use serde_json::{json, Map, Value};
use zygo_core::Agent;
use zygo_hooks::session_text::mechanics;
use zygo_proxy::session::SessionConfig;
use zygo_work::policy::Policy;

use crate::adapter::{AgentAdapter, ChildSpec, LaunchCtx, Plugins, Preflight};

const HINT: &str = "zygo: claude not found on PATH; install Claude Code: https://code.claude.com/docs/en/setup";
const PT_MODEL: &str = "--passthrough sends model names to the vendor; use a vendor model id";
const PT_BARE: &str = "--passthrough does not work with --bare: bare mode reads only API keys";
const PT_MANAGED: &str = "passthrough needs your claude.ai login; a managed setting replaces it";
const BAD_AGENTS: &str = "--agents: not a JSON object (or, with --print, a file that holds one)";
const SETTINGS_WARN: &str = "zygo: your --settings replaces zygo's settings file; the lead guard is off";
const NO_EDIT: [&str; 3] = ["Edit", "Write", "NotebookEdit"];

#[cfg(target_os = "macos")]
const MANAGED: &str = "/Library/Application Support/ClaudeCode/managed-settings.json";
#[cfg(windows)]
const MANAGED: &str = r"C:\Program Files\ClaudeCode\managed-settings.json";
#[cfg(not(any(target_os = "macos", windows)))]
const MANAGED: &str = "/etc/claude-code/managed-settings.json";

pub struct ClaudeCode {
    pub program: String,
    pub managed_settings: PathBuf,
    pub zygo: PathBuf,
}

impl Default for ClaudeCode {
    fn default() -> Self {
        ClaudeCode { program: "claude".into(), managed_settings: MANAGED.into(), zygo: std::env::current_exe().unwrap_or_default() }
    }
}

/// `p` as one shell word: single-quoted on unix (`'` written as `'\''`), double-quoted on Windows.
pub fn shell_quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) { format!("\"{s}\"") } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

/// The values of `name <v>` and `name=<v>`, in order.
fn flag_values(args: &[String], name: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            out.extend(it.next().cloned());
        } else if let Some(v) = a.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
            out.push(v.into());
        }
    }
    out
}

/// `args` without `name <v>` and `name=<v>`.
fn without_flag(args: &[String], name: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            it.next();
        } else if !a.strip_prefix(name).is_some_and(|r| r.starts_with('=')) {
            out.push(a.clone());
        }
    }
    out
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name || a.strip_prefix(name).is_some_and(|r| r.starts_with('=')))
}

fn strip_1m(v: &str) -> &str {
    v.strip_suffix("[1m]").unwrap_or(v)
}

/// The user's `--agents` values merged in order; `None` when one is not a JSON object (with `-p`/`--print`, a value
/// that is not JSON is read as a file that holds one).
fn user_agents(ctx: &LaunchCtx) -> Option<Map<String, Value>> {
    let print = ctx.args.iter().any(|a| a == "-p" || a == "--print");
    let mut out = Map::new();
    for v in flag_values(&ctx.args, "--agents") {
        let parsed = match serde_json::from_str::<Value>(&v) {
            Ok(x) => x,
            Err(_) if print => serde_json::from_slice(&std::fs::read(ctx.cwd.join(&v)).ok()?).ok()?,
            Err(_) => return None,
        };
        let Value::Object(m) = parsed else { return None };
        out.extend(m);
    }
    Some(out)
}

/// The role a Claude Code model alias names (§8.5).
fn alias_role(v: &str) -> Option<&'static str> {
    match v {
        "fable" | "best" => Some("design"),
        "opus" | "opusplan" => Some("lead"),
        "sonnet" => Some("code"),
        "haiku" => Some("read"),
        _ => None,
    }
}

/// `role` when enabled, else the first enabled of `lead`, `code`, `read`, else `main_role` (§8.5).
fn alias(cfg: &SessionConfig, role: &str) -> String {
    [role, "lead", "code", "read"].into_iter().find(|r| cfg.roles.contains_key(*r)).unwrap_or(cfg.main_role.as_str()).to_string()
}

fn env_rows(ctx: &LaunchCtx) -> Vec<(String, String)> {
    let c = &ctx.config;
    let mut headers = vec![format!("X-Zygo-Session: {}", ctx.session)];
    if !ctx.agent_version.is_empty() {
        headers.push(format!("X-Zygo-Agent-Version: {}", ctx.agent_version));
    }
    if let Some(r) = &ctx.repo {
        headers.push(format!("X-Zygo-Repo: {r}"));
    }
    if let Some(w) = &ctx.work {
        headers.push(format!("X-Zygo-Work: {}:{}", w.source, w.id));
    }
    if ctx.passthrough {
        headers.push(format!("X-Zygo-Token: {}", ctx.token));
    }
    let base = ("ANTHROPIC_BASE_URL", format!("{}/anthropic", ctx.proxy_url.as_str().trim_end_matches('/')));
    let hint = ("CLAUDE_CODE_GATEWAY_HINT_HEADERS", "1".to_string());
    let custom = ("ANTHROPIC_CUSTOM_HEADERS", headers.join("\n"));
    let rows = if ctx.passthrough {
        vec![base, hint, custom]
    } else {
        let fable = if c.roles.contains_key("design") { "design[1m]".into() } else { alias(c, "design") };
        vec![
            base,
            ("ANTHROPIC_MODEL", c.main_role.clone()),
            ("ANTHROPIC_DEFAULT_FABLE_MODEL", fable),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", alias(c, "lead")),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", alias(c, "code")),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", alias(c, "read")),
            ("CLAUDE_CODE_SUBAGENT_MODEL", alias(c, "code")),
            hint,
            custom,
            ("DISABLE_LOGIN_COMMAND", "1".into()),
            ("DISABLE_LOGOUT_COMMAND", "1".into()),
        ]
    };
    rows.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// A settings file as a JSON object; `None` when absent, unreadable or not an object.
fn read_object(p: &Path) -> Option<Map<String, Value>> {
    match serde_json::from_slice(&std::fs::read(p).ok()?).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// A name that outranks or bypasses the session's key helper (§8.4).
fn credential(name: &str, fold: bool) -> bool {
    let n = if fold { name.to_ascii_uppercase() } else { name.to_string() };
    matches!(n.as_str(), "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_BASE_URL") || n.starts_with("CLAUDE_CODE_USE_")
}

fn sorted(m: &Map<String, Value>) -> Vec<&String> {
    let mut k: Vec<&String> = m.keys().collect();
    k.sort();
    k
}

/// The first key of a settings file that stops the launch: top-level keys, then `env` names, each sorted.
fn conflict(s: &Map<String, Value>, helper: bool, managed: bool) -> Option<String> {
    for k in sorted(s) {
        let hook_off = managed && matches!(k.as_str(), "disableAllHooks" | "allowManagedHooksOnly") && s[k] == Value::Bool(true);
        if credential(k, false) || (helper && k == "apiKeyHelper") || hook_off {
            return Some(k.clone());
        }
    }
    let env = s.get("env").and_then(Value::as_object)?;
    sorted(env).into_iter().find(|k| credential(k, cfg!(windows))).map(|k| format!("env.{k}"))
}

/// Writes `bytes` to `p`, mode 0600 on unix, set before any byte is written.
fn write_private(p: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(p).with_context(|| p.display().to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600)).with_context(|| p.display().to_string())?;
    }
    f.set_len(0).with_context(|| p.display().to_string())?;
    f.write_all(bytes).with_context(|| p.display().to_string())
}

fn agents(ctx: &LaunchCtx, user: Map<String, Value>) -> Value {
    let c = &ctx.config;
    let mech = mechanics(&ctx.view);
    let cap = Policy::resolve(&ctx.cfg, ctx.repo_root.as_deref().unwrap_or(&ctx.cwd)).turn_cap();
    let roles = c
        .roles
        .iter()
        .map(|(k, r)| (k, &r.description, &r.prompt, r.writes))
        .chain(c.custom_roles.iter().map(|(k, r)| (k, &r.description, &r.prompt, r.writes)));
    let mut out = Map::new();
    for (role, description, prompt, writes) in roles.filter(|(k, ..)| **k != c.main_role) {
        let mut a = json!({ "description": description, "prompt": format!("{prompt} {mech}") });
        if !ctx.passthrough {
            a["model"] = json!(role);
        }
        if !writes {
            a["disallowedTools"] = json!(NO_EDIT);
        }
        if let Some(n) = cap {
            a["maxTurns"] = json!(n);
        }
        out.insert(format!("zygo-{role}"), a);
    }
    out.extend(user);
    Value::Object(out)
}

/// The status line `refreshInterval` in seconds.
const REFRESH_INTERVAL: u64 = 30;

/// [`REFRESH_INTERVAL`]. Test seam (P-HUD-1): `ZYGO_TEST_REFRESH_INTERVAL` in the launch environment, a positive
/// integer of seconds, replaces it; any other value keeps the default. The code that reads the variable exists only
/// under the `test-hooks` feature.
#[cfg(feature = "test-hooks")]
fn refresh_interval(ctx: &LaunchCtx) -> u64 {
    ctx.env
        .get("ZYGO_TEST_REFRESH_INTERVAL")
        .and_then(|v| v.to_str()?.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(REFRESH_INTERVAL)
}

#[cfg(not(feature = "test-hooks"))]
fn refresh_interval(_ctx: &LaunchCtx) -> u64 {
    REFRESH_INTERVAL
}

impl ClaudeCode {
    fn hooks(&self, ctx: &LaunchCtx) -> Value {
        let z = shell_quote(&self.zygo);
        let entry = |matcher: Option<&str>, name: &str| {
            let hooks = json!([{ "type": "command", "command": format!("{z} hook {name}") }]);
            match matcher {
                Some(m) => json!({ "matcher": m, "hooks": hooks }),
                None => json!({ "hooks": hooks }),
            }
        };
        let mut pre = vec![];
        if ctx.effective_role.as_deref() == Some(ctx.config.main_role.as_str()) {
            pre.push(entry(Some("Edit|Write|NotebookEdit"), "role-guard"));
        }
        pre.push(entry(Some("Edit|Write|NotebookEdit|Read|Bash"), "guard"));
        let mut post = vec![entry(Some("Bash|Read|Grep|Glob|WebFetch"), "trim")];
        let mut h = Map::new();
        if ctx.view.work_units {
            pre.push(entry(Some("mcp__zygo__work_begin"), "work --event begin"));
            pre.push(entry(Some("mcp__zygo__work_verdict"), "work --event verdict"));
            post.push(entry(Some("*"), "work"));
            h.insert("PostToolUseFailure".into(), json!([entry(Some("*"), "work")]));
            h.insert("SubagentStop".into(), json!([entry(None, "work")]));
        }
        h.insert("PreToolUse".into(), json!(pre));
        h.insert("PostToolUse".into(), json!(post));
        h.insert("SessionStart".into(), json!([entry(None, "session-start")]));
        h.insert("UserPromptSubmit".into(), json!([entry(None, "prompt")]));
        h.insert("PreCompact".into(), json!([entry(None, "pre-compact")]));
        Value::Object(h)
    }

    fn settings(&self, ctx: &LaunchCtx, env: &[(String, String)]) -> Value {
        let z = shell_quote(&self.zygo);
        let env: Map<String, Value> = env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        let mut s = json!({
            "disableAllHooks": false,
            "statusLine": { "type": "command", "command": format!("{z} hook statusline"), "refreshInterval": refresh_interval(ctx) },
            "hooks": self.hooks(ctx),
            "env": env,
        });
        if !ctx.passthrough {
            let c = &ctx.config;
            let rows: Vec<(&String, &String, &String)> = c
                .roles
                .iter()
                .map(|(k, r)| (k, &r.model, &r.description))
                .chain(c.custom_roles.iter().map(|(k, r)| (k, &r.model, &r.description)))
                .collect();
            let options: Vec<Value> = rows
                .iter()
                .map(|(k, model, description)| {
                    let mut o = json!({ "model": k, "label": k, "description": description });
                    if model.starts_with("claude-") {
                        o["behavesAs"] = json!(model);
                    }
                    o
                })
                .collect();
            s["apiKeyHelper"] = json!(format!("{z} hook token"));
            s["model"] = json!(c.main_role);
            s["availableModels"] = json!(rows.iter().map(|(k, ..)| k).collect::<Vec<_>>());
            s["modelPicker"] = json!({ "options": options });
        }
        s
    }

    /// The settings files of §8.4 in the order read, each with whether it is the managed file.
    fn settings_files(&self, ctx: &LaunchCtx) -> Vec<(PathBuf, bool)> {
        let var = |n: &str| ctx.env.get(n).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let user = var("CLAUDE_CONFIG_DIR").or_else(|| var(home).map(|h| h.join(".claude")));
        let mut files: Vec<(PathBuf, bool)> = user.map(|d| (d.join("settings.json"), false)).into_iter().collect();
        let mut dirs = vec![ctx.cwd.as_path()];
        dirs.extend(ctx.repo_root.as_deref().filter(|r| *r != ctx.cwd));
        for d in dirs {
            files.extend(["settings.json", "settings.local.json"].map(|f| (d.join(".claude").join(f), false)));
        }
        files.push((self.managed_settings.clone(), true));
        files
    }
}

impl AgentAdapter for ClaudeCode {
    fn agent(&self) -> Agent {
        Agent::ClaudeCode
    }
    fn program(&self) -> &str {
        &self.program
    }
    fn install_hint(&self) -> &str {
        HINT
    }
    fn picked_model(&self, args: &[String]) -> Option<String> {
        let v = flag_values(args, "--model").pop()?;
        Some(alias_role(strip_1m(&v)).unwrap_or(strip_1m(&v)).to_string())
    }
    fn picked_role(&self, args: &[String], cfg: &SessionConfig) -> Option<String> {
        let v = flag_values(args, "--model").pop()?;
        Some(alias_role(strip_1m(&v)).map_or_else(|| strip_1m(&v).to_string(), |r| alias(cfg, r)))
    }
    fn build(&self, ctx: &LaunchCtx) -> anyhow::Result<ChildSpec> {
        let zygo = self.zygo.to_str().filter(|z| !z.is_empty()).ok_or_else(|| anyhow!("cannot find the path of the zygo binary"))?;
        ctx.session_dir.root.to_str().ok_or_else(|| anyhow!("cannot use the session directory path: it is not valid UTF-8"))?;
        let sd = &ctx.session_dir;
        let env = env_rows(ctx);
        let settings = sd.settings_json();
        write_private(&settings, self.settings(ctx, &env).to_string().as_bytes())?;
        let mcp = sd.root.join("mcp.json");
        let servers = json!({ "mcpServers": { "zygo": { "command": zygo, "args": ["mcp", "--agent", "claude-code"] } } });
        std::fs::write(&mcp, servers.to_string()).with_context(|| mcp.display().to_string())?;
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let mut args = vec![
            "--mcp-config".into(),
            path(&mcp),
            "--settings".into(),
            path(&settings),
            "--agents".into(),
            agents(ctx, user_agents(ctx).unwrap_or_default()).to_string(),
        ];
        if let Plugins::Snapshot(s) = &ctx.plugins
            && !s.plugin_dirs().is_empty()
        {
            args.extend(["--plugin-dir".into(), path(&sd.root.join("plugins"))]);
        }
        if ctx.yolo {
            args.push("--dangerously-skip-permissions".into());
        }
        args.extend(without_flag(&ctx.args, "--agents"));
        let (mut lines, mut log) = (vec![], vec![]);
        if has_flag(&ctx.args, "--settings") {
            lines.push(SETTINGS_WARN.to_string());
            log.push("degraded: --settings".to_string());
        }
        if has_flag(&ctx.args, "--bare") {
            log.push("degraded: --bare".to_string());
        }
        if !ctx.passthrough
            && ctx.effective_role.is_none()
            && let Some(v) = flag_values(&ctx.args, "--model").pop()
        {
            log.push(format!("degraded: --model {v}"));
        }
        Ok(ChildSpec { args, env, lines, log })
    }
    fn preflight(&self, ctx: &LaunchCtx) -> Result<(), Preflight> {
        for (file, managed) in self.settings_files(ctx) {
            let Some(s) = read_object(&file) else { continue };
            if let Some(key) = conflict(&s, managed || ctx.passthrough, managed) {
                return Err(Preflight { file, key });
            }
        }
        Ok(())
    }
    fn refusal(&self, ctx: &LaunchCtx) -> Option<String> {
        let Some(user) = user_agents(ctx) else { return Some(BAD_AGENTS.into()) };
        if !ctx.passthrough {
            return None;
        }
        if has_flag(&ctx.args, "--bare") {
            return Some(PT_BARE.into());
        }
        let c = &ctx.config;
        let is_role = |m: &str| {
            let m = strip_1m(m.trim());
            ctx.cfg.config.role.contains_key(m) || c.roles.contains_key(m) || c.custom_roles.contains_key(m)
        };
        let fallback = flag_values(&ctx.args, "--fallback-model");
        let mut models = flag_values(&ctx.args, "--model")
            .into_iter()
            .chain(fallback.iter().flat_map(|v| v.split(',').map(str::to_string)))
            .chain(user.values().filter_map(|a| a.get("model")?.as_str().map(str::to_string)));
        if models.any(|m| is_role(&m)) {
            return Some(PT_MODEL.into());
        }
        let m = read_object(&self.managed_settings)?;
        let env = m.get("env").and_then(Value::as_object);
        let key = |k: &str| m.contains_key(k) || env.is_some_and(|e| e.keys().any(|n| if cfg!(windows) { n.eq_ignore_ascii_case(k) } else { n == k }));
        (m.contains_key("apiKeyHelper") || key("ANTHROPIC_API_KEY") || key("ANTHROPIC_AUTH_TOKEN")).then(|| PT_MANAGED.into())
    }
}
