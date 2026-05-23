// Pedantic clippy allows for acceptable patterns
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::format_push_string)]

pub mod context;
pub mod core;
pub mod deferred;
pub mod dispatch;
pub mod error;
pub mod guardrails;
pub mod limits;
pub mod mcp;
pub mod parallel;
pub mod permission;
pub mod providers;
pub mod registry;
pub mod result_budget;
pub mod result_storage;
pub mod schema;
pub mod security;
pub mod tool;

pub use context::{AutoApprove, ToolContext, UserInteraction};
pub use core::{
    register_core_tools, register_core_tools_with_deferred, register_core_tools_with_providers,
    AskUserTool, CoreToolConfig, EnterPlanModeTool, ExitPlanModeTool, GlobTool,
    NotebookEditTool, TaskEntry, TaskKind, TaskStatus, TodoWriteTool, WebFetchTool,
    WebSearchTool, clear_file_cache,
};
pub use deferred::{DeferredRegistry, DeferredTool};
pub use dispatch::Dispatcher;
pub use error::ToolError;
pub use guardrails::{GuardrailConfig, GuardrailController, GuardrailDecision};
pub use limits::OutputLimits;
pub use mcp::{McpContent, McpResource, McpToolDef, McpToolResult, McpTransport};
pub use mcp::adapter::{McpToolAdapter, register_mcp_tools};
pub use mcp::resource_tools::{ListMcpResourcesTool, ReadMcpResourceTool};
pub use permission::{
    DecisionReason, DenialTracker, PermissionConfig, PermissionDecision, PermissionMode,
    PermissionRule, RuleAction,
};
pub use providers::{SearchResult, WebProvider};
pub use registry::ToolRegistry;
pub use result_budget::ResultBudget;
pub use result_storage::persist_if_large;
pub use schema::Schema;
pub use security::{DangerLevel, detect_dangerous_command};
pub use tool::{Permission, Tool, ToolResult};
