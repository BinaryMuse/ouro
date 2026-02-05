//! HTTP fetch utility for retrieving web page content.
//!
//! Provides a standalone `fetch_url` function that fetches a URL via HTTP GET
//! and returns the content as markdown, raw HTML, or JSON. This module is
//! consumed by the tool dispatch layer (Plan 03) but has no direct coupling
//! to tool schemas.
//!
//! Errors are returned as JSON strings (never `Err`), matching the
//! `dispatch_tool_call` convention.

use std::time::Duration;

use serde_json::json;

/// Fetch a URL and return its content.
///
/// # Parameters
///
/// - `url`: The URL to fetch.
/// - `format`: `"markdown"` to convert HTML to markdown via `htmd`, or
///   `"html"` to return raw HTML. JSON responses are always returned as-is
///   regardless of this parameter.
/// - `max_length`: Optional character limit. If the content exceeds this,
///   it is truncated with a summary suffix.
///
/// # Returns
///
/// The fetched content as a string, or a JSON error object on failure.
pub async fn fetch_url(url: &str, format: &str, max_length: Option<usize>) -> String {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("Mozilla/5.0 (compatible; Ouro/0.1)")
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({"error": format!("web_fetch: failed to build client: {e}")}).to_string(),
    };

    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return json!({"error": format!("web_fetch: {e}")}).to_string(),
    };

    let status = response.status();
    if !status.is_success() {
        return json!({"error": format!("web_fetch: HTTP {status}")}).to_string();
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            return json!({"error": format!("web_fetch: failed to read body: {e}")}).to_string()
        }
    };

    // JSON responses: return as-is (no conversion).
    if content_type.contains("application/json") {
        return maybe_truncate(&body, max_length);
    }

    // HTML: convert based on format parameter.
    let output = if content_type.contains("text/html") && format == "markdown" {
        htmd::convert(&body).unwrap_or(body)
    } else {
        body
    };

    maybe_truncate(&output, max_length)
}

/// Truncate content to `max_length` characters if specified.
///
/// When truncated, appends a summary line showing the truncation point and
/// total original length.
fn maybe_truncate(content: &str, max_length: Option<usize>) -> String {
    match max_length {
        Some(limit) if content.len() > limit => {
            format!(
                "{}...\n[truncated at {} chars, total {}]",
                &content[..limit],
                limit,
                content.len()
            )
        }
        _ => content.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_within_limit() {
        let result = maybe_truncate("hello world", Some(100));
        assert_eq!(result, "hello world");
    }

    #[test]
    fn truncate_at_limit() {
        let result = maybe_truncate("hello world", Some(5));
        assert!(result.starts_with("hello"));
        assert!(result.contains("[truncated at 5 chars, total 11]"));
    }

    #[test]
    fn truncate_none_returns_full() {
        let result = maybe_truncate("hello world", None);
        assert_eq!(result, "hello world");
    }
}
