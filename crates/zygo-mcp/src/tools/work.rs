use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use zygo_config::Level;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_usage::Store;
use zygo_work::decisions::record::{valid_id, Class};
use zygo_work::gate::check::{check_keep, format_problems, Disposition, KeepRequest, Proof};
use zygo_work::gate::git::{ProcessGit, VerdictGit};
use zygo_work::guards;
use zygo_work::hooks::{self, BeginArgs};
use zygo_work::policy::Guard;

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

/// `work_begin` and `work_verdict` (§13.3), inside a session only.
pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>> {
    let Some(sd) = ctx.session_dir.as_ref() else { return Vec::new() };
    let sd2 = sd.clone();
    let verdict = WorkVerdict::new(
        Paths::from_env().usage_db,
        hooks::session_id(sd),
        sd.clone(),
        Box::new(move |root| dispositions_level(&Paths::from_env(), &sd2, root)),
        Box::new(|root| Box::new(ProcessGit::new(root))),
    );
    vec![Box::new(WorkBegin), Box::new(verdict)]
}

/// The repository root (`None`: no root) -> the `dispositions` level of one keep and the config warning line, if any.
pub type LevelFn = dyn Fn(Option<&Path>) -> (Level, Option<String>) + Send + Sync;
/// The repository root -> the git reader of one keep.
pub type GitFn = dyn Fn(&Path) -> Box<dyn VerdictGit> + Send + Sync;
/// The most bytes of the compact JSON of the arguments (D§9.2: 256 KiB).
pub const ARGS_MAX: usize = 262_144;

/// The `dispositions` level (D§9.3): config, `.zygo.toml` at `root`, the session override, clamped to `min_level`.
/// A config that does not load gives `enforce`: a gate does not open on an error (D§9.4). The second value is the
/// warning line of a `team.toml` that was not used; the reply shows it.
pub fn dispositions_level(paths: &Paths, sd: &SessionDir, root: Option<&Path>) -> (Level, Option<String>) {
    let (m, warning) = hooks::session_config(paths, Some(sd), root.map(Path::to_path_buf));
    let level = m.map_or(Level::Enforce, |m| guards::policy(&m, sd, root.unwrap_or(&sd.root)).level(Guard::Dispositions));
    (level, warning)
}

const VERDICTS: [&str; 3] = ["keep", "discard", "rework"];

/// The reply only: the `work` hook (`--event begin`) records the unit, because only the hook sees `agent_id`.
pub(crate) struct WorkBegin;

impl Tool for WorkBegin {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "work_begin".into(),
            description: "Record the unit of work you are starting: its id, its round, the one command that proves it done and the files it may change. Call it once, first.".into(),
            input_schema: json!({"type": "object", "properties": {
                "unit": {"type": "string"},
                "round": {"type": "integer", "minimum": 1},
                "check": {"type": "string", "maxLength": 1024},
                "files": {"type": "array", "items": {"type": "string"}, "maxItems": 64}
            }, "required": ["unit", "round"]}),
        }
    }
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a = BeginArgs::parse(&args).map_err(ToolError::InvalidParams)?;
        Ok(Value::String(format!("noted {} round {}", a.unit, a.round)))
    }
}

const DESCRIPTION: &str = "Lead only. Record your verdict on a unit of work and round after its subagent has stopped: keep, discard or rework, with your findings. On keep, give `head` (the commit you judged) and `dispositions` (an empty list when the review had no findings): one entry for each review finding, fixed with its commit, rejected with its reason, or recorded with its DDR id; in one checkout, also give `base`.";
const CLASSES: [&str; 7] = ["none", "acceptance", "security", "irreversible", "contract", "spec", "rule"];
const DISPOSITIONS: [&str; 3] = ["fixed", "rejected", "recorded"];
const DISPOSITIONS_MAX: usize = 256;

pub struct WorkVerdict {
    db: PathBuf,
    session: String,
    session_dir: SessionDir,
    level: Box<LevelFn>,
    git: Box<GitFn>,
}

impl WorkVerdict {
    pub fn new(db: PathBuf, session: String, session_dir: SessionDir, level: Box<LevelFn>, git: Box<GitFn>) -> WorkVerdict {
        WorkVerdict { db, session, session_dir, level, git }
    }
}

/// The D§9.2 keep keys, checked for every verdict.
struct KeepArgs {
    head: Option<String>,
    base: Option<String>,
    dispositions: Option<Vec<Disposition>>,
}

fn bad(m: impl Into<String>) -> ToolError { ToolError::InvalidParams(m.into()) }

/// `^[0-9a-f]{7,64}$`.
fn hex_id(s: &str) -> bool { (7..=64).contains(&s.len()) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) }

/// A string of `min..=max` chars (JSON Schema counts code points).
fn sized(s: &str, min: usize, max: usize) -> bool { (min..=max).contains(&s.chars().count()) }

/// A present commit key: a string matching the hex pattern.
fn rev(args: &Map<String, Value>, key: &str) -> Result<Option<String>, ToolError> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if hex_id(s) => Ok(Some(s.clone())),
        Some(_) => Err(bad(format!("{key} must be a commit id of 7 to 64 lowercase hex digits"))),
    }
}

impl KeepArgs {
    fn parse(args: &Map<String, Value>) -> Result<KeepArgs, ToolError> {
        let head = rev(args, "head")?;
        let base = rev(args, "base")?;
        let dispositions = match args.get("dispositions") {
            None => None,
            Some(Value::Array(a)) if a.len() <= DISPOSITIONS_MAX => {
                Some(a.iter().enumerate().map(|(i, e)| entry(i, e)).collect::<Result<Vec<_>, _>>()?)
            }
            Some(_) => return Err(bad(format!("dispositions must be an array of at most {DISPOSITIONS_MAX} entries"))),
        };
        Ok(KeepArgs { head, base, dispositions })
    }
}

/// One `dispositions` entry (D§9.2); a missing proof is `""`, which the check reports. Unknown keys are ignored.
fn entry(i: usize, e: &Value) -> Result<Disposition, ToolError> {
    let at = |m: &str| bad(format!("dispositions[{i}]: {m}"));
    let e = e.as_object().ok_or_else(|| at("must be an object"))?;
    let str_key = |k: &str| match e.get(k) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(at(&format!("{k} must be a string"))),
    };
    let finding = str_key("finding")?.ok_or_else(|| at("finding is required"))?;
    if !sized(finding, 1, 200) {
        return Err(at("finding must be 1 to 200 characters"));
    }
    let class = str_key("class")?.ok_or_else(|| at("class is required"))?;
    let class = Class::parse(class).ok_or_else(|| at(&format!("class must be one of {}", CLASSES.join(", "))))?;
    let disposition = str_key("disposition")?.ok_or_else(|| at("disposition is required"))?;
    if !DISPOSITIONS.contains(&disposition) {
        return Err(at("disposition must be fixed, rejected or recorded"));
    }
    let commit = str_key("commit")?;
    if commit.is_some_and(|c| !hex_id(c)) {
        return Err(at("commit must be a commit id of 7 to 64 lowercase hex digits"));
    }
    let reason = str_key("reason")?;
    if reason.is_some_and(|r| !sized(r, 1, 2000)) {
        return Err(at("reason must be 1 to 2000 characters"));
    }
    let record = str_key("record")?;
    if record.is_some_and(|r| !valid_id(r)) {
        return Err(at("record must be a DDR id"));
    }
    for (key, value, takes) in [("commit", commit, "fixed"), ("reason", reason, "rejected"), ("record", record, "recorded")] {
        if value.is_some() && disposition != takes {
            return Err(at(&format!("{key} is only for {takes}")));
        }
    }
    let own = |v: Option<&str>| v.unwrap_or_default().to_string();
    let proof = match disposition {
        "fixed" => Proof::Fixed { commit: own(commit) },
        "rejected" => Proof::Rejected { reason: own(reason) },
        _ => Proof::Recorded { record: own(record) },
    };
    Ok(Disposition { finding: finding.to_string(), class, proof })
}

impl Tool for WorkVerdict {
    fn def(&self) -> ToolDef {
        let rev = json!({"type": "string", "pattern": "^[0-9a-f]{7,64}$"});
        ToolDef {
            name: "work_verdict".into(),
            description: DESCRIPTION.into(),
            input_schema: json!({"type": "object", "properties": {
                "unit": {"type": "string"},
                "round": {"type": "integer", "minimum": 1},
                "verdict": {"type": "string", "enum": VERDICTS},
                "findings": {"type": "string"},
                "head": rev,
                "base": rev,
                "dispositions": {"type": "array", "maxItems": DISPOSITIONS_MAX, "items": {"type": "object", "properties": {
                    "finding": {"type": "string", "minLength": 1, "maxLength": 200},
                    "class": {"type": "string", "enum": CLASSES},
                    "disposition": {"type": "string", "enum": DISPOSITIONS},
                    "commit": rev,
                    "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "record": {"type": "string", "pattern": "^DDR-[0-9A-HJKMNP-TV-Z]{26}$"}
                }, "required": ["finding", "class", "disposition"]}}
            }, "required": ["unit", "round", "verdict"]}),
        }
    }
    /// `findings`, the labels and the reasons are accepted, not stored (§14 stores no free text; D-C1).
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        if serde_json::to_vec(&args).map_or(true, |b| b.len() > ARGS_MAX) {
            return Err(bad("the arguments are over 256 KiB"));
        }
        let unit = args.get("unit").and_then(Value::as_str).filter(|u| hooks::valid_unit(u)).ok_or_else(|| bad("unit must be a work unit id"))?;
        let round = args
            .get("round")
            .and_then(Value::as_u64)
            .filter(|r| *r >= 1)
            .and_then(|r| u32::try_from(r).ok())
            .ok_or_else(|| bad("round must be an integer from 1"))?;
        let verdict = args
            .get("verdict")
            .and_then(Value::as_str)
            .filter(|v| VERDICTS.contains(v))
            .ok_or_else(|| bad("verdict must be keep, discard or rework"))?;
        if args.get("findings").is_some_and(|f| !f.is_string()) {
            return Err(bad("findings must be a string"));
        }
        let keep = KeepArgs::parse(args.as_object().ok_or_else(|| bad("the arguments must be an object"))?)?;
        let recorded = format!("recorded {verdict} for {unit} round {round}");
        if verdict != "keep" {
            self.record(unit, round, verdict)?;
            return Ok(Value::String(recorded));
        }
        // The guard (D§9.3): read the level per call, so a session override applies at the next keep.
        let root = SessionView::load(&self.session_dir).ok().and_then(|v| v.repo_root).map(PathBuf::from);
        let (level, warning) = (self.level)(root.as_deref());
        let end = |text: String| match &warning {
            Some(w) => format!("{text}\n{w}"),
            None => text,
        };
        if level == Level::Monitor {
            self.record(unit, round, verdict)?;
            return Ok(Value::String(end(recorded)));
        }
        let problems = match &root {
            None => vec!["cannot check: no repository root".to_string()],
            Some(root) => {
                let req = KeepRequest { unit: unit.to_string(), round, head: keep.head, base: keep.base, dispositions: keep.dispositions };
                check_keep(&req, &mut *(self.git)(root))
            }
        };
        if level == Level::Enforce && !problems.is_empty() {
            return Err(ToolError::Failed(end(format!("zygo: keep refused: {}", format_problems(&problems)))));
        }
        self.record(unit, round, verdict)?;
        if problems.is_empty() {
            return Ok(Value::String(end(recorded)));
        }
        Ok(Value::String(end(format!("{recorded}\nzygo: dispositions: {}", format_problems(&problems)))))
    }
}

impl WorkVerdict {
    fn record(&self, unit: &str, round: u32, verdict: &str) -> Result<(), ToolError> {
        if !Store::open(&self.db)?.set_verdict(&self.session, unit, round, verdict)? {
            return Err(ToolError::Failed(format!("no folded row for {unit} round {round} in this session yet")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zygo_config::Features;
    use zygo_core::session_dir::SessionDir;
    use zygo_core::Agent;
    use zygo_usage::{Filter, Store, WorkUnitRow};

    fn test_tool(db: PathBuf, t: &tempfile::TempDir) -> WorkVerdict {
        WorkVerdict::new(db, "S1".into(), SessionDir::new(t.path().join("s")), Box::new(|_| (Level::Monitor, None)), Box::new(|_| unreachable!()))
    }

    #[test]
    fn definitions() {
        let d = WorkBegin.def();
        assert_eq!(d.name, "work_begin");
        assert_eq!(d.description, "Record the unit of work you are starting: its id, its round, the one command that proves it done and the files it may change. Call it once, first.");
        assert_eq!(d.input_schema["required"], json!(["unit", "round"]));
        let t = tempfile::tempdir().unwrap();
        let v = test_tool("unused.db".into(), &t).def();
        assert_eq!(v.name, "work_verdict");
        assert_eq!(v.input_schema["properties"]["verdict"]["enum"], json!(["keep", "discard", "rework"]));
        assert_eq!(v.input_schema["required"], json!(["unit", "round", "verdict"]));
    }

    #[test]
    fn begin_replies_noted() {
        assert_eq!(WorkBegin.call(json!({"unit": "work-graph:zygo-v1-N12", "round": 2, "check": "", "files": []})),
                   Ok(json!("noted work-graph:zygo-v1-N12 round 2")));
        assert_eq!(WorkBegin.call(json!({"unit": "beads:bd-a1b2", "round": 1, "future_arg": 1})), Ok(json!("noted beads:bd-a1b2 round 1")));
        for bad in [json!({"round": 1}), json!({"unit": "beads", "round": 1}), json!({"unit": "beads:bd-a1b2", "round": 0}),
                    json!({"unit": "beads:bd-a1b2", "round": 1, "check": "x".repeat(1025)})] {
            assert!(matches!(WorkBegin.call(bad), Err(ToolError::InvalidParams(_))));
        }
        assert_eq!(WorkBegin.call(json!({"unit": "beads:bd-a1b2", "round": 1, "files": ["p".repeat(4097)]})),
                   Err(ToolError::InvalidParams("`files` has a path over 4,096 bytes".into())));
        let files: Vec<String> = (0..64).map(|i| format!("src/f{i}.rs")).collect();
        assert_eq!(WorkBegin.call(json!({"unit": "beads:bd-a1b2", "round": 1, "check": "c".repeat(1024), "files": files})),
                   Ok(json!("noted beads:bd-a1b2 round 1")));
    }

    #[test]
    fn verdict_is_recorded_on_the_row() {
        let t = tempfile::tempdir().unwrap();
        let db = t.path().join("usage.db");
        Store::open(&db).unwrap().insert_work_unit(&WorkUnitRow { unit: "beads:bd-a1b2".into(), round: 1, session: "S1".into(),
            agent_id: "a1".into(), began_at: 100, ..Default::default() }).unwrap();
        let tool = test_tool(db.clone(), &t);
        assert_eq!(tool.call(json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "keep", "findings": "check passes"})),
                   Ok(json!("recorded keep for beads:bd-a1b2 round 1")));
        assert_eq!(Store::open(&db).unwrap().work_units(&Filter::default()).unwrap()[0].verdict.as_deref(), Some("keep"));
        assert!(matches!(tool.call(json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "maybe"})), Err(ToolError::InvalidParams(_))));
        assert!(matches!(tool.call(json!({"unit": "beads:bd-a1b2", "round": 9, "verdict": "keep"})), Err(ToolError::Failed(_))));
    }

    #[test]
    fn tools_need_a_session() {
        let t = tempfile::tempdir().unwrap();
        let mut c = McpCtx { agent: Agent::ClaudeCode, features: Features::default(), session_dir: None };
        assert!(tools(&c).is_empty());
        c.session_dir = Some(SessionDir::new(t.path().join("s")));
        let names: Vec<String> = tools(&c).iter().map(|t| t.def().name).collect();
        assert_eq!(names, ["work_begin", "work_verdict"]);
    }
}
