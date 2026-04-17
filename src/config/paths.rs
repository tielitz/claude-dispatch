use crate::config::error::ConfigError;
use std::path::PathBuf;

/// Expands a path that begins with `~/` to the user's home directory.
/// Absolute paths and other paths are returned unchanged.
pub fn expand_path(path: &str) -> PathBuf {
    if let (Some(rest), Some(home)) = (path.strip_prefix("~/"), dirs::home_dir()) {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// Returns the path to the user's config file for this platform.
/// Filled in by Task 2.
#[allow(dead_code)]
pub fn user_config_path() -> Result<PathBuf, ConfigError> {
    unimplemented!("filled in by Task 2")
}

/// Returns a config path adjacent to the running binary, if determinable.
/// Filled in by Task 2.
#[allow(dead_code)]
pub fn binary_adjacent_config_path() -> Option<PathBuf> {
    unimplemented!("filled in by Task 2")
}
