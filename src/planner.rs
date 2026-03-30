use crate::config::{Config, expand_path};
use crate::jira::client::JiraTicket;
use tracing::{error, info};

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
    let base_branch = &config.worktree.base_branch;

    let rendered = template
        .replace("{{TICKET_KEY}}", &ticket.key)
        .replace("{{TICKET_SUMMARY}}", &ticket.summary)
        .replace("{{TICKET_CONTENT}}", ticket_content)
        .replace("{{BRANCH}}", &branch)
        .replace("{{BASE_BRANCH}}", base_branch);

    Ok(rendered)
}

/// Runs a headless Claude Code session to draft an implementation plan for the
/// given Jira ticket.
pub async fn draft_plan(
    config: &Config,
    ticket: &JiraTicket,
    ticket_markdown_content: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let prompt = render_prompt(config, ticket, ticket_markdown_content)?;

    let repo_root = config.repo_root();
    let claude_home = config.claude_home();

    info!(
        ticket_key = %ticket.key,
        repo_root = %repo_root.display(),
        "Drafting implementation plan with headless Claude"
    );

    let output = tokio::process::Command::new("claude")
        .arg("--print")
        .arg("-p")
        .arg(&prompt)
        .current_dir(&repo_root)
        .env("CLAUDE_HOME", &claude_home)
        .output()
        .await
        .map_err(|e| format!("Failed to spawn claude process: {}", e))?;

    if output.status.success() {
        let plan = String::from_utf8_lossy(&output.stdout).into_owned();
        info!(ticket_key = %ticket.key, "Plan drafted successfully");
        Ok(plan)
    } else {
        let exit_code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        error!(
            ticket_key = %ticket.key,
            exit_code = exit_code,
            stderr = %stderr,
            "Claude process failed"
        );
        Err(format!("claude exited with code {}: {}", exit_code, stderr).into())
    }
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
}
