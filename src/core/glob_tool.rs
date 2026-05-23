use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Fast file pattern matching via glob patterns, sorted by modification time.
pub struct GlobTool {
    config: CoreToolConfig,
}

impl GlobTool {
    #[must_use]
    pub const fn new(config: CoreToolConfig) -> Self {
        Self { config }
    }
}

impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> &'static str {
        "Find files matching a glob pattern, sorted by modification time (newest first)."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "pattern",
                Schema::string().description("Glob pattern (e.g. \"**/*.rs\")"),
            )
            .property(
                "path",
                Schema::string()
                    .description("Base directory to search in")
                    .default("."),
            )
            .property(
                "max_results",
                Schema::integer()
                    .description("Maximum number of results to return")
                    .default(100)
                    .minimum(1),
            )
            .build()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }

    fn permission(&self) -> Permission {
        Permission::Auto
    }

    fn max_output_bytes(&self) -> usize {
        self.config.max_output_bytes
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(self.execute_inner(args, ctx))
    }
}

impl GlobTool {
    #[allow(clippy::unused_async)]
    async fn execute_inner(
        &self,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let pattern_str = args["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'pattern'".into()))?;

        let base_path = args["path"].as_str().unwrap_or(".");
        let max_results = args["max_results"].as_u64().unwrap_or(100) as usize;

        let resolved_base = ctx.resolve_path(base_path)?;

        // Build the full glob pattern
        let full_pattern = resolved_base.join(pattern_str);
        let full_pattern_str = full_pattern.to_string_lossy();

        let paths = glob::glob(&full_pattern_str).map_err(|e| {
            ToolError::Execution(format!("invalid glob pattern: {e}"))
        })?;

        // Collect matching paths with modification times
        let mut entries: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in paths {
            let Ok(path) = entry else { continue };

            // Validate path is within root
            if crate::security::validate_path(&path, &ctx.root).is_err() {
                continue;
            }

            let mtime = path
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            entries.push((path, mtime));
        }

        // Sort by modification time, newest first
        entries.sort_by_key(|e| std::cmp::Reverse(e.1));

        // Truncate to max_results
        entries.truncate(max_results);

        if entries.is_empty() {
            return Ok(ToolResult::text(format!(
                "No files matching pattern '{pattern_str}' in '{}'",
                resolved_base.display()
            )));
        }

        let mut output = format!("Found {} file(s):\n", entries.len());
        for (path, _) in &entries {
            // Show relative path from base
            let display = path
                .strip_prefix(&resolved_base)
                .unwrap_or(path);
            let _ = writeln!(output, "{}", display.display());
        }

        Ok(ToolResult::text(output))
    }
}
