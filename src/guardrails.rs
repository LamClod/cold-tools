use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde_json::Value;

/// Configuration for guardrail thresholds.
#[derive(Debug, Clone)]
pub struct GuardrailConfig {
    /// Warn after this many identical (name+args) failures.
    pub exact_failure_warn_after: u32,
    /// Block after this many identical (name+args) failures.
    pub exact_failure_block_after: u32,
    /// Halt after this many consecutive same-tool invocations.
    pub same_tool_halt_after: u32,
    /// Block after this many consecutive no-progress calls.
    pub no_progress_block_after: u32,
}

impl Default for GuardrailConfig {
    fn default() -> Self {
        Self {
            exact_failure_warn_after: 2,
            exact_failure_block_after: 5,
            same_tool_halt_after: 8,
            no_progress_block_after: 5,
        }
    }
}

/// Decision from the guardrail check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardrailDecision {
    /// Execution is allowed.
    Allow,
    /// Execution is allowed but with a warning.
    Warn(String),
    /// Execution is blocked.
    Block(String),
}

impl GuardrailDecision {
    /// Whether this decision allows tool execution.
    #[must_use] 
    pub const fn allows_execution(&self) -> bool {
        !matches!(self, Self::Block(_))
    }
}

/// Controller that tracks tool call patterns and detects loops.
pub struct GuardrailController {
    config: GuardrailConfig,
    /// hash(name + `canonical_json(args)`) -> failure count
    failure_counts: HashMap<u64, u32>,
    /// tool name -> consecutive failure count
    tool_failures: HashMap<String, u32>,
    /// tool name -> consecutive no-progress count
    no_progress: HashMap<String, u32>,
    /// Last tool name called (for consecutive tracking).
    last_tool: Option<String>,
    /// Consecutive calls to the same tool.
    consecutive_same: u32,
}

impl GuardrailController {
    /// Create a new guardrail controller with the given config.
    #[must_use] 
    pub fn new(config: GuardrailConfig) -> Self {
        Self {
            config,
            failure_counts: HashMap::new(),
            tool_failures: HashMap::new(),
            no_progress: HashMap::new(),
            last_tool: None,
            consecutive_same: 0,
        }
    }

    /// Check guardrails before a tool call.
    pub fn before_call(&mut self, name: &str, args: &Value) -> GuardrailDecision {
        // Track consecutive same-tool calls
        if self.last_tool.as_deref() == Some(name) {
            self.consecutive_same += 1;
        } else {
            self.consecutive_same = 1;
        }
        self.last_tool = Some(name.to_string());

        // Check same-tool halt
        if self.consecutive_same >= self.config.same_tool_halt_after {
            return GuardrailDecision::Block(format!(
                "tool '{name}' called {} consecutive times (limit: {})",
                self.consecutive_same, self.config.same_tool_halt_after
            ));
        }

        // Check exact-failure counts
        let sig = call_signature(name, args);
        let count = self.failure_counts.get(&sig).copied().unwrap_or(0);

        if count >= self.config.exact_failure_block_after {
            return GuardrailDecision::Block(format!(
                "identical call to '{name}' failed {count} times (limit: {})",
                self.config.exact_failure_block_after
            ));
        }

        if count >= self.config.exact_failure_warn_after {
            return GuardrailDecision::Warn(format!(
                "identical call to '{name}' has failed {count} times"
            ));
        }

        // Check no-progress count
        let np = self.no_progress.get(name).copied().unwrap_or(0);
        if np >= self.config.no_progress_block_after {
            return GuardrailDecision::Block(format!(
                "tool '{name}' shows no progress after {np} calls (limit: {})",
                self.config.no_progress_block_after
            ));
        }

        GuardrailDecision::Allow
    }

    /// Update guardrails after a tool call completes.
    pub fn after_call(&mut self, name: &str, args: &Value, succeeded: bool) {
        let sig = call_signature(name, args);

        if succeeded {
            // Reset failure counts on success
            self.failure_counts.remove(&sig);
            self.tool_failures.remove(name);
        } else {
            *self.failure_counts.entry(sig).or_insert(0) += 1;
            *self.tool_failures.entry(name.to_string()).or_insert(0) += 1;
        }
    }

    /// Reset guardrails for a new conversational turn.
    pub fn reset_for_turn(&mut self) {
        self.last_tool = None;
        self.consecutive_same = 0;
        self.no_progress.clear();
    }
}

/// Compute a hash signature for a (name, args) pair.
fn call_signature(name: &str, args: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    // Canonical JSON (sorted keys via serde_json's to_string)
    serde_json::to_string(args)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_allows_first_call() {
        let mut ctrl = GuardrailController::new(GuardrailConfig::default());
        let decision = ctrl.before_call("read_file", &json!({"path": "test.rs"}));
        assert_eq!(decision, GuardrailDecision::Allow);
    }

    #[test]
    fn test_blocks_after_repeated_failures() {
        let mut ctrl = GuardrailController::new(GuardrailConfig {
            exact_failure_block_after: 3,
            exact_failure_warn_after: 1,
            ..GuardrailConfig::default()
        });

        let args = json!({"path": "missing.rs"});

        // First call — allow
        ctrl.before_call("read_file", &args);
        ctrl.after_call("read_file", &args, false);

        // Second call — warn
        let d = ctrl.before_call("read_file", &args);
        assert!(matches!(d, GuardrailDecision::Warn(_)));
        ctrl.after_call("read_file", &args, false);

        // Third call — still warn (count=2, block_after=3)
        let d = ctrl.before_call("read_file", &args);
        assert!(matches!(d, GuardrailDecision::Warn(_)));
        ctrl.after_call("read_file", &args, false);

        // Fourth call — block (count=3)
        let d = ctrl.before_call("read_file", &args);
        assert!(matches!(d, GuardrailDecision::Block(_)));
    }

    #[test]
    fn test_consecutive_same_tool_halt() {
        let mut ctrl = GuardrailController::new(GuardrailConfig {
            same_tool_halt_after: 3,
            ..GuardrailConfig::default()
        });

        let args = json!({});
        ctrl.before_call("think", &args);
        ctrl.before_call("think", &args);
        let d = ctrl.before_call("think", &args);
        assert!(matches!(d, GuardrailDecision::Block(_)));
    }

    #[test]
    fn test_reset_for_turn() {
        let mut ctrl = GuardrailController::new(GuardrailConfig {
            same_tool_halt_after: 3,
            ..GuardrailConfig::default()
        });

        let args = json!({});
        ctrl.before_call("think", &args);
        ctrl.before_call("think", &args);
        ctrl.reset_for_turn();

        let d = ctrl.before_call("think", &args);
        assert_eq!(d, GuardrailDecision::Allow);
    }
}
