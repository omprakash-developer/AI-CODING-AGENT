use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug)]
pub struct SandboxResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub struct DockerSandbox;

impl DockerSandbox {
    pub fn new() -> Self {
        Self
    }

    /// Run a command in the Docker sandbox from /workspace.
    pub async fn run(&self, workspace_path: &Path, command: &str) -> Result<SandboxResult, String> {
        self.run_in_directory(workspace_path, "/workspace", command)
            .await
    }

    /// Run a command in the Docker sandbox from a specific directory.
    pub async fn run_in_directory(
        &self,
        workspace_path: &Path,
        working_directory: &str,
        command: &str,
    ) -> Result<SandboxResult, String> {
        println!("========================================");
        println!("DOCKER SANDBOX");
        println!("Workspace: {}", workspace_path.display());
        println!("Working directory: {}", working_directory);
        println!("Command: {}", command);

        // Check that workspace exists.
        if !workspace_path.exists() {
            return Err(format!(
                "Workspace does not exist: {}",
                workspace_path.display()
            ));
        }

        // Check that workspace is a directory.
        if !workspace_path.is_dir() {
            return Err(format!(
                "Workspace is not a directory: {}",
                workspace_path.display()
            ));
        }

        // Resolve the absolute/canonical workspace path.
        let workspace_path = workspace_path
            .canonicalize()
            .map_err(|e| format!("Failed to resolve workspace path: {}", e))?;

        println!("Canonical workspace: {}", workspace_path.display());

        // Windows canonicalize() can return:
        //
        // \\?\F:\AI CODING AGENT
        //
        // Docker does not accept that form in a -v mount.
        //
        // Convert it to:
        //
        // F:\AI CODING AGENT:/workspace:rw
        let workspace_path_string = workspace_path.to_string_lossy().replace("\\\\?\\", "");

        let workspace_mount = format!("{}:/workspace:rw", workspace_path_string);

        println!("Docker mount: {}", workspace_mount);

        // Validate the Docker working directory.
        if !working_directory.starts_with("/workspace") {
            return Err(format!(
                "Invalid Docker working directory: {}",
                working_directory
            ));
        }

        // Execute inside the requested working directory.
        let shell_command = format!(
            "cd '{}' && {}",
            working_directory.replace('\'', "'\\''"),
            command
        );

        let docker_command = Command::new("docker")
            .args([
                "run",
                "--rm",
                // Resource limits.
                "--cpus",
                "8",
                "--memory",
                "6g",
                "--pids-limit",
                "256",
                // Security restrictions.
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges",
                // Cargo HTTPS certificate configuration.
                "-e",
                "CARGO_HTTP_CAINFO=/etc/ssl/certs/ca-certificates.crt",
                // Persistent Linux Cargo build directory.
                "-e",
                "CARGO_TARGET_DIR=/cargo-target",
                // Persistent Cargo target volume.
                "-v",
                "ai-coding-agent-cargo-target:/cargo-target:rw",
                // Project workspace.
                "-v",
                &workspace_mount,
                "ai-coding-agent-sandbox:latest",
                "sh",
                "-c",
                &shell_command,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        // Allow enough time for native dependencies such as Kuzu
        // to compile when the cache is incomplete.
        let result = timeout(Duration::from_secs(300), docker_command)
            .await
            .map_err(|_| "Docker sandbox timed out after 300 seconds.".to_string())?
            .map_err(|error| format!("Failed to start Docker sandbox: {}", error))?;

        // Output.
        let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();

        let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();

        let exit_code = result.status.code();
        let success = result.status.success();

        println!("Exit code: {:?}", exit_code);
        println!("Success: {}", success);

        if !stdout.is_empty() {
            println!("stdout:");
            println!("{}", stdout);
        }

        if !stderr.is_empty() {
            println!("stderr:");
            println!("{}", stderr);
        }

        println!("========================================");

        Ok(SandboxResult {
            success,
            exit_code,
            stdout,
            stderr,
        })
    }
}
