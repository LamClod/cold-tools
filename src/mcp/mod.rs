pub mod adapter;
pub mod resource_tools;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP tool definition from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: Value,
}

/// MCP tool call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    /// Content blocks returned by the tool.
    pub content: Vec<McpContent>,
    /// Whether the result is an error.
    pub is_error: Option<bool>,
}

/// A single content block in an MCP result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpContent {
    /// Plain text content.
    #[serde(rename = "text")]
    Text {
        /// The text value.
        text: String,
    },
    /// Base64-encoded image content.
    #[serde(rename = "image")]
    Image {
        /// Base64-encoded image data.
        data: String,
        /// MIME type of the image.
        mime_type: String,
    },
}

/// An MCP resource exposed by a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    /// Resource URI.
    pub uri: String,
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional MIME type.
    pub mime_type: Option<String>,
}

/// Trait for MCP transport (stdio or SSE).
pub trait McpTransport: Send + Sync {
    /// List available tools from the MCP server.
    fn list_tools(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<McpToolDef>, crate::ToolError>> + Send + '_>,
    >;

    /// Call a tool on the MCP server.
    fn call_tool(
        &self,
        name: &str,
        args: Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<McpToolResult, crate::ToolError>> + Send + '_>,
    >;

    /// List available resources from the MCP server.
    fn list_resources(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<McpResource>, crate::ToolError>> + Send + '_>,
    >;

    /// Read a specific resource by URI.
    fn read_resource(
        &self,
        uri: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, crate::ToolError>> + Send + '_>,
    >;
}
