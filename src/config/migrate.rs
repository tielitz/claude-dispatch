//! Migrations for `schema_version`.
//!
//! ## Simple rename (e.g. `fetch_limit` -> `fetch_batch_size`)
//! 1. Add `#[serde(alias = "fetch_limit")]` on the new field.
//! 2. Add a key-scan in `warn_deprecated_keys` that emits `tracing::warn!`.
//!
//! ## Structural change (e.g. split `paths.state_dir` into `[state] dir = ...`)
//! 1. Bump `CURRENT_VERSION`.
//! 2. Add to `SUPPORTED_VERSIONS`.
//! 3. Write `fn migrate_N_to_M(doc: &mut toml::Value) -> Result<(), ConfigError>`.
//! 4. In `run`, dispatch on `version` and chain migrations up to `CURRENT_VERSION`:
//!
//! ```ignore
//! let mut v = version;
//! while v < CURRENT_VERSION {
//!     match v {
//!         1 => migrate_1_to_2(doc)?,
//!         _ => return Err(ConfigError::UnknownSchemaVersion {
//!             found: v, supported: SUPPORTED_VERSIONS,
//!         }),
//!     }
//!     v += 1;
//! }
//! ```

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

    // No real migrations exist yet — the `SUPPORTED_VERSIONS` check above
    // already guarantees `version == CURRENT_VERSION`. The dispatch loop
    // (see module-level docs) lands here once the first structural change
    // bumps `CURRENT_VERSION`.

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
