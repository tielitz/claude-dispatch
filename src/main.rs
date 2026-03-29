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
