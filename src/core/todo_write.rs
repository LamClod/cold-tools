use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Manage a per-project TODO list stored as JSON.
pub struct TodoWriteTool;

impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str {
        "todo_write"
    }

    fn description(&self) -> &'static str {
        "Manage a TODO list. Actions: add, update, remove, list, clear."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "action",
                Schema::string()
                    .description("Action to perform")
                    .enum_values(&["add", "update", "remove", "list", "clear"]),
            )
            .property(
                "task",
                Schema::string().description("Task text (for 'add' action)"),
            )
            .property(
                "id",
                Schema::integer().description("Task ID (for 'update'/'remove' action)"),
            )
            .property(
                "status",
                Schema::string()
                    .description("New status (for 'update' action)")
                    .enum_values(&["pending", "in_progress", "done", "blocked"]),
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

/// On-disk representation of a single TODO item.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct TodoItem {
    id: u64,
    text: String,
    status: String,
    created_at: String,
}

/// The full TODO file structure.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
struct TodoFile {
    next_id: u64,
    tasks: Vec<TodoItem>,
}

fn todo_path(ctx: &ToolContext) -> std::path::PathBuf {
    ctx.root.join(".cold").join("todo.json")
}

fn load_todo(ctx: &ToolContext) -> TodoFile {
    let path = todo_path(ctx);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(TodoFile {
            next_id: 1,
            tasks: Vec::new(),
        })
}

fn save_todo(ctx: &ToolContext, file: &TodoFile) -> Result<(), ToolError> {
    let path = todo_path(ctx);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(file)?;
    std::fs::write(&path, json)?;
    Ok(())
}

#[allow(clippy::unused_async)]
async fn execute_inner(args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
    use std::fmt::Write;

    let action = args["action"]
        .as_str()
        .ok_or_else(|| ToolError::Execution("missing required parameter 'action'".into()))?;

    match action {
        "list" => {
            let file = load_todo(ctx);
            if file.tasks.is_empty() {
                return Ok(ToolResult::text("No tasks."));
            }
            let mut output = String::from("Tasks:\n");
            for item in &file.tasks {
                let _ = writeln!(
                    output,
                    "  #{}: [{}] {}",
                    item.id, item.status, item.text
                );
            }
            Ok(ToolResult::text(output))
        }
        "add" => {
            let text = args["task"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::Execution("'task' required for 'add' action".into())
                })?;

            let mut file = load_todo(ctx);
            let id = file.next_id;
            file.next_id += 1;

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());

            file.tasks.push(TodoItem {
                id,
                text: text.to_string(),
                status: "pending".to_string(),
                created_at: now.to_string(),
            });

            save_todo(ctx, &file)?;
            Ok(ToolResult::text(format!("Added task #{id}: {text}")))
        }
        "update" => {
            let id = args["id"]
                .as_u64()
                .ok_or_else(|| {
                    ToolError::Execution("'id' required for 'update' action".into())
                })?;

            let status = args["status"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::Execution("'status' required for 'update' action".into())
                })?;

            let mut file = load_todo(ctx);
            let item = file.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| {
                ToolError::Execution(format!("task #{id} not found"))
            })?;

            item.status = status.to_string();
            save_todo(ctx, &file)?;
            Ok(ToolResult::text(format!(
                "Updated task #{id} to status '{status}'"
            )))
        }
        "remove" => {
            let id = args["id"]
                .as_u64()
                .ok_or_else(|| {
                    ToolError::Execution("'id' required for 'remove' action".into())
                })?;

            let mut file = load_todo(ctx);
            let before = file.tasks.len();
            file.tasks.retain(|t| t.id != id);
            if file.tasks.len() == before {
                return Err(ToolError::Execution(format!("task #{id} not found")));
            }
            save_todo(ctx, &file)?;
            Ok(ToolResult::text(format!("Removed task #{id}")))
        }
        "clear" => {
            let file = TodoFile {
                next_id: 1,
                tasks: Vec::new(),
            };
            save_todo(ctx, &file)?;
            Ok(ToolResult::text("All tasks cleared."))
        }
        _ => Err(ToolError::Execution(format!(
            "unknown action: '{action}'"
        ))),
    }
}
