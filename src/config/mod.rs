mod error;
mod loader;
mod migrate;
mod paths;
mod schema;
mod validate;
mod wizard;

pub use error::ConfigError;
pub use paths::{binary_adjacent_config_path, expand_path, user_config_path};
pub use schema::{
    ClaudeConfig, Config, GitConfig, JiraConfig, PathsConfig, SpawnerConfig, TmuxConfig,
    WorktreeConfig,
};

use std::path::PathBuf;

impl Config {
    /// Load a `Config`. For Task 1 the caller must supply an explicit path;
    /// future tasks will layer user/binary-adjacent defaults in.
    pub fn load(cli: Option<&std::path::Path>) -> Result<Config, ConfigError> {
        loader::load(cli)
    }

    /// Expanded `paths.output_dir`.
    pub fn output_dir(&self) -> PathBuf {
        expand_path(&self.paths.output_dir)
    }

    /// Expanded `paths.repo_root`.
    pub fn repo_root(&self) -> PathBuf {
        expand_path(&self.paths.repo_root)
    }

    /// Expanded `paths.state_dir`.
    pub fn state_dir(&self) -> PathBuf {
        expand_path(&self.paths.state_dir)
    }

    /// Expanded `paths.log_dir`.
    pub fn log_dir(&self) -> PathBuf {
        expand_path(&self.paths.log_dir)
    }

    /// Expanded `claude.home_dir`.
    pub fn claude_home(&self) -> PathBuf {
        expand_path(&self.claude.home_dir)
    }

    /// Path to the SQLite database file inside the state directory.
    pub fn db_path(&self) -> PathBuf {
        self.state_dir().join("pipeline.db")
    }

    /// Base URL for the Jira instance (e.g. `https://mycompany.atlassian.net`).
    pub fn jira_base_url(&self) -> String {
        if self.jira.instance.starts_with("https://") || self.jira.instance.starts_with("http://") {
            self.jira.instance.trim_end_matches('/').to_string()
        } else {
            format!("https://{}.atlassian.net", self.jira.instance)
        }
    }

    /// Git branch name for a given Jira ticket key.
    pub fn branch_for_ticket(&self, key: &str) -> String {
        format!("{}/{}", self.git.branch_prefix, key.to_lowercase())
    }
}
