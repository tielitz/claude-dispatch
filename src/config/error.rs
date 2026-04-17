use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("env var {var} could not be parsed as {ty}: {value}")]
    EnvCoerce {
        var: String,
        ty: &'static str,
        value: String,
    },
    #[error("unknown schema_version {found}; this binary supports {supported:?}")]
    UnknownSchemaVersion {
        found: u32,
        supported: &'static [u32],
    },
    #[error("configuration is invalid:\n  - {}", .0.join("\n  - "))]
    Validation(Vec<String>),
    #[error("no config found; wrote template to {0}")]
    WizardBootstrap(PathBuf),
    #[error("could not determine user config directory for this platform")]
    NoUserConfigDir,
}
