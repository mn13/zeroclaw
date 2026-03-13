use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

/// A single history row, matching the proto `HistoryEntry`.
#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub turn_index: u64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// SQLite-backed conversation history with interior mutability via `Mutex`.
///
/// Write operations obtain the lock, execute, and release immediately.
/// This is safe because all access is synchronous SQLite calls wrapped
/// in a blocking-friendly mutex.
pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    /// Open (or create) the history database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open history database")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                turn_index  INTEGER NOT NULL,
                role        TEXT    NOT NULL,
                content     TEXT    NOT NULL,
                created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_history_turn ON history (turn_index);",
        )
        .context("failed to initialise history schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory db")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                turn_index  INTEGER NOT NULL,
                role        TEXT    NOT NULL,
                content     TEXT    NOT NULL,
                created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_history_turn ON history (turn_index);",
        )
        .context("failed to initialise history schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Append a message to the history.
    pub fn append(&self, turn_index: u64, role: &str, content: &str) -> Result<()> {
        let conn = self.conn.lock().expect("history mutex poisoned");
        conn.execute(
            "INSERT INTO history (turn_index, role, content) VALUES (?1, ?2, ?3)",
            rusqlite::params![turn_index as i64, role, content],
        )
        .context("failed to insert history row")?;
        Ok(())
    }

    /// Load paginated history (ordered by id ascending).
    pub fn load(&self, offset: u64, limit: u64) -> Result<Vec<HistoryRow>> {
        let conn = self.conn.lock().expect("history mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT turn_index, role, content, created_at
                 FROM history ORDER BY id ASC LIMIT ?1 OFFSET ?2",
            )
            .context("failed to prepare load statement")?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64, offset as i64], |row| {
                Ok(HistoryRow {
                    turn_index: row.get::<_, i64>(0)? as u64,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .context("failed to query history")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to collect history rows")?;
        Ok(rows)
    }

    /// Return the total number of rows.
    pub fn count(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("history mutex poisoned");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .context("failed to count history")?;
        Ok(count as u64)
    }

    /// Load the most recent `n` messages (for context injection).
    pub fn load_recent(&self, n: u64) -> Result<Vec<HistoryRow>> {
        let conn = self.conn.lock().expect("history mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT turn_index, role, content, created_at
                 FROM history ORDER BY id DESC LIMIT ?1",
            )
            .context("failed to prepare recent-load statement")?;
        let mut rows = stmt
            .query_map(rusqlite::params![n as i64], |row| {
                Ok(HistoryRow {
                    turn_index: row.get::<_, i64>(0)? as u64,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .context("failed to query recent history")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to collect recent history rows")?;
        rows.reverse(); // oldest-first
        Ok(rows)
    }

    /// Delete all rows and return how many were removed.
    pub fn clear(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("history mutex poisoned");
        let deleted = conn
            .execute("DELETE FROM history", [])
            .context("failed to clear history")?;
        Ok(deleted as u64)
    }
}
