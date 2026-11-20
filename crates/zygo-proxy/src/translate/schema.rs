//! Strict schemas (§7.6): each `refuse` rule rewrites its construct in tool and output schemas, in a fixed order; an
//! injected `null` is removed on the way back. Every pass is linear in the body (C5).

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};
use zygo_config::{RuleSeverity, SchemaRule};

use super::{s, SseEvent};
use crate::pipeline::Adjustment;
use crate::tee::HOLD_MAX;

/// The fixed rewrite order (§7.6). `array_items_required` and `empty_schema` have no rewrite (N4 note).
pub const ORDER: [&str; 13] = [
    "array_items_required",
    "empty_schema",
    "map_object",
    "one_of",
    "all_of",
    "conditional",
    "closed_object",
    "numeric_bounds",
    "string_length",
    "array_bounds",
    "pattern",
    "optional_property",
    "optional_properties_max",
];

/// One step of a path to an injected null: a `properties` key, or the elements of an array (`items`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Items,
}

pub type Path = Vec<Seg>;

/// The paths of injected nulls: per tool name, and in the output schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NullMap {
    pub tools: BTreeMap<String, Vec<Path>>,
    pub output: Vec<Path>,
}

impl NullMap {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.output.is_empty()
    }

    /// Removes injected nulls from `tool_use` inputs and from JSON text blocks of a Messages body; true when one was
    /// removed.
    pub fn restore_message(&self, msg: &mut Value) -> bool {
        let tools: BTreeMap<&str, Trie> = self.tools.iter().map(|(n, p)| (n.as_str(), Trie::of(p))).collect();
        let output = (!self.output.is_empty()).then(|| Trie::of(&self.output));
        let mut removed = false;
        for b in msg.get_mut("content").and_then(Value::as_array_mut).into_iter().flatten() {
            match s(b, "type") {
                Some("tool_use") => {
                    if let Some(t) = s(b, "name").and_then(|n| tools.get(n))
                        && let Some(input) = b.get_mut("input")
                    {
                        removed |= t.strip(input);
                    }
                }
                Some("text") => {
                    if let Some(t) = &output
                        && let Some(x) = s(b, "text").and_then(|x| t.strip_text(x))
                    {
                        b["text"] = Value::String(x);
                        removed = true;
                    }
                }
                _ => {}
            }
        }
        removed
    }
}

/// The paths as a tree: one node per `Seg`; `null` marks a path's end (reading 16).
#[derive(Debug, Default)]
struct Trie {
    null: bool,
    keys: BTreeMap<String, Trie>,
    items: Option<Box<Trie>>,
}

impl Trie {
    fn of(paths: &[Path]) -> Trie {
        let mut root = Trie::default();
        for p in paths {
            let mut t = &mut root;
            for seg in p {
                t = match seg {
                    Seg::Key(k) => t.keys.entry(k.clone()).or_default(),
                    Seg::Items => t.items.get_or_insert_with(Box::default),
                };
            }
            t.null = true;
        }
        root
    }

    /// One walk along the trie: a key whose path ends here and whose value is `null` is removed.
    fn strip(&self, v: &mut Value) -> bool {
        let mut removed = false;
        match v {
            Value::Object(o) => {
                let before = o.len();
                o.retain(|k, x| !(x.is_null() && self.keys.get(k).is_some_and(|t| t.null)));
                removed = o.len() != before;
                for (k, x) in o.iter_mut() {
                    if let Some(t) = self.keys.get(k) {
                        removed |= t.strip(x);
                    }
                }
            }
            Value::Array(a) => {
                if let Some(t) = &self.items {
                    for x in a {
                        removed |= t.strip(x);
                    }
                }
            }
            _ => {}
        }
        removed
    }

    /// `text` as JSON with the injected nulls removed, as compact JSON; `None` when nothing was removed.
    fn strip_text(&self, text: &str) -> Option<String> {
        let mut v: Value = serde_json::from_str(text).ok()?;
        self.strip(&mut v).then(|| v.to_string())
    }
}

/// Where a schema sits in the request.
enum Loc {
    Tool(String),
    Output,
}

/// The tool schemas (a tool without `type` or with `type: "custom"`), in order, then the output schema.
fn locations(req: &mut Value) -> Vec<(Loc, &mut Value)> {
    let (mut tools, mut output) = (Vec::new(), Vec::new());
    let Some(obj) = req.as_object_mut() else { return tools };
    for (k, v) in obj.iter_mut() {
        match k.as_str() {
            "tools" => {
                for t in v.as_array_mut().into_iter().flatten() {
                    if s(t, "type").is_some_and(|ty| ty != "custom") {
                        continue;
                    }
                    let name = s(t, "name").unwrap_or("").to_string();
                    if let Some(sch) = t.get_mut("input_schema") {
                        tools.push((Loc::Tool(name), sch));
                    }
                }
            }
            "output_config" => {
                if let Some(f) = v.get_mut("format").filter(|f| s(f, "type") == Some("json_schema"))
                    && let Some(sch) = f.get_mut("schema")
                {
                    output.push((Loc::Output, sch));
                }
            }
            _ => {}
        }
    }
    tools.extend(output);
    tools
}

type Visit<'a> = dyn FnMut(&mut Map<String, Value>, Option<&Path>) + 'a;

/// Pre-order over every schema node; `path` is kept through `properties`, `items`, `anyOf` and `oneOf` only.
fn walk(v: &mut Value, mut path: Option<&mut Path>, f: &mut Visit) {
    let Some(node) = v.as_object_mut() else { return };
    f(node, path.as_deref());
    for (k, child) in node.iter_mut() {
        match k.as_str() {
            "properties" => {
                for (name, c) in child.as_object_mut().into_iter().flatten() {
                    down(c, path.as_deref_mut(), Some(Seg::Key(name.clone())), f);
                }
            }
            "$defs" | "definitions" | "patternProperties" => {
                for c in child.as_object_mut().into_iter().flat_map(|m| m.values_mut()) {
                    walk(c, None, f);
                }
            }
            "anyOf" | "oneOf" => {
                for c in child.as_array_mut().into_iter().flatten() {
                    walk(c, path.as_deref_mut(), f);
                }
            }
            "allOf" | "prefixItems" => {
                for c in child.as_array_mut().into_iter().flatten() {
                    walk(c, None, f);
                }
            }
            "items" => down(child, path.as_deref_mut(), Some(Seg::Items), f),
            "additionalProperties" | "not" | "if" | "then" | "else" | "contains" => walk(child, None, f),
            _ => {}
        }
    }
}

fn down(c: &mut Value, path: Option<&mut Path>, seg: Option<Seg>, f: &mut Visit) {
    match (path, seg) {
        (Some(p), Some(seg)) => {
            p.push(seg);
            walk(c, Some(p), f);
            p.pop();
        }
        (p, _) => walk(c, p, f),
    }
}

/// Deletes the `keys` that `pick` selects from `node` and appends their compact JSON object (in node order) to
/// `description`. True when one was moved.
fn moved(node: &mut Map<String, Value>, pick: impl Fn(&str, &Value) -> bool) -> bool {
    let names: Vec<String> = node.iter().filter(|(k, v)| pick(k, v)).map(|(k, _)| k.clone()).collect();
    if names.is_empty() {
        return false;
    }
    let mut m = Map::new();
    for n in names {
        if let Some(v) = node.shift_remove(&n) {
            m.insert(n, v);
        }
    }
    let text = Value::Object(m).to_string();
    match node.get_mut("description") {
        Some(Value::String(d)) if !d.is_empty() => {
            d.push_str("\n\n");
            d.push_str(&text);
        }
        _ => {
            node.insert("description".into(), Value::String(text));
        }
    }
    true
}

fn is_object_type(node: &Map<String, Value>) -> bool {
    match node.get("type") {
        Some(Value::String(t)) => t == "object",
        Some(Value::Array(a)) => a.iter().any(|t| t == "object"),
        _ => false,
    }
}

/// `type` `"null"`, a `type` array with `"null"` (and an `enum`/`const`, if present, that accepts null), or an `anyOf`/`oneOf` branch that accepts null; with no `type`,
/// also an `enum` with null or `const: null`; the schema `true` and the empty schema `{}` (Round 2, F2).
fn accepts_null(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return *v == Value::Bool(true) };
    match o.get("type") {
        Some(Value::String(t)) if t == "null" => return true,
        // Only when `enum` and `const`, if present, also accept null (Round 3, D2).
        Some(Value::Array(a))
            if a.iter().any(|t| t == "null")
                && o.get("enum").and_then(Value::as_array).is_none_or(|e| e.contains(&Value::Null))
                && o.get("const").is_none_or(Value::is_null) =>
        {
            return true;
        }
        Some(_) => {}
        None => {
            if o.is_empty()
                || o.get("enum").and_then(Value::as_array).is_some_and(|e| e.contains(&Value::Null))
                || o.get("const") == Some(&Value::Null)
            {
                return true;
            }
        }
    }
    ["anyOf", "oneOf"].iter().any(|k| v.get(*k).and_then(Value::as_array).is_some_and(|a| a.iter().any(accepts_null)))
}

/// Makes `v` accept null (reading 15); true when a null was injected.
fn nullable(v: &mut Value) -> bool {
    let Some(node) = v.as_object_mut() else { return false };
    // A non-null `const` forbids null whatever `type` says: wrapped (Round 2, F4).
    if node.get("const").is_some_and(|c| !c.is_null()) {
        let old = std::mem::take(v);
        *v = json!({"anyOf":[old, {"type":"null"}]});
        return true;
    }
    let typed = match node.get_mut("type") {
        Some(Value::String(t)) => {
            let t = std::mem::take(t);
            node.insert("type".into(), json!([t, "null"]));
            true
        }
        Some(Value::Array(a)) => {
            if !a.iter().any(|t| t == "null") {
                a.push(json!("null"));
            }
            true
        }
        _ => false,
    };
    if let Some(e) = node.get_mut("enum").and_then(Value::as_array_mut) {
        if !e.contains(&Value::Null) {
            e.push(Value::Null);
        }
        return true;
    }
    if typed {
        return true;
    }
    if let Some(a) = node.get_mut("anyOf").and_then(Value::as_array_mut) {
        a.push(json!({"type":"null"}));
        return true;
    }
    let old = std::mem::take(v);
    *v = json!({"anyOf":[old, {"type":"null"}]});
    true
}

/// `rules` = one family's rules; only `refuse` rules apply, in ORDER. Codes `schema:<code>:<n>` with `n` > 0.
pub fn rewrite(req: &mut Value, rules: &[SchemaRule]) -> (Vec<Adjustment>, NullMap) {
    let mut codes = Vec::new();
    let mut nulls = NullMap::default();
    for code in ORDER {
        let Some(r) = rules.iter().find(|r| r.code == code && r.severity == RuleSeverity::Refuse) else { continue };
        let limit = r.limit.unwrap_or(0);
        let mut n = 0u64;
        // optional_properties_max: optional properties seen so far, in walk order.
        let mut seen = 0u64;
        for (loc, sch) in locations(req) {
            let mut found: Vec<Path> = Vec::new();
            let mut visit = |node: &mut Map<String, Value>, path: Option<&Path>| {
                if rule(code, limit, node, path, &mut n, &mut seen, &mut found) {
                    n += 1;
                }
            };
            walk(sch, Some(&mut Vec::new()), &mut visit);
            if !found.is_empty() {
                match loc {
                    Loc::Tool(name) => nulls.tools.entry(name).or_default().extend(found),
                    Loc::Output => nulls.output.extend(found),
                }
            }
        }
        if n > 0 {
            codes.push(Adjustment(format!("schema:{code}:{n}")));
        }
    }
    (codes, nulls)
}

/// One rule on one node; true when the node counts as rewritten. The optional rules count properties in `n` instead.
fn rule(code: &str, limit: u64, node: &mut Map<String, Value>, path: Option<&Path>, n: &mut u64, seen: &mut u64, found: &mut Vec<Path>) -> bool {
    match code {
        "map_object" => match node.get("additionalProperties") {
            Some(Value::Bool(false)) | None => false,
            Some(_) => {
                moved(node, |k, _| k == "additionalProperties");
                node.insert("additionalProperties".into(), Value::Bool(false));
                true
            }
        },
        "one_of" => {
            if !node.contains_key("oneOf") {
                return false;
            }
            if node.contains_key("anyOf") {
                return moved(node, |k, _| k == "oneOf");
            }
            // Renamed in place: the key keeps its position.
            *node = std::mem::take(node).into_iter().map(|(k, v)| if k == "oneOf" { ("anyOf".into(), v) } else { (k, v) }).collect();
            true
        }
        "all_of" => {
            let Some(Value::Array(branches)) = node.get("allOf") else { return false };
            // Merged into a copy: a conflict leaves the node as the client sent it, not counted (Round 3, D1).
            let mut out = node.clone();
            out.shift_remove("allOf");
            // Until no `allOf` remains (Round 2, F5): a branch's own `allOf` branches are merged right after it.
            let mut stack: Vec<Value> = branches.iter().rev().cloned().collect();
            while let Some(b) = stack.pop() {
                let mut b = match b {
                    Value::Object(b) => b,
                    Value::Bool(true) => continue,
                    _ => return false,
                };
                let nested = match b.shift_remove("allOf") {
                    Some(Value::Array(a)) => a,
                    Some(v) => {
                        b.insert("allOf".into(), v);
                        Vec::new()
                    }
                    None => Vec::new(),
                };
                if !merge(&mut out, b) {
                    return false;
                }
                stack.extend(nested.into_iter().rev());
            }
            *node = out;
            true
        }
        "conditional" => moved(node, |k, _| matches!(k, "not" | "if" | "then" | "else" | "dependentRequired" | "dependentSchemas")),
        "closed_object" => {
            if (is_object_type(node) || node.contains_key("properties")) && !node.contains_key("additionalProperties") {
                node.insert("additionalProperties".into(), Value::Bool(false));
                true
            } else {
                false
            }
        }
        "numeric_bounds" => moved(node, |k, _| matches!(k, "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf")),
        "string_length" => moved(node, |k, _| matches!(k, "minLength" | "maxLength")),
        "array_bounds" => moved(node, |k, v| k == "maxItems" || (k == "minItems" && v.as_u64().is_none_or(|m| m > limit))),
        "pattern" => moved(node, |k, _| k == "pattern"),
        "optional_property" | "optional_properties_max" => {
            optional(node, path, code == "optional_properties_max", limit, n, seen, found);
            false
        }
        _ => false,
    }
}

/// Merges `from` into `into` key by key (Round 3, D1): an absent key is added, equal values stay, two `required`
/// arrays are united, and two objects (a map such as `properties`, or a schema) merge with this same merge. False
/// when a key has two values that cannot be merged; `into` is then partly merged.
fn merge(into: &mut Map<String, Value>, from: Map<String, Value>) -> bool {
    for (k, v) in from {
        let Some(have) = into.get_mut(&k) else {
            into.insert(k, v);
            continue;
        };
        if *have == v {
            continue;
        }
        let ok = match (have, v) {
            (Value::Array(r), Value::Array(br)) if k == "required" => {
                let mut seen: HashSet<Value> = r.iter().cloned().collect();
                r.extend(br.into_iter().filter(|x| seen.insert(x.clone())));
                true
            }
            (Value::Object(a), Value::Object(b)) => merge(a, b),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Each optional property (after the first `limit` when `max`) is made required and, unless it accepts null,
/// nullable, with its path in `found`. A node without a path is not rewritten.
fn optional(node: &mut Map<String, Value>, path: Option<&Path>, max: bool, limit: u64, n: &mut u64, seen: &mut u64, found: &mut Vec<Path>) {
    let Some(Value::Object(props)) = node.get("properties") else { return };
    let required: HashSet<&str> = node.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut add = Vec::new();
    // A `false` property must stay absent: not made required (Round 2, F3).
    for name in props.iter().filter(|(k, v)| !required.contains(k.as_str()) && **v != Value::Bool(false)).map(|(k, _)| k) {
        *seen += 1;
        if path.is_some() && (!max || *seen > limit) {
            add.push(name.clone());
        }
    }
    let Some(path) = path.filter(|_| !add.is_empty()) else { return };
    if let Some(Value::Object(props)) = node.get_mut("properties") {
        for name in &add {
            // A path only where a null was injected (Round 2, F2).
            if let Some(p) = props.get_mut(name)
                && !accepts_null(p)
                && nullable(p)
            {
                let mut x = path.clone();
                x.push(Seg::Key(name.clone()));
                found.push(x);
            }
        }
    }
    match node.get_mut("required").and_then(Value::as_array_mut) {
        Some(r) => r.extend(add.iter().map(|a| json!(a))),
        None => {
            node.insert("required".into(), json!(add));
        }
    }
    *n += add.len() as u64;
}

/// A held block (reading 16): its `index`, the tool name (`None` = a text block of the output schema), its held
/// events (event path) or bytes (raw path) in order, and the text of its deltas. Raw path: `others` = the held
/// non-delta events, and `first` = their length when the first delta came.
struct Hold {
    index: Value,
    tool: Option<String>,
    events: Vec<SseEvent>,
    bytes: Vec<u8>,
    others: Vec<u8>,
    first: Option<usize>,
    text: String,
    size: usize,
}

impl Hold {
    /// The delta's `partial_json` or `text`, added to `text`; its size counts toward HOLD_MAX.
    fn add(&mut self, d: &Value) {
        let k = if self.tool.is_some() { "partial_json" } else { "text" };
        match d.get("delta").and_then(|x| s(x, k)) {
            Some(x) => {
                self.text.push_str(x);
                self.size += x.len();
            }
            None => self.size += d.to_string().len(),
        }
    }
}

/// Removes injected nulls from a Messages response stream: holds one block's deltas until its `content_block_stop`;
/// a block from which a null was removed gets one delta with the compact JSON in place of its deltas.
pub struct RestoreFilter {
    tools: BTreeMap<String, Trie>,
    output: Option<Trie>,
    hold: Option<Hold>,
    /// Raw path: bytes after the last whole line; `pos` is where the newline search resumes.
    buf: Vec<u8>,
    pos: usize,
    /// Raw path: the current line went over HOLD_MAX; it passes unchanged up to its newline.
    over: bool,
    /// Raw path, holding: the lines of the current event and its `data:`.
    event: Vec<u8>,
    data: Option<Vec<u8>>,
}

fn kind(d: &Value) -> Option<&str> {
    s(d, "type")
}

impl RestoreFilter {
    pub fn new(nulls: NullMap) -> RestoreFilter {
        RestoreFilter {
            tools: nulls.tools.iter().map(|(n, p)| (n.clone(), Trie::of(p))).collect(),
            output: (!nulls.output.is_empty()).then(|| Trie::of(&nulls.output)),
            hold: None,
            buf: Vec::new(),
            pos: 0,
            over: false,
            event: Vec::new(),
            data: None,
        }
    }

    /// A `content_block_start` of a `tool_use` block named in the map, or of a text block when output paths exist.
    fn start(&self, d: &Value) -> Option<Hold> {
        if kind(d) != Some("content_block_start") {
            return None;
        }
        let b = d.get("content_block")?;
        let tool = match s(b, "type") {
            Some("tool_use") => Some(s(b, "name").filter(|n| self.tools.contains_key(*n))?.to_string()),
            Some("text") if self.output.is_some() => None,
            _ => return None,
        };
        Some(Hold { index: d.get("index")?.clone(), tool, events: Vec::new(), bytes: Vec::new(), others: Vec::new(), first: None, text: String::new(), size: 0 })
    }

    /// The one delta that replaces the block's deltas; `None` when no null was removed.
    fn rebuilt(&self, h: &Hold) -> Option<SseEvent> {
        let (t, d) = match &h.tool {
            Some(n) => (self.tools.get(n)?, "input_json_delta"),
            None => (self.output.as_ref()?, "text_delta"),
        };
        let x = t.strip_text(&h.text)?;
        let delta = if h.tool.is_some() { json!({"type":d,"partial_json":x}) } else { json!({"type":d,"text":x}) };
        Some(SseEvent { event: "content_block_delta".into(), data: json!({"type":"content_block_delta","index":h.index,"delta":delta}) })
    }

    /// Translated streams: one Messages event in, the events to send out.
    pub fn push(&mut self, ev: SseEvent) -> Vec<SseEvent> {
        let Some(h) = &mut self.hold else {
            self.hold = self.start(&ev.data);
            return vec![ev];
        };
        let ty = kind(&ev.data);
        let mine = ev.data.get("index") == Some(&h.index);
        // A ping is held in place with the block's deltas (Round 2, F1).
        let ping = ty == Some("ping");
        if ping || (ty == Some("content_block_delta") && mine) {
            if ping {
                h.size += ev.data.to_string().len();
            } else {
                h.add(&ev.data);
            }
            h.events.push(ev);
            // Over the bound: flushed unchanged; the rest of the block passes.
            if h.size > HOLD_MAX {
                return self.hold.take().map(|h| h.events).unwrap_or_default();
            }
            return Vec::new();
        }
        let Some(h) = self.hold.take() else { return vec![ev] };
        let mut out = match (ty == Some("content_block_stop") && mine).then(|| self.rebuilt(&h)).flatten() {
            // The held pings keep their order; the rebuilt delta goes where the first delta was.
            Some(e) => {
                let mut e = Some(e);
                let mut out = Vec::new();
                for x in h.events {
                    if kind(&x.data) == Some("content_block_delta") {
                        out.extend(e.take());
                    } else {
                        out.push(x);
                    }
                }
                out
            }
            None => h.events,
        };
        if !mine {
            self.hold = self.start(&ev.data);
        }
        out.push(ev);
        out
    }

    /// The held deltas, unchanged.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        self.hold.take().map(|h| h.events).unwrap_or_default()
    }

    /// Untranslated streams: raw SSE bytes in, bytes out. Every line passes as it came, except the delta events of a
    /// block from which a null was removed (reading 17).
    pub fn push_raw(&mut self, mut chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if self.over {
            match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    out.extend_from_slice(&chunk[..=i]);
                    chunk = &chunk[i + 1..];
                    self.over = false;
                }
                None => {
                    out.extend_from_slice(chunk);
                    return out;
                }
            }
        }
        let mut buf = std::mem::take(&mut self.buf);
        buf.extend_from_slice(chunk);
        let mut start = 0;
        while let Some(i) = buf[self.pos..].iter().position(|&b| b == b'\n') {
            let end = self.pos + i + 1;
            self.line(&buf[start..end], &mut out);
            start = end;
            self.pos = end;
        }
        buf.drain(..start);
        self.pos = buf.len();
        // Over the bound (§10.2.6): everything held goes out unchanged; the rest of the line and block pass.
        if self.hold.as_ref().map_or(0, |h| h.bytes.len()) + self.event.len() + buf.len() > HOLD_MAX {
            if let Some(h) = self.hold.take() {
                out.extend_from_slice(&h.bytes);
            }
            out.append(&mut self.event);
            self.data = None;
            self.over = !buf.is_empty();
            out.append(&mut buf);
            self.pos = 0;
        }
        self.buf = buf;
        out
    }

    /// The held bytes, unchanged.
    pub fn finish_raw(&mut self) -> Vec<u8> {
        let mut out = self.hold.take().map(|h| h.bytes).unwrap_or_default();
        out.append(&mut self.event);
        out.append(&mut self.buf);
        self.data = None;
        self.pos = 0;
        out
    }

    /// One whole line, with its "\n".
    fn line(&mut self, line: &[u8], out: &mut Vec<u8>) {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        let data = body.strip_prefix(b"data:").map(|d| d.strip_prefix(b" ").unwrap_or(d));
        if self.hold.is_none() {
            out.extend_from_slice(line);
            if let Some(v) = data.and_then(|d| serde_json::from_slice::<Value>(d).ok()) {
                self.hold = self.start(&v);
            }
            return;
        }
        // Over the bound: everything held and this line go out unchanged; the rest of the block passes.
        if self.hold.as_ref().map_or(0, |h| h.bytes.len()) + self.event.len() + line.len() > HOLD_MAX {
            if let Some(h) = self.hold.take() {
                out.extend_from_slice(&h.bytes);
            }
            out.append(&mut self.event);
            out.extend_from_slice(line);
            self.data = None;
            return;
        }
        self.event.extend_from_slice(line);
        if body.is_empty() {
            let ev = std::mem::take(&mut self.event);
            let data = self.data.take();
            self.raw_event(ev, data, out);
        } else if let Some(d) = data {
            match &mut self.data {
                Some(x) => {
                    x.push(b'\n');
                    x.extend_from_slice(d);
                }
                None => self.data = Some(d.to_vec()),
            }
        }
    }

    /// A whole event while holding.
    fn raw_event(&mut self, ev: Vec<u8>, data: Option<Vec<u8>>, out: &mut Vec<u8>) {
        let Some(h) = &mut self.hold else { return out.extend_from_slice(&ev) };
        // No `data:` line, data that is not JSON, or a ping: held in place with the block's deltas (Round 2, F1).
        let Some(v) = data.and_then(|d| serde_json::from_slice::<Value>(&d).ok()).filter(|v| kind(v) != Some("ping")) else {
            h.others.extend_from_slice(&ev);
            h.bytes.extend_from_slice(&ev);
            return;
        };
        let ty = kind(&v);
        let mine = v.get("index") == Some(&h.index);
        if ty == Some("content_block_delta") && mine {
            h.add(&v);
            h.first.get_or_insert(h.others.len());
            h.bytes.extend_from_slice(&ev);
            return;
        }
        let Some(h) = self.hold.take() else { return };
        match (ty == Some("content_block_stop") && mine).then(|| self.rebuilt(&h)).flatten() {
            // The held non-delta events keep their order; the rebuilt delta goes where the first delta was.
            Some(e) => {
                let at = h.first.unwrap_or(h.others.len());
                out.extend_from_slice(&h.others[..at]);
                out.extend_from_slice(&e.to_bytes());
                out.extend_from_slice(&h.others[at..]);
            }
            None => out.extend_from_slice(&h.bytes),
        }
        if !mine {
            self.hold = self.start(&v);
        }
        out.extend_from_slice(&ev);
    }
}
