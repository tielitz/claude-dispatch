use crate::config::{error::ConfigError, schema::Config};
use std::path::Path;

pub fn load(cli: Option<&Path>) -> Result<Config, ConfigError> {
    let path = cli.ok_or(ConfigError::NoUserConfigDir)?;
    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut config: Config = toml::from_str(&content).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;
    config.config_path = Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    validate(&config)?;
    Ok(config)
}

fn validate(cfg: &Config) -> Result<(), ConfigError> {
    use crate::config::schema::is_safe_git_ident;
    if !is_safe_git_ident(&cfg.git.branch_prefix) {
        return Err(ConfigError::Validation(vec![format!(
            "git.branch_prefix contains disallowed characters: {:?} (allowed: A-Z a-z 0-9 / _ - .)",
            cfg.git.branch_prefix
        )]));
    }
    if !is_safe_git_ident(&cfg.git.base_branch) {
        return Err(ConfigError::Validation(vec![format!(
            "git.base_branch contains disallowed characters: {:?} (allowed: A-Z a-z 0-9 / _ - .)",
            cfg.git.base_branch
        )]));
    }
    Ok(())
}
