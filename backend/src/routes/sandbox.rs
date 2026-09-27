use axum::{extract::State, http::StatusCode, Json};

use serde::{Deserialize, Serialize};

use crate::{
    gemini::client::GeminiClient,
    revision::r#loop::RevisionLoop,
    sandbox::docker::DockerSandbox,
    state::AppState,
    tools::workspace::Workspace,
    verification::runner::{VerificationResult, VerificationRunner},
};

#[derive(Debug, Deserialize)]
pub struct SandboxRunRequest {
    pub command: String,
}

#[derive(Debug, Serialize)]
pub struct SandboxRunResponse {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationResponse {
    pub passed: bool,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Deserialize)]
pub struct RevisionRequest {
    pub task: String,
    pub command: String,
    pub current_code: String,
    pub max_attempts: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct RevisionResponse {
    pub success: bool,
    pub attempts: usize,
}

// ============================================================
// TEST DOCKER SANDBOX
// ============================================================

pub async fn test_sandbox(
    State(_state): State<AppState>,
) -> Result<Json<SandboxRunResponse>, (StatusCode, String)> {
    let sandbox = DockerSandbox::new();

    // Create/access the real workspace.
    let workspace = Workspace::new().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize workspace: {}", error),
        )
    })?;

    let result = sandbox
        .run(workspace.root(), "echo 'AI Coding Agent Sandbox'")
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(SandboxRunResponse {
        success: result.success,
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
    }))
}

// ============================================================
// RUN DOCKER SANDBOX
// ============================================================

pub async fn run_sandbox(
    State(_state): State<AppState>,
    Json(request): Json<SandboxRunRequest>,
) -> Result<Json<SandboxRunResponse>, (StatusCode, String)> {
    if request.command.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Command cannot be empty.".to_string(),
        ));
    }

    let sandbox = DockerSandbox::new();

    // Create/access the real workspace.
    let workspace = Workspace::new().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize workspace: {}", error),
        )
    })?;

    let result = sandbox
        .run(workspace.root(), &request.command)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(SandboxRunResponse {
        success: result.success,
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
    }))
}

// ============================================================
// RUN VERIFICATION
// ============================================================

pub async fn run_verification(
    State(_state): State<AppState>,
    Json(request): Json<SandboxRunRequest>,
) -> Result<Json<VerificationResponse>, (StatusCode, String)> {
    if request.command.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Verification command cannot be empty.".to_string(),
        ));
    }

    let runner = VerificationRunner::new();

    // Create/access the real workspace.
    let workspace = Workspace::new().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize workspace: {}", error),
        )
    })?;

    let result: VerificationResult = runner
        .verify(workspace.root(), &request.command)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(VerificationResponse {
        passed: result.passed,
        command: result.command,
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
    }))
}

// ============================================================
// RUN REVISION LOOP
// ============================================================

pub async fn run_revision_loop(
    State(_state): State<AppState>,
    Json(request): Json<RevisionRequest>,
) -> Result<Json<RevisionResponse>, (StatusCode, String)> {
    // --------------------------------------------------------
    // Validate task
    // --------------------------------------------------------

    if request.task.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Task cannot be empty.".to_string()));
    }

    // --------------------------------------------------------
    // Validate verification command
    // --------------------------------------------------------

    if request.command.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Verification command cannot be empty.".to_string(),
        ));
    }

    // --------------------------------------------------------
    // Validate current code
    // --------------------------------------------------------

    if request.current_code.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Current code cannot be empty.".to_string(),
        ));
    }

    // --------------------------------------------------------
    // Validate maximum attempts
    // --------------------------------------------------------

    let max_attempts = request.max_attempts.unwrap_or(3);

    if max_attempts == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "max_attempts must be greater than 0.".to_string(),
        ));
    }

    // --------------------------------------------------------
    // Create Gemini client
    // --------------------------------------------------------

    let gemini = GeminiClient::new().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize Gemini client: {}", error),
        )
    })?;

    // --------------------------------------------------------
    // Create revision loop
    // --------------------------------------------------------

    let revision_loop = RevisionLoop::new(max_attempts, gemini);

    // --------------------------------------------------------
    // Run revision loop
    // --------------------------------------------------------

    let result = revision_loop
        .run(&request.task, &request.command, &request.current_code)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    // --------------------------------------------------------
    // Return result
    // --------------------------------------------------------

    Ok(Json(RevisionResponse {
        success: result.success,
        attempts: result.attempts.len(),
    }))
}
