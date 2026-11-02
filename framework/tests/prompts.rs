//! Prompts: tasks an app offers by name — listed to an MCP client
//! (`prompts/list`, `prompts/get`) and suggested in the assistant's panel,
//! each only where the abilities it needs are granted. An argument, or a
//! template's variable, completes from a live command the caller may run.

use std::sync::Mutex;
use std::time::Duration;

use elyra::mcp::{Completion, Mcp, McpServer, Prompt};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde::{Deserialize, Serialize};
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

#[derive(Deserialize, specta::Type, Debug, Default)]
struct CustomerQuery {
    search: Option<String>,
    per_page: Option<u32>,
}

#[derive(Serialize, specta::Type)]
struct Customer {
    id: i64,
    name: String,
}

#[derive(Serialize, specta::Type)]
struct CustomerPage {
    data: Vec<Customer>,
}

/// The searches the customers' index was asked, in order.
static SEARCHED: Mutex<Vec<(Option<String>, Option<u32>)>> = Mutex::new(Vec::new());

const NAMES: [&str; 4] = [
    "Ada Lovelace",
    "Grace Hopper",
    "Alan Turing",
    "Barbara Liskov",
];

/// Customers whose name has `search` in it.
#[command(live, can = "customers.view")]
async fn customers_index(_ctx: Ctx, query: CustomerQuery) -> CustomerPage {
    SEARCHED
        .lock()
        .unwrap()
        .push((query.search.clone(), query.per_page));
    let term = query.search.unwrap_or_default().to_lowercase();
    let data = (1..)
        .zip(NAMES)
        .filter(|(_, name)| name.to_lowercase().contains(&term))
        .map(|(id, name)| Customer {
            id,
            name: name.into(),
        })
        .collect();
    CustomerPage { data }
}

/// One customer.
#[command(live, can = "customers.view")]
async fn customers_show(_ctx: Ctx, id: i64) -> Customer {
    Customer {
        id,
        name: NAMES[0].into(),
    }
}

/// Add a customer.
#[command(can = "customers.create")]
async fn customers_store(_ctx: Ctx, name: String) {
    let _ = name;
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

/// `summary()`, its customer suggested from the customers' names.
fn completed_summary() -> Prompt {
    summary().complete("customer", Completion::new("customers_index", "name"))
}

fn app(mcp: Mcp) -> TestApp {
    TestApp::new(
        App::new()
            .commands(commands![
                invoices_index,
                invoices_destroy,
                customers_index,
                customers_show,
                customers_store
            ])
            .mcp(mcp),
    )
}

/// `completion/complete`'s answer for `reference`'s `argument` as `value`.
async fn completes(
    mcp: &elyra::testing::McpClient,
    reference: Value,
    argument: &str,
    value: &str,
) -> Value {
    mcp.request(
        "completion/complete",
        json!({ "ref": reference, "argument": { "name": argument, "value": value } }),
    )
    .await
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
async fn an_argument_completes_from_its_source_run_as_the_agent() {
    let app = app(Mcp::new()
        .allow_abilities(["invoices.*", "customers.view"])
        .prompt(completed_summary()));
    let mcp = app.mcp();
    let prompt = json!({ "type": "ref/prompt", "name": "customer_summary" });
    let done = completes(&mcp, prompt.clone(), "customer", " a ").await;
    assert_eq!(
        done["result"]["completion"],
        json!({ "values": ["Ada Lovelace", "Grace Hopper", "Alan Turing", "Barbara Liskov"], "total": 4, "hasMore": false })
    );
    let done = completes(&mcp, prompt.clone(), "customer", "gr").await;
    assert_eq!(
        done["result"]["completion"]["values"],
        json!(["Grace Hopper"])
    );
    // As make:resource's index takes it: the search, 20 to a page.
    let searched = SEARCHED.lock().unwrap().clone();
    assert!(
        searched.contains(&(Some("gr".into()), Some(20))),
        "{searched:?}"
    );
    assert!(
        searched.contains(&(Some("a".into()), Some(20))),
        "trimmed: {searched:?}"
    );
    // The other argument has none.
    let none = completes(&mcp, prompt, "period", "th").await;
    assert_eq!(none["result"]["completion"]["values"], json!([]));
    // The list doesn't say which do: a client asks.
    let list = app.mcp().request("prompts/list", json!({})).await;
    assert!(list["result"]["prompts"][0]["arguments"][0]
        .get("completes")
        .is_none());
}

#[tokio::test]
async fn a_template_variable_completes_from_its_source() {
    let template = json!({ "type": "ref/resource", "uri": "app://customers_show/{id}" });
    // Without a source, an integer has nothing to offer.
    let plain = app(Mcp::new().allow_ability("customers.view"));
    let none = completes(&plain.mcp(), template.clone(), "id", "").await;
    assert_eq!(none["result"]["completion"]["values"], json!([]));

    let app = app(Mcp::new().allow_ability("customers.view").complete(
        "customers_show",
        "id",
        Completion::new("customers_index", "id"),
    ));
    let mcp = app.mcp();
    let done = completes(&mcp, template.clone(), "id", "l").await;
    assert_eq!(
        done["result"]["completion"]["values"],
        json!(["1", "3", "4"])
    );
    let wrong = completes(&mcp, template, "nope", "").await;
    assert_eq!(wrong["error"]["code"], -32602);

    // A source is for its variable only: the others keep their schema's.
    let index = json!({ "type": "ref/resource", "uri": "app://customers_index{?per_page,search}" });
    let app = self::app(Mcp::new().allow_ability("customers.view").complete(
        "customers_index",
        "search",
        Completion::new("customers_index", "name"),
    ));
    let mcp = app.mcp();
    let names = completes(&mcp, index.clone(), "search", "tur").await;
    assert_eq!(
        names["result"]["completion"]["values"],
        json!(["Alan Turing"])
    );
    let paging = completes(&mcp, index, "per_page", "tur").await;
    assert_eq!(paging["result"]["completion"]["values"], json!([]));
}

#[tokio::test]
async fn a_completion_counts_against_the_sources_rate_limit() {
    let app = app(Mcp::new()
        .allow_abilities(["invoices.*", "customers.view"])
        .rate_limit("customers.view", 1, Duration::from_secs(60))
        .prompt(completed_summary()));
    let mcp = app.mcp();
    let prompt = json!({ "type": "ref/prompt", "name": "customer_summary" });
    let first = completes(&mcp, prompt.clone(), "customer", "ada").await;
    assert_eq!(
        first["result"]["completion"]["values"],
        json!(["Ada Lovelace"])
    );
    // Over the limit: no suggestions, and no error.
    let second = completes(&mcp, prompt, "customer", "ada").await;
    assert_eq!(second["result"]["completion"]["values"], json!([]));
}

#[test]
fn a_completion_from_what_the_grant_doesnt_offer_stops_the_server() {
    let prepared = App::new()
        .commands(commands![
            invoices_index,
            customers_index,
            customers_show,
            customers_store
        ])
        .prepare();
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
    let invoices = || Mcp::new().allow_ability("invoices.view");
    // Not granted.
    let ungranted = build(invoices().prompt(completed_summary())).unwrap();
    assert!(
        ungranted.contains("prompt `customer_summary`, argument `customer`: `customers_index` isn't a command this grant offers"),
        "{ungranted}"
    );
    // Not live: it may write.
    let writes = summary().complete("customer", Completion::new("customers_store", "name"));
    let why = build(invoices().allow_ability("customers.*").prompt(writes)).unwrap();
    assert!(
        why.contains("`customers_store` isn't a live command"),
        "{why}"
    );
    // Confirmed first.
    let why = build(
        invoices()
            .allow_ability("customers.view")
            .confirm("customers.view")
            .prompt(completed_summary()),
    )
    .unwrap();
    assert!(why.contains("needs the user's confirmation"), "{why}");
    // A prompt the grant doesn't offer isn't checked, its source neither.
    assert_eq!(
        build(invoices().prompt(completed_summary().needs("customers.delete"))),
        None
    );
    assert_eq!(
        build(
            Mcp::new()
                .allow_ability("customers.view")
                .prompt(completed_summary())
        ),
        None
    );
    // Templates: one this grant offers, with that variable.
    let source = || Completion::new("customers_index", "id");
    let customers = || Mcp::new().allow_ability("customers.view");
    assert!(
        build(customers().complete("invoices_index", "id", source()))
            .unwrap()
            .contains("`invoices_index` isn't a resource template this grant offers")
    );
    assert!(
        build(customers().complete("customers_show", "nope", source()))
            .unwrap()
            .contains("has no variable `nope`")
    );
    assert!(build(invoices().complete("customers_show", "id", source())).is_some());
    assert_eq!(
        build(customers().complete("customers_show", "id", source())),
        None
    );
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
            .allow_abilities(["invoices.*", "customers.view"])
            .commands(commands![invoices_index, invoices_destroy, customers_index])
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

    #[tokio::test]
    async fn the_panel_completes_an_argument_as_the_user_in_the_window() {
        let assistant = || {
            Assistant::new()
                .allow_abilities(["invoices.*", "customers.view"])
                .prompts([completed_summary(), overdue()])
        };
        let app = TestApp::new(app_with(assistant()));
        assert_eq!(
            app.assistant()
                .complete("customer_summary", "customer", "lis")
                .await,
            Ok(vec!["Barbara Liskov".to_owned()])
        );
        assert_eq!(
            app.assistant()
                .complete("customer_summary", "period", "x")
                .await,
            Ok(vec![])
        );
        assert!(app
            .assistant()
            .complete("nope", "customer", "")
            .await
            .unwrap_err()
            .contains("isn't suggested here"));
        assert!(app
            .assistant()
            .complete("overdue_invoices", "customer", "")
            .await
            .unwrap_err()
            .contains("has no argument `customer`"));

        // In a window that leaves the customers out, the suggestion is
        // offered, and its source refused: none.
        let shell = TestShell::new(app_with(assistant()).prepare())
            .window(WindowConfig::new("main"))
            .window(WindowConfig::new("invoices").only_abilities(["invoices.*"]))
            .window(WindowConfig::new("customers").only_abilities(["customers.view"]));
        let ask = |label: &'static str, body: Value| {
            let shell = &shell;
            async move {
                let request = wry::http::Request::builder()
                    .method("POST")
                    .uri("elyra://localhost/__assistant/complete")
                    .header("x-elyra-token", shell.token())
                    .body(rmp_serde::to_vec_named(&body).unwrap())
                    .unwrap();
                shell.handle_in(label, request).await
            }
        };
        let body = json!({ "prompt": "customer_summary", "argument": "customer", "value": "gr" });
        let main = ask("main", body.clone()).await;
        assert_eq!(
            rmp_serde::from_slice::<Vec<String>>(main.body()).unwrap(),
            ["Grace Hopper"]
        );
        let narrow = ask("invoices", body).await;
        assert!(rmp_serde::from_slice::<Vec<String>>(narrow.body())
            .unwrap()
            .is_empty());
        let refused = ask("main", json!({ "prompt": "nope", "argument": "customer" })).await;
        assert_eq!(
            refused.headers().get("x-elyra-error-kind").unwrap(),
            "denied"
        );
        // Not suggested in a window that leaves out what it needs: refused,
        // though its source is allowed there.
        let elsewhere = ask(
            "customers",
            json!({ "prompt": "customer_summary", "argument": "customer", "value": "gr" }),
        )
        .await;
        assert_eq!(
            elsewhere.headers().get("x-elyra-error-kind").unwrap(),
            "denied"
        );

        // The panel learns which arguments complete.
        let request = wry::http::Request::builder()
            .method("POST")
            .uri("elyra://localhost/__assistant/prompts")
            .header("x-elyra-token", shell.token())
            .body(Vec::new())
            .unwrap();
        let prompts: Vec<Value> =
            rmp_serde::from_slice(shell.handle_in("main", request).await.body()).unwrap();
        let arguments = &prompts[0]["arguments"];
        assert_eq!(arguments[0]["completes"], true);
        assert!(arguments[1].get("completes").is_none());
    }

    #[test]
    #[should_panic(expected = "isn't a live command")]
    fn a_completion_that_may_write_stops_the_app() {
        let writes = summary().complete("customer", Completion::new("invoices_destroy", "id"));
        let _ = app_with(Assistant::new().allow_ability("invoices.*").prompt(writes)).prepare();
    }

    #[test]
    #[should_panic(expected = "invalid assistant")]
    fn a_malformed_prompt_stops_the_app() {
        let _ = app_with(Assistant::new().prompt(Prompt::new("bad name", "B", "b"))).prepare();
    }
}
