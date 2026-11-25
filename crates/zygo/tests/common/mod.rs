#![allow(dead_code)] // each test binary uses a part of this module

use std::env::consts::EXE_SUFFIX;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Points every per-user location of `zygo` at `home` (see `Paths::from_env`; the unix and the Windows cache), so a
/// test never reads or sweeps the real cache.
pub fn scratch_home<'a>(cmd: &'a mut Command, home: &Path) -> &'a mut Command {
    cmd.env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home).env("ZYGO_CONFIG", home.join("config.toml"))
}

/// The stub agent in `dir`, or the message that names the command which builds it.
pub fn stub_agent_in(dir: &Path) -> Result<PathBuf, String> {
    let p = dir.join(format!("stub-agent{EXE_SUFFIX}"));
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("stub-agent is not built at {}; run `cargo build -p zygo-launch --bin stub-agent` (or `cargo test --workspace`)", p.display()))
    }
}

/// The built stub agent of `zygo-launch` (`crates/zygo-launch/tests/stub_agent/stub.rs`). A test binary is
/// `<target>/<profile>/deps/<name>-<hash>` and cargo puts `stub-agent` in `<target>/<profile>/`. `cargo test -p zygo`
/// alone does not build it, so when the file is missing this builds it once per test process: `cargo build -p
/// zygo-launch --bin stub-agent --locked` with the `cargo` that runs the tests (`CARGO`) in this workspace, and the
/// inherited environment, so the build uses the same target dir (`CARGO_TARGET_DIR`, else the workspace `target`).
/// Panics with the cargo output when the build fails or the file is still missing; it never skips.
pub fn stub_agent() -> PathBuf {
    static BUILT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    let built = BUILT.get_or_init(|| {
        let exe = std::env::current_exe().map_err(|e| format!("the path of the test binary: {e}"))?;
        let dir = exe.parent().and_then(Path::parent).ok_or("a test binary is in <target>/<profile>/deps")?;
        if let Ok(p) = stub_agent_in(dir) {
            return Ok(p);
        }
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let args = ["build", "-p", "zygo-launch", "--bin", "stub-agent", "--locked"];
        let o = Command::new(&cargo)
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot run {} to build stub-agent: {e}", Path::new(&cargo).display()))?;
        if !o.status.success() {
            return Err(format!("`cargo {}` failed ({}):\n{}", args.join(" "), o.status, String::from_utf8_lossy(&o.stderr)));
        }
        stub_agent_in(dir)
    });
    built.clone().unwrap_or_else(|e| panic!("{e}"))
}

/// Appends one step line to `dir/steps`, the control file of a stub agent started with `ZYGO_STUB_CONTROL=dir`
/// (the protocol is in the head of `stub.rs`). It makes `dir` and checks nothing else.
pub fn stub_control(dir: &Path, step: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    std::fs::OpenOptions::new().create(true).append(true).open(dir.join("steps"))?.write_all(format!("{step}\n").as_bytes())
}

/// The workspace test harness (M2, W§18 preamble).
#[cfg(unix)]
pub mod workspace;

/// Writes `bytes` to a child's stdin and closes it. A child may exit before it reads stdin (statusline: H§6.8, AU24, PH11), so a broken pipe is not an error.
pub fn feed_stdin(mut stdin: std::process::ChildStdin, bytes: &[u8]) {
    use std::io::Write;
    if let Err(e) = stdin.write_all(bytes) {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "write to child stdin: {e}");
    }
}
