//! The embedded `zygo` plugin (spec §15.5): commands generated from `VERB_SPECS` plus the `zygo-cli` skill.
//! It holds no code, so it needs no approval (§15.7).

use zygo_core::verbs::{DOWN_SLASH_ARGS, MAIL_SLASH_ARGS, VERB_SPECS};

pub const NAME: &str = "zygo";

const SKILL: &str = include_str!("../embedded/skills/zygo-cli/SKILL.md");

/// The body of `commands/down.md` (OP11): `zygo down` in a shell names the folder's workspace, not the session's.
const DOWN_BODY: &str = "zygo handles /zygo:down before the model sees it. If this text reaches you, /zygo:down did not run: tell the user that it failed. Do not run zygo down in a shell: it stops the workspace of the current folder, not the workspace of this session.\n";

/// The body of `commands/mail.md` (M§8): `/zygo:mail` lists only, so the fallback runs only `zygo mail ls`; send and
/// read stay MCP tools.
const MAIL_BODY: &str = "zygo handles /zygo:mail before the model sees it. If this text reaches you, the arguments are: $ARGUMENTS\nIf they are empty or their first word is ls, run: zygo mail ls $ARGUMENTS (take out that first ls, so that ls is not given twice). For any other first word, do not run zygo: tell the user that /zygo:mail only lists, and that send and read are MCP tools (mail_send and mail_read).\n";

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
        // The hint shows the slash arguments: `/zygo:down` takes only `--now` (W§12, OP11); `/zygo:mail` lists only (M§8).
        let hint = match v.name { "down" => DOWN_SLASH_ARGS, "mail" => MAIL_SLASH_ARGS, _ => v.args };
        if !hint.is_empty() { t.push_str(&format!("argument-hint: {}\n", quoted(hint))); }
        t.push_str("disable-model-invocation: true\n---\n");
        if v.name == "down" {
            t.push_str(DOWN_BODY);
        } else if v.name == "mail" {
            t.push_str(MAIL_BODY);
        } else {
            t.push_str(&format!(
                "zygo handles /zygo:{0} before the model sees it. If this text reaches you, run: zygo {0} $ARGUMENTS\n",
                v.name));
        }
        out.push((format!("commands/{}.md", v.name), t.into_bytes()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
