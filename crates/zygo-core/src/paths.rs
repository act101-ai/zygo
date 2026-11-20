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
