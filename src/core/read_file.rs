use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use crate::context::ToolContext;
use crate::core::CoreToolConfig;
use crate::error::ToolError;
use crate::schema::Schema;
use crate::tool::{Permission, Tool, ToolResult};

/// Known binary file extensions (images, archives, etc.).
const BINARY_EXTENSIONS: &[&str] = &[
    // Images
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "tif",
    // Archives
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst",
    // Compiled / binary
    "exe", "dll", "so", "dylib", "o", "obj", "class", "pyc", "wasm",
    // Media
    "mp3", "mp4", "avi", "mov", "mkv", "flac", "wav", "ogg",
    // Fonts
    "ttf", "otf", "woff", "woff2",
    // Other
    "bin", "dat",
];

/// Image extensions that get base64-encoded on read.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "tif", "svg",
];

/// Session-scoped content hash cache for deduplication.
///
/// Key is `(session_id, path)` so different sessions have independent caches.
static FILE_CACHE: std::sync::LazyLock<Mutex<HashMap<(String, PathBuf), u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Clear the file cache for a specific session.
pub fn clear_file_cache(session_id: &str) {
    let mut cache = FILE_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.retain(|(sid, _), _| sid != session_id);
}

/// Read file contents with line numbers, offset, and limit support.
pub struct ReadFileTool {
    config: CoreToolConfig,
}

impl ReadFileTool {
    #[must_use]
    pub const fn new(config: CoreToolConfig) -> Self {
        Self { config }
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read file contents with line numbers. Supports offset/limit, images (base64), Jupyter notebooks, and content caching."
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn parameters_schema(&self) -> Value {
        Schema::object()
            .required_property("path", Schema::string().description("File path to read"))
            .property(
                "offset",
                Schema::integer()
                    .description("Start line number (1-based)")
                    .default(1)
                    .minimum(1),
            )
            .property(
                "limit",
                Schema::integer()
                    .description("Maximum number of lines to read")
                    .minimum(1),
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

    #[allow(clippy::misnamed_getters)]
    fn max_output_bytes(&self) -> usize {
        self.config.max_read_bytes
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

impl ReadFileTool {
    async fn execute_inner(
        &self,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing required parameter 'path'".into()))?;

        let offset = args["offset"].as_u64().unwrap_or(1).max(1) as usize;
        let limit = args["limit"].as_u64().map(|v| v as usize);

        let path = ctx.resolve_path(path_str)?;

        // Check file extension for special handling
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // PDF detection — not supported
        if ext == "pdf" {
            return Ok(ToolResult::error(
                format!(
                    "PDF files are not supported by read_file. File: {} — consider using a dedicated PDF tool.",
                    path.display()
                ),
                true,
            ));
        }

        // Image support — return base64
        if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            return self.read_image(&path).await;
        }

        // Binary detection by extension
        if BINARY_EXTENSIONS.contains(&ext.as_str()) {
            let meta = tokio::fs::metadata(&path).await.map_err(|e| {
                ToolError::Execution(format!("cannot stat '{}': {e}", path.display()))
            })?;
            return Ok(ToolResult::text(format!(
                "Binary file detected: {} ({} bytes)",
                path.display(),
                meta.len()
            )));
        }

        // Jupyter notebook support
        if ext == "ipynb" {
            return self.read_notebook(&path).await;
        }

        // Read raw bytes
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            ToolError::Execution(format!("cannot read '{}': {e}", path.display()))
        })?;

        // Binary detection: check first 8KB for null bytes
        let check_len = bytes.len().min(8192);
        if bytes[..check_len].contains(&0) {
            return Ok(ToolResult::text(format!(
                "Binary file detected: {} ({} bytes)",
                path.display(),
                bytes.len()
            )));
        }

        // Content hash caching (session-scoped)
        let content_hash = compute_hash(&bytes);
        let cache_key = (ctx.task_id.clone(), path.clone());
        {
            let mut cache = FILE_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(&cached_hash) = cache.get(&cache_key) {
                if cached_hash == content_hash {
                    return Ok(ToolResult::text(format!(
                        "[File unchanged since last read: {}]",
                        path.display()
                    )));
                }
            }
            cache.insert(cache_key, content_hash);
        }

        let content = String::from_utf8_lossy(&bytes);

        // Apply offset and limit, add line numbers
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = (offset - 1).min(total_lines);
        let end = limit.map_or(total_lines, |lim| (start + lim).min(total_lines));

        let mut output = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            let _ = writeln!(output, "{line_num}\t{line}");
        }

        // Truncate if needed
        if output.len() > self.config.max_read_bytes {
            let mut result = ToolResult::text(output);
            result.truncate(self.config.max_read_bytes);
            return Ok(result);
        }

        Ok(ToolResult::text(output))
    }

    async fn read_image(&self, path: &std::path::Path) -> Result<ToolResult, ToolError> {
        use base64::Engine;

        let bytes = tokio::fs::read(path).await.map_err(|e| {
            ToolError::Execution(format!("cannot read image '{}': {e}", path.display()))
        })?;

        let size = bytes.len();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        Ok(ToolResult::text(format!(
            "[Image: {} ({size} bytes, base64 encoded)]\n{b64}",
            path.display()
        )))
    }

    async fn read_notebook(&self, path: &std::path::Path) -> Result<ToolResult, ToolError> {
        use std::fmt::Write;

        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            ToolError::Execution(format!("cannot read notebook '{}': {e}", path.display()))
        })?;

        let notebook: Value = serde_json::from_str(&content).map_err(|e| {
            ToolError::Execution(format!("invalid notebook JSON in '{}': {e}", path.display()))
        })?;

        let cells = notebook["cells"]
            .as_array()
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "no 'cells' array in notebook '{}'",
                    path.display()
                ))
            })?;

        let mut output = String::new();
        for (i, cell) in cells.iter().enumerate() {
            let cell_type = cell["cell_type"].as_str().unwrap_or("unknown");
            let _ = writeln!(output, "[Cell {} ({cell_type})]", i + 1);

            if let Some(source) = cell["source"].as_array() {
                for line in source {
                    if let Some(s) = line.as_str() {
                        output.push_str(s);
                    }
                }
                output.push('\n');
            } else if let Some(source) = cell["source"].as_str() {
                output.push_str(source);
                output.push('\n');
            }

            output.push('\n');
        }

        Ok(ToolResult::text(output))
    }
}

/// Compute a simple hash of byte content.
fn compute_hash(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_extension_detection() {
        assert!(BINARY_EXTENSIONS.contains(&"png"));
        assert!(BINARY_EXTENSIONS.contains(&"zip"));
        assert!(!BINARY_EXTENSIONS.contains(&"rs"));
    }

    #[test]
    fn test_image_extension_detection() {
        assert!(IMAGE_EXTENSIONS.contains(&"png"));
        assert!(IMAGE_EXTENSIONS.contains(&"svg"));
        assert!(!IMAGE_EXTENSIONS.contains(&"zip"));
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let data = b"hello world";
        let h1 = compute_hash(data);
        let h2 = compute_hash(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_hash_different() {
        let h1 = compute_hash(b"hello");
        let h2 = compute_hash(b"world");
        assert_ne!(h1, h2);
    }
}
