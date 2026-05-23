use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Create or overwrite a file, auto-creating parent directories.
pub struct WriteFileTool {
    config: CoreToolConfig,
}

impl WriteFileTool {
    #[must_use] 
    pub const fn new(config: CoreToolConfig) -> Self {
        Self { config }
    }
}

impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Create or overwrite a file. Parent directories are created automatically."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property("path", Schema::string().description("File path to write"))
            .required_property(
                "content",
                Schema::string().description("Content to write to the file"),
            )
            .build()
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        // Path-overlap safety is checked by the parallel scheduler.
        true
    }

    fn permission(&self) -> Permission {
        Permission::Ask
    }

    fn max_output_bytes(&self) -> usize {
        self.config.max_output_bytes
    }

    fn execute<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>> {
        Box::pin(self.execute_inner(args, ctx))
    }
}

impl WriteFileTool {
    async fn execute_inner(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'path'".into()))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'content'".into()))?;

        let path = ctx.resolve_path(path_str)?;

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let byte_count = content.len();
        tokio::fs::write(&path, content).await?;

        Ok(ToolResult::text(format!(
            "Wrote {byte_count} bytes to '{}'",
            path.display()
        )))
    }
}
