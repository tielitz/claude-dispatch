use clap::Parser;
use claude_dispatch::{Cli, Commands, handle_mark_done, run_pipeline, validate_ticket_key};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load config
    let config = match claude_dispatch::config::Config::load(&cli.config) {
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

    claude_dispatch::setup_tracing(&config);

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
