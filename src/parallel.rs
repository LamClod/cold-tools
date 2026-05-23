use serde_json::Value;

use crate::registry::ToolRegistry;

/// Decide if a batch of tool calls can safely run in parallel.
///
/// Rules:
/// - All tools must be read-only OR have non-overlapping paths.
/// - `think` is always parallelizable.
/// - `terminal` is NEVER parallelized (side effects).
/// - File tools with different `path` args can parallelize.
#[must_use] 
pub fn should_parallelize(calls: &[(String, Value)], registry: &ToolRegistry) -> bool {
    if calls.len() <= 1 {
        return true;
    }

    // Check if all tools are concurrency-safe for their given args
    let all_safe = calls.iter().all(|(name, args)| {
        registry
            .get(name)
            .is_some_and(|t| t.is_concurrency_safe(args))
    });

    if all_safe {
        // Even when individually safe, file tools must not overlap paths
    } else {
        return false;
    }

    // Check for path conflicts among non-read-only tools
    let write_calls: Vec<&(String, Value)> = calls
        .iter()
        .filter(|(name, _)| {
            registry
                .get(name)
                .is_none_or(|t| !t.is_read_only())
        })
        .collect();

    // Check pairwise for conflicts
    for i in 0..write_calls.len() {
        for j in (i + 1)..write_calls.len() {
            if paths_conflict(&write_calls[i].1, &write_calls[j].1) {
                return false;
            }
        }
    }

    true
}

/// Check if two file-path-scoped calls conflict (have overlapping paths).
pub fn paths_conflict(a: &Value, b: &Value) -> bool {
    let path_a = a.get("path").and_then(Value::as_str);
    let path_b = b.get("path").and_then(Value::as_str);

    match (path_a, path_b) {
        (Some(a), Some(b)) => {
            // Same path = conflict
            if a == b {
                return true;
            }
            // One is a prefix of the other (directory containment)
            let a_norm = a.replace('\\', "/");
            let b_norm = b.replace('\\', "/");
            a_norm.starts_with(&b_norm) || b_norm.starts_with(&a_norm)
        }
        // No path args — assume conflict (conservative)
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_paths_conflict() {
        assert!(paths_conflict(
            &json!({"path": "src/main.rs"}),
            &json!({"path": "src/main.rs"})
        ));
        assert!(!paths_conflict(
            &json!({"path": "src/main.rs"}),
            &json!({"path": "src/lib.rs"})
        ));
        assert!(paths_conflict(
            &json!({"path": "src"}),
            &json!({"path": "src/main.rs"})
        ));
    }

    #[test]
    fn test_no_path_args_conflict() {
        assert!(paths_conflict(&json!({}), &json!({})));
    }
}
