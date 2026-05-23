/// A tool that is registered by name but not yet loaded into the main registry.
///
/// Deferred tools are discoverable via the `ToolSearch` meta-tool.
#[derive(Debug, Clone)]
pub struct DeferredTool {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Keywords for fuzzy discovery.
    pub search_hints: Vec<String>,
}

/// Registry of deferred (lazily-loaded) tools.
#[derive(Debug, Default)]
pub struct DeferredRegistry {
    deferred: Vec<DeferredTool>,
}

impl DeferredRegistry {
    /// Create an empty deferred registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            deferred: Vec::new(),
        }
    }

    /// Register a deferred tool for later discovery.
    pub fn defer(&mut self, tool: DeferredTool) {
        self.deferred.push(tool);
    }

    /// Search for deferred tools matching `query` (case-insensitive keyword match).
    ///
    /// A tool matches when any of its search hints, name, or description
    /// contains the query as a substring (case-insensitive).
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&DeferredTool> {
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        if keywords.is_empty() {
            return self.deferred.iter().collect();
        }

        let mut scored: Vec<(usize, &DeferredTool)> = self
            .deferred
            .iter()
            .filter_map(|tool| {
                let score = keyword_score(tool, &keywords);
                if score > 0 { Some((score, tool)) } else { None }
            })
            .collect();

        scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        scored.into_iter().map(|(_, tool)| tool).collect()
    }

    /// List all deferred tools.
    #[must_use]
    pub fn list(&self) -> &[DeferredTool] {
        &self.deferred
    }
}

/// Score a tool against a set of lowercase keywords.  Higher = better match.
fn keyword_score(tool: &DeferredTool, keywords: &[&str]) -> usize {
    let name_lower = tool.name.to_lowercase();
    let desc_lower = tool.description.to_lowercase();

    let mut score = 0usize;
    for kw in keywords {
        let name_hit = name_lower.contains(kw);
        let hint_hit = tool
            .search_hints
            .iter()
            .any(|h| h.to_lowercase().contains(kw));
        let desc_hit = desc_lower.contains(kw);

        if !name_hit && !hint_hit && !desc_hit {
            return 0; // all keywords must match at least somewhere
        }

        if name_hit {
            score += 3;
        }
        if hint_hit {
            score += 2;
        }
        if desc_hit {
            score += 1;
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_registry() -> DeferredRegistry {
        let mut reg = DeferredRegistry::new();
        reg.defer(DeferredTool {
            name: "WebSearch".into(),
            description: "Search the web".into(),
            search_hints: vec!["google".into(), "internet".into(), "query".into()],
        });
        reg.defer(DeferredTool {
            name: "WebFetch".into(),
            description: "Fetch a URL".into(),
            search_hints: vec!["http".into(), "download".into(), "url".into()],
        });
        reg.defer(DeferredTool {
            name: "NotebookEdit".into(),
            description: "Edit Jupyter notebooks".into(),
            search_hints: vec!["jupyter".into(), "ipynb".into(), "cell".into()],
        });
        reg
    }

    #[test]
    fn test_search_by_name() {
        let reg = sample_registry();
        let results = reg.search("web");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_hint() {
        let reg = sample_registry();
        let results = reg.search("jupyter");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "NotebookEdit");
    }

    #[test]
    fn test_search_no_match() {
        let reg = sample_registry();
        let results = reg.search("zzzzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_empty_query_returns_all() {
        let reg = sample_registry();
        let results = reg.search("");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_list() {
        let reg = sample_registry();
        assert_eq!(reg.list().len(), 3);
    }
}
