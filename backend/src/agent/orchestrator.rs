
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::events::AgentEvent,
    gemini::client::GeminiClient,
    models::{FileChange, TaskProposal},
    state::AppState,
    tools::{git::GitTool, workspace::Workspace},
};

pub struct Orchestrator {
    gemini: GeminiClient,
}

impl Orchestrator {
    pub fn new(gemini: GeminiClient) -> Self {
        Self { gemini }
    }

    // ============================================================
    // MAIN AGENT ORCHESTRATOR
    // ============================================================

    pub async fn run(&self, state: AppState, task_id: Uuid, prompt: &str) -> Vec<AgentEvent> {
        let mut events = Vec::new();

        // --------------------------------------------------
        // 1. Initialize workspace
        // --------------------------------------------------

        let workspace = match Workspace::new() {
            Ok(workspace) => workspace,

            Err(error) => {
                events.push(AgentEvent::error(format!(
                    "Workspace initialization failed: {}",
                    error
                )));

                return events;
            }
        };

        events.push(AgentEvent::plan("Workspace initialized successfully."));

        // --------------------------------------------------
        // 2. Initialize Git tool
        // --------------------------------------------------

        let git = match GitTool::new(&workspace) {
            Ok(git) => git,

            Err(error) => {
                events.push(AgentEvent::error(format!(
                    "Git tool initialization failed: {}",
                    error
                )));

                return events;
            }
        };

        // --------------------------------------------------
        // 3. Check Git status
        // --------------------------------------------------

        match git.status().await {
            Ok(status) => {
                events.push(AgentEvent::plan(format!("Git status checked:\n{}", status)));
            }

            Err(error) => {
                events.push(AgentEvent::plan(format!(
                    "Git status check failed: {}",
                    error
                )));
            }
        }

        // --------------------------------------------------
        // 4. List workspace files
        // --------------------------------------------------

        let files = match workspace.list_files("").await {
            Ok(files) => files,

            Err(error) => {
                events.push(AgentEvent::error(format!(
                    "Failed to list workspace files: {}",
                    error
                )));

                return events;
            }
        };

        events.push(AgentEvent::plan(format!(
            "Workspace contains {} accessible files.",
            files.len()
        )));

        // --------------------------------------------------
        // 5. Select context files
        // --------------------------------------------------

        let priority_files = Self::select_context_files(&files);

        events.push(AgentEvent::plan(format!(
            "Selected {} files for Gemini context.",
            priority_files.len()
        )));

        // --------------------------------------------------
        // 6. Read context files
        // --------------------------------------------------

        let mut context = String::new();

        for file in priority_files {
            match workspace.read_file(&file).await {
                Ok(content) => {
                    context.push_str(&format!("\n\n===== FILE: {} =====\n", file));

                    context.push_str(&content);
                }

                Err(error) => {
                    events.push(AgentEvent::plan(format!(
                        "Skipped unreadable file {}: {}",
                        file, error
                    )));
                }
            }
        }

        // --------------------------------------------------
        // 7. Read current Git diff
        // --------------------------------------------------

        let current_diff = match git.diff().await {
            Ok(diff) => diff,

            Err(error) => {
                events.push(AgentEvent::plan(format!("Git diff unavailable: {}", error)));

                String::new()
            }
        };

        // --------------------------------------------------
        // 8. Build Gemini prompt
        // --------------------------------------------------

        let gemini_prompt = format!(
            r#"
You are the PLANNING ENGINE of an AI Coding Agent.

You are NOT the file execution engine.

The developer gave this task:

TASK:
{}

WORKSPACE CONTEXT:
{}

CURRENT GIT DIFF:
{}

============================================================
YOUR JOB
============================================================

Analyze the developer's task and produce a structured
implementation proposal for human approval.

You are ONLY planning the changes.

You must NOT actually claim that you created, modified,
deleted, executed, installed, committed, or deployed anything.

The actual file changes will be performed later by the
AI Coding Agent after the developer approves the proposal.

============================================================
STRICT RULES
============================================================

1. Return ONLY valid JSON.

2. Do NOT return Markdown.

3. Do NOT use Markdown code fences.

4. Do NOT write explanations outside the JSON object.

5. Do NOT say:
   - "I created the file."
   - "I modified the file."
   - "The file has been created."
   - "The changes have been applied."
   - "I implemented the task."
   - "I completed the task."

6. Instead, describe proposed changes inside the JSON.

7. Do NOT execute commands.

8. Do NOT pretend that commands were executed.

9. Do NOT pretend that tests passed.

10. Do NOT invent test results.

11. Every proposed file must contain COMPLETE file content.

12. Use relative file paths only.

13. Never use absolute paths.

14. Never use ".." path traversal.

15. Never modify:
    - .env
    - secrets
    - credentials
    - private keys
    - certificates
    - API keys

16. Keep the changes focused on the developer's task.

17. Do NOT invent unnecessary files.

18. Prefer modifying existing files when appropriate.

19. If the task requires a new file, use action "create".

20. If an existing file needs to change, use action "modify".

21. Supported actions are ONLY:
    - "create"
    - "modify"

22. Include appropriate verification commands.
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

23. Verification commands are suggestions only.
    They have NOT been executed.

24. Include risks only when there is a meaningful risk.

25. If there are no meaningful risks, return an empty risks array.

============================================================
IMPORTANT FOR SIMPLE FILE CREATION TASKS
============================================================

If the developer asks to create a simple file with exact
content, do NOT overcomplicate the solution.

For example, if the task is:

"Create backend/test.txt containing exactly:
Hello World"

The proposal should contain one change:

{{
  "file": "backend/test.txt",
  "action": "create",
  "reason": "Create the requested file with the exact content.",
  "content": "Hello World"
}}

Do NOT claim that the file has already been created.

============================================================
REQUIRED JSON STRUCTURE
============================================================

Return exactly one JSON object using this structure:

{{
  "summary": "Short explanation of the proposed implementation.",
  "changes": [
    {{
      "file": "relative/path/to/file",
      "action": "create",
      "reason": "Why this file should be created or modified.",
      "content": "Complete file content."
    }}
  ],
  "verification": [
    "Command or verification step that should be executed after approval."
  ],
  "risks": [
    "Possible risk associated with the proposed change."
  ]
}}

============================================================
FINAL REQUIREMENT
============================================================

Return ONLY the JSON object.

Do not return any introductory sentence.

Do not return any concluding sentence.

Do not say that you performed the change.

The response must be a proposal for human approval, not
a report of completed work.
"#,
            prompt, context, current_diff
        );

        events.push(AgentEvent::plan(
            "Sending workspace context to Gemini for structured planning.",
        ));

        // --------------------------------------------------
        // 9. Ask Gemini for structured proposal
        // --------------------------------------------------

        let proposal_json = match self.gemini.generate_json(&gemini_prompt).await {
            Ok(value) => value,

            Err(error) => {
                let error_message = format!("Gemini proposal generation failed: {}", error);

                events.push(AgentEvent::error(error_message.clone()));

                let _ = state
                    .database
                    .save_memory(&task_id.to_string(), "error", &error_message);

                return events;
            }
        };

        // --------------------------------------------------
        // 10. Convert Gemini JSON to TaskProposal
        // --------------------------------------------------

        let proposal: TaskProposal = match serde_json::from_value(proposal_json) {
            Ok(proposal) => proposal,

            Err(error) => {
                let error_message = format!("Gemini proposal validation failed: {}", error);

                events.push(AgentEvent::error(error_message.clone()));

                let _ = state
                    .database
                    .save_memory(&task_id.to_string(), "error", &error_message);

                return events;
            }
        };

        // --------------------------------------------------
        // 11. Validate proposed changes
        // --------------------------------------------------

        for change in &proposal.changes {
            if let Err(error) = Self::validate_change(change) {
                let error_message = format!("Unsafe proposal rejected: {}", error);

                events.push(AgentEvent::error(error_message.clone()));

                let _ = state
                    .database
                    .save_memory(&task_id.to_string(), "error", &error_message);

                return events;
            }
        }

        // --------------------------------------------------
        // 12. Store proposal in application state
        // --------------------------------------------------

        {
            let mut proposals = state.proposals.write().await;

            proposals.insert(task_id, proposal.clone());
        }

        // --------------------------------------------------
        // 13. Save proposal to SQLite
        // --------------------------------------------------

        if let Err(error) = state
            .database
            .save_proposal(&task_id.to_string(), &proposal)
        {
            let error_message = format!("Failed to persist task proposal: {}", error);

            events.push(AgentEvent::error(error_message.clone()));

            let _ = state
                .database
                .save_memory(&task_id.to_string(), "error", &error_message);

            return events;
        }

        events.push(AgentEvent::plan(
            "Gemini proposal validated and stored for approval.",
        ));

        // --------------------------------------------------
        // 14. Save task summary as persistent memory
        // --------------------------------------------------

        let task_memory = format!("Task: {}\n\nProposal summary: {}", prompt, proposal.summary);

        if let Err(error) = state
            .database
            .save_memory(&task_id.to_string(), "task", &task_memory)
        {
            events.push(AgentEvent::plan(format!(
                "Task memory could not be saved: {}",
                error
            )));
        }

        // --------------------------------------------------
        // 15. Save proposed solution as persistent memory
        // --------------------------------------------------

        let mut solution_memory = String::new();

        solution_memory.push_str("Proposed solution:\n");

        solution_memory.push_str(&proposal.summary);

        solution_memory.push_str("\n\nProposed file changes:\n");

        for change in &proposal.changes {
            solution_memory.push_str(&format!(
                "- {} [{}]: {}\n",
                change.file, change.action, change.reason
            ));
        }

        if let Err(error) =
            state
                .database
                .save_memory(&task_id.to_string(), "solution", &solution_memory)
        {
            events.push(AgentEvent::plan(format!(
                "Solution memory could not be saved: {}",
                error
            )));
        }

        // --------------------------------------------------
        // 16. Emit proposed diffs
        // --------------------------------------------------

        for change in &proposal.changes {
            events.push(AgentEvent::diff(
                change.file.clone(),
                change.content.clone(),
            ));
        }

        // --------------------------------------------------
        // 17. Emit verification plan
        // --------------------------------------------------

        if proposal.verification.is_empty() {
            events.push(AgentEvent::test(
                "verification",
                "No verification steps were provided by Gemini.",
            ));

            let _ = state.database.save_memory(
                &task_id.to_string(),
                "verification",
                "Gemini did not provide verification steps.",
            );
        } else {
            let verification_memory = proposal.verification.join("\n");

            if let Err(error) = state.database.save_memory(
                &task_id.to_string(),
                "verification",
                &verification_memory,
            ) {
                events.push(AgentEvent::plan(format!(
                    "Verification memory could not be saved: {}",
                    error
                )));
            }

            for command in &proposal.verification {
                events.push(AgentEvent::test(
                    command.clone(),
                    "Waiting for approval before execution.",
                ));
            }
        }

        // --------------------------------------------------
        // 18. Emit risks
        // --------------------------------------------------

        if proposal.risks.is_empty() {
            events.push(AgentEvent::plan(
                "No specific risks were identified by Gemini.",
            ));
        } else {
            let risk_memory = proposal.risks.join("\n");

            if let Err(error) =
                state
                    .database
                    .save_memory(&task_id.to_string(), "risk", &risk_memory)
            {
                events.push(AgentEvent::plan(format!(
                    "Risk memory could not be saved: {}",
                    error
                )));
            }

            for risk in &proposal.risks {
                events.push(AgentEvent::plan(format!("Risk: {}", risk)));
            }
        }

        // --------------------------------------------------
        // 19. Approval gate
        // --------------------------------------------------

        events.push(AgentEvent::plan(
            "Proposal is ready for human approval. No files have been modified.",
        ));

        events.push(AgentEvent::verify(
            "Waiting for developer approval before applying changes.",
            true,
        ));

        events
    }

    // ============================================================
    // GEMINI REVISION ANALYZER
    // ============================================================

    /// Analyze a failed verification using Gemini.
    ///
    /// This method does not modify files.
    /// It only asks Gemini to generate a structured
    /// correction proposal.
    pub async fn analyze_verification_failure(
        &self,
        task: &str,
        verification_command: &str,
        verification_output: &str,
        current_code: &str,
    ) -> Result<Value, String> {
        self.gemini
            .generate_revision(
                task,
                verification_command,
                verification_output,
                current_code,
            )
            .await
    }

    // ============================================================
    // SELECT USEFUL CONTEXT FILES
    // ============================================================

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
            if Self::is_sensitive(file) {
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

    // ============================================================
    // VALIDATE PROPOSED FILE CHANGE
    // ============================================================

    fn validate_change(change: &FileChange) -> Result<(), String> {
        let path = std::path::Path::new(&change.file);

        if change.file.trim().is_empty() {
            return Err("Proposal contains an empty file path.".to_string());
        }

        if path.is_absolute() {
            return Err(format!("Absolute path is not allowed: {}", change.file));
        }

        if change.file.contains("..") {
            return Err(format!(
                "Parent-directory traversal is not allowed: {}",
                change.file
            ));
        }

        if Self::is_sensitive(&change.file) {
            return Err(format!("Sensitive file is not allowed: {}", change.file));
        }

        match change.action.as_str() {
            "modify" | "create" => {}

            action => {
                return Err(format!("Unsupported file action '{}'.", action));
            }
        }

        if change.content.is_empty() {
            return Err(format!(
                "Proposed file '{}' contains empty content.",
                change.file
            ));
        }

        Ok(())
    }

    // ============================================================
    // SENSITIVE FILE PROTECTION
    // ============================================================

    fn is_sensitive(path: &str) -> bool {
        let lower = path.to_lowercase();

        lower == ".env"
            || lower.ends_with("/.env")
            || lower.contains("secret")
            || lower.contains("credential")
            || lower.ends_with(".pem")
            || lower.ends_with(".key")
    }
}
