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
