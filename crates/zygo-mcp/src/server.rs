use std::cell::Cell;
use std::io::{self, BufRead, Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;
use anyhow::Result;
use serde_json::{json, Value};
use zygo_core::Agent;
use crate::tool::{Tool, ToolDef, ToolError};

const MAX_LINE: u64 = 4 * 1024 * 1024;
const LATEST: &str = "2025-11-25";
const SUPPORTED: [&str; 4] = [LATEST, "2025-06-18", "2025-03-26", "2024-11-05"];

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

thread_local!(static IN_TOOL: Cell<bool> = const { Cell::new(false) });

/// Silences the default panic message for a panic inside a tool call; the server writes its own line.
fn quiet_tool_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let prev = panic::take_hook();
        panic::set_hook(Box::new(move |info| if !IN_TOOL.with(Cell::get) { prev(info) }));
    });
}

/// One stderr line. A closed or full stderr is ignored: `eprintln!` panics on any write error.
fn note(agent: Agent, msg: &str) {
    let _ = writeln!(io::stderr().lock(), "zygo mcp ({agent}): {msg}");
}

enum Read_ { Eof, Line, TooLong }

/// Reads one line of at most MAX_LINE bytes; a longer line is discarded up to its newline.
fn read_line<R: BufRead>(input: &mut R, buf: &mut Vec<u8>) -> io::Result<Read_> {
    buf.clear();
    let n = input.by_ref().take(MAX_LINE + 1).read_until(b'\n', buf)?;
    if n == 0 { return Ok(Read_::Eof); }
    if buf.last() == Some(&b'\n') || n as u64 <= MAX_LINE { return Ok(Read_::Line); }
    buf.clear();
    let mut scratch = Vec::new();
    loop {
        scratch.clear();
        let m = input.by_ref().take(MAX_LINE).read_until(b'\n', &mut scratch)?;
        if m == 0 || scratch.last() == Some(&b'\n') { return Ok(Read_::TooLong); }
    }
}

pub fn serve_stdio(agent: Agent, tools: Vec<Box<dyn Tool>>) -> Result<()> {
    serve(agent, &tools, io::stdin().lock(), io::stdout().lock())
}

pub fn serve<R: BufRead, W: Write>(agent: Agent, tools: &[Box<dyn Tool>], mut input: R, mut out: W) -> Result<()> {
    quiet_tool_panics();
    let defs: Vec<ToolDef> = tools.iter().map(|t| t.def()).collect();
    let mut line = Vec::new();
    loop {
        let reply = match read_line(&mut input, &mut line)? {
            Read_::Eof => return Ok(()),
            Read_::TooLong => {
                note(agent, "discarded a line over 4 MiB");
                Some(error(Value::Null, -32700, "Parse error: line over 4 MiB"))
            }
            Read_::Line if line.iter().all(u8::is_ascii_whitespace) => continue,
            Read_::Line => handle(agent, tools, &defs, &line),
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut out, &reply)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
}

fn handle(agent: Agent, tools: &[Box<dyn Tool>], defs: &[ToolDef], line: &[u8]) -> Option<Value> {
    let Ok(msg) = serde_json::from_slice::<Value>(line) else {
        note(agent, "ignored a line that is not JSON");
        return Some(error(Value::Null, -32700, "Parse error"));
    };
    let Some(obj) = msg.as_object() else { return Some(error(Value::Null, -32600, "Invalid Request")); };
    let id = obj.get("id").cloned();                    // Some(Null) is a request
    if id.as_ref().is_some_and(|v| !(v.is_null() || v.is_string() || v.is_i64() || v.is_u64())) {
        return Some(error(Value::Null, -32600, "Invalid Request: id must be a string or an integer"));
    }
    let method = obj.get("method").and_then(Value::as_str);
    let (Some(method), Some("2.0")) = (method, obj.get("jsonrpc").and_then(Value::as_str)) else {
        return id.map(|id| error(id, -32600, "Invalid Request"));
    };
    let id = id?;                                       // no id key: a notification, no reply
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Some(dispatch(agent, tools, defs, id, method, &params))
}

fn dispatch(agent: Agent, tools: &[Box<dyn Tool>], defs: &[ToolDef], id: Value, method: &str, params: &Value) -> Value {
    let result = match method {
        "initialize" => {
            let Some(want) = params.get("protocolVersion").and_then(Value::as_str) else {
                return error(id, -32602, "Invalid params: protocolVersion is required");
            };
            let v = if SUPPORTED.contains(&want) { want } else { LATEST };
            json!({"protocolVersion": v, "capabilities": {"tools": {}},
                   "serverInfo": {"name": "zygo", "version": env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": defs}),
        "tools/call" => match call_tool(agent, tools, defs, params) { Ok(r) => r, Err(m) => return error(id, -32602, &m) },
        _ => return error(id, -32601, "Method not found"),
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn call_tool(agent: Agent, tools: &[Box<dyn Tool>], defs: &[ToolDef], params: &Value) -> Result<Value, String> {
    let name = params.get("name").and_then(Value::as_str).ok_or("Invalid params: name is required")?;
    let i = defs.iter().position(|d| d.name == name).ok_or_else(|| format!("Unknown tool: {name}"))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if !args.is_object() { return Err("Invalid params: arguments must be an object".into()); }
    IN_TOOL.with(|f| f.set(true));
    let called = panic::catch_unwind(AssertUnwindSafe(|| tools[i].call(args)));
    IN_TOOL.with(|f| f.set(false));
    let (text, is_error) = match called {
        Ok(Ok(Value::String(s))) => (s, false),
        Ok(Ok(v)) => (v.to_string(), false),
        Ok(Err(ToolError::Failed(m))) => (m, true),
        Ok(Err(ToolError::InvalidParams(m))) => return Err(m),
        Err(_) => {
            note(agent, &format!("tool {name} panicked"));
            (format!("zygo: tool {name} failed"), true)
        }
    };
    Ok(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
}
