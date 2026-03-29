pub mod config;
pub mod jira;
pub mod markdown;
pub mod planner;
pub mod spawner;
pub mod state;

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
pub struct Cli {
    /// Path to config file
    #[arg(long, default_value = "config.toml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Internal: mark a ticket as done (called by tmux wrapper)
    MarkDone {
        /// Jira ticket key (e.g., PROJ-123)
        ticket_key: String,
    },
}

pub fn handle_mark_done(config: &config::Config, ticket_key: &str) {
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

pub async fn run_pipeline(config: config::Config) {
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

pub fn setup_tracing(config: &config::Config) {
    let log_dir = config.log_dir();

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
}
