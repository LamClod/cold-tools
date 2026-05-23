use std::path::Path;

use crate::tool::ToolResult;

/// Preview length in characters when a result is persisted to disk.
const PREVIEW_CHARS: usize = 500;

/// Persist a tool result to disk if it exceeds `max_bytes`.
///
/// Saved to `{root}/.cold/tool-results/{tool_name}_{timestamp}.txt`.
/// Returns the original result unchanged when it fits within the limit.
#[must_use]
pub fn persist_if_large(
    result: &ToolResult,
    max_bytes: usize,
    root: &Path,
    tool_name: &str,
) -> ToolResult {
    let content = match result {
        ToolResult::Text(s) => s.as_str(),
        ToolResult::Json(v) => return persist_json_if_large(v, max_bytes, root, tool_name),
        ToolResult::Error { .. } | ToolResult::Empty => return result.clone(),
    };

    if content.len() <= max_bytes {
        return result.clone();
    }

    persist_content(content, root, tool_name)
}

/// Handle JSON variant separately to avoid intermediate allocation when small.
fn persist_json_if_large(
    value: &serde_json::Value,
    max_bytes: usize,
    root: &Path,
    tool_name: &str,
) -> ToolResult {
    let serialized = value.to_string();
    if serialized.len() <= max_bytes {
        return ToolResult::Json(value.clone());
    }
    persist_content(&serialized, root, tool_name)
}

/// Write `content` to disk and return a preview result.
fn persist_content(content: &str, root: &Path, tool_name: &str) -> ToolResult {
    let dir = root.join(".cold").join("tool-results");

    // Best-effort directory creation (sync — called from sync context after await)
    if std::fs::create_dir_all(&dir).is_err() {
        // Fall back: return truncated content without persisting
        return ToolResult::text(safe_preview(content));
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());

    let filename = format!("{tool_name}_{timestamp}.txt");
    let file_path = dir.join(&filename);

    if std::fs::write(&file_path, content).is_err() {
        return ToolResult::text(safe_preview(content));
    }

    let preview = safe_preview(content);
    ToolResult::text(format!(
        "[Result saved to {}]\n\nPreview:\n{preview}...",
        file_path.display()
    ))
}

/// Take the first `PREVIEW_CHARS` characters, respecting char boundaries.
fn safe_preview(s: &str) -> &str {
    if s.len() <= PREVIEW_CHARS {
        return s;
    }
    let mut end = PREVIEW_CHARS;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_result_unchanged() {
        let result = ToolResult::text("hello");
        let out = persist_if_large(&result, 1000, Path::new("/tmp"), "test");
        assert_eq!(out.as_text(), "hello");
    }

    #[test]
    fn test_large_result_persisted() {
        let dir = std::env::temp_dir().join("cold-tools-test-persist");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let big = "x".repeat(2000);
        let result = ToolResult::text(big);
        let out = persist_if_large(&result, 100, &dir, "test_tool");
        let text = out.as_text();
        assert!(text.contains("[Result saved to"));
        assert!(text.contains("Preview:"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_error_result_unchanged() {
        let result = ToolResult::error("oops", true);
        let out = persist_if_large(&result, 1, Path::new("/tmp"), "test");
        assert_eq!(out.as_text(), "oops");
    }

    #[test]
    fn test_empty_result_unchanged() {
        let result = ToolResult::Empty;
        let out = persist_if_large(&result, 1, Path::new("/tmp"), "test");
        assert_eq!(out.as_text(), "");
    }
}
