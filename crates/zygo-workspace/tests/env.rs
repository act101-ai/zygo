use std::ffi::OsString;
use zygo_workspace::env::{cleaned, REMOVE_ALWAYS, REMOVE_INSIDE_AGENT};

/// Kept in every case: the user setting (AU39), provider keys and the team token (AU64), other variables.
const KEEP: &[&str] =
    &["ZYGO_CONFIG", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY", "ZYGO_TEAM_TOKEN", "ZYGO_TEST_KEY", "PATH", "HOME"];

fn rows(names: &[&str]) -> Vec<(OsString, OsString)> {
    names.iter().map(|n| (OsString::from(n), OsString::from(format!("value of {n}")))).collect()
}

#[test] // AU39 and AU64: the lists are the lists of the spec
fn name_lists_are_the_au64_lists() {
    assert_eq!(REMOVE_ALWAYS, &[
        "ZYGO_SESSION", "ZYGO_SESSION_DIR", "ZYGO_AGENT", "ZYGO_URL", "ZYGO_REPO", "ZYGO_WORK", "ZYGO_MAIN_ROLE", "ZYGO_PANE", "ZYGO_WORKSPACE",
        "ZYGO_RELAUNCHED", "CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_BRIDGE_SESSION_ID",
    ][..]);
    assert_eq!(REMOVE_INSIDE_AGENT, &[
        "ANTHROPIC_BASE_URL", "ANTHROPIC_CUSTOM_HEADERS", "ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_FABLE_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL", "CLAUDE_CODE_GATEWAY_HINT_HEADERS",
        "DISABLE_LOGIN_COMMAND", "DISABLE_LOGOUT_COMMAND", "OPENCODE_CONFIG_CONTENT", "OPENCODE_CONFIG_DIR",
    ][..]);
}

#[test] // AU64 without ZYGO_SESSION: the caller is not an agent; AU93: ZYGO_WORKSPACE and ZYGO_PANE go
fn outside_an_agent_only_the_always_list_goes() {
    let always: Vec<&str> = REMOVE_ALWAYS.iter().copied().filter(|n| *n != "ZYGO_SESSION").collect();
    let mut vars = rows(&[KEEP, always.as_slice(), REMOVE_INSIDE_AGENT].concat());
    vars.push((OsString::from("CLAUDECODE"), OsString::from("1"))); // a second row of a name goes too
    vars.push((OsString::from("zygo_agent"), OsString::from("x"))); // names are compared byte for byte
    let mut want = rows(&[KEEP, REMOVE_INSIDE_AGENT].concat());
    want.push((OsString::from("zygo_agent"), OsString::from("x")));
    assert_eq!(cleaned(vars), want); // order and values unchanged
}

#[test] // AU64 with ZYGO_SESSION: the values the launcher set for its agent child go too; provider keys stay
fn inside_an_agent_both_lists_go() {
    assert_eq!(cleaned(rows(&[REMOVE_ALWAYS, KEEP, REMOVE_INSIDE_AGENT].concat())), rows(KEEP));
    // set with an empty value is set
    let mut vars = rows(&[KEEP, REMOVE_INSIDE_AGENT].concat());
    vars.insert(3, (OsString::from("ZYGO_SESSION"), OsString::new()));
    assert_eq!(cleaned(vars), rows(KEEP));
    assert_eq!(cleaned(vec![]), vec![]);
}

#[cfg(unix)]
#[test] // a name or value that is not UTF-8 is never changed and never matches a list name
fn non_utf8_rows() {
    use std::os::unix::ffi::OsStringExt;
    let odd = (OsString::from_vec(vec![0xff, b'A']), OsString::from_vec(vec![0xfe, 0xff]));
    let vars = vec![odd.clone(), (OsString::from("ZYGO_URL"), OsString::from_vec(vec![0xfe])), (OsString::from("PATH"), OsString::from_vec(vec![b'/', 0xff]))];
    assert_eq!(cleaned(vars), vec![odd, (OsString::from("PATH"), OsString::from_vec(vec![b'/', 0xff]))]);
}
