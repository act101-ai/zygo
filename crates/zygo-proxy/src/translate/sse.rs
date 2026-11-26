//! Streamed responses (§7.6): Responses and chat/completions SSE events as Messages SSE events. A partial event is
//! held up to HOLD_MAX (§10.2.6); every byte is scanned once (C5).

use serde_json::{json, Value};
use zygo_config::Dialect;

use super::reasoning::encode;
use super::{chat_stop, context_error, error_body, error_message, error_object, responses_stop, s, scrub, usage, usage_of, Target};
use crate::tee::HOLD_MAX;

#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: String,
    pub data: Value,
}

impl SseEvent {
    /// `"event: <event>\ndata: <compact JSON>\n\n"`.
    pub fn to_bytes(&self) -> Vec<u8> {
        format!("event: {}\ndata: {}\n\n", self.event, self.data).into_bytes()
    }
}

/// A block key: Responses (`output_index`, `content_index`); chat text (0, MAX), chat tool call (1, its position).
type Key = (u64, u64);
const CHAT_TEXT: Key = (0, u64::MAX);
/// The held count of a chat tool call before its id, name and argument bytes: an empty call is not free.
const CALL_COST: usize = 64;
/// The most `partial_json`, `text` or `thinking` bytes in one delta that `message_events`, the chat mapper and the
/// null-restore filter send (K§6.4.3 stream obligation 8).
pub const INPUT_DELTA_MAX: usize = 64 << 10;
/// The count of tool calls dropped at a refusal (their arguments may be cut; K§6.4.3 stream obligation 5): chat calls
/// at `finish_reason` `content_filter`, and a Responses `function_call` still held (no `output_item.done` with status
/// `completed`) at a refusal. A top-level key of
/// the Messages body and of `message_delta`, absent when none. The Responses encoder reads it so that the refusal
/// still ends in `response.failed`, with adjustment `refusal_call_dropped:<n>`. A Messages client never gets it
/// (`StreamMapper::calls_dropped_key(false)`, `take_calls_dropped`).
pub const CALLS_DROPPED: &str = "zygo_calls_dropped";

/// Removes the top-level `CALLS_DROPPED` key of a Messages body or `message_delta` and returns its count; 0 when
/// absent or `v` is not an object.
pub fn take_calls_dropped(v: &mut Value) -> u64 {
    v.as_object_mut().and_then(|o| o.remove(CALLS_DROPPED)).and_then(|n| n.as_u64()).unwrap_or(0)
}

struct Open {
    key: Key,
    index: u32,
    /// A delta was sent (on a held block: a non-empty `partial_json` was added).
    delta: bool,
    /// A Responses `function_call` block: it waits in `pending` until it closes.
    held: bool,
}

/// A chat tool call, held until the choice finishes: chat deltas of several calls may interleave, and an Anthropic
/// block must start, get its deltas and stop before the next one starts. Also the held Responses `function_call`.
struct Call {
    /// chat: the delta's `index`, else its position in `tool_calls` (several calls can share it). Responses: unused.
    index: u64,
    id: String,
    name: String,
    args: String,
}

/// No `Debug`: it holds the key.
pub struct StreamMapper {
    from: Dialect,
    target: Target,
    estimate: u64,
    key: Option<String>,
    /// Bytes after the last whole line; `pos` is where the newline search resumes.
    buf: Vec<u8>,
    pos: usize,
    /// The `data:` lines of the event being read, joined with "\n".
    data: Option<Vec<u8>>,
    started: bool,
    ended: bool,
    id: String,
    model: Option<String>,
    next: u32,
    open: Option<Open>,
    tool: bool,
    /// chat: the tool calls in order of first delta, and their held bytes (CALL_COST + id + name + arguments per call);
    /// Responses: the held `function_call` block's bytes (CALL_COST + id + name + arguments). `held` plus the
    /// partial line (`buf`, `data`) is at most HOLD_MAX.
    calls: Vec<Call>,
    held: usize,
    /// Responses: the held `function_call` block: its id, name and arguments joined in one string.
    pending: Option<Call>,
    /// Responses: the `output_index` of each `function_call` block already sent; a later done or delta for it is
    /// ignored.
    sent: Vec<u64>,
    /// The calls dropped at a refusal (`CALLS_DROPPED`).
    dropped: usize,
    /// `end` writes `CALLS_DROPPED` on `message_delta` (off for a Messages client).
    dropped_key: bool,
    /// chat: the `finish_reason` and the last usage.
    finish: Option<String>,
    usage: Value,
}

impl StreamMapper {
    /// `from` = Openai | OpenaiChat; Anthropic gives no events.
    pub fn new(from: Dialect, target: &Target) -> StreamMapper {
        StreamMapper {
            from,
            target: target.clone(),
            estimate: 0,
            key: None,
            buf: Vec::new(),
            pos: 0,
            data: None,
            started: false,
            ended: false,
            id: String::new(),
            model: None,
            next: 0,
            open: None,
            tool: false,
            calls: Vec::new(),
            held: 0,
            pending: None,
            sent: Vec::new(),
            dropped: 0,
            dropped_key: true,
            finish: None,
            usage: usage(0, 0, 0),
        }
    }

    /// The request's token estimate and the provider key, for in-stream errors.
    pub fn errors(mut self, estimate: u64, key: Option<&str>) -> StreamMapper {
        self.estimate = estimate;
        self.key = key.map(str::to_string);
        self
    }

    /// `on` = `end` writes `CALLS_DROPPED` on `message_delta` (the default); a Messages client gets `false`.
    pub fn calls_dropped_key(mut self, on: bool) -> StreamMapper {
        self.dropped_key = on;
        self
    }

    /// The calls dropped at a refusal so far (K§6.4.3 stream obligation 5).
    pub fn calls_dropped(&self) -> u64 {
        self.dropped as u64
    }

    /// Test accessor: the held byte count (`held`) and the number of calls that wait in `pending` (0 or 1).
    #[doc(hidden)]
    pub fn held_state(&self) -> (usize, usize) {
        (self.held, usize::from(self.pending.is_some()))
    }

    /// A terminal or error event was emitted.
    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if self.ended || self.from == Dialect::Anthropic {
            return out;
        }
        self.buf.extend_from_slice(chunk);
        let mut start = 0;
        while let Some(i) = self.buf[self.pos..].iter().position(|&b| b == b'\n') {
            let end = self.pos + i;
            let line = self.buf[start..end].to_vec();
            start = end + 1;
            self.pos = start;
            self.line(&line, &mut out);
            if self.ended {
                self.buf = Vec::new();
                self.pos = 0;
                return out;
            }
        }
        self.buf.drain(..start);
        // No newline in the rest: the next search starts after it.
        self.pos = self.buf.len();
        if self.held + self.buf.len() + self.data.as_ref().map_or(0, Vec::len) > HOLD_MAX {
            self.buf = Vec::new();
            self.pos = 0;
            self.data = None;
            let m = if self.held > 0 { "upstream tool call too long" } else { "upstream stream line too long" };
            self.emit_error(error_body("api_error", m), &mut out);
        }
        out
    }

    /// The held tail as a last line; without a terminal event, an `upstream stream ended early` error.
    pub fn finish(&mut self) -> Vec<u8> {
        self.finish_events().iter().flat_map(SseEvent::to_bytes).collect()
    }

    /// `finish` as events.
    pub fn finish_events(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if self.ended || self.from == Dialect::Anthropic {
            return out;
        }
        let tail = std::mem::take(&mut self.buf);
        self.pos = 0;
        if !tail.is_empty() {
            self.line(&tail, &mut out);
        }
        if !self.ended && self.data.is_some() {
            self.line(b"", &mut out);
        }
        if !self.ended {
            self.emit_error(error_body("api_error", "upstream stream ended early"), &mut out);
        }
        out
    }

    fn line(&mut self, l: &[u8], out: &mut Vec<SseEvent>) {
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        if l.is_empty() {
            if let Some(d) = self.data.take() {
                self.event(&d, out);
            }
        } else if let Some(rest) = l.strip_prefix(b"data:") {
            let rest = rest.strip_prefix(b" ").unwrap_or(rest);
            match &mut self.data {
                Some(d) => {
                    d.push(b'\n');
                    d.extend_from_slice(rest);
                }
                None => self.data = Some(rest.to_vec()),
            }
        }
    }

    fn event(&mut self, data: &[u8], out: &mut Vec<SseEvent>) {
        if self.from == Dialect::OpenaiChat && data.trim_ascii() == b"[DONE]" {
            let stop = chat_stop(self.finish.as_deref(), self.tool);
            let u = self.usage.clone();
            return self.end(stop, u, out);
        }
        let Ok(v) = serde_json::from_slice::<Value>(data) else { return };
        match self.from {
            Dialect::Openai => self.responses(&v, out),
            Dialect::OpenaiChat => self.chat(&v, out),
            Dialect::Anthropic => {}
        }
    }

    fn responses(&mut self, v: &Value, out: &mut Vec<SseEvent>) {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        let (oi, ci) = (n("output_index"), n("content_index"));
        let item = v.get("item").unwrap_or(&Value::Null);
        let text = json!({"type":"text","text":""});
        let thinking = json!({"type":"thinking","thinking":"","signature":""});
        match s(v, "type").unwrap_or("") {
            "response.created" => {
                self.meta(&v["response"]);
                self.start(out);
            }
            "response.output_item.added" => match s(item, "type") {
                Some("reasoning") => self.open((oi, 0), thinking, out),
                Some("function_call") => {
                    self.tool = true;
                    let b = json!({"type":"tool_use","id":s(item, "call_id").unwrap_or(""),"name":s(item, "name").unwrap_or(""),"input":{}});
                    self.open((oi, 0), b, out);
                }
                _ => {}
            },
            "response.reasoning_summary_part.added" if n("summary_index") > 0 => {
                self.ensure((oi, 0), thinking, out);
                self.delta(json!({"type":"thinking_delta","thinking":"\n\n"}), out);
            }
            "response.reasoning_summary_text.delta" => {
                self.ensure((oi, 0), thinking, out);
                self.delta(json!({"type":"thinking_delta","thinking":s(v, "delta").unwrap_or("")}), out);
            }
            "response.content_part.added" if matches!(s(&v["part"], "type"), Some("output_text" | "refusal")) => {
                self.open((oi, ci), text, out);
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                self.ensure((oi, ci), text, out);
                self.delta(json!({"type":"text_delta","text":s(v, "delta").unwrap_or("")}), out);
            }
            "response.content_part.done" => {
                let p = &v["part"];
                let late = s(p, "text").or_else(|| s(p, "refusal")).unwrap_or("");
                if !self.got((oi, ci)) && !late.is_empty() {
                    self.ensure((oi, ci), text, out);
                    self.delta(json!({"type":"text_delta","text":late}), out);
                }
                self.close_key((oi, ci), out);
            }
            // A late delta for a call already sent is ignored.
            "response.function_call_arguments.delta" if self.sent.contains(&oi) => {}
            "response.function_call_arguments.delta" => {
                let b = json!({"type":"tool_use","id":"","name":"","input":{}});
                self.ensure((oi, 0), b, out);
                self.delta(json!({"type":"input_json_delta","partial_json":s(v, "delta").unwrap_or("")}), out);
                if self.held > HOLD_MAX {
                    self.emit_error(error_body("api_error", "upstream tool call too long"), out);
                }
            }
            "response.output_item.done" => match s(item, "type") {
                Some("reasoning") => {
                    self.ensure((oi, 0), thinking, out);
                    if let Some(e) = s(item, "encrypted_content") {
                        let sig = encode(&self.target, s(item, "id").unwrap_or(""), e);
                        self.delta(json!({"type":"signature_delta","signature":sig}), out);
                    }
                    self.close(out);
                }
                // A late done for a call already sent is ignored.
                Some("function_call") if self.sent.contains(&oi) => {}
                // Sent only when `completed`; else it stays held, as an open call: dropped at a refusal, sent at any
                // other end (as `responses::response` does).
                Some("function_call") => {
                    let args = s(item, "arguments").unwrap_or("");
                    if !self.got((oi, 0)) && !args.is_empty() {
                        let b = json!({"type":"tool_use","id":s(item, "call_id").unwrap_or(""),"name":s(item, "name").unwrap_or(""),"input":{}});
                        self.ensure((oi, 0), b, out);
                        self.delta(json!({"type":"input_json_delta","partial_json":args}), out);
                    }
                    if s(item, "status") == Some("completed") {
                        self.close_key((oi, 0), out);
                    }
                }
                Some("message") => self.close(out),
                _ => {}
            },
            t @ ("response.completed" | "response.incomplete") => {
                let r = &v["response"];
                self.meta(r);
                let status = s(r, "status").or_else(|| t.strip_prefix("response."));
                let reason = r.get("incomplete_details").and_then(|d| s(d, "reason"));
                let stop = responses_stop(status, reason, self.tool);
                self.end(stop, usage_of(r.get("usage"), Dialect::Openai), out);
            }
            "response.failed" => self.fail(&v["response"]["error"], out),
            "error" => self.fail(v, out),
            _ => {}
        }
    }

    fn chat(&mut self, v: &Value, out: &mut Vec<SseEvent>) {
        if v.get("error").is_some_and(|e| !e.is_null()) {
            return self.fail(v, out);
        }
        self.meta(v);
        self.start(out);
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage = usage_of(Some(u), Dialect::OpenaiChat);
        }
        let Some(c) = v.get("choices").and_then(|c| c.get(0)) else { return };
        let d = c.get("delta").unwrap_or(&Value::Null);
        if let Some(x) = s(d, "content").filter(|x| !x.is_empty()) {
            self.ensure(CHAT_TEXT, json!({"type":"text","text":""}), out);
            for d in deltas("text_delta", "text", x) {
                self.delta(d, out);
            }
        }
        for (pos, tc) in d.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let index = tc.get("index").and_then(Value::as_u64);
            let i = index.unwrap_or(pos as u64);
            let f = tc.get("function").unwrap_or(&Value::Null);
            self.tool = true;
            // Without `index`, an `id` that differs from the matched call's id starts a new call.
            let found = self.calls.iter().rposition(|c| c.index == i);
            let found = found.filter(|&k| index.is_some() || s(tc, "id").is_none_or(|id| id == self.calls[k].id));
            let k = match found {
                Some(k) => k,
                None => {
                    let id = s(tc, "id").map_or_else(|| format!("call_{}_{i}", self.id), str::to_string);
                    self.held += CALL_COST + id.len();
                    self.calls.push(Call { index: i, id, name: String::new(), args: String::new() });
                    self.calls.len() - 1
                }
            };
            let call = &mut self.calls[k];
            if call.name.is_empty() {
                call.name = s(f, "name").unwrap_or("").into();
                self.held += call.name.len();
            }
            let a = s(f, "arguments").unwrap_or("");
            call.args.push_str(a);
            self.held += a.len();
            if self.held > HOLD_MAX {
                return self.emit_error(error_body("api_error", "upstream tool call too long"), out);
            }
        }
        if let Some(fr) = s(c, "finish_reason") {
            self.finish = Some(fr.into());
            self.close(out);
            self.calls_out(out);
        }
    }

    /// The held chat tool calls, each as one whole `tool_use` block; its arguments in `input_json_delta` pieces of
    /// `deltas`. At `finish_reason` `content_filter` none: they are counted in `dropped` (`CALLS_DROPPED`).
    fn calls_out(&mut self, out: &mut Vec<SseEvent>) {
        if self.finish.as_deref() == Some("content_filter") {
            self.dropped += std::mem::take(&mut self.calls).len();
        }
        for c in std::mem::take(&mut self.calls) {
            self.open((1, c.index), json!({"type":"tool_use","id":c.id,"name":c.name,"input":{}}), out);
            for d in deltas("input_json_delta", "partial_json", &c.args) {
                self.delta(d, out);
            }
            self.close(out);
        }
        self.held = 0;
    }

    /// The upstream `id` and `model`, read until `message_start` is sent.
    fn meta(&mut self, r: &Value) {
        if self.started {
            return;
        }
        if let Some(id) = s(r, "id") {
            self.id = id.into();
        }
        if let Some(m) = s(r, "model") {
            self.model = Some(m.into());
        }
    }

    fn start(&mut self, out: &mut Vec<SseEvent>) {
        if self.started {
            return;
        }
        self.started = true;
        let model = self.model.as_deref().unwrap_or(&self.target.upstream_model);
        out.push(ev("message_start", json!({"type":"message_start","message":{"id":self.id,"type":"message","role":"assistant","model":model,
            "content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}})));
    }

    /// Closes the open block and opens `block` under `key`. A Responses `tool_use` block is held: its id and name wait
    /// in `pending`, sent by `close` (K§6.4.3 stream obligation 5).
    fn open(&mut self, key: Key, block: Value, out: &mut Vec<SseEvent>) {
        self.start(out);
        self.close(out);
        let held = self.from == Dialect::Openai && block["type"] == "tool_use";
        if held {
            let (id, name) = (s(&block, "id").unwrap_or("").to_string(), s(&block, "name").unwrap_or("").to_string());
            self.held += CALL_COST + id.len() + name.len();
            self.pending = Some(Call { index: 0, id, name, args: String::new() });
        } else {
            out.push(ev("content_block_start", json!({"type":"content_block_start","index":self.next,"content_block":block})));
        }
        self.open = Some(Open { key, index: self.next, delta: false, held });
        self.next += 1;
    }

    /// `open`, unless `key`'s block is open.
    fn ensure(&mut self, key: Key, block: Value, out: &mut Vec<SseEvent>) {
        if self.open.as_ref().map(|o| o.key) != Some(key) {
            self.open(key, block, out);
        }
    }

    /// `key`'s block is open and got a delta.
    fn got(&self, key: Key) -> bool {
        self.open.as_ref().is_some_and(|o| o.key == key && o.delta)
    }

    /// A delta on the open block. On a held block its `partial_json` is appended to the held arguments and counted in
    /// `held`; an empty or missing one adds nothing.
    fn delta(&mut self, d: Value, out: &mut Vec<SseEvent>) {
        if let Some(o) = &mut self.open {
            if !o.held {
                o.delta = true;
                out.push(ev("content_block_delta", json!({"type":"content_block_delta","index":o.index,"delta":d})));
            } else if let (Some(c), Some(a)) = (&mut self.pending, s(&d, "partial_json").filter(|a| !a.is_empty())) {
                o.delta = true;
                c.args.push_str(a);
                self.held += a.len();
            }
        }
    }

    /// Closes the open block. A held block goes out whole first: its `content_block_start`, then its arguments in
    /// `input_json_delta` pieces of `deltas`.
    fn close(&mut self, out: &mut Vec<SseEvent>) {
        if let Some(o) = self.open.take() {
            if let Some(c) = self.pending.take().filter(|_| o.held) {
                let b = json!({"type":"tool_use","id":c.id,"name":c.name,"input":{}});
                out.push(ev("content_block_start", json!({"type":"content_block_start","index":o.index,"content_block":b})));
                for d in deltas("input_json_delta", "partial_json", &c.args) {
                    out.push(ev("content_block_delta", json!({"type":"content_block_delta","index":o.index,"delta":d})));
                }
                self.held = 0;
                self.sent.push(o.key.0);
            }
            out.push(ev("content_block_stop", json!({"type":"content_block_stop","index":o.index})));
        }
    }

    /// A held open block (a Responses `function_call` with no `output_item.done` of status `completed`) is dropped, not sent with cut
    /// arguments, and counted in `dropped` (K§6.4.3 stream obligation 5).
    fn drop_held(&mut self) {
        if self.open.as_ref().is_some_and(|o| o.held) {
            self.open = None;
            self.pending = None;
            self.held = 0;
            self.dropped += 1;
        }
    }

    fn close_key(&mut self, key: Key, out: &mut Vec<SseEvent>) {
        if self.open.as_ref().is_some_and(|o| o.key == key) {
            self.close(out);
        }
    }

    fn end(&mut self, stop: &str, usage: Value, out: &mut Vec<SseEvent>) {
        self.start(out);
        if stop == "refusal" {
            self.drop_held();
        }
        self.close(out);
        self.calls_out(out);
        let mut d = json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":usage});
        if self.dropped > 0 && self.dropped_key {
            d[CALLS_DROPPED] = json!(self.dropped);
        }
        out.push(ev("message_delta", d));
        out.push(ev("message_stop", json!({"type":"message_stop"})));
        self.ended = true;
    }

    /// An upstream error: a context-limit refusal, else `api_error` with the upstream message (key replaced).
    fn fail(&mut self, v: &Value, out: &mut Vec<SseEvent>) {
        let body = context_error(error_object(v), self.estimate).unwrap_or_else(|| {
            let m = error_message(v).map_or_else(|| "upstream response failed".into(), |m| scrub(m, self.key.as_deref()));
            error_body("api_error", &m)
        });
        self.emit_error(body, out);
    }

    fn emit_error(&mut self, body: Value, out: &mut Vec<SseEvent>) {
        out.push(ev("error", body));
        self.ended = true;
    }
}

/// A Messages body as the Messages SSE events that build it: a text or thinking block in `text_delta` or
/// `thinking_delta` pieces (one empty delta for an empty block), then a thinking block's one `signature_delta`; a
/// tool_use input as compact JSON in `input_json_delta` pieces; each piece of at most `INPUT_DELTA_MAX` bytes, cut on a
/// char boundary, so that each event stays far within `HOLD_MAX` (K§6.4.3 stream obligation 8); any other block (of another type or with no
/// `type`) goes out whole in its content_block_start with no delta; the usage on message_delta. The Responses encoder
/// keeps redacted_thinking from that arm and drops server-tool and unknown blocks; a Messages client never reaches
/// it, because its translated bodies hold only text, thinking and tool_use.
pub fn message_events(m: &Value) -> Vec<SseEvent> {
    let mut start = m.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    start["usage"] = json!({"input_tokens":0,"output_tokens":0});
    // The body's `CALLS_DROPPED` goes on message_delta, as the chat mapper sends it.
    let dropped = start.as_object_mut().and_then(|o| o.remove(CALLS_DROPPED));
    let mut out = vec![ev("message_start", json!({"type":"message_start","message":start}))];
    for (i, b) in m.get("content").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let (first, deltas) = match s(b, "type") {
            Some("thinking") => {
                let mut d = block_deltas("thinking_delta", "thinking", s(b, "thinking").unwrap_or(""));
                if let Some(g) = s(b, "signature").filter(|g| !g.is_empty()) {
                    d.push(json!({"type":"signature_delta","signature":g}));
                }
                (json!({"type":"thinking","thinking":"","signature":""}), d)
            }
            Some("tool_use") => {
                let input = b.get("input").map_or_else(|| "{}".into(), Value::to_string);
                (json!({"type":"tool_use","id":b["id"],"name":b["name"],"input":{}}), deltas("input_json_delta", "partial_json", &input))
            }
            Some("text") => (json!({"type":"text","text":""}), block_deltas("text_delta", "text", s(b, "text").unwrap_or(""))),
            _ => (b.clone(), vec![]),
        };
        out.push(ev("content_block_start", json!({"type":"content_block_start","index":i,"content_block":first})));
        out.extend(deltas.into_iter().map(|d| ev("content_block_delta", json!({"type":"content_block_delta","index":i,"delta":d}))));
        out.push(ev("content_block_stop", json!({"type":"content_block_stop","index":i})));
    }
    let mut d = json!({"type":"message_delta","delta":{"stop_reason":m["stop_reason"],"stop_sequence":null},"usage":m["usage"]});
    if let Some(n) = dropped {
        d[CALLS_DROPPED] = n;
    }
    out.push(ev("message_delta", d));
    out.push(ev("message_stop", json!({"type":"message_stop"})));
    out
}

/// `s` as delta values `{"type": kind, key: piece}` of at most `INPUT_DELTA_MAX` bytes each, cut on a char boundary
/// (a char has at most 4 bytes, so no piece is empty); none for an empty string (K§6.4.3 stream obligation 8).
pub(super) fn deltas(kind: &str, key: &str, s: &str) -> Vec<Value> {
    let mut d = vec![];
    let mut rest = s;
    while !rest.is_empty() {
        let (p, r) = rest.split_at(rest.floor_char_boundary(INPUT_DELTA_MAX));
        d.push(json!({"type": kind, key: p}));
        rest = r;
    }
    d
}

/// A text or thinking block's deltas: `deltas`, or one empty delta for an empty string.
fn block_deltas(kind: &str, key: &str, s: &str) -> Vec<Value> {
    if s.is_empty() {
        vec![json!({"type": kind, key: ""})]
    } else {
        deltas(kind, key, s)
    }
}

fn ev(event: &str, data: Value) -> SseEvent {
    SseEvent { event: event.into(), data }
}
