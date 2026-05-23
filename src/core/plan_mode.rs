use std::sync::atomic::Ordering;

use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Enter plan mode, blocking write operations.
pub struct EnterPlanModeTool;

impl Tool for EnterPlanModeTool {
    fn name(&self) -> &'static str {
        "enter_plan_mode"
    }

    fn description(&self) -> &'static str {
        "Enter plan mode. Write operations will be blocked until exit_plan_mode is called."
    }

    fn toolset(&self) -> &'static str {
        "core"
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
        ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(async move {
            ctx.plan_mode.store(true, Ordering::Relaxed);
            Ok(ToolResult::text(
                "Entered plan mode. Write operations are now blocked.",
            ))
        })
    }
}

/// Exit plan mode, allowing write operations again.
pub struct ExitPlanModeTool;

impl Tool for ExitPlanModeTool {
    fn name(&self) -> &'static str {
        "exit_plan_mode"
    }

    fn description(&self) -> &'static str {
        "Exit plan mode. Write operations are now allowed."
    }

    fn toolset(&self) -> &'static str {
        "core"
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
        ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(async move {
            ctx.plan_mode.store(false, Ordering::Relaxed);
            Ok(ToolResult::text(
                "Exited plan mode. Write operations are now allowed.",
            ))
        })
    }
}
