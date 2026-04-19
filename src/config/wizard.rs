//! First-run wizard. Writes `config.example.toml` to the per-OS user config
//! directory so new users can get started without hunting for a template.

use crate::config::error::ConfigError;
use std::fs;
use std::path::Path;

const TEMPLATE: &str = include_str!("../../config.example.toml");

/// Writes the embedded `config.example.toml` template to `path`, creating any
/// missing parent directories.
///
/// **Overwrites any existing file at `path`.** The first-run loader only calls
/// this when no sources exist, so the path is guaranteed empty in that flow.
/// The `--init` CLI flag uses it to refresh a known-stale template, where
/// overwriting is the intended behavior.
pub fn write_template(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    fs::write(path, TEMPLATE).map_err(|e| ConfigError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}
