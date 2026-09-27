use std::{
    env,
    path::{Path, PathBuf},
};

pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    // =========================================================
    // CREATE WORKSPACE
    // =========================================================

    pub fn new() -> Result<Self, String> {
        let root = env::var("AGENT_WORKSPACE")
            .map_err(|_| "AGENT_WORKSPACE is not set in .env".to_string())?;

        let root = PathBuf::from(root);

        if !root.exists() {
            return Err(format!("Workspace does not exist: {}", root.display()));
        }

        if !root.is_dir() {
            return Err(format!("Workspace is not a directory: {}", root.display()));
        }

        let root = root
            .canonicalize()
            .map_err(|error| format!("Failed to resolve workspace path: {}", error))?;

        Ok(Self { root })
    }

    // =========================================================
    // WORKSPACE ROOT
    // =========================================================

    pub fn root(&self) -> &Path {
        &self.root
    }

    // =========================================================
    // RESOLVE PATH
    // =========================================================

    pub fn resolve_path(&self, relative_path: &str) -> Result<PathBuf, String> {
        if relative_path.trim().is_empty() {
            return Err("Path cannot be empty.".to_string());
        }

        let requested = Path::new(relative_path);

        if requested.is_absolute() {
            return Err("Absolute paths are not allowed.".to_string());
        }

        for component in requested.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err("Parent directory traversal is not allowed.".to_string());
            }
        }

        let candidate = self.root.join(requested);

        // Existing path
        if candidate.exists() {
            let canonical = candidate
                .canonicalize()
                .map_err(|error| format!("Failed to resolve path: {}", error))?;

            if !canonical.starts_with(&self.root) {
                return Err("Path is outside the workspace.".to_string());
            }

            return Ok(canonical);
        }

        // New file
        let parent = candidate
            .parent()
            .ok_or_else(|| "Invalid file path.".to_string())?;

        if !parent.exists() {
            return Err(format!(
                "Parent directory does not exist: {}",
                parent.display()
            ));
        }

        let canonical_parent = parent
            .canonicalize()
            .map_err(|error| format!("Failed to resolve parent directory: {}", error))?;

        if !canonical_parent.starts_with(&self.root) {
            return Err("Path is outside the workspace.".to_string());
        }

        Ok(candidate)
    }

    // =========================================================
    // CHECK PATH
    // =========================================================

    pub fn is_inside(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    // =========================================================
    // LIST FILES
    // =========================================================

    pub async fn list_files(&self, relative_path: &str) -> Result<Vec<String>, String> {
        let directory = if relative_path.trim().is_empty() {
            self.root.clone()
        } else {
            self.resolve_path(relative_path)?
        };

        if !directory.exists() {
            return Err(format!("Directory does not exist: {}", relative_path));
        }

        if !directory.is_dir() {
            return Err(format!("Path is not a directory: {}", relative_path));
        }

        let mut files = Vec::new();

        self.collect_files(&directory, &mut files).await?;

        files.sort();

        Ok(files)
    }

    // =========================================================
    // COLLECT FILES
    // =========================================================

    async fn collect_files(&self, directory: &Path, files: &mut Vec<String>) -> Result<(), String> {
        let mut directories = vec![directory.to_path_buf()];

        while let Some(current_directory) = directories.pop() {
            let mut entries = tokio::fs::read_dir(&current_directory)
                .await
                .map_err(|error| {
                    format!(
                        "Failed to read directory {}: {}",
                        current_directory.display(),
                        error
                    )
                })?;

            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|error| format!("Failed to read directory entry: {}", error))?
            {
                let path = entry.path();

                let file_type = entry
                    .file_type()
                    .await
                    .map_err(|error| format!("Failed to inspect {}: {}", path.display(), error))?;

                if file_type.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();

                    // Ignore generated/dependency directories.
                    if name == "target" || name == "node_modules" || name == ".git" {
                        continue;
                    }

                    directories.push(path);
                } else if file_type.is_file() {
                    let relative = path
                        .strip_prefix(&self.root)
                        .map_err(|error| format!("Failed to create relative path: {}", error))?
                        .to_string_lossy()
                        .replace('\\', "/");

                    if !Self::is_sensitive_path(&relative) {
                        files.push(relative);
                    }
                }
            }
        }

        Ok(())
    }

    // =========================================================
    // READ FILE
    // =========================================================

    pub async fn read_file(&self, relative_path: &str) -> Result<String, String> {
        if Self::is_sensitive_path(relative_path) {
            return Err("Reading sensitive files is not allowed.".to_string());
        }

        let path = self.resolve_path(relative_path)?;

        if !path.exists() {
            return Err(format!("File does not exist: {}", relative_path));
        }

        if !path.is_file() {
            return Err(format!("Path is not a file: {}", relative_path));
        }

        tokio::fs::read_to_string(&path)
            .await
            .map_err(|error| format!("Failed to read {}: {}", relative_path, error))
    }

    // =========================================================
    // WRITE FILE
    // =========================================================

    pub async fn write_file(&self, relative_path: &str, content: &str) -> Result<(), String> {
        if Self::is_sensitive_path(relative_path) {
            return Err("Writing sensitive files is not allowed.".to_string());
        }

        let path = self.resolve_path(relative_path)?;

        if !self.is_inside(&path) {
            return Err("File path is outside the workspace.".to_string());
        }

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                format!("Failed to create directory {}: {}", parent.display(), error)
            })?;
        }

        tokio::fs::write(&path, content)
            .await
            .map_err(|error| format!("Failed to write {}: {}", relative_path, error))
    }

    // =========================================================
    // CHECK FILE
    // =========================================================

    pub async fn file_exists(&self, relative_path: &str) -> Result<bool, String> {
        if Self::is_sensitive_path(relative_path) {
            return Ok(false);
        }

        let path = self.resolve_path(relative_path)?;

        Ok(path.exists() && path.is_file())
    }

    // =========================================================
    // DELETE FILE
    // =========================================================
    //
    // Used only when the revision system explicitly needs to
    // remove a generated file.
    //
    // Sensitive files can never be deleted.
    // =========================================================

    pub async fn delete_file(&self, relative_path: &str) -> Result<(), String> {
        if Self::is_sensitive_path(relative_path) {
            return Err("Deleting sensitive files is not allowed.".to_string());
        }

        let path = self.resolve_path(relative_path)?;

        if !self.is_inside(&path) {
            return Err("File path is outside the workspace.".to_string());
        }

        if !path.exists() {
            return Ok(());
        }

        if !path.is_file() {
            return Err(format!("Path is not a file: {}", relative_path));
        }

        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| format!("Failed to delete {}: {}", relative_path, error))
    }

    // =========================================================
    // WRITE MULTIPLE FILES
    // =========================================================
    //
    // This will be used by the revision system when Gemini
    // proposes changes to more than one file.
    // =========================================================

    pub async fn write_files(&self, files: &[(String, String)]) -> Result<(), String> {
        for (path, content) in files {
            self.write_file(path, content).await?;
        }

        Ok(())
    }

    // =========================================================
    // SENSITIVE PATH CHECK
    // =========================================================

    pub fn is_sensitive_path(path: &str) -> bool {
        let lower = path.replace('\\', "/").to_ascii_lowercase();

        lower == ".env"
            || lower.ends_with("/.env")
            || lower.contains("secret")
            || lower.contains("credential")
            || lower.ends_with(".pem")
            || lower.ends_with(".key")
    }
}
