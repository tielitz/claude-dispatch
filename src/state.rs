use std::path::Path;

use chrono::Utc;
use rusqlite::{Connection, Result, params};

#[derive(Debug, Clone)]
#[allow(dead_code)]
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

pub struct StateDb {
    conn: Connection,
}

impl StateDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    pub fn init(&self) -> Result<()> {
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
            );",
        )?;
        Ok(())
    }

    pub fn is_known(&self, key: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM processed_tickets WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn insert_synced(&self, key: &str, summary: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO processed_tickets (key, summary, status, synced_at) VALUES (?1, ?2, 'synced', ?3)",
            params![key, summary, now],
        )?;
        Ok(())
    }

    pub fn mark_planning(&self, key: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE processed_tickets SET status = 'planning' WHERE key = ?1",
            params![key],
        )?;
        Ok(())
    }

    pub fn mark_planned(&self, key: &str, plan_file: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE processed_tickets SET status = 'planned', plan_file = ?2, planned_at = ?3 WHERE key = ?1",
            params![key, plan_file, now],
        )?;
        Ok(())
    }

    /// Atomically claim a ticket for spawning. Only succeeds if status='planned'.
    /// Returns true if the row was updated (i.e., claim was successful).
    pub fn claim_for_spawning(&self, key: &str, pid: i64) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let rows_changed = self.conn.execute(
            "UPDATE processed_tickets SET status = 'spawned', claimed_by = ?2, spawned_at = ?3 WHERE key = ?1 AND status = 'planned'",
            params![key, pid, now],
        )?;
        Ok(rows_changed > 0)
    }

    pub fn mark_done(&self, key: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE processed_tickets SET status = 'done', completed_at = ?2, claimed_by = NULL WHERE key = ?1",
            params![key, now],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, key: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE processed_tickets SET status = 'failed', completed_at = ?2, claimed_by = NULL WHERE key = ?1",
            params![key, now],
        )?;
        Ok(())
    }

    pub fn get_planned_tickets(&self) -> Result<Vec<TicketRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, summary, status, plan_file, claimed_by, synced_at, planned_at, spawned_at, completed_at
             FROM processed_tickets WHERE status = 'planned'",
        )?;
        let records = stmt
            .query_map([], |row| {
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
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(records)
    }

    #[allow(dead_code)]
    pub fn get_ticket(&self, key: &str) -> Result<Option<TicketRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, summary, status, plan_file, claimed_by, synced_at, planned_at, spawned_at, completed_at
             FROM processed_tickets WHERE key = ?1",
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
            Some(record) => Ok(Some(record?)),
            None => Ok(None),
        }
    }
}

// Unit tests for internal database hardening (access private `conn` field).
// Public API tests are in tests/state.rs (integration tests).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wal_mode_enabled() -> Result<()> {
        let db = StateDb::open_in_memory()?;
        let mode: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        // In-memory databases may report "memory" instead of "wal",
        // so test with a file-based database for a definitive check.
        // For in-memory, we just verify the pragma doesn't error.
        assert!(
            mode == "wal" || mode == "memory",
            "journal_mode should be wal (or memory for in-memory db), got: {}",
            mode
        );
        Ok(())
    }

    #[test]
    fn test_busy_timeout_is_set() -> Result<()> {
        let db = StateDb::open_in_memory()?;
        let timeout: i64 = db
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
        assert_eq!(
            timeout, 5000,
            "busy_timeout should be 5000ms to handle concurrent access"
        );
        Ok(())
    }

    #[test]
    fn test_wal_and_busy_timeout_on_file_db() -> Result<()> {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("test.db");
        let db = StateDb::open(&db_path)?;

        let mode: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!(mode, "wal", "file-backed db should use WAL mode");

        let timeout: i64 = db
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
        assert_eq!(timeout, 5000);

        Ok(())
    }
}
