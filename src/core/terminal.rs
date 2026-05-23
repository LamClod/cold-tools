use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::security::{DangerLevel, detect_dangerous_command};
use crate::tool::{Permission, Tool, ToolResult};

/// Sandbox output limit (10 KB vs normal 50 KB).
const SANDBOX_MAX_OUTPUT: usize = 10_000;

/// Patterns that are always blocked in sandbox mode, regardless of confirmation.
const SANDBOX_BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf",
    "mkfs",
    "dd if=",
    "format c:",
    ":(){ :|:",
    "shutdown",
    "reboot",
    "curl|sh",
    "curl | sh",
    "curl|bash",
    "curl | bash",
    "wget|sh",
    "wget | sh",
    "wget|bash",
    "wget | bash",
    "> /dev/sda",
    "chmod 777",
];

/// Execute shell commands with timeout, output limits, and danger detection.
pub struct TerminalTool {
    config: CoreToolConfig,
    sandbox: bool,
}

impl TerminalTool {
    #[must_use]
    pub const fn new(config: CoreToolConfig) -> Self {
        Self {
            config,
            sandbox: false,
        }
    }

    /// Create a terminal tool with sandbox mode enabled.
    #[must_use]
    pub const fn new_sandboxed(config: CoreToolConfig) -> Self {
        Self {
            config,
            sandbox: true,
        }
    }
}

impl Tool for TerminalTool {
    fn name(&self) -> &'static str {
        "terminal"
    }

    fn description(&self) -> &'static str {
        "Execute a shell command. Detects dangerous commands and applies timeouts."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "command",
                Schema::string().description("Shell command to execute"),
            )
            .property(
                "timeout",
                Schema::integer()
                    .description("Override default timeout in seconds")
                    .minimum(1),
            )
            .build()
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        false // shell commands have arbitrary side effects
    }

    fn permission(&self) -> Permission {
        Permission::Ask
    }

    fn max_output_bytes(&self) -> usize {
        if self.sandbox {
            SANDBOX_MAX_OUTPUT
        } else {
            self.config.max_output_bytes
        }
    }

    fn timeout_secs(&self) -> u64 {
        self.config.terminal_timeout
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

impl TerminalTool {
    async fn execute_inner(
        &self,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'command'".into()))?;
        let timeout_override = args["timeout"].as_u64();

        // Sandbox: block dangerous patterns unconditionally
        if self.sandbox {
            let cmd_lower = command.to_lowercase();
            for &pattern in SANDBOX_BLOCKED_PATTERNS {
                if cmd_lower.contains(pattern) {
                    return Err(ToolError::PermissionDenied(format!(
                        "sandbox: command blocked (matched '{pattern}')"
                    )));
                }
            }
        }

        // Check for dangerous commands
        if let Some(danger) = detect_dangerous_command(command) {
            match danger {
                DangerLevel::Critical(msg) => {
                    let confirmed = ctx
                        .user
                        .confirm(&format!("CRITICAL: {msg}\nProceed anyway?"))
                        .await;
                    if !confirmed {
                        return Err(ToolError::PermissionDenied(format!(
                            "critical command blocked: {msg}"
                        )));
                    }
                }
                DangerLevel::Warning(msg) => {
                    ctx.user.notify(&format!("Warning: {msg}"));
                }
            }
        }

        let timeout_secs = timeout_override.unwrap_or(self.config.terminal_timeout);
        let timeout = std::time::Duration::from_secs(timeout_secs);

        // Build command with optional sandbox restrictions
        let mut cmd = self.build_command(command, ctx);

        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::Execution(format!("failed to spawn process: {e}")))?;

        // Take stdout/stderr handles before waiting
        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        // Wait with timeout
        let result = tokio::time::timeout(timeout, child.wait()).await;

        match result {
            Ok(Ok(status)) => {
                use tokio::io::AsyncReadExt;

                let mut stdout_buf = Vec::new();
                let mut stderr_buf = Vec::new();

                if let Some(ref mut h) = stdout_handle {
                    let _ = h.read_to_end(&mut stdout_buf).await;
                }
                if let Some(ref mut h) = stderr_handle {
                    let _ = h.read_to_end(&mut stderr_buf).await;
                }

                let stdout = String::from_utf8_lossy(&stdout_buf);
                let stderr = String::from_utf8_lossy(&stderr_buf);
                let exit_code = status.code().unwrap_or(-1);

                let prefix = if self.sandbox { "[SANDBOXED] " } else { "" };

                let mut text = format!("{prefix}Exit code: {exit_code}\n");

                if !stdout.is_empty() {
                    use std::fmt::Write;
                    let _ = write!(text, "--- stdout ---\n{stdout}");
                }
                if !stderr.is_empty() {
                    use std::fmt::Write;
                    let _ = write!(text, "--- stderr ---\n{stderr}");
                }

                Ok(ToolResult::text(text))
            }
            Ok(Err(e)) => Err(ToolError::Execution(format!("process error: {e}"))),
            Err(_) => {
                // Timeout — kill the process
                let _ = child.kill().await;
                Err(ToolError::Timeout {
                    tool: "terminal".to_string(),
                    timeout_secs,
                })
            }
        }
    }

    fn build_command(
        &self,
        command: &str,
        ctx: &ToolContext,
    ) -> tokio::process::Command {
        if self.sandbox {
            self.build_sandboxed_command(command, ctx)
        } else {
            self.build_normal_command(command, ctx)
        }
    }

    #[allow(clippy::unused_self)]
    fn build_normal_command(
        &self,
        command: &str,
        ctx: &ToolContext,
    ) -> tokio::process::Command {
        #[cfg(target_family = "unix")]
        {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c")
                .arg(command)
                .current_dir(&ctx.cwd)
                .envs(&ctx.env)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }

        #[cfg(target_family = "windows")]
        {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/C")
                .arg(command)
                .current_dir(&ctx.cwd)
                .envs(&ctx.env)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }
    }

    #[allow(clippy::unused_self)]
    fn build_sandboxed_command(
        &self,
        command: &str,
        ctx: &ToolContext,
    ) -> tokio::process::Command {
        // Sensitive env vars to clear in sandbox mode
        let sensitive_vars = [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AZURE_CLIENT_SECRET",
            "GCP_SERVICE_ACCOUNT_KEY",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GITLAB_TOKEN",
            "NPM_TOKEN",
            "DOCKER_PASSWORD",
            "DATABASE_URL",
            "REDIS_URL",
            "SECRET_KEY",
            "PRIVATE_KEY",
            "API_KEY",
            "SSH_AUTH_SOCK",
        ];

        #[cfg(target_family = "unix")]
        {
            // On Unix: use restricted PATH and timeout wrapper
            let timeout_secs = self.config.terminal_timeout;
            let wrapped = format!("timeout {timeout_secs} sh -c {}", shell_escape(command));

            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c")
                .arg(&wrapped)
                .current_dir(&ctx.cwd)
                .env_clear();

            // Set minimal safe environment
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin")
                .env("TERM", "dumb")
                .env("LANG", "C.UTF-8");

            // Re-add user env vars except sensitive ones
            for (k, v) in &ctx.env {
                if !sensitive_vars.contains(&k.as_str()) {
                    cmd.env(k, v);
                }
            }

            cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }

        #[cfg(target_family = "windows")]
        {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/C")
                .arg(command)
                .current_dir(&ctx.cwd)
                .env_clear();

            // Set minimal safe environment
            if let Ok(sys_root) = std::env::var("SystemRoot") {
                cmd.env("SystemRoot", &sys_root);
                cmd.env(
                    "PATH",
                    format!("{sys_root}\\System32;{sys_root}"),
                );
            }
            cmd.env("TERM", "dumb");

            // Re-add user env vars except sensitive ones
            for (k, v) in &ctx.env {
                if !sensitive_vars.contains(&k.as_str()) {
                    cmd.env(k, v);
                }
            }

            cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }
    }
}

/// Escape a command string for use inside `sh -c '...'`.
#[cfg(target_family = "unix")]
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}
