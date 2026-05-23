use std::path::{Path, PathBuf};

use crate::error::ToolError;

/// Level of danger detected in a shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DangerLevel {
    /// Potentially dangerous — user should be warned.
    Warning(String),
    /// Critically dangerous — should be blocked or require explicit confirmation.
    Critical(String),
}

/// Validate that `path` resolves to a location within `root`.
///
/// Returns the canonicalized path on success.
pub fn validate_path(path: &Path, root: &Path) -> Result<PathBuf, ToolError> {
    // For paths that don't exist yet, we canonicalize the existing ancestor
    // and append the remaining components.
    let canonical = if path.exists() {
        path.canonicalize().map_err(|e| {
            ToolError::PathViolation(format!("cannot canonicalize '{}': {e}", path.display()))
        })?
    } else {
        // Walk up to find the deepest existing ancestor
        let mut existing = path.to_path_buf();
        let mut remainder = Vec::new();
        while !existing.exists() {
            if let Some(file_name) = existing.file_name() {
                remainder.push(file_name.to_os_string());
            } else {
                break;
            }
            if !existing.pop() {
                break;
            }
        }
        let mut canonical = existing.canonicalize().map_err(|e| {
            ToolError::PathViolation(format!(
                "cannot canonicalize ancestor of '{}': {e}",
                path.display()
            ))
        })?;
        for component in remainder.into_iter().rev() {
            canonical.push(component);
        }
        canonical
    };

    let canonical_root = root.canonicalize().map_err(|e| {
        ToolError::PathViolation(format!("cannot canonicalize root '{}': {e}", root.display()))
    })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(ToolError::PathViolation(format!(
            "path '{}' is outside root '{}'",
            canonical.display(),
            canonical_root.display()
        )));
    }

    if is_device_path(&canonical) {
        return Err(ToolError::PathViolation(format!(
            "device path not allowed: '{}'",
            canonical.display()
        )));
    }

    if is_sensitive_path(&canonical) {
        return Err(ToolError::PathViolation(format!(
            "sensitive path not allowed: '{}'",
            canonical.display()
        )));
    }

    Ok(canonical)
}

/// Check for directory traversal patterns in a path string.
#[must_use] 
pub fn has_traversal(path: &str) -> bool {
    path.contains("..") || path.contains('~')
}

/// Check if a path refers to a device file.
#[must_use] 
pub fn is_device_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with("/dev/")
        || s.starts_with("/proc/")
        || s.starts_with("/sys/")
        || s.contains("\\\\.\\")
}

/// Check if a path targets a sensitive directory.
#[must_use] 
pub fn is_sensitive_path(path: &Path) -> bool {
    let sensitive = [
        ".ssh",
        ".gnupg",
        ".gpg",
        ".aws",
        ".azure",
        ".kube",
        ".docker",
        ".config/gcloud",
        ".npmrc",
        ".pypirc",
        ".netrc",
        ".env",
        ".credentials",
    ];

    let s = path.to_string_lossy();
    for pattern in &sensitive {
        if s.contains(pattern) {
            return true;
        }
    }
    false
}

/// Detect dangerous shell commands. Returns `None` if the command is safe.
#[must_use] 
pub fn detect_dangerous_command(cmd: &str) -> Option<DangerLevel> {
    let cmd_lower = cmd.to_lowercase();
    let cmd_trimmed = cmd_lower.trim();

    // Critical patterns — these can destroy systems
    let critical_patterns: &[(&str, &str)] = &[
        (":(){ :|:&", "fork bomb"),
        ("mkfs", "filesystem format"),
        ("format c:", "filesystem format"),
    ];

    for &(pattern, desc) in critical_patterns {
        if cmd_trimmed.contains(pattern) {
            return Some(DangerLevel::Critical(format!(
                "detected {desc}: '{cmd}'"
            )));
        }
    }

    // rm -rf with dangerous targets
    if cmd_trimmed.contains("rm ") && cmd_trimmed.contains("-rf") {
        if cmd_trimmed.contains(" /")
            && !cmd_trimmed.contains(" /.")
            && !cmd_trimmed.contains(" /h")
            && !cmd_trimmed.contains(" /t")
            && !cmd_trimmed.contains(" /v")
        {
            // Probably targeting root-level path
            if cmd_trimmed.contains("rm -rf /")
                || cmd_trimmed.contains("rm -rf /*")
                || cmd_trimmed.contains("rm -rf ~")
            {
                return Some(DangerLevel::Critical(format!(
                    "destructive rm detected: '{cmd}'"
                )));
            }
        }
        if cmd_trimmed.contains(" ~") || cmd_trimmed.ends_with(" /") {
            return Some(DangerLevel::Critical(format!(
                "destructive rm detected: '{cmd}'"
            )));
        }
    }

    // dd with if= (raw disk write)
    if cmd_trimmed.starts_with("dd ") && cmd_trimmed.contains("if=") {
        return Some(DangerLevel::Critical(format!(
            "raw disk operation detected: '{cmd}'"
        )));
    }

    // Warning patterns
    let warning_patterns: &[(&str, &str)] = &[
        ("git push -f", "force push"),
        ("git push --force", "force push"),
        ("git reset --hard", "hard reset"),
        ("chmod 777", "world-writable permissions"),
        ("curl|sh", "pipe to shell"),
        ("curl |sh", "pipe to shell"),
        ("curl | sh", "pipe to shell"),
        ("wget|sh", "pipe to shell"),
        ("wget |sh", "pipe to shell"),
        ("wget | sh", "pipe to shell"),
        ("curl|bash", "pipe to shell"),
        ("curl | bash", "pipe to shell"),
        ("wget|bash", "pipe to shell"),
        ("wget | bash", "pipe to shell"),
        ("> /dev/sda", "raw disk write"),
        ("shutdown", "system shutdown"),
        ("reboot", "system reboot"),
    ];

    for &(pattern, desc) in warning_patterns {
        if cmd_trimmed.contains(pattern) {
            return Some(DangerLevel::Warning(format!(
                "detected {desc}: '{cmd}'"
            )));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_traversal() {
        assert!(has_traversal("../etc/passwd"));
        assert!(has_traversal("~/secrets"));
        assert!(!has_traversal("src/main.rs"));
    }

    #[test]
    fn test_is_device_path() {
        assert!(is_device_path(Path::new("/dev/zero")));
        assert!(is_device_path(Path::new("/proc/self/mem")));
        assert!(!is_device_path(Path::new("/home/user/file.txt")));
    }

    #[test]
    fn test_is_sensitive_path() {
        assert!(is_sensitive_path(Path::new("/home/user/.ssh/id_rsa")));
        assert!(is_sensitive_path(Path::new("/home/user/.aws/credentials")));
        assert!(!is_sensitive_path(Path::new("/home/user/code/main.rs")));
    }

    #[test]
    fn test_dangerous_commands() {
        assert!(matches!(
            detect_dangerous_command("rm -rf /"),
            Some(DangerLevel::Critical(_))
        ));
        assert!(matches!(
            detect_dangerous_command("dd if=/dev/zero of=/dev/sda"),
            Some(DangerLevel::Critical(_))
        ));
        assert!(matches!(
            detect_dangerous_command("git push -f origin main"),
            Some(DangerLevel::Warning(_))
        ));
        assert!(matches!(
            detect_dangerous_command("chmod 777 /tmp/file"),
            Some(DangerLevel::Warning(_))
        ));
        assert!(detect_dangerous_command("ls -la").is_none());
    }
}
