//! # Elyra AI SDK
//!
//! An ergonomic, Laravel-inspired AI SDK for Elyra apps: **agents**, **tools**,
//! **structured output**, **images**, and **embeddings** over Anthropic and
//! OpenAI. Async (tokio + reqwest), keys stay in the Rust backend.
//!
//! ```no_run
//! use elyra_ai::{Ai, Agent};
//!
//! # async fn demo() -> elyra_ai::Result<()> {
//! let ai = Ai::from_env();
//!
//! // Anonymous agent (one-off chat):
//! let reply = ai.chat()
//!     .instructions("You are a concise Rust expert.")
//!     .prompt("What is ownership?")
//!     .await?;
//! println!("{reply}");
//! # Ok(()) }
//! ```
//!
//! Named agents implement [`Agent`] and are prompted with [`Ai::prompt`].
//! Structured output uses [`Chat::prompt_as`] with a `serde` + `schemars` type.

mod agent;
mod anthropic;
mod audio;
mod chat;
mod client;
mod embeddings;
mod error;
mod image;
mod message;
mod openai;
mod provider;
mod provider_tool;
mod request;
mod response;
mod stream;
mod subagent;
mod tool;
mod vector;

pub use agent::Agent;
pub use audio::{GeneratedAudio, SpeechRequest, TranscriptionRequest};
pub use chat::Chat;
pub use client::{Ai, AiBuilder};
pub use embeddings::EmbeddingRequest;
pub use error::{Error, Result};
pub use image::{GeneratedImage, ImageRequest};
pub use message::{Message, Role};
pub use provider::Provider;
pub use provider_tool::{UserLocation, WebFetch, WebSearch};
pub use response::{Response, Usage};
pub use stream::{StreamChunk, TextStream};
pub use subagent::AgentTool;
pub use tool::Tool;
pub use vector::{cosine_similarity, Match, VectorStore};

// Re-exports for tool/agent authors so they don't need matching versions.
pub use async_trait::async_trait;
pub use schemars::{self, JsonSchema};
pub use serde_json::{json, Value};
