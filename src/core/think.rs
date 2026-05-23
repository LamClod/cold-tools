use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// A reasoning scratchpad tool. The thought lives in conversation context
/// but produces no output to the user.
pub struct ThinkTool;

impl Tool for ThinkTool {
    fn name(&self) -> &'static str {
        "think"
    }

    fn description(&self) -> &'static str {
        "Use this tool to think through a problem step-by-step. The content is not shown to the user but helps structure reasoning."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "thought",
                Schema::string().description("The reasoning content"),
            )
            .build()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }

    fn permission(&self) -> Permission {
        Permission::Auto
    }

    fn max_output_bytes(&self) -> usize {
        0
    }

    fn execute<'a>(&'a self, _args: Value, _ctx: &'a ToolContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>> {
        Box::pin(async { Ok(ToolResult::Empty) })
    }
}
