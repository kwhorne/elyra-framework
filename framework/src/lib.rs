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

#[cfg(feature = "backend")]
pub mod abilities;
pub mod about;
mod agent;
#[cfg(feature = "ai")]
pub mod ai;
pub mod app;
mod app_id;
pub mod assets;
#[cfg(feature = "ai")]
pub mod assistant;
#[cfg(feature = "backend")]
pub mod auth;
#[cfg(feature = "autostart")]
pub mod autostart;
#[cfg(feature = "backend")]
pub mod backend;
mod badge;
#[cfg(feature = "backend")]
pub mod broadcasting;
pub mod cache;
pub mod codegen;
pub mod command;
pub mod config;
pub mod container;
mod deeplink;
/// The running app, for the agent working on it (RFC 0026): debug builds only.
#[cfg(debug_assertions)]
mod dev;
pub mod diagnostics;
pub mod dispatcher;
#[cfg(all(feature = "backend", feature = "system"))]
pub mod editing;
pub mod error;
pub mod event;
pub mod files;
#[cfg(feature = "http")]
pub mod http;
pub mod i18n;
pub mod icon;
pub mod inspector;
mod instance;
mod launcher;
mod lifetime;
#[cfg(feature = "backend")]
pub mod listening;
#[cfg(feature = "database")]
pub mod live;
pub mod log;
pub mod matrix;
pub mod mcp;
pub mod menu;
pub mod middleware;
#[cfg(feature = "backend")]
pub mod notifications;
pub mod notifier;
#[cfg(all(feature = "backend", feature = "database"))]
pub mod offline;
#[cfg(feature = "backend")]
pub(crate) mod optimistic;
mod point;
#[cfg(feature = "backend")]
pub mod presence;
mod theme;
// Used by the event loop only with the tray.
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
mod popover;
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
#[cfg(feature = "backend")]
pub mod transfers;
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
pub use app::{App, Problem, Problems};
pub use assets::{asset_resolver, mime_for, Asset, AssetResolver};
#[cfg(feature = "backend")]
pub use auth::Auth;
#[cfg(feature = "backend")]
pub use backend::{Backend, BackendError};
#[cfg(feature = "backend")]
pub use broadcasting::{BackendEvent, Broadcasting};
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
pub use icon::Icon;
#[cfg(feature = "backend")]
pub use listening::Listening;
pub use log::{Level, LogProvider};
pub use mcp::{Completion, Mcp, Prompt};
pub use menu::{Menu, Submenu};
pub use middleware::{CommandRequest, Middleware, Next, Origin};
#[cfg(feature = "backend")]
pub use presence::Presence;
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
pub use tray::{Tray, TrayConfig, TrayItem};
pub use validation::{ValidationErrors, Validator};
pub use window::{WindowConfig, Windows};
pub use wire::WireError;

pub mod microphone;
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

/// Composite keys (RFC 0044), as the derive checks them. Two `#[model(id)]`
/// fields make the key `(order_id, line_no)`:
///
/// ```
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "order_lines")]
/// struct OrderLine {
///     #[model(id)]
///     order_id: i64,
///     #[model(id)]
///     line_no: i64,
///     qty: i64,
/// }
///
/// let _: <OrderLine as elyra::Model>::Key = (42i64, 3i64);
/// ```
///
/// Such a model can't be the parent of `belongs_to_many`, whose pivot holds
/// one `i64` per side (`has_many` and `has_one` work: see
/// [`relations_on_any_key`]):
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "tags")]
/// struct Tag { id: i64 }
///
/// #[derive(Model)]
/// #[model(table = "order_lines", belongs_to_many(Tag))]
/// struct OrderLine {
///     #[model(id)]
///     order_id: i64,
///     #[model(id)]
///     line_no: i64,
/// }
/// ```
///
/// And a key has at most four columns:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// struct Wide {
///     #[model(id)] a: i64,
///     #[model(id)] b: i64,
///     #[model(id)] c: i64,
///     #[model(id)] d: i64,
///     #[model(id)] e: i64,
/// }
/// ```
#[cfg(all(feature = "database", doc))]
pub mod composite_keys {}

/// Pivot columns (RFC 0046), as the derive checks them. A `belongs_to_many`
/// names the pivot's own model with `using`; the model has the pivot's two
/// keys as its composite key, and implements `Default`:
///
/// ```
/// use elyra::Model;
///
/// #[derive(Model, Default)]
/// #[model(table = "role_user")]
/// struct RoleUser {
///     #[model(id)]
///     user_id: i64,
///     #[model(id)]
///     role_id: i64,
///     note: String,
/// }
///
/// #[derive(Model)]
/// #[model(table = "roles")]
/// struct Role { id: i64, name: String }
///
/// #[derive(Model)]
/// #[model(table = "users", belongs_to_many(Role, using = RoleUser))]
/// struct User { id: i64 }
///
/// #[derive(Model)]
/// #[model(table = "users")]
/// struct UserWithRoles {
///     id: i64,
///     #[model(belongs_to_many(Role, using = RoleUser))]
///     roles: Vec<(Role, RoleUser)>,
/// }
/// ```
///
/// The pivot table is the model's, so `pivot` beside `using` is an error:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model, Default)]
/// #[model(table = "role_user")]
/// struct RoleUser {
///     #[model(id)]
///     user_id: i64,
///     #[model(id)]
///     role_id: i64,
///     note: String,
/// }
///
/// #[derive(Model)]
/// #[model(table = "roles")]
/// struct Role { id: i64, name: String }
///
/// #[derive(Model)]
/// #[model(table = "users", belongs_to_many(Role, using = RoleUser, pivot = "role_user"))]
/// struct User { id: i64 }
/// ```
///
/// A `using` model without `Default` (attaching by ids builds the row from it):
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "role_user")]
/// struct RoleUser {
///     #[model(id)]
///     user_id: i64,
///     #[model(id)]
///     role_id: i64,
/// }
///
/// #[derive(Model)]
/// #[model(table = "roles")]
/// struct Role { id: i64 }
///
/// #[derive(Model)]
/// #[model(table = "users", belongs_to_many(Role, using = RoleUser))]
/// struct User { id: i64 }
/// ```
///
/// A `using` model with a single key, which can't name a pivot row:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model, Default)]
/// #[model(table = "role_user")]
/// struct RoleUser { id: i64, user_id: i64, role_id: i64 }
///
/// #[derive(Model)]
/// #[model(table = "roles")]
/// struct Role { id: i64 }
///
/// #[derive(Model)]
/// #[model(table = "users", belongs_to_many(Role, using = RoleUser))]
/// struct User { id: i64 }
/// ```
///
/// And a `Vec<(R, T)>` field without `using`:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model, Default)]
/// #[model(table = "role_user")]
/// struct RoleUser {
///     #[model(id)]
///     user_id: i64,
///     #[model(id)]
///     role_id: i64,
///     note: String,
/// }
///
/// #[derive(Model)]
/// #[model(table = "roles")]
/// struct Role { id: i64, name: String }
///
/// #[derive(Model)]
/// #[model(table = "users")]
/// struct UserWithRoles {
///     id: i64,
///     #[model(belongs_to_many(Role, pivot = "role_user", fk = "user_id"))]
///     roles: Vec<(Role, RoleUser)>,
/// }
/// ```
#[cfg(all(feature = "database", doc))]
pub mod pivot_columns {}

/// Relations on any key (RFC 0047), as the derive checks them. A parent keyed
/// by a string, or by more than one column (`fk` then names a column per key
/// column, in the key's order):
///
/// ```
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "teams", has_many(Member, fk = "team_code", as = "members"))]
/// struct Team { #[model(id)] code: String }
///
/// #[derive(Model)]
/// #[model(table = "members", belongs_to(Team, fk = "team_code"))]
/// struct Member { id: i64, team_code: String }
///
/// #[derive(Model)]
/// #[model(table = "order_lines", has_many(LineNote, fk = ("order_id", "line_no"), as = "notes"))]
/// struct OrderLine { #[model(id)] order_id: i64, #[model(id)] line_no: i64 }
///
/// #[derive(Model)]
/// #[model(table = "line_notes", belongs_to(OrderLine, fk = ("order_id", "line_no"), as = "line"))]
/// struct LineNote { id: i64, order_id: i64, line_no: Option<i64> }
/// ```
///
/// A composite parent's `has_many` without `fk` (there's no default for two
/// columns):
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "line_notes")]
/// struct LineNote { id: i64, order_id: i64, line_no: i64 }
///
/// #[derive(Model)]
/// #[model(table = "order_lines", has_many(LineNote, as = "notes"))]
/// struct OrderLine { #[model(id)] order_id: i64, #[model(id)] line_no: i64 }
/// ```
///
/// An `fk` with another number of columns than the key:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "line_notes")]
/// struct LineNote { id: i64, order_id: i64, line_no: i64 }
///
/// #[derive(Model)]
/// #[model(table = "order_lines", has_many(LineNote, fk = "order_id", as = "notes"))]
/// struct OrderLine { #[model(id)] order_id: i64, #[model(id)] line_no: i64 }
/// ```
///
/// A `belongs_to` whose `fk` isn't one of the model's columns:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "teams")]
/// struct Team { #[model(id)] code: String }
///
/// #[derive(Model)]
/// #[model(table = "members", belongs_to(Team, fk = "team"))]
/// struct Member { id: i64, team_code: String }
/// ```
///
/// A `belongs_to` naming one column for an owner keyed by two:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "order_lines")]
/// struct OrderLine { #[model(id)] order_id: i64, #[model(id)] line_no: i64 }
///
/// #[derive(Model)]
/// #[model(table = "line_notes", belongs_to(OrderLine, fk = "order_id", as = "line"))]
/// struct LineNote { id: i64, order_id: i64 }
/// ```
///
/// And `belongs_to_many` to a model whose key isn't `i64`:
///
/// ```compile_fail
/// use elyra::Model;
///
/// #[derive(Model)]
/// #[model(table = "teams")]
/// struct Team { #[model(id)] code: String }
///
/// #[derive(Model)]
/// #[model(table = "users", belongs_to_many(Team))]
/// struct User { id: i64 }
/// ```
#[cfg(all(feature = "database", doc))]
pub mod relations_on_any_key {}

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
