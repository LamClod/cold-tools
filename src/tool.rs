use crate::context::ToolContext;
use crate::error::ToolError;

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub enum ToolResult {
    /// Plain text output.
    Text(String),
    /// Structured JSON output.
    Json(serde_json::Value),
    /// Error result returned to the model (not a Rust error).
    Error {
        message: String,
        recoverable: bool,
    },
    /// No output (e.g., think tool).
    Empty,
}

impl ToolResult {
    /// Create a text result.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Create an error result returned to the model.
    pub fn error(msg: impl Into<String>, recoverable: bool) -> Self {
        Self::Error {
            message: msg.into(),
            recoverable,
        }
    }

    /// Truncate output to `max_bytes`, appending `[truncated]` if needed.
    ///
    /// Uses safe char-boundary truncation.
    pub fn truncate(&mut self, max_bytes: usize) {
        match self {
            Self::Text(s) => {
                if s.len() > max_bytes {
                    let truncated = safe_truncate(s, max_bytes);
                    *s = format!("{truncated}\n[truncated]");
                }
            }
            Self::Json(v) => {
                let s = v.to_string();
                if s.len() > max_bytes {
                    let truncated = safe_truncate(&s, max_bytes);
                    *self = Self::Text(format!("{truncated}\n[truncated]"));
                }
            }
            Self::Error { .. } | Self::Empty => {}
        }
    }

    /// Get text content (for both `Text` and `Json` variants).
    #[must_use] 
    pub fn as_text(&self) -> &str {
        match self {
            Self::Text(s) | Self::Error { message: s, .. } => s,
            Self::Json(_) => "<json>",
            Self::Empty => "",
        }
    }
}

/// Permission level required for tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Always auto-approve.
    Auto,
    /// Ask user before execution.
    Ask,
    /// Require explicit confirmation.
    Confirm,
}

/// The core tool trait. All tools implement this.
///
/// Uses native async fn in trait (edition 2024).
pub trait Tool: Send + Sync {
    /// Unique tool name.
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// Toolset this tool belongs to.
    fn toolset(&self) -> &'static str {
        "default"
    }

    /// JSON Schema for the tool's parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given arguments.
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>>;

    /// Whether the tool is currently available.
    fn is_available(&self) -> bool {
        true
    }

    /// Whether the tool is read-only (no side effects).
    fn is_read_only(&self) -> bool {
        false
    }

    /// Whether this tool call is safe to run concurrently with other calls.
    ///
    /// Defaults to `is_read_only()`.  Override for finer-grained control
    /// (e.g. file tools that operate on non-overlapping paths).
    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        self.is_read_only()
    }

    /// Maximum output bytes before truncation.
    fn max_output_bytes(&self) -> usize {
        50_000
    }

    /// Permission level required for execution.
    fn permission(&self) -> Permission {
        Permission::Auto
    }

    /// Timeout in seconds.
    fn timeout_secs(&self) -> u64 {
        120
    }

    /// Whether this tool should be deferred (not shown in initial prompt).
    /// Deferred tools are discovered via `ToolSearch`.
    fn should_defer(&self) -> bool {
        false
    }

    /// Whether this tool must always be loaded (never deferred).
    fn always_load(&self) -> bool {
        true
    }

    /// Keywords for `ToolSearch` discovery when deferred.
    fn search_hints(&self) -> Vec<String> {
        vec![]
    }
}

/// Truncate a string to at most `max_bytes`, respecting char boundaries.
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest char boundary <= max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_truncate_ascii() {
        let s = "hello world";
        assert_eq!(safe_truncate(s, 5), "hello");
    }

    #[test]
    fn test_safe_truncate_multibyte() {
        let s = "hello 你好";
        // "hello " is 6 bytes, "你" is 3 bytes = 9, "好" is 3 bytes = 12
        assert_eq!(safe_truncate(s, 7), "hello ");
        assert_eq!(safe_truncate(s, 9), "hello 你");
    }

    #[test]
    fn test_truncate_result() {
        let mut result = ToolResult::text("abcdefghij");
        result.truncate(5);
        assert_eq!(result.as_text(), "abcde\n[truncated]");
    }
}
