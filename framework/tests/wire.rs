//! End-to-end wire-contract tests for the M0 bridge.
//!
//! These run the exact path the protocol handler takes — decode a compact
//! MessagePack argument array, dispatch, encode a named result — but without a
//! window, so they're pure and CI-friendly. They mirror what `@elyra/runtime`
//! sends: `encode([...])` on the way in, `decode(bytes)` on the way out.

use std::collections::BTreeMap;
use std::sync::Arc;

use elyra::{command, commands, CommandRegistry, Container, Ctx};
use serde::{Deserialize, Serialize};

struct Multiplier(i64);

#[command]
async fn scale(ctx: Ctx, x: i64) -> i64 {
    x * ctx.get::<Multiplier>().0
}

#[derive(Serialize, Deserialize, specta::Type, PartialEq, Debug)]
struct Point {
    x: i64,
    y: i64,
}

#[command]
async fn shift(_ctx: Ctx, p: Point) -> Point {
    Point {
        x: p.x + 1,
        y: p.y + 1,
    }
}

#[command]
async fn ping(_ctx: Ctx) -> String {
    "pong".into()
}

fn setup() -> (Arc<CommandRegistry>, Ctx) {
    let mut container = Container::new();
    container.bind(Multiplier(3));
    let ctx = Ctx::new(Arc::new(container));

    let mut registry = CommandRegistry::new();
    registry.extend(commands![scale, shift, ping]);

    (Arc::new(registry), ctx)
}

#[tokio::test]
async fn scalar_arg_uses_container() {
    let (reg, ctx) = setup();

    // Frontend: invoke("scale", 7) -> encode([7])  (compact array).
    let args = rmp_serde::to_vec(&(7i64,)).unwrap();
    let out = reg.dispatch(ctx, "scale", &args).await.unwrap();

    let value: i64 = rmp_serde::from_slice(&out).unwrap();
    assert_eq!(value, 21);
}

#[tokio::test]
async fn struct_result_is_named_map() {
    let (reg, ctx) = setup();

    // Argument struct arrives inside the args array as a msgpack map.
    let args = rmp_serde::to_vec(&(Point { x: 10, y: 20 },)).unwrap();
    let out = reg.dispatch(ctx, "shift", &args).await.unwrap();

    // Decodes back to the typed struct...
    let point: Point = rmp_serde::from_slice(&out).unwrap();
    assert_eq!(point, Point { x: 11, y: 21 });

    // ...and, crucially, is a *named* map (field names preserved), which is
    // what lets it become a plain `{ x, y }` object on the JS side.
    let as_map: BTreeMap<String, i64> = rmp_serde::from_slice(&out).unwrap();
    assert_eq!(as_map.get("x"), Some(&11));
    assert_eq!(as_map.get("y"), Some(&21));
}

#[tokio::test]
async fn zero_arg_command_ignores_body() {
    let (reg, ctx) = setup();

    // Zero-arg commands ignore the request body entirely.
    let out = reg.dispatch(ctx, "ping", &[]).await.unwrap();
    let value: String = rmp_serde::from_slice(&out).unwrap();
    assert_eq!(value, "pong");
}

#[tokio::test]
async fn unknown_command_errors() {
    let (reg, ctx) = setup();
    let err = reg.dispatch(ctx, "nope", &[]).await.unwrap_err();
    assert!(err.to_string().contains("unknown command"));
}
