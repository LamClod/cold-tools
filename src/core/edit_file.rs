use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Edit a file by replacing text occurrences.
pub struct EditFileTool {
    config: CoreToolConfig,
}

impl EditFileTool {
    #[must_use] 
    pub const fn new(config: CoreToolConfig) -> Self {
        Self { config }
    }
}

impl Tool for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Edit a file by replacing occurrences of old_string with new_string. By default requires exactly one match."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property("path", Schema::string().description("File path to edit"))
            .required_property(
                "old_string",
                Schema::string().description("Text to find and replace"),
            )
            .required_property(
                "new_string",
                Schema::string().description("Replacement text"),
            )
            .property(
                "replace_all",
                Schema::boolean()
                    .description("Replace all occurrences instead of requiring exactly one")
                    .default(false),
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

impl EditFileTool {
    async fn execute_inner(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'path'".into()))?;
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'old_string'".into()))?;
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'new_string'".into()))?;
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);

        let path = ctx.resolve_path(path_str)?;

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            ToolError::Execution(format!("cannot read '{}': {e}", path.display()))
        })?;

        let match_count = content.matches(old_string).count();

        if match_count == 0 {
            return Err(ToolError::Execution(format!(
                "old_string not found in '{}'",
                path.display()
            )));
        }

        if !replace_all && match_count > 1 {
            return Err(ToolError::Execution(format!(
                "old_string found {match_count} times in '{}' — use replace_all=true or provide a more specific string",
                path.display()
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        tokio::fs::write(&path, &new_content).await?;

        Ok(ToolResult::text(format!(
            "Replaced {match_count} occurrence(s) in '{}'",
            path.display()
        )))
    }
}

