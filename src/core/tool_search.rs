use std::sync::Arc;

use serde_json::Value;

use crate::context::ToolContext;
use crate::deferred::DeferredRegistry;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Meta-tool that lets the model discover deferred (lazily-loaded) tools.
pub struct ToolSearchTool {
    deferred: Arc<DeferredRegistry>,
}

impl ToolSearchTool {
    /// Create a new `ToolSearchTool` backed by the given deferred registry.
    #[must_use]
    pub const fn new(deferred: Arc<DeferredRegistry>) -> Self {
        Self { deferred }
    }
}

impl Tool for ToolSearchTool {
    fn name(&self) -> &'static str {
        "tool_search"
    }

    fn description(&self) -> &'static str {
        "Search for available deferred tools by keyword. Returns matching tool names and descriptions."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "query",
                Schema::string().description("Search query to find tools by keyword"),
            )
            .build()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn permission(&self) -> Permission {
        Permission::Auto
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(self.execute_inner(args))
    }
}

impl ToolSearchTool {
    #[allow(clippy::unused_async)]
    async fn execute_inner(&self, args: Value) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let query = args["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'query'".into()))?;

        let matches = self.deferred.search(query);

        if matches.is_empty() {
            return Ok(ToolResult::text(format!(
                "No deferred tools match query '{query}'"
            )));
        }

        let mut output = format!("Found {} matching tool(s):\n\n", matches.len());
        for tool in &matches {
            let _ = writeln!(output, "- **{}**: {}", tool.name, tool.description);
            if !tool.search_hints.is_empty() {
                let _ = writeln!(output, "  hints: {}", tool.search_hints.join(", "));
            }
        }

        Ok(ToolResult::text(output))
    }
}
