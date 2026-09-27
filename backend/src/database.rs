use rusqlite::{ffi::sqlite3_auto_extension, params, Connection};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

use crate::models::{Task, TaskProposal};

// =========================================================
// SQLITE-VEC REGISTRATION
// =========================================================

fn register_sqlite_vec() {
    static REGISTER: Once = Once::new();

    REGISTER.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

// =========================================================
// DATABASE
// =========================================================

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        // Register sqlite-vec before opening connections.
        register_sqlite_vec();

        let path = path.as_ref().to_path_buf();

        // Create the database directory if it does not exist.
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("Failed to create database directory: {}", error))?;
            }
        }

        let database = Self { path };

        // Create tables and indexes.
        database.initialize()?;

        Ok(database)
    }

    // =====================================================
    // CONNECTION
    // =====================================================

    fn open(&self) -> Result<Connection, String> {
        Connection::open(&self.path)
            .map_err(|error| format!("Failed to open SQLite database: {}", error))
    }

    // =====================================================
    // DATABASE INITIALIZATION
    // =====================================================

    fn initialize(&self) -> Result<(), String> {
        let connection = self.open()?;

        connection
            .execute_batch(
                r#"
                PRAGMA foreign_keys = ON;

                -- =========================================
                -- TASKS
                -- =========================================

                CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    prompt TEXT NOT NULL,
                    status TEXT NOT NULL,
                    created_at TEXT NOT NULL
                        DEFAULT CURRENT_TIMESTAMP
                );

                -- =========================================
                -- PROPOSALS
                -- =========================================

                CREATE TABLE IF NOT EXISTS proposals (
                    task_id TEXT PRIMARY KEY,
                    summary TEXT NOT NULL,
                    changes_json TEXT NOT NULL,
                    verification_json TEXT NOT NULL,
                    risks_json TEXT NOT NULL,

                    FOREIGN KEY(task_id)
                        REFERENCES tasks(id)
                        ON DELETE CASCADE
                );

                -- =========================================
                -- AGENT EVENTS
                -- =========================================

                CREATE TABLE IF NOT EXISTS events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    task_id TEXT NOT NULL,
                    event_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                        DEFAULT CURRENT_TIMESTAMP,

                    FOREIGN KEY(task_id)
                        REFERENCES tasks(id)
                        ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_events_task_id
                    ON events(task_id);

                -- =========================================
                -- PERSISTENT AGENT MEMORY
                -- =========================================

                CREATE TABLE IF NOT EXISTS memories (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    task_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL
                        DEFAULT CURRENT_TIMESTAMP,

                    FOREIGN KEY(task_id)
                        REFERENCES tasks(id)
                        ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_memories_task_id
                    ON memories(task_id);

                CREATE INDEX IF NOT EXISTS idx_memories_kind
                    ON memories(kind);

                -- =========================================
                -- SQLITE-VEC EMBEDDINGS
                -- =========================================
                --
                -- Gemini embedding-2 will use 768 dimensions.
                --
                -- The actual vector bytes will be inserted
                -- after Gemini generates the embedding.
                --
                -- rowid is linked to memories.id.
                -- =========================================

                CREATE VIRTUAL TABLE IF NOT EXISTS memory_embeddings
                USING vec0(
                    embedding float[768]
                );
                "#,
            )
            .map_err(|error| format!("Failed to initialize SQLite database: {}", error))?;

        Ok(())
    }

    // =====================================================
    // TASK METHODS
    // =====================================================

    pub fn save_task(&self, task: &Task) -> Result<(), String> {
        let connection = self.open()?;

        let status = serde_json::to_string(&task.status)
            .map_err(|error| format!("Failed to serialize task status: {}", error))?;

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO tasks
                    (id, prompt, status)
                VALUES
                    (?1, ?2, ?3)
                "#,
                params![task.id.to_string(), task.prompt, status.trim_matches('"')],
            )
            .map_err(|error| format!("Failed to save task: {}", error))?;

        Ok(())
    }

    // =====================================================
    // PROPOSAL METHODS
    // =====================================================

    pub fn save_proposal(&self, task_id: &str, proposal: &TaskProposal) -> Result<(), String> {
        let connection = self.open()?;

        let changes_json = serde_json::to_string(&proposal.changes)
            .map_err(|error| format!("Failed to serialize proposal changes: {}", error))?;

        let verification_json = serde_json::to_string(&proposal.verification)
            .map_err(|error| format!("Failed to serialize verification plan: {}", error))?;

        let risks_json = serde_json::to_string(&proposal.risks)
            .map_err(|error| format!("Failed to serialize risks: {}", error))?;

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO proposals (
                    task_id,
                    summary,
                    changes_json,
                    verification_json,
                    risks_json
                )
                VALUES
                    (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    task_id,
                    proposal.summary,
                    changes_json,
                    verification_json,
                    risks_json
                ],
            )
            .map_err(|error| format!("Failed to save task proposal: {}", error))?;

        Ok(())
    }

    // =====================================================
    // EVENT METHODS
    // =====================================================

    pub fn save_event(&self, task_id: &str, event_json: &str) -> Result<(), String> {
        let connection = self.open()?;

        connection
            .execute(
                r#"
                INSERT INTO events (
                    task_id,
                    event_json
                )
                VALUES
                    (?1, ?2)
                "#,
                params![task_id, event_json],
            )
            .map_err(|error| format!("Failed to save agent event: {}", error))?;

        Ok(())
    }

    // =====================================================
    // MEMORY METHODS
    // =====================================================

    /// Save persistent agent memory.
    ///
    /// Returns the newly created memory ID.
    ///
    /// `kind` examples:
    /// - "task"
    /// - "decision"
    /// - "solution"
    /// - "error"
    /// - "verification"
    /// - "user_preference"
    pub fn save_memory(&self, task_id: &str, kind: &str, content: &str) -> Result<i64, String> {
        let connection = self.open()?;

        connection
            .execute(
                r#"
                INSERT INTO memories (
                    task_id,
                    kind,
                    content
                )
                VALUES
                    (?1, ?2, ?3)
                "#,
                params![task_id, kind, content],
            )
            .map_err(|error| format!("Failed to save agent memory: {}", error))?;

        Ok(connection.last_insert_rowid())
    }

    // =====================================================
    // SQLITE-VEC VECTOR METHODS
    // =====================================================

    /// Save a 768-dimensional embedding for a memory.
    ///
    /// sqlite-vec stores vectors as raw bytes.
    pub fn save_memory_embedding(&self, memory_id: i64, embedding: &[f32]) -> Result<(), String> {
        if embedding.len() != 768 {
            return Err(format!(
                "Invalid embedding dimension: expected 768, got {}",
                embedding.len()
            ));
        }

        let connection = self.open()?;

        let vector_bytes = f32_slice_as_bytes(embedding);

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO memory_embeddings (
                    rowid,
                    embedding
                )
                VALUES (?1, ?2)
                "#,
                params![memory_id, vector_bytes],
            )
            .map_err(|error| format!("Failed to save memory embedding: {}", error))?;

        Ok(())
    }

    /// Search memories using sqlite-vec nearest-neighbor search.
    pub fn search_memory_vectors(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        if embedding.len() != 768 {
            return Err(format!(
                "Invalid query embedding dimension: expected 768, got {}",
                embedding.len()
            ));
        }

        if limit == 0 {
            return Ok(Vec::new());
        }

        let connection = self.open()?;

        let vector_bytes = f32_slice_as_bytes(embedding);

        let mut statement = connection
            .prepare(
                r#"
                SELECT
                    m.id,
                    m.task_id,
                    m.kind,
                    m.content,
                    m.created_at,
                    e.distance
                FROM memory_embeddings AS e
                JOIN memories AS m
                    ON m.id = e.rowid
                WHERE e.embedding MATCH ?1
                  AND k = ?2
                ORDER BY e.distance
                "#,
            )
            .map_err(|error| format!("Failed to prepare vector memory search: {}", error))?;

        let rows = statement
            .query_map(params![vector_bytes, limit as i64], |row| {
                Ok(MemorySearchResult {
                    memory: MemoryRecord {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        kind: row.get(2)?,
                        content: row.get(3)?,
                        created_at: row.get(4)?,
                    },
                    distance: row.get(5)?,
                })
            })
            .map_err(|error| format!("Failed to execute vector memory search: {}", error))?;

        let mut results = Vec::new();

        for row in rows {
            let result =
                row.map_err(|error| format!("Failed to read vector search result: {}", error))?;

            results.push(result);
        }

        Ok(results)
    }

    // =====================================================
    // MEMORY TEXT SEARCH
    // =====================================================

    /// Search persistent memories using SQLite LIKE matching.
    ///
    /// This remains available as a fallback.
    /// Semantic vector search is provided by
    /// `search_memory_vectors()`.
    pub fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>, String> {
        let connection = self.open()?;

        let search_pattern = format!("%{}%", query);

        let mut statement = connection
            .prepare(
                r#"
                SELECT
                    id,
                    task_id,
                    kind,
                    content,
                    created_at
                FROM memories
                WHERE content LIKE ?1
                   OR kind LIKE ?1
                ORDER BY id DESC
                LIMIT ?2
                "#,
            )
            .map_err(|error| format!("Failed to prepare memory search: {}", error))?;

        let rows = statement
            .query_map(params![search_pattern, limit as i64], |row| {
                Ok(MemoryRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    kind: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|error| format!("Failed to search memories: {}", error))?;

        let mut memories = Vec::new();

        for row in rows {
            let memory = row.map_err(|error| format!("Failed to read memory result: {}", error))?;

            memories.push(memory);
        }

        Ok(memories)
    }

    // =====================================================
    // TASK MEMORY
    // =====================================================

    /// Get all memories belonging to a specific task.
    pub fn get_task_memories(&self, task_id: &str) -> Result<Vec<MemoryRecord>, String> {
        let connection = self.open()?;

        let mut statement = connection
            .prepare(
                r#"
                SELECT
                    id,
                    task_id,
                    kind,
                    content,
                    created_at
                FROM memories
                WHERE task_id = ?1
                ORDER BY id ASC
                "#,
            )
            .map_err(|error| format!("Failed to prepare task memory query: {}", error))?;

        let rows = statement
            .query_map(params![task_id], |row| {
                Ok(MemoryRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    kind: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|error| format!("Failed to query task memories: {}", error))?;

        let mut memories = Vec::new();

        for row in rows {
            let memory = row.map_err(|error| format!("Failed to read task memory: {}", error))?;

            memories.push(memory);
        }

        Ok(memories)
    }

    // =====================================================
    // DATABASE PATH
    // =====================================================

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// =========================================================
// SQLITE-VEC BYTE CONVERSION
// =========================================================

/// Convert a slice of f32 values into the byte representation
/// expected by sqlite-vec.
///
/// The returned bytes are only used while the SQLite statement
/// is executing.
fn f32_slice_as_bytes(values: &[f32]) -> Vec<u8> {
    let byte_length = values.len() * std::mem::size_of::<f32>();

    let pointer = values.as_ptr() as *const u8;

    unsafe { std::slice::from_raw_parts(pointer, byte_length).to_vec() }
}

// =========================================================
// MEMORY RECORD
// =========================================================

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecord {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub content: String,
    pub created_at: String,
}

// =========================================================
// MEMORY SEARCH RESULT
// =========================================================

#[derive(Debug, Clone, Serialize)]
pub struct MemorySearchResult {
    pub memory: MemoryRecord,
    pub distance: f64,
}
