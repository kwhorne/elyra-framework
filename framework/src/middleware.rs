//! The command middleware pipeline — Elyra's HTTP-middleware analogue.
//!
//! Middleware wraps [`CommandRegistry::dispatch`](crate::command::CommandRegistry::dispatch),
//! forming an onion around the command call: cross-cutting concerns like
//! logging, timing, auth, and rate limiting live here instead of in every
//! command. Registration order is outermost-first (the first added wraps all
//! the rest), matching Laravel's global middleware stack.
//!
//! ```ignore
//! struct Timing;
//! impl Middleware for Timing {
//!     fn handle(&self, ctx: Ctx, req: CommandRequest, next: Next)
//!         -> BoxFuture<'static, Result<Vec<u8>>>
//!     {
//!         Box::pin(async move {
//!             let started = std::time::Instant::now();
//!             let name = req.name.clone();
//!             let out = next.run(ctx, req).await;
//!             eprintln!("{name} took {:?}", started.elapsed());
//!             out
//!         })
//!     }
//! }
//!
//! App::new().middleware(Timing).commands(commands![..]).run()
//! ```

use std::sync::Arc;

use crate::command::{BoxFuture, CommandRegistry};
use crate::{Ctx, Result};

/// A command invocation as it flows through the pipeline.
///
/// `args` is the raw MessagePack argument body — opaque here, but available for
/// middleware that wants to inspect or short-circuit before decoding.
pub struct CommandRequest {
    pub name: String,
    pub args: Vec<u8>,
}

/// The continuation handed to a middleware: call the next one, or — at the end
/// of the chain — the command itself.
pub struct Next {
    registry: Arc<CommandRegistry>,
    index: usize,
}

impl Next {
    pub(crate) fn new(registry: Arc<CommandRegistry>) -> Self {
        Self { registry, index: 0 }
    }

    /// Continue the pipeline.
    pub fn run(self, ctx: Ctx, req: CommandRequest) -> BoxFuture<'static, Result<Vec<u8>>> {
        Box::pin(async move {
            match self.registry.middleware().get(self.index) {
                Some(mw) => {
                    let mw = mw.clone();
                    let next = Next {
                        registry: self.registry.clone(),
                        index: self.index + 1,
                    };
                    mw.handle(ctx, req, next).await
                }
                None => self.registry.invoke(ctx, &req.name, &req.args).await,
            }
        })
    }
}

/// A pipeline stage. Must be `Send + Sync + 'static` (runs on the tokio runtime).
pub trait Middleware: Send + Sync + 'static {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, Result<Vec<u8>>>;
}
