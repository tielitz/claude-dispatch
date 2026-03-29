use claude_dispatch::state::StateDb;
use rusqlite::Result;

fn open_test_db() -> StateDb {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("test.db");
    // Leak the tempdir so it isn't cleaned up while the DB is open
    let db = StateDb::open(&db_path).expect("open test db");
    std::mem::forget(dir);
    db
}

#[test]
fn test_insert_and_query() -> Result<()> {
    let db = open_test_db();

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
    let db = open_test_db();

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
    let db = open_test_db();

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
    let db = open_test_db();

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
    let db = open_test_db();

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
    let db = open_test_db();

    db.insert_synced("PROJ-99", "First insert")?;

    let result = db.insert_synced("PROJ-99", "Duplicate insert");
    assert!(result.is_err(), "Duplicate insert should return an error");

    Ok(())
}

// --- Security: SQL injection resistance (parameterized queries) ---

#[test]
fn test_sql_injection_in_key_is_harmless() -> Result<()> {
    let db = open_test_db();

    // Attempt SQL injection via ticket key — should be treated as a literal string
    let malicious_key = "'; DROP TABLE processed_tickets; --";
    db.insert_synced(malicious_key, "injection attempt")?;

    // Table should still exist and be functional
    let known = db.is_known(malicious_key)?;
    assert!(known, "malicious key should be stored as a literal string");

    // Other operations should still work
    db.insert_synced("PROJ-1", "normal ticket")?;
    assert!(db.is_known("PROJ-1")?);

    Ok(())
}

#[test]
fn test_sql_injection_in_summary_is_harmless() -> Result<()> {
    let db = open_test_db();

    let malicious_summary = "test'); DELETE FROM processed_tickets WHERE ('1'='1";
    db.insert_synced("PROJ-1", malicious_summary)?;

    let ticket = db.get_ticket("PROJ-1")?.expect("ticket should exist");
    assert_eq!(
        ticket.summary, malicious_summary,
        "summary should be stored literally"
    );

    Ok(())
}
