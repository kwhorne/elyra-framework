//! Prompts: tasks an app offers by name — listed to an MCP client
//! (`prompts/list`, `prompts/get`) and suggested in the assistant's panel,
//! each only where the abilities it needs are granted.

use elyra::mcp::{Mcp, McpServer, Prompt};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

/// Invoices, a page at a time.
#[command(live, can = "invoices.view")]
async fn invoices_index(_ctx: Ctx) -> Vec<i64> {
    Vec::new()
}

/// Delete an invoice.
#[command(can = "invoices.delete")]
async fn invoices_destroy(_ctx: Ctx, id: i64) {
    let _ = id;
}

fn overdue() -> Prompt {
    Prompt::new(
        "overdue_invoices",
        "Overdue invoices",
        "Find the invoices that are overdue and draft a reminder for each.",
    )
    .description("Invoices past their due date, with a reminder each")
    .needs("invoices.view")
}

fn summary() -> Prompt {
    Prompt::new(
        "customer_summary",
        "Summarize a customer",
        "Summarize {customer}{period}.",
    )
    .argument("customer", "The customer's name or number")
    .optional_argument("period", "A period, like \" this year\"")
    .needs("invoices.view")
}

fn cleanup() -> Prompt {
    Prompt::new(
        "cleanup",
        "Clean up",
        "Delete the drafts older than a year.",
    )
    .needs("invoices.delete")
}

fn app(mcp: Mcp) -> TestApp {
    TestApp::new(
        App::new()
            .commands(commands![invoices_index, invoices_destroy])
            .mcp(mcp),
    )
}

fn names(list: &Value) -> Vec<&str> {
    list["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn an_agent_is_listed_the_prompts_its_grant_covers() {
    let app =
        app(Mcp::new()
            .allow_ability("invoices.view")
            .prompts([overdue(), summary(), cleanup()]));
    let mcp = app.mcp();
    let discover = mcp.request("server/discover", json!({})).await;
    assert!(discover["result"]["capabilities"]["prompts"].is_object());

    let list = mcp.request("prompts/list", json!({})).await;
    // `cleanup` needs `invoices.delete`, which the agent isn't granted.
    assert_eq!(names(&list), ["overdue_invoices", "customer_summary"]);
    assert!(list["result"]["ttlMs"].is_number());
    let summary = &list["result"]["prompts"][1];
    assert_eq!(summary["title"], "Summarize a customer");
    assert_eq!(
        summary["arguments"],
        json!([
            { "name": "customer", "description": "The customer's name or number", "required": true },
            { "name": "period", "description": "A period, like \" this year\"", "required": false },
        ])
    );
    // The list doesn't carry the text, nor what it needs.
    assert!(summary.get("text").is_none() && summary.get("needs").is_none());
    assert_eq!(
        list["result"]["prompts"][0]["description"],
        "Invoices past their due date, with a reminder each"
    );
}

#[tokio::test]
async fn getting_a_prompt_fills_in_its_arguments() {
    let app =
        app(Mcp::new()
            .allow_ability("invoices.*")
            .prompts([overdue(), summary(), cleanup()]));
    let mcp = app.mcp();
    let got = mcp
        .request(
            "prompts/get",
            json!({ "name": "customer_summary", "arguments": { "customer": "Ada", "period": " this year" } }),
        )
        .await;
    assert_eq!(
        got["result"]["messages"],
        json!([{ "role": "user", "content": { "type": "text", "text": "Summarize Ada this year." } }])
    );
    let plain = mcp
        .request("prompts/get", json!({ "name": "overdue_invoices" }))
        .await;
    assert_eq!(
        plain["result"]["messages"][0]["content"]["text"],
        "Find the invoices that are overdue and draft a reminder for each."
    );
    assert_eq!(
        plain["result"]["description"],
        "Invoices past their due date, with a reminder each"
    );

    // A required argument left out, an unknown prompt, one the grant
    // doesn't cover: invalid params.
    let missing = mcp
        .request(
            "prompts/get",
            json!({ "name": "customer_summary", "arguments": {} }),
        )
        .await;
    assert_eq!(missing["error"]["code"], -32602);
    assert!(missing["error"]["message"]
        .as_str()
        .unwrap()
        .contains("`customer` is required"));
    let unknown = mcp.request("prompts/get", json!({ "name": "nope" })).await;
    assert_eq!(unknown["error"]["message"], "Prompt not found");
    let narrow = self::app(Mcp::new().allow_ability("invoices.view").prompt(cleanup()));
    let hidden = narrow
        .mcp()
        .request("prompts/get", json!({ "name": "cleanup" }))
        .await;
    assert_eq!(hidden["error"]["message"], "Prompt not found");
    let wrong = mcp
        .request(
            "prompts/get",
            json!({ "name": "overdue_invoices", "arguments": [] }),
        )
        .await;
    assert_eq!(wrong["error"]["code"], -32602);
}

#[tokio::test]
async fn a_prompts_arguments_complete_to_nothing_but_exist() {
    let app = app(Mcp::new().allow_ability("invoices.*").prompt(summary()));
    let mcp = app.mcp();
    let done = mcp
        .request(
            "completion/complete",
            json!({ "ref": { "type": "ref/prompt", "name": "customer_summary" },
                    "argument": { "name": "customer", "value": "A" } }),
        )
        .await;
    assert_eq!(done["result"]["completion"]["values"], json!([]));
    let wrong = mcp
        .request(
            "completion/complete",
            json!({ "ref": { "type": "ref/prompt", "name": "customer_summary" },
                    "argument": { "name": "nope", "value": "" } }),
        )
        .await;
    assert_eq!(wrong["error"]["code"], -32602);
}

#[tokio::test]
async fn a_legacy_client_lists_and_gets_them_too() {
    let app = app(Mcp::new().allow_ability("invoices.*").prompt(overdue()));
    let mcp = app.mcp();
    mcp.raw(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "old", "version": "1" } },
    }))
    .await
    .unwrap();
    let list = mcp
        .raw(json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/list", "params": {} }))
        .await
        .unwrap();
    assert_eq!(names(&list), ["overdue_invoices"]);
    assert!(list["result"].get("ttlMs").is_none());
    let got = mcp
        .raw(json!({ "jsonrpc": "2.0", "id": 3, "method": "prompts/get", "params": { "name": "overdue_invoices" } }))
        .await
        .unwrap();
    assert_eq!(got["result"]["messages"][0]["role"], "user");
}

#[test]
fn a_malformed_prompt_stops_the_server_from_starting() {
    let prepared = App::new().commands(commands![invoices_index]).prepare();
    let build = |mcp: Mcp| {
        McpServer::new(
            prepared.ctx.clone(),
            prepared.registry.clone(),
            mcp,
            "test",
            "1",
        )
        .err()
    };
    let undeclared = Prompt::new("p", "P", "About {who}");
    assert!(build(Mcp::new().prompt(undeclared))
        .unwrap()
        .contains("`{who}`"));
    let twice = Mcp::new().prompts([overdue(), overdue()]);
    assert!(build(twice).unwrap().contains("two prompts"));
    assert_eq!(build(Mcp::new().prompts([overdue(), summary()])), None);
}

#[cfg(feature = "ai")]
mod assistant {
    use super::*;
    use elyra::ai::{Ai, AiFake};
    use elyra::assistant::Assistant;
    use elyra::security::Capability;
    use elyra::testing::TestShell;
    use elyra::window::WindowConfig;

    fn app_with(assistant: Assistant) -> App {
        App::new()
            .allow_ability("invoices.*")
            .commands(commands![invoices_index, invoices_destroy])
            .swap(Ai::fake(AiFake::new()))
            .assistant(assistant)
    }

    #[tokio::test]
    async fn the_panel_suggests_what_the_user_may_do() {
        let app = TestApp::new(app_with(
            Assistant::new().allow_ability("invoices.view").prompts([
                &overdue(),
                &summary(),
                &cleanup(),
            ]),
        ));
        // `cleanup` needs `invoices.delete`: not in the assistant's grant.
        assert_eq!(
            app.assistant().prompts(),
            ["overdue_invoices", "customer_summary"]
        );
        // Taken away (as the inspector does, in a debug build): no
        // suggestion needs it.
        if cfg!(debug_assertions) {
            app.get::<elyra::inspector::Mask>()
                .set("invoices.view", true);
            assert!(app.assistant().prompts().is_empty());
        }
    }

    #[tokio::test]
    async fn a_window_with_limits_gets_only_its_suggestions() {
        let prepared = app_with(
            Assistant::new()
                .allow_ability("invoices.*")
                .prompts([overdue(), cleanup()]),
        )
        .prepare();
        let shell = TestShell::new(prepared)
            .window(WindowConfig::new("main"))
            .window(WindowConfig::new("preview").only_abilities(["invoices.view"]))
            .window(WindowConfig::new("quiet").deny(Capability::Assistant));
        let ask = |label: &'static str| {
            let shell = &shell;
            async move {
                let request = wry::http::Request::builder()
                    .method("POST")
                    .uri("elyra://localhost/__assistant/prompts")
                    .header("x-elyra-token", shell.token())
                    .body(Vec::new())
                    .unwrap();
                shell.handle_in(label, request).await
            }
        };
        let names = |bytes: &[u8]| -> Vec<String> {
            let prompts: Vec<Value> = rmp_serde::from_slice(bytes).unwrap();
            prompts
                .iter()
                .map(|p| p["name"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            names(ask("main").await.body()),
            ["overdue_invoices", "cleanup"]
        );
        let preview = ask("preview").await;
        assert_eq!(names(preview.body()), ["overdue_invoices"]);
        // The panel gets the text and the arguments, never what it needs.
        let first: Vec<Value> = rmp_serde::from_slice(preview.body()).unwrap();
        assert_eq!(
            first[0]["text"],
            "Find the invoices that are overdue and draft a reminder for each."
        );
        assert!(first[0].get("needs").is_none());
        assert_eq!(ask("quiet").await.status(), 403);
    }

    #[test]
    #[should_panic(expected = "invalid assistant")]
    fn a_malformed_prompt_stops_the_app() {
        let _ = app_with(Assistant::new().prompt(Prompt::new("bad name", "B", "b"))).prepare();
    }
}
