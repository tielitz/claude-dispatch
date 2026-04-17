//! First-run wizard. Writes `config.example.toml` to the per-OS user config
//! directory so new users can get started without hunting for a template.

use crate::config::error::ConfigError;
use std::fs;
use std::path::Path;

const TEMPLATE: &str = include_str!("../../config.example.toml");

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
