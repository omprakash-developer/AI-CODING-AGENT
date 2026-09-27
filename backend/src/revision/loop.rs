use std::path::Path;

use serde_json::Value;

use crate::{
    agent::orchestrator::Orchestrator,
    gemini::client::GeminiClient,
    tools::workspace::Workspace,
    verification::runner::{VerificationResult, VerificationRunner},
};

#[derive(Debug)]
pub struct RevisionAttempt {
    pub attempt: u32,
    pub verification: VerificationResult,
    pub revision: Option<Value>,
    pub applied_files: Vec<String>,
}

#[derive(Debug)]
pub struct RevisionLoopResult {
    pub success: bool,
    pub attempts: Vec<RevisionAttempt>,
}

pub struct RevisionLoop {
    max_attempts: u32,
    gemini: GeminiClient,
}

impl RevisionLoop {
    pub fn new(max_attempts: u32, gemini: GeminiClient) -> Self {
        Self {
            max_attempts,
            gemini,
        }
    }

    pub async fn run(
        &self,
        task: &str,
        verification_command: &str,
        current_code: &str,
    ) -> Result<RevisionLoopResult, String> {
        println!("========================================");
        println!("AGENT REVISION LOOP");
        println!("Maximum attempts: {}", self.max_attempts);
        println!("========================================");

        // -----------------------------------------------------
        // Initialize workspace
        // -----------------------------------------------------

        let workspace = Workspace::new()?;

        println!("Revision workspace: {}", workspace.root().display());

        // -----------------------------------------------------
        // Verification runner
        // -----------------------------------------------------

        let runner = VerificationRunner::new();

        // -----------------------------------------------------
        // Gemini orchestrator
        // -----------------------------------------------------

        let orchestrator = Orchestrator::new(self.gemini.clone());

        let mut attempts = Vec::new();

        // -----------------------------------------------------
        // Current code context
        // -----------------------------------------------------

        let mut code_context = current_code.to_string();

        // -----------------------------------------------------
        // Revision attempts
        // -----------------------------------------------------

        for attempt_number in 1..=self.max_attempts {
            println!("----------------------------------------");
            println!("Revision attempt {}/{}", attempt_number, self.max_attempts);
            println!("----------------------------------------");

            // -------------------------------------------------
            // Run verification
            // -------------------------------------------------

            let verification = runner
                .verify(workspace.root(), verification_command)
                .await?;

            let passed = verification.passed;

            println!(
                "Verification result: {}",
                if passed { "PASS" } else { "FAIL" }
            );

            // -------------------------------------------------
            // Verification passed
            // -------------------------------------------------

            if passed {
                println!("Verification passed on attempt {}.", attempt_number);

                attempts.push(RevisionAttempt {
                    attempt: attempt_number,
                    verification,
                    revision: None,
                    applied_files: Vec::new(),
                });

                println!("========================================");

                return Ok(RevisionLoopResult {
                    success: true,
                    attempts,
                });
            }

            // -------------------------------------------------
            // Verification failed
            // -------------------------------------------------

            println!("Verification failed on attempt {}.", attempt_number);

            if !verification.stdout.is_empty() {
                println!("STDOUT:");
                println!("{}", verification.stdout);
            }

            if !verification.stderr.is_empty() {
                println!("STDERR:");
                println!("{}", verification.stderr);
            }

            // -------------------------------------------------
            // Prepare verification information
            // -------------------------------------------------

            let verification_output = format!(
                "Exit code: {:?}\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                verification.exit_code, verification.stdout, verification.stderr
            );

            println!("Sending verification failure to Gemini...");

            // -------------------------------------------------
            // Ask Gemini for correction
            // -------------------------------------------------

            let revision = orchestrator
                .analyze_verification_failure(
                    task,
                    verification_command,
                    &verification_output,
                    &code_context,
                )
                .await?;

            println!("Gemini revision analysis received.");

            // -------------------------------------------------
            // Display analysis
            // -------------------------------------------------

            if let Some(analysis) = revision.get("analysis") {
                println!("Analysis: {}", analysis);
            }

            if let Some(root_cause) = revision.get("root_cause") {
                println!("Root cause: {}", root_cause);
            }

            if let Some(action) = revision.get("action") {
                println!("Recommended action: {}", action);
            }

            // -------------------------------------------------
            // Apply correction
            // -------------------------------------------------

            let applied_files = Self::apply_revision(&workspace, &revision).await?;

            if applied_files.is_empty() {
                println!("Gemini did not provide any files to modify.");
            } else {
                println!("Applied {} file change(s).", applied_files.len());

                for file in &applied_files {
                    println!("Applied: {}", file);
                }
            }

            // -------------------------------------------------
            // Refresh code context
            // -------------------------------------------------

            if !applied_files.is_empty() {
                code_context = Self::build_updated_context(&workspace, &applied_files).await?;

                println!("Updated code context loaded for next revision.");
            }

            // -------------------------------------------------
            // Gemini recommended verification command
            // -------------------------------------------------

            if let Some(command) = revision.get("verification_command") {
                println!("Gemini verification command: {}", command);
            }

            // -------------------------------------------------
            // Save attempt
            // -------------------------------------------------

            attempts.push(RevisionAttempt {
                attempt: attempt_number,
                verification,
                revision: Some(revision.clone()),
                applied_files: applied_files.clone(),
            });

            // -------------------------------------------------
            // Continue
            // -------------------------------------------------

            if attempt_number < self.max_attempts {
                println!("Revision applied.");

                println!("Starting next verification attempt...");
            }
        }

        // -----------------------------------------------------
        // Maximum attempts reached
        // -----------------------------------------------------

        println!(
            "Revision loop exhausted after {} attempts.",
            self.max_attempts
        );

        println!("========================================");

        Ok(RevisionLoopResult {
            success: false,
            attempts,
        })
    }

    // =========================================================
    // APPLY REVISION
    // =========================================================

    async fn apply_revision(
        workspace: &Workspace,
        revision: &Value,
    ) -> Result<Vec<String>, String> {
        let files = revision
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| "Gemini revision does not contain a valid 'files' array.".to_string())?;

        let mut applied_files = Vec::new();

        for file in files {
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "Revision file is missing 'path'.".to_string())?;

            let operation = file
                .get("operation")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Revision file '{}' is missing 'operation'.", path))?;

            // -------------------------------------------------
            // Path validation
            // -------------------------------------------------

            if path.trim().is_empty() {
                return Err("Revision contains an empty file path.".to_string());
            }

            if PathIsUnsafe::check(path) {
                return Err(format!("Unsafe revision path rejected: {}", path));
            }

            if Workspace::is_sensitive_path(path) {
                return Err(format!("Sensitive file modification rejected: {}", path));
            }

            // -------------------------------------------------
            // Operation validation
            // -------------------------------------------------

            match operation {
                "create" | "modify" => {}

                "delete" => {
                    return Err(format!(
                        "Delete operation is not allowed in automatic revision: {}",
                        path
                    ));
                }

                other => {
                    return Err(format!(
                        "Unsupported revision operation '{}' for '{}'.",
                        other, path
                    ));
                }
            }

            // -------------------------------------------------
            // Content validation
            // -------------------------------------------------

            let content = file.get("content").and_then(Value::as_str).ok_or_else(|| {
                format!("Revision file '{}' is missing complete 'content'.", path)
            })?;

            if content.trim().is_empty() {
                return Err(format!("Revision file '{}' contains empty content.", path));
            }

            // -------------------------------------------------
            // Resolve path
            // -------------------------------------------------

            let resolved = workspace.resolve_path(path)?;

            if !workspace.is_inside(&resolved) {
                return Err(format!("Revision path is outside workspace: {}", path));
            }

            // -------------------------------------------------
            // Write corrected file
            // -------------------------------------------------

            workspace.write_file(path, content).await?;

            applied_files.push(path.to_string());
        }

        Ok(applied_files)
    }

    // =========================================================
    // BUILD UPDATED CODE CONTEXT
    // =========================================================

    async fn build_updated_context(
        workspace: &Workspace,
        files: &[String],
    ) -> Result<String, String> {
        let mut context = String::new();

        for file in files {
            let content = workspace.read_file(file).await?;

            context.push_str(&format!("\n\n===== UPDATED FILE: {} =====\n", file));

            context.push_str(&content);
        }

        Ok(context)
    }
}

// =============================================================
// PATH SAFETY
// =============================================================

struct PathIsUnsafe;

impl PathIsUnsafe {
    fn check(path: &str) -> bool {
        use std::path::{Component, Path};

        let requested = Path::new(path);

        if requested.is_absolute() {
            return true;
        }

        for component in requested.components() {
            if matches!(component, Component::ParentDir) {
                return true;
            }
        }

        false
    }
}
