//! The service container and the per-command [`Ctx`].
//!
//! Mirrors Laravel's container. Three ways to bind, all resolved the same way —
//! `ctx.get::<T>()`:
//!
//! ```ignore
//! c.bind(Config::load()?);                                  // a concrete singleton
//! c.bind_as::<dyn Mailer>(Arc::new(SmtpMailer::new()));     // interface -> implementation
//! c.bind_lazy::<dyn Mailer>(|ctx| Arc::new(SmtpMailer::from(&*ctx.get::<Config>())));
//!
//! let mailer = ctx.get::<dyn Mailer>();                     // Arc<dyn Mailer>
//! ```
//!
//! `bind_as` is what makes the `substrate` contracts useful: bind
//! `dyn substrate::Cache` once and depend on the trait, not the backend.
//! `bind_lazy` builds on first resolution, with a full [`Ctx`] — the answer to
//! "`register` must not resolve other services": a lazy binding *can*, because it
//! runs after every provider has registered.
//!
//! Bindings are shared (`Arc`) and must be `Send + Sync` because commands run on
//! the tokio runtime.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

type Build<T> = Box<dyn Fn(&Ctx) -> Arc<T> + Send + Sync>;

/// A singleton built on first resolution.
struct Lazy<T: ?Sized> {
    cell: OnceLock<Arc<T>>,
    build: Build<T>,
}

/// Each binding stores an `Arc<T>` (or a `Lazy<T>`) behind `dyn Any`. Storing
/// the `Arc` rather than `T` itself is what lets `T` be unsized — `dyn Trait`.
enum Binding {
    Ready(Box<dyn Any + Send + Sync>),
    Lazy(Box<dyn Any + Send + Sync>),
}

/// A type-keyed registry of shared singletons.
#[derive(Default)]
pub struct Container {
    bindings: HashMap<TypeId, Binding>,
}

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a singleton. A later bind of the same type replaces the previous one.
    pub fn bind<T: Any + Send + Sync>(&mut self, value: T) {
        self.bind_as::<T>(Arc::new(value));
    }

    /// Bind a shared value under `T`, which may be a trait object — Laravel's
    /// `bind(Interface::class, Impl::class)`:
    ///
    /// ```
    /// # use std::sync::Arc;
    /// # use elyra::Container;
    /// trait Greeter: Send + Sync { fn hi(&self) -> String; }
    /// struct English;
    /// impl Greeter for English { fn hi(&self) -> String { "hello".into() } }
    ///
    /// let mut c = Container::new();
    /// c.bind_as::<dyn Greeter>(Arc::new(English));
    /// assert_eq!(c.get::<dyn Greeter>().unwrap().hi(), "hello");
    /// ```
    pub fn bind_as<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) {
        self.bindings
            .insert(TypeId::of::<T>(), Binding::Ready(Box::new(value)));
    }

    /// Bind a singleton that is built the first time it is resolved, with a full
    /// [`Ctx`] — so it may depend on anything else in the container, whichever
    /// provider bound it. `T` may be a trait object.
    ///
    /// A dependency cycle (`A` needs `B` needs `A`) panics with the chain of
    /// types instead of deadlocking.
    pub fn bind_lazy<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        build: impl Fn(&Ctx) -> Arc<T> + Send + Sync + 'static,
    ) {
        let lazy: Lazy<T> = Lazy {
            cell: OnceLock::new(),
            build: Box::new(build),
        };
        self.bindings
            .insert(TypeId::of::<T>(), Binding::Lazy(Box::new(lazy)));
    }

    /// Whether anything is bound under `T` (built or not).
    pub fn has<T: ?Sized + 'static>(&self) -> bool {
        self.bindings.contains_key(&TypeId::of::<T>())
    }

    /// Resolve a singleton, if one is bound for `T`.
    ///
    /// Without a [`Ctx`] a lazy binding can't be built, so one that hasn't been
    /// resolved yet reads as `None` here; use [`Ctx::get`] once the app is up.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        match self.bindings.get(&TypeId::of::<T>())? {
            Binding::Ready(any) => any.downcast_ref::<Arc<T>>().cloned(),
            Binding::Lazy(any) => any.downcast_ref::<Lazy<T>>()?.cell.get().cloned(),
        }
    }

    /// Resolve `T`, building a lazy binding with `ctx` if needed.
    fn resolve<T: ?Sized + Send + Sync + 'static>(&self, ctx: &Ctx) -> Option<Arc<T>> {
        match self.bindings.get(&TypeId::of::<T>())? {
            Binding::Ready(any) => any.downcast_ref::<Arc<T>>().cloned(),
            Binding::Lazy(any) => {
                let lazy = any.downcast_ref::<Lazy<T>>()?;
                if let Some(built) = lazy.cell.get() {
                    return Some(built.clone());
                }
                let _guard = Resolving::enter::<T>();
                Some(lazy.cell.get_or_init(|| (lazy.build)(ctx)).clone())
            }
        }
    }
}

thread_local! {
    /// The lazy bindings being built on this thread, outermost first.
    static RESOLVING: RefCell<Vec<(TypeId, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// Marks a lazy binding as "being built" for cycle detection; pops on drop, so a
/// panicking factory doesn't leave a stale entry behind.
struct Resolving;

impl Resolving {
    fn enter<T: ?Sized + 'static>() -> Self {
        let id = TypeId::of::<T>();
        let name = std::any::type_name::<T>();
        RESOLVING.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.iter().any(|(seen, _)| *seen == id) {
                let chain: Vec<&str> = stack
                    .iter()
                    .map(|(_, n)| *n)
                    .chain(std::iter::once(name))
                    .collect();
                drop(stack);
                panic!(
                    "circular dependency between lazy bindings: {}",
                    chain.join(" -> ")
                );
            }
            stack.push((id, name));
        });
        Resolving
    }
}

impl Drop for Resolving {
    fn drop(&mut self) {
        RESOLVING.with(|stack| {
            stack.borrow_mut().pop();
        });
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

    /// Resolve a bound singleton (building a lazy one on first use), panicking
    /// if it is missing. `T` may be a trait object: `ctx.get::<dyn Mailer>()`.
    ///
    /// Missing bindings are a wiring bug, not a runtime condition — fail loudly.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.container.resolve::<T>(self).unwrap_or_else(|| {
            panic!(
                "no binding registered for `{}` — did you forget App::bind()?",
                std::any::type_name::<T>()
            )
        })
    }

    /// Fallible resolution for optional dependencies.
    pub fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.container.resolve::<T>(self)
    }

    /// Whether anything is bound under `T`, without building it.
    pub fn has<T: ?Sized + 'static>(&self) -> bool {
        self.container.has::<T>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    trait Greeter: Send + Sync {
        fn hi(&self) -> String;
    }
    struct English;
    impl Greeter for English {
        fn hi(&self) -> String {
            "hello".into()
        }
    }
    struct Norwegian(String);
    impl Greeter for Norwegian {
        fn hi(&self) -> String {
            self.0.clone()
        }
    }

    fn ctx(c: Container) -> Ctx {
        Ctx::new(Arc::new(c))
    }

    #[test]
    fn a_trait_object_resolves_to_its_implementation() {
        let mut c = Container::new();
        c.bind_as::<dyn Greeter>(Arc::new(English));
        assert_eq!(ctx(c).get::<dyn Greeter>().hi(), "hello");
    }

    #[test]
    fn a_later_binding_replaces_the_implementation() {
        let mut c = Container::new();
        c.bind_as::<dyn Greeter>(Arc::new(English));
        c.bind_as::<dyn Greeter>(Arc::new(Norwegian("hei".into())));
        assert_eq!(ctx(c).get::<dyn Greeter>().hi(), "hei");
    }

    #[test]
    fn concrete_and_trait_bindings_are_separate_keys() {
        let mut c = Container::new();
        c.bind(English);
        assert!(c.has::<English>());
        assert!(!c.has::<dyn Greeter>());
    }

    #[test]
    fn a_lazy_binding_builds_once_on_first_use_and_can_depend_on_others() {
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let mut c = Container::new();
        // Bound *before* its dependency: order doesn't matter, only resolution.
        c.bind_lazy::<dyn Greeter>(|ctx| {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Arc::new(Norwegian(format!("{}!", ctx.get::<String>())))
        });
        c.bind(String::from("hei"));
        assert!(c.get::<dyn Greeter>().is_none(), "not built without a Ctx");

        let ctx = ctx(c);
        assert_eq!(BUILDS.load(Ordering::SeqCst), 0, "nothing built yet");
        assert_eq!(ctx.get::<dyn Greeter>().hi(), "hei!");
        assert_eq!(ctx.get::<dyn Greeter>().hi(), "hei!");
        assert_eq!(
            BUILDS.load(Ordering::SeqCst),
            1,
            "a singleton, not a factory"
        );
    }

    struct A;
    struct B;

    #[test]
    fn a_lazy_cycle_panics_with_the_chain_instead_of_deadlocking() {
        let mut c = Container::new();
        c.bind_lazy::<A>(|ctx| {
            ctx.get::<B>();
            Arc::new(A)
        });
        c.bind_lazy::<B>(|ctx| {
            ctx.get::<A>();
            Arc::new(B)
        });
        let ctx = ctx(c);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.get::<A>();
        }))
        .expect_err("a cycle must panic");
        let message = err.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(message.contains("circular dependency"), "{message}");
        assert!(message.contains("::A -> "), "{message}");
        assert!(message.contains("::B -> "), "{message}");

        // The guard unwound cleanly: a fresh, acyclic lazy binding still works
        // on this thread.
        let mut c = Container::new();
        c.bind_lazy::<A>(|_| Arc::new(A));
        assert!(Ctx::new(Arc::new(c)).try_get::<A>().is_some());
    }

    #[test]
    fn a_missing_binding_names_the_type() {
        let err = std::panic::catch_unwind(|| {
            ctx(Container::new()).get::<dyn Greeter>();
        })
        .expect_err("missing binding must panic");
        let message = err.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(message.contains("Greeter"), "{message}");
    }
}
