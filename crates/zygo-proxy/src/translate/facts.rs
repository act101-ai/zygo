//! Model facts on a translated request (§7.6 facts table): applied to the Anthropic working copy before translation;
//! each change is an adjustment code, in table order.

use serde_json::{json, Value};
use zygo_config::{Dialect, Effort, Facts, Reasoning, StructuredOutput};

use super::s;
use crate::pipeline::Adjustment;

pub const IMAGE_OMITTED: &str = "[image omitted: the model does not accept images]";
pub const SCHEMA_PROMPT: &str = "Answer with one JSON object that fits this schema: ";

/// On the Anthropic working copy of a translated request: effort, `max_output_tokens`, sampling, vision, forced tool
/// choice, structured output. Codes in table order.
pub fn apply(req: &mut Value, f: &Facts) -> Vec<Adjustment> {
    let mut out = Vec::new();
    let Some(obj) = req.as_object_mut() else { return out };
    effort(obj, f, &mut out);
    if f.max_output_tokens > 0
        && let Some(n) = obj.get("max_tokens").and_then(Value::as_u64).filter(|&n| n > f.max_output_tokens)
    {
        obj.insert("max_tokens".into(), json!(f.max_output_tokens));
        out.push(Adjustment(format!("max_output_clamped:{n}>{}", f.max_output_tokens)));
    }
    if !f.sampling {
        let t = obj.shift_remove("temperature").is_some();
        let p = obj.shift_remove("top_p").is_some();
        if t || p {
            out.push(Adjustment("sampling_dropped".into()));
        }
    }
    if !f.vision {
        let mut n = 0;
        for m in obj.get_mut("messages").and_then(Value::as_array_mut).into_iter().flatten() {
            for b in m.get_mut("content").and_then(Value::as_array_mut).into_iter().flatten() {
                omit_image(b, &mut n);
                if s(b, "type") == Some("tool_result") {
                    for x in b.get_mut("content").and_then(Value::as_array_mut).into_iter().flatten() {
                        omit_image(x, &mut n);
                    }
                }
            }
        }
        if n > 0 {
            out.push(Adjustment(format!("images_dropped:{n}")));
        }
    }
    if !f.forced_tool_choice
        && let Some(tc) = obj.get_mut("tool_choice").filter(|tc| s(tc, "type") == Some("tool"))
    {
        let mut any = json!({"type":"any"});
        if let Some(d) = tc.get("disable_parallel_tool_use") {
            any["disable_parallel_tool_use"] = d.clone();
        }
        *tc = any;
        out.push(Adjustment("tool_choice_widened".into()));
    }
    if f.structured_output == StructuredOutput::None
        && let Some(schema) = obj
            .get("output_config")
            .and_then(|oc| oc.get("format"))
            .filter(|f| s(f, "type") == Some("json_schema"))
            .and_then(|f| f.get("schema"))
            .map(Value::to_string)
    {
        if let Some(oc) = obj.get_mut("output_config").and_then(Value::as_object_mut) {
            oc.shift_remove("format");
            if oc.is_empty() {
                obj.shift_remove("output_config");
            }
        }
        let p = format!("{SCHEMA_PROMPT}{schema}");
        match obj.get_mut("system") {
            Some(Value::String(x)) => {
                x.push_str("\n\n");
                x.push_str(&p);
            }
            Some(Value::Array(a)) => a.push(json!({"type":"text","text":p})),
            _ => {
                obj.insert("system".into(), Value::String(p));
            }
        }
        out.push(Adjustment("structured_output_prompted".into()));
    }
    out
}

fn omit_image(b: &mut Value, n: &mut u32) {
    if s(b, "type") == Some("image") {
        *b = json!({"type":"text","text":IMAGE_OMITTED});
        *n += 1;
    }
}

/// The lowercase ladder name.
fn name(e: Effort) -> String {
    serde_json::to_value(e).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// Reads the client's effort (`thinking.type = disabled` → `none`, first), removes `thinking` and
/// `output_config.effort`, and sets `output_config.effort` to the value to send.
fn effort(obj: &mut serde_json::Map<String, Value>, f: &Facts, out: &mut Vec<Adjustment>) {
    let disabled = obj.get("thinking").and_then(|t| s(t, "type")) == Some("disabled");
    obj.shift_remove("thinking");
    let raw = obj.get_mut("output_config").and_then(Value::as_object_mut).and_then(|oc| oc.shift_remove("effort"));
    let from = if disabled { Some(Some(Effort::None)) } else { raw.map(|v| serde_json::from_value::<Effort>(v).ok()) };
    let sent = match from {
        None => None,
        Some(None) => {
            out.push(Adjustment("effort_dropped".into()));
            None
        }
        Some(Some(_)) if f.reasoning == Reasoning::None || f.effort_levels.is_empty() => {
            out.push(Adjustment("effort_dropped".into()));
            None
        }
        Some(Some(e)) if f.effort_levels.contains(&e) => Some(e),
        Some(Some(e)) => {
            // Nearest by ladder; the lower on a tie.
            let to = f.effort_levels.iter().copied().min_by_key(|&l| ((l as i8 - e as i8).abs(), l)).unwrap_or(e);
            out.push(Adjustment(format!("effort_clamped:{}>{}", name(e), name(to))));
            Some(to)
        }
    };
    match sent {
        Some(e) => match obj.get_mut("output_config").and_then(Value::as_object_mut) {
            Some(oc) => {
                oc.insert("effort".into(), json!(name(e)));
            }
            None => {
                obj.insert("output_config".into(), json!({"effort":name(e)}));
            }
        },
        None => {
            if obj.get("output_config").and_then(Value::as_object).is_some_and(|oc| oc.is_empty()) {
                obj.shift_remove("output_config");
            }
        }
    }
}

/// `from.output_config.effort` (set by `apply`) → Responses `reasoning.effort` / chat `reasoning_effort` in `out`;
/// nothing when absent or `to` is `anthropic` (v1 has no translation to it).
pub fn write_effort(from: &Value, out: &mut Value, to: Dialect) {
    let Some(e) = from.get("output_config").and_then(|oc| s(oc, "effort")) else { return };
    let Some(o) = out.as_object_mut() else { return };
    match to {
        Dialect::Openai => {
            o.insert("reasoning".into(), json!({"effort":e}));
        }
        Dialect::OpenaiChat => {
            o.insert("reasoning_effort".into(), json!(e));
        }
        Dialect::Anthropic => {}
    }
}
