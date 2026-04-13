use crate::config::{Config, expand_path};
use crate::jira::client::JiraTicket;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tracing::{error, info, warn};

const DEFAULT_PLAN_PROMPT: &str = r#"You are drafting an implementation plan for a Jira ticket. Read the repository's CLAUDE.md and explore the codebase to understand existing patterns and architecture.

## Ticket

Key: {{TICKET_KEY}}
Summary: {{TICKET_SUMMARY}}
Branch: {{BRANCH}}
Base branch: {{BASE_BRANCH}}

## Ticket Content

{{TICKET_CONTENT}}

## Instructions

Draft a detailed implementation plan for this ticket. Your plan should:

1. Identify which files need to be created or modified
2. Outline the changes needed in each file
3. Specify the order of implementation
4. Note any edge cases or risks
5. Suggest what tests should be written
6. Specify the git workflow:
   - List the exact service directories (relative to repo root, e.g. `services/Puzzle`) that will be modified
   - State the branch to create: `{{BRANCH}}` (from `{{BASE_BRANCH}}`)
   - Provide a suggested commit message for each service (prefix with the ticket key, e.g. `{{TICKET_KEY}}: description`)

Output ONLY the implementation plan in markdown format. Do not implement anything.
"#;

/// Loads the prompt template: returns the DEFAULT_PLAN_PROMPT if the config
/// path is empty, otherwise reads the file at the configured path.
fn load_template(config: &Config) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if config.claude.plan_prompt_template.is_empty() {
        return Ok(DEFAULT_PLAN_PROMPT.to_string());
    }

    let path = expand_path(&config.claude.plan_prompt_template);
    let content = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read prompt template at {}: {}",
            path.display(),
            e
        )
    })?;
    Ok(content)
}

/// Renders the prompt by replacing all known placeholders with ticket data.
fn render_prompt(
    config: &Config,
    ticket: &JiraTicket,
    ticket_content: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let template = load_template(config)?;

    let branch = config.branch_for_ticket(&ticket.key);
    let base_branch = &config.git.base_branch;

    let rendered = template
        .replace("{{TICKET_KEY}}", &ticket.key)
        .replace("{{TICKET_SUMMARY}}", &ticket.summary)
        .replace("{{TICKET_CONTENT}}", ticket_content)
        .replace("{{BRANCH}}", &branch)
        .replace("{{BASE_BRANCH}}", base_branch);

    Ok(rendered)
}

/// Per-ticket log file path for planner output.
fn plan_log_path(config: &Config, ticket_key: &str) -> PathBuf {
    config.log_dir().join(format!("plan-{ticket_key}.log"))
}

/// Returns the last `max_lines` lines of `s`, joined with newlines.
fn tail_lines(s: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// Runs a headless Claude Code session to draft an implementation plan for the
/// given Jira ticket.
///
/// Captures Claude's stdout (the plan itself) and stderr to a per-ticket log
/// file at `<log_dir>/plan-<TICKET_KEY>.log` so failures can be diagnosed
/// after the fact. On non-zero exit, the tail of both streams is also
/// emitted via `tracing::error!` so the daemon log shows what went wrong.
pub async fn draft_plan(
    config: &Config,
    ticket: &JiraTicket,
    ticket_markdown_content: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let prompt = render_prompt(config, ticket, ticket_markdown_content)?;

    let repo_root = config.repo_root();
    let claude_home = config.claude_home();
    let log_dir = config.log_dir();
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        warn!(
            ticket_key = %ticket.key,
            error = %e,
            log_dir = %log_dir.display(),
            "failed to create log directory; planner output will not be persisted"
        );
    }
    let log_path = plan_log_path(config, &ticket.key);

    info!(
        ticket_key = %ticket.key,
        repo_root = %repo_root.display(),
        claude_home = %claude_home.display(),
        log_path = %log_path.display(),
        prompt_bytes = prompt.len(),
        "Drafting implementation plan with headless Claude"
    );

    let mut log_file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
    {
        Ok(f) => Some(f),
        Err(e) => {
            warn!(
                ticket_key = %ticket.key,
                error = %e,
                log_path = %log_path.display(),
                "failed to open planner log file; continuing without on-disk capture"
            );
            None
        }
    };

    if let Some(f) = log_file.as_mut() {
        let header = format!(
            "\n===== {} draft_plan {} =====\nrepo_root: {}\nclaude_home: {}\nprompt_bytes: {}\n\n",
            chrono::Utc::now().to_rfc3339(),
            ticket.key,
            repo_root.display(),
            claude_home.display(),
            prompt.len(),
        );
        let _ = f.write_all(header.as_bytes()).await;
        let _ = f.flush().await;
    }

    let mut child = tokio::process::Command::new("claude")
        .arg("--print")
        .arg("-p")
        .arg(&prompt)
        .current_dir(&repo_root)
        .env("CLAUDE_CONFIG_DIR", &claude_home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn claude process: {e}"))?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    // Tee both streams concurrently:
    //   - stdout always goes to the per-ticket log file (and to terminal in debug)
    //   - stderr always goes to the per-ticket log file (and to terminal in debug)
    let debug = config.debug;
    let log_path_for_stdout = log_path.clone();
    let log_path_for_stderr = log_path.clone();
    let ticket_key_stdout = ticket.key.clone();
    let ticket_key_stderr = ticket.key.clone();

    let stdout_task = tokio::spawn(async move {
        tee_stream(
            BufReader::new(stdout),
            &log_path_for_stdout,
            "stdout",
            &ticket_key_stdout,
            debug,
        )
        .await
    });
    let stderr_task = tokio::spawn(async move {
        tee_stream(
            BufReader::new(stderr),
            &log_path_for_stderr,
            "stderr",
            &ticket_key_stderr,
            debug,
        )
        .await
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait on claude process: {e}"))?;

    let captured_stdout = stdout_task
        .await
        .map_err(|e| format!("stdout reader task panicked: {e}"))??;
    let captured_stderr = stderr_task
        .await
        .map_err(|e| format!("stderr reader task panicked: {e}"))??;

    if let Some(f) = log_file.as_mut() {
        let footer = format!(
            "\n--- exit_code: {} ---\n",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string()),
        );
        let _ = f.write_all(footer.as_bytes()).await;
        let _ = f.flush().await;
    }

    if status.success() {
        info!(
            ticket_key = %ticket.key,
            log_path = %log_path.display(),
            stdout_bytes = captured_stdout.len(),
            stderr_bytes = captured_stderr.len(),
            "Plan drafted successfully"
        );
        Ok(captured_stdout)
    } else {
        let exit_code = status.code().unwrap_or(-1);
        let stdout_tail = tail_lines(&captured_stdout, 40);
        let stderr_tail = tail_lines(&captured_stderr, 40);
        error!(
            ticket_key = %ticket.key,
            exit_code = exit_code,
            log_path = %log_path.display(),
            stdout_bytes = captured_stdout.len(),
            stderr_bytes = captured_stderr.len(),
            stdout_tail = %stdout_tail,
            stderr_tail = %stderr_tail,
            "Claude planner process failed; see log file for full output"
        );
        Err(format!(
            "claude exited with code {exit_code} (see {} for full output): {}",
            log_path.display(),
            if stderr_tail.is_empty() {
                stdout_tail
            } else {
                stderr_tail
            },
        )
        .into())
    }
}

/// Read every line from `reader`, append it to `log_path` prefixed with
/// `[<stream>] ` and (in debug mode) also mirror it to the daemon's stderr.
/// Returns the full collected text from this stream.
async fn tee_stream<R: AsyncRead + Unpin>(
    reader: BufReader<R>,
    log_path: &std::path::Path,
    stream_name: &str,
    ticket_key: &str,
    debug: bool,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await
        .ok();

    let mut lines = reader.lines();
    let mut collected = String::new();
    let mut stderr_handle = if debug {
        Some(tokio::io::stderr())
    } else {
        None
    };

    while let Some(line) = lines.next_line().await? {
        collected.push_str(&line);
        collected.push('\n');

        if let Some(f) = file.as_mut() {
            let _ = f
                .write_all(format!("[{stream_name}] {line}\n").as_bytes())
                .await;
            let _ = f.flush().await;
        }
        if let Some(s) = stderr_handle.as_mut() {
            let _ = s
                .write_all(format!("[planner {ticket_key} {stream_name}] {line}\n").as_bytes())
                .await;
            let _ = s.flush().await;
        }
    }

    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::client::JiraTicket;

    fn minimal_config() -> Config {
        let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "token"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[git]

[worktree]

[tmux]

[spawner]
"#;
        toml::from_str(toml).expect("parse minimal config")
    }

    fn minimal_ticket() -> JiraTicket {
        JiraTicket {
            key: "PROJ-42".to_string(),
            summary: "Implement feature X".to_string(),
            issue_type: "Story".to_string(),
            priority: "High".to_string(),
            status: "In Progress".to_string(),
            labels: vec![],
            components: vec![],
            assignee: "Jane Doe".to_string(),
            parent_key: None,
            description_adf: None,
            subtasks: vec![],
        }
    }

    #[test]
    fn test_render_prompt_default_template() {
        let config = minimal_config();
        let ticket = minimal_ticket();
        let content = "## Description\nDo the thing.";

        let rendered =
            render_prompt(&config, &ticket, content).expect("render_prompt should succeed");

        assert!(
            !rendered.contains("{{TICKET_KEY}}"),
            "TICKET_KEY placeholder not replaced"
        );
        assert!(
            !rendered.contains("{{TICKET_SUMMARY}}"),
            "TICKET_SUMMARY placeholder not replaced"
        );
        assert!(
            !rendered.contains("{{TICKET_CONTENT}}"),
            "TICKET_CONTENT placeholder not replaced"
        );
        assert!(
            !rendered.contains("{{BRANCH}}"),
            "BRANCH placeholder not replaced"
        );
        assert!(
            !rendered.contains("{{BASE_BRANCH}}"),
            "BASE_BRANCH placeholder not replaced"
        );

        assert!(
            rendered.contains("PROJ-42"),
            "rendered prompt should contain ticket key"
        );
        assert!(
            rendered.contains("Implement feature X"),
            "rendered prompt should contain summary"
        );
        assert!(
            rendered.contains("Do the thing."),
            "rendered prompt should contain ticket content"
        );
        assert!(
            rendered.contains("feature/proj-42"),
            "rendered prompt should contain branch"
        );
        assert!(
            rendered.contains("main"),
            "rendered prompt should contain base branch"
        );
    }

    #[test]
    fn test_load_template_default() {
        let config = minimal_config();

        let template = load_template(&config).expect("load_template should succeed");

        assert!(
            template.contains("{{TICKET_KEY}}"),
            "default template should contain {{TICKET_KEY}} placeholder"
        );
    }

    #[tokio::test]
    async fn test_tee_stream_captures_all_lines_and_writes_file() {
        let input = "line one\nline two\nline three\n";
        let cursor = std::io::Cursor::new(input.as_bytes().to_vec());
        let reader = tokio::io::BufReader::new(cursor);

        let tmp =
            std::env::temp_dir().join(format!("claude-dispatch-tee-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let result = super::tee_stream(reader, &tmp, "stdout", "PROJ-1", false)
            .await
            .unwrap();

        assert_eq!(result, "line one\nline two\nline three\n");

        let written = std::fs::read_to_string(&tmp).unwrap();
        assert!(written.contains("[stdout] line one"));
        assert!(written.contains("[stdout] line two"));
        assert!(written.contains("[stdout] line three"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_tail_lines_returns_last_n() {
        let s = "a\nb\nc\nd\ne\n";
        assert_eq!(super::tail_lines(s, 3), "c\nd\ne");
        assert_eq!(super::tail_lines(s, 100), "a\nb\nc\nd\ne");
        assert_eq!(super::tail_lines("", 3), "");
    }
}
