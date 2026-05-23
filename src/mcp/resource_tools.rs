use std::sync::Arc;

use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

use super::McpTransport;

/// List available resources from an MCP server.
pub struct ListMcpResourcesTool {
    transport: Arc<dyn McpTransport>,
    server_name: String,
}

impl ListMcpResourcesTool {
    /// Create a new `ListMcpResourcesTool`.
    #[must_use]
    pub fn new(transport: Arc<dyn McpTransport>, server_name: String) -> Self {
        Self {
            transport,
            server_name,
        }
    }
}

impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &'static str {
        "mcp_list_resources"
    }

    fn description(&self) -> &'static str {
        "List available resources from the MCP server."
    }

    fn toolset(&self) -> &'static str {
        "mcp"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object().build()
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
        _args: Value,
        _ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(self.execute_inner())
    }
}

impl ListMcpResourcesTool {
    async fn execute_inner(&self) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let resources = self.transport.list_resources().await?;

        if resources.is_empty() {
            return Ok(ToolResult::text(format!(
                "No resources available from MCP server '{}'",
                self.server_name
            )));
        }

        let mut output = format!(
            "Resources from MCP server '{}' ({} total):\n\n",
            self.server_name,
            resources.len()
        );

        for res in &resources {
            let _ = writeln!(output, "- {} ({})", res.name, res.uri);
            if let Some(ref desc) = res.description {
                let _ = writeln!(output, "  {desc}");
            }
            if let Some(ref mime) = res.mime_type {
                let _ = writeln!(output, "  Type: {mime}");
            }
        }

        Ok(ToolResult::text(output))
    }
}

/// Read a specific resource from an MCP server by URI.
pub struct ReadMcpResourceTool {
    transport: Arc<dyn McpTransport>,
    server_name: String,
}

impl ReadMcpResourceTool {
    /// Create a new `ReadMcpResourceTool`.
    #[must_use]
    pub fn new(transport: Arc<dyn McpTransport>, server_name: String) -> Self {
        Self {
            transport,
            server_name,
        }
    }
}

impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &'static str {
        "mcp_read_resource"
    }

    fn description(&self) -> &'static str {
        "Read a specific resource from the MCP server by URI."
    }

    fn toolset(&self) -> &'static str {
        "mcp"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "uri",
                Schema::string().description("Resource URI to read"),
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

impl ReadMcpResourceTool {
    async fn execute_inner(&self, args: Value) -> Result<ToolResult, ToolError> {
        let uri = args["uri"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'uri'".into()))?;

        let content = self.transport.read_resource(uri).await?;

        Ok(ToolResult::text(format!(
            "Resource '{uri}' from MCP server '{}':\n\n{content}",
            self.server_name
        )))
    }
}
