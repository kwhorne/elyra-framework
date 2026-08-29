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
//! | Facades / HTTP client | generated `api.*` (M2) |

pub mod about;
#[cfg(feature = "ai")]
pub mod ai;
pub mod app;
pub mod assets;
#[cfg(feature = "autostart")]
pub mod autostart;
pub mod cache;
pub mod codegen;
pub mod command;
pub mod config;
pub mod container;
mod deeplink;
pub mod error;
pub mod event;
mod instance;
pub mod log;
pub mod menu;
pub mod middleware;
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
pub use cache::{Cache, CacheProvider};
pub use command::{Command, CommandRegistry};
pub use config::{Config, ConfigProvider};
pub use container::{Container, Ctx};
pub use error::{Error, Result};
pub use event::EventBus;
pub use log::{Level, LogProvider};
pub use menu::{Menu, Submenu};
pub use middleware::{CommandRequest, Middleware, Next};
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
    pub use crate::error::{Error, Result};
    pub use crate::event::EventBus;
    pub use crate::middleware::{CommandRequest, Middleware, Next};
    pub use crate::provider::Provider;
    pub use crate::window::{WindowConfig, Windows};
    pub use crate::{command, commands};

    #[cfg(feature = "database")]
    pub use crate::{Database, Model, Query};
}

#[doc(hidden)]
pub mod __private {
    //! Re-exports used by macro-generated code. Not a stable API.
    pub use crate::error::Error;
    pub use rmp_serde as rmp;
}
