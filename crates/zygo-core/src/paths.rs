use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub marketplaces_dir: PathBuf,
    pub plugin_store: PathBuf,
    pub approvals_file: PathBuf,
    pub usage_db: PathBuf,
}

fn var(name: &str) -> Option<PathBuf> {
    env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The OS home directory; the temp dir only if the OS gives none. Never a relative path.
fn os_home() -> PathBuf {
    env::home_dir().filter(|h| h.is_absolute()).unwrap_or_else(env::temp_dir)
}

impl Paths {
    pub fn from_env() -> Paths {
        #[cfg(unix)]
        let (config_file, cache_dir, data_dir) = {
            let home = var("HOME").unwrap_or_else(os_home);
            (
                var("ZYGO_CONFIG").unwrap_or_else(|| home.join(".config/zygo/config.toml")),
                home.join(".cache/zygo"),
                home.join(".local/share/zygo"),
            )
        };
        #[cfg(windows)]
        let (config_file, cache_dir, data_dir) = {
            let appdata = var("APPDATA").unwrap_or_else(|| os_home().join("AppData").join("Roaming"));
            let local = var("LOCALAPPDATA").unwrap_or_else(|| os_home().join("AppData").join("Local"));
            (
                var("ZYGO_CONFIG").unwrap_or_else(|| appdata.join("zygo").join("config.toml")),
                local.join("zygo").join("cache"),
                local.join("zygo"),
            )
        };
        Paths {
            sessions_dir: cache_dir.join("sessions"),
            marketplaces_dir: cache_dir.join("marketplaces"),
            plugin_store: data_dir.join("plugins"),
            approvals_file: data_dir.join("approvals.toml"),
            usage_db: data_dir.join("usage.db"),
            config_file,
            cache_dir,
            data_dir,
        }
    }
}

/// `p` without the Windows verbatim prefix when the plain form names the same file: `\\?\C:\x` → `C:\x`,
/// `\\?\UNC\host\share\x` → `\\host\share\x`. Other paths, and every path on other systems, are returned as is.
/// `canonicalize` gives verbatim paths on Windows; git, `cmd.exe` and other programs refuse or misread them.
pub fn plain(p: &std::path::Path) -> PathBuf {
    let Some(s) = p.to_str() else { return p.to_path_buf() };
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        if !rest.split('\\').any(|c| c == "." || c == "..") { return PathBuf::from(format!(r"\\{rest}")); }
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        let disk = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
        // A component that a plain path would read differently (`.`, `..`, a trailing dot or space) keeps the prefix.
        let odd = rest[3.min(rest.len())..].split('\\').filter(|c| !c.is_empty())
            .any(|c| c == "." || c == ".." || c.ends_with('.') || c.ends_with(' '));
        if disk && !odd && !rest.contains('/') { return PathBuf::from(rest); }
    }
    p.to_path_buf()
}

/// `std::fs::canonicalize` followed by `plain`.
pub fn canonical(p: &std::path::Path) -> std::io::Result<PathBuf> { std::fs::canonicalize(p).map(|c| plain(&c)) }
