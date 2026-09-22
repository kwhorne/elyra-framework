//! Per-command middleware: aliases, groups, ordering, and failing loudly on an
//! unknown name.

use std::sync::Arc;

use elyra::command::BoxFuture;
use elyra::testing::TestApp;
use elyra::{command, commands, App, CommandRequest, Ctx, Middleware, Next};
use parking_lot::Mutex;

type Log = Arc<Mutex<Vec<String>>>;

/// Records `tag` on the way in, then continues.
struct Tag(&'static str, Log);

impl Middleware for Tag {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, elyra::Result<Vec<u8>>> {
        self.1.lock().push(format!("{}:{}", self.0, req.name));
        next.run(ctx, req)
    }
}

/// Refuses every call — an auth check with no session.
struct Deny;

impl Middleware for Deny {
    fn handle(
        &self,
        _ctx: Ctx,
        _req: CommandRequest,
        _next: Next,
    ) -> BoxFuture<'static, elyra::Result<Vec<u8>>> {
        Box::pin(async { Err(elyra::Error::command("not signed in")) })
    }
}

#[command]
async fn public(ctx: Ctx) {
    ctx.get::<Log>().lock().push("body:public".into());
}

#[command(middleware = "audit")]
async fn audited(ctx: Ctx) {
    ctx.get::<Log>().lock().push("body:audited".into());
}

#[command(middleware = ["auth", "audit"])]
async fn ordered(ctx: Ctx) {
    ctx.get::<Log>().lock().push("body:ordered".into());
}

#[command(middleware = ["admin", "audit"])]
async fn grouped(ctx: Ctx) {
    ctx.get::<Log>().lock().push("body:grouped".into());
}

fn app(log: &Log) -> TestApp {
    TestApp::new(
        App::new()
            .bind(log.clone())
            .middleware(Tag("global", log.clone()))
            .middleware_alias("auth", Tag("auth", log.clone()))
            .middleware_alias("audit", Tag("audit", log.clone()))
            .middleware_group("admin", ["auth", "audit"])
            .commands(commands![public, audited, ordered, grouped]),
    )
}

#[tokio::test]
async fn a_named_middleware_runs_only_for_the_commands_that_ask_for_it() {
    let log = Log::default();
    let app = app(&log);
    app.invoke_ok::<()>("public", ()).await;
    app.invoke_ok::<()>("audited", ()).await;
    assert_eq!(
        *log.lock(),
        vec![
            "global:public",
            "body:public",
            "global:audited",
            "audit:audited",
            "body:audited"
        ]
    );
}

#[tokio::test]
async fn global_runs_outermost_then_the_command_middleware_in_declared_order() {
    let log = Log::default();
    app(&log).invoke_ok::<()>("ordered", ()).await;
    assert_eq!(
        *log.lock(),
        vec![
            "global:ordered",
            "auth:ordered",
            "audit:ordered",
            "body:ordered"
        ]
    );
}

#[tokio::test]
async fn a_group_expands_in_order_and_a_repeated_middleware_runs_once() {
    // `admin` = [auth, audit], then `audit` again: audit must not run twice.
    let log = Log::default();
    app(&log).invoke_ok::<()>("grouped", ()).await;
    assert_eq!(
        *log.lock(),
        vec![
            "global:grouped",
            "auth:grouped",
            "audit:grouped",
            "body:grouped"
        ]
    );
}

#[tokio::test]
async fn a_command_middleware_can_refuse_the_call() {
    let log = Log::default();
    let app = TestApp::new(
        App::new()
            .bind(log.clone())
            .middleware_alias("auth", Deny)
            .middleware_alias("audit", Tag("audit", log.clone()))
            .commands(commands![ordered, public]),
    );
    assert_eq!(app.invoke_err("ordered", ()).await, "not signed in");
    assert!(
        log.lock().is_empty(),
        "neither later middleware nor the body may run after a refusal"
    );
    // A command without it is unaffected.
    app.invoke_ok::<()>("public", ()).await;
}

fn startup_panic(app: App) -> String {
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = TestApp::new(app);
    }))
    .expect_err("the app must refuse to start");
    err.downcast_ref::<String>().cloned().unwrap_or_default()
}

#[test]
fn an_unknown_middleware_name_stops_the_app_at_startup() {
    // The alternative is `audited` silently running without its middleware.
    let msg = startup_panic(App::new().commands(commands![audited]));
    assert!(msg.contains("invalid middleware wiring"), "{msg}");
    assert!(msg.contains("`audited`"), "{msg}");
    assert!(msg.contains("`audit`"), "{msg}");
}

#[test]
fn a_group_cycle_is_reported_not_looped() {
    let log = Log::default();
    let msg = startup_panic(
        App::new()
            .middleware_alias("auth", Tag("auth", log))
            .middleware_group("admin", ["auth", "staff"])
            .middleware_group("staff", ["admin"])
            .commands(commands![grouped]),
    );
    assert!(msg.contains("cycle"), "{msg}");
    assert!(msg.contains("admin -> staff -> admin"), "{msg}");
}
