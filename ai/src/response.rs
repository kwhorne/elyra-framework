use serde::de::DeserializeOwned;

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
