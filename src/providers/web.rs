use std::future::Future;
use std::pin::Pin;

use crate::error::ToolError;

/// A search result from a web search provider.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Title of the search result.
    pub title: String,
    /// URL of the search result.
    pub url: String,
    /// Snippet/summary of the search result.
    pub snippet: String,
}

/// Trait for web content fetching and searching.
///
/// Implementations can use reqwest, a proxy service, or any HTTP client.
/// This keeps cold-tools free of HTTP client dependencies.
pub trait WebProvider: Send + Sync {
    /// Fetch the content at `url`, returning at most `max_length` bytes of text.
    fn fetch(
        &self,
        url: &str,
        max_length: usize,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + '_>>;

    /// Search the web for `query`, returning at most `max_results` results.
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SearchResult>, ToolError>> + Send + '_>>;
}
