# Claude Jira Workflow — Rust Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a single Rust binary that polls Jira for in-progress tickets, drafts implementation plans via headless Claude, and spawns interactive Claude Code sessions in tmux windows for review.

**Architecture:** Single async tokio binary running two concurrent loops — a Jira sync loop (poll → plan → write markdown) and a session spawner loop (poll SQLite → claim → tmux). State tracked in SQLite with atomic claim semantics. A hidden `mark-done` subcommand handles tmux session completion callbacks.

**Tech Stack:** Rust, tokio, reqwest, rusqlite (bundled), serde, toml, clap, tracing, chrono, base64, dirs

---

## File Structure

```
claude-dispatch-workflow/
├── Cargo.toml                  # Dependencies and project metadata
├── justfile                    # Build/run/test commands
├── config.example.toml         # Example configuration
└── src/
    ├── main.rs                 # CLI parsing (clap), tokio runtime, spawns both loops
    ├── config.rs               # TOML config deserialization, path expansion
    ├── state.rs                # SQLite schema, CRUD operations, atomic claiming
    ├── jira/
    │   ├── mod.rs              # Jira sync loop orchestration
    │   ├── client.rs           # REST API v3 HTTP client (reqwest, basic auth)
    │   └── adf.rs              # Atlassian Document Format → markdown converter
    ├── markdown.rs             # Ticket markdown file generation (frontmatter + body)
    ├── planner.rs              # Headless Claude session (tokio::process::Command)
    └── spawner.rs              # SQLite polling loop, tmux window management, wrapper scripts
```

---

### Task 1: Initialize Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `justfile`
- Create: `config.example.toml`
- Create: `src/main.rs` (minimal placeholder)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "claude-dispatch"
version = "0.1.0"
edition = "2024"

[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
clap = { version = "4", features = ["derive"] }
base64 = "0.22"
chrono = { version = "0.4", features = ["serde"] }
dirs = "6"
```

- [ ] **Step 2: Create justfile**

```just
default:
    just --list

build:
    cargo build

release:
    cargo build --release

run:
    cargo run

run-mark-done ticket:
    cargo run -- mark-done {{ticket}}

test:
    cargo test

check:
    cargo clippy -- -D warnings && cargo fmt --check

fmt:
    cargo fmt

clean:
    cargo clean
```

- [ ] **Step 3: Create config.example.toml**

```toml
[jira]
instance = "mycompany"
email = "you@company.com"
api_token = "your-api-token"
poll_interval_secs = 60
fetch_limit = 5
jql = 'assignee = currentUser() AND status = "In Progress"'

[claude]
home_dir = "~/.claude-dispatch"
extra_flags = ""
plan_prompt_template = ""

[paths]
output_dir = "~/.dev-pipeline/tickets"
repo_root = "~/projects/my-service"
state_dir = "~/.dev-pipeline"
log_dir = "~/.dev-pipeline/logs"

[worktree]
enabled = true
branch_prefix = "feature"
base_branch = "main"

[tmux]
session_name = "dev-pipeline"

[spawner]
poll_interval_secs = 10
```

- [ ] **Step 4: Create minimal src/main.rs placeholder**

```rust
fn main() {
    println!("claude-dispatch placeholder");
}
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build`
Expected: compiles successfully, downloads dependencies

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock justfile config.example.toml src/main.rs
git commit -m "feat: initialize Rust project with dependencies and justfile"
```

---

### Task 2: Configuration Module

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write config parsing tests**

Add to `src/config.rs`:

```rust
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub jira: JiraConfig,
    pub claude: ClaudeConfig,
    pub paths: PathsConfig,
    pub worktree: WorktreeConfig,
    pub tmux: TmuxConfig,
    pub spawner: SpawnerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JiraConfig {
    pub instance: String,
    pub email: String,
    pub api_token: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_fetch_limit")]
    pub fetch_limit: u32,
    pub jql: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    pub home_dir: String,
    #[serde(default)]
    pub extra_flags: String,
    #[serde(default)]
    pub plan_prompt_template: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PathsConfig {
    pub output_dir: String,
    pub repo_root: String,
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorktreeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TmuxConfig {
    #[serde(default = "default_session_name")]
    pub session_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SpawnerConfig {
    #[serde(default = "default_spawner_poll")]
    pub poll_interval_secs: u64,
}

fn default_poll_interval() -> u64 { 60 }
fn default_fetch_limit() -> u32 { 5 }
fn default_state_dir() -> String { "~/.dev-pipeline".to_string() }
fn default_log_dir() -> String { "~/.dev-pipeline/logs".to_string() }
fn default_true() -> bool { true }
fn default_branch_prefix() -> String { "feature".to_string() }
fn default_base_branch() -> String { "main".to_string() }
fn default_session_name() -> String { "dev-pipeline".to_string() }
fn default_spawner_poll() -> u64 { 10 }

/// Expand ~ to the user's home directory
pub fn expand_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn output_dir(&self) -> PathBuf { expand_path(&self.paths.output_dir) }
    pub fn repo_root(&self) -> PathBuf { expand_path(&self.paths.repo_root) }
    pub fn state_dir(&self) -> PathBuf { expand_path(&self.paths.state_dir) }
    pub fn log_dir(&self) -> PathBuf { expand_path(&self.paths.log_dir) }
    pub fn claude_home(&self) -> PathBuf { expand_path(&self.claude.home_dir) }
    pub fn db_path(&self) -> PathBuf { expand_path(&self.paths.state_dir).join("state.db") }

    pub fn jira_base_url(&self) -> String {
        format!("https://{}.atlassian.net", self.jira.instance)
    }

    pub fn branch_for_ticket(&self, key: &str) -> String {
        format!("{}/{}", self.worktree.branch_prefix, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let toml_str = r#"
[jira]
instance = "testco"
email = "test@test.com"
api_token = "tok123"
poll_interval_secs = 30
fetch_limit = 10
jql = 'assignee = currentUser() AND status = "In Progress"'

[claude]
home_dir = "~/.claude-test"
extra_flags = "--model sonnet"
plan_prompt_template = ""

[paths]
output_dir = "~/.dev-pipeline/tickets"
repo_root = "~/projects/test"
state_dir = "~/.dev-pipeline"
log_dir = "~/.dev-pipeline/logs"

[worktree]
enabled = false
branch_prefix = "fix"
base_branch = "develop"

[tmux]
session_name = "test-pipeline"

[spawner]
poll_interval_secs = 5
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.jira.instance, "testco");
        assert_eq!(config.jira.poll_interval_secs, 30);
        assert_eq!(config.jira.fetch_limit, 10);
        assert!(!config.worktree.enabled);
        assert_eq!(config.worktree.branch_prefix, "fix");
        assert_eq!(config.tmux.session_name, "test-pipeline");
        assert_eq!(config.jira_base_url(), "https://testco.atlassian.net");
        assert_eq!(config.branch_for_ticket("PROJ-123"), "fix/PROJ-123");
    }

    #[test]
    fn test_parse_minimal_config_with_defaults() {
        let toml_str = r#"
[jira]
instance = "testco"
email = "test@test.com"
api_token = "tok123"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude-test"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]

[tmux]

[spawner]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.jira.poll_interval_secs, 60);
        assert_eq!(config.jira.fetch_limit, 5);
        assert!(config.worktree.enabled);
        assert_eq!(config.worktree.branch_prefix, "feature");
        assert_eq!(config.tmux.session_name, "dev-pipeline");
        assert_eq!(config.spawner.poll_interval_secs, 10);
    }

    #[test]
    fn test_expand_path_with_tilde() {
        let expanded = expand_path("~/test/path");
        assert!(expanded.to_str().unwrap().contains("test/path"));
        assert!(!expanded.to_str().unwrap().starts_with("~"));
    }

    #[test]
    fn test_expand_path_absolute() {
        let expanded = expand_path("/absolute/path");
        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test config`
Expected: all 4 tests pass

- [ ] **Step 3: Wire config into main.rs**

Replace `src/main.rs` with:

```rust
mod config;

fn main() {
    println!("config module ready");
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add TOML configuration module with path expansion"
```

---

### Task 3: SQLite State Module

**Files:**
- Create: `src/state.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write state module with tests**

Create `src/state.rs`:

```rust
use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::Path;

pub struct StateDb {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct TicketRecord {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub plan_file: Option<String>,
    pub claimed_by: Option<i64>,
    pub synced_at: String,
    pub planned_at: Option<String>,
    pub spawned_at: Option<String>,
    pub completed_at: Option<String>,
}

impl StateDb {
    pub fn open(path: &Path) -> SqliteResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let db = StateDb { conn };
        db.init()?;
        Ok(db)
    }

    pub fn open_in_memory() -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        let db = StateDb { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> SqliteResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS processed_tickets (
                key TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'synced',
                plan_file TEXT,
                claimed_by INTEGER,
                synced_at TEXT NOT NULL,
                planned_at TEXT,
                spawned_at TEXT,
                completed_at TEXT
            );"
        )?;
        Ok(())
    }

    pub fn is_known(&self, key: &str) -> SqliteResult<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM processed_tickets WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn insert_synced(&self, key: &str, summary: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO processed_tickets (key, summary, status, synced_at)
             VALUES (?1, ?2, 'synced', ?3)",
            params![key, summary, now],
        )?;
        Ok(())
    }

    pub fn mark_planning(&self, key: &str) -> SqliteResult<()> {
        self.conn.execute(
            "UPDATE processed_tickets SET status = 'planning' WHERE key = ?1",
            params![key],
        )?;
        Ok(())
    }

    pub fn mark_planned(&self, key: &str, plan_file: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE processed_tickets SET status = 'planned', plan_file = ?1, planned_at = ?2
             WHERE key = ?3",
            params![plan_file, now, key],
        )?;
        Ok(())
    }

    /// Atomically claim a planned ticket for spawning. Returns true if claimed.
    pub fn claim_for_spawning(&self, key: &str, pid: i64) -> SqliteResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows = self.conn.execute(
            "UPDATE processed_tickets
             SET status = 'spawned', claimed_by = ?1, spawned_at = ?2
             WHERE key = ?3 AND status = 'planned'",
            params![pid, now, key],
        )?;
        Ok(rows > 0)
    }

    pub fn mark_done(&self, key: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE processed_tickets
             SET status = 'done', completed_at = ?1, claimed_by = NULL
             WHERE key = ?2",
            params![now, key],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, key: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE processed_tickets
             SET status = 'failed', completed_at = ?1, claimed_by = NULL
             WHERE key = ?2",
            params![now, key],
        )?;
        Ok(())
    }

    pub fn get_planned_tickets(&self) -> SqliteResult<Vec<TicketRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, summary, status, plan_file, claimed_by,
                    synced_at, planned_at, spawned_at, completed_at
             FROM processed_tickets WHERE status = 'planned'"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TicketRecord {
                key: row.get(0)?,
                summary: row.get(1)?,
                status: row.get(2)?,
                plan_file: row.get(3)?,
                claimed_by: row.get(4)?,
                synced_at: row.get(5)?,
                planned_at: row.get(6)?,
                spawned_at: row.get(7)?,
                completed_at: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_ticket(&self, key: &str) -> SqliteResult<Option<TicketRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, summary, status, plan_file, claimed_by,
                    synced_at, planned_at, spawned_at, completed_at
             FROM processed_tickets WHERE key = ?1"
        )?;
        let mut rows = stmt.query_map(params![key], |row| {
            Ok(TicketRecord {
                key: row.get(0)?,
                summary: row.get(1)?,
                status: row.get(2)?,
                plan_file: row.get(3)?,
                claimed_by: row.get(4)?,
                synced_at: row.get(5)?,
                planned_at: row.get(6)?,
                spawned_at: row.get(7)?,
                completed_at: row.get(8)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_query() {
        let db = StateDb::open_in_memory().unwrap();
        assert!(!db.is_known("PROJ-1").unwrap());

        db.insert_synced("PROJ-1", "Test ticket").unwrap();
        assert!(db.is_known("PROJ-1").unwrap());

        let ticket = db.get_ticket("PROJ-1").unwrap().unwrap();
        assert_eq!(ticket.status, "synced");
        assert_eq!(ticket.summary, "Test ticket");
    }

    #[test]
    fn test_lifecycle_synced_to_done() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_synced("PROJ-2", "Lifecycle ticket").unwrap();

        db.mark_planning("PROJ-2").unwrap();
        let t = db.get_ticket("PROJ-2").unwrap().unwrap();
        assert_eq!(t.status, "planning");

        db.mark_planned("PROJ-2", "/tmp/PROJ-2.md").unwrap();
        let t = db.get_ticket("PROJ-2").unwrap().unwrap();
        assert_eq!(t.status, "planned");
        assert_eq!(t.plan_file.as_deref(), Some("/tmp/PROJ-2.md"));

        let claimed = db.claim_for_spawning("PROJ-2", 12345).unwrap();
        assert!(claimed);
        let t = db.get_ticket("PROJ-2").unwrap().unwrap();
        assert_eq!(t.status, "spawned");
        assert_eq!(t.claimed_by, Some(12345));

        db.mark_done("PROJ-2").unwrap();
        let t = db.get_ticket("PROJ-2").unwrap().unwrap();
        assert_eq!(t.status, "done");
        assert!(t.completed_at.is_some());
        assert!(t.claimed_by.is_none());
    }

    #[test]
    fn test_claim_only_planned_tickets() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_synced("PROJ-3", "Not planned").unwrap();

        // Cannot claim a synced ticket
        let claimed = db.claim_for_spawning("PROJ-3", 99).unwrap();
        assert!(!claimed);

        let t = db.get_ticket("PROJ-3").unwrap().unwrap();
        assert_eq!(t.status, "synced"); // unchanged
    }

    #[test]
    fn test_get_planned_tickets() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_synced("A-1", "First").unwrap();
        db.insert_synced("A-2", "Second").unwrap();
        db.insert_synced("A-3", "Third").unwrap();

        db.mark_planning("A-1").unwrap();
        db.mark_planned("A-1", "/tmp/A-1.md").unwrap();
        db.mark_planning("A-2").unwrap();
        db.mark_planned("A-2", "/tmp/A-2.md").unwrap();
        // A-3 stays synced

        let planned = db.get_planned_tickets().unwrap();
        assert_eq!(planned.len(), 2);
    }

    #[test]
    fn test_mark_failed() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_synced("F-1", "Will fail").unwrap();
        db.mark_failed("F-1").unwrap();

        let t = db.get_ticket("F-1").unwrap().unwrap();
        assert_eq!(t.status, "failed");
        assert!(t.completed_at.is_some());
    }

    #[test]
    fn test_duplicate_insert_fails() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_synced("DUP-1", "First").unwrap();
        let result = db.insert_synced("DUP-1", "Duplicate");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test state`
Expected: all 6 tests pass

- [ ] **Step 3: Wire into main.rs**

Update `src/main.rs`:

```rust
mod config;
mod state;

fn main() {
    println!("config + state modules ready");
}
```

- [ ] **Step 4: Commit**

```bash
git add src/state.rs src/main.rs
git commit -m "feat: add SQLite state tracking with lifecycle management"
```

---

### Task 4: ADF-to-Markdown Converter

**Files:**
- Create: `src/jira/adf.rs`
- Create: `src/jira/mod.rs`

- [ ] **Step 1: Create jira module directory and write ADF converter with tests**

Create `src/jira/mod.rs`:

```rust
pub mod adf;
pub mod client;
```

Create `src/jira/adf.rs`:

```rust
use serde_json::Value;

/// Convert Atlassian Document Format (ADF) JSON to markdown text.
pub fn adf_to_markdown(adf: &Value) -> String {
    let mut output = String::new();
    convert_node(adf, &mut output);
    output.trim().to_string()
}

fn convert_node(node: &Value, out: &mut String) {
    match node {
        Value::Object(map) => {
            let node_type = map.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match node_type {
                "doc" | "paragraph" => {
                    convert_children(node, out);
                    if node_type == "paragraph" {
                        out.push_str("\n\n");
                    }
                }
                "text" => {
                    let text = map.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    out.push_str(text);
                }
                "hardBreak" => {
                    out.push('\n');
                }
                "heading" => {
                    let level = map.get("attrs")
                        .and_then(|a| a.get("level"))
                        .and_then(|l| l.as_u64())
                        .unwrap_or(2);
                    out.push('\n');
                    for _ in 0..level {
                        out.push('#');
                    }
                    out.push(' ');
                    convert_children(node, out);
                    out.push_str("\n\n");
                }
                "bulletList" => {
                    convert_children(node, out);
                }
                "orderedList" => {
                    if let Some(content) = map.get("content").and_then(|c| c.as_array()) {
                        for (i, item) in content.iter().enumerate() {
                            out.push_str(&format!("{}. ", i + 1));
                            convert_list_item_inline(item, out);
                            out.push('\n');
                        }
                    }
                }
                "listItem" => {
                    out.push_str("- ");
                    convert_list_item_inline(node, out);
                    out.push('\n');
                }
                "codeBlock" => {
                    let lang = map.get("attrs")
                        .and_then(|a| a.get("language"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("");
                    out.push_str(&format!("\n```{}\n", lang));
                    convert_children(node, out);
                    out.push_str("\n```\n\n");
                }
                "inlineCard" => {
                    let url = map.get("attrs")
                        .and_then(|a| a.get("url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    out.push_str(url);
                }
                "blockquote" => {
                    let mut inner = String::new();
                    convert_children(node, &mut inner);
                    for line in inner.lines() {
                        out.push_str("> ");
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push('\n');
                }
                _ => {
                    convert_children(node, out);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                convert_node(item, out);
            }
        }
        _ => {}
    }
}

fn convert_children(node: &Value, out: &mut String) {
    if let Some(content) = node.get("content").and_then(|c| c.as_array()) {
        for child in content {
            convert_node(child, out);
        }
    }
}

/// Extract inline text from a list item without adding paragraph breaks.
fn convert_list_item_inline(node: &Value, out: &mut String) {
    if let Some(content) = node.get("content").and_then(|c| c.as_array()) {
        for child in content {
            if let Some(inner) = child.get("content").and_then(|c| c.as_array()) {
                for item in inner {
                    let node_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if node_type == "text" {
                        let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        out.push_str(text);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_paragraph() {
        let adf = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "Hello world"
                }]
            }]
        });
        assert_eq!(adf_to_markdown(&adf), "Hello world");
    }

    #[test]
    fn test_heading() {
        let adf = json!({
            "type": "doc",
            "content": [{
                "type": "heading",
                "attrs": { "level": 2 },
                "content": [{
                    "type": "text",
                    "text": "My Heading"
                }]
            }]
        });
        assert_eq!(adf_to_markdown(&adf), "## My Heading");
    }

    #[test]
    fn test_bullet_list() {
        let adf = json!({
            "type": "doc",
            "content": [{
                "type": "bulletList",
                "content": [
                    {
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Item one" }]
                        }]
                    },
                    {
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Item two" }]
                        }]
                    }
                ]
            }]
        });
        assert_eq!(adf_to_markdown(&adf), "- Item one\n- Item two");
    }

    #[test]
    fn test_code_block() {
        let adf = json!({
            "type": "doc",
            "content": [{
                "type": "codeBlock",
                "attrs": { "language": "rust" },
                "content": [{
                    "type": "text",
                    "text": "fn main() {}"
                }]
            }]
        });
        let result = adf_to_markdown(&adf);
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("```"));
    }

    #[test]
    fn test_inline_card() {
        let adf = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "inlineCard",
                    "attrs": { "url": "https://example.com" }
                }]
            }]
        });
        assert_eq!(adf_to_markdown(&adf), "https://example.com");
    }

    #[test]
    fn test_empty_doc() {
        let adf = json!({
            "type": "doc",
            "content": []
        });
        assert_eq!(adf_to_markdown(&adf), "");
    }

    #[test]
    fn test_null_value() {
        let adf = Value::Null;
        assert_eq!(adf_to_markdown(&adf), "");
    }
}
```

- [ ] **Step 2: Update main.rs**

```rust
mod config;
mod jira;
mod state;

fn main() {
    println!("modules ready");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test adf`
Expected: all 7 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/jira/mod.rs src/jira/adf.rs src/main.rs
git commit -m "feat: add ADF-to-markdown converter for Jira descriptions"
```

---

### Task 5: Jira REST API Client

**Files:**
- Create: `src/jira/client.rs`

- [ ] **Step 1: Write the Jira client**

Create `src/jira/client.rs`:

```rust
use base64::Engine;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

pub struct JiraClient {
    client: Client,
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
    pub description_adf: Option<Value>,
    pub subtasks: Vec<SubtaskInfo>,
}

#[derive(Debug, Clone)]
pub struct SubtaskInfo {
    pub key: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    total: u32,
    issues: Vec<Value>,
}

impl JiraClient {
    pub fn new(base_url: &str, email: &str, api_token: &str) -> Self {
        let credentials = format!("{}:{}", email, api_token);
        let auth_header = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        );
        JiraClient {
            client: Client::new(),
            base_url: base_url.to_string(),
            auth_header,
        }
    }

    pub async fn search_tickets(
        &self,
        jql: &str,
        max_results: u32,
    ) -> Result<Vec<JiraTicket>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "{}/rest/api/3/search",
            self.base_url
        );

        let response = self.client
            .get(&url)
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/json")
            .query(&[
                ("jql", jql),
                ("maxResults", &max_results.to_string()),
                ("fields", "summary,description,issuetype,priority,status,labels,components,assignee,parent,subtasks"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Jira API request failed: {} {}", status, body).into());
        }

        let search: SearchResponse = response.json().await?;
        let tickets = search.issues.iter().map(|issue| parse_issue(issue)).collect();
        Ok(tickets)
    }
}

fn parse_issue(issue: &Value) -> JiraTicket {
    let fields = &issue["fields"];

    let subtasks: Vec<SubtaskInfo> = fields["subtasks"]
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
        key: issue["key"].as_str().unwrap_or("UNKNOWN").to_string(),
        summary: fields["summary"].as_str().unwrap_or("No summary").to_string(),
        issue_type: fields["issuetype"]["name"].as_str().unwrap_or("Task").to_string(),
        priority: fields["priority"]["name"].as_str().unwrap_or("Medium").to_string(),
        status: fields["status"]["name"].as_str().unwrap_or("To Do").to_string(),
        labels: fields["labels"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        components: fields["components"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        assignee: fields["assignee"]["displayName"]
            .as_str()
            .unwrap_or("Unassigned")
            .to_string(),
        parent_key: fields["parent"]["key"].as_str().map(String::from),
        description_adf: fields.get("description").cloned(),
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
            "key": "PROJ-123",
            "fields": {
                "summary": "Add health check",
                "issuetype": { "name": "Task" },
                "priority": { "name": "High" },
                "status": { "name": "In Progress" },
                "labels": ["backend", "api"],
                "components": [{ "name": "user-service" }],
                "assignee": { "displayName": "John Doe" },
                "parent": { "key": "PROJ-100" },
                "description": {
                    "type": "doc",
                    "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "Add endpoint" }] }]
                },
                "subtasks": [
                    { "key": "PROJ-124", "fields": { "summary": "Write tests" } }
                ]
            }
        });

        let ticket = parse_issue(&issue);
        assert_eq!(ticket.key, "PROJ-123");
        assert_eq!(ticket.summary, "Add health check");
        assert_eq!(ticket.issue_type, "Task");
        assert_eq!(ticket.priority, "High");
        assert_eq!(ticket.labels, vec!["backend", "api"]);
        assert_eq!(ticket.components, vec!["user-service"]);
        assert_eq!(ticket.assignee, "John Doe");
        assert_eq!(ticket.parent_key, Some("PROJ-100".to_string()));
        assert_eq!(ticket.subtasks.len(), 1);
        assert_eq!(ticket.subtasks[0].key, "PROJ-124");
    }

    #[test]
    fn test_parse_issue_minimal() {
        let issue = json!({
            "key": "MIN-1",
            "fields": {
                "summary": "Minimal",
                "issuetype": {},
                "priority": {},
                "status": {},
                "labels": [],
                "components": [],
                "assignee": null,
                "parent": null,
                "description": null,
                "subtasks": []
            }
        });

        let ticket = parse_issue(&issue);
        assert_eq!(ticket.key, "MIN-1");
        assert_eq!(ticket.assignee, "Unassigned");
        assert_eq!(ticket.issue_type, "Task");
        assert!(ticket.parent_key.is_none());
        assert!(ticket.description_adf.is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test client`
Expected: both tests pass

- [ ] **Step 3: Commit**

```bash
git add src/jira/client.rs
git commit -m "feat: add Jira REST API v3 client with basic auth"
```

---

### Task 6: Markdown File Generator

**Files:**
- Create: `src/markdown.rs`

- [ ] **Step 1: Write markdown generator with tests**

Create `src/markdown.rs`:

```rust
use crate::jira::adf::adf_to_markdown;
use crate::jira::client::JiraTicket;

/// Generate a complete markdown file for a ticket, including the Claude-drafted plan.
pub fn generate_ticket_markdown(ticket: &JiraTicket, plan: &str) -> String {
    let description = ticket
        .description_adf
        .as_ref()
        .filter(|v| !v.is_null())
        .map(|adf| adf_to_markdown(adf))
        .unwrap_or_else(|| "No description provided.".to_string());

    let subtasks_text = if ticket.subtasks.is_empty() {
        "No subtasks.".to_string()
    } else {
        ticket
            .subtasks
            .iter()
            .map(|s| format!("- {}: {}", s.key, s.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let labels_str = if ticket.labels.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", ticket.labels.join(", "))
    };

    let components_str = if ticket.components.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", ticket.components.join(", "))
    };

    let parent_str = ticket.parent_key.as_deref().unwrap_or("");
    let now = chrono::Utc::now().to_rfc3339();

    format!(
        r#"---
key: {key}
summary: "{summary}"
type: {issue_type}
priority: {priority}
status: {status}
labels: {labels}
components: {components}
assignee: {assignee}
parent: {parent}
fetched_at: {fetched_at}
---

# {key}: {summary}

## Description

{description}

## Subtasks

{subtasks}

## Implementation Plan

{plan}
"#,
        key = ticket.key,
        summary = ticket.summary.replace('"', r#"\""#),
        issue_type = ticket.issue_type,
        priority = ticket.priority,
        status = ticket.status,
        labels = labels_str,
        components = components_str,
        assignee = ticket.assignee,
        parent = parent_str,
        fetched_at = now,
        description = description,
        subtasks = subtasks_text,
        plan = plan,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::client::{JiraTicket, SubtaskInfo};
    use serde_json::json;

    fn make_test_ticket() -> JiraTicket {
        JiraTicket {
            key: "TEST-1".to_string(),
            summary: "Add health check endpoint".to_string(),
            issue_type: "Task".to_string(),
            priority: "High".to_string(),
            status: "In Progress".to_string(),
            labels: vec!["backend".to_string(), "api".to_string()],
            components: vec!["user-service".to_string()],
            assignee: "John Doe".to_string(),
            parent_key: Some("TEST-0".to_string()),
            description_adf: Some(json!({
                "type": "doc",
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": "Add a /health endpoint." }]
                }]
            })),
            subtasks: vec![
                SubtaskInfo {
                    key: "TEST-2".to_string(),
                    summary: "Write unit tests".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_generate_markdown_includes_frontmatter() {
        let ticket = make_test_ticket();
        let md = generate_ticket_markdown(&ticket, "Step 1: do the thing");
        assert!(md.starts_with("---\n"));
        assert!(md.contains("key: TEST-1"));
        assert!(md.contains(r#"summary: "Add health check endpoint""#));
        assert!(md.contains("type: Task"));
        assert!(md.contains("priority: High"));
        assert!(md.contains("labels: [backend, api]"));
        assert!(md.contains("parent: TEST-0"));
    }

    #[test]
    fn test_generate_markdown_includes_plan() {
        let ticket = make_test_ticket();
        let md = generate_ticket_markdown(&ticket, "## Step 1\nDo the thing\n## Step 2\nTest it");
        assert!(md.contains("## Implementation Plan"));
        assert!(md.contains("## Step 1"));
        assert!(md.contains("Do the thing"));
    }

    #[test]
    fn test_generate_markdown_includes_subtasks() {
        let ticket = make_test_ticket();
        let md = generate_ticket_markdown(&ticket, "plan");
        assert!(md.contains("- TEST-2: Write unit tests"));
    }

    #[test]
    fn test_generate_markdown_no_description() {
        let mut ticket = make_test_ticket();
        ticket.description_adf = None;
        let md = generate_ticket_markdown(&ticket, "plan");
        assert!(md.contains("No description provided."));
    }

    #[test]
    fn test_generate_markdown_no_subtasks() {
        let mut ticket = make_test_ticket();
        ticket.subtasks = vec![];
        let md = generate_ticket_markdown(&ticket, "plan");
        assert!(md.contains("No subtasks."));
    }
}
```

- [ ] **Step 2: Update main.rs**

```rust
mod config;
mod jira;
mod markdown;
mod state;

fn main() {
    println!("modules ready");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test markdown`
Expected: all 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/markdown.rs src/main.rs
git commit -m "feat: add markdown file generator with YAML frontmatter"
```

---

### Task 7: Headless Claude Plan Drafter

**Files:**
- Create: `src/planner.rs`

- [ ] **Step 1: Write the planner module**

Create `src/planner.rs`:

```rust
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{error, info};

use crate::config::Config;
use crate::jira::client::JiraTicket;

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
        "Running headless Claude session for plan drafting"
    );

    let output = Command::new("claude")
        .arg("--print")
        .arg("-p")
        .arg(&prompt)
        .current_dir(&repo_root)
        .env("CLAUDE_HOME", &claude_home)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(
            ticket_key = %ticket.key,
            exit_code = ?output.status.code(),
            stderr = %stderr,
            "Headless Claude session failed"
        );
        return Err(format!(
            "Headless Claude session failed for {}: exit code {:?}",
            ticket.key,
            output.status.code()
        )
        .into());
    }

    let plan = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(plan)
}

fn render_prompt(
    config: &Config,
    ticket: &JiraTicket,
    ticket_content: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let template = load_template(config)?;
    let branch = config.branch_for_ticket(&ticket.key);

    let rendered = template
        .replace("{{TICKET_KEY}}", &ticket.key)
        .replace("{{TICKET_SUMMARY}}", &ticket.summary)
        .replace("{{TICKET_CONTENT}}", ticket_content)
        .replace("{{BRANCH}}", &branch)
        .replace("{{BASE_BRANCH}}", &config.worktree.base_branch);

    Ok(rendered)
}

fn load_template(config: &Config) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let template_path = &config.claude.plan_prompt_template;
    if template_path.is_empty() {
        return Ok(DEFAULT_PLAN_PROMPT.to_string());
    }

    let path = crate::config::expand_path(template_path);
    if path.exists() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Err(format!("Prompt template not found: {}", path.display()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::client::JiraTicket;

    fn test_config() -> Config {
        let toml_str = r#"
[jira]
instance = "test"
email = "test@test.com"
api_token = "tok"
jql = 'status = "In Progress"'

[claude]
home_dir = "/tmp/claude-test"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]
base_branch = "main"
branch_prefix = "feature"

[tmux]

[spawner]
"#;
        toml::from_str(toml_str).unwrap()
    }

    fn test_ticket() -> JiraTicket {
        JiraTicket {
            key: "PROJ-42".to_string(),
            summary: "Add endpoint".to_string(),
            issue_type: "Task".to_string(),
            priority: "High".to_string(),
            status: "In Progress".to_string(),
            labels: vec![],
            components: vec![],
            assignee: "Test".to_string(),
            parent_key: None,
            description_adf: None,
            subtasks: vec![],
        }
    }

    #[test]
    fn test_render_prompt_default_template() {
        let config = test_config();
        let ticket = test_ticket();
        let rendered = render_prompt(&config, &ticket, "ticket body here").unwrap();

        assert!(rendered.contains("PROJ-42"));
        assert!(rendered.contains("Add endpoint"));
        assert!(rendered.contains("ticket body here"));
        assert!(rendered.contains("feature/PROJ-42"));
        assert!(rendered.contains("main"));
    }

    #[test]
    fn test_load_template_default() {
        let config = test_config();
        let template = load_template(&config).unwrap();
        assert!(template.contains("{{TICKET_KEY}}"));
    }
}
```

- [ ] **Step 2: Update main.rs**

```rust
mod config;
mod jira;
mod markdown;
mod planner;
mod state;

fn main() {
    println!("modules ready");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test planner`
Expected: both tests pass

- [ ] **Step 4: Commit**

```bash
git add src/planner.rs src/main.rs
git commit -m "feat: add headless Claude plan drafter with customizable prompt template"
```

---

### Task 8: Jira Sync Loop

**Files:**
- Modify: `src/jira/mod.rs`

- [ ] **Step 1: Write the sync loop in jira/mod.rs**

Replace `src/jira/mod.rs` with:

```rust
pub mod adf;
pub mod client;

use std::path::Path;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

use crate::config::Config;
use crate::jira::adf::adf_to_markdown;
use crate::jira::client::JiraClient;
use crate::markdown::generate_ticket_markdown;
use crate::planner;
use crate::state::StateDb;

pub async fn run_sync_loop(config: Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = JiraClient::new(
        &config.jira_base_url(),
        &config.jira.email,
        &config.jira.api_token,
    );
    let interval = Duration::from_secs(config.jira.poll_interval_secs);

    // Ensure output directory exists
    let output_dir = config.output_dir();
    std::fs::create_dir_all(&output_dir)?;

    info!(
        jql = %config.jira.jql,
        poll_interval_secs = config.jira.poll_interval_secs,
        "Starting Jira sync loop"
    );

    loop {
        if let Err(e) = sync_once(&config, &client, &output_dir).await {
            error!(error = %e, "Jira sync cycle failed");
        }
        sleep(interval).await;
    }
}

async fn sync_once(
    config: &Config,
    client: &JiraClient,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Polling Jira (JQL: {})", config.jira.jql);

    let tickets = client
        .search_tickets(&config.jira.jql, config.jira.fetch_limit)
        .await?;

    if tickets.is_empty() {
        info!("No matching tickets found");
        return Ok(());
    }

    // Open DB for each cycle to avoid holding lock across async boundaries
    let db = StateDb::open(&config.db_path())?;

    for ticket in &tickets {
        if db.is_known(&ticket.key)? {
            debug!(ticket_key = %ticket.key, "Skipping (already processed)");
            continue;
        }

        info!(
            ticket_key = %ticket.key,
            summary = %ticket.summary,
            "Picked up new Jira ticket: {} — {}",
            ticket.key,
            ticket.summary
        );

        db.insert_synced(&ticket.key, &ticket.summary)?;

        // Build preliminary ticket content for the prompt
        let description = ticket
            .description_adf
            .as_ref()
            .filter(|v| !v.is_null())
            .map(|adf| adf_to_markdown(adf))
            .unwrap_or_else(|| "No description provided.".to_string());

        db.mark_planning(&ticket.key)?;

        // Run headless Claude to draft implementation plan
        let plan = match planner::draft_plan(config, ticket, &description).await {
            Ok(plan) => {
                info!(
                    ticket_key = %ticket.key,
                    "Aggregated implementation plan for {}",
                    ticket.key
                );
                plan
            }
            Err(e) => {
                error!(
                    ticket_key = %ticket.key,
                    error = %e,
                    "Failed to draft plan for {}",
                    ticket.key
                );
                db.mark_failed(&ticket.key)?;
                continue;
            }
        };

        // Generate and write the markdown file
        let md_content = generate_ticket_markdown(ticket, &plan);
        let file_path = output_dir.join(format!("{}.md", ticket.key));
        std::fs::write(&file_path, &md_content)?;

        let file_path_str = file_path.to_string_lossy().to_string();
        info!(
            ticket_key = %ticket.key,
            path = %file_path_str,
            "Written plan file: {}",
            file_path_str
        );

        db.mark_planned(&ticket.key, &file_path_str)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/jira/mod.rs
git commit -m "feat: add Jira sync loop with plan drafting orchestration"
```

---

### Task 9: Session Spawner (SQLite Polling + Tmux)

**Files:**
- Create: `src/spawner.rs`

- [ ] **Step 1: Write the spawner module**

Create `src/spawner.rs`:

```rust
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::state::StateDb;

pub async fn run_spawner_loop(
    config: Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let interval = Duration::from_secs(config.spawner.poll_interval_secs);

    info!(
        poll_interval_secs = config.spawner.poll_interval_secs,
        "Starting session spawner loop"
    );

    loop {
        if let Err(e) = spawn_planned_tickets(&config).await {
            error!(error = %e, "Spawner cycle failed");
        }
        sleep(interval).await;
    }
}

async fn spawn_planned_tickets(
    config: &Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db = StateDb::open(&config.db_path())?;
    let planned = db.get_planned_tickets()?;

    if planned.is_empty() {
        return Ok(());
    }

    let pid = std::process::id() as i64;

    for ticket in &planned {
        let claimed = db.claim_for_spawning(&ticket.key, pid)?;
        if !claimed {
            continue;
        }

        info!(
            ticket_key = %ticket.key,
            "Detected planned ticket {}, spawning tmux session",
            ticket.key
        );

        let plan_file = match &ticket.plan_file {
            Some(f) => f.clone(),
            None => {
                warn!(ticket_key = %ticket.key, "No plan file for ticket, skipping");
                continue;
            }
        };

        if let Err(e) = spawn_tmux_session(config, &ticket.key, &plan_file).await {
            error!(
                ticket_key = %ticket.key,
                error = %e,
                "Failed to spawn tmux session for {}",
                ticket.key
            );
            // Revert to planned so it gets retried
            db.mark_planned(&ticket.key, &plan_file)?;
        }
    }

    Ok(())
}

async fn spawn_tmux_session(
    config: &Config,
    ticket_key: &str,
    plan_file: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state_dir = config.state_dir();
    let wrapper_path = state_dir.join(format!("run-{}.sh", ticket_key));
    let binary_path = std::env::current_exe()?;

    // Read the plan content
    let plan_content = std::fs::read_to_string(plan_file)?;

    // Build Claude flags
    let mut claude_args = Vec::new();
    if config.worktree.enabled {
        claude_args.push(format!(
            "--worktree {}",
            config.branch_for_ticket(ticket_key)
        ));
    }
    claude_args.push("-p".to_string());
    let claude_flags = claude_args.join(" ");

    let extra_flags = if config.claude.extra_flags.is_empty() {
        String::new()
    } else {
        format!(" {}", config.claude.extra_flags)
    };

    // Write wrapper script
    let wrapper_content = format!(
        r#"#!/bin/bash
set -uo pipefail

TICKET_KEY="{ticket_key}"
REPO_ROOT="{repo_root}"
CLAUDE_HOME_DIR="{claude_home}"
BINARY="{binary}"
PLAN_FILE="{plan_file}"

cd "$REPO_ROOT"
export CLAUDE_HOME="$CLAUDE_HOME_DIR"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Dev Pipeline — $TICKET_KEY"
echo "  Plan file: $PLAN_FILE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

PROMPT=$(cat "$PLAN_FILE")

claude {claude_flags}{extra_flags} "$PROMPT"
EXIT_CODE=$?

if [ $EXIT_CODE -eq 0 ]; then
    echo ""
    echo "Session completed successfully for $TICKET_KEY"
else
    echo ""
    echo "Session failed for $TICKET_KEY (exit code: $EXIT_CODE)"
fi

# Mark as done via callback
"$BINARY" mark-done "$TICKET_KEY" --config "{config_path}"

echo ""
echo "Session ended. Press Enter to close this pane."
read -r
"#,
        ticket_key = ticket_key,
        repo_root = config.repo_root().display(),
        claude_home = config.claude_home().display(),
        binary = binary_path.display(),
        plan_file = plan_file,
        claude_flags = claude_flags,
        extra_flags = extra_flags,
        config_path = "", // Will be set from CLI args at runtime
    );

    std::fs::write(&wrapper_path, &wrapper_content)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&wrapper_path, perms)?;
    }

    // Check if tmux session exists, create or add window
    let session_name = &config.tmux.session_name;
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?
        .success();

    let wrapper_cmd = format!("bash {}", wrapper_path.display());

    if session_exists {
        Command::new("tmux")
            .args([
                "new-window",
                "-t",
                session_name,
                "-n",
                ticket_key,
                &wrapper_cmd,
            ])
            .status()
            .await?;
        info!(
            ticket_key = %ticket_key,
            "Opened tmux window: {}:{}",
            session_name,
            ticket_key
        );
    } else {
        Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                session_name,
                "-n",
                ticket_key,
                &wrapper_cmd,
            ])
            .status()
            .await?;
        info!(
            ticket_key = %ticket_key,
            "Created tmux session with window: {}:{}",
            session_name,
            ticket_key
        );
    }

    Ok(())
}
```

- [ ] **Step 2: Update main.rs**

```rust
mod config;
mod jira;
mod markdown;
mod planner;
mod spawner;
mod state;

fn main() {
    println!("modules ready");
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/spawner.rs src/main.rs
git commit -m "feat: add session spawner with SQLite polling and tmux integration"
```

---

### Task 10: CLI + Main Entry Point

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write the full main.rs with clap CLI and tokio runtime**

Replace `src/main.rs` with:

```rust
mod config;
mod jira;
mod markdown;
mod planner;
mod spawner;
mod state;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "claude-dispatch")]
#[command(about = "Automated Jira-to-Claude Code implementation pipeline")]
struct Cli {
    /// Path to config file
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Internal: mark a ticket as done (called by tmux wrapper)
    MarkDone {
        /// Jira ticket key (e.g., PROJ-123)
        ticket_key: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load config
    let config = match config::Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config from {}: {}", cli.config.display(), e);
            std::process::exit(1);
        }
    };

    // Ensure directories exist
    let dirs = [config.state_dir(), config.log_dir(), config.output_dir()];
    for dir in &dirs {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("Failed to create directory {}: {}", dir.display(), e);
            std::process::exit(1);
        }
    }

    // Set up logging
    let log_dir = config.log_dir();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "claude-dispatch.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    match cli.command {
        Some(Commands::MarkDone { ticket_key }) => {
            handle_mark_done(&config, &ticket_key);
        }
        None => {
            run_pipeline(config).await;
        }
    }
}

fn handle_mark_done(config: &config::Config, ticket_key: &str) {
    match state::StateDb::open(&config.db_path()) {
        Ok(db) => {
            if let Err(e) = db.mark_done(ticket_key) {
                error!(
                    ticket_key = %ticket_key,
                    error = %e,
                    "Failed to mark ticket as done"
                );
                std::process::exit(1);
            }
            info!(
                ticket_key = %ticket_key,
                "{} session closed, marked as done",
                ticket_key
            );

            // Clean up wrapper script
            let wrapper = config.state_dir().join(format!("run-{}.sh", ticket_key));
            let _ = std::fs::remove_file(wrapper);
        }
        Err(e) => {
            eprintln!("Failed to open database: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_pipeline(config: config::Config) {
    info!("Starting claude-dispatch pipeline");
    info!(
        jira_instance = %config.jira.instance,
        output_dir = %config.output_dir().display(),
        repo_root = %config.repo_root().display(),
        worktree_enabled = config.worktree.enabled,
        "Configuration loaded"
    );

    let sync_config = config.clone();
    let spawner_config = config.clone();

    let sync_handle = tokio::spawn(async move {
        if let Err(e) = jira::run_sync_loop(sync_config).await {
            error!(error = %e, "Jira sync loop failed");
        }
    });

    let spawner_handle = tokio::spawn(async move {
        if let Err(e) = spawner::run_spawner_loop(spawner_config).await {
            error!(error = %e, "Session spawner loop failed");
        }
    });

    tokio::select! {
        r = sync_handle => {
            error!("Jira sync loop exited unexpectedly: {:?}", r);
        }
        r = spawner_handle => {
            error!("Session spawner loop exited unexpectedly: {:?}", r);
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 3: Verify CLI help works**

Run: `cargo run -- --help`
Expected: shows usage with `--config` option and `mark-done` subcommand

- [ ] **Step 4: Verify mark-done subcommand parses**

Run: `cargo run -- mark-done --help`
Expected: shows mark-done usage with `ticket_key` argument

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass (config: 4, state: 6, adf: 7, client: 2, markdown: 5, planner: 2 = 26 total)

- [ ] **Step 6: Run clippy and fmt**

Run: `just check`
Expected: no warnings, formatting is clean

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: add CLI entry point with concurrent sync and spawner loops"
```

---

### Task 11: Fix Spawner Config Path Passthrough

**Files:**
- Modify: `src/spawner.rs`
- Modify: `src/main.rs`

The wrapper script needs to know the config file path so `mark-done` can load the same config. We need to thread the config path through.

- [ ] **Step 1: Add config_path field to Config**

In `src/config.rs`, add a field to store the path the config was loaded from:

Add after the `Config` struct definition:

```rust
impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        config.config_path = Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        Ok(config)
    }
    // ... rest stays the same
}
```

Add to the `Config` struct:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
    // ... existing fields
}
```

- [ ] **Step 2: Update spawner.rs to use config_path**

In the wrapper script generation in `spawn_tmux_session`, replace the empty `config_path`:

```rust
        config_path = config.config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo build && cargo test`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/spawner.rs
git commit -m "fix: thread config file path through to tmux wrapper for mark-done callback"
```

---

### Task 12: End-to-End Smoke Test

**Files:** No new files — manual verification.

- [ ] **Step 1: Create a test config**

```bash
cp config.example.toml config.toml
```

Edit `config.toml` with valid Jira credentials and paths.

- [ ] **Step 2: Build release**

Run: `just release`
Expected: compiles without errors

- [ ] **Step 3: Run with invalid config to test error handling**

Run: `cargo run -- --config /nonexistent.toml`
Expected: clean error message about file not found

- [ ] **Step 4: Run all tests one final time**

Run: `just test`
Expected: all 26+ tests pass

- [ ] **Step 5: Run lints**

Run: `just check`
Expected: clean clippy + fmt

- [ ] **Step 6: Final commit if any cleanup needed**

```bash
git add -A
git commit -m "chore: final cleanup and verification"
```
