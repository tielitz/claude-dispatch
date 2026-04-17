use serde::Deserialize;
use std::path::{Path, PathBuf};

fn default_cron_schedule() -> String {
    "0 */5 * * * *".to_string()
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

fn default_log_level() -> String {
    "info".to_string()
}

fn default_claude_home() -> String {
    "~/.claude".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
    #[serde(skip)]
    pub debug: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub jira: JiraConfig,
    pub claude: ClaudeConfig,
    pub paths: PathsConfig,
    #[serde(default)]
    pub git: GitConfig,
    pub worktree: WorktreeConfig,
    pub tmux: TmuxConfig,
    pub spawner: SpawnerConfig,
}

#[derive(Deserialize, Clone)]
pub struct JiraConfig {
    pub instance: String,
    pub email: String,
    pub api_token: String,
    #[serde(default = "default_fetch_limit")]
    pub fetch_limit: u32,
    pub jql: String,
    /// Cron expression for the Jira sync schedule.
    /// Uses 6-field cron syntax: `sec min hour day-of-month month day-of-week`.
    /// Defaults to `"0 */5 * * * *"` (every 5 minutes).
    #[serde(default = "default_cron_schedule")]
    pub cron_schedule: String,
}

impl std::fmt::Debug for JiraConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JiraConfig")
            .field("instance", &self.instance)
            .field("email", &self.email)
            .field("api_token", &"[REDACTED]")
            .field("fetch_limit", &self.fetch_limit)
            .field("jql", &self.jql)
            .field("cron_schedule", &self.cron_schedule)
            .finish()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default = "default_claude_home")]
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
pub struct GitConfig {
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            branch_prefix: default_branch_prefix(),
            base_branch: default_base_branch(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorktreeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
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

/// Returns true if `s` only contains characters safe to embed in a
/// double-quoted shell string without risk of command substitution or quote
/// escape. The allowed set matches valid git ref name characters plus `.`.
fn is_safe_git_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'_' || b == b'-' || b == b'.')
}

impl Config {
    /// Load a `Config` from a TOML file at `path`.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        config.config_path = Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        config.validate()?;
        Ok(config)
    }

    /// Validate config values that get interpolated into shell commands or
    /// prompts. `git.branch_prefix` and `git.base_branch` must be restricted
    /// to a safe git-ref charset so they cannot inject shell metacharacters
    /// when embedded in the double-quoted implementation prompt.
    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !is_safe_git_ident(&self.git.branch_prefix) {
            return Err(format!(
                "config.git.branch_prefix contains disallowed characters: {:?} (allowed: A-Z a-z 0-9 / _ - .)",
                self.git.branch_prefix
            )
            .into());
        }
        if !is_safe_git_ident(&self.git.base_branch) {
            return Err(format!(
                "config.git.base_branch contains disallowed characters: {:?} (allowed: A-Z a-z 0-9 / _ - .)",
                self.git.base_branch
            )
            .into());
        }
        Ok(())
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
