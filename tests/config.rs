use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

use claude_dispatch::config::{Config, ConfigError, expand_path};
use once_cell::sync::Lazy;

fn write_temp_toml(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("create temp file");
    f.write_all(content.as_bytes()).expect("write toml");
    f
}

/// Serializes tests that mutate process-wide env vars.
static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

struct EnvGuard {
    key: String,
    prev: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        // SAFETY: tests holding ENV_LOCK are the only env-mutating code path.
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            prev,
        }
    }

    fn unset(key: &str) -> Self {
        let prev = std::env::var(key).ok();
        // SAFETY: same as set().
        unsafe {
            std::env::remove_var(key);
        }
        Self {
            key: key.to_string(),
            prev,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: same as set().
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

/// Helper: unset all CLAUDE_DISPATCH_* env vars so a test starts clean.
/// Returns guards that restore prior values on drop.
fn clear_all_claude_dispatch_env() -> Vec<EnvGuard> {
    std::env::vars()
        .filter(|(k, _)| k.starts_with("CLAUDE_DISPATCH_"))
        .map(|(k, _)| EnvGuard::unset(&k))
        .collect()
}

/// Isolates `user_config_path()` behind a tempdir.
///
/// `directories::ProjectDirs` on Linux prefers `$XDG_CONFIG_HOME` over `$HOME`
/// when both are set (which is the default on GitHub Actions runners). Setting
/// only `HOME` therefore isn't enough — we also unset the XDG overrides so the
/// resolved path always lands inside the tempdir. On macOS the XDG unset is a
/// no-op; `HOME` alone is sufficient there.
fn isolate_user_config() -> (tempfile::TempDir, Vec<EnvGuard>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guards = vec![
        EnvGuard::set("HOME", tmp.path().to_str().unwrap()),
        EnvGuard::unset("XDG_CONFIG_HOME"),
        EnvGuard::unset("XDG_DATA_HOME"),
        EnvGuard::unset("XDG_CACHE_HOME"),
    ];
    (tmp, guards)
}

#[test]
fn test_parse_full_config() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "secret-token"
cron_schedule = "0 */2 * * * *"
fetch_limit = 10
jql = 'assignee = currentUser()'

[claude]
home_dir = "/opt/claude"
extra_flags = "--verbose"
plan_prompt_template = "Do the thing"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/home/user/repo"
state_dir = "/tmp/state"
log_dir = "/tmp/logs"

[git]
branch_prefix = "task"
base_branch = "develop"

[worktree]
enabled = false

[tmux]
session_name = "my-pipeline"

[spawner]
poll_interval_secs = 30
"#;
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse full config");

    assert_eq!(cfg.jira.instance, "acme");
    assert_eq!(cfg.jira.email, "dev@acme.com");
    assert_eq!(cfg.jira.api_token, "secret-token");
    assert_eq!(cfg.jira.cron_schedule, "0 */2 * * * *");
    assert_eq!(cfg.jira.fetch_limit, 10);
    assert_eq!(cfg.jira.jql, "assignee = currentUser()");

    assert_eq!(cfg.claude.home_dir, "/opt/claude");
    assert_eq!(cfg.claude.extra_flags, "--verbose");
    assert_eq!(cfg.claude.plan_prompt_template, "Do the thing");

    assert_eq!(cfg.paths.output_dir, "/tmp/tickets");
    assert_eq!(cfg.paths.repo_root, "/home/user/repo");
    assert_eq!(cfg.paths.state_dir, "/tmp/state");
    assert_eq!(cfg.paths.log_dir, "/tmp/logs");

    assert!(!cfg.worktree.enabled);
    assert_eq!(cfg.git.branch_prefix, "task");
    assert_eq!(cfg.git.base_branch, "develop");

    assert_eq!(cfg.tmux.session_name, "my-pipeline");
    assert_eq!(cfg.spawner.poll_interval_secs, 30);

    assert!(cfg.config_path.is_some());
}

#[test]
fn test_parse_minimal_config_with_defaults() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "token"
jql = 'status = "In Progress"'

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "~/.dev-pipeline/tickets"
repo_root = "~/projects/repo"

[worktree]

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse minimal config");

    // Defaults
    assert_eq!(cfg.jira.cron_schedule, "0 */5 * * * *");
    assert_eq!(cfg.jira.fetch_limit, 5);

    assert_eq!(cfg.paths.state_dir, "~/.dev-pipeline");
    assert_eq!(cfg.paths.log_dir, "~/.dev-pipeline/logs");

    assert!(cfg.worktree.enabled);
    assert_eq!(cfg.git.branch_prefix, "feature");
    assert_eq!(cfg.git.base_branch, "main");

    assert_eq!(cfg.tmux.session_name, "dev-pipeline");
    assert_eq!(cfg.spawner.poll_interval_secs, 10);
}

#[test]
fn test_expand_path_with_tilde() {
    let home = dirs::home_dir().expect("home dir must exist in test env");
    let expanded = expand_path("~/foo/bar");
    assert_eq!(expanded, home.join("foo/bar"));
}

#[test]
fn test_expand_path_absolute() {
    let path = "/absolute/path/to/file";
    let expanded = expand_path(path);
    assert_eq!(expanded, PathBuf::from(path));
}

// --- jira_base_url: supports both short name and full URL ---

#[test]
fn test_jira_base_url_from_short_name() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse config");
    assert_eq!(cfg.jira_base_url(), "https://acme.atlassian.net");
}

#[test]
fn test_jira_base_url_from_full_url() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "https://mercedes-benz.atlassian.net"
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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse config");
    assert_eq!(cfg.jira_base_url(), "https://mercedes-benz.atlassian.net");
}

#[test]
fn test_jira_base_url_strips_trailing_slash() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "https://example.atlassian.net/"
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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse config");
    assert_eq!(cfg.jira_base_url(), "https://example.atlassian.net");
}

// --- Security: Debug output must not leak secrets ---

#[test]
fn test_jira_config_debug_redacts_api_token() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "super-secret-token-12345"
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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse config");

    let debug_output = format!("{:?}", cfg.jira);

    assert!(
        !debug_output.contains("super-secret-token-12345"),
        "API token must not appear in Debug output, got: {}",
        debug_output
    );
    assert!(
        debug_output.contains("[REDACTED]"),
        "Debug output should show [REDACTED] for api_token, got: {}",
        debug_output
    );
    // Other fields should still be visible
    assert!(
        debug_output.contains("acme"),
        "instance should be visible in Debug"
    );
    assert!(
        debug_output.contains("dev@acme.com"),
        "email should be visible in Debug"
    );
}

#[test]
fn test_full_config_debug_redacts_api_token() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "dev@acme.com"
api_token = "my-secret-key-xyz"
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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("parse config");

    // When the full Config is debug-printed, the nested JiraConfig
    // must still redact the token
    let debug_output = format!("{:?}", cfg);

    assert!(
        !debug_output.contains("my-secret-key-xyz"),
        "API token must not leak through Config Debug, got: {}",
        debug_output
    );
}

// --- Security: reject unsafe git identifiers that could inject shell metachars ---

fn minimal_toml_with_git_block(git_block: &str) -> String {
    format!(
        r#"
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

{git_block}

[worktree]

[tmux]

[spawner]
"#
    )
}

#[test]
fn test_load_rejects_unsafe_branch_prefix() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = minimal_toml_with_git_block(
        r#"[git]
branch_prefix = "feat$(whoami)"
base_branch = "main"
"#,
    );
    let f = write_temp_toml(&toml);
    let err = Config::load(Some(f.path())).expect_err("unsafe branch_prefix must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("branch_prefix"),
        "error should mention branch_prefix: {msg}"
    );
}

#[test]
fn test_load_rejects_unsafe_base_branch() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = minimal_toml_with_git_block(
        r#"[git]
branch_prefix = "feature"
base_branch = "main;rm -rf /"
"#,
    );
    let f = write_temp_toml(&toml);
    let err = Config::load(Some(f.path())).expect_err("unsafe base_branch must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("base_branch"),
        "error should mention base_branch: {msg}"
    );
}

#[test]
fn test_config_error_variants_format() {
    use claude_dispatch::config::ConfigError;
    use std::path::PathBuf;

    let io = ConfigError::Io {
        path: PathBuf::from("/tmp/foo.toml"),
        source: std::io::Error::from(std::io::ErrorKind::NotFound),
    };
    assert!(io.to_string().contains("/tmp/foo.toml"));
    assert!(io.to_string().contains("failed to read"));

    let unknown = ConfigError::UnknownSchemaVersion {
        found: 99,
        supported: &[1],
    };
    assert!(unknown.to_string().contains("99"));

    let val = ConfigError::Validation(vec!["a is bad".into(), "b is worse".into()]);
    let s = val.to_string();
    assert!(s.contains("a is bad"));
    assert!(s.contains("b is worse"));

    let wiz = ConfigError::WizardBootstrap(PathBuf::from("/tmp/cfg"));
    assert!(wiz.to_string().contains("/tmp/cfg"));

    let nod = ConfigError::NoUserConfigDir;
    assert!(!nod.to_string().is_empty());
}

// --- paths::user_config_path / paths::binary_adjacent_config_path ---

#[test]
fn test_user_config_path_is_absolute_and_under_home() {
    use claude_dispatch::config::user_config_path;
    let p = user_config_path().expect("user config path resolvable on this OS");
    assert!(p.is_absolute(), "expected absolute path, got {:?}", p);
    assert!(
        p.file_name().map(|n| n == "config.toml").unwrap_or(false),
        "expected filename config.toml, got {:?}",
        p
    );
    let home = dirs::home_dir().expect("home dir must exist in test env");
    assert!(
        p.starts_with(&home),
        "expected path under home dir {:?}, got {:?}",
        home,
        p
    );
}

#[test]
fn test_cli_path_loads_specified_file() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

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
poll_interval_secs = 42
"#;
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("cli path loads");
    assert_eq!(cfg.spawner.poll_interval_secs, 42);
    assert_eq!(cfg.jira.instance, "acme");
}

#[test]
fn test_binary_adjacent_config_path_matches_exe_dir() {
    use claude_dispatch::config::binary_adjacent_config_path;
    let p = binary_adjacent_config_path().expect("current_exe should resolve in tests");
    assert!(p.is_absolute(), "expected absolute path, got {:?}", p);
    assert!(
        p.file_name().map(|n| n == "config.toml").unwrap_or(false),
        "expected filename config.toml, got {:?}",
        p
    );
    // Sibling of the current exe.
    let exe = std::env::current_exe().expect("current_exe in tests");
    assert_eq!(
        p.parent().expect("path must have parent"),
        exe.parent().expect("exe must have parent"),
        "binary-adjacent path must be sibling of exe"
    );
}

// --- Env var overlay (Task 4) ---

#[test]
fn test_env_overrides_file_value() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let _g = EnvGuard::set("CLAUDE_DISPATCH_JIRA__EMAIL", "c@d");
    let cfg = Config::load(Some(f.path())).expect("load with env overlay");
    assert_eq!(cfg.jira.email, "c@d");
    assert_eq!(cfg.jira.instance, "acme"); // file value still present
}

#[test]
fn test_env_nesting_double_underscore() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
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
poll_interval_secs = 10
"#;
    let f = write_temp_toml(toml);
    let _g = EnvGuard::set("CLAUDE_DISPATCH_SPAWNER__POLL_INTERVAL_SECS", "99");
    let cfg = Config::load(Some(f.path())).expect("load with nested env");
    assert_eq!(cfg.spawner.poll_interval_secs, 99);
}

#[test]
fn test_env_coerces_bool_and_int() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
api_token = "token"
jql = 'status = "In Progress"'
fetch_limit = 5

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]
enabled = true

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let _g1 = EnvGuard::set("CLAUDE_DISPATCH_WORKTREE__ENABLED", "false");
    let _g2 = EnvGuard::set("CLAUDE_DISPATCH_JIRA__FETCH_LIMIT", "7");
    let cfg = Config::load(Some(f.path())).expect("load with coercion");
    assert!(!cfg.worktree.enabled);
    assert_eq!(cfg.jira.fetch_limit, 7);
}

#[test]
fn test_env_string_field_with_boolish_value_round_trips() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
api_token = "placeholder"
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
    let f = write_temp_toml(toml);
    // Regression: prior to the env-coercion fix, this would deserialize as a
    // TOML Boolean and fail with an opaque type-mismatch error against the
    // String-typed api_token field.
    let _g = EnvGuard::set("CLAUDE_DISPATCH_JIRA__API_TOKEN", "true");
    let cfg = Config::load(Some(f.path())).expect("string field accepts boolish env value");
    assert_eq!(cfg.jira.api_token, "true");
}

#[test]
fn test_env_non_matching_prefix_ignored() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    // Unrelated env vars should not appear.
    let _g1 = EnvGuard::set("OTHER_VAR", "foo");
    let _g2 = EnvGuard::set("FOO_BAR", "bar");
    let cfg = Config::load(Some(f.path())).expect("load ignores non-matching env");
    // No assertion needed beyond successful load; we're making sure collection didn't
    // accidentally merge something named "foo" or "other" into the config root.
    assert_eq!(cfg.jira.email, "a@b");
}

#[test]
fn test_env_only_load_succeeds_with_all_required() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    // Use a bogus HOME so user_config_path points somewhere that doesn't exist.
    // (ProjectDirs consults HOME on Unix.)
    let (_tmp_home, _home_guards) = isolate_user_config();

    // No -c flag, no file at user_config_path. Supply all required fields via env.
    // `worktree`, `tmux`, `spawner` sections are required to exist in the Config
    // schema (their inner fields have defaults, but the parent tables don't), so
    // we seed one field in each to materialize them.
    let guards = vec![
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__INSTANCE", "acme"),
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__EMAIL", "e@f"),
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__API_TOKEN", "tok"),
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__JQL", "status"),
        EnvGuard::set("CLAUDE_DISPATCH_CLAUDE__HOME_DIR", "~/.claude"),
        EnvGuard::set("CLAUDE_DISPATCH_PATHS__OUTPUT_DIR", "/tmp/tickets"),
        EnvGuard::set("CLAUDE_DISPATCH_PATHS__REPO_ROOT", "/tmp/repo"),
        EnvGuard::set("CLAUDE_DISPATCH_WORKTREE__ENABLED", "true"),
        EnvGuard::set("CLAUDE_DISPATCH_TMUX__SESSION_NAME", "dev-pipeline"),
        EnvGuard::set("CLAUDE_DISPATCH_SPAWNER__POLL_INTERVAL_SECS", "10"),
    ];
    let cfg = Config::load(None).expect("env-only load should succeed");
    assert_eq!(cfg.jira.instance, "acme");
    assert_eq!(cfg.jira.email, "e@f");
    // No -c was passed, so config_path stays None — sub-invocations re-discover
    // the layered sources rather than fixate on whichever file happened to win.
    assert!(cfg.config_path.is_none());
    drop(guards);
}

// --- First-run wizard (Task 5) ---

#[test]
fn test_wizard_writes_template_when_no_sources() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let (_tmp_home, _home_guards) = isolate_user_config();

    let result = Config::load(None);
    let path = match result {
        Err(ConfigError::WizardBootstrap(p)) => p,
        other => panic!("expected WizardBootstrap, got {:?}", other.map(|_| ())),
    };
    assert!(
        path.exists(),
        "wizard must create template file at {:?}",
        path
    );
    let content = std::fs::read_to_string(&path).expect("read template");
    let expected = include_str!("../config.example.toml");
    assert_eq!(
        content, expected,
        "template content must match config.example.toml byte-for-byte"
    );
}

#[test]
fn test_wizard_creates_parent_dir() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let (_tmp_home, _home_guards) = isolate_user_config();

    // user_config_path lives under HOME/.config/... (linux) or
    // HOME/Library/Application Support/... (macOS). Neither is pre-created in
    // the tempdir, so write_template must create the parent.
    let _ = Config::load(None); // triggers wizard; errors expected
    let expected = claude_dispatch::config::user_config_path().unwrap();
    assert!(expected.exists(), "wizard should have written template");
    assert!(
        expected.parent().unwrap().is_dir(),
        "wizard should have created parent dir"
    );
}

#[test]
fn test_wizard_not_triggered_when_cli_provided() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let (_tmp_home, _home_guards) = isolate_user_config();

    let bogus = _tmp_home.path().join("nonexistent.toml");
    let err = Config::load(Some(&bogus)).expect_err("should fail on missing cli path");
    match err {
        ConfigError::Io { .. } => {}
        other => panic!("expected Io error for missing file, got {:?}", other),
    }
    // Wizard must NOT have written a template — `-c` opts out of the bootstrap.
    let user_cfg = claude_dispatch::config::user_config_path().unwrap();
    assert!(
        !user_cfg.exists(),
        "wizard should not have written template at {:?}",
        user_cfg
    );
}

// --- Schema versioning (Task 6) ---

#[test]
fn test_schema_version_1_loads() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
schema_version = 1

[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("schema v1 should load");
    assert_eq!(cfg.schema_version, 1);
}

#[test]
fn test_missing_schema_version_defaults_to_1() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let cfg = Config::load(Some(f.path())).expect("missing version should default");
    assert_eq!(cfg.schema_version, 1);
}

#[test]
fn test_unknown_schema_version_errors() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
schema_version = 999

[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("schema_version 999 must fail");
    match err {
        ConfigError::UnknownSchemaVersion { found, supported } => {
            assert_eq!(found, 999);
            assert_eq!(supported, &[1]);
        }
        other => panic!("expected UnknownSchemaVersion, got {:?}", other),
    }
}

#[test]
fn test_unknown_schema_version_via_env_errors() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    // Regression: env vars arrive as toml::Value::String, but schema_version
    // extraction in loader.rs previously only consulted as_integer(), which
    // returns None for strings — so the unknown-version guard was bypassed
    // and Config.schema_version silently took the env value (parsed by
    // from_str_or_native). Loader now also accepts a parseable string here.
    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let _g = EnvGuard::set("CLAUDE_DISPATCH_SCHEMA_VERSION", "999");
    let err = Config::load(Some(f.path())).expect_err("env-set schema_version 999 must fail");
    match err {
        ConfigError::UnknownSchemaVersion { found, supported } => {
            assert_eq!(found, 999);
            assert_eq!(supported, &[1]);
        }
        other => panic!("expected UnknownSchemaVersion, got {:?}", other),
    }
}

#[test]
fn test_wizard_not_triggered_when_env_provides_config() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let (_tmp_home, _home_guards) = isolate_user_config();

    // No file anywhere. Env supplies everything.
    let _guards = vec![
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__INSTANCE", "acme"),
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__EMAIL", "e@f"),
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__API_TOKEN", "tok"),
        EnvGuard::set("CLAUDE_DISPATCH_JIRA__JQL", "status"),
        EnvGuard::set("CLAUDE_DISPATCH_CLAUDE__HOME_DIR", "~/.claude"),
        EnvGuard::set("CLAUDE_DISPATCH_PATHS__OUTPUT_DIR", "/tmp/tickets"),
        EnvGuard::set("CLAUDE_DISPATCH_PATHS__REPO_ROOT", "/tmp/repo"),
        EnvGuard::set("CLAUDE_DISPATCH_WORKTREE__ENABLED", "true"),
        EnvGuard::set("CLAUDE_DISPATCH_TMUX__SESSION_NAME", "x"),
        EnvGuard::set("CLAUDE_DISPATCH_SPAWNER__POLL_INTERVAL_SECS", "10"),
    ];
    let cfg = Config::load(None).expect("env-only load should not trigger wizard");
    assert_eq!(cfg.jira.email, "e@f");
}

// --- Multi-issue validation (Task 7) ---

#[test]
fn test_validation_accumulates_multiple_issues() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "noatsign"
api_token = "token"
jql = 'status = "In Progress"'
cron_schedule = "totally wrong"

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[git]
branch_prefix = "feat$(whoami)"
base_branch = "main"

[worktree]

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("multi-issue should error");
    match err {
        ConfigError::Validation(v) => {
            assert_eq!(v.len(), 3, "expected exactly three issues, got {:?}", v);
            assert!(
                v.iter().any(|s| s.contains("branch_prefix")),
                "missing branch_prefix: {:?}",
                v
            );
            assert!(
                v.iter().any(|s| s.contains("email")),
                "missing email: {:?}",
                v
            );
            assert!(
                v.iter().any(|s| s.contains("cron_schedule")),
                "missing cron: {:?}",
                v
            );
        }
        other => panic!("expected Validation, got {:?}", other),
    }
}

#[test]
fn test_validation_rejects_placeholder_token() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
api_token = "your-api-token"
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
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("placeholder token must fail");
    let msg = err.to_string();
    assert!(msg.contains("api_token"), "{msg}");
    assert!(msg.contains("placeholder"), "{msg}");
}

#[test]
fn test_validation_rejects_bad_cron() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
api_token = "token"
jql = 'status = "In Progress"'
cron_schedule = "totally wrong"

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("bad cron must fail");
    assert!(err.to_string().contains("cron_schedule"), "{err}");
}

#[test]
fn test_validation_rejects_bad_log_level() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
log_level = "loud"

[jira]
instance = "acme"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("bad log_level must fail");
    assert!(err.to_string().contains("log_level"), "{err}");
}

// --- CLI flag parsing (Task 8) ---

#[test]
fn test_cli_parse_config_flag() {
    use clap::Parser;
    use claude_dispatch::Cli;

    let cli = Cli::try_parse_from(["claude-dispatch", "-c", "foo.toml"]).expect("parse -c");
    assert_eq!(
        cli.config.as_deref(),
        Some(std::path::Path::new("foo.toml"))
    );
    assert!(!cli.init);
    assert!(!cli.print_config);
}

#[test]
fn test_cli_parse_long_config_flag() {
    use clap::Parser;
    use claude_dispatch::Cli;

    let cli =
        Cli::try_parse_from(["claude-dispatch", "--config", "bar.toml"]).expect("parse --config");
    assert_eq!(
        cli.config.as_deref(),
        Some(std::path::Path::new("bar.toml"))
    );
}

#[test]
fn test_cli_parse_init_flag() {
    use clap::Parser;
    use claude_dispatch::Cli;

    let cli = Cli::try_parse_from(["claude-dispatch", "--init"]).expect("parse --init");
    assert!(cli.init);
    assert!(cli.config.is_none());
}

#[test]
fn test_cli_parse_print_config_flag() {
    use clap::Parser;
    use claude_dispatch::Cli;

    let cli =
        Cli::try_parse_from(["claude-dispatch", "--print-config"]).expect("parse --print-config");
    assert!(cli.print_config);
}

#[test]
fn test_cli_default_config_is_none() {
    use clap::Parser;
    use claude_dispatch::Cli;

    let cli = Cli::try_parse_from(["claude-dispatch"]).expect("parse no args");
    assert!(
        cli.config.is_none(),
        "config should default to None (triggers layered search)"
    );
    assert!(!cli.init);
    assert!(!cli.print_config);
}

#[test]
fn test_init_writes_template_to_user_config_path() {
    use claude_dispatch::config::{user_config_path, write_template};

    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let (_tmp_home, _home_guards) = isolate_user_config();

    let path = user_config_path().expect("user config path");
    write_template(&path).expect("write template");
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).expect("read back");
    let expected = include_str!("../config.example.toml");
    assert_eq!(content, expected);
}

#[test]
fn test_validation_rejects_zero_fetch_limit() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    let toml = r#"
[jira]
instance = "acme"
email = "a@b"
api_token = "token"
jql = 'status = "In Progress"'
fetch_limit = 0

[claude]
home_dir = "~/.claude"

[paths]
output_dir = "/tmp/tickets"
repo_root = "/tmp/repo"

[worktree]

[tmux]

[spawner]
"#;
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("fetch_limit=0 must fail");
    assert!(err.to_string().contains("fetch_limit"), "{err}");
}

#[test]
fn test_validation_rejects_schemeless_fqdn_instance() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _cleanup = clear_all_claude_dispatch_env();

    // Regression: a schemeless FQDN like "acme.atlassian.net" would otherwise
    // round-trip through `jira_base_url()` as "https://acme.atlassian.net.atlassian.net".
    let toml = r#"
[jira]
instance = "acme.atlassian.net"
email = "a@b"
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
    let f = write_temp_toml(toml);
    let err = Config::load(Some(f.path())).expect_err("schemeless FQDN must fail");
    let msg = err.to_string();
    assert!(msg.contains("jira.instance"), "{msg}");
    assert!(msg.contains("https://"), "{msg}");
}
