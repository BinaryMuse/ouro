//! Web search providers for the agent.
//!
//! Provides DuckDuckGo (zero-config, HTML scraping) and Brave Search (API key
//! required) as search backends. Both return structured `SearchResult` objects
//! serialized as JSON strings.
//!
//! Rate-limited wrappers enforce minimum delays between requests to avoid
//! being blocked by upstream providers.
//!
//! This module is consumed by the tool dispatch layer (Plan 03) but has no
//! direct coupling to tool schemas.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

/// A single search result with title, URL, and snippet.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Search DuckDuckGo via the lite HTML endpoint.
///
/// Sends a GET request to `https://lite.duckduckgo.com/lite/` and parses
/// result links, titles, and snippets from the table-based HTML layout
/// using CSS selectors.
///
/// # Parameters
///
/// - `query`: The search query string.
/// - `count`: Maximum number of results to return.
///
/// # Returns
///
/// JSON array of `SearchResult` objects, or a JSON error string.
pub async fn search_duckduckgo(query: &str, count: usize) -> String {
    let client = match reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0",
        )
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return json!({"error": format!("web_search: failed to build client: {e}")}).to_string()
        }
    };

    let resp = match client
        .get("https://lite.duckduckgo.com/lite/")
        .query(&[("q", query)])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return json!({"error": format!("web_search: DuckDuckGo request failed: {e}")})
                .to_string()
        }
    };

    let html = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            return json!({"error": format!("web_search: failed to read DDG response: {e}")})
                .to_string()
        }
    };

    let results = parse_ddg_lite_html(&html, count);

    serde_json::to_string(&results).unwrap_or_else(|e| {
        json!({"error": format!("web_search: failed to serialize results: {e}")}).to_string()
    })
}

/// Parse DuckDuckGo Lite HTML to extract search results.
///
/// The DDG lite page uses a table layout where result rows contain:
/// - A link (`<a>`) with the result URL and title text
/// - A subsequent row with the snippet text in a `<td>` with class `result-snippet`
fn parse_ddg_lite_html(html: &str, count: usize) -> Vec<SearchResult> {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // DDG lite result links are in <a> tags inside table cells with class "result-link"
    let link_selector = Selector::parse("a.result-link").unwrap();
    let snippet_selector = Selector::parse("td.result-snippet").unwrap();

    let links: Vec<_> = document.select(&link_selector).collect();
    let snippets: Vec<_> = document.select(&snippet_selector).collect();

    let mut results = Vec::new();

    for (i, link) in links.iter().enumerate() {
        if results.len() >= count {
            break;
        }

        let title = link.text().collect::<String>().trim().to_string();
        let url = link
            .value()
            .attr("href")
            .unwrap_or("")
            .trim()
            .to_string();

        // Skip empty/invalid results
        if title.is_empty() || url.is_empty() {
            continue;
        }

        let snippet = snippets
            .get(i)
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

/// Search using the Brave Search REST API.
///
/// Requires a valid API key (`X-Subscription-Token` header). Parses the
/// JSON response and extracts results from the `web.results` array.
///
/// # Parameters
///
/// - `query`: The search query string.
/// - `count`: Maximum number of results to return.
/// - `api_key`: Brave Search API subscription token.
///
/// # Returns
///
/// JSON array of `SearchResult` objects, or a JSON error string.
pub async fn search_brave(query: &str, count: usize, api_key: &str) -> String {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return json!({"error": format!("web_search: failed to build client: {e}")}).to_string()
        }
    };

    let resp = match client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", &count.to_string())])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return json!({"error": format!("web_search: Brave request failed: {e}")}).to_string()
        }
    };

    let status = resp.status();

    if status.as_u16() == 401 {
        return json!({"error": "web_search: Brave API key is invalid or expired"}).to_string();
    }

    if status.as_u16() == 429 {
        return json!({"error": "web_search: Brave Search rate limit exceeded, try again later"})
            .to_string();
    }

    if !status.is_success() {
        return json!({"error": format!("web_search: Brave HTTP {status}")}).to_string();
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return json!({"error": format!("web_search: failed to parse Brave response: {e}")})
                .to_string()
        }
    };

    let empty_vec = vec![];
    let results: Vec<SearchResult> = body["web"]["results"]
        .as_array()
        .unwrap_or(&empty_vec)
        .iter()
        .take(count)
        .filter_map(|r| {
            Some(SearchResult {
                title: r["title"].as_str()?.to_string(),
                url: r["url"].as_str()?.to_string(),
                snippet: r["description"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    serde_json::to_string(&results).unwrap_or_else(|e| {
        json!({"error": format!("web_search: failed to serialize results: {e}")}).to_string()
    })
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

/// Global last-request tracker for DuckDuckGo.
static DDG_LAST_REQUEST: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Global last-request tracker for Brave Search.
static BRAVE_LAST_REQUEST: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Enforce a minimum delay then search DuckDuckGo.
///
/// If the time since the last DDG request is less than `rate_limit_secs`,
/// sleeps for the remaining duration before issuing the request.
pub async fn rate_limited_ddg_search(query: &str, count: usize, rate_limit_secs: f64) -> String {
    enforce_rate_limit(&DDG_LAST_REQUEST, rate_limit_secs).await;
    search_duckduckgo(query, count).await
}

/// Enforce a minimum delay then search Brave.
///
/// If the time since the last Brave request is less than `rate_limit_secs`,
/// sleeps for the remaining duration before issuing the request.
pub async fn rate_limited_brave_search(
    query: &str,
    count: usize,
    api_key: &str,
    rate_limit_secs: f64,
) -> String {
    enforce_rate_limit(&BRAVE_LAST_REQUEST, rate_limit_secs).await;
    search_brave(query, count, api_key).await
}

/// Wait if necessary to enforce a minimum interval between requests,
/// then update the last-request timestamp.
async fn enforce_rate_limit(tracker: &Mutex<Option<std::time::Instant>>, min_secs: f64) {
    let min_interval = Duration::from_secs_f64(min_secs);

    // Read the last request time (lock released immediately).
    let remaining = {
        let guard = tracker.lock().unwrap();
        guard.and_then(|last| {
            let elapsed = last.elapsed();
            if elapsed < min_interval {
                Some(min_interval - elapsed)
            } else {
                None
            }
        })
    };

    // Sleep outside the lock if needed.
    if let Some(wait) = remaining {
        tokio::time::sleep(wait).await;
    }

    // Update the timestamp.
    let mut guard = tracker.lock().unwrap();
    *guard = Some(std::time::Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ddg_empty_html() {
        let results = parse_ddg_lite_html("<html><body></body></html>", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn parse_ddg_with_results() {
        let html = r#"
        <html><body>
        <table>
            <tr>
                <td><a class="result-link" href="https://example.com">Example Title</a></td>
            </tr>
            <tr>
                <td class="result-snippet">This is a snippet</td>
            </tr>
            <tr>
                <td><a class="result-link" href="https://other.com">Other Result</a></td>
            </tr>
            <tr>
                <td class="result-snippet">Another snippet</td>
            </tr>
        </table>
        </body></html>
        "#;

        let results = parse_ddg_lite_html(html, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].snippet, "This is a snippet");
        assert_eq!(results[1].title, "Other Result");
        assert_eq!(results[1].url, "https://other.com");
    }

    #[test]
    fn parse_ddg_respects_count_limit() {
        let html = r#"
        <html><body>
        <table>
            <tr><td><a class="result-link" href="https://a.com">A</a></td></tr>
            <tr><td class="result-snippet">Snippet A</td></tr>
            <tr><td><a class="result-link" href="https://b.com">B</a></td></tr>
            <tr><td class="result-snippet">Snippet B</td></tr>
            <tr><td><a class="result-link" href="https://c.com">C</a></td></tr>
            <tr><td class="result-snippet">Snippet C</td></tr>
        </table>
        </body></html>
        "#;

        let results = parse_ddg_lite_html(html, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "A");
        assert_eq!(results[1].title, "B");
    }

    #[test]
    fn search_result_serializes_to_json() {
        let result = SearchResult {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            snippet: "A test result".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"url\":\"https://example.com\""));
        assert!(json.contains("\"snippet\":\"A test result\""));
    }
}
