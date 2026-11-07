//! Code execution against the real Messages API (RFC 0027): costs money, so
//! it's ignored unless asked for, with a key:
//!
//! ```sh
//! ANTHROPIC_API_KEY=… cargo test -p elyra-ai --test live_code_execution -- --ignored
//! ```

use std::sync::Arc;

use elyra_ai::{Ai, CodeEvent, Provider, Tool};
use parking_lot::Mutex;
use serde_json::{json, Value};

/// Invoice `id` totals `id × 100`; each call is counted.
struct InvoicesShow(Arc<Mutex<Vec<i64>>>);

#[elyra_ai::async_trait]
impl Tool for InvoicesShow {
    fn name(&self) -> String {
        "invoices_show".into()
    }
    fn description(&self) -> String {
        "One invoice by its id: a JSON object with `id` and `total`.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]})
    }
    async fn call(&self, args: Value) -> elyra_ai::Result<String> {
        let id = args["id"].as_i64().unwrap_or_default();
        self.0.lock().push(id);
        Ok(json!({"id": id, "total": id * 100}).to_string())
    }
    fn callable_from_code(&self) -> bool {
        true
    }
}

#[tokio::test]
#[ignore = "calls the real Messages API, which costs money"]
async fn code_calls_a_tool_through_the_real_api() {
    let ai = Ai::builder()
        .provider_key(
            Provider::Anthropic,
            std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY"),
        )
        .build();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::new(Mutex::new(Vec::new()));
    let tell = told.clone();
    let reply = ai
        .chat()
        .provider(Provider::Anthropic)
        .model("claude-haiku-5-5")
        .max_tokens(2000)
        .code_execution()
        .tool(InvoicesShow(calls.clone()))
        .on_code(move |event| tell.lock().push(event))
        .prompt(
            "Write code that calls invoices_show for ids 1, 2 and 3 and prints the sum of their \
             totals. Then tell me the sum, as a number.",
        )
        .await
        .expect("a reply");
    let calls = calls.lock().clone();
    let told = told.lock().clone();
    eprintln!(
        "answer: {}\ncalls: {calls:?}\nsteps: {}\ncode events: {told:#?}",
        reply.text(),
        reply.steps()
    );
    let mut ids = calls.clone();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2, 3], "each invoice once");
    assert!(
        told.iter().any(|e| matches!(e, CodeEvent::Started { .. })),
        "code ran"
    );
    assert!(
        reply.text().replace([',', ' ', '.'], "").contains("600"),
        "{}",
        reply.text()
    );
}
