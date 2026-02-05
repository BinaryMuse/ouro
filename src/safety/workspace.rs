use std::path::{Path, PathBuf};

/// Enforces workspace-scoped write access.
/// Reads are unrestricted; writes must target paths within the workspace.
pub struct WorkspaceGuard {
    /// Canonical (absolute, symlinks resolved) workspace root.
    canonical_root: PathBuf,
}

impl WorkspaceGuard {
    /// Create a new guard for the given workspace path.
    /// Creates the directory if it doesn't exist and resolves to canonical path.
    pub fn new(workspace_path: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(workspace_path)?;
        let canonical_root = std::fs::canonicalize(workspace_path)?;
        Ok(Self { canonical_root })
    }

    /// Check if a write to the given path is allowed.
    /// Resolves symlinks to prevent escape via symlink traversal.
    #[allow(dead_code)]
    pub fn is_write_allowed(&self, target: &Path) -> Result<bool, std::io::Error> {
        let canonical = if target.exists() {
            std::fs::canonicalize(target)?
        } else {
            let parent = target
                .parent()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent"))?;
            if parent.exists() {
                let canonical_parent = std::fs::canonicalize(parent)?;
                canonical_parent.join(target.file_name().unwrap_or_default())
            } else {
                return Ok(false);
            }
        };

        Ok(canonical.starts_with(&self.canonical_root))
    }

    /// Get the canonical workspace root path.
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }
}
