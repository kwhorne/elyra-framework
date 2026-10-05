//! The MCP server (RFC 0003 steps 2 and 4): JSON-RPC over the stdio framing,
//! both protocol eras, tool calls through the real middleware pipeline, and
//! confirmation in the client.

use std::sync::{Arc, Mutex};

use elyra::command::BoxFuture;
use elyra::mcp::server::{LEGACY, MODERN};
use elyra::mcp::{Mcp, McpServer};
use elyra::testing::TestApp;
use elyra::{
    command, commands, App, CommandRequest, Ctx, Middleware, Next, Origin, ValidationErrors,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, specta::Type, Debug)]
struct NewCustomer {
    name: String,
    score: f64,
    note: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Debug, PartialEq)]
struct Customer {
    id: i64,
    name: String,
    score: f64,
}

/// Create a customer.
#[command(can = "customers.create")]
async fn customers_store(
    _ctx: Ctx,
    input: NewCustomer,
    tag: Option<String>,
) -> Result<Customer, ValidationErrors> {
    let _ = tag;
    if input.name.trim().is_empty() {
        let mut bag = ValidationErrors::new();
        bag.add("name", "The name field is required.");
        return Err(bag);
    }
    Ok(Customer {
        id: 7,
        name: input.name,
        score: input.score,
    })
}

#[command(can = "customers.view")]
async fn customers_fail(_ctx: Ctx) -> Result<i64, String> {
    Err("the ledger is closed".into())
}

#[command(can = "customers.view")]
async fn customers_panic(_ctx: Ctx) -> i64 {
    panic!("boom")
}

/// The ids `customers_destroy` ran for — each test uses its own.
static DELETED: Mutex<Vec<i64>> = Mutex::new(Vec::new());

fn deleted(id: i64) -> usize {
    DELETED.lock().unwrap().iter().filter(|d| **d == id).count()
}

/// Delete a customer.
///
/// It can't be undone.
#[command(can = "customers.delete")]
async fn customers_destroy(_ctx: Ctx, id: i64) {
    DELETED.lock().unwrap().push(id);
}

#[command(can = "admin.wipe")]
async fn wipe(_ctx: Ctx) {}

/// Records the origin of every call it sees.
struct Spy(Arc<Mutex<Vec<Origin>>>);

impl Middleware for Spy {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, elyra::Result<Vec<u8>>> {
        self.0.lock().unwrap().push(req.origin.clone());
        Box::pin(async move { next.run(ctx, req).await })
    }
}

fn app() -> (TestApp, Arc<Mutex<Vec<Origin>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = TestApp::new(
        App::new()
            .middleware(Spy(seen.clone()))
            .commands(commands![
                customers_store,
                customers_fail,
                customers_panic,
                customers_destroy,
                wipe
            ])
            .mcp(
                Mcp::new()
                    .allow_abilities(["customers.*"])
                    .confirm("customers.delete"),
            ),
    );
    (app, seen)
}

#[tokio::test]
async fn discover_and_list() {
    let (app, _) = app();
    let mcp = app.mcp();
    let discover = mcp.request("server/discover", json!({})).await;
    let result = &discover["result"];
    assert_eq!(result["resultType"], "complete");
    let versions: Vec<&str> = result["supportedVersions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(versions[0], MODERN);
    assert!(LEGACY.iter().all(|v| versions.contains(v)));
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"].is_string());

    let list = mcp.request("tools/list", json!({})).await;
    let result = &list["result"];
    assert!(result["ttlMs"].as_u64().unwrap() > 0);
    assert_eq!(result["cacheScope"], "private");
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "customers_destroy",
            "customers_fail",
            "customers_panic",
            "customers_store"
        ],
        "granted only, in name order — not `wipe`"
    );
    let store = &result["tools"][3];
    assert_eq!(store["description"], "Create a customer.");
    assert_eq!(store["inputSchema"]["required"], json!(["input"]));
    assert!(store.get("ability").is_none(), "Elyra's ability isn't sent");
}

#[tokio::test]
async fn a_call_runs_the_command_and_returns_its_value() {
    let (app, seen) = app();
    let mcp = app.mcp();
    // `score` is a whole number in JSON; the command takes an f64.
    let result = mcp
        .call(
            "customers_store",
            json!({ "input": { "name": "Ada", "score": 9, "note": null } }),
        )
        .await;
    assert!(!result.is_error, "{}", result.text);
    let expected = json!({ "id": 7, "name": "Ada", "score": 9.0 });
    assert_eq!(result.structured, Some(expected.clone()));
    assert_eq!(
        serde_json::from_str::<Value>(&result.text).unwrap(),
        expected
    );

    // An Option argument (and field) may be left out.
    let result = mcp
        .call(
            "customers_store",
            json!({ "input": { "name": "Bo", "score": 1.5 } }),
        )
        .await;
    assert!(!result.is_error, "{}", result.text);

    // Through the middleware, marked as the agent's.
    assert_eq!(
        seen.lock().unwrap()[0],
        Origin::Agent {
            client: "test-client".into()
        }
    );
    app.invoke_ok::<Customer>(
        "customers_store",
        (
            NewCustomer {
                name: "X".into(),
                score: 1.0,
                note: None,
            },
            None::<String>,
        ),
    )
    .await;
    assert_eq!(*seen.lock().unwrap().last().unwrap(), Origin::Frontend);
}

#[tokio::test]
async fn failures_come_back_as_tool_errors_the_model_can_act_on() {
    let (app, _) = app();
    let mcp = app.mcp();

    let invalid = mcp
        .call(
            "customers_store",
            json!({ "input": { "name": " ", "score": 1 } }),
        )
        .await;
    assert!(invalid.is_error);
    assert!(
        invalid.text.contains("- name: The name field is required."),
        "{}",
        invalid.text
    );
    assert_eq!(
        invalid.structured.unwrap()["errors"]["name"][0],
        "The name field is required."
    );

    let failed = mcp.call("customers_fail", json!({})).await;
    assert!(failed.is_error && failed.text.contains("the ledger is closed"));

    let panicked = mcp.call("customers_panic", json!({})).await;
    assert!(
        panicked.is_error && panicked.text.contains("panicked"),
        "{}",
        panicked.text
    );

    let unknown_arg = mcp
        .call("customers_store", json!({ "input": {}, "surprise": 1 }))
        .await;
    assert!(unknown_arg.is_error && unknown_arg.text.contains("no argument `surprise`"));

    let wrong_type = mcp
        .call(
            "customers_store",
            json!({ "input": { "name": 5, "score": "high" } }),
        )
        .await;
    assert!(
        wrong_type.is_error,
        "a decode failure is the model's to fix"
    );
}

#[tokio::test]
async fn unknown_and_ungranted_tools_are_protocol_errors() {
    let (app, _) = app();
    let mcp = app.mcp();
    for name in ["nope", "wipe"] {
        let response = mcp
            .request("tools/call", json!({ "name": name, "arguments": {} }))
            .await;
        assert_eq!(response["error"]["code"], -32602, "{response}");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }
}

#[tokio::test]
async fn a_client_that_cant_ask_never_runs_a_confirmed_tool() {
    let (app, _) = app();
    let result = app
        .mcp()
        .call("customers_destroy", json!({ "id": 101 }))
        .await;
    assert!(
        result.is_error && result.text.contains("can't ask"),
        "{}",
        result.text
    );
    assert_eq!(deleted(101), 0, "nothing was deleted");
}

#[tokio::test]
async fn the_user_confirms_in_the_client() {
    let (app, _) = app();
    let result = app
        .mcp()
        .confirming("accept")
        .call("customers_destroy", json!({ "id": 102 }))
        .await;
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(deleted(102), 1);
    let asked = result.confirmation.expect("the user was asked");
    assert!(
        asked.contains("test-client")
            && asked.contains("`customers_destroy`")
            && asked.contains("Delete a customer.")
            && !asked.contains("can't be undone")
            && asked.contains("102"),
        "who, what, the description's first line and the arguments: {asked}"
    );

    // A tool `confirm` doesn't name runs without a question.
    let store = app
        .mcp()
        .confirming("accept")
        .call(
            "customers_store",
            json!({ "input": { "name": "Ada", "score": 1 } }),
        )
        .await;
    assert!(!store.is_error && store.confirmation.is_none());
}

#[tokio::test]
async fn declined_or_dismissed_it_doesnt_run() {
    let (app, _) = app();
    for (id, action) in [(103, "decline"), (104, "cancel")] {
        let result = app
            .mcp()
            .confirming(action)
            .call("customers_destroy", json!({ "id": id }))
            .await;
        assert!(result.confirmation.is_some());
        assert!(
            result.is_error && result.text.contains("didn't confirm"),
            "{action}: {}",
            result.text
        );
        assert_eq!(deleted(id), 0, "{action}");
    }
}

/// The multi round-trip by hand: what a state is good for, and what not.
#[tokio::test]
async fn a_request_state_is_good_for_one_run_of_one_call() {
    let (app, _) = app();
    let mcp = app.mcp().confirming("accept");
    let call = |id: i64| json!({ "name": "customers_destroy", "arguments": { "id": id } });

    let first = mcp.request("tools/call", call(105)).await;
    let result = &first["result"];
    assert_eq!(result["resultType"], "input_required", "{first}");
    let ask = &result["inputRequests"]["confirm"];
    assert_eq!(ask["method"], "elicitation/create");
    assert_eq!(ask["params"]["mode"], "form");
    assert_eq!(ask["params"]["requestedSchema"]["type"], "object");
    let state = result["requestState"].as_str().unwrap().to_owned();
    assert!(
        !state.contains("customers_destroy"),
        "opaque, not plain JSON"
    );
    assert_eq!(deleted(105), 0, "asking runs nothing");

    let accept = json!({ "confirm": { "action": "accept" } });
    let retry = |id: i64, state: &str| {
        let mut params = call(id);
        params["inputResponses"] = accept.clone();
        params["requestState"] = json!(state);
        params
    };

    // The state was for id 105: a retry for 106 is asked again.
    let other = mcp.request("tools/call", retry(106, &state)).await;
    assert_eq!(other["result"]["resultType"], "input_required");
    assert_eq!(deleted(106), 0);

    // An accept with no state, or a made-up one, is asked again too.
    let mut bare = call(105);
    bare["inputResponses"] = accept.clone();
    let bare = mcp.request("tools/call", bare).await;
    assert_eq!(bare["result"]["resultType"], "input_required");
    let forged = mcp.request("tools/call", retry(105, "00.00")).await;
    assert_eq!(forged["result"]["resultType"], "input_required");
    assert_eq!(deleted(105), 0);

    let done = mcp.request("tools/call", retry(105, &state)).await;
    assert_eq!(done["result"]["resultType"], "complete", "{done}");
    assert_eq!(done["result"]["isError"], false);
    assert_eq!(deleted(105), 1);

    // Replayed: asked again, not run again.
    let replay = mcp.request("tools/call", retry(105, &state)).await;
    assert_eq!(replay["result"]["resultType"], "input_required");
    assert_eq!(deleted(105), 1);
}

#[tokio::test]
async fn versions_are_checked_per_request() {
    let (app, _) = app();
    let mcp = app.mcp();
    let response = mcp
        .raw(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list",
            "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "1900-01-01" } }
        }))
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], -32022);
    assert_eq!(response["error"]["data"]["requested"], "1900-01-01");
    assert_eq!(response["error"]["data"]["supported"][0], MODERN);

    // Neither `_meta` nor an `initialize` first: refused, saying how.
    let bare = mcp
        .raw(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }))
        .await
        .unwrap();
    assert_eq!(bare["error"]["code"], -32602);
    // But the era probe is always answered.
    let probe = mcp
        .raw(json!({ "jsonrpc": "2.0", "id": 3, "method": "server/discover" }))
        .await
        .unwrap();
    assert!(probe["result"]["supportedVersions"].is_array());
}

#[tokio::test]
async fn a_legacy_client_opens_with_initialize() {
    let (app, seen) = app();
    let mcp = app.mcp();
    let init = mcp
        .raw(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "old-desktop", "version": "1" }
            }
        }))
        .await
        .unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert!(init["result"]["serverInfo"]["name"].is_string());
    assert!(mcp
        .raw(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await
        .is_none());

    let list = mcp
        .raw(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }))
        .await
        .unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 4);
    assert!(
        list["result"].get("ttlMs").is_none(),
        "no modern fields for a legacy client"
    );
    // An output schema that isn't an object is left out for a legacy client.
    let fail = tools
        .iter()
        .find(|t| t["name"] == "customers_fail")
        .unwrap();
    assert!(fail.get("outputSchema").is_none());

    let call = mcp
        .raw(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "customers_store", "arguments": { "input": { "name": "Ada", "score": 1 } } }
        }))
        .await
        .unwrap();
    assert_eq!(call["result"]["isError"], false);
    assert_eq!(
        seen.lock().unwrap()[0],
        Origin::Agent {
            client: "old-desktop".into()
        }
    );
    // An unknown legacy version gets the newest legacy one.
    let other = app.mcp();
    let init = other
        .raw(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2024-01-01" } }))
        .await
        .unwrap();
    assert_eq!(init["result"]["protocolVersion"], LEGACY[0]);
}

#[tokio::test]
async fn malformed_messages() {
    let (app, _) = app();
    let mcp = app.mcp();
    let batch = mcp
        .raw(json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }]))
        .await
        .unwrap();
    assert_eq!(batch["error"]["code"], -32600);
    let unknown = mcp.request("resources/list", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);
}

#[tokio::test]
async fn serve_speaks_newline_delimited_json_over_a_stream() {
    let (app, _) = app();
    let server = McpServer::new(
        app.ctx().clone(),
        app.registry(),
        Mcp::new().allow_ability("customers.create"),
        "test",
        "1",
    )
    .unwrap();
    let (client, server_end) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_end);
    let serving = tokio::spawn(async move {
        server
            .serve(tokio::io::BufReader::new(server_read), server_write)
            .await
    });

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client_read, mut client_write) = tokio::io::split(client);
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": MODERN });
    let requests = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": { "_meta": meta } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "_meta": meta, "name": "customers_store",
            "arguments": { "input": { "name": "Ada", "score": 2 } } } }),
    ];
    for request in &requests {
        client_write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
    }
    client_write.write_all(b"this is not json\n").await.unwrap();
    client_write.shutdown().await.unwrap();

    let mut lines = tokio::io::BufReader::new(client_read).lines();
    let mut replies = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        assert!(!line.contains('\n'));
        replies.push(serde_json::from_str::<Value>(&line).unwrap());
    }
    assert_eq!(replies.len(), 3, "{replies:?}");
    let by_id = |id: i64| replies.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(by_id(1)["result"]["tools"][0]["name"], "customers_store");
    assert_eq!(by_id(2)["result"]["structuredContent"]["name"], "Ada");
    assert!(replies.iter().any(|r| r["error"]["code"] == -32700));
    serving.await.unwrap().unwrap();
}

/// A legacy client over a real stream: `initialize`, then lines both ways.
struct Legacy {
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
}

impl Legacy {
    async fn open(app: &TestApp, version: &str, capabilities: Value) -> Self {
        let server = McpServer::new(
            app.ctx().clone(),
            app.registry(),
            Mcp::new()
                .allow_ability("customers.delete")
                .confirm("customers.delete"),
            "test",
            "1",
        )
        .unwrap();
        let (client, server_end) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server_end);
        tokio::spawn(async move {
            server
                .serve(tokio::io::BufReader::new(server_read), server_write)
                .await
        });
        use tokio::io::AsyncBufReadExt;
        let (read, write) = tokio::io::split(client);
        let mut legacy = Self {
            lines: tokio::io::BufReader::new(read).lines(),
            write,
        };
        legacy
            .send(
                json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": version, "capabilities": capabilities,
                "clientInfo": { "name": "old-client", "version": "1" } } }),
            )
            .await;
        assert_eq!(legacy.next().await["result"]["protocolVersion"], version);
        legacy
    }

    async fn send(&mut self, message: Value) {
        use tokio::io::AsyncWriteExt;
        self.write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), self.lines.next_line())
            .await
            .expect("a message in time")
            .unwrap()
            .expect("the server is still there");
        serde_json::from_str(&line).unwrap()
    }

    async fn destroy(&mut self, id: i64) {
        self.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "customers_destroy", "arguments": { "id": id } } }))
            .await;
    }
}

#[tokio::test]
async fn a_legacy_client_is_asked_with_a_request_of_its_own() {
    let (app, _) = app();
    for (id, version, action) in [
        (201, "2025-11-25", "accept"),
        (202, "2025-06-18", "accept"),
        (203, "2025-11-25", "decline"),
    ] {
        let mut client = Legacy::open(&app, version, json!({ "elicitation": {} })).await;
        client.destroy(id).await;
        let ask = client.next().await;
        assert_eq!(ask["method"], "elicitation/create", "{ask}");
        assert!(ask["params"]["message"]
            .as_str()
            .unwrap()
            .contains("old-client"));
        // `mode` is a 2025-11-25 field.
        assert_eq!(
            ask["params"].get("mode").is_some(),
            version == "2025-11-25",
            "{ask}"
        );
        assert_eq!(deleted(id), 0, "nothing runs while the user is asked");

        client
            .send(json!({ "jsonrpc": "2.0", "id": ask["id"], "result": { "action": action } }))
            .await;
        let result = client.next().await;
        assert_eq!(result["id"], 2, "{result}");
        let ran = action == "accept";
        assert_eq!(result["result"]["isError"], !ran, "{result}");
        assert_eq!(deleted(id), usize::from(ran), "{version} {action}");
    }
}

#[tokio::test]
async fn a_legacy_client_that_cant_ask_gets_an_error() {
    let (app, _) = app();
    // No elicitation capability; and 2025-03-26 had none to declare.
    for (id, version, capabilities) in [
        (204, "2025-11-25", json!({})),
        (205, "2025-03-26", json!({ "elicitation": {} })),
    ] {
        let mut client = Legacy::open(&app, version, capabilities).await;
        client.destroy(id).await;
        let result = client.next().await;
        assert_eq!(result["id"], 2, "no question asked: {result}");
        assert!(result["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("can't ask"));
        assert_eq!(deleted(id), 0);
    }
}
