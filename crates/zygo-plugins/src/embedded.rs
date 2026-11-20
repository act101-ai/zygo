//! The embedded `zygo` plugin (spec §15.5): commands generated from `VERB_SPECS` plus the `zygo-cli` skill.
//! It holds no code, so it needs no approval (§15.7).

use zygo_core::verbs::VERB_SPECS;

pub const NAME: &str = "zygo";

const SKILL: &str = include_str!("../embedded/skills/zygo-cli/SKILL.md");

/// A double-quoted YAML scalar.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The plugin files, plugin-relative paths, sorted.
pub fn files() -> Vec<(String, Vec<u8>)> {
    let manifest = serde_json::json!({
        "name": NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "description": "zygo commands and the zygo-cli skill",
    });
    let mut out = vec![
        (".claude-plugin/plugin.json".to_string(), serde_json::to_vec_pretty(&manifest).unwrap_or_default()),
        ("skills/zygo-cli/SKILL.md".to_string(), SKILL.as_bytes().to_vec()),
    ];
    for v in VERB_SPECS {
        let mut t = format!("---\ndescription: {}\n", quoted(v.summary));
        if !v.args.is_empty() { t.push_str(&format!("argument-hint: {}\n", quoted(v.args))); }
        t.push_str(&format!(
            "disable-model-invocation: true\n---\nzygo handles /zygo:{0} before the model sees it. If this text reaches you, run: zygo {0} $ARGUMENTS\n",
            v.name));
        out.push((format!("commands/{}.md", v.name), t.into_bytes()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
