pub mod cmd;

/// The panic line: a location, never the payload (C3: a payload can hold a secret).
pub fn panic_line(loc: Option<(&str, u32)>) -> String {
    match loc {
        Some((file, line)) => format!("zygo: internal error at {file}:{line}"),
        None => "zygo: internal error".to_string(),
    }
}

/// Replaces the default panic hook, which prints the payload. A failed stderr write is ignored: `eprintln!` would
/// panic inside the hook, and a panic in the hook aborts the process.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|i| {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "{}", panic_line(i.location().map(|l| (l.file(), l.line()))));
    }));
}

/// Test-only panic trigger: panics when `ZYGO_TEST_PANIC` is set. The code exists only under the `test-hooks` feature.
#[cfg(feature = "test-hooks")]
pub fn test_panic() {
    if std::env::var_os("ZYGO_TEST_PANIC").is_some() {
        panic!("failed printing to stdout: Broken pipe (panicked)");
    }
}

#[cfg(test)]
mod tests {
    #[test] // C3: the hook prints a location only; `panic_line` takes no payload, so none can reach stderr
    fn panic_line_has_location_and_no_payload() {
        assert_eq!(super::panic_line(Some(("src/a.rs", 7))), "zygo: internal error at src/a.rs:7");
        assert_eq!(super::panic_line(None), "zygo: internal error");
    }
}
