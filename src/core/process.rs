use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// The kind of task being managed.
#[derive(Debug, Clone)]
pub enum TaskKind {
    /// A shell command task.
    Shell {
        /// The shell command string.
        command: String,
    },
    /// An agent-driven task (future use).
    Agent {
        /// The goal description for the agent.
        goal: String,
    },
}

/// Status of a managed task.
#[derive(Debug, Clone)]
pub enum TaskStatus {
    /// Task is currently running.
    Running,
    /// Task completed with an optional exit code.
    Completed {
        /// Process exit code, if available.
        exit_code: Option<i32>,
    },
    /// Task failed with an error message.
    Failed(String),
}

/// A managed task entry.
pub struct TaskEntry {
    /// Unique task identifier.
    pub id: u32,
    /// What kind of task this is.
    pub kind: TaskKind,
    /// Current status.
    pub status: TaskStatus,
    /// Path to the output log file.
    pub output_file: Option<PathBuf>,
    /// When the task was started.
    pub started_at: std::time::Instant,
    /// The underlying child process (if shell task).
    child: Option<tokio::process::Child>,
}

/// Registry of managed background tasks.
type TaskRegistry = Arc<Mutex<HashMap<u32, TaskEntry>>>;

/// Manage background processes and tasks: start, list, stop, status.
pub struct ProcessTool {
    config: CoreToolConfig,
    tasks: TaskRegistry,
    next_id: Arc<Mutex<u32>>,
}

impl ProcessTool {
    #[must_use]
    pub fn new(config: CoreToolConfig) -> Self {
        Self {
            config,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }
}

impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        "process"
    }

    fn description(&self) -> &'static str {
        "Manage background processes. Actions: start, list, stop, status."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "action",
                Schema::enum_values(&["start", "list", "stop", "status"]),
            )
            .property(
                "command",
                Schema::string().description("Shell command (for 'start' action)"),
            )
            .property(
                "pid",
                Schema::integer().description("Task ID (for 'stop'/'status' action)"),
            )
            .build()
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        false // process management has side effects
    }

    fn permission(&self) -> Permission {
        Permission::Ask
    }

    fn max_output_bytes(&self) -> usize {
        self.config.max_output_bytes
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>,
    > {
        Box::pin(self.execute_inner(args, ctx))
    }
}

impl ProcessTool {
    async fn execute_inner(
        &self,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'action'".into()))?;

        match action {
            "start" => self.start_task(&args, ctx).await,
            "list" => self.list_tasks().await,
            "stop" => self.stop_task(&args).await,
            "status" => self.task_status(&args).await,
            _ => Err(ToolError::Execution(format!(
                "unknown action: '{action}'"
            ))),
        }
    }

    async fn start_task(
        &self,
        args: &Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("'command' required for 'start' action".into()))?;

        // Allocate task ID
        let task_id = {
            let mut id = self.next_id.lock().await;
            let current = *id;
            *id += 1;
            current
        };

        // Create output directory and log file
        let tasks_dir = ctx.root.join(".cold").join("tasks");
        let _ = std::fs::create_dir_all(&tasks_dir);
        let log_path = tasks_dir.join(format!("{task_id}.log"));
        let log_file = std::fs::File::create(&log_path).map_err(|e| {
            ToolError::Execution(format!("cannot create log file: {e}"))
        })?;
        let stderr_file = log_file.try_clone().map_err(|e| {
            ToolError::Execution(format!("cannot clone log file handle: {e}"))
        })?;

        #[cfg(target_family = "unix")]
        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&ctx.cwd)
            .envs(&ctx.env)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .spawn()
            .map_err(|e| ToolError::Execution(format!("failed to spawn process: {e}")))?;

        #[cfg(target_family = "windows")]
        let child = tokio::process::Command::new("cmd")
            .arg("/C")
            .arg(command)
            .current_dir(&ctx.cwd)
            .envs(&ctx.env)
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .spawn()
            .map_err(|e| ToolError::Execution(format!("failed to spawn process: {e}")))?;

        let entry = TaskEntry {
            id: task_id,
            kind: TaskKind::Shell {
                command: command.to_string(),
            },
            status: TaskStatus::Running,
            output_file: Some(log_path.clone()),
            started_at: std::time::Instant::now(),
            child: Some(child),
        };

        self.tasks.lock().await.insert(task_id, entry);

        Ok(ToolResult::text(format!(
            "Started task #{task_id}: {command}\nLog: {}",
            log_path.display()
        )))
    }

    async fn list_tasks(&self) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;
        let mut tasks = self.tasks.lock().await;

        if tasks.is_empty() {
            return Ok(ToolResult::text("No managed tasks"));
        }

        let mut output = String::from("ID\tStatus\tElapsed\tCommand\n");

        for (id, entry) in tasks.iter_mut() {
            // Update status by checking child
            update_task_status(entry);

            let elapsed = entry.started_at.elapsed().as_secs();
            let status_str = format_status(&entry.status);
            let cmd_str = match &entry.kind {
                TaskKind::Shell { command } => command.as_str(),
                TaskKind::Agent { goal } => goal.as_str(),
            };
            let _ = writeln!(output, "{id}\t{status_str}\t{elapsed}s\t{cmd_str}");
        }

        // Clean up completed tasks older than 5 minutes
        let stale: Vec<u32> = tasks
            .iter()
            .filter(|(_, e)| {
                !matches!(e.status, TaskStatus::Running) && e.started_at.elapsed().as_secs() > 300
            })
            .map(|(&id, _)| id)
            .collect();
        for id in stale {
            tasks.remove(&id);
        }
        drop(tasks);

        Ok(ToolResult::text(output))
    }

    async fn stop_task(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let task_id = args["pid"]
            .as_u64()
            .ok_or_else(|| ToolError::Execution("'pid' required for 'stop' action".into()))?
            as u32;

        let mut tasks = self.tasks.lock().await;
        let Some(entry) = tasks.get_mut(&task_id) else {
            return Err(ToolError::Execution(format!(
                "no managed task with ID {task_id}"
            )));
        };

        if let Some(ref mut child) = entry.child {
            let _ = child.kill().await;
            entry.status = TaskStatus::Failed("killed by user".into());
        }
        entry.child = None;

        let cmd_str = match &entry.kind {
            TaskKind::Shell { command } => command.clone(),
            TaskKind::Agent { goal } => goal.clone(),
        };
        drop(tasks);

        Ok(ToolResult::text(format!(
            "Stopped task #{task_id}: {cmd_str}"
        )))
    }

    async fn task_status(&self, args: &Value) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let task_id = args["pid"]
            .as_u64()
            .ok_or_else(|| ToolError::Execution("'pid' required for 'status' action".into()))?
            as u32;

        let mut tasks = self.tasks.lock().await;
        let Some(entry) = tasks.get_mut(&task_id) else {
            return Err(ToolError::Execution(format!(
                "no managed task with ID {task_id}"
            )));
        };

        update_task_status(entry);

        let elapsed = entry.started_at.elapsed().as_secs();
        let status_str = format_status(&entry.status);
        let cmd_str = match &entry.kind {
            TaskKind::Shell { command } => command.clone(),
            TaskKind::Agent { goal } => goal.clone(),
        };
        let log_path = entry.output_file.clone();
        drop(tasks);

        let mut output = String::new();
        let _ = writeln!(output, "Task #{task_id}");
        let _ = writeln!(output, "Status: {status_str}");
        let _ = writeln!(output, "Elapsed: {elapsed}s");
        let _ = writeln!(output, "Command: {cmd_str}");

        // Read last 20 lines from output file
        if let Some(ref log_path) = log_path {
            let _ = writeln!(output, "Log: {}", log_path.display());

            if let Ok(content) = std::fs::read_to_string(log_path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(20);
                let tail: Vec<&str> = lines[start..].to_vec();

                if !tail.is_empty() {
                    let _ = writeln!(output, "\n--- last {} lines ---", tail.len());
                    for line in tail {
                        let _ = writeln!(output, "{line}");
                    }
                }
            }
        }

        Ok(ToolResult::text(output))
    }
}

/// Update a task's status by checking its child process.
fn update_task_status(entry: &mut TaskEntry) {
    if !matches!(entry.status, TaskStatus::Running) {
        return;
    }

    if let Some(ref mut child) = entry.child {
        match child.try_wait() {
            Ok(Some(exit)) => {
                entry.status = TaskStatus::Completed {
                    exit_code: exit.code(),
                };
            }
            Ok(None) => {} // still running
            Err(e) => {
                entry.status = TaskStatus::Failed(format!("check failed: {e}"));
            }
        }
    }
}

/// Format a task status for display.
fn format_status(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Running => "running".to_string(),
        TaskStatus::Completed { exit_code } => {
            format!("completed (exit: {})", exit_code.unwrap_or(-1))
        }
        TaskStatus::Failed(msg) => format!("failed: {msg}"),
    }
}
