# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Is

`claude-dispatch` is a Rust daemon that automates a Jira-to-Claude-Code implementation pipeline. It polls Jira for in-progress tickets, drafts implementation plans using headless Claude Code sessions, then spawns tmux windows where Claude Code executes those plans against the target repo.

## Build & Development Commands

```bash
just build          # cargo build
just release        # cargo build --release
just run            # cargo run (starts the daemon)
just test           # cargo test
just check          # cargo clippy -- -D warnings && cargo fmt --check
just fmt            # cargo fmt
just setup          # configure git hooks (run once after clone)
just doctor         # verify all required tools and config are present
```

Run a single test: `cargo test test_name`

The project uses Rust 2024 edition. A pre-commit hook enforces `cargo fmt --check`.

## Configuration

The binary reads a `config.toml` (see `config.example.toml`). Paths support `~/` expansion via `config::expand_path`. All config sections (`jira`, `claude`, `paths`, `worktree`, `tmux`, `spawner`) are required in the TOML but most fields have defaults.

## Architecture

Two concurrent tokio tasks run in the main pipeline:

1. **Sync loop** (`src/jira/mod.rs` → `run_sync_loop`): Polls Jira via REST API v3, inserts new tickets into SQLite, converts ADF descriptions to markdown, then invokes `claude --print -p` to draft an implementation plan. Writes a `{TICKET_KEY}.md` plan file to the output directory.

2. **Spawner loop** (`src/spawner.rs` → `run_spawner_loop`): Polls SQLite for tickets in `planned` state, atomically claims them, generates a bash wrapper script, and spawns a tmux window that runs Claude Code with the plan as the prompt.

### Ticket Lifecycle (SQLite state machine)

`synced` → `planning` → `planned` → `spawned` → `done`/`failed`

State transitions are in `src/state.rs`. The `claim_for_spawning` method uses a conditional UPDATE for atomic claiming.

### Key Modules

- `src/jira/client.rs` — Jira REST client with Basic auth, parses API v3 JSON responses into `JiraTicket` structs
- `src/jira/adf.rs` — Recursive converter from Atlassian Document Format (ADF) JSON to markdown
- `src/markdown.rs` — Generates ticket markdown files with YAML frontmatter + implementation plan
- `src/planner.rs` — Renders a prompt template with ticket data, invokes `claude --print -p` as a subprocess
- `src/config.rs` — TOML config deserialization with `~/` path expansion
- `src/state.rs` — SQLite wrapper using rusqlite with WAL mode

### The `mark-done` Subcommand

`claude-dispatch mark-done TICKET_KEY` is called by the tmux wrapper script when a Claude session ends. It updates the ticket to `done` in SQLite and cleans up the wrapper script.

## Workflow Preferences

- Prefer to work on a feature branch unless the user explicitly asks for a worktree.
