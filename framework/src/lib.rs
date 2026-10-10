//! # Elyra Framework
//!
//! A Rust + Svelte 5 framework for hyper-responsive desktop apps. Laravel's
//! ergonomics — container, providers, a typed bridge — but compiled and binary,
//! with no runtime overhead.
//!
//! This is the **M0** milestone: tao + wry + a custom-protocol handler + one
//! `#[command]` end to end over MessagePack. See the module docs for the map
//! from Laravel concepts to Elyra ones.
//!
//! | Laravel | Elyra |
//! |---|---|
//! | Application + container | [`App`] + [`Container`] (`ctx.get::<T>()`) |
//! | routes/web.php | [`commands!`] |
//! | Controller | `#[command] async fn` |
//! | Middleware | pipeline in [`command::CommandRegistry::dispatch`] |
//! | Facades | generated `api.*` (M2) |
//! | `Http` facade | [`http::Http`] (feature `http`) |

pub mod about;
#[cfg(feature = "ai")]
pub mod ai;
pub mod app;
pub mod assets;
#[cfg(feature = "autostart")]
pub mod autostart;
#[cfg(feature = "backend")]
pub mod backend;
pub mod cache;
pub mod codegen;
pub mod command;
pub mod config;
pub mod container;
mod deeplink;
pub mod dispatcher;
pub mod error;
pub mod event;
#[cfg(feature = "http")]
pub mod http;
pub mod i18n;
mod instance;
#[cfg(feature = "database")]
pub mod live;
pub mod log;
pub mod mcp;
pub mod menu;
pub mod middleware;
mod proof;
pub mod provider;
pub mod queue;
pub mod ratelimit;
pub mod scheduler;
#[cfg(feature = "secrets")]
pub mod secrets;
pub mod security;
#[cfg(feature = "database")]
pub mod seeder;
pub mod shell;
#[cfg(feature = "sidecar")]
pub mod sidecar;
pub mod storage;
pub mod store;
pub mod testing;
pub mod tray;
#[cfg(feature = "updater")]
pub mod updater;
pub mod validation;
mod winstate;
pub mod wire;
#[cfg(feature = "updater")]
pub use updater::UpdaterConfig;
pub mod window;

pub use about::AboutInfo;
pub use app::App;
pub use assets::{asset_resolver, mime_for, Asset, AssetResolver};
#[cfg(feature = "backend")]
pub use backend::{Backend, BackendError};
pub use cache::{Cache, CacheProvider};
pub use command::{Command, CommandRegistry};
pub use config::{Config, ConfigProvider};
pub use container::{Container, Ctx};
pub use dispatcher::Dispatcher;
pub use error::{Error, Result};
pub use event::EventBus;
#[cfg(feature = "http")]
pub use http::{Http, HttpFake};
pub use i18n::{I18nProvider, Translator};
pub use log::{Level, LogProvider};
pub use mcp::Mcp;
pub use menu::{Menu, Submenu};
pub use middleware::{CommandRequest, Middleware, Next, Origin};
pub use provider::Provider;
pub use queue::{Queue, QueueProvider};
pub use ratelimit::RateLimiter;
pub use scheduler::{Scheduler, SchedulerProvider};
pub use security::Policy;
pub use storage::{Storage, StorageProvider};
pub use store::Store;
/// The shared, backend-agnostic Cache/Storage/Queue contracts, also implemented
/// by the Askr/Laravel side. Elyra's facades conform to these traits.
pub use substrate_core as substrate;
/// One page of results, in Laravel's paginator shape: what `Query::paginate`
/// returns and what a Laravel API answers (RFC 0005).
pub use substrate_core::Page;
pub use tray::{TrayConfig, TrayItem};
pub use validation::{ValidationErrors, Validator};
pub use window::{WindowConfig, Windows};
pub use wire::WireError;

#[cfg(feature = "system")]
pub mod system;

pub use elyra_macros::command;

/// Database drivers + migrations (behind the `database` feature).
#[cfg(feature = "database")]
pub use elyra_db as db;
/// Model factories: `User::factory().count(3).create(&db)`.
#[cfg(feature = "database")]
pub use elyra_db::factory::Factory;
/// Active-Record models: the `Model` trait, the `Query` builder, and the
/// `#[derive(Model)]` macro (same-name derive + trait, like serde).
#[cfg(feature = "database")]
pub use elyra_db::model::{Model, Query, Value};
#[cfg(feature = "database")]
pub use elyra_db::{Database, Driver};
#[cfg(feature = "database")]
pub use elyra_macros::Model;

/// Build a `Vec<Box<dyn Command>>` from `#[command]`-annotated functions.
///
/// Because `#[command]` turns each function into a unit struct of the same
/// name, you pass the bare identifiers:
///
/// ```ignore
/// App::new().commands(commands![greet, add, system_info]).run()
/// ```
#[macro_export]
macro_rules! commands {
    ($($cmd:expr),* $(,)?) => {
        ::std::vec![
            $( ::std::boxed::Box::new($cmd) as ::std::boxed::Box<dyn $crate::command::Command> ),*
        ]
    };
}

/// Everything a typical app touches, in one glob import.
///
/// Laravel apps `use App\Http\Controllers\Controller` and get the world; the
/// Rust equivalent is a prelude. It carries the app builder, the container
/// handles, the command macro pair, and the error type — nothing feature-gated
/// except the database items, so `use elyra::prelude::*;` compiles on any
/// feature set.
///
/// ```no_run
/// use elyra::prelude::*;
///
/// #[command]
/// async fn greet(_ctx: Ctx, name: String) -> String {
///     format!("Hello, {name}!")
/// }
///
/// App::new().commands(commands![greet]).run().unwrap();
/// ```
pub mod prelude {
    pub use crate::app::App;
    pub use crate::command::Command;
    pub use crate::container::{Container, Ctx};
    pub use crate::dispatcher::Dispatcher;
    pub use crate::error::{Error, Result};
    pub use crate::event::EventBus;
    pub use crate::middleware::{CommandRequest, Middleware, Next};
    pub use crate::provider::Provider;
    pub use crate::window::{WindowConfig, Windows};
    pub use crate::{command, commands};

    #[cfg(feature = "database")]
    pub use crate::{Database, Factory, Model, Query};
}

#[doc(hidden)]
pub mod __private {
    //! Re-exports used by macro-generated code. Not a stable API.
    pub use crate::error::Error;
    pub use rmp_serde as rmp;

    /// A command's `Err`, on its way to an [`Error`]: an `elyra::Error`
    /// passes through as it is — its kind (`Error::with_kind`) included —
    /// and anything else that's `Display` becomes `Error::Command`. Chosen at
    /// compile time by method resolution (autoref): `KeepError` applies to
    /// `&ErrorOf<Error>` directly, `DisplayError` only after one more `&`.
    /// The value is taken out of the cell, hence `&self`.
    pub struct ErrorOf<T>(pub std::cell::Cell<Option<T>>);

    pub trait KeepError {
        fn take_error(&self) -> Error;
    }

    impl KeepError for ErrorOf<Error> {
        fn take_error(&self) -> Error {
            self.0.take().expect("a command error is converted once")
        }
    }

    pub trait DisplayError {
        fn take_error(&self) -> Error;
    }

    impl<T: std::fmt::Display> DisplayError for &ErrorOf<T> {
        fn take_error(&self) -> Error {
            Error::command(self.0.take().expect("a command error is converted once"))
        }
    }
}
