//! Commands and the dispatch registry.
//!
//! A [`Command`] is the compiled equivalent of a Laravel controller action.
//! `#[command]` generates the [`Command`] impl; [`CommandRegistry`] is the
//! router, and `dispatch` is where the middleware pipeline will live (M3).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::middleware::{CommandRequest, Middleware, Next};
use crate::{Ctx, Error, Result};

/// A boxed, `Send` future — the return of every command invocation.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A command's type signature, collected for codegen (M2).
///
/// `#[command]` builds this from the function's argument names/types and return
/// type via [`specta::Type`]. Every argument and return type must therefore
/// implement `specta::Type`.
pub struct CommandSig {
    pub name: &'static str,
    pub args: Vec<(&'static str, specta::datatype::DataType)>,
    pub ret: specta::datatype::DataType,
}

/// A dispatchable command. Implemented by `#[command]`.
///
/// `args` is the raw MessagePack body of the request (a compact array of the
/// call arguments). The returned bytes are the MessagePack-encoded result.
pub trait Command: Send + Sync {
    /// The routing name, e.g. `"greet"`.
    fn name(&self) -> &'static str;

    /// Decode `args`, run the handler, encode the result.
    fn call<'a>(&'a self, ctx: Ctx, args: &'a [u8]) -> BoxFuture<'a, Result<Vec<u8>>>;

    /// The command's type signature, registering referenced types into `types`.
    fn signature(&self, types: &mut specta::Types) -> CommandSig;
}

/// Routes command names to their [`Command`] implementations, through the
/// middleware pipeline.
#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<&'static str, Box<dyn Command>>,
    middleware: Vec<Arc<dyn Middleware>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a single command.
    pub fn register(&mut self, cmd: Box<dyn Command>) {
        self.commands.insert(cmd.name(), cmd);
    }

    /// Register everything produced by `commands![...]`.
    pub fn extend(&mut self, cmds: Vec<Box<dyn Command>>) {
        for cmd in cmds {
            self.register(cmd);
        }
    }

    /// Append a middleware to the pipeline. Runs in registration order, outermost
    /// first (like Laravel: the first added wraps all the rest).
    pub fn add_middleware(&mut self, mw: Arc<dyn Middleware>) {
        self.middleware.push(mw);
    }

    pub(crate) fn middleware(&self) -> &[Arc<dyn Middleware>] {
        &self.middleware
    }

    /// Dispatch `name` through the middleware pipeline, then the command.
    pub async fn dispatch(self: Arc<Self>, ctx: Ctx, name: &str, args: &[u8]) -> Result<Vec<u8>> {
        let req = CommandRequest {
            name: name.to_owned(),
            args: args.to_vec(),
        };
        Next::new(self).run(ctx, req).await
    }

    /// The pipeline terminal: resolve and invoke the command itself.
    pub(crate) async fn invoke(&self, ctx: Ctx, name: &str, args: &[u8]) -> Result<Vec<u8>> {
        let cmd = self
            .commands
            .get(name)
            .ok_or_else(|| Error::UnknownCommand(name.to_string()))?;
        cmd.call(ctx, args).await
    }

    /// All registered command names.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.commands.keys().copied()
    }

    /// All registered commands (used by the M2 codegen).
    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> + '_ {
        self.commands.values().map(|boxed| boxed.as_ref())
    }
}
