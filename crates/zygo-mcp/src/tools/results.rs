use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use zygo_results::{is_id, Store, Stored};

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>> {
    let Some(sd) = ctx.session_dir.as_ref() else { return Vec::new() };
    let dir = sd.results();
    vec![Box::new(ResultView { dir: dir.clone() }), Box::new(ResultSearch { dir: dir.clone() }), Box::new(ResultTree { dir })]
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
        stored.range(first, last).map(Value::String).map_err(ToolError::InvalidParams)
    }
}

struct ResultSearch {
    dir: PathBuf,
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
        stored.search(pattern?).map(Value::String).map_err(ToolError::InvalidParams)
    }
}

struct ResultTree {
    dir: PathBuf,
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
        zygo_results::kind::tree(&stored).map(Value::String).map_err(ToolError::Failed)
    }
}
