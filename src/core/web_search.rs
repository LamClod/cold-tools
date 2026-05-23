use std::sync::Arc;

use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::providers::WebProvider;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Search the web via an injected `WebProvider`.
pub struct WebSearchTool {
    provider: Arc<dyn WebProvider>,
}

impl WebSearchTool {
    /// Create a new `WebSearchTool` backed by the given provider.
    #[must_use]
    pub fn new(provider: Arc<dyn WebProvider>) -> Self {
        Self { provider }
    }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn description(&self) -> &'static str {
        "Search the web and return a list of results with titles, URLs, and snippets."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "query",
                Schema::string().description("Search query"),
            )
            .property(
                "max_results",
                Schema::integer()
                    .description("Maximum number of results to return")
                    .default(10)
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

impl WebSearchTool {
    async fn execute_inner(&self, args: Value) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let query = args["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'query'".into()))?;

        let max_results = args["max_results"].as_u64().unwrap_or(10) as usize;

        let results = self.provider.search(query, max_results).await?;

        if results.is_empty() {
            return Ok(ToolResult::text(format!(
                "No results found for query: {query}"
            )));
        }

        let mut output = format!("Search results for \"{query}\":\n\n");
        for (i, result) in results.iter().enumerate() {
            let _ = writeln!(output, "{}. {}", i + 1, result.title);
            let _ = writeln!(output, "   URL: {}", result.url);
            let _ = writeln!(output, "   {}\n", result.snippet);
        }

        Ok(ToolResult::text(output))
    }
}
