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

    /// The ability the **frontend** must hold to call this command, declared as
    /// `#[command(can = "posts.delete")]`.
    ///
    /// `None` — the default — means the blanket
    /// [`Capability::Commands`](crate::security::Capability::Commands) grant is
    /// enough. A declared ability is denied by default: the app must grant it
    /// with `App::allow_ability`. Rust-side dispatch is never gated by this;
    /// it is a limit on what a script in the webview can reach.
    fn ability(&self) -> Option<&'static str> {
        None
    }

    /// Named middleware this command runs through, in order, from
    /// `#[command(middleware = ["auth", "audit"])]`. Each name is an alias or a
    /// group registered with `App::middleware_alias` / `App::middleware_group`;
    /// they run inside the global middleware, outermost first.
    fn middleware(&self) -> &'static [&'static str] {
        &[]
    }

    /// Decode `args`, run the handler, encode the result.
    fn call<'a>(&'a self, ctx: Ctx, args: &'a [u8]) -> BoxFuture<'a, Result<Vec<u8>>>;

    /// The command's type signature, registering referenced types into `types`.
    fn signature(&self, types: &mut specta::Types) -> CommandSig;
}

/// A resolved middleware chain: the global stack, then a command's own.
pub(crate) type Chain = Arc<[Arc<dyn Middleware>]>;

/// Routes command names to their [`Command`] implementations, through the
/// middleware pipeline.
#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<&'static str, Box<dyn Command>>,
    middleware: Vec<Arc<dyn Middleware>>,
    /// Named middleware a command can ask for (`App::middleware_alias`).
    aliases: HashMap<String, Arc<dyn Middleware>>,
    /// Named lists of aliases or other groups (`App::middleware_group`).
    groups: HashMap<String, Vec<String>>,
    /// Chains resolved once by [`finalize`](CommandRegistry::finalize).
    chains: HashMap<&'static str, Chain>,
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

    /// Register a named middleware that commands can ask for.
    pub fn alias_middleware(&mut self, name: impl Into<String>, mw: Arc<dyn Middleware>) {
        self.aliases.insert(name.into(), mw);
    }

    /// Register a named group of aliases (or other groups).
    pub fn middleware_group(&mut self, name: impl Into<String>, members: Vec<String>) {
        self.groups.insert(name.into(), members);
    }

    /// Expand one name (alias or group) onto `out`, skipping names already in
    /// the chain and refusing group cycles.
    fn expand(
        &self,
        command: &str,
        name: &str,
        stack: &mut Vec<String>,
        seen: &mut Vec<String>,
        out: &mut Vec<Arc<dyn Middleware>>,
    ) -> std::result::Result<(), String> {
        if let Some(mw) = self.aliases.get(name) {
            if !seen.iter().any(|s| s == name) {
                seen.push(name.to_owned());
                out.push(mw.clone());
            }
            return Ok(());
        }
        let Some(members) = self.groups.get(name) else {
            return Err(format!(
                "command `{command}` uses middleware `{name}`, which is not registered \
                 (App::middleware_alias or App::middleware_group)"
            ));
        };
        if stack.iter().any(|s| s == name) {
            stack.push(name.to_owned());
            return Err(format!("middleware group cycle: {}", stack.join(" -> ")));
        }
        stack.push(name.to_owned());
        for member in members {
            self.expand(command, member, stack, seen, out)?;
        }
        stack.pop();
        Ok(())
    }

    /// The full chain for `name`: the global stack, then the command's own.
    fn resolve(&self, name: &str) -> std::result::Result<Chain, String> {
        let mut chain = self.middleware.clone();
        if let Some(cmd) = self.commands.get(name) {
            let mut seen = Vec::new();
            for mw in cmd.middleware() {
                self.expand(name, mw, &mut Vec::new(), &mut seen, &mut chain)?;
            }
        }
        Ok(chain.into())
    }

    /// Resolve every command's chain once, failing on an unknown middleware
    /// name. Called when the app starts, so a misspelt `"auht"` stops the app
    /// instead of silently running the command without its middleware.
    pub fn finalize(&mut self) -> std::result::Result<(), String> {
        let mut chains = HashMap::new();
        let mut names: Vec<&'static str> = self.commands.keys().copied().collect();
        names.sort_unstable();
        for name in names {
            chains.insert(name, self.resolve(name)?);
        }
        self.chains = chains;
        Ok(())
    }

    /// Dispatch `name` through the middleware pipeline, then the command.
    pub async fn dispatch(self: Arc<Self>, ctx: Ctx, name: &str, args: &[u8]) -> Result<Vec<u8>> {
        // The cached chain when finalized; otherwise resolve now — never skip a
        // middleware because a registry was used without `finalize`.
        let chain = match self.chains.get(name) {
            Some(chain) => chain.clone(),
            None => self.resolve(name).map_err(Error::Command)?,
        };
        let req = CommandRequest {
            name: name.to_owned(),
            args: args.to_vec(),
        };
        Next::new(self, chain).run(ctx, req).await
    }

    /// The pipeline terminal: resolve and invoke the command itself.
    pub(crate) async fn invoke(&self, ctx: Ctx, name: &str, args: &[u8]) -> Result<Vec<u8>> {
        let cmd = self
            .commands
            .get(name)
            .ok_or_else(|| Error::UnknownCommand(name.to_string()))?;
        cmd.call(ctx, args).await
    }

    /// The ability `name` declares, if it is registered and declares one.
    ///
    /// An unregistered name yields `None` and falls through to dispatch, which
    /// answers with `UnknownCommand` — the shell must not turn a typo into an
    /// authorization error.
    pub(crate) fn ability_of(&self, name: &str) -> Option<&'static str> {
        self.commands.get(name).and_then(|cmd| cmd.ability())
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
