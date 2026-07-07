//! The service container and the per-command [`Ctx`].
//!
//! Mirrors Laravel's container: bind singletons by type, resolve them anywhere
//! via `ctx.get::<T>()`. Bindings are shared (`Arc`) and must be `Send + Sync`
//! because commands run on the tokio runtime.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// A type-keyed registry of shared singletons.
#[derive(Default)]
pub struct Container {
    bindings: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a singleton. A later bind of the same type replaces the previous one.
    pub fn bind<T: Any + Send + Sync>(&mut self, value: T) {
        self.bindings.insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Resolve a singleton, if one is bound for `T`.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.bindings
            .get(&TypeId::of::<T>())
            .and_then(|any| any.clone().downcast::<T>().ok())
    }
}

/// The context handed to every command. Cheap to clone (an `Arc` bump).
#[derive(Clone)]
pub struct Ctx {
    container: Arc<Container>,
}

impl Ctx {
    pub fn new(container: Arc<Container>) -> Self {
        Self { container }
    }

    /// Resolve a bound singleton, panicking if it is missing.
    ///
    /// Missing bindings are a wiring bug, not a runtime condition — fail loudly.
    pub fn get<T: Any + Send + Sync>(&self) -> Arc<T> {
        self.container.get::<T>().unwrap_or_else(|| {
            panic!(
                "no binding registered for `{}` — did you forget App::bind()?",
                std::any::type_name::<T>()
            )
        })
    }

    /// Fallible resolution for optional dependencies.
    pub fn try_get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.container.get::<T>()
    }
}
