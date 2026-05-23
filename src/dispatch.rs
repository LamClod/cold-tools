use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tokio::task::JoinSet;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::guardrails::{GuardrailConfig, GuardrailController, GuardrailDecision};
use crate::limits::OutputLimits;
use crate::parallel::should_parallelize;
use crate::permission::check_permission;
use crate::registry::ToolRegistry;
use crate::result_storage::persist_if_large;
use crate::tool::ToolResult;

/// High-level dispatcher that orchestrates tool execution with guardrails,
/// permissions, timeouts, and output truncation.
pub struct Dispatcher {
    registry: Arc<ToolRegistry>,
    guardrails: GuardrailController,
    limits: OutputLimits,
}

impl Dispatcher {
    /// Create a new dispatcher with default guardrails and limits.
    #[must_use]
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            guardrails: GuardrailController::new(GuardrailConfig::default()),
            limits: OutputLimits::default(),
        }
    }

    /// Configure custom guardrails.
    #[must_use]
    pub fn with_guardrails(mut self, config: GuardrailConfig) -> Self {
        self.guardrails = GuardrailController::new(config);
        self
    }

    /// Configure custom output limits.
    #[must_use]
    pub const fn with_limits(mut self, limits: OutputLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Execute a single tool call.
    ///
    /// Flow: guardrails -> permission -> timeout -> truncate -> guardrails update.
    pub async fn execute_one(
        &mut self,
        name: &str,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        // 1. Check guardrails
        let decision = self.guardrails.before_call(name, &args);
        match &decision {
            GuardrailDecision::Block(msg) => {
                return Err(ToolError::Blocked(msg.clone()));
            }
            GuardrailDecision::Warn(msg) => {
                #[cfg(feature = "tracing")]
                tracing::warn!("guardrail warning for '{name}': {msg}");
                ctx.user.notify(&format!("Warning: {msg}"));
            }
            GuardrailDecision::Allow => {}
        }

        // 2. Look up tool
        let tool = self
            .registry
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        // 3. Check permission
        check_permission(tool, &args, ctx).await?;

        // 4. Execute with timeout
        let timeout = std::time::Duration::from_secs(tool.timeout_secs());
        let result = tokio::time::timeout(timeout, tool.execute(args.clone(), ctx)).await;

        let result = if let Ok(inner) = result {
            let succeeded = inner.is_ok();
            self.guardrails.after_call(name, &args, succeeded);
            inner?
        } else {
            self.guardrails.after_call(name, &args, false);
            return Err(ToolError::Timeout {
                tool: name.to_string(),
                timeout_secs: tool.timeout_secs(),
            });
        };

        // 5. Persist large results to disk before truncation
        let max_bytes = tool.max_output_bytes().min(self.limits.max_bytes);
        let mut result = persist_if_large(&result, max_bytes, &ctx.root, name);

        // 6. Truncate output
        result.truncate(max_bytes);

        Ok(result)
    }

    /// Execute a batch of tool calls with true parallel execution.
    ///
    /// Concurrent-safe calls run in parallel via `JoinSet`; serial-only calls
    /// execute sequentially.  Results are returned in the original call order.
    ///
    /// If any parallel tool returns `ToolResult::Error { recoverable: false }`,
    /// a shared abort flag is set so sibling tasks can check `ctx.cancelled`.
    #[allow(clippy::too_many_lines)]
    pub async fn execute_batch(
        &mut self,
        calls: Vec<(String, Value)>,
        ctx: &ToolContext,
    ) -> Vec<Result<ToolResult, ToolError>> {
        if calls.is_empty() {
            return Vec::new();
        }

        if calls.len() == 1 {
            let (name, args) = calls.into_iter().next().expect("checked len");
            return vec![self.execute_one(&name, args, ctx).await];
        }

        // Partition into concurrent-safe and serial-only calls
        let can_parallel = should_parallelize(&calls, &self.registry);

        if !can_parallel {
            let mut results = Vec::with_capacity(calls.len());
            for (name, args) in calls {
                results.push(self.execute_one(&name, args, ctx).await);
            }
            return results;
        }

        // --- True parallel path ---

        // Pre-check guardrails sequentially (needs &mut self)
        let mut pre_checked: Vec<Option<(String, Value)>> = Vec::with_capacity(calls.len());
        for (name, args) in &calls {
            let decision = self.guardrails.before_call(name, args);
            if let GuardrailDecision::Block(_) = decision {
                pre_checked.push(None);
            } else {
                if let GuardrailDecision::Warn(msg) = &decision {
                    ctx.user.notify(&format!("Warning: {msg}"));
                }
                pre_checked.push(Some((name.clone(), args.clone())));
            }
        }

        // Shared abort flag for sibling cancellation
        let abort_flag = Arc::new(AtomicBool::new(false));

        // Spawn concurrent tasks
        let mut join_set: JoinSet<(usize, Result<ToolResult, ToolError>)> = JoinSet::new();

        for (idx, item) in pre_checked.iter().enumerate() {
            let Some((name, args)) = item else {
                continue;
            };

            let registry = Arc::clone(&self.registry);
            let name = name.clone();
            let args = args.clone();
            let max_bytes = self.limits.max_bytes;
            let root = ctx.root.clone();
            let abort = Arc::clone(&abort_flag);

            // Build a lightweight context for the spawned task.
            // Share the same cancelled flag chain: original OR abort_flag.
            let task_ctx = ToolContext {
                cwd: ctx.cwd.clone(),
                root: ctx.root.clone(),
                task_id: ctx.task_id.clone(),
                user: Arc::clone(&ctx.user),
                cancelled: Arc::clone(&ctx.cancelled),
                env: ctx.env.clone(),
                plan_mode: Arc::clone(&ctx.plan_mode),
            };

            join_set.spawn(async move {
                // Check abort flag before starting
                if abort.load(Ordering::Relaxed) {
                    return (
                        idx,
                        Err(ToolError::Execution(
                            "aborted: sibling task failed".into(),
                        )),
                    );
                }

                let Some(tool) = registry.get(&name) else {
                    return (idx, Err(ToolError::NotFound(name.clone())));
                };

                if !tool.is_available() {
                    return (
                        idx,
                        Err(ToolError::NotFound(format!(
                            "tool '{name}' exists but is not available"
                        ))),
                    );
                }

                let timeout = std::time::Duration::from_secs(tool.timeout_secs());
                let res = tokio::time::timeout(timeout, tool.execute(args, &task_ctx)).await;

                let res = match res {
                    Ok(Ok(result)) => Ok(result),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(ToolError::Timeout {
                        tool: name.clone(),
                        timeout_secs: tool.timeout_secs(),
                    }),
                };

                // Post-process: persist + truncate
                let res = match res {
                    Ok(result) => {
                        let tool_max = tool.max_output_bytes().min(max_bytes);
                        let mut result = persist_if_large(&result, tool_max, &root, &name);
                        result.truncate(tool_max);

                        // Check for non-recoverable errors and signal abort
                        if let ToolResult::Error {
                            recoverable: false, ..
                        } = &result
                        {
                            abort.store(true, Ordering::Relaxed);
                        }

                        Ok(result)
                    }
                    Err(e) => {
                        // Signal abort on any error
                        abort.store(true, Ordering::Relaxed);
                        Err(e)
                    }
                };

                (idx, res)
            });
        }

        // Collect results in original order
        let mut results: Vec<Option<Result<ToolResult, ToolError>>> =
            (0..calls.len()).map(|_| None).collect();

        // Fill in blocked entries
        for (idx, item) in pre_checked.iter().enumerate() {
            if item.is_none() {
                results[idx] = Some(Err(ToolError::Blocked(
                    "blocked by guardrails".to_string(),
                )));
            }
        }

        // Collect from join set
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((idx, res)) => {
                    let succeeded = res.is_ok();
                    self.guardrails
                        .after_call(&calls[idx].0, &calls[idx].1, succeeded);
                    results[idx] = Some(res);
                }
                Err(e) => {
                    // JoinError (panic in task) — find the missing index
                    if let Some(idx) = results.iter().position(Option::is_none) {
                        results[idx] = Some(Err(ToolError::Execution(format!(
                            "task panicked: {e}"
                        ))));
                    }
                }
            }
        }

        results
            .into_iter()
            .map(|r| r.unwrap_or_else(|| Err(ToolError::Execution("result missing".into()))))
            .collect()
    }

    /// Reset guardrails for a new conversational turn.
    pub fn reset_for_turn(&mut self) {
        self.guardrails.reset_for_turn();
    }
}
