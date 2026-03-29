use claude_dispatch::jira::client::{JiraTicket, SubtaskInfo};
use claude_dispatch::markdown::generate_ticket_markdown;
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
