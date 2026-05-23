use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Ask the user a question and return their response.
pub struct AskUserTool;

impl Tool for AskUserTool {
    fn name(&self) -> &'static str {
        "ask_user"
    }

    fn description(&self) -> &'static str {
        "Ask the user a question and return their answer. Supports free-text and multiple-choice."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "question",
                Schema::string().description("The question to ask the user"),
            )
            .property(
                "options",
                Schema::array(Schema::string().into())
                    .description("Optional list of choices for multiple-choice questions"),
            )
            .build()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        false // user interaction is inherently sequential
    }

    fn permission(&self) -> Permission {
        Permission::Auto
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(execute_inner(args, ctx))
    }
}

async fn execute_inner(args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
    use std::fmt::Write;

    let question = args["question"]
        .as_str()
        .ok_or_else(|| ToolError::Execution("missing required parameter 'question'".into()))?;

    let options = args["options"].as_array();

    let prompt = options.map_or_else(
        || question.to_string(),
        |opts| {
            let mut formatted = String::from(question);
            formatted.push('\n');
            for (i, opt) in opts.iter().enumerate() {
                if let Some(s) = opt.as_str() {
                    let _ = writeln!(formatted, "  {}. {s}", i + 1);
                }
            }
            formatted
        },
    );

    let answer = ctx.user.ask(&prompt).await;
    Ok(ToolResult::text(answer))
}
