use base64::Engine;
use serde_json::{json, Value};

use crate::{
    client::Ai,
    error::{Error, Result},
    provider::Provider,
};

/// A fluent image-generation request (OpenAI images).
pub struct ImageRequest<'a> {
    ai: &'a Ai,
    prompt: String,
    size: String,
    quality: Option<String>,
}

impl<'a> ImageRequest<'a> {
    pub(crate) fn new(ai: &'a Ai, prompt: String) -> Self {
        Self {
            ai,
            prompt,
            size: "1024x1024".into(),
            quality: None,
        }
    }

    /// Explicit size, e.g. `"1024x1024"`.
    pub fn size(mut self, size: impl Into<String>) -> Self {
        self.size = size.into();
        self
    }

    /// 16:9-ish landscape (1536×1024).
    pub fn landscape(mut self) -> Self {
        self.size = "1536x1024".into();
        self
    }

    /// Portrait (1024×1536).
    pub fn portrait(mut self) -> Self {
        self.size = "1024x1536".into();
        self
    }

    /// Square (1024×1024).
    pub fn square(mut self) -> Self {
        self.size = "1024x1024".into();
        self
    }

    /// Quality: `"high"`, `"medium"`, `"low"`, or `"auto"`.
    pub fn quality(mut self, quality: impl Into<String>) -> Self {
        self.quality = Some(quality.into());
        self
    }

    /// Generate the image, returning the decoded bytes.
    pub async fn generate(self) -> Result<GeneratedImage> {
        let key = self.ai.key(Provider::OpenAI)?.to_string();
        let url = format!(
            "{}/v1/images/generations",
            self.ai.base_url(Provider::OpenAI)
        );
        let mut body = json!({
            "model": self.ai.image_model(),
            "prompt": self.prompt,
            "size": self.size,
            "n": 1,
        });
        if let Some(q) = &self.quality {
            body["quality"] = json!(q);
        }

        let resp = self
            .ai
            .send(self.ai.http.post(&url).bearer_auth(&key).json(&body))
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
        let b64 = val["data"][0]["b64_json"].as_str().ok_or(Error::Empty)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(GeneratedImage { bytes })
    }
}

/// A generated image.
pub struct GeneratedImage {
    bytes: Vec<u8>,
}

impl GeneratedImage {
    /// The raw image bytes (PNG).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume and return the bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Write the image to `path`.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, &self.bytes)
    }
}
