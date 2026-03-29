use serde::Deserialize;
use std::path::{Path, PathBuf};

fn default_poll_interval() -> u64 {
    60
}

fn default_fetch_limit() -> u32 {
    5
}

fn default_state_dir() -> String {
    "~/.dev-pipeline".to_string()
}

fn default_log_dir() -> String {
    "~/.dev-pipeline/logs".to_string()
}

fn default_true() -> bool {
    true
}

fn default_branch_prefix() -> String {
    "feature".to_string()
}

fn default_base_branch() -> String {
    "main".to_string()
}

fn default_session_name() -> String {
    "dev-pipeline".to_string()
}

fn default_spawner_poll() -> u64 {
    10
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
    pub jira: JiraConfig,
    pub claude: ClaudeConfig,
    pub paths: PathsConfig,
    pub worktree: WorktreeConfig,
    pub tmux: TmuxConfig,
    pub spawner: SpawnerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JiraConfig {
    pub instance: String,
    pub email: String,
    pub api_token: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_fetch_limit")]
    pub fetch_limit: u32,
    pub jql: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    pub home_dir: String,
    #[serde(default)]
    pub extra_flags: String,
    #[serde(default)]
    pub plan_prompt_template: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PathsConfig {
    pub output_dir: String,
    pub repo_root: String,
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorktreeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TmuxConfig {
    #[serde(default = "default_session_name")]
    pub session_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SpawnerConfig {
    #[serde(default = "default_spawner_poll")]
    pub poll_interval_secs: u64,
}

/// Expands a path that begins with `~/` to the user's home directory.
/// Absolute paths and other paths are returned unchanged.
pub fn expand_path(path: &str) -> PathBuf {
    if let (Some(rest), Some(home)) = (path.strip_prefix("~/"), dirs::home_dir()) {
        return home.join(rest);
    }
    PathBuf::from(path)
}

impl Config {
    /// Load a `Config` from a TOML file at `path`.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        config.config_path = Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        Ok(config)
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
        format!("https://{}.atlassian.net", self.jira.instance)
    }

    /// Git branch name for a given Jira ticket key.
    pub fn branch_for_ticket(&self, key: &str) -> String {
        format!("{}/{}", self.worktree.branch_prefix, key.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_temp_toml(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("create temp file");
        f.write_all(content.as_bytes()).expect("write toml");
        f
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "secret-token"
poll_interval_secs = 120
fetch_limit = 10
jql = 'assignee = currentUser()'

[claude]
home_dir = "/opt/claude"
extra_flags = "--verbose"
plan_prompt_template = "Do the thing"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/home/user/repo"
state_dir = "/tmp/state"
log_dir = "/tmp/logs"

[worktree]
enabled = false
branch_prefix = "task"
base_branch = "develop"

[tmux]
session_name = "my-pipeline"

[spawner]
poll_interval_secs = 30
"#;
        let f = write_temp_toml(toml);
        let cfg = Config::load(f.path()).expect("parse full config");

        assert_eq!(cfg.jira.instance, "acme");
        assert_eq!(cfg.jira.email, "dev@acme.com");
        assert_eq!(cfg.jira.api_token, "secret-token");
        assert_eq!(cfg.jira.poll_interval_secs, 120);
        assert_eq!(cfg.jira.fetch_limit, 10);
        assert_eq!(cfg.jira.jql, "assignee = currentUser()");

        assert_eq!(cfg.claude.home_dir, "/opt/claude");
        assert_eq!(cfg.claude.extra_flags, "--verbose");
        assert_eq!(cfg.claude.plan_prompt_template, "Do the thing");

        assert_eq!(cfg.paths.output_dir, "/tmp/tickets");
        assert_eq!(cfg.paths.repo_root, "/home/user/repo");
        assert_eq!(cfg.paths.state_dir, "/tmp/state");
        assert_eq!(cfg.paths.log_dir, "/tmp/logs");

        assert!(!cfg.worktree.enabled);
        assert_eq!(cfg.worktree.branch_prefix, "task");
        assert_eq!(cfg.worktree.base_branch, "develop");

        assert_eq!(cfg.tmux.session_name, "my-pipeline");
        assert_eq!(cfg.spawner.poll_interval_secs, 30);

        assert!(cfg.config_path.is_some());
    }

    #[test]
    fn test_parse_minimal_config_with_defaults() {
        let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "token"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "~/.dev-pipeline/tickets"
repo_root = "~/projects/repo"

[worktree]

[tmux]

[spawner]
"#;
        let f = write_temp_toml(toml);
        let cfg = Config::load(f.path()).expect("parse minimal config");

        // Defaults
        assert_eq!(cfg.jira.poll_interval_secs, 60);
        assert_eq!(cfg.jira.fetch_limit, 5);

        assert_eq!(cfg.paths.state_dir, "~/.dev-pipeline");
        assert_eq!(cfg.paths.log_dir, "~/.dev-pipeline/logs");

        assert!(cfg.worktree.enabled);
        assert_eq!(cfg.worktree.branch_prefix, "feature");
        assert_eq!(cfg.worktree.base_branch, "main");

        assert_eq!(cfg.tmux.session_name, "dev-pipeline");
        assert_eq!(cfg.spawner.poll_interval_secs, 10);
    }

    #[test]
    fn test_expand_path_with_tilde() {
        let home = dirs::home_dir().expect("home dir must exist in test env");
        let expanded = expand_path("~/foo/bar");
        assert_eq!(expanded, home.join("foo/bar"));
    }

    #[test]
    fn test_expand_path_absolute() {
        let path = "/absolute/path/to/file";
        let expanded = expand_path(path);
        assert_eq!(expanded, PathBuf::from(path));
    }
}
