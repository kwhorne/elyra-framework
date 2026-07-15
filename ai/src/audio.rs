use serde_json::{json, Value};

use crate::{
    client::Ai,
    error::{Error, Result},
    provider::Provider,
};

/// A fluent text-to-speech request (OpenAI audio).
pub struct SpeechRequest<'a> {
    ai: &'a Ai,
    input: String,
    voice: String,
    model: Option<String>,
    instructions: Option<String>,
    format: String,
}

impl<'a> SpeechRequest<'a> {
    pub(crate) fn new(ai: &'a Ai, input: String) -> Self {
        Self {
            ai,
            input,
            voice: "alloy".into(),
            model: None,
            instructions: None,
            format: "mp3".into(),
        }
    }

    /// Pick a specific voice (e.g. `"nova"`, `"onyx"`, `"alloy"`).
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = voice.into();
        self
    }

    /// A typically-female voice.
    pub fn female(mut self) -> Self {
        self.voice = "nova".into();
        self
    }

    /// A typically-male voice.
    pub fn male(mut self) -> Self {
        self.voice = "onyx".into();
        self
    }

    /// Override the TTS model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Steer delivery (e.g. "Speak like a pirate").
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Output format: `"mp3"` (default), `"wav"`, `"opus"`, `"flac"`, `"aac"`, `"pcm"`.
    pub fn format(mut self, format: impl Into<String>) -> Self {
        self.format = format.into();
        self
    }

    /// Synthesize speech, returning the audio bytes.
    pub async fn generate(self) -> Result<GeneratedAudio> {
        let key = self.ai.key(Provider::OpenAI)?.to_string();
        let url = format!("{}/v1/audio/speech", self.ai.base_url(Provider::OpenAI));
        let mut body = json!({
            "model": self.model.clone().unwrap_or_else(|| self.ai.tts_model().to_string()),
            "input": self.input,
            "voice": self.voice,
            "response_format": self.format,
        });
        if let Some(i) = &self.instructions {
            body["instructions"] = json!(i);
        }

        let resp = self
            .ai
            .send(self.ai.http.post(&url).bearer_auth(&key).json(&body))
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                message,
            });
        }
        let bytes = resp.bytes().await?.to_vec();
        Ok(GeneratedAudio { bytes })
    }
}

/// Generated audio bytes.
pub struct GeneratedAudio {
    bytes: Vec<u8>,
}

impl GeneratedAudio {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, &self.bytes)
    }
}

/// A fluent speech-to-text (transcription) request (OpenAI audio).
pub struct TranscriptionRequest<'a> {
    ai: &'a Ai,
    bytes: Vec<u8>,
    filename: String,
    model: Option<String>,
    language: Option<String>,
}

impl<'a> TranscriptionRequest<'a> {
    pub(crate) fn new(ai: &'a Ai, bytes: Vec<u8>, filename: String) -> Self {
        Self {
            ai,
            bytes,
            filename,
            model: None,
            language: None,
        }
    }

    /// Override the transcription model (default `whisper-1`).
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Hint the source language (ISO-639-1, e.g. `"en"`, `"no"`).
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Transcribe, returning the recognized text.
    pub async fn generate(self) -> Result<String> {
        let key = self.ai.key(Provider::OpenAI)?.to_string();
        let url = format!(
            "{}/v1/audio/transcriptions",
            self.ai.base_url(Provider::OpenAI)
        );
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| self.ai.transcribe_model().to_string());

        let part = reqwest::multipart::Part::bytes(self.bytes)
            .file_name(self.filename)
            .mime_str("application/octet-stream")
            .map_err(|e| Error::Http(e.to_string()))?;
        let mut form = reqwest::multipart::Form::new()
            .text("model", model)
            .part("file", part);
        if let Some(lang) = self.language {
            form = form.text("language", lang);
        }

        let resp = self
            .ai
            .send(self.ai.http.post(&url).bearer_auth(&key).multipart(form))
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
        val["text"].as_str().map(str::to_string).ok_or(Error::Empty)
    }
}
