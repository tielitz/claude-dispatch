# Claude Jira Workflow — Rust Application Design

## Context

The existing shell-script-based Jira-to-Claude pipeline needs to be reimplemented as a Rust application. The original pipeline (in `spec/src/`) uses bash scripts for Jira polling, filesystem watching, and tmux session spawning. The Rust rewrite improves reliability, type safety, configurability, and maintainability.

The application automates: fetching Jira tickets assigned to the current user that are "In Progress", drafting implementation plans via headless Claude Code sessions, and spawning interactive Claude Code sessions in tmux for the user to review and execute those plans.

## Architecture Overview

Single async binary running two concurrent loops via `tokio::spawn`:

```
┌──────────────────────────────────────────────────────────────┐
│                    claude-dispatch (single binary)               │
│                                                              │
│  ┌─────────────────────┐     ┌────────────────────────────┐  │
│  │  Jira Sync Loop     │     │  Session Spawner Loop      │  │
│  │                     │     │                            │  │
│  │  1. Poll Jira API   │     │  1. Poll SQLite for        │  │
│  │  2. Fetch ticket    │     │     status='planned'       │  │
│  │  3. Run headless    │     │  2. Claim ticket (atomic)  │  │
│  │     Claude for plan │     │  3. Spawn tmux window      │  │
│  │  4. Write .md file  │     │     with Claude in plan    │  │
│  │  5. Insert SQLite   │     │     mode                   │  │
│  │     (status=planned)│     │  4. Wrapper calls back     │  │
│  │  6. Sleep interval  │     │     mark-done on exit      │  │
│  └────────┬────────────┘     └──────────┬─────────────────┘  │
│           │                             │                    │
│           └──────────┬──────────────────┘                    │
│                      ▼                                       │
│              ┌───────────────┐                               │
│              │    SQLite     │                               │
│              │  state.db     │                               │
│              └───────────────┘                               │
└──────────────────────────────────────────────────────────────┘
```

## Project Structure

```
claude-dispatch-workflow/
├── Cargo.toml
├── justfile
├── config.example.toml
└── src/
    ├── main.rs              # Entry point, spawns both async loops
    ├── config.rs            # TOML config deserialization via serde
    ├── jira/
    │   ├── mod.rs           # Jira sync loop (poll → fetch → plan → write)
    │   ├── client.rs        # Jira REST API v3 client (reqwest + basic auth)
    │   └── adf.rs           # Atlassian Document Format → markdown converter
    ├── planner.rs           # Headless Claude session for plan drafting
    ├── spawner.rs           # Session spawner loop + tmux integration
    ├── state.rs             # SQLite state tracking (rusqlite)
    └── markdown.rs          # Ticket markdown file generation (frontmatter + body)
```

## Configuration

TOML config file (`config.toml`), path configurable via `--config` CLI flag (defaults to `./config.toml`).

```toml
[jira]
instance = "mycompany"                    # → mycompany.atlassian.net
email = "you@company.com"
api_token = "your-api-token"
poll_interval_secs = 60
fetch_limit = 5
jql = 'assignee = currentUser() AND status = "In Progress"'

[claude]
home_dir = "~/.claude-dispatch"              # Custom CLAUDE_HOME for this pipeline
extra_flags = ""                          # Additional flags passed to claude CLI
plan_prompt_template = ""                 # Path to custom prompt template file (optional)

[paths]
output_dir = "~/.dev-pipeline/tickets"   # Where markdown plan files are written
repo_root = "~/projects/my-service"      # Target repository for Claude sessions
state_dir = "~/.dev-pipeline"            # SQLite DB + logs directory
log_dir = "~/.dev-pipeline/logs"         # Log file directory

[worktree]
enabled = true                            # false = work on current branch
branch_prefix = "feature"                 # Branch: feature/PROJ-123
base_branch = "main"

[tmux]
session_name = "dev-pipeline"

[spawner]
poll_interval_secs = 10                   # How often to check for planned tickets
```

### Prompt Template

The headless Claude plan-drafting session uses a customizable prompt template. If `claude.plan_prompt_template` is set, the file at that path is loaded; otherwise a built-in default is used.

Available placeholders:
- `{{TICKET_KEY}}` — e.g., `PROJ-123`
- `{{TICKET_SUMMARY}}` — ticket title
- `{{TICKET_CONTENT}}` — full markdown content of the ticket
- `{{BRANCH}}` — e.g., `feature/PROJ-123`
- `{{BASE_BRANCH}}` — e.g., `main`

Default prompt instructs Claude to draft an implementation plan considering the repository context (CLAUDE.md, existing patterns, architecture).

## Phase 1: Jira Sync + Plan Drafting

### Flow

1. **Poll Jira REST API v3** at `https://<instance>.atlassian.net/rest/api/3/search` with configured JQL
2. **For each ticket** not already in SQLite:
   a. Parse issue fields: key, summary, description (ADF), issuetype, priority, status, labels, components, assignee, parent, subtasks
   b. Convert ADF description to markdown text
   c. Log: `INFO "Picked up new Jira ticket: PROJ-123 — <summary>"`
   d. Insert into SQLite with `status = 'synced'`
3. **Run headless Claude session** inside `repo_root`:
   ```
   CLAUDE_HOME=<claude.home_dir> claude --print -p "<rendered prompt template>"
   ```
   Claude has full repo access (reads CLAUDE.md, explores code) to produce a context-aware plan.
   Update SQLite: `status = 'planning'`
4. **On Claude completion**, capture stdout as the implementation plan
   Log: `INFO "Aggregated implementation plan for PROJ-123"`
5. **Write markdown file** to `<output_dir>/PROJ-123.md`:
   ```markdown
   ---
   key: PROJ-123
   summary: "Add health check endpoint"
   type: Task
   priority: High
   status: In Progress
   labels: [backend, api]
   components: [user-service]
   assignee: John Doe
   parent: PROJ-100
   fetched_at: 2026-03-29T10:00:00+00:00
   ---

   # PROJ-123: Add health check endpoint

   ## Description

   <converted ticket description>

   ## Subtasks

   <subtask list>

   ## Implementation Plan

   <Claude's drafted plan>
   ```
   Log: `INFO "Written plan file: <path>/PROJ-123.md"`
6. **Update SQLite**: `status = 'planned'`, `planned_at = now()`, `plan_file = <path>`
7. **Sleep** `poll_interval_secs`, repeat

### ADF Conversion

Jira Cloud v3 returns descriptions in Atlassian Document Format (JSON). The `adf.rs` module recursively walks the ADF tree and converts to markdown:
- `heading` → `## text`
- `bulletList` / `listItem` → `- text`
- `orderedList` → `1. text`
- `codeBlock` → fenced code block
- `text` → plain text
- `hardBreak` → newline
- `inlineCard` → URL

### Jira Authentication

HTTP Basic auth: `base64(email:api_token)` in `Authorization: Basic <token>` header.

## Phase 2: Session Spawner

### Flow

1. **Poll SQLite** every `spawner.poll_interval_secs` for rows where `status = 'planned'`
2. **Claim ticket** atomically: `UPDATE processed_tickets SET status = 'spawned', claimed_by = <pid>, spawned_at = now() WHERE key = ? AND status = 'planned'`
3. Log: `INFO "Detected planned ticket PROJ-123, spawning tmux session"`
4. **Read the plan markdown file** from the path stored in SQLite
5. **Write a wrapper shell script** to a temp file (`<state_dir>/run-PROJ-123.sh`):
   ```bash
   #!/bin/bash
   cd <repo_root>
   export CLAUDE_HOME=<claude.home_dir>
   claude [--worktree feature/PROJ-123] -p "<plan content>"
   # On exit:
   <binary_path> mark-done PROJ-123
   ```
   - If `worktree.enabled = true`: includes `--worktree <branch_prefix>/PROJ-123`
   - If `worktree.enabled = false`: no worktree flag, runs on current branch
6. **Open tmux window**: `tmux new-window -t <session_name> -n PROJ-123 "bash /path/to/run-PROJ-123.sh"`
   - If tmux session doesn't exist yet, create it with `tmux new-session -d -s <session_name> -n PROJ-123 ...`
7. Log: `INFO "Opened tmux window: dev-pipeline:PROJ-123"`

### Completion Callback

The binary exposes a hidden `mark-done` subcommand used only by the wrapper script:

```
claude-dispatch mark-done PROJ-123
```

This:
1. Updates SQLite: `status = 'done'`, `completed_at = now()`, clears `claimed_by`
2. Logs: `INFO "PROJ-123 session closed, marked as done"`
3. Cleans up the wrapper script file

## SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS processed_tickets (
    key TEXT PRIMARY KEY,
    summary TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'synced',
    -- Lifecycle: synced → planning → planned → spawned → done/failed
    plan_file TEXT,
    claimed_by INTEGER,
    synced_at TEXT NOT NULL,
    planned_at TEXT,
    spawned_at TEXT,
    completed_at TEXT
);
```

Database location: `<state_dir>/state.db`

## CLI Interface

```
claude-dispatch [OPTIONS]                  # Run both loops (default)
claude-dispatch mark-done <TICKET_KEY>     # Internal: mark ticket as done (called by wrapper)
claude-dispatch --config <PATH>            # Specify config file path
```

The `mark-done` subcommand is not documented to users — it's an internal implementation detail for the tmux wrapper callback.

## Logging

Using `tracing` + `tracing-subscriber`. Output to both stderr and `<log_dir>/claude-dispatch.log`.

| Event | Level | Message |
|-------|-------|---------|
| Jira poll start | INFO | `Polling Jira (JQL: ...)` |
| New ticket picked up | INFO | `Picked up new Jira ticket: PROJ-123 — <summary>` |
| Plan aggregation done | INFO | `Aggregated implementation plan for PROJ-123` |
| Plan file written | INFO | `Written plan file: <path>/PROJ-123.md` |
| Ticket claimed | INFO | `Detected planned ticket PROJ-123, spawning tmux session` |
| Tmux window opened | INFO | `Opened tmux window: dev-pipeline:PROJ-123` |
| Session completed | INFO | `PROJ-123 session closed, marked as done` |
| Duplicate skipped | DEBUG | `Skipping PROJ-123 (already processed)` |
| Jira API error | ERROR | `Jira API request failed: <status> <message>` |
| Claude process failed | ERROR | `Headless Claude session failed for PROJ-123: exit code <N>` |

## Justfile

```just
default:
    just --list

build:
    cargo build

release:
    cargo build --release

run:
    cargo run

test:
    cargo test

check:
    cargo clippy -- -D warnings && cargo fmt --check

fmt:
    cargo fmt

clean:
    cargo clean
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | latest | Async runtime (features: full) |
| `reqwest` | latest | HTTP client for Jira REST API (features: json, rustls-tls) |
| `rusqlite` | latest | SQLite database (features: bundled) |
| `serde` | latest | Serialization framework (features: derive) |
| `serde_json` | latest | JSON parsing for Jira API responses |
| `toml` | latest | TOML config file parsing |
| `tracing` | latest | Structured logging |
| `tracing-subscriber` | latest | Log formatting and output |
| `tracing-appender` | latest | File-based log output |
| `clap` | latest | CLI argument parsing (features: derive) |
| `base64` | latest | Jira basic auth encoding |
| `chrono` | latest | Timestamps (features: serde) |
| `dirs` | latest | Home directory / tilde expansion |

## Error Handling

- Jira API failures: log error, skip cycle, retry next interval. Don't crash.
- Headless Claude failure: log error, update SQLite `status = 'failed'`, continue to next ticket.
- SQLite errors: fatal — log and exit (database is critical path).
- Tmux spawn failure: log error, revert SQLite status to `planned` so it gets retried.
- Config parse failure: fatal — log and exit with helpful message.

## Verification

1. **Build**: `just build` compiles without errors
2. **Config**: Create `config.toml` with test Jira credentials, verify parsing
3. **Jira sync**: Run with valid credentials, verify tickets appear in SQLite and markdown files are written
4. **Plan drafting**: Verify headless Claude produces a plan section in the markdown output
5. **Tmux spawning**: Verify tmux windows open with Claude in plan mode
6. **Completion**: Close a tmux window, verify SQLite status updates to `done`
7. **Duplicate prevention**: Run sync twice, verify same ticket isn't processed again
8. **Worktree toggle**: Test with `worktree.enabled = true` and `false`
