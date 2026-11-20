use serde::Deserialize;
use crate::schema::RedactionPattern;

#[derive(Deserialize)]
struct File { pattern: Vec<RedactionPattern> }

/// The built-in secret patterns (§16.1), in their fixed order.
pub fn builtin_patterns() -> Vec<RedactionPattern> {
    let f: File = toml::from_str(include_str!("../data/redaction_patterns.toml"))
        .expect("built-in redaction patterns are valid TOML");
    f.pattern
}
