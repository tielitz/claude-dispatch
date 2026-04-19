//! Multi-issue configuration validation. Reports every failure in one error.

use crate::config::error::ConfigError;
use crate::config::schema::{Config, is_safe_git_ident};
use std::str::FromStr;

const VALID_LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error", "off"];

pub fn run(cfg: &Config) -> Result<(), ConfigError> {
    let mut issues = Vec::new();

    if !is_safe_git_ident(&cfg.git.branch_prefix) {
        issues.push(format!(
            "git.branch_prefix has disallowed chars (allowed: A-Z a-z 0-9 / _ - .): {:?}",
            cfg.git.branch_prefix
        ));
    }
    if !is_safe_git_ident(&cfg.git.base_branch) {
        issues.push(format!(
            "git.base_branch has disallowed chars: {:?}",
            cfg.git.base_branch
        ));
    }

    if cfg.jira.instance.trim().is_empty() {
        issues.push("jira.instance is empty".into());
    } else {
        // jira.instance is either a short name (e.g. "acme" → https://acme.atlassian.net)
        // or a full URL (e.g. "https://acme.atlassian.net"). A schemeless FQDN
        // like "acme.atlassian.net" silently produces "https://acme.atlassian.net.atlassian.net";
        // catch it here.
        let inst = cfg.jira.instance.trim();
        let has_scheme = inst.starts_with("http://") || inst.starts_with("https://");
        if !has_scheme && inst.contains('.') {
            issues.push(format!(
                "jira.instance looks like a hostname; prefix it with https:// (got {:?})",
                cfg.jira.instance
            ));
        }
    }
    if !cfg.jira.email.contains('@') {
        issues.push(format!(
            "jira.email is not a valid email: {:?}",
            cfg.jira.email
        ));
    }
    if cfg.jira.api_token.trim().is_empty() || cfg.jira.api_token == "your-api-token" {
        issues.push("jira.api_token is empty or still the template placeholder".into());
    }
    if let Err(e) = cron::Schedule::from_str(&cfg.jira.cron_schedule) {
        issues.push(format!("jira.cron_schedule failed to parse: {e}"));
    }
    if cfg.jira.fetch_limit == 0 {
        issues.push("jira.fetch_limit must be > 0".into());
    }

    if cfg.paths.output_dir.trim().is_empty() {
        issues.push("paths.output_dir is empty".into());
    }
    if cfg.paths.repo_root.trim().is_empty() {
        issues.push("paths.repo_root is empty".into());
    }

    if cfg.spawner.poll_interval_secs == 0 {
        issues.push("spawner.poll_interval_secs must be > 0".into());
    }

    if !VALID_LOG_LEVELS
        .iter()
        .any(|l| l.eq_ignore_ascii_case(&cfg.log_level))
    {
        issues.push(format!(
            "log_level is not recognized (expected one of {:?}): {:?}",
            VALID_LOG_LEVELS, cfg.log_level
        ));
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(ConfigError::Validation(issues))
    }
}
