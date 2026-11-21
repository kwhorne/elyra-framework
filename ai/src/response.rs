use std::ops::Range;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{Error, Result};

/// Token usage for a request.
#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// The result of prompting an agent.
#[derive(Clone, Debug)]
pub struct Response {
    pub(crate) text: String,
    pub(crate) usage: Usage,
    pub(crate) steps: u32,
    pub(crate) citations: Vec<Citation>,
}

impl Response {
    pub(crate) fn new(text: String, usage: Usage, steps: u32) -> Self {
        Self {
            text,
            usage,
            steps,
            citations: Vec::new(),
        }
    }

    /// With `citations`, sorted by where they start in the text (RFC 0051).
    pub(crate) fn citing(mut self, mut citations: Vec<Citation>) -> Self {
        citations.sort_by_key(|c| c.span.start);
        self.citations = citations;
        self
    }
}

impl Response {
    /// The generated text (or, for structured output, the raw JSON).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Token usage across all steps.
    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// How many model round-trips (tool-use steps) were taken.
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// The sources the answer cites, in the order they appear in the text
    /// (RFC 0051): what a [web search](crate::WebSearch) found, on Anthropic
    /// and OpenAI. Empty when the answer cites none: without web search, on
    /// other providers, from the cache.
    ///
    /// ```
    /// # fn show(answer: &elyra_ai::Response) {
    /// for c in answer.citations() {
    ///     println!("{} — {}", &answer.text()[c.span.clone()], c.url);
    /// }
    /// # }
    /// ```
    pub fn citations(&self) -> &[Citation] {
        &self.citations
    }

    /// Parse the response text as JSON into `T`.
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.text).map_err(|e| Error::Decode(e.to_string()))
    }
}

impl std::fmt::Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// A source a web-search answer cites (RFC 0051).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Citation {
    /// The page.
    pub url: String,
    /// The page's title, when the lab gives one.
    pub title: Option<String>,
    /// The words quoted from the page (Anthropic sends them, up to about 150
    /// characters; OpenAI doesn't).
    pub cited_text: Option<String>,
    /// The part of [`Response::text`] this source backs: a byte range on
    /// `char` boundaries, so `&text[span]` is the words.
    pub span: Range<usize>,
}

impl Citation {
    /// A citation of `url` backing `span` of the text; for tests and
    /// [`AiFake::text_citing`](crate::AiFake::text_citing).
    pub fn new(url: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            url: url.into(),
            title: None,
            cited_text: None,
            span,
        }
    }

    /// The page's title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The words quoted from the page.
    pub fn cited_text(mut self, text: impl Into<String>) -> Self {
        self.cited_text = Some(text.into());
        self
    }
}
