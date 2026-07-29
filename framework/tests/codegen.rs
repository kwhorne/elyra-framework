//! Codegen contract test: the generated `bindings.ts` shape is locked here so
//! CI catches drift without needing a window or a frontend build.

use elyra::{codegen, command, commands, CommandRegistry, Ctx};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, specta::Type)]
struct Info {
    name: String,
    count: u32,
}

#[command]
async fn hello(_ctx: Ctx, who: String) -> String {
    who
}

#[command]
async fn tally(_ctx: Ctx) -> Info {
    Info {
        name: "x".into(),
        count: 1,
    }
}

#[command]
async fn sum64(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

#[test]
fn generates_typed_bindings() {
    let mut registry = CommandRegistry::new();
    registry.extend(commands![hello, tally, sum64]);

    let ts = codegen::generate(&registry).expect("codegen should succeed");

    // Runtime import + named type declaration.
    assert!(ts.contains("import { invoke } from \"@elyra/runtime\";"));
    assert!(ts.contains("export type Info"));

    // Typed facade: scalars, a named-type return, and i64 -> number (the
    // ElyraFormat numeric policy) all render correctly.
    assert!(ts.contains("hello(who: string): Promise<string>"));
    assert!(ts.contains("tally(): Promise<Info>"));
    assert!(ts.contains("sum64(a: number, b: number): Promise<number>"));

    // The call delegates to invoke with the registered command name.
    assert!(ts.contains("return invoke(\"hello\", who);"));
    assert!(ts.contains("return invoke(\"tally\");"));
}

#[derive(Serialize, Deserialize, specta::Type)]
struct Ledger {
    id: i64,
    balance: f64,
}

#[command]
async fn ledger(_ctx: Ctx) -> Ledger {
    Ledger {
        id: 1,
        balance: 2.0,
    }
}

#[test]
fn coerces_bigint_and_float_struct_fields() {
    let mut registry = CommandRegistry::new();
    registry.extend(commands![ledger]);
    let ts = codegen::generate(&registry).expect("codegen should succeed with i64 struct field");

    // The i64 field renders as `number` (no bigint error), and the f64 field as
    // `number` (not specta-typescript's `number | null`).
    assert!(ts.contains("export type Ledger"));
    assert!(ts.contains("id: number"));
    assert!(ts.contains("balance: number"));
    assert!(!ts.contains("number | null"));
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct Meta {
    created_at: i64,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct Account {
    first_name: String,
    #[serde(flatten)]
    meta: Meta,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Shape {
    RoundThing { radius: f64 },
    Square,
}

#[command]
async fn account(_ctx: Ctx) -> Account {
    Account {
        first_name: "a".into(),
        meta: Meta { created_at: 1 },
    }
}

#[command]
async fn shape(_ctx: Ctx) -> Shape {
    Shape::Square
}

#[test]
fn reflects_serde_container_attributes() {
    let mut registry = CommandRegistry::new();
    registry.extend(commands![account, shape]);
    let ts = codegen::generate(&registry).expect("codegen should succeed");

    // `rename_all = "camelCase"` applies to struct fields.
    assert!(ts.contains("firstName: string"));
    assert!(!ts.contains("first_name"));

    // `flatten` merges the nested struct (as a TS intersection); the renamed
    // i64 field is coerced to `number`.
    assert!(ts.contains("} & Meta"));
    assert!(ts.contains("createdAt: number"));

    // Internally tagged enum -> discriminated union with snake_case variants;
    // the f64 field is coerced to `number` (not `number | null`).
    assert!(ts.contains(r#"kind: "round_thing""#));
    assert!(ts.contains(r#"kind: "square""#));
    assert!(ts.contains("radius: number"));
    assert!(!ts.contains("number | null"));
}

// --- typed event channels + number policy -----------------------------------

#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
struct Progress {
    percent: u8,
    label: String,
}

#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
struct Counter {
    /// Beyond 2^53, so the number policy matters.
    total: i64,
}

#[elyra::command]
async fn ping(_ctx: elyra::Ctx) -> i64 {
    1
}

/// `ELYRA_CODEGEN_OUT` is process-global, so codegen runs must not overlap.
static CODEGEN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn bindings(app: elyra::App) -> String {
    let _guard = CODEGEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let out = std::env::temp_dir().join(format!(
        "elyra-bindings-{}-{}.ts",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var("ELYRA_CODEGEN_OUT", &out);
    app.run().expect("codegen mode must succeed");
    std::env::remove_var("ELYRA_CODEGEN_OUT");
    let ts = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(out);
    ts
}

#[test]
fn emits_a_typed_event_map_and_channel_helper() {
    let ts = bindings(
        elyra::App::new()
            .commands(elyra::commands![ping])
            .event::<Progress>("progress")
            .event::<Counter>("counter"),
    );

    assert!(ts.contains("export type ElyraEvents = {"), "{ts}");
    assert!(ts.contains("\"progress\": Progress;"), "{ts}");
    assert!(ts.contains("\"counter\": Counter;"), "{ts}");
    assert!(ts.contains("export type ElyraEventName = keyof ElyraEvents;"));
    // The narrowed helper delegates to the runtime's untyped channel.
    assert!(ts.contains("export function channel<K extends ElyraEventName>"));
    assert!(ts.contains("channel as rawChannel"));
    // The payload interfaces themselves are exported.
    assert!(ts.contains("percent: number"));
}

#[test]
fn without_registered_events_nothing_extra_is_emitted() {
    let ts = bindings(elyra::App::new().commands(elyra::commands![ping]));
    assert!(!ts.contains("ElyraEvents"));
    assert!(!ts.contains("rawChannel"));
    assert!(ts.contains("export const api = {"));
}

#[test]
fn the_bigint_policy_widens_64_bit_integers() {
    let numbers = bindings(
        elyra::App::new()
            .commands(elyra::commands![ping])
            .event::<Counter>("counter"),
    );
    assert!(numbers.contains("total: number"), "{numbers}");

    let bigints = bindings(
        elyra::App::new()
            .commands(elyra::commands![ping])
            .event::<Counter>("counter")
            .codegen_bigint(),
    );
    assert!(bigints.contains("total: bigint"), "{bigints}");
}
