use crate::config::{error::ConfigError, paths, schema::Config};
use std::fs;
use std::path::{Path, PathBuf};

pub fn load(cli: Option<&Path>) -> Result<Config, ConfigError> {
    let sources = resolve_file_sources(cli);
    let env = collect_env();
    let env_empty = env.as_table().map(|t| t.is_empty()).unwrap_or(true);

    if cli.is_none() && sources.is_empty() && env_empty {
        // Task 5 replaces this with wizard bootstrap.
        return Err(ConfigError::Io {
            path: paths::user_config_path()?,
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });
    }

    let mut merged = toml::Value::Table(Default::default());
    for path in &sources {
        let text = fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.clone(),
            source: e,
        })?;
        let layer: toml::Value = toml::from_str(&text).map_err(|e| ConfigError::Parse {
            path: path.clone(),
            source: e,
        })?;
        merge_toml(&mut merged, layer);
    }

    merge_toml(&mut merged, env);

    let last_path = sources.last().cloned();
    let mut cfg: Config = merged
        .try_into()
        .map_err(|e: toml::de::Error| ConfigError::Parse {
            path: last_path.clone().unwrap_or_default(),
            source: e,
        })?;
    if let Some(p) = last_path {
        cfg.config_path = Some(p.canonicalize().unwrap_or(p));
    }
    validate(&cfg)?;
    Ok(cfg)
}

fn resolve_file_sources(cli: Option<&Path>) -> Vec<PathBuf> {
    if let Some(p) = cli {
        return vec![p.to_path_buf()];
    }
    let mut out = Vec::new();
    if let Ok(p) = paths::user_config_path()
        && p.exists()
    {
        out.push(p);
    }
    if let Some(p) = paths::binary_adjacent_config_path()
        && p.exists()
    {
        out.push(p);
    }
    out
}

pub(crate) fn merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                let slot = b.entry(k).or_insert(toml::Value::Table(Default::default()));
                merge_toml(slot, v);
            }
        }
        (slot, other) => *slot = other,
    }
}

fn collect_env() -> toml::Value {
    let mut root = toml::value::Table::new();
    for (k, v) in std::env::vars() {
        let Some(rest) = k.strip_prefix("CLAUDE_DISPATCH_") else {
            continue;
        };
        let path: Vec<String> = rest.split("__").map(|s| s.to_ascii_lowercase()).collect();
        if path.is_empty() || path.iter().any(String::is_empty) {
            continue;
        }
        let val = parse_env_value(&v);
        insert_nested(&mut root, &path, val);
    }
    toml::Value::Table(root)
}

fn parse_env_value(s: &str) -> toml::Value {
    if let Ok(b) = s.parse::<bool>() {
        return toml::Value::Boolean(b);
    }
    if let Ok(i) = s.parse::<i64>() {
        return toml::Value::Integer(i);
    }
    toml::Value::String(s.to_string())
}

fn insert_nested(table: &mut toml::value::Table, path: &[String], value: toml::Value) {
    match path {
        [] => {}
        [only] => {
            table.insert(only.clone(), value);
        }
        [head, tail @ ..] => {
            let entry = table
                .entry(head.clone())
                .or_insert(toml::Value::Table(Default::default()));
            if let toml::Value::Table(sub) = entry {
                insert_nested(sub, tail, value);
            }
            // If entry existed and wasn't a table, silently drop — env shouldn't
            // change the shape of an already-set node. This is a noop (defensive).
        }
    }
}

fn validate(cfg: &Config) -> Result<(), ConfigError> {
    use crate::config::schema::is_safe_git_ident;
    if !is_safe_git_ident(&cfg.git.branch_prefix) {
        return Err(ConfigError::Validation(vec![format!(
            "git.branch_prefix contains disallowed characters: {:?} (allowed: A-Z a-z 0-9 / _ - .)",
            cfg.git.branch_prefix
        )]));
    }
    if !is_safe_git_ident(&cfg.git.base_branch) {
        return Err(ConfigError::Validation(vec![format!(
            "git.base_branch contains disallowed characters: {:?} (allowed: A-Z a-z 0-9 / _ - .)",
            cfg.git.base_branch
        )]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_toml_deep() {
        let mut base: toml::Value = toml::from_str(
            r#"
[a]
x = 1
y = 2
[b]
k = "base"
"#,
        )
        .unwrap();
        let overlay: toml::Value = toml::from_str(
            r#"
[a]
y = 99
z = 3
[b]
k = "overlay"
"#,
        )
        .unwrap();
        merge_toml(&mut base, overlay);
        let a = base.get("a").and_then(|v| v.as_table()).unwrap();
        assert_eq!(a.get("x").and_then(|v| v.as_integer()), Some(1));
        assert_eq!(a.get("y").and_then(|v| v.as_integer()), Some(99));
        assert_eq!(a.get("z").and_then(|v| v.as_integer()), Some(3));
        let b = base.get("b").and_then(|v| v.as_table()).unwrap();
        assert_eq!(b.get("k").and_then(|v| v.as_str()), Some("overlay"));
    }

    #[test]
    fn test_merge_toml_array_replaces() {
        let mut base: toml::Value = toml::from_str("arr = [1, 2, 3]").unwrap();
        let overlay: toml::Value = toml::from_str("arr = [9]").unwrap();
        merge_toml(&mut base, overlay);
        let arr = base.get("arr").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].as_integer(), Some(9));
    }

    #[test]
    fn test_merge_preserves_non_overlapping_and_overrides_overlapping() {
        let mut base: toml::Value = toml::from_str(
            r#"
[spawner]
poll_interval_secs = 10

[git]
branch_prefix = "feature"
"#,
        )
        .unwrap();
        let overlay: toml::Value = toml::from_str(
            r#"
[spawner]
poll_interval_secs = 20
"#,
        )
        .unwrap();
        merge_toml(&mut base, overlay);
        // binary-adjacent (overlay) wins
        assert_eq!(
            base.get("spawner")
                .unwrap()
                .get("poll_interval_secs")
                .unwrap()
                .as_integer(),
            Some(20)
        );
        // user-config (base) non-overlapping key survives
        assert_eq!(
            base.get("git")
                .unwrap()
                .get("branch_prefix")
                .unwrap()
                .as_str(),
            Some("feature")
        );
    }
}
