use serde::Deserialize;
use std::path::PathBuf;

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

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
    #[serde(skip)]
    pub debug: bool,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
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

/// Returns true if `s` only contains characters safe to embed in a
/// double-quoted shell string without risk of command substitution or quote
/// escape. The allowed set matches valid git ref name characters plus `.`.
pub(crate) fn is_safe_git_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'_' || b == b'-' || b == b'.')
}
