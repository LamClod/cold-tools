mod ask_user;
mod edit_file;
mod glob_tool;
mod list_dir;
mod notebook_edit;
mod plan_mode;
mod process;
mod read_file;
mod search_files;
mod terminal;
mod think;
mod todo_write;
pub mod tool_search;
mod web_fetch;
mod web_search;
mod write_file;

use std::path::PathBuf;
use std::sync::Arc;

use crate::deferred::DeferredRegistry;
use crate::providers::WebProvider;
use crate::registry::ToolRegistry;

pub use ask_user::AskUserTool;
pub use edit_file::EditFileTool;
pub use glob_tool::GlobTool;
pub use list_dir::ListDirTool;
pub use notebook_edit::NotebookEditTool;
pub use plan_mode::{EnterPlanModeTool, ExitPlanModeTool};
pub use process::{ProcessTool, TaskEntry, TaskKind, TaskStatus};
pub use read_file::{ReadFileTool, clear_file_cache};
pub use search_files::SearchFilesTool;
pub use terminal::TerminalTool;
pub use think::ThinkTool;
pub use todo_write::TodoWriteTool;
pub use tool_search::ToolSearchTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;
pub use write_file::WriteFileTool;

/// Configuration for core tools.
#[derive(Debug, Clone)]
pub struct CoreToolConfig {
    /// Security root directory.
    pub root_dir: PathBuf,
    /// Terminal command timeout in seconds.
    pub terminal_timeout: u64,
    /// Maximum bytes to read from a file.
    pub max_read_bytes: usize,
    /// Maximum output bytes.
    pub max_output_bytes: usize,
    /// Whether terminal commands run in sandbox mode.
    pub sandbox: bool,
}

impl Default for CoreToolConfig {
    fn default() -> Self {
        Self {
            root_dir: PathBuf::from("."),
            terminal_timeout: 120,
            max_read_bytes: 100_000,
            max_output_bytes: 50_000,
            sandbox: false,
        }
    }
}

impl CoreToolConfig {
    /// Set sandbox mode for terminal execution.
    #[must_use]
    pub const fn with_sandbox(mut self, sandbox: bool) -> Self {
        self.sandbox = sandbox;
        self
    }
}

/// Register all Tier 1 core tools with the given registry.
pub fn register_core_tools(registry: &mut ToolRegistry, config: CoreToolConfig) {
    register_core_tools_with_providers(registry, config, None);
}

/// Register all Tier 1 core tools **plus** the `ToolSearch` meta-tool.
pub fn register_core_tools_with_deferred(
    registry: &mut ToolRegistry,
    config: CoreToolConfig,
    deferred: Arc<DeferredRegistry>,
) {
    register_core_tools(registry, config);
    registry.register(ToolSearchTool::new(deferred));
}

/// Register all Tier 1 core tools, optionally including web tools if a provider is given.
pub fn register_core_tools_with_providers(
    registry: &mut ToolRegistry,
    config: CoreToolConfig,
    web_provider: Option<Arc<dyn WebProvider>>,
) {
    registry.register(ReadFileTool::new(config.clone()));
    registry.register(WriteFileTool::new(config.clone()));
    registry.register(EditFileTool::new(config.clone()));
    registry.register(SearchFilesTool::new(config.clone()));
    registry.register(ListDirTool::new(config.clone()));
    registry.register(GlobTool::new(config.clone()));

    if config.sandbox {
        registry.register(TerminalTool::new_sandboxed(config.clone()));
    } else {
        registry.register(TerminalTool::new(config.clone()));
    }

    registry.register(ProcessTool::new(config));
    registry.register(ThinkTool);
    registry.register(AskUserTool);
    registry.register(TodoWriteTool);
    registry.register(NotebookEditTool);
    registry.register(EnterPlanModeTool);
    registry.register(ExitPlanModeTool);

    // Web tools require an injected provider
    if let Some(provider) = web_provider {
        registry.register(WebFetchTool::new(Arc::clone(&provider)));
        registry.register(WebSearchTool::new(provider));
    }
}
