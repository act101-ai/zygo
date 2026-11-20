use std::path::Path;
use std::process::Command;

/// Points every per-user location of `zygo` at `home` (see `Paths::from_env`; the unix and the Windows cache), so a
/// test never reads or sweeps the real cache.
pub fn scratch_home<'a>(cmd: &'a mut Command, home: &Path) -> &'a mut Command {
    cmd.env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home).env("ZYGO_CONFIG", home.join("config.toml"))
}
