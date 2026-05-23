use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// List directory contents with type, size, and optional recursion.
pub struct ListDirTool {
    config: CoreToolConfig,
}

impl ListDirTool {
    #[must_use] 
    pub const fn new(config: CoreToolConfig) -> Self {
        Self { config }
    }
}

impl Tool for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List directory contents with file types and sizes. Supports recursion depth control."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .property(
                "path",
                Schema::string()
                    .description("Directory path to list")
                    .default("."),
            )
            .property(
                "depth",
                Schema::integer()
                    .description("Recursion depth (1 = no recursion)")
                    .default(1)
                    .minimum(1),
            )
            .property(
                "show_hidden",
                Schema::boolean()
                    .description("Show hidden files (starting with '.')")
                    .default(false),
            )
            .build()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }

    fn permission(&self) -> Permission {
        Permission::Auto
    }

    fn max_output_bytes(&self) -> usize {
        self.config.max_output_bytes
    }

    fn execute<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + 'a>> {
        Box::pin(self.execute_inner(args, ctx))
    }
}

impl ListDirTool {
    #[allow(clippy::unused_async)]
    async fn execute_inner(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;
        let dir_path = args["path"].as_str().unwrap_or(".");
        let depth = args["depth"].as_u64().unwrap_or(1) as usize;
        let show_hidden = args["show_hidden"].as_bool().unwrap_or(false);

        let resolved = ctx.resolve_path(dir_path)?;

        if !resolved.is_dir() {
            return Err(ToolError::Execution(format!(
                "'{}' is not a directory",
                resolved.display()
            )));
        }

        let mut entries: Vec<EntryInfo> = Vec::new();

        for entry in walkdir::WalkDir::new(&resolved)
            .min_depth(1)
            .max_depth(depth)
            .follow_links(false)
            .sort_by_file_name()
        {
            let Ok(entry) = entry else { continue };

            let name = entry
                .path()
                .strip_prefix(&resolved)
                .unwrap_or_else(|_| entry.path())
                .to_string_lossy()
                .to_string();

            // Skip hidden files unless requested
            if !show_hidden {
                let base_name = entry.file_name().to_string_lossy();
                if base_name.starts_with('.') {
                    continue;
                }
            }

            let ft = entry.file_type();
            let (kind, size) = if ft.is_dir() {
                ("dir", None)
            } else if ft.is_symlink() {
                ("symlink", None)
            } else {
                let size = entry.metadata().map(|m| m.len()).ok();
                ("file", size)
            };

            entries.push(EntryInfo {
                name,
                kind,
                size,
            });
        }

        // Sort: directories first, then files, alphabetically within each group
        entries.sort_by(|a, b| {
            let a_is_dir = a.kind == "dir";
            let b_is_dir = b.kind == "dir";
            b_is_dir.cmp(&a_is_dir).then_with(|| a.name.cmp(&b.name))
        });

        if entries.is_empty() {
            return Ok(ToolResult::text(format!(
                "Empty directory: '{}'",
                resolved.display()
            )));
        }

        // Format as aligned table
        let max_name_len = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
        let max_kind_len = entries.iter().map(|e| e.kind.len()).max().unwrap_or(0);

        let mut output = String::new();
        for entry in &entries {
            let size_str = entry.size.map_or_else(|| "-".to_string(), format_size);
            let _ = writeln!(
                output,
                "{:<name_w$}  {:<kind_w$}  {}",
                entry.name,
                entry.kind,
                size_str,
                name_w = max_name_len,
                kind_w = max_kind_len,
            );
        }

        Ok(ToolResult::text(output))
    }
}


struct EntryInfo {
    name: String,
    kind: &'static str,
    size: Option<u64>,
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
