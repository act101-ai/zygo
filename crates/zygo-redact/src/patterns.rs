//! Compiles `[[redaction.pattern]]` and the act-lite store values (§16.1).

use crate::act_lite_store::StoredValue;
use anyhow::{anyhow, bail};
use sha2::{Digest, Sha256};
use zygo_config::{Action, ContentConfig, RedactionConfig};

pub const ACT_LITE_ID: &str = "act_lite_secret";

pub fn compile(cfg: &RedactionConfig) -> anyhow::Result<Compiled> {
    compile_with_store(cfg, &[])
}

/// `v` as it appears inside a JSON string: serde_json's escaping, then (`ascii`) `\uXXXX` (UTF-16,
/// lower- or upper-case hex) for each non-ASCII character.
fn json_escaped(v: &str, ascii: Option<bool>) -> String {
    let quoted = serde_json::to_string(v).unwrap_or_default();
    let inner = quoted.get(1..quoted.len().saturating_sub(1)).unwrap_or_default();
    let Some(upper) = ascii else { return inner.to_string() };
    let mut out = String::with_capacity(inner.len());
    for ch in inner.chars() {
        if ch.is_ascii() {
            out.push(ch);
            continue;
        }
        let mut u = [0u16; 2];
        for x in ch.encode_utf16(&mut u) {
            out.push_str(&if upper { format!("\\u{x:04X}") } else { format!("\\u{x:04x}") });
        }
    }
    out
}

/// `cfg.pattern` in config order, preceded by one exact-value pattern for `stored` when it is non-empty.
/// Each stored value also matches in its JSON-string-escaped and ASCII-escaped forms.
pub fn compile_with_store(cfg: &RedactionConfig, stored: &[StoredValue]) -> anyhow::Result<Compiled> {
    let mut values: Vec<String> = stored
        .iter()
        .map(StoredValue::expose)
        .flat_map(|v| [v.to_string(), json_escaped(v, None), json_escaped(v, Some(false)), json_escaped(v, Some(true))])
        .collect();
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    let mut specs: Vec<(String, String, Action)> = Vec::new();
    if !values.is_empty() {
        let re = values.iter().map(|v| regex::escape(v)).collect::<Vec<_>>().join("|");
        specs.push((ACT_LITE_ID.to_string(), re, Action::Redact));
    }
    specs.extend(cfg.pattern.iter().map(|p| (p.id.clone(), p.pattern.clone(), p.action)));
    if specs.len() > u16::MAX as usize {
        bail!("too many redaction patterns");
    }
    let mut pats = Vec::with_capacity(specs.len());
    let mut h = Sha256::new();
    h.update(b"zygo-secret-v2\n");
    for (i, (id, re, action)) in specs.iter().enumerate() {
        let compiled = regex::Regex::new(re).map_err(|e| {
            if i == 0 && !values.is_empty() {
                anyhow!("act-lite store values do not compile")
            } else {
                anyhow!("redaction pattern {id} does not compile: {e}")
            }
        })?;
        let secret_groups = compiled
            .capture_names()
            .enumerate()
            .filter(|(_, n)| n.is_some_and(|n| n.starts_with("secret")))
            .map(|(g, _)| g)
            .collect();
        let act: &[u8] = match action {
            Action::Redact => b"redact",
            Action::Block => b"block",
        };
        for x in [id.as_bytes(), re.as_bytes(), act] {
            h.update((x.len() as u64).to_le_bytes());
            h.update(x);
        }
        pats.push(Pat { id: id.clone(), re: compiled, action: *action, secret_groups });
    }
    let set = regex::RegexSet::new(specs.iter().map(|s| &s.1)).map_err(|_| anyhow!("redaction patterns do not compile"))?;
    let mut version = [0u8; 16];
    version.copy_from_slice(&h.finalize()[..16]);
    let ids = specs.iter().map(|s| regex::escape(&s.0)).collect::<Vec<_>>().join("|");
    let placeholder = regex::Regex::new(&format!(r"\[REDACTED:(?:{ids})\]")).expect("placeholder regex");
    Ok(Compiled { set, pats, version, placeholder, content: cfg.content.clone() })
}

/// No `Debug` (the exact-value regex holds store values).
pub struct Compiled {
    pub(crate) set: regex::RegexSet,
    pub(crate) pats: Vec<Pat>,
    version: [u8; 16],
    /// `[REDACTED:<configured id>]`; a span wholly inside one is dropped.
    pub(crate) placeholder: regex::Regex,
    pub content: ContentConfig,
}

pub(crate) struct Pat {
    pub id: String,
    pub re: regex::Regex,
    pub action: Action,
    pub secret_groups: Vec<usize>,
}

impl Compiled {
    pub fn ids(&self) -> Vec<&str> {
        self.pats.iter().map(|p| p.id.as_str()).collect()
    }
    /// For cache keying only; never logged (it hashes the act-lite value set).
    pub fn version(&self) -> &[u8; 16] {
        &self.version
    }
}
