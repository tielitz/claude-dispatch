use std::fs;
use std::os::unix::fs::PermissionsExt;

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

    let claude_args_str = build_claude_args(config, ticket_key).join(" ");

    // --- Write wrapper script ---
    // All dynamic values are passed via environment variables set by tmux,
    // avoiding shell metacharacter injection from interpolated strings.
    let script_path = state_dir.join(format!("run-{ticket_key}.sh"));

    // The script reads all dynamic values from environment variables that are
    // set when tmux launches the script (see spawn_tmux_session below).
    let script_content = format!(
        r#"#!/bin/bash
set -uo pipefail

# All dynamic values are passed via environment variables to avoid shell injection.
# See the tmux invocation that sets: CDP_TICKET_KEY, CDP_REPO_ROOT, etc.

cd "$CDP_REPO_ROOT" || exit 1
export CLAUDE_CONFIG_DIR="$CDP_CLAUDE_HOME"

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

"$CDP_BINARY" --config "$CDP_CONFIG_PATH" mark-done "$CDP_TICKET_KEY"

echo ""
echo "Closing in 10 seconds..."
sleep 10
"#,
    );

    // Ensure state dir exists.
    fs::create_dir_all(&state_dir)?;
    fs::write(&script_path, &script_content)?;
    // 0o700: only the owning user can read/write/execute the wrapper script
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))?;

    info!(script = %script_path.display(), "wrapper script written");

    // --- Tmux: split a new pane in the self-hosted session ---
    let session_name = &config.tmux.session_name;
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

    // Split a new pane in the existing session for this agent.
    // Environment variables must be passed via tmux's -e flag, not .env(),
    // because .env() only sets vars on the tmux client process — the tmux
    // server spawns the pane command using the session environment instead.
    let mut cmd = Command::new("tmux");
    cmd.args(["split-window", "-t", session_name]);
    for (k, v) in &env_vars {
        cmd.args(["-e", &format!("{k}={v}")]);
    }
    cmd.arg(&script_path_str);
    let status = cmd.status().await?;

    if !status.success() {
        return Err(format!(
            "tmux split-window failed for ticket {} (exit: {:?})",
            ticket_key,
            status.code()
        )
        .into());
    }

    // Rebalance all panes into a tiled grid layout.
    let rebalance = Command::new("tmux")
        .args(["select-layout", "-t", session_name, "tiled"])
        .status()
        .await?;

    if !rebalance.success() {
        warn!(key = %ticket_key, "tmux select-layout tiled failed, panes may be uneven");
    }

    Ok(())
}

/// Shell-escape a string by wrapping it in single quotes.
/// Any embedded single quotes are replaced with `'\''` (end quote, escaped quote, start quote).
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build the list of shell-escaped claude CLI arguments.
/// Extracted for testability — the same logic is used in spawn_tmux_session.
fn build_claude_args(config: &Config, ticket_key: &str) -> Vec<String> {
    let mut elements: Vec<String> = Vec::new();

    // Start in plan mode so Claude reviews the plan before executing.
    elements.push(shell_escape("--permission-mode"));
    elements.push(shell_escape("plan"));

    if config.worktree.enabled {
        let branch = config.branch_for_ticket(ticket_key);
        elements.push(shell_escape("--worktree"));
        elements.push(shell_escape(&branch));
    }

    elements.push("\"Implement the plan in @$CDP_PLAN_FILE\"".to_string());

    if !config.claude.extra_flags.is_empty() {
        for flag in config.claude.extra_flags.split_whitespace() {
            if !flag.starts_with('-') {
                warn!(flag = %flag, "Skipping extra_flag that doesn't start with '-'");
                continue;
            }
            elements.push(shell_escape(flag));
        }
    }

    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(extra_flags: &str, worktree_enabled: bool) -> Config {
        let toml_str = format!(
            r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "token"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude"
extra_flags = "{extra_flags}"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]
enabled = {worktree_enabled}

[tmux]

[spawner]
"#,
            extra_flags = extra_flags,
            worktree_enabled = worktree_enabled,
        );
        toml::from_str(&toml_str).expect("parse test config")
    }

    // --- shell_escape ---

    #[test]
    fn test_shell_escape_simple_string() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_with_spaces() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_escape_with_semicolon() {
        // Semicolons inside single quotes are literal, not command separators
        assert_eq!(shell_escape("; rm -rf /"), "'; rm -rf /'");
    }

    #[test]
    fn test_shell_escape_with_dollar_sign() {
        // $ inside single quotes is literal
        assert_eq!(shell_escape("$(whoami)"), "'$(whoami)'");
    }

    #[test]
    fn test_shell_escape_with_backticks() {
        assert_eq!(shell_escape("`id`"), "'`id`'");
    }

    #[test]
    fn test_shell_escape_with_double_quotes() {
        assert_eq!(shell_escape("say \"hi\""), "'say \"hi\"'");
    }

    #[test]
    fn test_shell_escape_with_single_quotes() {
        // This is the tricky case: embedded single quotes must be escaped
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_escape_with_newline() {
        assert_eq!(shell_escape("line1\nline2"), "'line1\nline2'");
    }

    #[test]
    fn test_shell_escape_empty_string() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_pipe_and_redirect() {
        assert_eq!(shell_escape("| cat > /etc/passwd"), "'| cat > /etc/passwd'");
    }

    // --- extra_flags validation ---

    #[test]
    fn test_extra_flags_valid_flags_only() {
        let config = test_config("--verbose --debug", true);
        let args = build_claude_args(&config, "PROJ-1");
        assert!(args.contains(&"'--verbose'".to_string()));
        assert!(args.contains(&"'--debug'".to_string()));
    }

    #[test]
    fn test_extra_flags_rejects_non_flag_values() {
        // "payload" doesn't start with '-' and should be silently dropped
        let config = test_config("--verbose payload --debug", true);
        let args = build_claude_args(&config, "PROJ-1");
        assert!(args.contains(&"'--verbose'".to_string()));
        assert!(args.contains(&"'--debug'".to_string()));
        assert!(!args.contains(&"'payload'".to_string()));
    }

    #[test]
    fn test_extra_flags_rejects_shell_injection() {
        let config = test_config("; rm -rf /", true);
        let args = build_claude_args(&config, "PROJ-1");
        // ";" and "rm" and "-rf" and "/" — only "-rf" starts with '-'
        // but even that is shell-escaped to "'-rf'"
        assert!(!args.contains(&"';'".to_string()));
        assert!(!args.contains(&"'rm'".to_string()));
        assert!(!args.contains(&"'/'".to_string()));
        // -rf would pass the starts_with('-') check but is still shell-escaped
        assert!(args.contains(&"'-rf'".to_string()));
    }

    #[test]
    fn test_extra_flags_empty() {
        let config = test_config("", false);
        let args = build_claude_args(&config, "PROJ-1");
        // --permission-mode plan + prompt reference
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], "'--permission-mode'");
        assert_eq!(args[1], "'plan'");
    }

    // --- wrapper script content: no hardcoded dynamic values ---

    #[test]
    fn test_script_uses_env_vars_not_interpolation() {
        let config = test_config("--verbose", true);
        let args = build_claude_args(&config, "PROJ-42");
        let claude_args_str = args.join(" ");

        // Simulate the same format!() used in spawn_tmux_session
        let script_content = format!(
            r#"#!/bin/bash
set -uo pipefail
cd "$CDP_REPO_ROOT" || exit 1
export CLAUDE_CONFIG_DIR="$CDP_CLAUDE_HOME"
claude {claude_args_str}
"$CDP_BINARY" --config "$CDP_CONFIG_PATH" mark-done "$CDP_TICKET_KEY"
"#,
            claude_args_str = claude_args_str,
        );

        // The script must NOT contain any hardcoded paths or ticket keys.
        // All dynamic values come from $CDP_* environment variables.
        assert!(
            !script_content.contains("PROJ-42"),
            "ticket key should not be interpolated into script"
        );
        assert!(
            !script_content.contains("/tmp/repo"),
            "repo_root should not be interpolated into script"
        );
        assert!(
            !script_content.contains("/tmp/tickets"),
            "output_dir should not be interpolated into script"
        );

        // Verify the script references env vars instead
        assert!(script_content.contains("$CDP_REPO_ROOT"));
        assert!(script_content.contains("$CDP_CLAUDE_HOME"));
        assert!(script_content.contains("$CDP_TICKET_KEY"));
        assert!(script_content.contains("$CDP_BINARY"));
        assert!(script_content.contains("$CDP_CONFIG_PATH"));
        assert!(script_content.contains("$CDP_PLAN_FILE"));
    }

    #[test]
    fn test_worktree_branch_is_shell_escaped() {
        let config = test_config("", true);
        let args = build_claude_args(&config, "PROJ-42");
        // branch_prefix defaults to "feature", so branch = "feature/proj-42"
        assert!(args.contains(&"'--worktree'".to_string()));
        assert!(args.contains(&"'feature/proj-42'".to_string()));
    }

    #[test]
    fn test_prompt_references_plan_file_via_at_path() {
        let config = test_config("", false);
        let args = build_claude_args(&config, "PROJ-1");
        let joined = args.join(" ");
        assert!(
            joined.contains("@$CDP_PLAN_FILE"),
            "prompt should reference plan file via @<path>: {joined}"
        );
    }
}
