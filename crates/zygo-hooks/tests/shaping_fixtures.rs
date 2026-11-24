//! Shaping fixture checks (O§8.1, O§16 item 16): byte-exact files, format, size and secrets.
mod common { pub mod shaping; }
use common::shaping;
use std::process::Command;

#[test]
fn gitattributes_keeps_shaping_bytes() {
    let text = std::fs::read_to_string(shaping::repo().join(".gitattributes")).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let auto = lines.iter().position(|l| *l == "* text=auto eol=lf").expect("* text=auto eol=lf");
    let raw = lines.iter().position(|l| *l == "fixtures/shaping/** -text").expect("fixtures/shaping/** -text");
    assert!(auto < raw, "the -text line must come after * text=auto eol=lf");
    let out = Command::new("git").args(["check-attr", "text", "--", "fixtures/shaping/clean/c/stdout"]).current_dir(shaping::repo()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "fixtures/shaping/clean/c/stdout: text: unset\n");
}
