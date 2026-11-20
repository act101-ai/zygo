//! The reasoning round trip (§7.6): a Responses reasoning item travels in an Anthropic `thinking` signature.

use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Target;

pub const PREFIX: &str = "zygo1:";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub provider: String,
    pub model: String,
    pub id: String,
    pub encrypted_content: String,
}

/// C3: the encrypted content is shown as its length only.
impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signature")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("id", &self.id)
            .field("encrypted_content", &format_args!("<{} bytes>", self.encrypted_content.len()))
            .finish()
    }
}

/// `zygo1:` + base64 of `{"provider","model","id","encrypted_content"}`.
pub fn encode(t: &Target, id: &str, encrypted_content: &str) -> String {
    let s = Signature {
        provider: t.provider.clone(),
        model: t.upstream_model.clone(),
        id: id.into(),
        encrypted_content: encrypted_content.into(),
    };
    format!("{PREFIX}{}", STANDARD.encode(serde_json::to_vec(&s).unwrap_or_default()))
}

/// `None` unless `PREFIX` + valid base64 + JSON with the four string fields.
pub fn decode(sig: &str) -> Option<Signature> {
    let raw = STANDARD.decode(sig.strip_prefix(PREFIX)?).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// A `thinking` block whose signature has the zygo prefix, whether or not it decodes.
pub(crate) fn is_zygo(b: &Value) -> bool {
    b.get("type").and_then(Value::as_str) == Some("thinking")
        && b.get("signature").and_then(Value::as_str).is_some_and(|s| s.starts_with(PREFIX))
}

/// Toward `anthropic`: a copy without zygo blocks and without assistant messages left empty, and the count; `None`
/// when there is none.
pub fn without_zygo_thinking(req: &Value) -> Option<(Value, u32)> {
    let msgs = req.get("messages")?.as_array()?;
    let n: usize = msgs
        .iter()
        .filter_map(|m| m.get("content").and_then(Value::as_array))
        .map(|c| c.iter().filter(|b| is_zygo(b)).count())
        .sum();
    if n == 0 {
        return None;
    }
    let mut out = req.clone();
    let kept: Vec<Value> = msgs
        .iter()
        .filter_map(|m| {
            let Some(c) = m.get("content").and_then(Value::as_array) else { return Some(m.clone()) };
            if !c.iter().any(is_zygo) {
                return Some(m.clone());
            }
            let rest: Vec<Value> = c.iter().filter(|b| !is_zygo(b)).cloned().collect();
            if rest.is_empty() && m.get("role").and_then(Value::as_str) == Some("assistant") {
                return None;
            }
            let mut m = m.clone();
            m["content"] = Value::Array(rest);
            Some(m)
        })
        .collect();
    out["messages"] = Value::Array(kept);
    Some((out, u32::try_from(n).unwrap_or(u32::MAX)))
}
