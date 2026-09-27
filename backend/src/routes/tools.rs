use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};

use serde::{Deserialize, Serialize};

use serde_json::json;
use uuid::Uuid;

use crate::{
    state::AppState,
    tools::{git::GitTool, workspace::Workspace},
};

// ==================================================
// FILE QUERY
// ==================================================

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub path: String,
}

// ==================================================
// WRITE FILE REQUEST
// ==================================================

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub task_id: Uuid,
    pub path: String,
    pub content: String,
}

// ==================================================
// WRITE FILE RESPONSE
// ==================================================

#[derive(Debug, Serialize)]
pub struct WriteFileResponse {
    pub success: bool,
    pub path: String,
}

// ==================================================
// LIST FILES
//
// GET /tools/files
// ==================================================

pub async fn list_files(State(_state): State<AppState>) -> impl IntoResponse {
    let workspace = match Workspace::new() {
        Ok(workspace) => workspace,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    match workspace.list_files("").await {
        Ok(files) => (
            StatusCode::OK,
            Json(json!({
                "files": files
            })),
        )
            .into_response(),

        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error
            })),
        )
            .into_response(),
    }
}

// ==================================================
// READ FILE
//
// GET /tools/file?path=...
// ==================================================

pub async fn read_file(
    State(_state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> impl IntoResponse {
    // --------------------------------------------------
    // Create workspace
    // --------------------------------------------------

    let workspace = match Workspace::new() {
        Ok(workspace) => workspace,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    // --------------------------------------------------
    // Block sensitive files
    // --------------------------------------------------

    if Workspace::is_sensitive_path(&query.path) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error":
                    "Sensitive files cannot be read."
            })),
        )
            .into_response();
    }

    // --------------------------------------------------
    // Read file
    // --------------------------------------------------

    match workspace.read_file(&query.path).await {
        Ok(content) => (
            StatusCode::OK,
            Json(json!({
                "path": query.path,
                "content": content
            })),
        )
            .into_response(),

        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error
            })),
        )
            .into_response(),
    }
}

// ==================================================
// WRITE FILE
//
// POST /tools/file
//
// IMPORTANT:
// File modification requires approval.
// ==================================================

pub async fn write_file(
    State(state): State<AppState>,
    Json(payload): Json<WriteFileRequest>,
) -> impl IntoResponse {
    // --------------------------------------------------
    // Check approval
    // --------------------------------------------------

    let approved = {
        let approved_tasks = state.approved_tasks.read().await;

        approved_tasks.contains(&payload.task_id)
    };

    if !approved {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error":
                    "Task has not been approved."
            })),
        )
            .into_response();
    }

    // --------------------------------------------------
    // Block sensitive files
    // --------------------------------------------------

    if Workspace::is_sensitive_path(&payload.path) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error":
                    "Sensitive files cannot be modified."
            })),
        )
            .into_response();
    }

    // --------------------------------------------------
    // Create workspace
    // --------------------------------------------------

    let workspace = match Workspace::new() {
        Ok(workspace) => workspace,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    // --------------------------------------------------
    // Validate workspace path
    // --------------------------------------------------

    if let Err(error) = workspace.resolve_path(&payload.path) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error
            })),
        )
            .into_response();
    }

    // --------------------------------------------------
    // Write file
    // --------------------------------------------------

    match workspace.write_file(&payload.path, &payload.content).await {
        Ok(_) => (
            StatusCode::OK,
            Json(WriteFileResponse {
                success: true,
                path: payload.path,
            }),
        )
            .into_response(),

        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error
            })),
        )
            .into_response(),
    }
}

// ==================================================
// GIT STATUS
//
// GET /tools/git/status
// ==================================================

pub async fn git_status(State(_state): State<AppState>) -> impl IntoResponse {
    let workspace = match Workspace::new() {
        Ok(workspace) => workspace,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    let git = match GitTool::new(&workspace) {
        Ok(git) => git,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    match git.status().await {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({
                "status": status
            })),
        )
            .into_response(),

        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error
            })),
        )
            .into_response(),
    }
}

// ==================================================
// GIT DIFF
//
// GET /tools/git/diff
// ==================================================

pub async fn git_diff(State(_state): State<AppState>) -> impl IntoResponse {
    let workspace = match Workspace::new() {
        Ok(workspace) => workspace,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    let git = match GitTool::new(&workspace) {
        Ok(git) => git,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    match git.diff().await {
        Ok(diff) => (
            StatusCode::OK,
            Json(json!({
                "diff": diff
            })),
        )
            .into_response(),

        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error
            })),
        )
            .into_response(),
    }
}

// ==================================================
// GIT BRANCH
//
// GET /tools/git/branch
// ==================================================

pub async fn git_branch(State(_state): State<AppState>) -> impl IntoResponse {
    let workspace = match Workspace::new() {
        Ok(workspace) => workspace,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    let git = match GitTool::new(&workspace) {
        Ok(git) => git,

        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error
                })),
            )
                .into_response();
        }
    };

    match git.current_branch().await {
        Ok(branch) => (
            StatusCode::OK,
            Json(json!({
                "branch": branch
            })),
        )
            .into_response(),

        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error
            })),
        )
            .into_response(),
    }
}
