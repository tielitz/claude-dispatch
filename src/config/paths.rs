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
///
/// Platform-specific locations:
/// - Linux:   `~/.config/claude-dispatch/config.toml`
/// - macOS:   `~/Library/Application Support/dev.claude-dispatch.claude-dispatch/config.toml`
/// - Windows: `C:\Users\<user>\AppData\Roaming\claude-dispatch\claude-dispatch\config\config.toml`
pub fn user_config_path() -> Result<PathBuf, ConfigError> {
    directories::ProjectDirs::from("dev", "claude-dispatch", "claude-dispatch")
        .map(|pd| pd.config_dir().join("config.toml"))
        .ok_or(ConfigError::NoUserConfigDir)
}

/// Returns a config path adjacent to the running binary, if determinable.
pub fn binary_adjacent_config_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.join("config.toml"))
}
