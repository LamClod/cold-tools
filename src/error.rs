use std::fmt;

/// Errors that can occur during tool execution.
#[derive(Debug)]
pub enum ToolError {
    /// Tool execution failed.
    Execution(String),
    /// Tool not found in registry.
    NotFound(String),
    /// Permission denied.
    PermissionDenied(String),
    /// Blocked by guardrails.
    Blocked(String),
    /// Timed out.
    Timeout {
        tool: String,
        timeout_secs: u64,
    },
    /// Path security violation.
    PathViolation(String),
    /// IO error.
    Io(std::io::Error),
    /// JSON error.
    Json(serde_json::Error),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execution(msg) => write!(f, "tool execution failed: {msg}"),
            Self::NotFound(name) => write!(f, "tool not found: {name}"),
            Self::PermissionDenied(msg) => write!(f, "permission denied: {msg}"),
            Self::Blocked(msg) => write!(f, "blocked by guardrails: {msg}"),
            Self::Timeout { tool, timeout_secs } => {
                write!(f, "tool '{tool}' timed out after {timeout_secs}s")
            }
            Self::PathViolation(msg) => write!(f, "path security violation: {msg}"),
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
        }
    }
}

impl std::error::Error for ToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ToolError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}
