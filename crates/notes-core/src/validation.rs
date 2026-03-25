use std::path::{Component, Path, PathBuf};

use crate::error::CoreError;

/// Validate that a project name is a single, safe directory component.
/// Rejects path traversal, hidden directories, reserved names, and control characters.
pub fn validate_project_name(name: &str) -> Result<(), CoreError> {
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "project name must not be empty".into(),
        ));
    }
    if name.len() > 255 {
        return Err(CoreError::InvalidInput(
            "project name must be 255 characters or fewer".into(),
        ));
    }
    if name.starts_with('.') {
        return Err(CoreError::InvalidInput(
            "project name must not start with '.'".into(),
        ));
    }
    if name.contains(['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(CoreError::InvalidInput(
            "project name contains reserved characters".into(),
        ));
    }
    if name.contains(|c: char| c.is_control()) {
        return Err(CoreError::InvalidInput(
            "project name contains control characters".into(),
        ));
    }
    if name == "." || name == ".." {
        return Err(CoreError::InvalidInput(
            "project name must not be '.' or '..'".into(),
        ));
    }

    // Windows reserved names
    let upper = name.to_uppercase();
    // Strip extension for Windows check (CON.txt is also reserved)
    let stem = upper.split('.').next().unwrap_or(&upper);
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if reserved.contains(&stem) {
        return Err(CoreError::InvalidInput(
            "project name is a reserved system name".into(),
        ));
    }

    Ok(())
}

/// Validate that a relative file path is safe (no traversal, no absolute paths).
/// Each component must be a normal filename.
pub fn validate_relative_path(path: &str) -> Result<(), CoreError> {
    if path.is_empty() {
        return Err(CoreError::InvalidInput("path must not be empty".into()));
    }
    if path.len() > 1024 {
        return Err(CoreError::InvalidInput(
            "path must be 1024 characters or fewer".into(),
        ));
    }
    if path.contains('\0') {
        return Err(CoreError::InvalidInput(
            "path must not contain null bytes".into(),
        ));
    }

    let p = Path::new(path);
    for component in p.components() {
        match component {
            Component::Normal(c) => {
                let s = c.to_string_lossy();
                if s.contains(|c: char| c.is_control()) {
                    return Err(CoreError::InvalidInput(
                        "path component contains control characters".into(),
                    ));
                }
            }
            _ => {
                return Err(CoreError::InvalidInput(
                    "path contains disallowed component (../, /, absolute, etc.)".into(),
                ));
            }
        }
    }

    // Max depth: 10 levels
    if path.matches('/').count() > 10 {
        return Err(CoreError::InvalidInput(
            "path is too deeply nested (max 10 levels)".into(),
        ));
    }

    Ok(())
}

/// Validate a note path — must be a valid relative path ending in .md.
pub fn validate_note_path(path: &str) -> Result<(), CoreError> {
    validate_relative_path(path)?;
    if !path.ends_with(".md") {
        return Err(CoreError::InvalidInput(
            "note path must end with .md".into(),
        ));
    }
    Ok(())
}

/// Verify that a resolved path stays within the expected base directory.
/// Uses canonicalization to resolve symlinks.
pub fn ensure_within(base: &Path, target: &Path) -> Result<PathBuf, CoreError> {
    // If target exists, canonicalize it directly
    if target.exists() {
        let resolved = target.canonicalize().map_err(CoreError::Io)?;
        let base_resolved = base.canonicalize().map_err(CoreError::Io)?;
        if !resolved.starts_with(&base_resolved) {
            return Err(CoreError::InvalidInput(
                "path escapes base directory".into(),
            ));
        }
        return Ok(resolved);
    }

    // If target doesn't exist, canonicalize the nearest existing parent
    let mut parent = target.parent();
    while let Some(p) = parent {
        if p.exists() {
            let resolved_parent = p.canonicalize().map_err(CoreError::Io)?;
            let base_resolved = base.canonicalize().map_err(CoreError::Io)?;
            if !resolved_parent.starts_with(&base_resolved) {
                return Err(CoreError::InvalidInput(
                    "path escapes base directory".into(),
                ));
            }
            return Ok(target.to_path_buf());
        }
        parent = p.parent();
    }

    // Could not resolve any parent — reject
    Err(CoreError::InvalidInput(
        "cannot resolve path ancestry".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Project name validation ──────────────────────────────────────

    #[test]
    fn test_valid_project_names() {
        assert!(validate_project_name("my-project").is_ok());
        assert!(validate_project_name("Project 123").is_ok());
        assert!(validate_project_name("notes").is_ok());
        assert!(validate_project_name("日記").is_ok());
    }

    #[test]
    fn test_empty_project_name() {
        assert!(validate_project_name("").is_err());
    }

    #[test]
    fn test_dotfile_project_name() {
        assert!(validate_project_name(".hidden").is_err());
        assert!(validate_project_name("..").is_err());
        assert!(validate_project_name(".").is_err());
    }

    #[test]
    fn test_path_traversal_project_name() {
        assert!(validate_project_name("../../etc").is_err());
        assert!(validate_project_name("foo/bar").is_err());
        assert!(validate_project_name("foo\\bar").is_err());
    }

    #[test]
    fn test_control_chars_project_name() {
        assert!(validate_project_name("hello\nworld").is_err());
        assert!(validate_project_name("hello\x00world").is_err());
    }

    #[test]
    fn test_reserved_names() {
        assert!(validate_project_name("CON").is_err());
        assert!(validate_project_name("con").is_err());
        assert!(validate_project_name("NUL").is_err());
        assert!(validate_project_name("COM1").is_err());
        assert!(validate_project_name("LPT3").is_err());
    }

    #[test]
    fn test_long_project_name() {
        let long = "a".repeat(256);
        assert!(validate_project_name(&long).is_err());
        let ok = "a".repeat(255);
        assert!(validate_project_name(&ok).is_ok());
    }

    // ── Relative path validation ─────────────────────────────────────

    #[test]
    fn test_valid_relative_paths() {
        assert!(validate_relative_path("hello.md").is_ok());
        assert!(validate_relative_path("notes/hello.md").is_ok());
        assert!(validate_relative_path("a/b/c/d.md").is_ok());
    }

    #[test]
    fn test_path_traversal_relative() {
        assert!(validate_relative_path("../evil.md").is_err());
        assert!(validate_relative_path("foo/../../bar.md").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
    }

    #[test]
    fn test_empty_path() {
        assert!(validate_relative_path("").is_err());
    }

    #[test]
    fn test_null_in_path() {
        assert!(validate_relative_path("hello\x00world.md").is_err());
    }

    #[test]
    fn test_deeply_nested_path() {
        let deep = (0..12)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join("/")
            + "/f.md";
        assert!(validate_relative_path(&deep).is_err());
    }

    // ── Note path validation ─────────────────────────────────────────

    #[test]
    fn test_valid_note_paths() {
        assert!(validate_note_path("hello.md").is_ok());
        assert!(validate_note_path("notes/hello.md").is_ok());
    }

    #[test]
    fn test_note_path_wrong_extension() {
        assert!(validate_note_path("hello.txt").is_err());
        assert!(validate_note_path("hello").is_err());
        assert!(validate_note_path("hello.md.bak").is_err());
    }

    // ── ensure_within ────────────────────────────────────────────────

    #[test]
    fn test_ensure_within_valid() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let target = base.join("project").join("notes");
        std::fs::create_dir_all(&target).unwrap();

        assert!(ensure_within(base, &target).is_ok());
    }

    #[test]
    fn test_ensure_within_escape() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("inner");
        std::fs::create_dir_all(&base).unwrap();

        // Try to escape to parent
        let evil = base.join("..").join("outside");
        std::fs::create_dir_all(dir.path().join("outside")).unwrap();

        assert!(ensure_within(&base, &evil).is_err());
    }
}
