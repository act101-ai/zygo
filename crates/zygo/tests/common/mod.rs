#![allow(dead_code)] // each test binary uses a part of this module

use std::env::consts::EXE_SUFFIX;
use std::path::{Path, PathBuf};
use std::process::Command;

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
/// `<target>/<profile>/deps/<name>-<hash>` and cargo puts `stub-agent` in `<target>/<profile>/`. Panics with the build
/// command when the file is missing: `cargo test -p zygo` alone does not build it.
pub fn stub_agent() -> PathBuf {
    let exe = std::env::current_exe().expect("the path of the test binary");
    let dir = exe.parent().and_then(Path::parent).expect("a test binary is in <target>/<profile>/deps");
    stub_agent_in(dir).unwrap_or_else(|e| panic!("{e}"))
}

/// Appends one step line to `dir/steps`, the control file of a stub agent started with `ZYGO_STUB_CONTROL=dir`
/// (the protocol is in the head of `stub.rs`). It makes `dir` and checks nothing else.
pub fn stub_control(dir: &Path, step: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    std::fs::OpenOptions::new().create(true).append(true).open(dir.join("steps"))?.write_all(format!("{step}\n").as_bytes())
}
