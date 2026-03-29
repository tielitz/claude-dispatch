use base64::Engine as _;
use serde_json::Value;

pub struct JiraClient {
    client: reqwest::Client,
    base_url: String,
    auth_header: String,
}

#[derive(Debug, Clone)]
pub struct JiraTicket {
    pub key: String,
    pub summary: String,
    pub issue_type: String,
    pub priority: String,
    pub status: String,
    pub labels: Vec<String>,
    pub components: Vec<String>,
    pub assignee: String,
    pub parent_key: Option<String>,
    pub description_adf: Option<serde_json::Value>,
    pub subtasks: Vec<SubtaskInfo>,
}

#[derive(Debug, Clone)]
pub struct SubtaskInfo {
    pub key: String,
    pub summary: String,
}

impl JiraClient {
    pub fn new(base_url: &str, email: &str, api_token: &str) -> Self {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", email, api_token));
        let auth_header = format!("Basic {}", credentials);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            base_url: base_url.to_string(),
            auth_header,
        }
    }

    pub async fn search_tickets(
        &self,
        jql: &str,
        max_results: u32,
    ) -> Result<Vec<JiraTicket>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/rest/api/3/search", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/json")
            .query(&[
                ("jql", jql.to_string()),
                ("maxResults", max_results.to_string()),
                (
                    "fields",
                    "summary,description,issuetype,priority,status,labels,components,assignee,parent,subtasks"
                        .to_string(),
                ),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Jira API error {}: {}", status, body).into());
        }

        let data: Value = response.json().await?;
        let issues = data["issues"].as_array().cloned().unwrap_or_default();
        let tickets = issues.iter().map(parse_issue).collect();
        Ok(tickets)
    }
}

fn parse_issue(issue: &Value) -> JiraTicket {
    let key = issue["key"].as_str().unwrap_or("").to_string();
    let fields = &issue["fields"];

    let summary = fields["summary"].as_str().unwrap_or("").to_string();

    let issue_type = fields["issuetype"]["name"]
        .as_str()
        .unwrap_or("Task")
        .to_string();

    let priority = fields["priority"]["name"]
        .as_str()
        .unwrap_or("Medium")
        .to_string();

    let status = fields["status"]["name"]
        .as_str()
        .unwrap_or("To Do")
        .to_string();

    let labels = fields["labels"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let components = fields["components"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let assignee = if fields["assignee"].is_null() {
        "Unassigned".to_string()
    } else {
        fields["assignee"]["displayName"]
            .as_str()
            .unwrap_or("Unassigned")
            .to_string()
    };

    let parent_key = fields["parent"]["key"].as_str().map(|s| s.to_string());

    let description_adf = if fields["description"].is_null() {
        None
    } else {
        Some(fields["description"].clone()).filter(|v| !v.is_null())
    };

    let subtasks = fields["subtasks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|s| SubtaskInfo {
                    key: s["key"].as_str().unwrap_or("").to_string(),
                    summary: s["fields"]["summary"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    JiraTicket {
        key,
        summary,
        issue_type,
        priority,
        status,
        labels,
        components,
        assignee,
        parent_key,
        description_adf,
        subtasks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_issue_full() {
        let issue = json!({
            "key": "PROJ-42",
            "fields": {
                "summary": "Implement feature X",
                "issuetype": { "name": "Story" },
                "priority": { "name": "High" },
                "status": { "name": "In Progress" },
                "labels": ["backend", "api"],
                "components": [
                    { "name": "Core" },
                    { "name": "Auth" }
                ],
                "assignee": { "displayName": "Jane Doe" },
                "parent": { "key": "PROJ-10" },
                "description": {
                    "type": "doc",
                    "version": 1,
                    "content": []
                },
                "subtasks": [
                    {
                        "key": "PROJ-43",
                        "fields": { "summary": "Write unit tests" }
                    },
                    {
                        "key": "PROJ-44",
                        "fields": { "summary": "Update docs" }
                    }
                ]
            }
        });

        let ticket = parse_issue(&issue);

        assert_eq!(ticket.key, "PROJ-42");
        assert_eq!(ticket.summary, "Implement feature X");
        assert_eq!(ticket.issue_type, "Story");
        assert_eq!(ticket.priority, "High");
        assert_eq!(ticket.status, "In Progress");
        assert_eq!(ticket.labels, vec!["backend", "api"]);
        assert_eq!(ticket.components, vec!["Core", "Auth"]);
        assert_eq!(ticket.assignee, "Jane Doe");
        assert_eq!(ticket.parent_key, Some("PROJ-10".to_string()));
        assert!(ticket.description_adf.is_some());
        assert_eq!(ticket.subtasks.len(), 2);
        assert_eq!(ticket.subtasks[0].key, "PROJ-43");
        assert_eq!(ticket.subtasks[0].summary, "Write unit tests");
        assert_eq!(ticket.subtasks[1].key, "PROJ-44");
        assert_eq!(ticket.subtasks[1].summary, "Update docs");
    }

    #[test]
    fn test_parse_issue_minimal() {
        let issue = json!({
            "key": "PROJ-1",
            "fields": {
                "summary": "Minimal issue",
                "issuetype": null,
                "priority": null,
                "status": null,
                "labels": [],
                "components": [],
                "assignee": null,
                "parent": null,
                "description": null,
                "subtasks": []
            }
        });

        let ticket = parse_issue(&issue);

        assert_eq!(ticket.key, "PROJ-1");
        assert_eq!(ticket.summary, "Minimal issue");
        assert_eq!(ticket.issue_type, "Task");
        assert_eq!(ticket.priority, "Medium");
        assert_eq!(ticket.status, "To Do");
        assert!(ticket.labels.is_empty());
        assert!(ticket.components.is_empty());
        assert_eq!(ticket.assignee, "Unassigned");
        assert!(ticket.parent_key.is_none());
        assert!(ticket.description_adf.is_none());
        assert!(ticket.subtasks.is_empty());
    }

    // --- Security: client construction and credential handling ---

    #[test]
    fn test_client_construction_does_not_panic() {
        // Ensures the reqwest::Client::builder() with timeouts builds successfully
        let _client = JiraClient::new("https://test.atlassian.net", "user@test.com", "token123");
    }

    #[test]
    fn test_auth_header_is_base64_basic_auth() {
        let client = JiraClient::new(
            "https://test.atlassian.net",
            "user@test.com",
            "secret-token",
        );

        // Verify it's a Basic auth header with properly encoded credentials
        assert!(
            client.auth_header.starts_with("Basic "),
            "auth_header should start with 'Basic '"
        );

        // Decode and verify the credentials are correctly formatted
        let encoded = client.auth_header.strip_prefix("Basic ").unwrap();
        let decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .expect("valid base64"),
        )
        .expect("valid utf8");
        assert_eq!(decoded, "user@test.com:secret-token");
    }

    #[test]
    fn test_auth_header_does_not_contain_plaintext_token() {
        let client = JiraClient::new(
            "https://test.atlassian.net",
            "user@test.com",
            "secret-token",
        );

        // The raw token should not appear unencoded in the auth header
        assert!(
            !client.auth_header.contains("secret-token"),
            "plaintext token should not appear in auth_header"
        );
    }

    // --- Security: resilient parsing of malicious API responses ---

    #[test]
    fn test_parse_issue_with_xss_in_summary() {
        let issue = json!({
            "key": "PROJ-99",
            "fields": {
                "summary": "<script>alert('xss')</script>",
                "issuetype": { "name": "Bug" },
                "priority": { "name": "High" },
                "status": { "name": "Open" },
                "labels": [],
                "components": [],
                "assignee": null,
                "parent": null,
                "description": null,
                "subtasks": []
            }
        });

        let ticket = parse_issue(&issue);
        // The raw content is preserved — sanitization is the output layer's job,
        // but critically, parsing must not panic or truncate.
        assert_eq!(ticket.summary, "<script>alert('xss')</script>");
    }

    #[test]
    fn test_parse_issue_with_shell_injection_in_key() {
        let issue = json!({
            "key": "$(rm -rf /)",
            "fields": {
                "summary": "evil",
                "issuetype": null,
                "priority": null,
                "status": null,
                "labels": [],
                "components": [],
                "assignee": null,
                "parent": null,
                "description": null,
                "subtasks": []
            }
        });

        let ticket = parse_issue(&issue);
        // parse_issue stores the key as-is; validation happens upstream
        assert_eq!(ticket.key, "$(rm -rf /)");
    }

    #[test]
    fn test_parse_issue_with_deeply_nested_description() {
        // Verify no stack overflow with moderately deep ADF nesting
        let mut inner = json!({"type": "text", "text": "deep"});
        for _ in 0..50 {
            inner = json!({
                "type": "paragraph",
                "content": [inner]
            });
        }
        let issue = json!({
            "key": "PROJ-1",
            "fields": {
                "summary": "deep nesting",
                "issuetype": null,
                "priority": null,
                "status": null,
                "labels": [],
                "components": [],
                "assignee": null,
                "parent": null,
                "description": inner,
                "subtasks": []
            }
        });

        let ticket = parse_issue(&issue);
        assert!(ticket.description_adf.is_some());
    }

    #[test]
    fn test_parse_issue_with_missing_fields_does_not_panic() {
        // A completely empty fields object — every field falls back to defaults
        let issue = json!({
            "key": "PROJ-1",
            "fields": {}
        });

        let ticket = parse_issue(&issue);
        assert_eq!(ticket.key, "PROJ-1");
        assert_eq!(ticket.summary, "");
        assert_eq!(ticket.issue_type, "Task");
        assert_eq!(ticket.assignee, "Unassigned");
    }
}
