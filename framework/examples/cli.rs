//! A command-line twin of an app (RFC 0054): the same commands, no window.
//! Built without elyra's `gui` feature, it needs no tao, wry or GTK.
//!
//! ```sh
//! cargo run -p elyra --no-default-features --example cli -- add 2 3
//! ```

use elyra::{command, commands, App, Ctx};

#[command]
async fn add(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

#[command]
async fn greet(_ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // What the app started is ended when this ends, however it ends (RFC 0053).
    let _exit = elyra::exit::guard();
    let app = App::new().commands(commands![add, greet]).prepare();

    let mut args = std::env::args().skip(1);
    let Some(name) = args.next() else {
        eprintln!("usage: cli <command> [args…]");
        return std::process::ExitCode::from(2);
    };
    // Each argument as JSON where it reads as JSON (`2`, `true`), else a string.
    let args: Vec<serde_json::Value> = args
        .map(|a| serde_json::from_str(&a).unwrap_or(serde_json::Value::String(a)))
        .collect();
    match app.call_json(&name, args).await {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{name}: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
