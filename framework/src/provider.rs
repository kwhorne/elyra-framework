//! Service providers — Elyra's `ServiceProvider`.
//!
//! A [`Provider`] is the idiomatic place to wire up a slice of the app. Like
//! Laravel, it has two phases:
//!
//! - [`register`](Provider::register): bind services into the [`Container`].
//!   Runs for **all** providers first — do not resolve other services here, they
//!   may not be bound yet.
//! - [`boot`](Provider::boot): run once everything is registered, with a fully
//!   populated [`Ctx`]. Resolve dependencies, spawn background work, seed state.
//!
//! ```ignore
//! struct DbProvider;
//! impl Provider for DbProvider {
//!     fn register(&self, c: &mut Container) { c.bind(Db::connect()); }
//!     fn boot(&self, ctx: &Ctx) { ctx.get::<Db>().migrate(); }
//! }
//!
//! App::new().provider(DbProvider).run()
//! ```

use crate::container::{Container, Ctx};

/// A unit of application wiring. Both methods default to no-ops so a provider
/// can implement only the phase it needs.
pub trait Provider: 'static {
    /// Bind services. Runs before any `boot`, for every provider.
    fn register(&self, container: &mut Container) {
        let _ = container;
    }

    /// Run after all providers have registered, with a populated context.
    fn boot(&self, ctx: &Ctx) {
        let _ = ctx;
    }
}
