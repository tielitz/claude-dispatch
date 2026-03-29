use crate::jira::adf::adf_to_markdown;
use crate::jira::client::JiraTicket;
use chrono::Utc;

/// Generate a complete markdown file for a Jira ticket, including YAML frontmatter
/// and the Claude-drafted implementation plan.
pub fn generate_ticket_markdown(ticket: &JiraTicket, plan: &str) -> String {
    let fetched_at = Utc::now().to_rfc3339();

    // Escape double-quotes in summary for YAML
    let escaped_summary = ticket.summary.replace('"', "\\\"");

    // Labels and components as [item1, item2] or []
    let labels = format_list(&ticket.labels);
    let components = format_list(&ticket.components);

    // Parent key or empty string
    let parent = ticket.parent_key.as_deref().unwrap_or("");

    // Description: convert ADF or fall back to placeholder
    let description = match &ticket.description_adf {
        Some(adf) => {
            let md = adf_to_markdown(adf);
            if md.is_empty() {
                "No description provided.".to_string()
            } else {
                md
            }
        }
        None => "No description provided.".to_string(),
    };

    // Subtasks
    let subtasks_section = if ticket.subtasks.is_empty() {
        "No subtasks.".to_string()
    } else {
        ticket
            .subtasks
            .iter()
            .map(|s| format!("- {}: {}", s.key, s.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "---\nkey: {key}\nsummary: \"{summary}\"\ntype: {issue_type}\npriority: {priority}\nstatus: {status}\nlabels: {labels}\ncomponents: {components}\nassignee: {assignee}\nparent: {parent}\nfetched_at: {fetched_at}\n---\n\n# {key}: {raw_summary}\n\n## Description\n\n{description}\n\n## Subtasks\n\n{subtasks}\n\n## Implementation Plan\n\n{plan}\n",
        key = ticket.key,
        summary = escaped_summary,
        issue_type = ticket.issue_type,
        priority = ticket.priority,
        status = ticket.status,
        labels = labels,
        components = components,
        assignee = ticket.assignee,
        parent = parent,
        fetched_at = fetched_at,
        raw_summary = ticket.summary,
        description = description,
        subtasks = subtasks_section,
        plan = plan,
    )
}

/// Format a slice of strings as `[item1, item2]` or `[]` if empty.
fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", items.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::client::{JiraTicket, SubtaskInfo};
    use serde_json::json;

    fn make_test_ticket() -> JiraTicket {
        JiraTicket {
            key: "PROJ-123".to_string(),
            summary: "Add health check endpoint".to_string(),
            issue_type: "Task".to_string(),
            priority: "High".to_string(),
            status: "In Progress".to_string(),
            labels: vec!["backend".to_string(), "api".to_string()],
            components: vec!["user-service".to_string()],
            assignee: "John Doe".to_string(),
            parent_key: Some("PROJ-100".to_string()),
            description_adf: Some(json!({
                "type": "doc",
                "content": [
                    {
                        "type": "paragraph",
                        "content": [
                            { "type": "text", "text": "Implement a /health endpoint." }
                        ]
                    }
                ]
            })),
            subtasks: vec![
                SubtaskInfo {
                    key: "PROJ-124".to_string(),
                    summary: "Write unit tests".to_string(),
                },
                SubtaskInfo {
                    key: "PROJ-125".to_string(),
                    summary: "Update API docs".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_generate_markdown_includes_frontmatter() {
        let ticket = make_test_ticket();
        let result = generate_ticket_markdown(&ticket, "Do the work.");

        assert!(result.starts_with("---\n"), "Should start with YAML fence");
        assert!(result.contains("key: PROJ-123"), "key field");
        assert!(
            result.contains("summary: \"Add health check endpoint\""),
            "summary field"
        );
        assert!(result.contains("type: Task"), "type field");
        assert!(result.contains("priority: High"), "priority field");
        assert!(result.contains("status: In Progress"), "status field");
        assert!(result.contains("labels: [backend, api]"), "labels field");
        assert!(
            result.contains("components: [user-service]"),
            "components field"
        );
        assert!(result.contains("assignee: John Doe"), "assignee field");
        assert!(result.contains("parent: PROJ-100"), "parent field");
        assert!(result.contains("fetched_at:"), "fetched_at field");
    }

    #[test]
    fn test_generate_markdown_includes_plan() {
        let ticket = make_test_ticket();
        let plan = "Step 1: do something.\nStep 2: do more.";
        let result = generate_ticket_markdown(&ticket, plan);

        assert!(
            result.contains("## Implementation Plan"),
            "Implementation Plan section header"
        );
        assert!(result.contains(plan), "Plan content should appear verbatim");
    }

    #[test]
    fn test_generate_markdown_includes_subtasks() {
        let ticket = make_test_ticket();
        let result = generate_ticket_markdown(&ticket, "plan");

        assert!(result.contains("## Subtasks"), "Subtasks section header");
        assert!(
            result.contains("- PROJ-124: Write unit tests"),
            "First subtask"
        );
        assert!(
            result.contains("- PROJ-125: Update API docs"),
            "Second subtask"
        );
    }

    #[test]
    fn test_generate_markdown_no_description() {
        let mut ticket = make_test_ticket();
        ticket.description_adf = None;
        let result = generate_ticket_markdown(&ticket, "plan");

        assert!(
            result.contains("No description provided."),
            "Should show fallback when no ADF"
        );
    }

    #[test]
    fn test_generate_markdown_no_subtasks() {
        let mut ticket = make_test_ticket();
        ticket.subtasks = vec![];
        let result = generate_ticket_markdown(&ticket, "plan");

        assert!(
            result.contains("No subtasks."),
            "Should show fallback when subtasks empty"
        );
    }
}
