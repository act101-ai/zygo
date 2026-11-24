//! N34: every flag, setting key, env row and agent key zygo writes is in the pinned evidence of the agent version (§8.1 A-21, §20 #2, #40).
//! The evidence is the files under `fixtures/` (N2); no agent is installed. No process-environment change.

#[path = "stub_agent/harness.rs"]
mod harness;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use harness::*;
use serde_json::Value;

fn fx(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel)).unwrap()
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '-'))).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Every key of every object stored under a `"properties"` key, recursively.
fn prop_names(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "properties" {
                    out.extend(x.as_object().into_iter().flat_map(|p| p.keys().cloned()));
                }
                prop_names(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| prop_names(x, out)),
        _ => {}
    }
}

/// Some help line, after its short flag (`-n, `) is removed, starts with `flag` and then a space, `<` or the end of the line.
fn help_has_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|l| {
        let l = l.trim_start();
        let l = match l.split_once(", ") {
            Some((short, rest)) if short.len() == 2 && short.starts_with('-') => rest,
            _ => l,
        };
        l.strip_prefix(flag).is_some_and(|r| r.is_empty() || r.starts_with([' ', '<']))
    })
}

#[test]
fn claude_code_flags_are_in_the_pinned_help() {
    let rig = rig();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |o| o.yolo = true);
    let flags: BTreeSet<&str> = r.cap.argv.iter().map(String::as_str).filter(|a| a.starts_with("--")).collect();
    assert_eq!(flags, ["--agents", "--dangerously-skip-permissions", "--mcp-config", "--plugin-dir", "--settings"].into_iter().collect());
    let help = fx("claude-code/help.txt");
    for f in &flags {
        assert!(help_has_flag(&help, f), "{f} is not in the pinned --help");
    }
}

#[test]
fn claude_code_setting_keys_are_in_the_pinned_schema() {
    let rig = harness::rig();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |_| {});
    let schema: Value = serde_json::from_str(&fx("claude-code/settings-schema.json")).unwrap();
    let top = schema["properties"].as_object().unwrap();
    let excerpt = fx("claude-code/settings-binary-excerpt.txt");
    for k in r.cap.settings.as_object().unwrap().keys() {
        if k == "modelPicker" {
            // absent from the schema; the pinned binary's validation text names the shape (spec A-21 note)
            assert!(excerpt.contains("modelPicker") && excerpt.contains(r#""options" array of { model, label?, description?, behavesAs? }"#));
        } else {
            assert!(top.contains_key(k), "settings key {k} is not in the pinned schema");
        }
    }
    let mut names = BTreeSet::new();
    prop_names(&schema, &mut names);
    for k in ["type", "command", "refreshInterval"] {
        assert!(r.cap.settings["statusLine"].get(k).is_some() && names.contains(k), "{k}");
    }
    for k in r.cap.settings["statusLine"].as_object().unwrap().keys() {
        assert!(names.contains(k.as_str()), "statusLine.{k}");
    }
    let events = schema["properties"]["hooks"]["properties"].as_object().unwrap();
    for (ev, list) in r.cap.settings["hooks"].as_object().unwrap() {
        assert!(events.contains_key(ev), "hook event {ev}");
        for entry in list.as_array().unwrap() {
            for k in entry.as_object().unwrap().keys() {
                assert!(names.contains(k.as_str()), "{ev}.{k}");
            }
            for h in entry["hooks"].as_array().unwrap() {
                for k in h.as_object().unwrap().keys() {
                    assert!(names.contains(k.as_str()), "{ev}.hooks.{k}");
                }
            }
        }
    }
    for k in ["model", "label", "description", "behavesAs"] {
        assert!(excerpt.contains(k), "{k}");
    }
    // FIX-21: the attribution keys zygo writes in every mode, with the types it writes
    for k in ["commit", "pr"] {
        assert_eq!(schema["properties"]["attribution"]["properties"][k]["type"], "string", "{k}");
    }
    assert_eq!(schema["properties"]["attribution"]["properties"]["sessionUrl"]["type"], "boolean");
    assert_eq!(schema["properties"]["includeCoAuthoredBy"]["type"], "boolean");
    // keys zygo only reads (§8.4 preflight): taken from the preflight's own refusal text, schema only (spec A-21 note)
    for key in ["allowManagedHooksOnly", "disableAllHooks"] {
        let rig = harness::rig();
        std::fs::write(rig.root.join("managed-settings.json"), format!("{{\"{key}\":true}}")).unwrap();
        let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |_| {});
        assert_eq!(r.code, 2, "{}", r.stderr);
        assert!(r.stderr.contains(&format!(": sets {key}; zygo cannot launch with it")), "{}", r.stderr);
        assert_eq!(schema["properties"][key]["type"], "boolean");
    }
}

#[test]
fn claude_code_env_rows_and_agent_keys_have_evidence() {
    let rig = rig();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |_| {});
    // `words` keeps a leading `--`, so a CLI flag `--disallowedTools` in help.txt is the word `--disallowedTools`.
    let corpus: BTreeSet<String> =
        ["claude-code/settings-schema.json", "claude-code/help.txt", "claude-code/q7/q7-claude.json", "claude-code/live/a20-tools.json"]
            .iter().flat_map(|f| words(&fx(f))).collect();
    for k in r.cap.settings["env"].as_object().unwrap().keys() {
        assert!(corpus.contains(k.as_str()), "env row {k}");
    }
    let i = r.cap.argv.iter().position(|a| a == "--agents").unwrap();
    let agents: Value = serde_json::from_str(&r.cap.argv[i + 1]).unwrap();
    let mut keys: BTreeSet<&str> = agents.as_object().unwrap().values().flat_map(|a| a.as_object().unwrap().keys().map(String::as_str)).collect();
    keys.insert("maxTurns"); // written only under a turn cap (§13.5); `Policy::turn_cap` in `claude_code.rs` `agents`
    let mut flag_only = BTreeSet::new();
    for k in keys {
        if corpus.contains(k) {
            continue;
        }
        assert!(corpus.contains(&format!("--{k}")), "agent key {k} has no evidence");
        flag_only.insert(k);
    }
    // evidence: `disallowedTools` in the `--agents` entry, observed live (N38, claude-code/live/a20-tools.json).
    assert!(flag_only.is_empty(), "agent keys with only CLI-flag evidence: {flag_only:?}");
}

/// `<manifest name>:<skill name>` for every plugin under `--plugin-dir`, in the layout of the observed probe plugin.
fn ids_of(files: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (path, text) in files {
        let Some((dir, rest)) = path.split_once('/') else { continue };
        let Some(skill) = rest.strip_prefix("skills/").and_then(|s| s.strip_suffix("/SKILL.md")) else { continue };
        let manifest: Value = serde_json::from_str(&files[&format!("{dir}/.claude-plugin/plugin.json")]).unwrap();
        let name = text.lines().find_map(|l| l.strip_prefix("name: ")).unwrap();
        assert_eq!(name, skill, "the SKILL.md name is its directory name, as in the probe plugin");
        assert_eq!(manifest["name"], dir);
        out.insert(format!("{}:{name}", manifest["name"].as_str().unwrap()));
    }
    out
}

#[test]
fn claude_code_plugin_snapshot_loads_plugin_colon_skill() {
    let rig = rig();
    let r = launch(&rig, &claude(&rig), CLAUDE_VERSION, |_| {});
    // The rule the pinned agent applied to the probe plugin (a1-init.json): `<manifest name>:<skill name>`; layout `.claude-plugin/plugin.json` + `skills/<dir>/SKILL.md`.
    let init: Value = serde_json::from_str(&fx("claude-code/q7/a1-init.json")).unwrap();
    let observed: Vec<&str> = init["skills"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(observed.contains(&"zygo-probe:probe-skill"));
    assert!(init["plugins"].as_array().unwrap().iter().any(|p| p["name"] == "zygo-probe"));
    let ids: BTreeSet<String> = ids_of(&r.cap.plugins);
    assert_eq!(ids, ["zygo:zygo-cli".to_string()].into());
}

#[test]
fn opencode_inline_config_keys_flags_and_env_are_in_the_pinned_schema() {
    let rig = rig();
    let r = launch(&rig, &opencode(&rig), OPENCODE_VERSION, |o| o.yolo = true);
    let schema: Value = serde_json::from_str::<Value>(&fx("opencode/config-schema.json")).unwrap()["schema"].clone();
    let cfg: Value = serde_json::from_str(&r.cap.env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    let top = schema["properties"].as_object().unwrap();
    for k in cfg.as_object().unwrap().keys() {
        assert!(top.contains_key(k), "inline config key {k}");
    }
    let mut names = BTreeSet::new();
    prop_names(&schema, &mut names);
    let z = &cfg["provider"]["zygo"];
    let nested: Vec<(&str, Vec<&String>)> = vec![
        ("provider.zygo", z.as_object().unwrap().keys().collect()),
        ("options", z["options"].as_object().unwrap().keys().collect()),
        ("model entry", z["models"].as_object().unwrap().values().flat_map(|m| m.as_object().unwrap().keys()).collect()),
        ("limit", z["models"].as_object().unwrap().values().flat_map(|m| m["limit"].as_object().unwrap().keys()).collect()),
        ("agent entry", cfg["agent"].as_object().unwrap().values().flat_map(|a| a.as_object().unwrap().keys()).collect()),
        (
            "permission",
            cfg["agent"].as_object().unwrap().values().filter_map(|a| a.get("permission")).flat_map(|p| p.as_object().unwrap().keys()).collect(),
        ),
        ("mcp entry", cfg["mcp"].as_object().unwrap().values().flat_map(|m| m.as_object().unwrap().keys()).collect()),
    ];
    for (what, keys) in nested {
        assert!(!keys.is_empty(), "{what}");
        for k in keys {
            assert!(names.contains(k.as_str()), "{what}: {k}");
        }
    }
    let help = fx("opencode/help.txt");
    for f in ["--auto", "--model", "--agent"] {
        assert!(help_has_flag(&help, f), "{f}");
    }
    let flags: Vec<&str> = r.cap.argv.iter().map(String::as_str).filter(|a| a.starts_with('-')).collect();
    assert_eq!(flags, ["--auto"]);
    let binary = fx("opencode/binary-names.txt");
    for n in ["OPENCODE_CONFIG_CONTENT", "OPENCODE_CONFIG_DIR"] {
        assert!(r.cap.env.contains_key(n) && binary.contains(n), "{n}");
    }
}
