/// Configuration for output truncation limits.
#[derive(Debug, Clone)]
pub struct OutputLimits {
    /// Maximum total bytes.
    pub max_bytes: usize,
    /// Maximum number of lines.
    pub max_lines: usize,
    /// Maximum length of a single line in bytes.
    pub max_line_length: usize,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            max_bytes: 50_000,
            max_lines: 2_000,
            max_line_length: 2_000,
        }
    }
}

impl OutputLimits {
    /// Truncate output according to the configured limits.
    ///
    /// Applies line-length, line-count, and total-byte limits in order.
    #[must_use] 
    pub fn truncate(&self, output: &str) -> String {
        let mut result = String::with_capacity(output.len().min(self.max_bytes));

        for (line_count, line) in output.lines().enumerate() {
            if line_count >= self.max_lines {
                result.push_str("\n[truncated: line limit reached]");
                return result;
            }

            let truncated_line = safe_truncate_str(line, self.max_line_length);
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(truncated_line);
            if truncated_line.len() < line.len() {
                result.push_str(" [line truncated]");
            }

            if result.len() >= self.max_bytes {
                // Truncate to max_bytes at a char boundary
                let end = find_char_boundary(&result, self.max_bytes);
                result.truncate(end);
                result.push_str("\n[truncated: byte limit reached]");
                return result;
            }
        }

        result
    }
}

/// Truncate a string slice to at most `max_bytes`, respecting char boundaries.
fn safe_truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let end = find_char_boundary(s, max_bytes);
    &s[..end]
}

/// Find the largest char boundary <= `max_bytes`.
fn find_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        let limits = OutputLimits::default();
        assert_eq!(limits.max_bytes, 50_000);
        assert_eq!(limits.max_lines, 2_000);
        assert_eq!(limits.max_line_length, 2_000);
    }

    #[test]
    fn test_truncate_lines() {
        let limits = OutputLimits {
            max_bytes: 100_000,
            max_lines: 3,
            max_line_length: 1000,
        };
        let input = "line1\nline2\nline3\nline4\nline5";
        let result = limits.truncate(input);
        assert!(result.contains("line3"));
        assert!(result.contains("[truncated: line limit reached]"));
    }

    #[test]
    fn test_truncate_bytes() {
        let limits = OutputLimits {
            max_bytes: 10,
            max_lines: 1000,
            max_line_length: 1000,
        };
        let input = "a".repeat(100);
        let result = limits.truncate(&input);
        assert!(result.contains("[truncated: byte limit reached]"));
    }
}
