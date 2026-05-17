use std::env;
use std::path::PathBuf;

pub(crate) fn global_config_dir() -> Option<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(path).join("nimbus"));
    }
    resolve_home_dir().map(|home| home.join(".config").join("nimbus"))
}

fn resolve_home_dir() -> Option<PathBuf> {
    if let Some(home) = env::var_os("HOME") {
        return Some(PathBuf::from(home));
    }
    if cfg!(windows) {
        if let Some(profile) = env::var_os("USERPROFILE") {
            return Some(PathBuf::from(profile));
        }
        if let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH"))
            && !drive.is_empty()
            && !path.is_empty()
        {
            return Some(PathBuf::from(drive).join(path));
        }
    }
    None
}

pub(crate) fn update_check_cache_path() -> Option<PathBuf> {
    global_config_dir().map(|dir| dir.join("update-check.json"))
}
