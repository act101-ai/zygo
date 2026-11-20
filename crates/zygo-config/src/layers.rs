//! Layers 1–7 (§6.1) merged table by table, with each key's source layer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use toml::{Table, Value};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;

use crate::repo_file::{read_default_branch, RepoFile};
use crate::schema::{Approval, Config, GuardsConfig, KeyRef, Level, GUARDS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Builtin,
    Team,
    User,
    Repository,
    Profile,
    Flags,
    Session,
}

const LAYERS: [Layer; 7] =
    [Layer::Builtin, Layer::Team, Layer::User, Layer::Repository, Layer::Profile, Layer::Flags, Layer::Session];

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::Builtin => "builtin",
            Layer::Team => "team",
            Layer::User => "user",
            Layer::Repository => "repository",
            Layer::Profile => "profile",
            Layer::Flags => "flags",
            Layer::Session => "session",
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Layer {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        LAYERS.into_iter().find(|l| l.name() == s).ok_or_else(|| format!("unknown layer {s}"))
    }
}

#[derive(Debug, Clone)]
pub struct LoadContext {
    pub paths: Paths,
    pub repo: Option<PathBuf>,
    pub team: Option<Table>,
}

/// The warning for a user `[[model]]` in serve mode (§6.1: the server's catalog is authoritative).
pub const MODEL_IGNORED: &str = "config: [[model]] in user config ignored; the server's catalog is authoritative";

/// The team layer of a serve session, in its session directory (0600 on unix; no token). Written by the launcher; read
/// by the session's verbs and hooks (§14 note 2026-11-19).
pub const TEAM_FILE: &str = "team.toml";

/// `<dir>/team.toml`; `None` when it does not exist. No parser text in the error.
pub fn read_team(dir: &Path) -> anyhow::Result<Option<Table>> {
    match std::fs::read_to_string(dir.join(TEAM_FILE)) {
        Ok(s) => Ok(Some(s.parse().map_err(|_| anyhow::anyhow!("not valid TOML"))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone)]
pub struct Merged {
    pub config: Config,
    pub repo_file: RepoFile,
    pub warnings: Vec<String>,
    /// Every leaf key (source-key form) the team layer sets.
    pub team_keys: BTreeSet<String>,
    /// The team layer was loaded (`LoadContext.team` was `Some`), whether or not it sets any key.
    pub team_loaded: bool,
    /// The user layer as parsed; on the server, its config file.
    pub user_table: Table,
    /// The team layer as served (empty outside serve mode): `build` holds its floors after every merge (§6.1 note 2026-11-20).
    team_table: Table,
    table: Table,
    sources: BTreeMap<String, Layer>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{layer} config {path}: {message}")]
    Parse { layer: &'static str, path: String, message: String },
    #[error("{layer} config {path}: {what}")]
    Duplicate { layer: &'static str, path: String, what: String },
    #[error("profile {}: {message}", zygo_core::esc(.name))]
    Profile { name: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("refused: `{key}` is set by the team config")]
    TeamSet { key: String },
    #[error("refused: guard {guard} cannot go below min_level {min}")]
    BelowFloor { guard: String, min: Level },
    #[error("refused: {plugin} is required by the team config")]
    Required { plugin: String },
    #[error("refused: secret redaction cannot be turned off in a session")]
    SecretsOff,
    #[error("refused: unknown {what} `{value}`")]
    Unknown { what: &'static str, value: String },
}

// ---- source keys ----

/// One segment of a source key: bare, or a TOML basic string.
pub fn seg(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

pub(crate) fn join<S: AsRef<str>>(segs: &[S]) -> String {
    segs.iter().map(|s| seg(s.as_ref())).collect::<Vec<_>>().join(".")
}

/// Inverse of `join`.
fn split_key(key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = key.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => cur.extend(chars.next()),
            '.' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn text<'a>(t: &'a Table, k: &str) -> &'a str {
    t.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The value at `path` in `t`, through tables only.
fn value_at<'a>(t: &'a Table, path: &[&str]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut t = t;
    for s in parents {
        t = t.get(*s)?.as_table()?;
    }
    t.get(*last)
}

/// The key of an element of a keyed array of tables at `path`, or `None` when the array is not keyed.
pub(crate) fn array_key<S: AsRef<str>>(path: &[S], elem: &Table) -> Option<String> {
    let p: Vec<&str> = path.iter().map(AsRef::as_ref).collect();
    Some(match p.as_slice() {
        ["model"] => text(elem, "id").into(),
        ["marketplace"] | ["model", _, "provider"] => text(elem, "name").into(),
        ["schema_rule"] => format!("{}:{}", text(elem, "family"), text(elem, "code")),
        ["redaction", "pattern"] => text(elem, "id").into(),
        ["serve", "tokens"] => text(elem, "label").into(),
        _ => return None,
    })
}

// ---- merge ----

struct Rec<'a> {
    sources: &'a mut BTreeMap<String, Layer>,
    team_keys: &'a mut BTreeSet<String>,
    warnings: &'a mut Vec<String>,
}

impl Rec<'_> {
    fn source(&self, key: &str) -> Option<Layer> {
        self.sources.get(key).copied()
    }

    /// `path` was replaced by `layer` with `v`: record it, drop its descendants' sources.
    fn record(&mut self, path: &[String], layer: Layer, v: &Value) {
        let key = join(path);
        let prefix = format!("{key}.");
        self.sources.retain(|k, _| !k.starts_with(&prefix));
        self.sources.insert(key, layer);
        if layer == Layer::Team {
            let mut leaves = Vec::new();
            table_leaves(path.to_vec(), v, &mut leaves);
            self.team_keys.extend(leaves);
        }
    }
}

fn table_leaves(path: Vec<String>, v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Table(t) => {
            for (k, v) in t {
                let mut p = path.clone();
                p.push(k.clone());
                table_leaves(p, v, out);
            }
        }
        _ => out.push(join(&path)),
    }
}

fn merge(base: &mut Table, top: Table, layer: Layer, path: &[String], rec: &mut Rec) {
    for (k, v) in top {
        let mut p = path.to_vec();
        p.push(k.clone());
        if path.is_empty() && k == "profile" {
            // A profile replaces a lower one of the same name whole (§11.2).
            if let Value::Table(profiles) = v {
                let dst = base.entry("profile").or_insert_with(|| Value::Table(Table::new()));
                if !dst.is_table() {
                    *dst = Value::Table(Table::new());
                }
                for (name, pv) in profiles {
                    let mut pp = p.clone();
                    pp.push(name.clone());
                    rec.record(&pp, layer, &pv);
                    dst.as_table_mut().unwrap().insert(name, pv);
                }
                continue;
            }
        }
        if path == ["limits"] && k == "subagent_max_turns" {
            // The lower non-zero value wins (§13.5).
            let old = base.get(&k).and_then(Value::as_integer).unwrap_or(0);
            let new = v.as_integer().unwrap_or(0);
            if old != 0 && (new == 0 || old <= new) {
                continue;
            }
        }
        match (v, base.get_mut(&k)) {
            (Value::Table(t), Some(Value::Table(b))) => merge(b, t, layer, &p, rec),
            (Value::Table(t), None) => {
                let mut b = Table::new();
                merge(&mut b, t, layer, &p, rec);
                base.insert(k, Value::Table(b));
            }
            // A keyed array always merges by key: an empty one adds and removes nothing.
            (Value::Array(a), b) if keyed(&p) && a.iter().all(Value::is_table) => {
                if !matches!(b, Some(Value::Array(_))) {
                    base.insert(k.clone(), Value::Array(Vec::new()));
                }
                let Some(Value::Array(dst)) = base.get_mut(&k) else { unreachable!() };
                for (i, e) in a.into_iter().enumerate() {
                    let Value::Table(e) = e else { continue };
                    merge_elem(dst, e, i, layer, &p, rec);
                }
            }
            (v, _) => {
                rec.record(&p, layer, &v);
                base.insert(k, v);
            }
        }
    }
}

fn keyed(path: &[String]) -> bool {
    array_key(path, &Table::new()).is_some()
}

/// One element (index `i` in its layer) of a keyed array: same key replaces it whole in place, a new key appends.
fn merge_elem(dst: &mut Vec<Value>, mut e: Table, i: usize, layer: Layer, path: &[String], rec: &mut Rec) {
    let key = array_key(path, &e).unwrap_or_default();
    let mut ep = path.to_vec();
    ep.push(key.clone());
    let pos = dst.iter().position(|o| o.as_table().and_then(|o| array_key(path, o)).as_deref() == Some(&key));
    let Some(at) = pos else {
        rec.record(&ep, layer, &Value::Table(e.clone()));
        dst.push(Value::Table(e));
        return;
    };
    let old = dst[at].as_table().cloned().unwrap_or_default();
    let mut kept_team = Vec::new();
    if path == ["marketplace"] && layer == Layer::User {
        match rec.source(&join(&ep)) {
            Some(Layer::Builtin) => {
                // A user entry may move the built-in marketplace to another ref, not another source (§15.1).
                if e.get("source").is_some_and(|s| s.as_str() != Some(text(&old, "source"))) {
                    rec.warnings.push(format!(
                        "config: user marketplace[{i}]: a user entry cannot change the built-in source; ignored"
                    ));
                    return;
                }
                let t = dst[at].as_table_mut().unwrap();
                // A user ref alone moves the pin: the built-in commit does not stay (§15.1).
                let clear = e.contains_key("ref") && !e.contains_key("commit");
                for f in ["ref", "commit"] {
                    if let Some(v) = e.remove(f) {
                        let mut fp = ep.clone();
                        fp.push(f.into());
                        rec.record(&fp, layer, &v);
                        t.insert(f.into(), v);
                    }
                }
                if clear { t.remove("commit"); }
                return;
            }
            Some(Layer::Team) => {
                for f in ["required", "approvals"] {
                    match old.get(f) {
                        Some(v) => {
                            e.insert(f.into(), v.clone());
                            kept_team.push(f);
                        }
                        None => {
                            e.remove(f);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    rec.record(&ep, layer, &Value::Table(e.clone()));
    for f in kept_team {
        rec.sources.insert(join(&[ep.as_slice(), &[f.to_string()]].concat()), Layer::Team);
    }
    dst[at] = Value::Table(e);
}

// ---- load ----

fn check_duplicates(t: &Table, layer: &'static str, path: &str) -> Result<(), ConfigError> {
    dup_walk(&mut Vec::new(), "", t).map_err(|what| ConfigError::Duplicate { layer, path: path.into(), what: zygo_core::esc(&what) })
}

/// The first two elements of one keyed array, at any depth, that have the same key (a provider within a model
/// included), named by index: the key is a value from the file (C3).
fn dup_walk(path: &mut Vec<String>, shown: &str, t: &Table) -> Result<(), String> {
    for (k, v) in t {
        path.push(k.clone());
        let here = if shown.is_empty() { seg(k) } else { format!("{shown}.{}", seg(k)) };
        match v {
            Value::Table(sub) => dup_walk(path, &here, sub)?,
            Value::Array(a) if keyed(path) => {
                let mut seen = BTreeMap::new();
                for (i, e) in a.iter().enumerate() {
                    let Some(e) = e.as_table() else { continue };
                    let key = array_key(path, e).unwrap_or_default();
                    if let Some(first) = seen.insert(key.clone(), i) {
                        return Err(format!("{here}[{first}] and {here}[{i}] have the same {}", key_noun(path)));
                    }
                    path.push(key);
                    dup_walk(path, &format!("{here}[{i}]"), e)?;
                    path.pop();
                }
            }
            _ => {}
        }
        path.pop();
    }
    Ok(())
}

/// The field or fields `array_key` reads for the keyed array at `path`.
fn key_noun(path: &[String]) -> &'static str {
    let p: Vec<&str> = path.iter().map(String::as_str).collect();
    match p.as_slice() {
        ["marketplace"] | ["model", _, "provider"] => "name",
        ["schema_rule"] => "family and code",
        ["serve", "tokens"] => "label",
        _ => "id",
    }
}

fn parse_layer(t: &Table, layer: &'static str, path: &str, text: Option<&str>) -> Result<(), ConfigError> {
    check_duplicates(t, layer, path)?;
    crate::typed::from_value::<Config>(Value::Table(t.clone()))
        .map(drop)
        .map_err(|e| ConfigError::Parse { layer, path: path.into(), message: e.describe(text) })
}

/// The built-in layer (layer 1), from `builtin::defaults`.
fn builtin_layer() -> Table {
    Table::try_from(crate::builtin::defaults()).expect("built-in config serializes to TOML")
}

/// Layers 1–4.
pub fn load(ctx: &LoadContext) -> Result<Merged, ConfigError> {
    load_with_builtin(ctx, builtin_layer())
}

#[doc(hidden)]
pub fn load_with_builtin(ctx: &LoadContext, mut builtin: Table) -> Result<Merged, ConfigError> {
    let serve = ctx.team.is_some();
    let user_path = ctx.paths.config_file.display().to_string();
    let user_text: Option<String> = match std::fs::read_to_string(&ctx.paths.config_file) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => return Err(ConfigError::Io { path: ctx.paths.config_file.clone(), source }),
    };
    let mut user = match &user_text {
        Some(s) => s.parse::<Table>().map_err(|e| ConfigError::Parse {
            layer: "user",
            path: user_path.clone(),
            message: crate::repo_file::syntax_error(s, e.message(), e.span()),
        })?,
        None => Table::new(),
    };
    parse_layer(&builtin, "builtin", "(built-in)", None)?;
    if let Some(team) = &ctx.team {
        parse_layer(team, "team", "(team)", None)?;
    }
    parse_layer(&user, "user", &user_path, user_text.as_deref())?;
    // Each layer's pattern ids in file order: a pattern error names the pattern by its index in its own layer.
    let pattern_ids = |t: &Table| -> Vec<String> {
        let list = t.get("redaction").and_then(|r| r.get("pattern")).and_then(Value::as_array);
        list.into_iter().flatten().map(|e| e.get("id").and_then(Value::as_str).unwrap_or_default().to_string()).collect()
    };
    let layer_ids = [
        (Layer::Builtin, pattern_ids(&builtin)),
        (Layer::Team, ctx.team.as_ref().map(pattern_ids).unwrap_or_default()),
        (Layer::User, pattern_ids(&user)),
    ];

    let mut m = Merged {
        config: Config::default(),
        repo_file: RepoFile::Absent,
        warnings: Vec::new(),
        team_keys: BTreeSet::new(),
        team_loaded: ctx.team.is_some(),
        user_table: user.clone(),
        team_table: ctx.team.clone().unwrap_or_default(),
        table: Table::new(),
        sources: BTreeMap::new(),
    };
    if serve {
        // The server's catalog is authoritative (§6.1).
        builtin.remove("model");
        if user.remove("model").is_some() {
            m.warnings.push(MODEL_IGNORED.into());
        }
    }
    m.merge(builtin, Layer::Builtin);
    if let Some(team) = &ctx.team {
        m.merge(team.clone(), Layer::Team);
    }
    m.merge(user, Layer::User);

    if let Some(repo) = &ctx.repo {
        m.repo_file = read_default_branch(repo);
        match &m.repo_file {
            RepoFile::Absent | RepoFile::Unread { .. } => {}
            RepoFile::Invalid { why, .. } => m.warnings.push(format!(".zygo.toml: invalid ({why}); ignored")),
            RepoFile::Found { table, ignored, .. } => {
                if !ignored.is_empty() {
                    let names: Vec<String> = ignored.iter().map(|n| format!("[{}]", zygo_core::esc(n))).collect();
                    m.warnings.push(format!(
                        ".zygo.toml: {} ignored; the repository file may set only [guards], [navigation], [limits], [work]",
                        names.join(", ")
                    ));
                }
                let mut t = table.clone();
                t.remove("version");
                if let Some(g) = t.get_mut("guards").and_then(Value::as_table_mut)
                    && g.remove("min_level").is_some()
                {
                    m.warnings.push(".zygo.toml: guards.min_level ignored; only user or team config may set it".into());
                }
                m.merge(t, Layer::Repository);
            }
        }
    }
    m.config = m.build().map_err(|e| ConfigError::Parse { layer: "merged", path: "(merged)".into(), message: e.describe(None) })?;
    // Every pattern compiles, or the config is refused (§16.1).
    for (i, p) in m.config.redaction.pattern.iter().enumerate() {
        if let Err(e) = regex::Regex::new(&p.pattern) {
            let key = join(&["redaction", "pattern", p.id.as_str()]);
            let layer = m.source_of(&key);
            let path = match layer {
                Layer::Builtin => "(built-in)".into(),
                Layer::Team => "(team)".into(),
                Layer::User => user_path,
                _ => "(merged)".into(),
            };
            let at = layer_ids.iter().find(|(l, _)| *l == layer).and_then(|(_, ids)| ids.iter().position(|x| *x == p.id)).unwrap_or(i);
            let message = format!("redaction.pattern[{at}].pattern: does not compile: {}", crate::check::regex_error(&p.pattern, &e));
            return Err(ConfigError::Parse { layer: layer.name(), path, message });
        }
    }
    Ok(m)
}

impl Merged {
    fn merge(&mut self, top: Table, layer: Layer) {
        let mut rec = Rec { sources: &mut self.sources, team_keys: &mut self.team_keys, warnings: &mut self.warnings };
        merge(&mut self.table, top, layer, &[], &mut rec);
    }

    /// The team floors (§6.1 note 2026-11-20): `guards.min_level` not below the team's, `features.redaction.secrets`
    /// on when the team sets it on, and each team marketplace's `required` list kept. A floored value has the source `team`.
    fn hold_team_floors(&mut self) {
        let level = |v: Option<&Value>| v.and_then(Value::as_str).and_then(|s| s.parse::<Level>().ok());
        // A `guards` that is not a table is left for the typed build to refuse.
        let guards_table = self.table.get("guards").is_none_or(Value::is_table);
        if guards_table && let Some(min) = level(value_at(&self.team_table, &["guards", "min_level"])) {
            // A present value that is not a level is left for the typed build to refuse.
            let lower = match value_at(&self.table, &["guards", "min_level"]) {
                None => Level::default() < min,
                Some(v) => level(Some(v)).is_some_and(|l| l < min),
            };
            if lower {
                self.set("guards.min_level", Value::String(min.name().into()), Layer::Team);
            }
        }
        let secrets = ["features", "redaction", "secrets"];
        if value_at(&self.team_table, &secrets).and_then(Value::as_bool) == Some(true)
            && value_at(&self.table, &secrets).and_then(Value::as_bool) == Some(false)
        {
            self.set("features.redaction.secrets", Value::Boolean(true), Layer::Team);
        }
        // Each team marketplace: every entry of its name keeps the team's `required` and `approvals`, as `merge_elem`
        // does for the user layer; an entry that is gone is added back when the team `required` list is not empty.
        let team: Vec<Table> = value_at(&self.team_table, &["marketplace"])
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_table)
            .cloned()
            .collect();
        // A field whose value is not its type is left for the typed build to refuse.
        let typed = |f: &str, v: &Value| match f {
            "required" => v.clone().try_into::<Vec<String>>().is_ok(),
            _ => v.clone().try_into::<Vec<Approval>>().is_ok(),
        };
        for e in team {
            let name = text(&e, "name").to_string();
            // Not an array: the typed build refuses it, and the caller keeps its input.
            let Some(Value::Array(dst)) = self.table.get_mut("marketplace") else { return };
            let at: Vec<usize> = (0..dst.len()).filter(|&i| dst[i].as_table().is_some_and(|o| text(o, "name") == name)).collect();
            if at.is_empty() {
                if e.get("required").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
                    dst.push(Value::Table(e));
                    self.sources.insert(join(&["marketplace", name.as_str()]), Layer::Team);
                }
                continue;
            }
            let mut kept = BTreeSet::new();
            for i in at {
                let o = dst[i].as_table_mut().expect("found as a table");
                for f in ["required", "approvals"] {
                    if o.get(f).is_some_and(|v| !typed(f, v)) {
                        continue;
                    }
                    match e.get(f) {
                        Some(v) => {
                            o.insert(f.into(), v.clone());
                            kept.insert(f);
                        }
                        None => {
                            o.remove(f);
                        }
                    }
                }
            }
            for f in kept {
                self.sources.insert(join(&["marketplace", name.as_str(), f]), Layer::Team);
            }
        }
    }

    /// Each guard below `guards.min_level` is raised to it (§13.5).
    fn raise_to_floor(&mut self) {
        let level = |v: Option<&Value>| v.and_then(Value::as_str).and_then(|s| s.parse::<Level>().ok());
        let guards = self.table.get("guards").and_then(Value::as_table);
        let min = level(guards.and_then(|g| g.get("min_level"))).unwrap_or_default();
        let defaults = GuardsConfig::default();
        let low: Vec<&str> = GUARDS
            .into_iter()
            .filter(|g| level(guards.and_then(|t| t.get(*g))).or(defaults.get(g)).unwrap_or_default() < min)
            .collect();
        if low.is_empty() {
            return;
        }
        let src = self.source_of("guards.min_level");
        let t = self.table.entry("guards").or_insert_with(|| Value::Table(Table::new()));
        let t = t.as_table_mut().expect("guards is a table: the layer parsed");
        for g in low {
            t.insert(g.into(), Value::String(min.name().into()));
            self.sources.insert(format!("guards.{g}"), src);
        }
    }

    /// Every merged result passes here: the team floors, then each guard raised to `guards.min_level` (§13.5), then the typed config.
    fn build(&mut self) -> Result<Config, crate::typed::TypeError> {
        self.hold_team_floors();
        self.raise_to_floor();
        let mut c: Config = crate::typed::from_value(Value::Table(self.table.clone()))?;
        for m in &mut c.catalog.0 {
            m.from_team = self.sources.get(&join(&["model", m.id.as_str()])) == Some(&Layer::Team);
        }
        Ok(c)
    }

    /// Sets the dotted source key `key` to `v` with source `layer`.
    fn set(&mut self, key: &str, v: Value, layer: Layer) {
        let segs = split_key(key);
        let (last, parents) = segs.split_last().expect("split_key gives at least one segment");
        let mut t = &mut self.table;
        for s in parents {
            let e = t.entry(s.clone()).or_insert_with(|| Value::Table(Table::new()));
            if !e.is_table() {
                *e = Value::Table(Table::new());
            }
            t = e.as_table_mut().unwrap();
        }
        t.insert(last.clone(), v.clone());
        let mut rec = Rec { sources: &mut self.sources, team_keys: &mut self.team_keys, warnings: &mut self.warnings };
        rec.record(&segs, layer, &v);
    }

    /// The layer that set `key` (source-key form): exact, else the nearest recorded ancestor, else `Builtin`.
    pub fn source_of(&self, key: &str) -> Layer {
        let segs = split_key(key);
        (1..=segs.len()).rev().find_map(|n| self.sources.get(&join(&segs[..n])).copied()).unwrap_or(Layer::Builtin)
    }

    /// Layer 5: the profile's `features` table (§11.1, §15.4).
    pub fn apply_profile(&self, name: &str) -> Result<Merged, ConfigError> {
        let err = |message: String| ConfigError::Profile { name: name.into(), message };
        let p = self.config.profile.get(name).ok_or_else(|| err("not found".into()))?;
        let mut out = self.clone();
        if let Some(f) = &p.features {
            if f.get("redaction").and_then(Value::as_table).is_some_and(|r| r.contains_key("secrets")) {
                return Err(err("features: redaction.secrets cannot be set by a profile".into()));
            }
            let mut top = Table::new();
            top.insert("features".into(), Value::Table(f.clone()));
            out.merge(top, Layer::Profile);
            out.config = out.build().map_err(|e| err(e.describe(None)))?;
        }
        Ok(out)
    }

    /// Layer 6: one launch flag; a value the schema refuses leaves the input unchanged with a warning.
    /// The §6.1 exceptions hold: guards stay at or above `guards.min_level`; the turn cap is the lower non-zero.
    pub fn apply_flag(&self, key: &str, value: Value) -> Merged {
        if key == "limits.subagent_max_turns"
            && let Some(new) = value.as_integer()
        {
            let old = i64::from(self.config.limits.subagent_max_turns);
            if old != 0 && (new == 0 || old <= new) {
                return self.clone();
            }
        }
        let mut out = self.clone();
        out.set(key, value, Layer::Flags);
        match out.build() {
            Ok(c) => {
                out.config = c;
                out
            }
            Err(e) => {
                let mut same = self.clone();
                same.warnings.push(format!("flag {key}: {}; ignored", e.describe(None)));
                same
            }
        }
    }

    /// Layer 7 (§6.1 A2): session overrides within their limits.
    pub fn with_overrides(&self, o: &Overrides) -> Result<Merged, Refusal> {
        let mut sets: Vec<(String, Value)> = Vec::new();
        let toggles = [
            ("features.results", o.results.map(Value::Boolean)),
            ("results.trim_threshold_bytes", o.results_threshold.map(|n| Value::Integer(n.min(i64::MAX as u64) as i64))),
            ("features.redaction.content", o.redaction_content.map(Value::Boolean)),
            ("features.work_units", o.work_units.map(Value::Boolean)),
        ];
        for (key, v) in toggles {
            if let Some(v) = v {
                self.refuse_team(key)?;
                sets.push((key.into(), v));
            }
        }
        for (g, l) in &o.guards {
            let key = format!("guards.{g}");
            self.refuse_team(&key)?;
            if !GUARDS.contains(&g.as_str()) {
                return Err(Refusal::Unknown { what: "guard", value: g.clone() });
            }
            let level: Level = l.parse().map_err(|_| Refusal::Unknown { what: "level", value: l.clone() })?;
            let min = self.config.guards.min_level;
            if level < min {
                return Err(Refusal::BelowFloor { guard: g.clone(), min });
            }
            sets.push((key, Value::String(level.name().into())));
        }
        let mut out = self.clone();
        for (k, v) in sets {
            out.set(&k, v, Layer::Session);
        }
        out.config = out.build().expect("override values are typed by the schema");
        Ok(out)
    }

    fn refuse_team(&self, key: &str) -> Result<(), Refusal> {
        if self.team_keys.contains(key) { Err(Refusal::TeamSet { key: key.into() }) } else { Ok(()) }
    }

    /// One line per leaf, sorted by source key: `<key> = <value>  # <layer>`; `Some(layer)` keeps that layer only.
    pub fn show(&self, layer: Option<Layer>, is_set: &dyn Fn(&KeyRef) -> bool) -> String {
        let table = Table::try_from(&self.config).expect("Config serializes to TOML");
        let mut leaves = Vec::new();
        show_leaves(Vec::new(), Value::Table(table), &mut leaves);
        let mut lines: Vec<(String, String)> = leaves
            .into_iter()
            .filter_map(|(segs, v)| {
                let key = join(&segs);
                let src = self.source_of(&key);
                if layer.is_some_and(|l| l != src) {
                    return None;
                }
                let is_key = matches!(
                    segs.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
                    ["model", _, "provider", _, "key"] | ["server", "token"]
                );
                let is_source = matches!(segs.iter().map(String::as_str).collect::<Vec<_>>().as_slice(), ["marketplace", _, "source"]);
                let shown = match (is_key, v.as_str().and_then(|s| s.parse::<KeyRef>().ok())) {
                    (true, Some(r)) => format!("\"{r}\" {}", if is_set(&r) { "<set>" } else { "<unset>" }),
                    _ => match v.as_str() {
                        // C3: a marketplace source prints without its URL userinfo.
                        Some(s) if is_source => Value::String(crate::display::display_source(s)).to_string(),
                        _ => v.to_string(),
                    },
                };
                Some((key.clone(), format!("{} = {}  # {src}\n", zygo_core::esc(&key), zygo_core::esc(&shown))))
            })
            .collect();
        lines.sort();
        lines.into_iter().map(|(_, l)| l).collect()
    }
}

fn show_leaves(path: Vec<String>, v: Value, out: &mut Vec<(Vec<String>, Value)>) {
    match v {
        Value::Table(t) => {
            for (k, v) in t {
                let mut p = path.clone();
                p.push(k);
                show_leaves(p, v, out);
            }
        }
        Value::Array(a) if !a.is_empty() && a.iter().all(Value::is_table) && keyed(&path) => {
            for e in a {
                let Value::Table(e) = e else { continue };
                let mut p = path.clone();
                p.push(array_key(&path, &e).unwrap_or_default());
                show_leaves(p, Value::Table(e), out);
            }
        }
        v => out.push((path, v)),
    }
}
