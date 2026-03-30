use clap::Parser;
use claude_dispatch::{Cli, Commands, handle_mark_done, run_pipeline, validate_ticket_key};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load config
    let mut config = match claude_dispatch::config::Config::load(&cli.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config from {}: {}", cli.config.display(), e);
            std::process::exit(1);
        }
    };
    config.debug = cli.debug;

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

    // Tmux self-hosting: if running the pipeline and not already inside tmux,
    // create a tmux session and re-exec ourselves inside it.
    if cli.command.is_none() && std::env::var("TMUX").is_err() {
        let session_name = &config.tmux.session_name;

        let session_exists = std::process::Command::new("tmux")
            .args(["has-session", "-t", session_name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !session_exists {
            let exe = std::env::current_exe().unwrap_or_else(|e| {
                eprintln!("Failed to get current executable path: {e}");
                std::process::exit(1);
            });
            let original_args: Vec<String> = std::env::args().skip(1).collect();

            let mut cmd = std::process::Command::new("tmux");
            cmd.args(["new-session", "-d", "-s", session_name, "-n", "daemon"]);
            cmd.arg(&exe);
            cmd.args(&original_args);

            let status = cmd.status().unwrap_or_else(|e| {
                eprintln!("Failed to create tmux session: {e}");
                std::process::exit(1);
            });
            if !status.success() {
                eprintln!("tmux new-session failed (exit: {:?})", status.code());
                std::process::exit(1);
            }

            // Enable mouse support (scrolling, pane selection)
            let _ = std::process::Command::new("tmux")
                .args(["set-option", "-t", session_name, "mouse", "on"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }

        // Attach to the session (blocks until user detaches or session ends)
        let status = std::process::Command::new("tmux")
            .args(["attach", "-t", session_name])
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Failed to attach to tmux session: {e}");
                std::process::exit(1);
            });
        std::process::exit(status.code().unwrap_or(1));
    }

    match cli.command {
        Some(Commands::MarkDone { ticket_key }) => {
            if !validate_ticket_key(&ticket_key) {
                eprintln!("Invalid ticket key format: {ticket_key}");
                std::process::exit(1);
            }
            handle_mark_done(&config, &ticket_key);
        }
        None => {
            run_pipeline(config).await;
        }
    }
}
