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
