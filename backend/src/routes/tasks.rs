use std::{collections::HashMap, path::Path, sync::OnceLock};

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use uuid::Uuid;

use crate::{
    agent::events::AgentEvent,
    gemini::{client::GeminiClient, GeminiEmbeddingClient},
    models::{CreateTaskRequest, FileChange, Task, TaskProposal, TaskStatus},
    state::AppState,
    tools::{git::GitTool, workspace::Workspace},
    verification::runner::VerificationRunner,
};

// ============================================================
// IN-MEMORY EVENT HISTORY (SSE REPLAY BUFFER)
// ============================================================

static EVENT_HISTORY: OnceLock<RwLock<HashMap<Uuid, Vec<String>>>> = OnceLock::new();

fn event_history() -> &'static RwLock<HashMap<Uuid, Vec<String>>> {
    EVENT_HISTORY.get_or_init(|| RwLock::new(HashMap::new()))
}

// ============================================================
// REQUEST & RESPONSE DTOs
// ============================================================

#[derive(Debug, Serialize)]
pub struct CreateTaskResponse {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: TaskStatus,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ApproveTaskResponse {
    pub task_id: Uuid,
    pub approved: bool,
    pub message: String,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct StreamQuery {
    pub token: Option<String>,
}

// ============================================================
// EVENT BROADCAST & PERSISTENCE HELPER
// ============================================================

async fn send_event(state: &AppState, task_id: Uuid, event: AgentEvent) {
    let message = match serde_json::to_string(&event) {
        Ok(json) => json,
        Err(error) => {
            eprintln!(
                "Failed to serialize agent event for task {}: {}",
                task_id, error
            );
            return;
        }
    };

    // 1. Store in EVENT_HISTORY (capped at 100 events per task)
    {
        let mut history = event_history().write().await;
        let events = history.entry(task_id).or_default();
        if events.len() >= 100 {
            events.remove(0);
        }
        events.push(message.clone());
    }

    // 2. Persist event to SQLite database
    if let Err(error) = state.database.save_event(&task_id.to_string(), &message) {
        eprintln!(
            "Failed to save event to database for task {}: {}",
            task_id, error
        );
    }

    // 3. Broadcast to active SSE subscribers
    let events = state.events.read().await;
    if let Some(sender) = events.get(&task_id) {
        let _ = sender.send(message);
    }
}

// ============================================================
// TASK STATUS HELPER
// ============================================================

async fn update_task_status(
    state: &AppState,
    task_id: Uuid,
    status: TaskStatus,
) -> Result<(), String> {
    let mut tasks = state.tasks.write().await;
    if let Some(task) = tasks.get_mut(&task_id) {
        task.status = status;
        let task_clone = task.clone();
        drop(tasks);
        state.database.save_task(&task_clone)?;
    } else {
        return Err(format!("Task {} not found in memory state", task_id));
    }
    Ok(())
}

// ============================================================
// POST /tasks
// ============================================================

pub async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<CreateTaskResponse>), (StatusCode, Json<serde_json::Value>)> {
    let prompt = request.prompt.trim().to_string();

    if prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Task prompt cannot be empty."
            })),
        ));
    }

    let task_id = Uuid::new_v4();

    let task = Task {
        id: task_id,
        prompt: prompt.clone(),
        status: TaskStatus::Planning,
    };

    // Save task to SQLite BEFORE saving anything that depends on task_id
    if let Err(error) = state.database.save_task(&task) {
        eprintln!("Failed to save task to database: {}", error);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to persist task: {}", error)
            })),
        ));
    }

    // Store task in memory
    state.tasks.write().await.insert(task_id, task.clone());

    // Create broadcast channel for live SSE streaming
    let (sender, _) = tokio::sync::broadcast::channel(100);
    state.events.write().await.insert(task_id, sender);

    // Save task prompt as persistent memory and generate vector embedding if available
    let state_mem = state.clone();
    let prompt_mem = prompt.clone();
    tokio::spawn(async move {
        match state_mem
            .database
            .save_memory(&task_id.to_string(), "task", &prompt_mem)
        {
            Ok(memory_id) => {
                if let Ok(embedding_client) = GeminiEmbeddingClient::new() {
                    match embedding_client.embed_document(&prompt_mem).await {
                        Ok(embedding) => {
                            if let Err(err) = state_mem
                                .database
                                .save_memory_embedding(memory_id, &embedding)
                            {
                                eprintln!(
                                    "Failed to save memory embedding for task {}: {}",
                                    task_id, err
                                );
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "Failed to generate embedding for task prompt {}: {}",
                                task_id, err
                            );
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!(
                    "Failed to save initial task memory for task {}: {}",
                    task_id, err
                );
            }
        }
    });

    // Start asynchronous Gemini task analysis flow
    let state_analysis = state.clone();
    let prompt_analysis = prompt.clone();
    tokio::spawn(async move {
        run_task_analysis(state_analysis, task_id, prompt_analysis).await;
    });

    Ok((
        StatusCode::CREATED,
        Json(CreateTaskResponse {
            id: task_id,
            task_id,
            status: TaskStatus::Planning,
            message: "Task created successfully. Gemini analysis has started.".to_string(),
        }),
    ))
}

// ============================================================
// GET /tasks/{id}
// ============================================================

pub async fn get_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<Task>, StatusCode> {
    let tasks = state.tasks.read().await;
    match tasks.get(&id) {
        Some(task) => Ok(Json(task.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

// ============================================================
// GET /tasks/{id}/stream
// ============================================================

pub async fn stream_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
    Query(_query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::Error>>>, StatusCode> {
    let task_exists = {
        let tasks = state.tasks.read().await;
        tasks.contains_key(&id)
    };

    if !task_exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let sender = {
        let mut events = state.events.write().await;
        events
            .entry(id)
            .or_insert_with(|| {
                let (tx, _) = tokio::sync::broadcast::channel(100);
                tx
            })
            .clone()
    };

    let receiver = sender.subscribe();

    // Fetch in-memory event history to replay missed events
    let history_messages = {
        let history = event_history().read().await;
        history.get(&id).cloned().unwrap_or_default()
    };

    let history_stream = tokio_stream::iter(
        history_messages
            .into_iter()
            .map(|msg| Ok::<Event, axum::Error>(Event::default().data(msg))),
    );

    let live_stream = BroadcastStream::new(receiver).filter_map(|msg| match msg {
        Ok(message) => Some(Ok(Event::default().data(message))),
        Err(_) => None,
    });

    let stream = history_stream.chain(live_stream);

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ============================================================
// POST /tasks/{id}/approve
// ============================================================

pub async fn approve_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<ApproveTaskResponse>, (StatusCode, Json<serde_json::Value>)> {
    let task_exists = {
        let tasks = state.tasks.read().await;
        tasks.contains_key(&id)
    };

    if !task_exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("Task {} was not found.", id)
            })),
        ));
    }

    let proposal_exists = {
        let proposals = state.proposals.read().await;
        proposals.contains_key(&id)
    };

    if !proposal_exists {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Task {} does not have a ready proposal to approve.", id)
            })),
        ));
    }

    // Record developer approval
    {
        let mut approved = state.approved_tasks.write().await;
        approved.insert(id);
    }

    if let Err(error) = update_task_status(&state, id, TaskStatus::Approved).await {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to update task status: {}", error)
            })),
        ));
    }

    send_event(
        &state,
        id,
        AgentEvent::plan("Task proposal approved by developer. Applying authorized changes..."),
    )
    .await;

    // Asynchronously apply file changes and run verification
    let state_exec = state.clone();
    tokio::spawn(async move {
        execute_approved_task(state_exec, id).await;
    });

    Ok(Json(ApproveTaskResponse {
        task_id: id,
        approved: true,
        message: "Task approved. File modifications are authorized and execution has begun."
            .to_string(),
    }))
}

// ============================================================
// GEMINI TASK ANALYSIS FLOW
// ============================================================

async fn run_task_analysis(state: AppState, task_id: Uuid, prompt: String) {
    send_event(
        &state,
        task_id,
        AgentEvent::plan(format!("Task received: {}", prompt)),
    )
    .await;

    send_event(
        &state,
        task_id,
        AgentEvent::plan("Gemini task analysis has started."),
    )
    .await;

    // 1. Initialize Workspace
    let workspace = match Workspace::new() {
        Ok(ws) => ws,
        Err(err) => {
            let error_msg = format!("Workspace initialization failed: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_msg);
            return;
        }
    };

    send_event(
        &state,
        task_id,
        AgentEvent::plan("Workspace initialized successfully."),
    )
    .await;

    // 2. Initialize Git Tool & collect git diff
    let git = GitTool::new(&workspace).ok();
    let mut current_diff = String::new();

    if let Some(ref git_tool) = git {
        match git_tool.status().await {
            Ok(status) if !status.trim().is_empty() => {
                send_event(
                    &state,
                    task_id,
                    AgentEvent::plan(format!("Git status checked:\n{}", status)),
                )
                .await;
            }
            _ => {}
        }

        if let Ok(diff) = git_tool.diff().await {
            current_diff = diff;
        }
    }

    // 3. List workspace files
    let files = match workspace.list_files("").await {
        Ok(f) => f,
        Err(err) => {
            let error_msg = format!("Failed to list workspace files: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_msg);
            return;
        }
    };

    send_event(
        &state,
        task_id,
        AgentEvent::plan(format!(
            "Workspace contains {} accessible files.",
            files.len()
        )),
    )
    .await;

    // 4. Select and read context files
    let priority_files = select_context_files(&files);
    send_event(
        &state,
        task_id,
        AgentEvent::plan(format!(
            "Selected {} files for context.",
            priority_files.len()
        )),
    )
    .await;

    let mut workspace_context = String::new();
    for file in priority_files {
        match workspace.read_file(&file).await {
            Ok(content) => {
                workspace_context.push_str(&format!("\n\n===== FILE: {} =====\n{}", file, content));
            }
            Err(err) => {
                send_event(
                    &state,
                    task_id,
                    AgentEvent::plan(format!("Skipped unreadable file {}: {}", file, err)),
                )
                .await;
            }
        }
    }

    // 5. Memory / RAG Context Retrieval
    let memory_context = retrieve_memory_context(&state, &prompt).await;
    if !memory_context.is_empty() {
        send_event(
            &state,
            task_id,
            AgentEvent::plan("Retrieved relevant memory context from previous sessions."),
        )
        .await;
    }

    // 6. Initialize Gemini Client
    let gemini = match GeminiClient::new() {
        Ok(client) => client,
        Err(err) => {
            let error_msg = format!("Failed to initialize Gemini client: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_msg);
            return;
        }
    };

    let gemini_prompt = format!(
        r#"You are the coding agent inside an AI Coding Agent project.

The developer gave this task:

TASK:
{prompt}

{memory_context}

WORKSPACE CONTEXT:
{workspace_context}

CURRENT GIT DIFF:
{current_diff}

Analyze the task carefully and produce a structured implementation proposal.

IMPORTANT RULES:
1. Do NOT invent unnecessary files.
2. Prefer modifying existing files when appropriate.
3. Use relative file paths only.
4. Never use absolute paths.
5. Never use ".." path traversal.
6. Never modify:
   - .env
   - secrets
   - credentials
   - private keys
   - certificates
7. Every proposed file must contain complete file content.
8. Do not claim that files have already been changed.
9. This is a proposal for human approval.
10. Keep changes focused on the developer's task.
11. Include verification commands or checks.
    VERIFICATION COMMAND RULES:
    - Verification commands must be fully non-interactive.
    - Never generate a verification command that waits for keyboard input.
    - If the generated program requires stdin, provide test input using printf, echo, or an equivalent pipe.
      For example, use:
      g++ odd_even.cpp -o odd_even && printf "5\n" | ./odd_even
      instead of:
      g++ odd_even.cpp -o odd_even && ./odd_even
    - Verification commands must be executable automatically inside the Docker sandbox.
    - Choose simple deterministic test inputs that exercise the requested behavior.
12. Supported file actions are only "create" and "modify".

Return ONLY valid JSON.

Required format:
{{
  "summary": "short explanation",
  "changes": [
    {{
      "file": "relative/path/to/file",
      "action": "modify",
      "reason": "why this file changes",
      "content": "complete file content"
    }}
  ],
  "verification": [
    "command or verification step"
  ],
  "risks": [
    "possible risk"
  ]
}}
"#
    );

    send_event(
        &state,
        task_id,
        AgentEvent::plan("Sending workspace context to Gemini for structured planning."),
    )
    .await;

    // 7. Request structured proposal from Gemini
    let proposal_val = match gemini.generate_json(&gemini_prompt).await {
        Ok(val) => val,
        Err(err) => {
            let error_msg = format!("Gemini proposal generation failed: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_msg);
            return;
        }
    };

    let proposal: TaskProposal = match serde_json::from_value(proposal_val) {
        Ok(p) => p,
        Err(err) => {
            let error_msg = format!("Gemini proposal validation failed: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_msg);
            return;
        }
    };

    // 8. Security and path validation for every proposed file change
    for change in &proposal.changes {
        if let Err(err) = validate_proposed_change(&workspace, change) {
            let error_msg = format!("Unsafe proposal rejected: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_msg);
            return;
        }
    }

    // 9. Store proposal in memory state and SQLite database
    state
        .proposals
        .write()
        .await
        .insert(task_id, proposal.clone());

    if let Err(err) = state
        .database
        .save_proposal(&task_id.to_string(), &proposal)
    {
        let error_msg = format!("Failed to persist task proposal: {}", err);
        send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
        let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
        let _ = state
            .database
            .save_memory(&task_id.to_string(), "error", &error_msg);
        return;
    }

    send_event(
        &state,
        task_id,
        AgentEvent::plan("Gemini proposal validated and stored for approval."),
    )
    .await;

    // 10. Save memories to SQLite database
    let task_memory = format!("Task: {}\n\nProposal summary: {}", prompt, proposal.summary);
    let _ = state
        .database
        .save_memory(&task_id.to_string(), "task", &task_memory);

    let mut solution_memory = format!(
        "Proposed solution:\n{}\n\nProposed file changes:\n",
        proposal.summary
    );
    for change in &proposal.changes {
        solution_memory.push_str(&format!(
            "- {} [{}]: {}\n",
            change.file, change.action, change.reason
        ));
    }
    let _ = state
        .database
        .save_memory(&task_id.to_string(), "solution", &solution_memory);

    if !proposal.verification.is_empty() {
        let _ = state.database.save_memory(
            &task_id.to_string(),
            "verification",
            &proposal.verification.join("\n"),
        );
    }

    if !proposal.risks.is_empty() {
        let _ =
            state
                .database
                .save_memory(&task_id.to_string(), "risk", &proposal.risks.join("\n"));
    }

    // 11. Emit file change diffs to event stream
    for change in &proposal.changes {
        send_event(
            &state,
            task_id,
            AgentEvent::tool_call(format!("file_{}", change.action), change.file.clone()),
        )
        .await;

        send_event(
            &state,
            task_id,
            AgentEvent::diff(change.file.clone(), change.content.clone()),
        )
        .await;
    }

    // 12. Emit verification plan
    if proposal.verification.is_empty() {
        send_event(
            &state,
            task_id,
            AgentEvent::test(
                "verification",
                "No verification steps were provided by Gemini.",
            ),
        )
        .await;
    } else {
        for verification in &proposal.verification {
            send_event(
                &state,
                task_id,
                AgentEvent::test(
                    verification.clone(),
                    "Waiting for approval before execution.",
                ),
            )
            .await;
        }
    }

    // 13. Emit risks
    if proposal.risks.is_empty() {
        send_event(
            &state,
            task_id,
            AgentEvent::plan("No specific risks were identified by Gemini."),
        )
        .await;
    } else {
        for risk in &proposal.risks {
            send_event(&state, task_id, AgentEvent::plan(format!("Risk: {}", risk))).await;
        }
    }

    // 14. Approval gate
    let _ = update_task_status(&state, task_id, TaskStatus::AwaitingApproval).await;

    send_event(
        &state,
        task_id,
        AgentEvent::plan("Proposal is ready for human approval. No files have been modified yet."),
    )
    .await;

    send_event(
        &state,
        task_id,
        AgentEvent::verify(
            "Waiting for developer approval before applying changes.",
            true,
        ),
    )
    .await;
}

// ============================================================
// APPROVED TASK EXECUTION WORKFLOW
// ============================================================

async fn execute_approved_task(state: AppState, task_id: Uuid) {
    let _ = update_task_status(&state, task_id, TaskStatus::Executing).await;

    let proposal = {
        let proposals = state.proposals.read().await;
        proposals.get(&task_id).cloned()
    };

    let proposal = match proposal {
        Some(p) => p,
        None => {
            let error_msg = format!("Task proposal for {} not found.", task_id);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            return;
        }
    };

    let workspace = match Workspace::new() {
        Ok(ws) => ws,
        Err(err) => {
            let error_msg = format!("Workspace initialization failed: {}", err);
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            return;
        }
    };

    let git = GitTool::new(&workspace).ok();
    if let Some(ref git_tool) = git {
        if let Ok(branch) = git_tool.current_branch().await {
            send_event(
                &state,
                task_id,
                AgentEvent::plan(format!("Working on Git branch: {}", branch)),
            )
            .await;
        }
    }

    send_event(
        &state,
        task_id,
        AgentEvent::plan(format!(
            "Applying {} approved file changes...",
            proposal.changes.len()
        )),
    )
    .await;

    // Apply file changes
    for change in &proposal.changes {
        if let Err(err) = validate_proposed_change(&workspace, change) {
            let error_msg = format!(
                "Security validation rejected change to {}: {}",
                change.file, err
            );
            send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            return;
        }

        send_event(
            &state,
            task_id,
            AgentEvent::tool_call(format!("file_{}", change.action), &change.file),
        )
        .await;

        match workspace.write_file(&change.file, &change.content).await {
            Ok(_) => {
                send_event(
                    &state,
                    task_id,
                    AgentEvent::diff(change.file.clone(), change.content.clone()),
                )
                .await;

                send_event(
                    &state,
                    task_id,
                    AgentEvent::plan(format!("Successfully wrote {}", change.file)),
                )
                .await;

                if let Some(ref git_tool) = git {
                    let _ = git_tool.add(&change.file).await;
                }
            }
            Err(err) => {
                let error_msg = format!("Failed to write file {}: {}", change.file, err);
                send_event(&state, task_id, AgentEvent::error(&error_msg)).await;
                let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
                return;
            }
        }
    }

    // Inspect Git diff after changes
    if let Some(ref git_tool) = git {
        if let Ok(diff) = git_tool.diff().await {
            if !diff.trim().is_empty() {
                send_event(
                    &state,
                    task_id,
                    AgentEvent::plan(format!("Git diff after changes:\n{}", diff)),
                )
                .await;
            }
        }
    }

    // Run verification steps
    if proposal.verification.is_empty() {
        send_event(
            &state,
            task_id,
            AgentEvent::test("verification", "No verification steps were specified."),
        )
        .await;
        let _ = update_task_status(&state, task_id, TaskStatus::Completed).await;
        send_event(
            &state,
            task_id,
            AgentEvent::completed("All approved file changes applied successfully."),
        )
        .await;
    } else {
        let _ = update_task_status(&state, task_id, TaskStatus::Verifying).await;
        send_event(
            &state,
            task_id,
            AgentEvent::plan("Starting verification in Docker sandbox..."),
        )
        .await;

        let runner = VerificationRunner::new();
        let mut all_passed = true;

        for command in &proposal.verification {
            send_event(
                &state,
                task_id,
                AgentEvent::test(command.clone(), "Executing verification command..."),
            )
            .await;

            match runner.verify(workspace.root(), command).await {
                Ok(result) => {
                    let output = if result.passed {
                        format!("Passed\n{}", result.stdout)
                    } else {
                        format!(
                            "Failed (exit code: {:?})\nSTDERR: {}\nSTDOUT: {}",
                            result.exit_code, result.stderr, result.stdout
                        )
                    };

                    send_event(
                        &state,
                        task_id,
                        AgentEvent::test(command.clone(), output.trim()),
                    )
                    .await;

                    send_event(
                        &state,
                        task_id,
                        AgentEvent::verify(
                            format!("Verification step: {}", command),
                            result.passed,
                        ),
                    )
                    .await;

                    if !result.passed {
                        all_passed = false;
                        let _ = state.database.save_memory(
                            &task_id.to_string(),
                            "verification_failure",
                            &format!("Verification failed for '{}': {}", command, result.stderr),
                        );
                        break;
                    }
                }
                Err(err) => {
                    all_passed = false;
                    let error_msg = format!("Sandbox execution error: {}", err);
                    send_event(
                        &state,
                        task_id,
                        AgentEvent::test(command.clone(), &error_msg),
                    )
                    .await;
                    send_event(
                        &state,
                        task_id,
                        AgentEvent::verify(format!("Verification failed: {}", command), false),
                    )
                    .await;
                    break;
                }
            }
        }

        if all_passed {
            if let Some(ref git_tool) = git {
                let _ = git_tool
                    .commit(&format!("Agent applied changes for task {}", task_id))
                    .await;
            }
            let _ = update_task_status(&state, task_id, TaskStatus::Completed).await;
            send_event(
                &state,
                task_id,
                AgentEvent::completed(
                    "All approved file changes applied and verified successfully.",
                ),
            )
            .await;
        } else {
            let _ = update_task_status(&state, task_id, TaskStatus::Failed).await;
            send_event(
                &state,
                task_id,
                AgentEvent::error("Verification failed for applied changes."),
            )
            .await;
        }
    }
}

// ============================================================
// VALIDATION & CONTEXT HELPERS
// ============================================================

fn validate_proposed_change(workspace: &Workspace, change: &FileChange) -> Result<(), String> {
    if change.file.trim().is_empty() {
        return Err("Proposal contains an empty file path.".to_string());
    }

    let path = Path::new(&change.file);

    if path.is_absolute() {
        return Err(format!("Absolute path is not allowed: {}", change.file));
    }

    if change.file.contains("..") {
        return Err(format!(
            "Parent-directory traversal is not allowed: {}",
            change.file
        ));
    }

    if Workspace::is_sensitive_path(&change.file) {
        return Err(format!("Sensitive file is not allowed: {}", change.file));
    }

    match change.action.as_str() {
        "modify" | "create" => {}
        action => return Err(format!("Unsupported file action '{}'.", action)),
    }

    let resolved = workspace.resolve_path(&change.file)?;
    if !workspace.is_inside(&resolved) {
        return Err("Path is outside the workspace.".to_string());
    }

    Ok(())
}

fn select_context_files(files: &[String]) -> Vec<String> {
    let mut selected = Vec::new();
    let priority_names = [
        "README.md",
        "Cargo.toml",
        "package.json",
        "tsconfig.json",
        "svelte.config.js",
        "vite.config.ts",
    ];

    for file in files {
        if Workspace::is_sensitive_path(file) {
            continue;
        }

        let priority = priority_names.iter().any(|name| file.ends_with(name));
        let source_file = file.ends_with(".rs")
            || file.ends_with(".ts")
            || file.ends_with(".svelte")
            || file.ends_with(".js")
            || file.ends_with(".json");

        if priority || source_file {
            selected.push(file.clone());
        }

        if selected.len() >= 30 {
            break;
        }
    }

    selected
}

async fn retrieve_memory_context(state: &AppState, query: &str) -> String {
    // 1. Semantic vector search if embedding client is available
    if let Ok(embedding_client) = GeminiEmbeddingClient::new() {
        if let Ok(embedding) = embedding_client.embed_query(query).await {
            if let Ok(results) = state.database.search_memory_vectors(&embedding, 5) {
                if !results.is_empty() {
                    let mut context =
                        String::from("RELEVANT MEMORY CONTEXT FROM PREVIOUS SESSIONS:\n");
                    for item in results {
                        context.push_str(&format!(
                            "- [{}] {}\n",
                            item.memory.kind,
                            item.memory.content.trim()
                        ));
                    }
                    return context;
                }
            }
        }
    }

    // 2. Fallback keyword text search
    if let Ok(records) = state.database.search_memories(query, 5) {
        if !records.is_empty() {
            let mut context = String::from("RELEVANT MEMORY CONTEXT FROM PREVIOUS SESSIONS:\n");
            for record in records {
                context.push_str(&format!("- [{}] {}\n", record.kind, record.content.trim()));
            }
            return context;
        }
    }

    String::new()
}
