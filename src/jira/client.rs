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
        Self {
            client: reqwest::Client::new(),
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
}
