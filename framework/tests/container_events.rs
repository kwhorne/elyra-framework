//! Trait-object and lazy container bindings, and domain events, through the
//! real app wiring: `App` -> providers -> `prepare` -> `TestApp`.

use std::sync::Arc;

use elyra::testing::TestApp;
use elyra::{command, commands, App, Container, Ctx, Dispatcher, Provider};
use parking_lot::Mutex;

// --- a contract and two implementations ----------------------------------------

trait Mailer: Send + Sync {
    fn send(&self, to: &str) -> String;
}

struct LogMailer;
impl Mailer for LogMailer {
    fn send(&self, to: &str) -> String {
        format!("logged mail to {to}")
    }
}

/// Needs configuration from elsewhere in the container.
struct SmtpMailer {
    host: String,
}
impl Mailer for SmtpMailer {
    fn send(&self, to: &str) -> String {
        format!("smtp://{} -> {to}", self.host)
    }
}

struct MailConfig {
    host: String,
}

#[command]
async fn notify(ctx: Ctx, to: String) -> String {
    ctx.get::<dyn Mailer>().send(&to)
}

#[tokio::test]
async fn a_command_depends_on_the_trait_not_the_backend() {
    let app = TestApp::new(
        App::new()
            .bind_as::<dyn Mailer>(Arc::new(LogMailer))
            .commands(commands![notify]),
    );
    let out: String = app.invoke_ok("notify", ("ada@example.test",)).await;
    assert_eq!(out, "logged mail to ada@example.test");
}

/// Binds the mailer lazily in `register`, depending on config a *later*
/// provider binds — which a plain `bind` in `register` couldn't do.
struct MailProvider;
impl Provider for MailProvider {
    fn register(&self, c: &mut Container) {
        c.bind_lazy::<dyn Mailer>(|ctx| {
            Arc::new(SmtpMailer {
                host: ctx.get::<MailConfig>().host.clone(),
            })
        });
    }
}

struct ConfigProvider;
impl Provider for ConfigProvider {
    fn register(&self, c: &mut Container) {
        c.bind(MailConfig {
            host: "mail.example.test".into(),
        });
    }
}

#[tokio::test]
async fn a_lazy_binding_resolves_a_dependency_registered_after_it() {
    let app = TestApp::new(
        App::new()
            .provider(MailProvider)
            .provider(ConfigProvider)
            .commands(commands![notify]),
    );
    let out: String = app.invoke_ok("notify", ("grace@example.test",)).await;
    assert_eq!(out, "smtp://mail.example.test -> grace@example.test");
    // And the test harness resolves trait objects too.
    assert_eq!(
        app.get::<dyn Mailer>().send("x"),
        "smtp://mail.example.test -> x"
    );
}

// --- domain events --------------------------------------------------------------

#[derive(Clone, serde::Serialize, specta::Type)]
struct OrderShipped {
    order_id: i64,
}

/// Where listeners record what they saw, so a test can assert on it.
#[derive(Default)]
struct Journal(Mutex<Vec<String>>);

#[command]
async fn ship(ctx: Ctx, order_id: i64) -> elyra::Result<()> {
    ctx.dispatch(OrderShipped { order_id }).await
}

/// Registers its listener in `boot` — Laravel's `EventServiceProvider`.
struct AuditProvider;
impl Provider for AuditProvider {
    fn boot(&self, ctx: &Ctx) {
        ctx.get::<Dispatcher>()
            .listen(|e: OrderShipped, ctx: Ctx| async move {
                ctx.get::<Journal>()
                    .0
                    .lock()
                    .push(format!("audit {}", e.order_id));
                Ok(())
            });
    }
}

#[tokio::test]
async fn a_command_dispatches_to_app_and_provider_listeners_in_order() {
    let app = TestApp::new(
        App::new()
            .bind(Journal::default())
            .listen(|e: OrderShipped, ctx: Ctx| async move {
                ctx.get::<Journal>()
                    .0
                    .lock()
                    .push(format!("mail {}", e.order_id));
                Ok(())
            })
            .provider(AuditProvider)
            .commands(commands![ship]),
    );
    app.invoke_ok::<()>("ship", (7i64,)).await;
    assert_eq!(
        *app.get::<Journal>().0.lock(),
        vec!["mail 7", "audit 7"],
        "App::listen is registered before any provider boots"
    );
}

#[tokio::test]
async fn a_failing_listener_fails_the_dispatching_command() {
    let app = TestApp::new(
        App::new()
            .listen(|_: OrderShipped, _ctx: Ctx| async {
                Err(elyra::Error::command("warehouse offline"))
            })
            .commands(commands![ship]),
    );
    assert_eq!(app.invoke_err("ship", (1i64,)).await, "warehouse offline");
}

#[tokio::test]
async fn broadcast_reaches_the_frontend_channel() {
    let app = TestApp::new(
        App::new()
            .broadcast::<OrderShipped>("orders:shipped")
            .commands(commands![ship]),
    );
    app.listen();
    app.invoke_ok::<()>("ship", (42i64,)).await;
    app.assert_emitted("orders:shipped").await;
}
