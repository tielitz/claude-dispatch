use std::io::Write as _;
use std::path::PathBuf;

use claude_dispatch::config::{Config, expand_path};

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
cron_schedule = "0 */2 * * * *"
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
    assert_eq!(cfg.jira.cron_schedule, "0 */2 * * * *");
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
    assert_eq!(cfg.jira.cron_schedule, "0 */5 * * * *");
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

// --- Security: Debug output must not leak secrets ---

#[test]
fn test_jira_config_debug_redacts_api_token() {
    let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "super-secret-token-12345"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let cfg = Config::load(f.path()).expect("parse config");

    let debug_output = format!("{:?}", cfg.jira);

    assert!(
        !debug_output.contains("super-secret-token-12345"),
        "API token must not appear in Debug output, got: {}",
        debug_output
    );
    assert!(
        debug_output.contains("[REDACTED]"),
        "Debug output should show [REDACTED] for api_token, got: {}",
        debug_output
    );
    // Other fields should still be visible
    assert!(
        debug_output.contains("acme"),
        "instance should be visible in Debug"
    );
    assert!(
        debug_output.contains("dev@acme.com"),
        "email should be visible in Debug"
    );
}

#[test]
fn test_full_config_debug_redacts_api_token() {
    let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "my-secret-key-xyz"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let cfg = Config::load(f.path()).expect("parse config");

    // When the full Config is debug-printed, the nested JiraConfig
    // must still redact the token
    let debug_output = format!("{:?}", cfg);

    assert!(
        !debug_output.contains("my-secret-key-xyz"),
        "API token must not leak through Config Debug, got: {}",
        debug_output
    );
}
