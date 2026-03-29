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
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_query() -> Result<()> {
        let db = StateDb::open_in_memory()?;

        assert!(!db.is_known("PROJ-1")?);

        db.insert_synced("PROJ-1", "Test ticket")?;

        assert!(db.is_known("PROJ-1")?);

        let ticket = db.get_ticket("PROJ-1")?.expect("ticket should exist");
        assert_eq!(ticket.key, "PROJ-1");
        assert_eq!(ticket.summary, "Test ticket");
        assert_eq!(ticket.status, "synced");
        assert!(ticket.plan_file.is_none());
        assert!(ticket.claimed_by.is_none());
        assert!(ticket.planned_at.is_none());
        assert!(ticket.spawned_at.is_none());
        assert!(ticket.completed_at.is_none());

        Ok(())
    }

    #[test]
    fn test_lifecycle_synced_to_done() -> Result<()> {
        let db = StateDb::open_in_memory()?;

        db.insert_synced("PROJ-2", "Lifecycle ticket")?;

        let t = db.get_ticket("PROJ-2")?.unwrap();
        assert_eq!(t.status, "synced");

        db.mark_planning("PROJ-2")?;
        let t = db.get_ticket("PROJ-2")?.unwrap();
        assert_eq!(t.status, "planning");

        db.mark_planned("PROJ-2", "/plans/PROJ-2.md")?;
        let t = db.get_ticket("PROJ-2")?.unwrap();
        assert_eq!(t.status, "planned");
        assert_eq!(t.plan_file.as_deref(), Some("/plans/PROJ-2.md"));
        assert!(t.planned_at.is_some());

        let claimed = db.claim_for_spawning("PROJ-2", 12345)?;
        assert!(claimed);
        let t = db.get_ticket("PROJ-2")?.unwrap();
        assert_eq!(t.status, "spawned");
        assert_eq!(t.claimed_by, Some(12345));
        assert!(t.spawned_at.is_some());

        db.mark_done("PROJ-2")?;
        let t = db.get_ticket("PROJ-2")?.unwrap();
        assert_eq!(t.status, "done");
        assert!(t.claimed_by.is_none());
        assert!(t.completed_at.is_some());

        Ok(())
    }

    #[test]
    fn test_claim_only_planned_tickets() -> Result<()> {
        let db = StateDb::open_in_memory()?;

        db.insert_synced("PROJ-3", "Unplanned ticket")?;

        // Ticket is in 'synced' state — claim should fail
        let claimed = db.claim_for_spawning("PROJ-3", 9999)?;
        assert!(!claimed);

        let t = db.get_ticket("PROJ-3")?.unwrap();
        assert_eq!(t.status, "synced");
        assert!(t.claimed_by.is_none());

        Ok(())
    }

    #[test]
    fn test_get_planned_tickets() -> Result<()> {
        let db = StateDb::open_in_memory()?;

        db.insert_synced("PROJ-10", "Synced ticket")?;
        db.insert_synced("PROJ-11", "Planned ticket A")?;
        db.insert_synced("PROJ-12", "Planned ticket B")?;
        db.insert_synced("PROJ-13", "Done ticket")?;

        db.mark_planning("PROJ-11")?;
        db.mark_planned("PROJ-11", "/plans/PROJ-11.md")?;

        db.mark_planning("PROJ-12")?;
        db.mark_planned("PROJ-12", "/plans/PROJ-12.md")?;

        db.mark_planning("PROJ-13")?;
        db.mark_planned("PROJ-13", "/plans/PROJ-13.md")?;
        db.claim_for_spawning("PROJ-13", 1)?;
        db.mark_done("PROJ-13")?;

        let planned = db.get_planned_tickets()?;
        assert_eq!(planned.len(), 2);

        let keys: Vec<&str> = planned.iter().map(|t| t.key.as_str()).collect();
        assert!(keys.contains(&"PROJ-11"));
        assert!(keys.contains(&"PROJ-12"));

        Ok(())
    }

    #[test]
    fn test_mark_failed() -> Result<()> {
        let db = StateDb::open_in_memory()?;

        db.insert_synced("PROJ-20", "Failing ticket")?;
        db.mark_planning("PROJ-20")?;
        db.mark_planned("PROJ-20", "/plans/PROJ-20.md")?;
        db.claim_for_spawning("PROJ-20", 42)?;

        db.mark_failed("PROJ-20")?;

        let t = db.get_ticket("PROJ-20")?.unwrap();
        assert_eq!(t.status, "failed");
        assert!(t.claimed_by.is_none());
        assert!(t.completed_at.is_some());

        Ok(())
    }

    #[test]
    fn test_duplicate_insert_fails() -> Result<()> {
        let db = StateDb::open_in_memory()?;

        db.insert_synced("PROJ-99", "First insert")?;

        let result = db.insert_synced("PROJ-99", "Duplicate insert");
        assert!(result.is_err(), "Duplicate insert should return an error");

        Ok(())
    }
}
