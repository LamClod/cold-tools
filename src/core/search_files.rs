use std::path::Path;

use regex::Regex;
use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Skip directories that are typically not useful to search.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vs",
    "dist",
    "build",
];

/// Search file contents using regex patterns with optional glob filtering.
pub struct SearchFilesTool {
    config: CoreToolConfig,
}

impl SearchFilesTool {
    #[must_use] 
    pub const fn new(config: CoreToolConfig) -> Self {
        Self { config }
    }
}

impl Tool for SearchFilesTool {
    fn name(&self) -> &'static str {
        "search_files"
    }

    fn description(&self) -> &'static str {
        "Search file contents using regex patterns. Supports glob filtering and context lines."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property(
                "pattern",
                Schema::string().description("Regex pattern to search for"),
            )
            .property(
                "path",
                Schema::string()
                    .description("Directory to search in")
                    .default("."),
            )
            .property(
                "glob_pattern",
                Schema::string().description("File name glob filter (e.g. \"*.rs\")"),
            )
            .property(
                "max_results",
                Schema::integer()
                    .description("Maximum number of matches to return")
                    .default(50)
                    .minimum(1),
            )
            .property(
                "context_lines",
                Schema::integer()
                    .description("Lines of context around each match")
                    .default(0)
                    .minimum(0),
            )
            .property(
                "output_mode",
                Schema::string()
                    .description("Output mode: content (default), files, or count")
                    .default("content")
                    .enum_values(&["content", "files", "count"]),
            )
            .property(
                "file_type",
                Schema::string()
                    .description("Filter by file extension (e.g. \"rs\", \"py\", \"js\")"),
            )
            .property(
                "multiline",
                Schema::boolean()
                    .description("Enable multiline regex matching")
                    .default(false),
            )
            .property(
                "offset",
                Schema::integer()
                    .description("Number of matches to skip for pagination")
                    .default(0)
                    .minimum(0),
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

impl SearchFilesTool {
    #[allow(clippy::unused_async, clippy::too_many_lines)]
    async fn execute_inner(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let pattern_str = args["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'pattern'".into()))?;

        let search_path = args["path"].as_str().unwrap_or(".");
        let glob_pattern = args["glob_pattern"].as_str();
        let max_results = args["max_results"].as_u64().unwrap_or(50) as usize;
        let context_lines = args["context_lines"].as_u64().unwrap_or(0) as usize;
        let output_mode = args["output_mode"].as_str().unwrap_or("content");
        let file_type = args["file_type"].as_str();
        let multiline = args["multiline"].as_bool().unwrap_or(false);
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;

        let regex = if multiline {
            regex::RegexBuilder::new(pattern_str)
                .multi_line(true)
                .dot_matches_new_line(true)
                .build()
        } else {
            Regex::new(pattern_str)
        }
        .map_err(|e| ToolError::Execution(format!("invalid regex pattern: {e}")))?;

        let search_dir = ctx.resolve_path(search_path)?;

        let glob_matcher = glob_pattern
            .map(glob::Pattern::new)
            .transpose()
            .map_err(|e| ToolError::Execution(format!("invalid glob pattern: {e}")))?;

        let mut matches = Vec::new();
        let mut match_count: usize = 0;
        let mut skipped: usize = 0;

        // For "files" and "count" modes we track per-file data
        let mut file_match_counts: Vec<(String, usize)> = Vec::new();
        let mut matched_files: Vec<String> = Vec::new();

        for entry in walkdir::WalkDir::new(&search_dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_string_lossy();
                    return !SKIP_DIRS.contains(&name.as_ref());
                }
                true
            })
        {
            if match_count >= max_results + offset {
                break;
            }

            if ctx.is_cancelled() {
                break;
            }

            let Ok(entry) = entry else { continue };

            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();

            // Apply glob filter
            if let Some(ref matcher) = glob_matcher {
                let file_name = file_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                if !matcher.matches(&file_name) {
                    continue;
                }
            }

            // Apply file_type filter (extension match)
            if let Some(ft) = file_type {
                let ext = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if !ext.eq_ignore_ascii_case(ft) {
                    continue;
                }
            }

            // Read file, skip binary
            let Ok(bytes) = std::fs::read(file_path) else { continue };

            let check_len = bytes.len().min(8192);
            if bytes[..check_len].contains(&0) {
                continue;
            }

            let Ok(content) = std::str::from_utf8(&bytes) else { continue };

            let lines: Vec<&str> = content.lines().collect();
            let rel_path = file_path
                .strip_prefix(&search_dir)
                .unwrap_or(file_path);
            let rel_str = rel_path.display().to_string();

            let mut file_hits: usize = 0;

            for (line_idx, line) in lines.iter().enumerate() {
                if match_count >= max_results + offset {
                    break;
                }

                if regex.is_match(line) {
                    file_hits += 1;
                    match_count += 1;

                    // Skip entries before offset (for pagination)
                    if match_count <= offset {
                        skipped += 1;
                        continue;
                    }

                    // Only emit detail for "content" mode
                    if output_mode == "content" {
                        if context_lines > 0 {
                            let start = line_idx.saturating_sub(context_lines);
                            let end = (line_idx + context_lines + 1).min(lines.len());

                            matches.push(format!("{}:", rel_path.display()));
                            for (i, ctx_line) in lines.iter().enumerate().take(end).skip(start) {
                                let marker = if i == line_idx { ">" } else { " " };
                                matches.push(format!("{marker}{}: {}", i + 1, ctx_line));
                            }
                            matches.push(String::new());
                        } else {
                            matches.push(format!(
                                "{}:{}:{}",
                                rel_path.display(),
                                line_idx + 1,
                                line
                            ));
                        }
                    }
                }
            }

            if file_hits > 0 {
                matched_files.push(rel_str.clone());
                file_match_counts.push((rel_str, file_hits));
            }
        }

        let effective_count = match_count.saturating_sub(skipped);

        if effective_count == 0 {
            return Ok(ToolResult::text(format!(
                "No matches found for pattern '{pattern_str}' in '{}'",
                relative_display(&search_dir, &ctx.cwd)
            )));
        }

        match output_mode {
            "files" => {
                use std::fmt::Write;
                let mut output = format!("Found matches in {} file(s):\n", matched_files.len());
                for f in &matched_files {
                    let _ = writeln!(output, "{f}");
                }
                Ok(ToolResult::text(output))
            }
            "count" => {
                use std::fmt::Write;
                let mut output =
                    format!("Match counts ({effective_count} total in {} files):\n", file_match_counts.len());
                for (path, count) in &file_match_counts {
                    let _ = writeln!(output, "{path}: {count}");
                }
                Ok(ToolResult::text(output))
            }
            _ => {
                // "content" mode (default)
                let header = format!("Found {effective_count} match(es):\n");
                Ok(ToolResult::text(header + &matches.join("\n")))
            }
        }
    }
}


fn relative_display(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}
