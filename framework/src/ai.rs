//! AI SDK integration (behind the `ai` feature).
//!
//! Re-exports the [`elyra-ai`](elyra_ai) crate as `elyra::ai` and adds
//! [`AiProvider`], which binds an [`Ai`](elyra_ai::Ai) client (configured from
//! the environment) into the container so commands can resolve it with
//! `ctx.get::<elyra::ai::Ai>()`.
//!
//! ```no_run
//! use elyra::App;
//! use elyra::ai::{Ai, AiProvider};
//!
//! # fn demo() {
//! App::new().provider(AiProvider).run().unwrap();
//! // inside a #[command]: let ai = ctx.get::<Ai>();
//! # }
//! ```

pub use elyra_ai::*;

use crate::{Container, Provider as ElyraProvider};

/// An Elyra [`Provider`](crate::Provider) that binds an [`Ai`] client built from
/// the environment (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …).
pub struct AiProvider;

impl ElyraProvider for AiProvider {
    fn register(&self, container: &mut Container) {
        container.bind(elyra_ai::Ai::from_env());
    }
}
