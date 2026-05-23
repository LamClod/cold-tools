use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::ToolError;

/// Trait for user interaction during tool execution.
///
/// Uses boxed futures to allow dyn dispatch.
pub trait UserInteraction: Send + Sync {
    /// Ask the user a question and return their response.
    fn ask(
        &self,
        question: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + '_>>;

    /// Ask the user for confirmation.
    fn confirm(
        &self,
        action: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>>;

    /// Notify the user of something (no response expected).
    fn notify(&self, message: &str);
}

/// Default no-op interaction that auto-confirms everything.
pub struct AutoApprove;

impl UserInteraction for AutoApprove {
    fn ask(
        &self,
        _question: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + '_>> {
        Box::pin(async { String::new() })
    }

    fn confirm(
        &self,
        _action: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async { true })
    }

    fn notify(&self, _message: &str) {}
}

/// Context passed to every tool execution.
pub struct ToolContext {
    /// Current working directory.
    pub cwd: PathBuf,
    /// Security root directory — paths must stay within this.
    pub root: PathBuf,
    /// Task identifier.
    pub task_id: String,
    /// User interaction handler.
    pub user: Arc<dyn UserInteraction>,
    /// Cancellation flag.
    pub cancelled: Arc<AtomicBool>,
    /// Environment variables for subprocesses.
    pub env: HashMap<String, String>,
    /// Plan mode flag — when true, write operations are blocked.
    pub plan_mode: Arc<AtomicBool>,
}

impl ToolContext {
    /// Check if execution has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Check if plan mode is active.
    #[must_use]
    pub fn is_plan_mode(&self) -> bool {
        self.plan_mode.load(Ordering::Relaxed)
    }

    /// Resolve a path relative to `cwd` and validate it is within `root`.
    ///
    /// Returns the canonicalized absolute path.
    pub fn resolve_path(&self, path: &str) -> Result<PathBuf, ToolError> {
        let candidate = if std::path::Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.cwd.join(path)
        };

        crate::security::validate_path(&candidate, &self.root)
    }
}

impl ToolContext {
    /// Create a new context with sensible defaults for testing.
    #[cfg(test)]
    pub fn test_context(cwd: PathBuf) -> Self {
        Self {
            root: cwd.clone(),
            cwd,
            task_id: "test".to_string(),
            user: Arc::new(AutoApprove),
            cancelled: Arc::new(AtomicBool::new(false)),
            env: HashMap::new(),
            plan_mode: Arc::new(AtomicBool::new(false)),
        }
    }
}
