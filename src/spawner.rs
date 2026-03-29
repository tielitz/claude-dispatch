use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

use tokio::process::Command;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::state::StateDb;
use crate::validate_ticket_key;

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
        if !validate_ticket_key(&ticket.key) {
            error!(key = %ticket.key, "ticket key failed validation, skipping");
            continue;
        }

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

    // --- Build claude arguments as a shell array ---
    // Using a bash array avoids injection via extra_flags or worktree branch names.
    let mut claude_array_elements: Vec<String> = Vec::new();

    if config.worktree.enabled {
        let branch = config.branch_for_ticket(ticket_key);
        claude_array_elements.push(shell_escape("--worktree"));
        claude_array_elements.push(shell_escape(&branch));
    }

    // The prompt references the plan file via @<path> so Claude reads it directly.
    // This avoids loading file content into a shell variable (ARG_MAX, metacharacters).
    claude_array_elements.push(shell_escape("-p"));
    claude_array_elements.push("\"Implement the plan in @$CDP_PLAN_FILE\"".to_string());

    // Parse extra_flags into individual arguments and validate each one
    if !config.claude.extra_flags.is_empty() {
        for flag in config.claude.extra_flags.split_whitespace() {
            if !flag.starts_with('-') {
                warn!(flag = %flag, "Skipping extra_flag that doesn't start with '-'");
                continue;
            }
            claude_array_elements.push(shell_escape(flag));
        }
    }

    let claude_args_str = claude_array_elements.join(" ");

    // --- Write wrapper script ---
    // All dynamic values are passed via environment variables set by tmux,
    // avoiding shell metacharacter injection from interpolated strings.
    let script_path = state_dir.join(format!("run-{}.sh", ticket_key));

    // The script reads all dynamic values from environment variables that are
    // set when tmux launches the script (see spawn_tmux_session below).
    let script_content = format!(
        r#"#!/bin/bash
set -uo pipefail

# All dynamic values are passed via environment variables to avoid shell injection.
# See the tmux invocation that sets: CDP_TICKET_KEY, CDP_REPO_ROOT, etc.

cd "$CDP_REPO_ROOT" || exit 1
export CLAUDE_HOME="$CDP_CLAUDE_HOME"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Dev Pipeline — $CDP_TICKET_KEY"
echo "  Plan file: $CDP_PLAN_FILE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Plan file is referenced via @<path> in the prompt — Claude reads it directly.
# This avoids loading file content into a shell variable (ARG_MAX, metacharacters).
claude {claude_args_str}
EXIT_CODE=$?

if [ $EXIT_CODE -eq 0 ]; then
    echo ""
    echo "Session completed successfully for $CDP_TICKET_KEY"
else
    echo ""
    echo "Session failed for $CDP_TICKET_KEY (exit code: $EXIT_CODE)"
fi

"$CDP_BINARY" mark-done "$CDP_TICKET_KEY" --config "$CDP_CONFIG_PATH"

echo ""
echo "Session ended. Press Enter to close this pane."
read -r
"#,
        claude_args_str = claude_args_str,
    );

    // Ensure state dir exists.
    fs::create_dir_all(&state_dir)?;
    fs::write(&script_path, &script_content)?;
    // 0o700: only the owning user can read/write/execute the wrapper script
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))?;

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

    // Build environment variables to pass dynamic values safely to the script.
    let env_vars = [
        ("CDP_TICKET_KEY", ticket_key.to_string()),
        ("CDP_REPO_ROOT", repo_root.display().to_string()),
        ("CDP_CLAUDE_HOME", claude_home.display().to_string()),
        ("CDP_BINARY", current_exe.display().to_string()),
        ("CDP_PLAN_FILE", plan_file.to_string()),
        ("CDP_CONFIG_PATH", config_path.clone()),
    ];

    if session_exists {
        // Add a new window to the existing session.
        let mut cmd = Command::new("tmux");
        cmd.args([
            "new-window",
            "-t",
            session_name,
            "-n",
            ticket_key,
            &script_path_str,
        ]);
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        let status = cmd.status().await?;

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
        let mut cmd = Command::new("tmux");
        cmd.args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-n",
            ticket_key,
            &script_path_str,
        ]);
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        let status = cmd.status().await?;

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

/// Shell-escape a string by wrapping it in single quotes.
/// Any embedded single quotes are replaced with `'\''` (end quote, escaped quote, start quote).
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
