//! Stage 3, the soft context window (§10.2): window arithmetic, the triggers and the `message_delta` usage rewrite.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

use http::HeaderMap;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::Value;
use zygo_config::{Dialect, Facts, ModelRow};
use zygo_core::ClientSpec;

use crate::errors::ZygoError;
use crate::pipeline::{Adjustment, ModelRoute, RequestCtx, ResponseCtx, Stage};
use crate::session::{SessionState, WindowArg};
use crate::tee::{SseLine, Usage};
use crate::translate::responses_client::flat_name;

/// Claude Code's assumed window for an unrecognized id (§10.2.2).
pub const UNRECOGNIZED_WINDOW: u64 = 1_000_000;

/// `facts.context_window` when the row has facts and it is > 0.
pub fn context_window(row: &ModelRow) -> Option<u64> {
    row.facts.as_ref().map(|f| f.context_window).filter(|&n| n > 0)
}

/// A Claude id (`claude-…`): its context window (0 without one); else `UNRECOGNIZED_WINDOW` (§10.2.2).
pub fn agent_window(row: &ModelRow) -> u64 {
    if row.id.starts_with("claude-") { context_window(row).unwrap_or(0) } else { UNRECOGNIZED_WINDOW }
}

/// Claude Code's assumed window for a name with the `[1m]` suffix (§10.2.2 note, N15-1M).
pub const ONE_M_WINDOW: u64 = 1_000_000;

/// The `anthropic-beta` token prefix Claude Code sends for a `[1m]` name; it removes the suffix from `model`
/// (Q7, 2.1.328: alias `design[1m]` → `model` `design`, beta `context-1m-2025-08-07`).
const ONE_M_BETA: &str = "context-1m-";

/// The agent sizes this request's model at 1M: the name kept its `[1m]` suffix (`suffix`, stage 2), or a
/// comma-separated token of any `anthropic-beta` header starts with `context-1m-`.
pub fn one_m(suffix: bool, headers: &HeaderMap) -> bool {
    suffix
        || headers
            .get_all("anthropic-beta")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .any(|t| t.trim().starts_with(ONE_M_BETA))
}

/// The agent's assumed window for one request (§10.2.2, AU29): 1M for a `[1m]` request, else `agent_window(row)`.
pub(crate) fn agent_window_for(suffix: bool, headers: &HeaderMap, row: &ModelRow) -> u64 {
    if one_m(suffix, headers) { ONE_M_WINDOW } else { agent_window(row) }
}

/// What the status needs of a served row (AU29): its id, its soft window, its context window and the agent window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestWindows {
    pub(crate) id: String,
    pub(crate) soft: Option<u64>,
    pub(crate) context_window: Option<u64>,
    pub(crate) agent_window: u64,
}

impl RequestWindows {
    pub(crate) fn of(row: &ModelRow, agent_window: u64) -> RequestWindows {
        RequestWindows { id: row.id.clone(), soft: row.soft_context_window, context_window: context_window(row), agent_window }
    }
    /// A row holding the values the window functions read: `id` (`agent_window`), `soft_context_window` and
    /// `facts.context_window` (`soft_threshold`, `effective_for`, `context_window`); the rest is default.
    pub(crate) fn view(&self) -> ModelRow {
        let facts = self.context_window.map(|context_window| Facts { context_window, ..Default::default() });
        ModelRow { id: self.id.clone(), soft_context_window: self.soft, facts, ..Default::default() }
    }
}

/// 90 % of `cw` (floor) when the agent assumes a larger window (§10.2.1).
pub fn safety(cw: u64, agent_window: u64) -> Option<u64> {
    (agent_window > cw).then(|| (u128::from(cw) * 9 / 10) as u64)
}

/// The rewrite target (§10.2.2).
pub fn window(cw: u64, agent_window: u64) -> u64 {
    cw.max(agent_window)
}

/// input + cache_read + cache_write, saturating (§10.2.1).
pub fn prompt(u: Usage) -> u64 {
    u.input.saturating_add(u.cache_read).saturating_add(u.cache_write)
}

/// The `zygo` MCP server's `compact` tool in the client's MCP tool-name form (`ClientSpec::mcp_server`), its server
/// ending in `zygo` (§10.2.3 trigger 2); never for a client that zygo does not list `compact` to
/// (`ClientSpec::lists_compact_tool`).
pub fn compact_tool(client: &ClientSpec, name: &str) -> bool {
    client.lists_compact_tool && (client.mcp_server)(name, "compact").is_some_and(|s| s.ends_with("zygo"))
}

/// The compact tool in a Responses `function_call` item (K§6.4.5): its `name`, joined to its `namespace` by
/// `flat_name` when it has one, is the §10.2.3 name `mcp__<server>__compact` with `<server>` ending in `zygo`. The
/// joined form is wire format (K§6.4.3), the same for every client, so it is not read with `ClientSpec::mcp_server`;
/// never for a client that zygo does not list `compact` to (`ClientSpec::lists_compact_tool`).
pub fn responses_compact_tool(client: &ClientSpec, namespace: Option<&str>, name: &str) -> bool {
    if !client.lists_compact_tool {
        return false;
    }
    let name = match namespace {
        Some(ns) => Cow::Owned(flat_name(ns, name)),
        None => Cow::Borrowed(name),
    };
    name.strip_suffix("__compact").and_then(|ns| ns.strip_prefix("mcp__")).is_some_and(|s| s.ends_with("zygo"))
}

/// Trigger 1's threshold: the override, else `soft`; none when `off`; then at most `safety` (§10.2.1).
pub fn soft_threshold(compact: Option<WindowArg>, row: &ModelRow, safety: Option<u64>) -> Option<u64> {
    let t = match compact {
        Some(WindowArg::Tokens(n)) => Some(n),
        Some(WindowArg::Off) => None,
        None => row.soft_context_window,
    };
    t.map(|n| safety.map_or(n, |s| n.min(s)))
}

/// §10.2.1 `effective` for `served` under the override `compact`: the trigger 1 threshold, else the safety window;
/// none without a context window.
pub fn effective_for(compact: Option<WindowArg>, served: &ModelRow, agent_window: u64) -> Option<u64> {
    let s = safety(context_window(served)?, agent_window);
    soft_threshold(compact, served, s).or(s)
}

/// `effective_for` with the session's override.
pub fn effective(s: &SessionState, served: &ModelRow, agent_window: u64) -> Option<u64> {
    effective_for(s.compact(), served, agent_window)
}

/// Stage 3 (§10.2): arms line mode at request time; on the response, the three triggers and the usage rewrite.
pub struct WindowStage;

impl Stage for WindowStage {
    /// Common conditions (§10.2.2): Messages or Responses (K§6.4.5), armed for the client (`ClientSpec::window_rewrite`),
    /// not a compaction request, the served model has a context window. The proxy step adds a 2xx SSE upstream response.
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        let client = ctx.session.agent.spec();
        ctx.line_mode = matches!(ctx.route, ModelRoute::Messages | ModelRoute::Responses)
            && client.window_rewrite
            && !client.compaction_header.is_some_and(|h| ctx.headers.contains_key(h))
            && ctx.resolved.as_ref().and_then(|r| ctx.cfg.config.catalog.get(&r.model)).and_then(context_window).is_some();
        Ok(())
    }

    /// Called only in line mode. A compact tool call start is noted; the terminal usage line is rewritten once when
    /// any trigger fires, and each fired trigger's code is recorded once (§10.2.3). The lines are Responses lines
    /// when `ctx.lines` is `Openai` (K§6.4.5), else Messages lines.
    fn on_response_line(&self, ctx: &mut ResponseCtx, line: &mut SseLine) {
        if !ctx.status.is_success() {
            return;
        }
        match ctx.lines {
            Dialect::Openai => responses_line(ctx, line),
            _ => messages_line(ctx, line),
        }
    }
}

/// A Messages line: a compact `tool_use` start is noted, or the `message_delta` usage is rewritten (§10.2.3).
fn messages_line(ctx: &mut ResponseCtx, line: &mut SseLine) {
    let Some(d) = line.0.strip_prefix(b"data:") else { return };
    let has = |p: &[u8]| d.windows(p.len()).any(|w| w == p);
    if !has(b"content_block_start") && !has(b"message_delta") {
        return;
    }
    let Ok(v) = serde_json::from_slice::<Value>(d) else { return };
    match v.get("type").and_then(Value::as_str) {
        Some("content_block_start") => {
            let b = &v["content_block"];
            if b["type"] == "tool_use" && b["name"].as_str().is_some_and(|n| compact_tool(ctx.session.agent.spec(), n)) {
                ctx.compact_call = true;
            }
        }
        Some("message_delta") => {
            let Some((window, fired)) = triggers(ctx, v["delta"]["stop_reason"] == "end_turn") else { return };
            let Some(out) = rewrite_usage(&line.0, ctx.usage, window) else { return };
            line.0 = out;
            record(ctx, fired);
        }
        _ => {}
    }
}

/// The Responses output item types that call a tool (K§6.4.5): a response with one does not end the turn.
const CALL_ITEMS: [&str; 6] = ["function_call", "custom_tool_call", "local_shell_call", "computer_call", "mcp_call", "mcp_approval_request"];

/// `item` is a call item (`CALL_ITEMS`).
fn call_item(item: &Value) -> bool {
    item["type"].as_str().is_some_and(|t| CALL_ITEMS.contains(&t))
}

/// A Responses line (K§6.4.5): a call item in a streamed item event is noted, and a compact `function_call` item
/// start; or the `response.completed` or `response.incomplete` usage is rewritten.
fn responses_line(ctx: &mut ResponseCtx, line: &mut SseLine) {
    let Some(d) = line.0.strip_prefix(b"data:") else { return };
    let has = |p: &[u8]| d.windows(p.len()).any(|w| w == p);
    if !has(b"response.output_item.") && !has(b"response.completed") && !has(b"response.incomplete") {
        return;
    }
    let Ok(v) = serde_json::from_slice::<Value>(d) else { return };
    match v.get("type").and_then(Value::as_str) {
        Some(ty @ ("response.output_item.added" | "response.output_item.done")) => {
            let item = &v["item"];
            ctx.call_item |= call_item(item);
            if ty != "response.output_item.added" {
                return;
            }
            let Some(name) = item["name"].as_str().filter(|_| item["type"] == "function_call") else { return };
            if responses_compact_tool(ctx.session.agent.spec(), item["namespace"].as_str(), name) {
                ctx.compact_call = true;
            }
        }
        Some(ty @ ("response.completed" | "response.incomplete")) => {
            let calls = ctx.call_item || v["response"]["output"].as_array().is_some_and(|o| o.iter().any(call_item));
            let Some((window, fired)) = triggers(ctx, ty == "response.completed" && !calls) else { return };
            let Some(out) = rewrite_responses_usage(&line.0, ctx.usage, window) else { return };
            line.0 = out;
            record(ctx, fired);
        }
        _ => {}
    }
}

/// The rewrite target and which of triggers 0, 1 and 2 fired (§10.2.3); `end_turn` is trigger 1's end of turn.
/// `None` without a served row or a context window, or when no trigger fired.
fn triggers(ctx: &ResponseCtx, end_turn: bool) -> Option<(u64, [bool; 3])> {
    let row = ctx.cfg.config.catalog.get(&ctx.resolved.model)?;
    let cw = context_window(row)?;
    let aw = agent_window_for(ctx.resolved.one_m, &ctx.headers, row);
    let s = safety(cw, aw);
    let p = prompt(ctx.usage);
    let main = ctx.session.agent.spec().request_class_header.and_then(|h| ctx.headers.get(h)).is_some_and(|c| c == "main");
    let t0 = s.is_some_and(|s| p > s);
    let t1 = main && end_turn && soft_threshold(ctx.session.compact(), row, s).is_some_and(|t| p > t);
    let t2 = ctx.compact_call;
    (t0 || t1 || t2).then_some((window(cw, aw), [t0, t1, t2]))
}

/// Each fired trigger's code, recorded once (§10.2.3).
fn record(ctx: &mut ResponseCtx, fired: [bool; 3]) {
    for (fired, code) in fired.into_iter().zip(["context_safety", "context_signal", "context_compact"]) {
        if fired && !ctx.adjustments.iter().any(|a| a.0 == code) {
            ctx.adjustments.push(Adjustment(code.into()));
        }
    }
}

/// An object key, unescaped; borrowed when it has no escape.
struct Key<'a>(Cow<'a, str>);

impl<'de> Deserialize<'de> for Key<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Key<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E>(self, s: &'de str) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Borrowed(s)))
            }
            fn visit_str<E>(self, s: &str) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Owned(s.to_owned())))
            }
        }
        d.deserialize_str(V)
    }
}

/// An object's members in received order, repeated keys kept.
struct Obj<'a>(Vec<(Cow<'a, str>, &'a RawValue)>);

impl<'de> Deserialize<'de> for Obj<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Obj<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Obj<'de>, A::Error> {
                let mut v = Vec::new();
                while let Some((Key(k), raw)) = m.next_entry::<Key<'de>, &'de RawValue>()? {
                    v.push((k, raw));
                }
                Ok(Obj(v))
            }
        }
        d.deserialize_map(V)
    }
}

impl<'a> Obj<'a> {
    /// The last member named `k` (as `JSON.parse` keeps it).
    fn raw(&self, k: &str) -> Option<&'a RawValue> {
        self.0.iter().rev().find(|(x, _)| x == k).map(|(_, v)| *v)
    }
}

/// One edit of the line: the byte range and its new text.
type Edit = (usize, usize, String);

/// The edits that set `vals` in object `o`, whose `{` is at `start`: each present value replaced, the absent fields
/// inserted after `{`. `None` when `o` repeats a key.
fn edit_obj(o: &Obj, start: usize, off: &dyn Fn(&RawValue) -> usize, vals: &[(&str, u64)], edits: &mut Vec<Edit>) -> Option<()> {
    let mut seen = HashSet::new();
    if !o.0.iter().all(|(k, _)| seen.insert(k.as_ref())) {
        return None;
    }
    let mut ins = String::new();
    for (k, n) in vals {
        match o.0.iter().find(|(x, _)| x == k) {
            Some((_, v)) => edits.push((off(v), off(v) + v.get().len(), n.to_string())),
            None => ins += &format!("\"{k}\":{n},"),
        }
    }
    if o.0.is_empty() {
        ins.pop();
    }
    if !ins.is_empty() {
        edits.push((start + 1, start + 1, ins));
    }
    Some(())
}

/// The `message_delta` data line with `input_tokens`, `cache_creation_input_tokens` and `cache_read_input_tokens` set
/// so the agent's measure equals `window` (§10.2.2), in `usage` and in the last `usage.iterations` object. Every other
/// byte is kept. `None` (line unchanged): not UTF-8, not a `message_delta` with a `usage` object, or an edited object
/// repeats a key.
pub fn rewrite_usage(line: &[u8], u: Usage, window: u64) -> Option<Vec<u8>> {
    let line = std::str::from_utf8(line).ok()?;
    let top: Obj = serde_json::from_str(line.strip_prefix("data:")?).ok()?;
    if serde_json::from_str::<String>(top.raw("type")?.get()).ok().as_deref() != Some("message_delta") {
        return None;
    }
    let usage = top.raw("usage").filter(|r| r.get().starts_with('{'))?;
    let base = line.as_ptr() as usize;
    let off = |r: &RawValue| r.get().as_ptr() as usize - base;
    let read = window.saturating_sub(u.input).saturating_sub(u.cache_write).max(u.cache_read);
    let vals = [("input_tokens", u.input), ("cache_creation_input_tokens", u.cache_write), ("cache_read_input_tokens", read)];
    let mut edits = Vec::new();
    let uo: Obj = serde_json::from_str(usage.get()).ok()?;
    edit_obj(&uo, off(usage), &off, &vals, &mut edits)?;
    if let Some(it) = uo.raw("iterations")
        && let Ok(items) = serde_json::from_str::<Vec<&RawValue>>(it.get())
        && let Some(last) = items.last().filter(|r| r.get().starts_with('{'))
    {
        let lo: Obj = serde_json::from_str(last.get()).ok()?;
        edit_obj(&lo, off(last), &off, &vals, &mut edits)?;
    }
    // The edits never overlap: applied from the end, each offset still holds.
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = line.to_string();
    for (a, b, text) in edits {
        out.replace_range(a..b, &text);
    }
    Some(out.into_bytes())
}

/// The `response.completed` or `response.incomplete` data line with `response.usage` set by K§6.4.5 so that the
/// client's measure equals `window`: `input_tokens_details.cached_tokens` := max(window − input − cache_write,
/// cache_read), `input_tokens` := input + cache_write + cached_tokens, `total_tokens` := input_tokens + u.output;
/// a missing field inserted; every other byte kept. `None` (line unchanged): not UTF-8, no `data:` prefix, another
/// `type`, no `response.usage` object, `input_tokens_details` not an object, an edited object repeats a key, or a
/// present `input_tokens`, `cached_tokens` or `output_tokens` is not a u64 integer (the tee then read 0 for it).
pub fn rewrite_responses_usage(line: &[u8], u: Usage, window: u64) -> Option<Vec<u8>> {
    let int = |r: Option<&RawValue>| r.is_none_or(|r| serde_json::from_str::<u64>(r.get()).is_ok()).then_some(());
    let line = std::str::from_utf8(line).ok()?;
    let top: Obj = serde_json::from_str(line.strip_prefix("data:")?).ok()?;
    let ty = serde_json::from_str::<String>(top.raw("type")?.get()).ok()?;
    if ty != "response.completed" && ty != "response.incomplete" {
        return None;
    }
    let resp = top.raw("response").filter(|r| r.get().starts_with('{'))?;
    let ro: Obj = serde_json::from_str(resp.get()).ok()?;
    let usage = ro.raw("usage").filter(|r| r.get().starts_with('{'))?;
    let uo: Obj = serde_json::from_str(usage.get()).ok()?;
    int(uo.raw("input_tokens"))?;
    int(uo.raw("output_tokens"))?;
    let base = line.as_ptr() as usize;
    let off = |r: &RawValue| r.get().as_ptr() as usize - base;
    let cached = window.saturating_sub(u.input).saturating_sub(u.cache_write).max(u.cache_read);
    let input = u.input.saturating_add(u.cache_write).saturating_add(cached);
    let total = input.saturating_add(u.output);
    let mut edits = Vec::new();
    let details = uo.raw("input_tokens_details");
    if let Some(det) = details {
        let det = Some(det).filter(|r| r.get().starts_with('{'))?;
        let dobj: Obj = serde_json::from_str(det.get()).ok()?;
        int(dobj.raw("cached_tokens"))?;
        edit_obj(&dobj, off(det), &off, &[("cached_tokens", cached)], &mut edits)?;
    }
    edit_obj(&uo, off(usage), &off, &[("input_tokens", input), ("total_tokens", total)], &mut edits)?;
    if details.is_none() {
        // Pushed last, so it lands before any text `edit_obj` inserted after the same `{`; that text has no trailing
        // comma only when `usage` was empty, and then it is never empty.
        let at = off(usage) + 1;
        edits.push((at, at, format!("\"input_tokens_details\":{{\"cached_tokens\":{cached}}},")));
    }
    // The edits never overlap: applied from the end (equal offsets in push order), each offset still holds.
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = line.to_string();
    for (a, b, text) in edits {
        out.replace_range(a..b, &text);
    }
    Some(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, soft: Option<u64>, cw: u64) -> ModelRow {
        ModelRow { id: id.into(), soft_context_window: soft, facts: Some(Facts { context_window: cw, ..Default::default() }), ..Default::default() }
    }

    #[test]
    fn view_keeps_the_agent_window_of_the_row() { // HUD-5 round 2 (F4): `view()` is a row `agent_window` reads right
        for r in [row("claude-t-1", Some(150_000), 200_000), row("gpt-t-1", None, 400_000), row("claude-nf", None, 0)] {
            let w = RequestWindows::of(&r, agent_window(&r));
            assert_eq!(agent_window(&w.view()), w.agent_window, "{}", r.id);
            assert_eq!((w.view().soft_context_window, context_window(&w.view())), (r.soft_context_window, context_window(&r)), "{}", r.id);
        }
        assert_eq!(agent_window(&RequestWindows::of(&row("claude-t-1", None, 200_000), 200_000).view()), 200_000);
    }
}
