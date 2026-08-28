//! The append-only event log, and rebuilding state from it.
//!
//! **The log is the database.** Positions, cash and P&L are not stored — they
//! are a fold over these rows, recomputed by replaying them (`CLAUDE.md`
//! rule 1). Nothing here writes a projection, which is why nothing here can
//! write one that disagrees with the log.
//!
//! **Appends are idempotent.** `seq` is the primary key and inserts conflict
//! into a no-op, so re-appending a batch after a crash mid-write duplicates
//! nothing. A duplicated fill is a real position and real money.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod wire;

use std::path::Path;

use domain::DomainError;
use engine::Journaled;
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("stored event is not valid domain data: {0}")]
    Domain(#[from] DomainError),
}

/// The port the engine's owner depends on. Two methods, because that is all a
/// log is: something you add to, and something you read back in order.
pub trait EventLog {
    fn append(&mut self, entries: &[Journaled]) -> Result<(), StoreError>;
    fn read_all(&self) -> Result<Vec<Journaled>, StoreError>;

    fn last_seq(&self) -> Result<u64, StoreError> {
        Ok(self.read_all()?.last().map_or(0, |e| e.seq))
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
    seq     INTEGER PRIMARY KEY,
    at      INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    payload TEXT    NOT NULL
);
";

pub struct SqliteLog {
    conn: Connection,
}

impl SqliteLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::from_conn(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    fn from_conn(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }
}

impl EventLog for SqliteLog {
    /// One transaction per batch: a command's events land together or not at
    /// all. A partially-written command would replay into a state the engine
    /// never actually held.
    fn append(&mut self, entries: &[Journaled]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO events (seq, at, kind, payload) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(seq) DO NOTHING",
            )?;
            for entry in entries {
                stmt.execute((
                    entry.seq,
                    entry.at.as_millis(),
                    entry.event.kind(),
                    wire::encode(&entry.event)?,
                ))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<Journaled>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, at, payload FROM events ORDER BY seq")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (seq, at, payload) = row?;
            #[allow(clippy::cast_sign_loss)]
            out.push(wire::journaled(seq as u64, at, &payload)?);
        }
        Ok(out)
    }
}

/// An in-memory log for tests.
///
/// A **fake, not a mock** (`.claude/rust.md`): it behaves like the real thing,
/// including the idempotent append, rather than asserting which methods were
/// called. A test written against it is testing behaviour, so it survives a
/// refactor of the SQLite side.
#[derive(Debug, Default)]
pub struct InMemoryLog {
    entries: Vec<Journaled>,
}

impl InMemoryLog {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventLog for InMemoryLog {
    fn append(&mut self, entries: &[Journaled]) -> Result<(), StoreError> {
        for entry in entries {
            if !self.entries.iter().any(|e| e.seq == entry.seq) {
                self.entries.push(entry.clone());
            }
        }
        self.entries.sort_by_key(|e| e.seq);
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<Journaled>, StoreError> {
        Ok(self.entries.clone())
    }
}
