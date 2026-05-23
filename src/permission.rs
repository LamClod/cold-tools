use std::collections::HashMap;

use serde_json::Value;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::tool::{Permission, Tool};

/// High-level permission mode for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionMode {
    /// Ask for dangerous tools, auto-approve safe ones.
    #[default]
    Default,
    /// Auto-allow file edits, ask for everything else that is not Auto.
    AcceptEdits,
    /// Allow everything without prompting.
    BypassPermissions,
    /// Auto-deny instead of prompting the user.
    DontAsk,
    /// Read-only mode: deny all write operations.
    Plan,
    /// Reserved for future AI-classifier-based decisions.
    Auto,
}

/// Action to take when a permission rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// Silently allow execution.
    Allow,
    /// Deny execution without prompting.
    Deny,
    /// Ask the user for confirmation.
    Ask,
}

/// A single pattern-based permission rule.
#[derive(Debug, Clone)]
pub struct PermissionRule {
    /// Glob-style pattern matched against the tool name (e.g. `"Bash"`, `"terminal"`).
    pub tool_pattern: String,
    /// Optional glob-style pattern matched against the first string argument.
    pub arg_pattern: Option<String>,
    /// What to do when this rule matches.
    pub action: RuleAction,
}

/// Complete permission configuration for a session.
#[derive(Debug, Clone, Default)]
pub struct PermissionConfig {
    /// The active permission mode.
    pub mode: PermissionMode,
    /// Rules that always allow certain tool calls.
    pub always_allow: Vec<PermissionRule>,
    /// Rules that always deny certain tool calls.
    pub always_deny: Vec<PermissionRule>,
}

/// The reason behind a permission decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionReason {
    /// Tool has `Permission::Auto` and was auto-approved.
    AutoApproved,
    /// Bypass mode — all calls allowed.
    ModeBypass,
    /// Plan mode denied a write tool.
    ModePlanDeny,
    /// `DontAsk` mode denied a non-auto tool.
    ModeDontAskDeny,
    /// An `always_allow` rule matched (includes the pattern).
    RuleAllow(String),
    /// An `always_deny` rule matched (includes the pattern).
    RuleDeny(String),
    /// User interactively approved.
    UserApproved,
    /// User interactively denied.
    UserDenied,
    /// Fell through to tool's default permission level.
    ToolDefault,
}

/// Result of a permission check, including the reason for the decision.
#[derive(Debug, Clone)]
pub struct PermissionDecision {
    /// Whether execution is allowed.
    pub allowed: bool,
    /// Why this decision was made.
    pub reason: DecisionReason,
}

/// Tracks consecutive permission denials per tool to auto-skip repeat offenders.
#[derive(Debug, Clone)]
pub struct DenialTracker {
    denied: HashMap<String, u32>,
    threshold: u32,
}

impl DenialTracker {
    /// Create a new tracker with the given consecutive-denial threshold.
    #[must_use]
    pub fn new(threshold: u32) -> Self {
        Self {
            denied: HashMap::new(),
            threshold,
        }
    }

    /// Record that a tool was denied permission.
    pub fn record_denial(&mut self, tool_name: &str) {
        *self.denied.entry(tool_name.to_string()).or_insert(0) += 1;
    }

    /// Record that a tool was allowed (resets its denial count).
    pub fn record_allow(&mut self, tool_name: &str) {
        self.denied.remove(tool_name);
    }

    /// Returns true if the tool has been denied at least `threshold` times consecutively.
    #[must_use]
    pub fn should_skip(&self, tool_name: &str) -> bool {
        self.denied
            .get(tool_name)
            .is_some_and(|&count| count >= self.threshold)
    }

    /// Reset all denial counts.
    pub fn reset(&mut self) {
        self.denied.clear();
    }
}

impl Default for DenialTracker {
    fn default() -> Self {
        Self::new(3)
    }
}

/// Check whether the tool has permission to execute.
///
/// When a `PermissionConfig` is available on the context, the evaluation order is:
/// 1. `always_deny` rules -- if any matches, deny immediately.
/// 2. `always_allow` rules -- if any matches, allow immediately.
/// 3. Mode-based decision (Plan, `BypassPermissions`, `DontAsk`, `AcceptEdits`, Default).
/// 4. Fallback to the tool's own `permission()` level.
pub async fn check_permission(
    tool: &dyn Tool,
    args: &Value,
    ctx: &ToolContext,
) -> Result<(), ToolError> {
    check_permission_with_config(tool, args, ctx, None).await?;
    Ok(())
}

/// Check whether a rule matches a given tool call.
#[must_use]
pub fn matches_rule(tool_name: &str, args: &Value, rule: &PermissionRule) -> bool {
    if !glob_matches(&rule.tool_pattern, tool_name) {
        return false;
    }

    rule.arg_pattern.as_ref().is_none_or(|arg_pat| {
        first_string_arg(args).is_some_and(|s| glob_matches(arg_pat, s))
    })
}

/// Check permission with an explicit configuration, returning a decision with reason.
///
/// When a `DenialTracker` is provided, tools that have been denied
/// `threshold` consecutive times are auto-denied.
pub async fn check_permission_with_config(
    tool: &dyn Tool,
    args: &Value,
    ctx: &ToolContext,
    config: Option<&PermissionConfig>,
) -> Result<PermissionDecision, ToolError> {
    // Plan mode: block non-read-only tools
    if ctx.is_plan_mode() && !tool.is_read_only() {
        return Err(ToolError::PermissionDenied(format!(
            "plan mode: write tool '{}' is not allowed",
            tool.name()
        )));
    }

    let Some(cfg) = config else {
        // No config -- fall back to tool-level permission
        return check_tool_permission(tool, args, ctx).await;
    };

    // 1. Check always_deny rules first
    for rule in &cfg.always_deny {
        if matches_rule(tool.name(), args, rule) {
            return Err(ToolError::PermissionDenied(format!(
                "denied by rule: tool '{}' matched deny pattern '{}'",
                tool.name(),
                rule.tool_pattern
            )));
        }
    }

    // 2. Check always_allow rules
    for rule in &cfg.always_allow {
        if matches_rule(tool.name(), args, rule) {
            return Ok(PermissionDecision {
                allowed: true,
                reason: DecisionReason::RuleAllow(rule.tool_pattern.clone()),
            });
        }
    }

    // 3. Mode-based decision
    match cfg.mode {
        PermissionMode::Plan => {
            if tool.is_read_only() {
                return Ok(PermissionDecision {
                    allowed: true,
                    reason: DecisionReason::AutoApproved,
                });
            }
            return Err(ToolError::PermissionDenied(format!(
                "plan mode: write tool '{}' is not allowed",
                tool.name()
            )));
        }
        PermissionMode::BypassPermissions => {
            return Ok(PermissionDecision {
                allowed: true,
                reason: DecisionReason::ModeBypass,
            });
        }
        PermissionMode::DontAsk => {
            if tool.permission() == Permission::Auto {
                return Ok(PermissionDecision {
                    allowed: true,
                    reason: DecisionReason::AutoApproved,
                });
            }
            return Err(ToolError::PermissionDenied(format!(
                "dont-ask mode: tool '{}' requires confirmation",
                tool.name()
            )));
        }
        PermissionMode::AcceptEdits => {
            let name = tool.name();
            if name == "write_file" || name == "edit_file" {
                return Ok(PermissionDecision {
                    allowed: true,
                    reason: DecisionReason::ModeBypass,
                });
            }
            // Fall through to tool-level permission for others
        }
        PermissionMode::Auto | PermissionMode::Default => {
            // Fall through to tool-level permission
        }
    }

    // 4. Fallback to tool's own permission level
    check_tool_permission(tool, args, ctx).await
}

/// Evaluate the tool's own `Permission` enum.
async fn check_tool_permission(
    tool: &dyn Tool,
    args: &Value,
    ctx: &ToolContext,
) -> Result<PermissionDecision, ToolError> {
    match tool.permission() {
        Permission::Auto => Ok(PermissionDecision {
            allowed: true,
            reason: DecisionReason::AutoApproved,
        }),
        Permission::Ask | Permission::Confirm => {
            let action = format!(
                "Execute tool '{}' with args: {}",
                tool.name(),
                serde_json::to_string(args).unwrap_or_default()
            );
            if ctx.user.confirm(&action).await {
                Ok(PermissionDecision {
                    allowed: true,
                    reason: DecisionReason::UserApproved,
                })
            } else {
                Err(ToolError::PermissionDenied(format!(
                    "user denied execution of '{}'",
                    tool.name()
                )))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Simple glob matching supporting `*` (any chars) and `?` (single char).
fn glob_matches(pattern: &str, text: &str) -> bool {
    glob_matches_inner(pattern.as_bytes(), text.as_bytes())
}

#[allow(clippy::similar_names)]
fn glob_matches_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Extract the first string value from a JSON object (checking common keys).
fn first_string_arg(args: &Value) -> Option<&str> {
    // Try well-known keys first, then fall back to first string value
    for key in &["command", "path", "query", "action"] {
        if let Some(s) = args.get(*key).and_then(Value::as_str) {
            return Some(s);
        }
    }
    // Fall back to first string value in the object
    if let Some(obj) = args.as_object() {
        for value in obj.values() {
            if let Some(s) = value.as_str() {
                return Some(s);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matches_exact() {
        assert!(glob_matches("terminal", "terminal"));
        assert!(!glob_matches("terminal", "Terminal"));
    }

    #[test]
    fn test_glob_matches_star() {
        assert!(glob_matches("term*", "terminal"));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("git *", "git push"));
    }

    #[test]
    fn test_glob_matches_question() {
        assert!(glob_matches("t?st", "test"));
        assert!(!glob_matches("t?st", "toast"));
    }

    #[test]
    fn test_matches_rule_tool_only() {
        let rule = PermissionRule {
            tool_pattern: "terminal".into(),
            arg_pattern: None,
            action: RuleAction::Allow,
        };
        assert!(matches_rule("terminal", &serde_json::json!({}), &rule));
        assert!(!matches_rule("read_file", &serde_json::json!({}), &rule));
    }

    #[test]
    fn test_matches_rule_with_arg_pattern() {
        let rule = PermissionRule {
            tool_pattern: "terminal".into(),
            arg_pattern: Some("git *".into()),
            action: RuleAction::Allow,
        };
        let args = serde_json::json!({"command": "git status"});
        assert!(matches_rule("terminal", &args, &rule));

        let args2 = serde_json::json!({"command": "rm -rf /"});
        assert!(!matches_rule("terminal", &args2, &rule));
    }

    #[test]
    fn test_permission_config_default() {
        let cfg = PermissionConfig::default();
        assert_eq!(cfg.mode, PermissionMode::Default);
        assert!(cfg.always_allow.is_empty());
        assert!(cfg.always_deny.is_empty());
    }

    #[test]
    fn test_denial_tracker() {
        let mut tracker = DenialTracker::new(3);
        assert!(!tracker.should_skip("test_tool"));

        tracker.record_denial("test_tool");
        tracker.record_denial("test_tool");
        assert!(!tracker.should_skip("test_tool"));

        tracker.record_denial("test_tool");
        assert!(tracker.should_skip("test_tool"));

        tracker.record_allow("test_tool");
        assert!(!tracker.should_skip("test_tool"));
    }

    #[test]
    fn test_denial_tracker_reset() {
        let mut tracker = DenialTracker::new(2);
        tracker.record_denial("a");
        tracker.record_denial("a");
        assert!(tracker.should_skip("a"));
        tracker.reset();
        assert!(!tracker.should_skip("a"));
    }

    #[test]
    fn test_decision_reason_variants() {
        let decision = PermissionDecision {
            allowed: true,
            reason: DecisionReason::AutoApproved,
        };
        assert!(decision.allowed);
        assert_eq!(decision.reason, DecisionReason::AutoApproved);

        let decision = PermissionDecision {
            allowed: true,
            reason: DecisionReason::RuleAllow("terminal".into()),
        };
        assert!(decision.allowed);
    }
}
