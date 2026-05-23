use crate::tool::ToolResult;

/// Per-message budget for total tool result size.
///
/// When the combined text output of all tool results in a message exceeds
/// `max_total_bytes`, the largest results are truncated first.
#[derive(Debug, Clone)]
pub struct ResultBudget {
    /// Maximum total bytes across all tool results in a single message.
    pub max_total_bytes: usize,
}

impl Default for ResultBudget {
    fn default() -> Self {
        Self {
            max_total_bytes: 200_000,
        }
    }
}

impl ResultBudget {
    /// Enforce the budget across a set of named tool results.
    ///
    /// If the total text length exceeds the budget, the largest results are
    /// truncated first, replaced with a stub message.
    pub fn enforce(&self, results: &mut [(String, ToolResult)]) {
        let total: usize = results.iter().map(|(_, r)| result_size(r)).sum();

        if total <= self.max_total_bytes {
            return;
        }

        let mut excess = total - self.max_total_bytes;

        // Build index sorted by size descending so we truncate the biggest first
        let mut indices: Vec<usize> = (0..results.len()).collect();
        indices.sort_by(|&a, &b| result_size(&results[b].1).cmp(&result_size(&results[a].1)));

        for &idx in &indices {
            if excess == 0 {
                break;
            }

            let size = result_size(&results[idx].1);
            if size == 0 {
                continue;
            }

            // How much can we reclaim from this result?
            let stub = "[Result truncated due to message budget]";
            let stub_len = stub.len();

            if size <= stub_len {
                continue;
            }

            let reclaimable = size - stub_len;
            let reclaim = reclaimable.min(excess);

            if reclaim > 0 {
                results[idx].1 = ToolResult::text(stub);
                excess = excess.saturating_sub(reclaimable);
            }
        }
    }
}

/// Get the byte size of a tool result's text content.
fn result_size(result: &ToolResult) -> usize {
    match result {
        ToolResult::Text(s) => s.len(),
        ToolResult::Json(v) => v.to_string().len(),
        ToolResult::Error { message, .. } => message.len(),
        ToolResult::Empty => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_under_budget_no_change() {
        let budget = ResultBudget {
            max_total_bytes: 1000,
        };
        let mut results = vec![
            ("a".into(), ToolResult::text("hello")),
            ("b".into(), ToolResult::text("world")),
        ];
        budget.enforce(&mut results);
        assert_eq!(results[0].1.as_text(), "hello");
        assert_eq!(results[1].1.as_text(), "world");
    }

    #[test]
    fn test_over_budget_truncates_largest() {
        let budget = ResultBudget {
            max_total_bytes: 100,
        };
        let big = "x".repeat(200);
        let mut results = vec![
            ("small".into(), ToolResult::text("hi")),
            ("big".into(), ToolResult::text(big)),
        ];
        budget.enforce(&mut results);
        assert_eq!(results[0].1.as_text(), "hi");
        assert!(results[1]
            .1
            .as_text()
            .contains("truncated due to message budget"));
    }

    #[test]
    fn test_empty_results() {
        let budget = ResultBudget::default();
        let mut results: Vec<(String, ToolResult)> = vec![];
        budget.enforce(&mut results);
        assert!(results.is_empty());
    }
}
