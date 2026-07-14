use serde_json::{json, Value};

use crate::{
    client::Ai,
    error::{Error, Result},
    provider::Provider,
};

/// A fluent embeddings request (OpenAI embeddings).
pub struct EmbeddingRequest<'a> {
    ai: &'a Ai,
    inputs: Vec<String>,
    model: Option<String>,
    dimensions: Option<u32>,
}

impl<'a> EmbeddingRequest<'a> {
    pub(crate) fn new(ai: &'a Ai, inputs: Vec<String>) -> Self {
        Self {
            ai,
            inputs,
            model: None,
            dimensions: None,
        }
    }

    /// Override the embedding model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Request a specific number of dimensions.
    pub fn dimensions(mut self, dimensions: u32) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    /// Generate embeddings — one vector per input, in order.
    pub async fn generate(self) -> Result<Vec<Vec<f32>>> {
        let key = self.ai.key(Provider::OpenAI)?.to_string();
        let url = format!("{}/v1/embeddings", self.ai.base_url(Provider::OpenAI));
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| self.ai.embed_model().to_string());
        let mut body = json!({"model": model, "input": self.inputs});
        if let Some(d) = self.dimensions {
            body["dimensions"] = json!(d);
        }

        let resp = self
            .ai
            .http
            .post(&url)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let val: Value = resp
            .json()
            .await
            .map_err(|e| Error::Decode(e.to_string()))?;
        if !status.is_success() {
            let message = val["error"]["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string();
            return Err(Error::Api {
                status: status.as_u16(),
                message,
            });
        }
        let data = val["data"].as_array().ok_or(Error::Empty)?;
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let vec = item["embedding"]
                .as_array()
                .ok_or(Error::Empty)?
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            out.push(vec);
        }
        Ok(out)
    }
}
