use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Edit a cell in a Jupyter notebook (.ipynb) file.
pub struct NotebookEditTool;

impl Tool for NotebookEditTool {
    fn name(&self) -> &'static str {
        "notebook_edit"
    }

    fn description(&self) -> &'static str {
        "Edit a cell in a Jupyter notebook by replacing its source content."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "path",
                Schema::string().description("Path to the .ipynb file"),
            )
            .required_property(
                "cell_index",
                Schema::integer()
                    .description("0-based cell index to edit")
                    .minimum(0),
            )
            .required_property(
                "new_source",
                Schema::string().description("New source content for the cell"),
            )
            .build()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn permission(&self) -> Permission {
        Permission::Ask
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
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| ToolError::Execution("missing required parameter 'path'".into()))?;

    let cell_index = args["cell_index"]
        .as_u64()
        .ok_or_else(|| {
            ToolError::Execution("missing required parameter 'cell_index'".into())
        })? as usize;

    let new_source = args["new_source"]
        .as_str()
        .ok_or_else(|| {
            ToolError::Execution("missing required parameter 'new_source'".into())
        })?;

    let path = ctx.resolve_path(path_str)?;

    let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
        ToolError::Execution(format!("cannot read '{}': {e}", path.display()))
    })?;

    let mut notebook: Value = serde_json::from_str(&content).map_err(|e| {
        ToolError::Execution(format!("invalid notebook JSON: {e}"))
    })?;

    let cells = notebook
        .get_mut("cells")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            ToolError::Execution("notebook has no 'cells' array".into())
        })?;

    if cell_index >= cells.len() {
        return Err(ToolError::Execution(format!(
            "cell_index {cell_index} out of range (notebook has {} cells)",
            cells.len()
        )));
    }

    // Convert new_source to array of lines (notebook format)
    let source_lines: Vec<Value> = new_source
        .lines()
        .enumerate()
        .map(|(i, line)| {
            // All lines except the last get a trailing newline
            let total = new_source.lines().count();
            if i < total - 1 {
                Value::String(format!("{line}\n"))
            } else {
                Value::String(line.to_string())
            }
        })
        .collect();

    let source_lines = if source_lines.is_empty() {
        vec![Value::String(String::new())]
    } else {
        source_lines
    };

    cells[cell_index]["source"] = Value::Array(source_lines);

    let output = serde_json::to_string_pretty(&notebook)?;
    tokio::fs::write(&path, &output).await.map_err(|e| {
        ToolError::Execution(format!("cannot write '{}': {e}", path.display()))
    })?;

    Ok(ToolResult::text(format!(
        "Updated cell {cell_index} in '{}'",
        path.display()
    )))
}
