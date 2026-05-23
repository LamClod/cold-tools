use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::context::ToolContext;
use crate::deferred::{DeferredRegistry, DeferredTool};
use crate::error::ToolError;
use crate::tool::{Tool, ToolResult};

/// Registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    generation: u64,
}

impl ToolRegistry {
    /// Create an empty registry.
    #[must_use] 
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            generation: 0,
        }
    }

    /// Register a tool. Increments the generation counter.
    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools
            .insert(tool.name().to_string(), Arc::new(tool));
        self.generation += 1;
    }

    /// Register a tool with automatic routing: if the tool declares
    /// `should_defer() == true` and `always_load() == false`, it is added to the
    /// given [`DeferredRegistry`] instead of the main registry.  Otherwise it is
    /// registered normally.
    ///
    /// Returns `true` if the tool was deferred, `false` if it was registered
    /// normally.
    pub fn register_auto(
        &mut self,
        tool: impl Tool + 'static,
        deferred: &mut DeferredRegistry,
    ) -> bool {
        if tool.should_defer() && !tool.always_load() {
            deferred.defer(DeferredTool {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                search_hints: tool.search_hints(),
            });
            true
        } else {
            self.register(tool);
            false
        }
    }

    /// Remove a tool by name. Returns `true` if it was present.
    pub fn deregister(&mut self, name: &str) -> bool {
        let removed = self.tools.remove(name).is_some();
        if removed {
            self.generation += 1;
        }
        removed
    }

    /// Look up a tool by name.
    #[must_use] 
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(std::convert::AsRef::as_ref)
    }

    /// List all registered tool names.
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(String::as_str).collect()
    }

    /// Current generation counter (incremented on every register/deregister).
    #[must_use] 
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Generate OpenAI-format tool definitions for all registered tools.
    #[must_use] 
    pub fn get_definitions(&self) -> Vec<Value> {
        self.tools
            .values()
            .filter(|t| t.is_available())
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters_schema(),
                    }
                })
            })
            .collect()
    }

    /// Generate definitions filtered by toolset names.
    #[must_use] 
    pub fn get_definitions_for_toolsets(&self, enabled: &[&str]) -> Vec<Value> {
        self.tools
            .values()
            .filter(|t| t.is_available() && enabled.contains(&t.toolset()))
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters_schema(),
                    }
                })
            })
            .collect()
    }

    /// Dispatch a tool call by name.
    pub async fn dispatch(
        &self,
        name: &str,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        if !tool.is_available() {
            return Err(ToolError::NotFound(format!(
                "tool '{name}' exists but is not available"
            )));
        }

        tool.execute(args, ctx).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Permission;

    struct DummyTool;

    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn description(&self) -> &str {
            "A dummy tool for testing"
        }
        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn permission(&self) -> Permission {
            Permission::Auto
        }
        fn is_read_only(&self) -> bool {
            true
        }
        fn execute<'a>(
            &'a self,
            _args: Value,
            _ctx: &'a ToolContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>> {
            Box::pin(async { Ok(ToolResult::text("ok")) })
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut reg = ToolRegistry::new();
        assert_eq!(reg.generation(), 0);
        reg.register(DummyTool);
        assert_eq!(reg.generation(), 1);
        assert!(reg.get("dummy").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_deregister() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        assert!(reg.deregister("dummy"));
        assert!(!reg.deregister("dummy"));
        assert!(reg.get("dummy").is_none());
    }

    struct DeferredDummyTool;

    impl Tool for DeferredDummyTool {
        fn name(&self) -> &str {
            "deferred_dummy"
        }
        fn description(&self) -> &str {
            "A deferred dummy tool"
        }
        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn permission(&self) -> Permission {
            Permission::Auto
        }
        fn is_read_only(&self) -> bool {
            true
        }
        fn should_defer(&self) -> bool {
            true
        }
        fn always_load(&self) -> bool {
            false
        }
        fn search_hints(&self) -> Vec<String> {
            vec!["deferred".into(), "test".into()]
        }
        fn execute<'a>(
            &'a self,
            _args: Value,
            _ctx: &'a ToolContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, crate::error::ToolError>> + Send + 'a>> {
            Box::pin(async { Ok(ToolResult::text("ok")) })
        }
    }

    #[test]
    fn test_register_auto_normal() {
        let mut reg = ToolRegistry::new();
        let mut deferred = DeferredRegistry::new();
        let was_deferred = reg.register_auto(DummyTool, &mut deferred);
        assert!(!was_deferred);
        assert!(reg.get("dummy").is_some());
        assert!(deferred.list().is_empty());
    }

    #[test]
    fn test_register_auto_deferred() {
        let mut reg = ToolRegistry::new();
        let mut deferred = DeferredRegistry::new();
        let was_deferred = reg.register_auto(DeferredDummyTool, &mut deferred);
        assert!(was_deferred);
        assert!(reg.get("deferred_dummy").is_none());
        assert_eq!(deferred.list().len(), 1);
        assert_eq!(deferred.list()[0].name, "deferred_dummy");
        assert_eq!(deferred.list()[0].search_hints, vec!["deferred", "test"]);
    }

    #[test]
    fn test_get_definitions() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        let defs = reg.get_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["type"], "function");
        assert_eq!(defs[0]["function"]["name"], "dummy");
    }
}
