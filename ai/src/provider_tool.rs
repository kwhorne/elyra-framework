//! Provider tools — native tools **executed by the AI provider**, not your app
//! (unlike [`Tool`](crate::Tool)). Web search and web fetch are supported
//! natively on **Anthropic** (server-side, within one turn). OpenAI exposes
//! these only through its Responses API, which this SDK does not use yet, so
//! provider tools with the OpenAI provider return an `Unsupported` error.

use serde_json::{json, Value};

/// Approximate user location to localize web-search results.
#[derive(Clone, Default)]
pub struct UserLocation {
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub timezone: Option<String>,
}

/// Native web-search tool.
#[derive(Clone, Default)]
pub struct WebSearch {
    max_uses: Option<u32>,
    allowed: Vec<String>,
    blocked: Vec<String>,
    location: Option<UserLocation>,
}

impl WebSearch {
    pub fn new() -> Self {
        Self::default()
    }
    /// Cap the number of searches per request.
    pub fn max(mut self, uses: u32) -> Self {
        self.max_uses = Some(uses);
        self
    }
    /// Restrict results to these domains.
    pub fn allow<I: IntoIterator<Item = S>, S: Into<String>>(mut self, domains: I) -> Self {
        self.allowed = domains.into_iter().map(Into::into).collect();
        self
    }
    /// Exclude these domains.
    pub fn block<I: IntoIterator<Item = S>, S: Into<String>>(mut self, domains: I) -> Self {
        self.blocked = domains.into_iter().map(Into::into).collect();
        self
    }
    /// Bias results toward a location.
    pub fn location(mut self, location: UserLocation) -> Self {
        self.location = Some(location);
        self
    }
}

/// Native web-fetch tool (retrieves a specific URL). Anthropic marks this beta.
#[derive(Clone, Default)]
pub struct WebFetch {
    max_uses: Option<u32>,
    allowed: Vec<String>,
}

impl WebFetch {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn max(mut self, uses: u32) -> Self {
        self.max_uses = Some(uses);
        self
    }
    pub fn allow<I: IntoIterator<Item = S>, S: Into<String>>(mut self, domains: I) -> Self {
        self.allowed = domains.into_iter().map(Into::into).collect();
        self
    }
}

/// A provider-executed tool (internal enum threaded to the driver).
#[derive(Clone)]
pub(crate) enum ProviderTool {
    WebSearch(WebSearch),
    WebFetch(WebFetch),
}

/// Whether `model` takes the 2026-02-09 web tools, which filter results with
/// code before they reach the context. Other models keep the basic ones.
fn filters_results(model: &str) -> bool {
    [
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-opus-5",
        "claude-sonnet-4-6",
        "claude-sonnet-5",
    ]
    .iter()
    .any(|family| model.starts_with(family))
}

impl ProviderTool {
    /// The Anthropic native tool spec for `model`. Version identifiers may
    /// need bumping as Anthropic revises these tools.
    pub(crate) fn anthropic_spec(&self, model: &str) -> Value {
        let filtering = filters_results(model);
        match self {
            ProviderTool::WebSearch(w) => {
                let kind = if filtering {
                    "web_search_20260209"
                } else {
                    "web_search_20250305"
                };
                let mut v = json!({"type": kind, "name": "web_search"});
                if let Some(n) = w.max_uses {
                    v["max_uses"] = json!(n);
                }
                if !w.allowed.is_empty() {
                    v["allowed_domains"] = json!(w.allowed);
                }
                if !w.blocked.is_empty() {
                    v["blocked_domains"] = json!(w.blocked);
                }
                if let Some(loc) = &w.location {
                    let mut l = json!({"type": "approximate"});
                    if let Some(c) = &loc.city {
                        l["city"] = json!(c);
                    }
                    if let Some(r) = &loc.region {
                        l["region"] = json!(r);
                    }
                    if let Some(c) = &loc.country {
                        l["country"] = json!(c);
                    }
                    if let Some(t) = &loc.timezone {
                        l["timezone"] = json!(t);
                    }
                    v["user_location"] = l;
                }
                v
            }
            ProviderTool::WebFetch(w) => {
                let kind = if filtering {
                    "web_fetch_20260209"
                } else {
                    "web_fetch_20250910"
                };
                let mut v = json!({"type": kind, "name": "web_fetch"});
                if let Some(n) = w.max_uses {
                    v["max_uses"] = json!(n);
                }
                if !w.allowed.is_empty() {
                    v["allowed_domains"] = json!(w.allowed);
                }
                v
            }
        }
    }

    /// The `anthropic-beta` header this tool requires on `model`, if any.
    pub(crate) fn anthropic_beta(&self, model: &str) -> Option<&'static str> {
        match self {
            ProviderTool::WebFetch(_) if !filters_results(model) => Some("web-fetch-2025-09-10"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_spec_reflects_config() {
        let spec = ProviderTool::WebSearch(
            WebSearch::new()
                .max(3)
                .allow(["example.com"])
                .block(["spam.com"]),
        )
        .anthropic_spec("claude-sonnet-5-5");
        assert_eq!(spec["type"], "web_search_20260209");
        assert_eq!(spec["max_uses"], 3);
        assert_eq!(spec["allowed_domains"][0], "example.com");
        assert_eq!(spec["blocked_domains"][0], "spam.com");
    }

    #[test]
    fn the_newer_tools_go_to_the_models_that_take_them() {
        let fetch = ProviderTool::WebFetch(WebFetch::new().max(2));
        let search = ProviderTool::WebSearch(WebSearch::new());
        for model in ["claude-opus-5-5", "claude-sonnet-4-6", "claude-opus-4-8"] {
            assert_eq!(fetch.anthropic_spec(model)["type"], "web_fetch_20260209");
            assert_eq!(search.anthropic_spec(model)["type"], "web_search_20260209");
            assert_eq!(fetch.anthropic_beta(model), None, "{model}");
        }
        // Haiku and older models keep the basic tools, and fetch its beta.
        for model in ["claude-haiku-5-5", "claude-haiku-4-5", "claude-sonnet-4-5"] {
            assert_eq!(fetch.anthropic_spec(model)["type"], "web_fetch_20250910");
            assert_eq!(search.anthropic_spec(model)["type"], "web_search_20250305");
            assert_eq!(fetch.anthropic_beta(model), Some("web-fetch-2025-09-10"));
            assert_eq!(search.anthropic_beta(model), None);
        }
    }
}
