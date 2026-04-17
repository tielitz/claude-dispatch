//! Migrations for `schema_version`.
//!
//! ## Simple rename (e.g. `fetch_limit` -> `fetch_batch_size`)
//! 1. Add `#[serde(alias = "fetch_limit")]` on the new field.
//! 2. Add a key-scan in `warn_deprecated_keys` that emits `tracing::warn!`.
//!
//! ## Structural change (e.g. split `paths.state_dir` into `[state] dir = ...`)
//! 1. Bump `CURRENT_VERSION`.
//! 2. Add to `SUPPORTED_VERSIONS`.
//! 3. Write `fn migrate_N_to_M(doc: &mut toml::Value)`.
//! 4. Wire into the `while v < CURRENT_VERSION` loop below.

use crate::config::error::ConfigError;

pub const SUPPORTED_VERSIONS: &[u32] = &[1];
pub const CURRENT_VERSION: u32 = 1;

pub fn run(doc: &mut toml::Value, version: u32) -> Result<(), ConfigError> {
    if !SUPPORTED_VERSIONS.contains(&version) {
        return Err(ConfigError::UnknownSchemaVersion {
            found: version,
            supported: SUPPORTED_VERSIONS,
        });
    }
    warn_deprecated_keys(doc);

    // The three `#[allow]` attributes below are only necessary while
    // `SUPPORTED_VERSIONS` has a single entry and no real migration bodies
    // exist. Remove them when the first structural migration lands.
    #[allow(unused_mut)]
    let mut v = version;
    #[allow(clippy::never_loop)]
    while v < CURRENT_VERSION {
        // Future migrations dispatch on v here:
        //   1 => migrate_1_to_2(doc)?,
        //   2 => migrate_2_to_3(doc)?,
        //   _ => unreachable!("no migration defined for version {v}"),
        unreachable!("no migration defined for version {v}");
        #[allow(unreachable_code)]
        {
            v += 1;
        }
    }

    if let Some(t) = doc.as_table_mut() {
        t.insert(
            "schema_version".into(),
            toml::Value::Integer(CURRENT_VERSION as i64),
        );
    }
    Ok(())
}

fn warn_deprecated_keys(_doc: &toml::Value) {
    // Placeholder. When fields are renamed, scan for old key and tracing::warn!.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_run_noop_at_current_version() {
        let mut doc: toml::Value = toml::from_str("foo = 1").unwrap();
        run(&mut doc, 1).expect("noop at v1");
        // schema_version should be stamped on
        let t = doc.as_table().unwrap();
        assert_eq!(
            t.get("schema_version").and_then(|v| v.as_integer()),
            Some(1)
        );
    }
}
