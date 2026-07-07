//! M3 wiring tests: providers, the middleware pipeline, and `Result` commands.
//! All exercised through `App::prepare()` — no window required.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use elyra::command::BoxFuture;
use elyra::{
    command, commands, App, CommandRequest, Container, Ctx, Middleware, Next, Provider, Result,
};
use serde::{Deserialize, Serialize};

// --- A service + the provider that binds it --------------------------------

struct Config {
    greeting: String,
}

struct ConfigProvider;

impl Provider for ConfigProvider {
    fn register(&self, container: &mut Container) {
        container.bind(Config {
            greeting: "hi".into(),
        });
    }

    fn boot(&self, ctx: &Ctx) {
        // The container is fully populated during boot.
        assert_eq!(ctx.get::<Config>().greeting, "hi");
    }
}

#[command]
async fn welcome(ctx: Ctx, who: String) -> String {
    format!("{} {who}", ctx.get::<Config>().greeting)
}

#[command]
async fn divide(_ctx: Ctx, a: i32, b: i32) -> std::result::Result<i32, String> {
    if b == 0 {
        Err("division by zero".into())
    } else {
        Ok(a / b)
    }
}

// --- A counting middleware --------------------------------------------------

#[derive(Clone)]
struct Counter(Arc<AtomicUsize>);

impl Middleware for Counter {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, Result<Vec<u8>>> {
        let hits = self.0.clone();
        Box::pin(async move {
            hits.fetch_add(1, Ordering::SeqCst);
            next.run(ctx, req).await
        })
    }
}

#[tokio::test]
async fn provider_binds_service_and_middleware_wraps_dispatch() {
    let hits = Arc::new(AtomicUsize::new(0));

    let prepared = App::new()
        .provider(ConfigProvider)
        .middleware(Counter(hits.clone()))
        .commands(commands![welcome, divide])
        .prepare();

    let reg = prepared.registry;
    let ctx = prepared.ctx;

    let args = rmp_serde::to_vec(&("world",)).unwrap();
    let out = reg.clone().dispatch(ctx, "welcome", &args).await.unwrap();
    let greeting: String = rmp_serde::from_slice(&out).unwrap();

    assert_eq!(greeting, "hi world"); // provider-bound Config resolved in the command
    assert_eq!(hits.load(Ordering::SeqCst), 1); // middleware ran exactly once
}

#[tokio::test]
async fn result_command_maps_ok_and_err() {
    let prepared = App::new().commands(commands![divide]).prepare();
    let reg = prepared.registry;
    let ctx = prepared.ctx;

    let ok = reg
        .clone()
        .dispatch(
            ctx.clone(),
            "divide",
            &rmp_serde::to_vec(&(10i32, 2i32)).unwrap(),
        )
        .await
        .unwrap();
    let value: i32 = rmp_serde::from_slice(&ok).unwrap();
    assert_eq!(value, 5);

    let err = reg
        .dispatch(ctx, "divide", &rmp_serde::to_vec(&(1i32, 0i32)).unwrap())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("division by zero"));
}

// Result commands surface their Ok type to codegen.
#[derive(Serialize, Deserialize, specta::Type)]
struct Account {
    balance: i32,
}

#[command]
async fn open_account(_ctx: Ctx) -> std::result::Result<Account, String> {
    Ok(Account { balance: 0 })
}

#[test]
fn result_command_codegen_uses_ok_type() {
    let mut reg = elyra::CommandRegistry::new();
    reg.extend(commands![open_account]);
    let ts = elyra::codegen::generate(&reg).unwrap();
    assert!(ts.contains("open_account(): Promise<Account>"));
}
