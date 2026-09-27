use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;

use crate::tools::workspace::Workspace;

pub struct GitTool {
    root: PathBuf,
}

impl GitTool {
    // --------------------------------------------------
    // Create Git tool
    // --------------------------------------------------

    pub fn new(workspace: &Workspace) -> Result<Self, String> {
        let root = workspace.root().to_path_buf();

        if !root.exists() {
            return Err(format!("Git workspace does not exist: {}", root.display()));
        }

        if !root.is_dir() {
            return Err(format!(
                "Git workspace is not a directory: {}",
                root.display()
            ));
        }

        Ok(Self { root })
    }

    // --------------------------------------------------
    // Git root
    // --------------------------------------------------

    pub fn root(&self) -> &Path {
        &self.root
    }

    // --------------------------------------------------
    // Check whether this is a Git repository
    // --------------------------------------------------

    pub async fn is_repository(&self) -> Result<bool, String> {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args(["rev-parse", "--is-inside-work-tree"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|error| format!("Failed to execute git: {}", error))?;

        if !output.status.success() {
            return Ok(false);
        }

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(result == "true")
    }

    // --------------------------------------------------
    // Git status
    // --------------------------------------------------

    pub async fn status(&self) -> Result<String, String> {
        self.run_git(&["status", "--short", "--branch"]).await
    }

    // --------------------------------------------------
    // Git diff
    // --------------------------------------------------

    pub async fn diff(&self) -> Result<String, String> {
        self.run_git(&["diff"]).await
    }

    // --------------------------------------------------
    // Git diff for a specific file
    // --------------------------------------------------

    pub async fn diff_file(&self, relative_path: &str) -> Result<String, String> {
        Self::validate_relative_path(relative_path)?;

        self.run_git(&["diff", "--", relative_path]).await
    }

    // --------------------------------------------------
    // Git staged diff
    // --------------------------------------------------

    pub async fn staged_diff(&self) -> Result<String, String> {
        self.run_git(&["diff", "--cached"]).await
    }

    // --------------------------------------------------
    // Git branch
    // --------------------------------------------------

    pub async fn current_branch(&self) -> Result<String, String> {
        self.run_git(&["branch", "--show-current"]).await
    }

    // --------------------------------------------------
    // Git log
    // --------------------------------------------------

    pub async fn log(&self, limit: usize) -> Result<String, String> {
        let limit = limit.clamp(1, 50);

        let limit_arg = format!("-{}", limit);

        self.run_git(&["log", "--oneline", "--decorate", &limit_arg])
            .await
    }

    // --------------------------------------------------
    // Git add
    // --------------------------------------------------

    pub async fn add(&self, relative_path: &str) -> Result<String, String> {
        Self::validate_relative_path(relative_path)?;

        self.run_git(&["add", "--", relative_path]).await
    }

    // --------------------------------------------------
    // Git add all changes
    // --------------------------------------------------

    pub async fn add_all(&self) -> Result<String, String> {
        self.run_git(&["add", "-A"]).await
    }

    // --------------------------------------------------
    // Git reset a file
    // --------------------------------------------------

    pub async fn reset_file(&self, relative_path: &str) -> Result<String, String> {
        Self::validate_relative_path(relative_path)?;

        self.run_git(&["restore", "--staged", "--", relative_path])
            .await
    }

    // --------------------------------------------------
    // Git checkout a file from HEAD
    // --------------------------------------------------

    pub async fn restore_file(&self, relative_path: &str) -> Result<String, String> {
        Self::validate_relative_path(relative_path)?;

        self.run_git(&["restore", "--", relative_path]).await
    }

    // --------------------------------------------------
    // Create a Git commit
    // --------------------------------------------------

    pub async fn commit(&self, message: &str) -> Result<String, String> {
        let message = message.trim();

        if message.is_empty() {
            return Err("Commit message cannot be empty.".to_string());
        }

        if message.len() > 200 {
            return Err("Commit message is too long.".to_string());
        }

        self.run_git(&["commit", "-m", message]).await
    }

    // --------------------------------------------------
    // Check whether working tree is clean
    // --------------------------------------------------

    pub async fn is_clean(&self) -> Result<bool, String> {
        let status = self.run_git(&["status", "--porcelain"]).await?;

        Ok(status.trim().is_empty())
    }

    // --------------------------------------------------
    // Validate a safe relative path
    // --------------------------------------------------

    fn validate_relative_path(relative_path: &str) -> Result<(), String> {
        let path = Path::new(relative_path);

        if relative_path.trim().is_empty() {
            return Err("Git path cannot be empty.".to_string());
        }

        if path.is_absolute() {
            return Err("Absolute Git paths are not allowed.".to_string());
        }

        for component in path.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err("Git path traversal is not allowed.".to_string());
            }
        }

        Ok(())
    }

    // --------------------------------------------------
    // Execute Git command
    // --------------------------------------------------

    async fn run_git(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|error| format!("Failed to execute git command: {}", error))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            return Err(format!(
                "Git command failed: {}\n{}",
                args.join(" "),
                if stderr.is_empty() { stdout } else { stderr }
            ));
        }

        Ok(stdout)
    }
}
