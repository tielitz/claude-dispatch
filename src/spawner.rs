use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

use tokio::process::Command;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::state::StateDb;

/// Main loop: sleep, then poll for planned tickets, forever.
pub async fn run_spawner_loop(
    config: Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let interval = std::time::Duration::from_secs(config.spawner.poll_interval_secs);
    loop {
        tokio::time::sleep(interval).await;
        if let Err(e) = spawn_planned_tickets(&config).await {
            error!("spawn_planned_tickets error: {}", e);
        }
    }
}

/// Open the DB, find all planned tickets, claim each one and spawn a tmux session.
async fn spawn_planned_tickets(
    config: &Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db = StateDb::open(&config.db_path())?;
    let tickets = db.get_planned_tickets()?;

    if tickets.is_empty() {
        return Ok(());
    }

    let pid = std::process::id() as i64;

    for ticket in tickets {
        let plan_file = match &ticket.plan_file {
            Some(p) => p.clone(),
            None => {
                warn!(key = %ticket.key, "planned ticket has no plan_file, skipping");
                continue;
            }
        };

        // Atomically claim the ticket.
        let claimed = db.claim_for_spawning(&ticket.key, pid)?;
        if !claimed {
            info!(key = %ticket.key, "ticket already claimed by another process, skipping");
            continue;
        }

        info!(key = %ticket.key, plan_file = %plan_file, "spawning tmux session");

        match spawn_tmux_session(config, &ticket.key, &plan_file).await {
            Ok(()) => {
                info!(key = %ticket.key, "tmux session spawned successfully");
            }
            Err(e) => {
                error!(key = %ticket.key, error = %e, "failed to spawn tmux session, reverting to planned");
                // Revert: mark back to planned so it can be retried.
                if let Err(revert_err) = db.mark_planned(&ticket.key, &plan_file) {
                    error!(
                        key = %ticket.key,
                        error = %revert_err,
                        "failed to revert ticket to planned state"
                    );
                }
            }
        }
    }

    Ok(())
}

/// Build and run the wrapper script, then attach it to a tmux window/session.
async fn spawn_tmux_session(
    config: &Config,
    ticket_key: &str,
    plan_file: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // --- Resolve paths ---
    let state_dir = config.state_dir();
    let repo_root = config.repo_root();
    let claude_home = config.claude_home();

    let current_exe = std::env::current_exe()?;

    let config_path = config
        .config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    // --- Build claude invocation ---
    // Always: claude -p "$PROMPT"
    // Optionally: --worktree <branch_prefix>/<ticket_key>
    // Optionally: extra_flags
    let mut claude_args = String::new();

    if config.worktree.enabled {
        let branch = format!("{}/{}", config.worktree.branch_prefix, ticket_key);
        claude_args.push_str(&format!("--worktree {} ", branch));
    }

    claude_args.push_str("-p \"$PROMPT\"");

    if !config.claude.extra_flags.is_empty() {
        claude_args.push(' ');
        claude_args.push_str(&config.claude.extra_flags);
    }

    // --- Write wrapper script ---
    let script_path = state_dir.join(format!("run-{}.sh", ticket_key));

    let script_content = format!(
        r#"#!/bin/bash
set -uo pipefail

TICKET_KEY="{ticket_key}"
REPO_ROOT="{repo_root}"
CLAUDE_HOME_DIR="{claude_home}"
BINARY="{binary}"
PLAN_FILE="{plan_file}"
CONFIG_PATH="{config_path}"

cd "$REPO_ROOT"
export CLAUDE_HOME="$CLAUDE_HOME_DIR"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Dev Pipeline — $TICKET_KEY"
echo "  Plan file: $PLAN_FILE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

PROMPT=$(cat "$PLAN_FILE")

claude {claude_args}
EXIT_CODE=$?

if [ $EXIT_CODE -eq 0 ]; then
    echo ""
    echo "Session completed successfully for $TICKET_KEY"
else
    echo ""
    echo "Session failed for $TICKET_KEY (exit code: $EXIT_CODE)"
fi

"$BINARY" mark-done "$TICKET_KEY" --config "$CONFIG_PATH"

echo ""
echo "Session ended. Press Enter to close this pane."
read -r
"#,
        ticket_key = ticket_key,
        repo_root = repo_root.display(),
        claude_home = claude_home.display(),
        binary = current_exe.display(),
        plan_file = plan_file,
        config_path = config_path,
        claude_args = claude_args,
    );

    // Ensure state dir exists.
    fs::create_dir_all(&state_dir)?;
    fs::write(&script_path, &script_content)?;
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;

    info!(script = %script_path.display(), "wrapper script written");

    // --- Tmux: check if session already exists ---
    let session_name = &config.tmux.session_name;
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?
        .success();

    let script_path_str = script_path.to_string_lossy().into_owned();

    if session_exists {
        // Add a new window to the existing session.
        let status = Command::new("tmux")
            .args([
                "new-window",
                "-t",
                session_name,
                "-n",
                ticket_key,
                &script_path_str,
            ])
            .status()
            .await?;

        if !status.success() {
            return Err(format!(
                "tmux new-window failed for ticket {} (exit: {:?})",
                ticket_key,
                status.code()
            )
            .into());
        }
    } else {
        // Create a brand-new detached session.
        let status = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                session_name,
                "-n",
                ticket_key,
                &script_path_str,
            ])
            .status()
            .await?;

        if !status.success() {
            return Err(format!(
                "tmux new-session failed for ticket {} (exit: {:?})",
                ticket_key,
                status.code()
            )
            .into());
        }
    }

    Ok(())
}
