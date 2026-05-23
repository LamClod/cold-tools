use std::sync::Arc;

use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::providers::WebProvider;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Fetch web page content via an injected `WebProvider`.
pub struct WebFetchTool {
    provider: Arc<dyn WebProvider>,
}

impl WebFetchTool {
    /// Create a new `WebFetchTool` backed by the given provider.
    #[must_use]
    pub fn new(provider: Arc<dyn WebProvider>) -> Self {
        Self { provider }
    }
}

impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch web page content from a URL. Returns text content with HTML tags stripped."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "url",
                Schema::string().description("URL to fetch"),
            )
            .property(
                "max_length",
                Schema::integer()
                    .description("Maximum content length in characters")
                    .default(50000)
                    .minimum(1),
            )
            .property(
                "selector",
                Schema::string()
                    .description("CSS selector to filter content (informational)"),
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

impl WebFetchTool {
    async fn execute_inner(&self, args: Value) -> Result<ToolResult, ToolError> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'url'".into()))?;

        let max_length = args["max_length"].as_u64().unwrap_or(50_000) as usize;
        let selector = args["selector"].as_str();

        let mut content = self.provider.fetch(url, max_length).await?;

        // Truncate if still over limit
        if content.len() > max_length {
            content.truncate(max_length);
            content.push_str("\n[truncated]");
        }

        let mut output = format!("Content from {url}:\n\n{content}");

        if let Some(sel) = selector {
            output.push_str(&format!("\n\n[Note: CSS selector '{sel}' was requested]"));
        }

        Ok(ToolResult::text(output))
    }
}
