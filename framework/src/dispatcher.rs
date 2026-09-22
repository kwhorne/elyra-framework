//! Domain events inside the app — Laravel's `Event::dispatch` and listeners.
//!
//! Not to be confused with [`EventBus`], which pushes values
//! *out* to the webview. The [`Dispatcher`] is in-process: one part of the Rust
//! side announces that something happened, and whoever cares reacts, without the
//! announcer knowing who they are.
//!
//! ```ignore
//! #[derive(Clone)]
//! struct OrderShipped { order_id: i64 }
//!
//! App::new()
//!     .listen(|e: OrderShipped, ctx: Ctx| async move {
//!         ctx.get::<Mailer>().send_shipped(e.order_id).await
//!     })
//!     // Forward to the frontend too, as a typed `channel("orders:shipped")`:
//!     .broadcast::<OrderShipped>("orders:shipped");
//!
//! // in a command, a job, a provider:
//! ctx.dispatch(OrderShipped { order_id: 7 }).await?;
//! ```
//!
//! An event is any `Clone + Send + Sync + 'static` type; each listener gets its
//! own copy. Listeners run **in registration order, one after another**, and the
//! first error stops the chain and is returned to the dispatcher — so a command
//! that dispatches learns that a listener failed. For fire-and-forget, use
//! [`Ctx::dispatch_background`], which runs the chain on its own task and logs
//! an error instead of returning it.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::command::BoxFuture;
use crate::{Ctx, EventBus, Result};

type Listener =
    Arc<dyn Fn(&(dyn Any + Send + Sync), Ctx) -> BoxFuture<'static, Result<()>> + Send + Sync>;

/// Routes dispatched events to their listeners. Bound in every app's container;
/// reach it with `ctx.get::<Dispatcher>()` to register listeners from a
/// provider's `register` or `boot`.
#[derive(Default)]
pub struct Dispatcher {
    listeners: RwLock<HashMap<TypeId, Vec<Listener>>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `listener` whenever an `E` is dispatched.
    pub fn listen<E, F, Fut>(&self, listener: F)
    where
        E: Clone + Send + Sync + 'static,
        F: Fn(E, Ctx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let erased: Listener = Arc::new(move |event, ctx| {
            // The map is keyed by `TypeId::of::<E>()`, so this cannot miss.
            let event = event
                .downcast_ref::<E>()
                .expect("listener registered under the wrong event type")
                .clone();
            Box::pin(listener(event, ctx))
        });
        self.listeners
            .write()
            .entry(TypeId::of::<E>())
            .or_default()
            .push(erased);
    }

    /// Also emit every dispatched `E` on the frontend [`EventBus`] under
    /// `channel` — Laravel's `ShouldBroadcast`. Pair it with
    /// [`App::event`](crate::App::event) (or use [`App::broadcast`](crate::App::broadcast),
    /// which does both) so the channel is typed in the generated bindings.
    pub fn broadcast<E>(&self, channel: &'static str)
    where
        E: serde::Serialize + Clone + Send + Sync + 'static,
    {
        self.listen(move |event: E, ctx: Ctx| async move {
            match ctx.try_get::<EventBus>() {
                Some(bus) => bus.emit(channel, &event),
                None => Ok(()),
            }
        });
    }

    /// How many listeners are registered for `E`.
    pub fn listener_count<E: 'static>(&self) -> usize {
        self.listeners
            .read()
            .get(&TypeId::of::<E>())
            .map_or(0, Vec::len)
    }

    /// Run every listener for `event`, in order, stopping at the first error.
    /// An event nobody listens for is not an error.
    pub async fn dispatch<E>(&self, ctx: &Ctx, event: E) -> Result<()>
    where
        E: Clone + Send + Sync + 'static,
    {
        // Snapshot, then release the lock: a listener may register another
        // listener, or dispatch a follow-up event, without deadlocking.
        let listeners: Vec<Listener> = self
            .listeners
            .read()
            .get(&TypeId::of::<E>())
            .cloned()
            .unwrap_or_default();
        for listener in listeners {
            listener(&event, ctx.clone()).await?;
        }
        Ok(())
    }
}

impl Ctx {
    /// Dispatch a domain event to its listeners and wait for them — see
    /// [`Dispatcher`]. The first listener error is returned.
    pub async fn dispatch<E>(&self, event: E) -> Result<()>
    where
        E: Clone + Send + Sync + 'static,
    {
        match self.try_get::<Dispatcher>() {
            Some(dispatcher) => dispatcher.dispatch(self, event).await,
            None => Ok(()),
        }
    }

    /// Dispatch on a background task and return immediately. A listener error is
    /// logged under `elyra::events` rather than returned. Needs a tokio runtime —
    /// commands, jobs and scheduled tasks have one; outside a runtime the event is
    /// dropped with an error log.
    pub fn dispatch_background<E>(&self, event: E)
    where
        E: Clone + Send + Sync + 'static,
    {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            crate::error!(
                target: "elyra::events",
                "dispatch_background({}) outside a tokio runtime; event dropped",
                std::any::type_name::<E>()
            );
            return;
        };
        let ctx = self.clone();
        handle.spawn(async move {
            if let Err(e) = ctx.dispatch(event).await {
                crate::error!(
                    target: "elyra::events",
                    "a listener for {} failed: {e}",
                    std::any::type_name::<E>()
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Container, Error};
    use parking_lot::Mutex;

    #[derive(Clone)]
    struct Shipped(i64);

    #[derive(Clone)]
    struct Unrelated;

    fn ctx_with(dispatcher: Arc<Dispatcher>) -> Ctx {
        let mut c = Container::new();
        c.bind_as::<Dispatcher>(dispatcher);
        Ctx::new(Arc::new(c))
    }

    #[tokio::test]
    async fn listeners_run_in_registration_order_with_their_own_copy() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let d = Arc::new(Dispatcher::new());
        for tag in ["first", "second"] {
            let seen = seen.clone();
            d.listen(move |e: Shipped, _ctx| {
                let seen = seen.clone();
                async move {
                    seen.lock().push(format!("{tag}:{}", e.0));
                    Ok(())
                }
            });
        }
        let ctx = ctx_with(d.clone());
        ctx.dispatch(Shipped(7)).await.unwrap();
        assert_eq!(*seen.lock(), vec!["first:7", "second:7"]);
        assert_eq!(d.listener_count::<Shipped>(), 2);
        assert_eq!(d.listener_count::<Unrelated>(), 0);
    }

    #[tokio::test]
    async fn an_event_nobody_listens_for_is_fine() {
        let ctx = ctx_with(Arc::new(Dispatcher::new()));
        ctx.dispatch(Unrelated).await.unwrap();
        // And a context without a dispatcher at all (a bare test container).
        Ctx::new(Arc::new(Container::new()))
            .dispatch(Unrelated)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_first_error_stops_the_chain_and_reaches_the_caller() {
        let ran_after = Arc::new(Mutex::new(false));
        let d = Arc::new(Dispatcher::new());
        d.listen(|_: Shipped, _| async { Err(Error::command("mailer down")) });
        let flag = ran_after.clone();
        d.listen(move |_: Shipped, _| {
            let flag = flag.clone();
            async move {
                *flag.lock() = true;
                Ok(())
            }
        });
        let err = ctx_with(d).dispatch(Shipped(1)).await.unwrap_err();
        assert_eq!(err.to_string(), "mailer down");
        assert!(!*ran_after.lock(), "later listeners must not run");
    }

    #[tokio::test]
    async fn a_listener_can_dispatch_a_follow_up_event() {
        // The listener lock is released before listeners run.
        let seen = Arc::new(Mutex::new(Vec::new()));
        let d = Arc::new(Dispatcher::new());
        d.listen(|e: Shipped, ctx: Ctx| async move {
            if e.0 == 1 {
                ctx.dispatch(Shipped(2)).await?;
            }
            Ok(())
        });
        let s = seen.clone();
        d.listen(move |e: Shipped, _| {
            let s = s.clone();
            async move {
                s.lock().push(e.0);
                Ok(())
            }
        });
        ctx_with(d).dispatch(Shipped(1)).await.unwrap();
        assert_eq!(*seen.lock(), vec![2, 1]);
    }

    #[tokio::test]
    async fn broadcast_forwards_to_the_event_bus() {
        #[derive(Clone, serde::Serialize)]
        struct Progress {
            percent: u8,
        }
        let d = Arc::new(Dispatcher::new());
        d.broadcast::<Progress>("progress");
        let bus = EventBus::new();
        bus.register_client("w");
        let mut c = Container::new();
        c.bind_as::<Dispatcher>(d);
        c.bind(bus.clone());
        let ctx = Ctx::new(Arc::new(c));

        ctx.dispatch(Progress { percent: 40 }).await.unwrap();
        assert_eq!(bus.pending_for("w"), 1);
    }

    #[tokio::test]
    async fn dispatch_background_returns_before_the_listener_finishes() {
        let (tx, rx) = tokio::sync::oneshot::channel::<i64>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let d = Arc::new(Dispatcher::new());
        d.listen(move |e: Shipped, _| {
            let tx = tx.clone();
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                if let Some(tx) = tx.lock().take() {
                    let _ = tx.send(e.0);
                }
                Ok(())
            }
        });
        let ctx = ctx_with(d);
        ctx.dispatch_background(Shipped(9));
        assert_eq!(rx.await.unwrap(), 9);
    }
}
