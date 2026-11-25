use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use zygo_core::paths::Paths;
use zygo_results::{is_id, Store, Stored};
use zygo_usage::ReadbackRow;

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

/// Writes one read-back row for each successful result reply (O§6.4). The store opens at the first read-back and is
/// kept; a failed open or insert drops it, so the next read-back opens again. A store error never changes the reply.
pub struct Readbacks {
    usage_db: PathBuf,
    read_in: String,
    bound: Duration,
    store: Mutex<Option<zygo_usage::Store>>,
}

impl Readbacks {
    pub fn new(usage_db: PathBuf, read_in: String, bound: Duration) -> Readbacks {
        Readbacks { usage_db, read_in, bound, store: Mutex::new(None) }
    }

    fn record(&self, stored: &Stored, tool: &str, reply: &str) {
        let mut kept = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let row = ReadbackRow {
            ts: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
            session: stored.session.clone().unwrap_or_else(|| self.read_in.clone()),
            read_in: self.read_in.clone(),
            result_id: stored.id.clone(),
            tool: tool.into(),
            bytes: reply.len() as u64,
        };
        let done = match kept.take() {
            Some(s) => Ok(s),
            None => zygo_usage::Store::open_bounded(&self.usage_db, self.bound),
        }
        .and_then(|s| s.insert_readback(&row).map(|()| s));
        match done {
            Ok(s) => *kept = Some(s),
            Err(_) => {
                let _ = writeln!(std::io::stderr().lock(), "zygo mcp: usage store unavailable; read-back not recorded");
            }
        }
    }
}

pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>> {
    let Some(sd) = ctx.session_dir.as_ref() else { return Vec::new() };
    let rb = Readbacks::new(Paths::from_env().usage_db, zygo_work::hooks::session_id(sd), zygo_usage::store::HOOK_DB_BOUND);
    tools_with(sd.results(), Some(rb))
}

pub fn tools_with(dir: PathBuf, rb: Option<Readbacks>) -> Vec<Box<dyn Tool>> {
    let rb = rb.map(Arc::new);
    vec![
        Box::new(ResultView { dir: dir.clone(), rb: rb.clone() }),
        Box::new(ResultSearch { dir: dir.clone(), rb: rb.clone() }),
        Box::new(ResultTree { dir, rb }),
    ]
}

/// The successful reply `Value`, after its read-back row (when `rb` is set).
fn reply(rb: &Option<Arc<Readbacks>>, stored: &Stored, tool: &str, text: String) -> Value {
    if let Some(rb) = rb {
        rb.record(stored, tool, &text);
    }
    Value::String(text)
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

fn load(dir: &Path, args: &Value) -> Result<Stored, ToolError> {
    let id = args.get("id").and_then(Value::as_str).filter(|id| is_id(id)).ok_or_else(|| ToolError::InvalidParams("id must be a result id".to_string()))?;
    Store::get(dir, id).map_err(|e| ToolError::Failed(e.to_string()))
}

fn parse_range(s: &str) -> Option<(u64, u64)> {
    let (a, b) = s.split_once('-')?;
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit());
    if !digits(a) || !digits(b) {
        return None;
    }
    Some((a.parse().ok()?, b.parse().ok()?))
}

struct ResultView {
    dir: PathBuf,
    rb: Option<Arc<Readbacks>>,
}

impl Tool for ResultView {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "result_view".to_string(),
            description: "Read lines of a stored tool output. `lines` is a range such as \"61-120\" (at most 400 lines per call; default \"1-400\").".to_string(),
            input_schema: schema(json!({"id": {"type": "string"}, "lines": {"type": "string"}}), &["id"]),
        }
    }
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let stored = load(&self.dir, &args)?;
        let (first, last) = match args.get("lines") {
            None => (1, zygo_results::store::MAX_RANGE_LINES),
            Some(v) => v.as_str().and_then(parse_range).ok_or_else(|| ToolError::InvalidParams("lines must be a range such as \"61-120\"".to_string()))?,
        };
        let text = stored.range(first, last).map_err(ToolError::InvalidParams)?;
        Ok(reply(&self.rb, &stored, "result_view", text))
    }
}

struct ResultSearch {
    dir: PathBuf,
    rb: Option<Arc<Readbacks>>,
}

impl Tool for ResultSearch {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "result_search".to_string(),
            description: "Search a stored tool output with a Rust regex. Returns at most 50 matches, each with its line number and one line of context.".to_string(),
            input_schema: schema(json!({"id": {"type": "string"}, "pattern": {"type": "string"}}), &["id", "pattern"]),
        }
    }
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let pattern = args.get("pattern").and_then(Value::as_str).ok_or_else(|| ToolError::InvalidParams("pattern must be a string".to_string()));
        let stored = load(&self.dir, &args)?;
        let text = stored.search(pattern?).map_err(ToolError::InvalidParams)?;
        Ok(reply(&self.rb, &stored, "result_search", text))
    }
}

struct ResultTree {
    dir: PathBuf,
    rb: Option<Arc<Readbacks>>,
}

impl Tool for ResultTree {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "result_tree".to_string(),
            description: "Outline of a stored tool output: JSON keys to depth 3 with array lengths, the files of a diff, the directories of a tree, the headings of text; nothing for a log.".to_string(),
            input_schema: schema(json!({"id": {"type": "string"}}), &["id"]),
        }
    }
    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let stored = load(&self.dir, &args)?;
        let text = zygo_results::kind::tree(&stored).map_err(ToolError::Failed)?;
        Ok(reply(&self.rb, &stored, "result_tree", text))
    }
}
