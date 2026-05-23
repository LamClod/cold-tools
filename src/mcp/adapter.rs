use std::sync::Arc;

use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::registry::ToolRegistry;
use crate::tool::{Permission, Tool, ToolResult};

use super::{McpContent, McpToolDef, McpTransport};

/// Adapts an MCP tool to the cold `Tool` trait.
pub struct McpToolAdapter {
    def: McpToolDef,
    transport: Arc<dyn McpTransport>,
    server_name: String,
}

impl McpToolAdapter {
    /// Create a new MCP tool adapter.
    #[must_use]
    pub fn new(def: McpToolDef, transport: Arc<dyn McpTransport>, server_name: String) -> Self {
        Self {
            def,
            transport,
            server_name,
        }
    }

    /// Get the MCP server name this tool belongs to.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.def.name
    }

    fn description(&self) -> &str {
        self.def
            .description
            .as_deref()
            .unwrap_or("MCP tool (no description)")
    }

    fn toolset(&self) -> &'static str {
        "mcp"
    }

    fn parameters_schema(&self) -> Value {
        self.def.input_schema.clone()
    }

    fn is_read_only(&self) -> bool {
        false // conservative: MCP tools may have side effects
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        false // conservative
    }

    fn permission(&self) -> Permission {
        Permission::Ask // MCP tools always ask
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

impl McpToolAdapter {
    async fn execute_inner(&self, args: Value) -> Result<ToolResult, ToolError> {
        let mcp_result = self.transport.call_tool(&self.def.name, args).await?;

        // Check if the MCP server reported an error
        if mcp_result.is_error == Some(true) {
            let message = mcp_result
                .content
                .iter()
                .filter_map(|c| match c {
                    McpContent::Text { text } => Some(text.as_str()),
                    McpContent::Image { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(ToolResult::error(message, true));
        }

        // Convert MCP content blocks to text
        let mut parts = Vec::new();
        for block in &mcp_result.content {
            match block {
                McpContent::Text { text } => {
                    parts.push(text.clone());
                }
                McpContent::Image { data, mime_type } => {
                    parts.push(format!("[Image: {mime_type}, {} bytes base64]", data.len()));
                }
            }
        }

        if parts.is_empty() {
            Ok(ToolResult::Empty)
        } else {
            Ok(ToolResult::text(parts.join("\n")))
        }
    }
}

/// Register all tools from an MCP server into the registry.
///
/// Returns the number of tools registered.
pub async fn register_mcp_tools(
    transport: Arc<dyn McpTransport>,
    server_name: &str,
    registry: &mut ToolRegistry,
) -> Result<usize, ToolError> {
    let tools = transport.list_tools().await?;
    let count = tools.len();

    for def in tools {
        let adapter = McpToolAdapter::new(def, Arc::clone(&transport), server_name.to_string());
        registry.register(adapter);
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpToolDef, McpToolResult};
    use serde_json::json;

    struct MockTransport {
        tools: Vec<McpToolDef>,
    }

    impl McpTransport for MockTransport {
        fn list_tools(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<McpToolDef>, ToolError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(self.tools.clone()) })
        }

        fn call_tool(
            &self,
            _name: &str,
            _args: Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<McpToolResult, ToolError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Ok(McpToolResult {
                    content: vec![McpContent::Text {
                        text: "mock result".into(),
                    }],
                    is_error: None,
                })
            })
        }

        fn list_resources(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<crate::mcp::McpResource>, ToolError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn read_resource(
            &self,
            _uri: &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, ToolError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(String::new()) })
        }
    }

    #[tokio::test]
    async fn test_register_mcp_tools() {
        let transport = Arc::new(MockTransport {
            tools: vec![
                McpToolDef {
                    name: "mcp_read".into(),
                    description: Some("Read via MCP".into()),
                    input_schema: json!({"type": "object"}),
                },
                McpToolDef {
                    name: "mcp_write".into(),
                    description: None,
                    input_schema: json!({"type": "object"}),
                },
            ],
        });

        let mut registry = ToolRegistry::new();
        let count = register_mcp_tools(transport, "test-server", &mut registry)
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert!(registry.get("mcp_read").is_some());
        assert!(registry.get("mcp_write").is_some());
    }

    #[tokio::test]
    async fn test_mcp_tool_adapter_execute() {
        let transport = Arc::new(MockTransport { tools: vec![] });
        let def = McpToolDef {
            name: "test_tool".into(),
            description: Some("Test".into()),
            input_schema: json!({"type": "object"}),
        };
        let adapter = McpToolAdapter::new(def, transport, "srv".into());
        let result = adapter.execute_inner(json!({})).await.unwrap();
        assert_eq!(result.as_text(), "mock result");
    }
}
