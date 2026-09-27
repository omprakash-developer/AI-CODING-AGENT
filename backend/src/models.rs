use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Planning,
    AwaitingApproval,
    Approved,
    Executing,
    Testing,
    Verifying,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub prompt: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProposal {
    pub summary: String,
    pub changes: Vec<FileChange>,
    pub verification: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub file: String,
    pub action: String,
    pub reason: String,
    pub content: String,
}

// =========================================================
// PERSISTENT AGENT MEMORY
// =========================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub content: String,
    pub created_at: String,
}

// =========================================================
// MEMORY SEARCH RESPONSE
// =========================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResponse {
    pub query: String,
    pub memories: Vec<Memory>,
}
