//! Live commands as MCP resources (RFC 0003 step 5): `resources/list` and
//! `resources/read`, and `subscriptions/listen` driven by the live registry.
#![cfg(feature = "database")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::command::BoxFuture;
use elyra::db::sqlx;
use elyra::live::LiveRegistry;
use elyra::mcp::server::MODERN;
use elyra::mcp::{Mcp, McpServer};
use elyra::testing::TestApp;
use elyra::{
    command, commands, App, CommandRequest, Ctx, Database, Middleware, Model, Next, Origin,
    ValidationErrors,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Model, Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq)]
#[model(table = "teams")]
struct Team {
    id: i64,
    name: String,
}

#[derive(Model, Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[model(table = "notes")]
struct Note {
    id: i64,
    body: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
struct TeamQuery {
    name: Option<String>,
}

/// Every team, oldest first.
#[command(live, can = "teams.view")]
async fn teams_index(ctx: Ctx) -> elyra::Result<Vec<Team>> {
    Ok(Team::query()
        .order_by("id")
        .get(&ctx.get::<Database>())
        .await?)
}

/// A struct whose fields are all optional: a resource, read with `{}`.
#[command(live, can = "teams.view")]
async fn teams_search(ctx: Ctx, query: TeamQuery) -> elyra::Result<i64> {
    let mut q = Team::query();
    if let Some(name) = query.name {
        q = q.where_eq("name", name);
    }
    Ok(q.count(&ctx.get::<Database>()).await?)
}

/// An optional argument: a resource, read with `null`.
#[command(live, can = "teams.view")]
async fn teams_maybe(ctx: Ctx, page: Option<i64>) -> elyra::Result<i64> {
    let _ = page;
    Ok(Team::query().count(&ctx.get::<Database>()).await?)
}

/// Validates its argument, as a generated command does.
#[command(live, can = "teams.view")]
async fn teams_top(ctx: Ctx, limit: i64) -> elyra::Result<i64> {
    if limit < 1 {
        let mut bag = ValidationErrors::new();
        bag.add("limit", "The limit must be at least 1.");
        return Err(bag.into());
    }
    Ok(Team::query()
        .count(&ctx.get::<Database>())
        .await?
        .min(limit))
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
enum TeamKind {
    Product,
    Platform,
    Support,
}

/// An enum and a boolean: completions to offer.
#[command(live, can = "teams.view")]
async fn teams_by_kind(
    ctx: Ctx,
    kind: Option<TeamKind>,
    active: Option<bool>,
) -> elyra::Result<i64> {
    let _ = (kind, active);
    Ok(Team::query().count(&ctx.get::<Database>()).await?)
}

/// A required argument: a tool, but not a resource.
#[command(live, can = "teams.view")]
async fn teams_page(ctx: Ctx, page: i64) -> elyra::Result<i64> {
    let _ = page;
    Ok(Team::query().count(&ctx.get::<Database>()).await?)
}

/// Needs confirmation: a tool, but not a resource.
#[command(live, can = "teams.audit")]
async fn teams_audit(ctx: Ctx) -> elyra::Result<i64> {
    Ok(Team::query().count(&ctx.get::<Database>()).await?)
}

/// Not granted at all.
#[command(live, can = "teams.secret")]
async fn teams_secret(_ctx: Ctx) -> i64 {
    0
}

#[command(live, can = "notes.view")]
async fn notes_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(Note::query().count(&ctx.get::<Database>()).await?)
}

#[command(can = "teams.create")]
async fn teams_store(ctx: Ctx, name: String) -> elyra::Result<Team> {
    let mut team = Team { id: 0, name };
    team.insert(&ctx.get::<Database>()).await?;
    Ok(team)
}

#[command(can = "notes.create")]
async fn notes_store(ctx: Ctx) -> elyra::Result<()> {
    let mut note = Note {
        id: 0,
        body: "x".into(),
    };
    Ok(note.insert(&ctx.get::<Database>()).await?)
}

/// Records who ran `teams_index`.
struct Spy(Arc<Mutex<Vec<Origin>>>);

impl Middleware for Spy {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, elyra::Result<Vec<u8>>> {
        if req.name == "teams_index" {
            self.0.lock().unwrap().push(req.origin.clone());
        }
        Box::pin(async move { next.run(ctx, req).await })
    }
}

async fn app() -> (TestApp, Arc<Mutex<Vec<Origin>>>) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-mcpres-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE teams (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL);",
    )
    .execute(db.pool())
    .await
    .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = TestApp::new(
        App::new()
            .bind(db)
            .live_batch_window(Duration::from_millis(5))
            .middleware(Spy(seen.clone()))
            .commands(commands![
                teams_index,
                teams_search,
                teams_maybe,
                teams_page,
                teams_top,
                teams_by_kind,
                teams_audit,
                teams_secret,
                notes_count,
                teams_store,
                notes_store
            ])
            .mcp(
                Mcp::new()
                    .allow_abilities(["teams.view", "teams.create", "teams.audit", "notes.*"])
                    .confirm("teams.audit"),
            ),
    );
    (app, seen)
}

#[tokio::test]
async fn live_commands_that_need_no_arguments_are_resources() {
    let (app, _) = app().await;
    let mcp = app.mcp();
    let discover = mcp.request("server/discover", json!({})).await;
    assert_eq!(
        discover["result"]["capabilities"]["resources"]["subscribe"],
        true
    );

    let list = mcp.request("resources/list", json!({})).await;
    let result = &list["result"];
    let uris: Vec<&str> = result["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["uri"].as_str())
        .collect();
    assert_eq!(
        uris,
        [
            "app://notes_count",
            "app://teams_by_kind",
            "app://teams_index",
            "app://teams_maybe",
            "app://teams_search"
        ],
        "not the one with a required argument, the confirmed one, or the ungranted one"
    );
    let index = &result["resources"][2];
    assert_eq!(index["name"], "teams_index");
    assert_eq!(index["mimeType"], "application/json");
    assert_eq!(index["description"], "Every team, oldest first.");
    assert!(result["ttlMs"].as_u64().is_some());

    let templates = mcp.request("resources/templates/list", json!({})).await;
    let listed: Vec<(&str, &str)> = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["name"].as_str().unwrap(),
                t["uriTemplate"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("teams_by_kind", "app://teams_by_kind{?kind,active}"),
            ("teams_maybe", "app://teams_maybe{?page}"),
            ("teams_page", "app://teams_page/{page}"),
            ("teams_search", "app://teams_search{?name}"),
            ("teams_top", "app://teams_top/{limit}"),
        ],
        "live commands with arguments; not the confirmed one, nor those without arguments"
    );
    assert_eq!(
        templates["result"]["resourceTemplates"][1]["mimeType"],
        "application/json"
    );
    assert!(templates["result"]["ttlMs"].as_u64().is_some());
}

#[tokio::test]
async fn reading_runs_the_command_as_the_agent() {
    let (app, seen) = app().await;
    let mcp = app.mcp();
    assert_eq!(mcp.read("app://teams_index").await.unwrap(), json!([]));
    let stored = mcp.call("teams_store", json!({ "name": "core" })).await;
    assert!(!stored.is_error, "{}", stored.text);
    assert_eq!(
        mcp.read("app://teams_index").await.unwrap(),
        json!([{ "id": 1, "name": "core" }])
    );
    assert_eq!(mcp.read("app://teams_search").await.unwrap(), json!(1));
    assert_eq!(mcp.read("app://teams_maybe").await.unwrap(), json!(1));
    assert!(seen.lock().unwrap().iter().all(|o| matches!(
        o,
        Origin::Agent { client } if client == "test-client"
    )));

    let read = mcp
        .request("resources/read", json!({ "uri": "app://teams_index" }))
        .await;
    assert_eq!(read["result"]["contents"][0]["uri"], "app://teams_index");
    assert_eq!(read["result"]["ttlMs"], 0, "live data isn't cached");

    for missing in [
        "app://teams_page",
        "app://teams_audit",
        "app://nope",
        "file:///etc/passwd",
    ] {
        let error = mcp.read(missing).await.unwrap_err();
        assert_eq!(error["error"]["code"], -32602, "{missing}: {error}");
        assert_eq!(error["error"]["data"]["uri"], missing);
    }
}

#[tokio::test]
async fn a_subscription_hears_about_what_it_read() {
    let (app, seen) = app().await;
    let mcp = app.mcp();
    let (id, honored) = mcp.listen(&["app://teams_index", "app://nope"]).await;
    assert_eq!(honored, ["app://teams_index"], "only what can be watched");

    // A write to what it read: one notification, with the subscription's id.
    mcp.call("teams_store", json!({ "name": "core" })).await;
    let note = mcp.next_message().await.expect("an update");
    assert_eq!(note["method"], "notifications/resources/updated", "{note}");
    assert_eq!(note["params"]["uri"], "app://teams_index");
    assert_eq!(
        note["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        id
    );
    // The first run and the re-run, both as the agent.
    let runs = seen.lock().unwrap().clone();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(
        runs.iter().all(|o| matches!(o, Origin::Agent { .. })),
        "{runs:?}"
    );

    // A write to something it didn't read: nothing.
    mcp.call("notes_store", json!({})).await;
    assert!(mcp.next_message().await.is_none());

    // Cancelled: nothing more, and the live registry let go of it.
    let live = app.ctx().get::<LiveRegistry>();
    assert_eq!(live.len(), 1);
    mcp.raw(
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": { "requestId": id } }),
    )
    .await;
    assert_eq!(live.len(), 0);
    mcp.call("teams_store", json!({ "name": "more" })).await;
    assert!(mcp.next_message().await.is_none());
}

#[tokio::test]
async fn a_subscription_without_a_version_is_refused() {
    let (app, _) = app().await;
    let mcp = app.mcp();
    let refused = mcp
        .raw(
            json!({ "jsonrpc": "2.0", "id": 9, "method": "subscriptions/listen",
            "params": { "notifications": { "resourceSubscriptions": ["app://teams_index"] } } }),
        )
        .await
        .expect("an error, not silence");
    assert_eq!(refused["error"]["code"], -32602);
    assert_eq!(app.ctx().get::<LiveRegistry>().len(), 0);
}

type Lines = tokio::io::Lines<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>;

async fn next(lines: &mut Lines) -> Value {
    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .expect("a message in time")
        .unwrap()
        .expect("the server is still there");
    serde_json::from_str(&line).unwrap()
}

/// A legacy client subscribes one URI at a time, over a real stream; when it
/// goes, its subscriptions go with it.
#[tokio::test]
async fn a_legacy_client_subscribes_with_resources_subscribe() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (app, _) = app().await;
    let server = McpServer::new(
        app.ctx().clone(),
        app.registry(),
        Mcp::new().allow_abilities(["teams.view", "teams.create"]),
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
    let (read, mut write) = tokio::io::split(client);
    let mut lines = tokio::io::BufReader::new(read).lines();
    for message in [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": { "name": "old", "version": "1" } } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/subscribe",
            "params": { "uri": "app://teams_index" } }),
    ] {
        write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }
    assert_eq!(next(&mut lines).await["id"], 1);
    let subscribed = next(&mut lines).await;
    assert_eq!(subscribed["id"], 2, "{subscribed}");
    assert!(subscribed["result"].is_object(), "{subscribed}");

    let store = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "teams_store", "arguments": { "name": "core" } } });
    write
        .write_all(format!("{store}\n").as_bytes())
        .await
        .unwrap();
    let mut got = vec![next(&mut lines).await, next(&mut lines).await];
    got.sort_by_key(|m| m.get("id").is_some());
    assert_eq!(
        got[0]["method"], "notifications/resources/updated",
        "{got:?}"
    );
    assert_eq!(got[0]["params"]["uri"], "app://teams_index");
    assert!(
        got[0]["params"].get("_meta").is_none(),
        "no subscription id before 2026"
    );
    assert_eq!(got[1]["id"], 3);

    let unknown = json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/subscribe",
        "params": { "uri": "app://nope" } });
    write
        .write_all(format!("{unknown}\n").as_bytes())
        .await
        .unwrap();
    assert_eq!(next(&mut lines).await["error"]["code"], -32602);
    let invalid = json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/subscribe",
        "params": { "uri": "app://teams_page/oops" } });
    write
        .write_all(format!("{invalid}\n").as_bytes())
        .await
        .unwrap();
    let refused = next(&mut lines).await;
    assert_eq!(
        refused["error"]["message"], "Invalid arguments",
        "{refused}"
    );
    assert!(refused["error"]["data"]["errors"]["page"].is_array());

    let live = app.ctx().get::<LiveRegistry>();
    assert_eq!(live.len(), 1);
    write.shutdown().await.unwrap();
    // `serve` returns only once nothing still writes to the stream — so a
    // subscription left behind would hang it.
    tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("serve ends when the client goes")
        .unwrap()
        .unwrap();
    assert_eq!(
        live.len(),
        0,
        "the connection's subscriptions ended with it"
    );
}

#[tokio::test]
async fn the_modern_stream_over_serve() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (app, _) = app().await;
    let server = McpServer::new(
        app.ctx().clone(),
        app.registry(),
        Mcp::new().allow_abilities(["teams.view", "teams.create"]),
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
    let (read, mut write) = tokio::io::split(client);
    let mut lines = tokio::io::BufReader::new(read).lines();
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": MODERN });
    let listen = json!({ "jsonrpc": "2.0", "id": "sub-1", "method": "subscriptions/listen",
        "params": { "_meta": meta, "notifications": {
            "resourceSubscriptions": ["app://teams_maybe"], "toolsListChanged": true } } });
    write
        .write_all(format!("{listen}\n").as_bytes())
        .await
        .unwrap();
    let ack = next(&mut lines).await;
    assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
    assert_eq!(
        ack["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        "sub-1"
    );
    assert_eq!(
        ack["params"]["notifications"],
        json!({ "resourceSubscriptions": ["app://teams_maybe"] }),
        "tools never change, so that isn't honored"
    );
    let store = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "_meta": meta, "name": "teams_store", "arguments": { "name": "a" } } });
    write
        .write_all(format!("{store}\n").as_bytes())
        .await
        .unwrap();
    let mut methods = Vec::new();
    for _ in 0..2 {
        let message = next(&mut lines).await;
        methods.push(message["method"].as_str().unwrap_or("(reply)").to_owned());
    }
    methods.sort();
    assert_eq!(methods, ["(reply)", "notifications/resources/updated"]);
}

#[tokio::test]
async fn an_expanded_template_reads_with_its_arguments() {
    let (app, _) = app().await;
    let mcp = app.mcp();
    for name in ["core", "web", "core"] {
        mcp.call("teams_store", json!({ "name": name })).await;
    }
    assert_eq!(
        mcp.read("app://teams_search?name=core").await.unwrap(),
        json!(2)
    );
    assert_eq!(
        mcp.read("app://teams_search?name=nobody").await.unwrap(),
        json!(0)
    );
    assert_eq!(mcp.read("app://teams_page/1").await.unwrap(), json!(3));
    assert_eq!(mcp.read("app://teams_top/2").await.unwrap(), json!(2));

    // Values that don't fit, or a parameter it doesn't take: `-32602` saying
    // which and why, and the command never runs.
    for (uri, var) in [
        ("app://teams_page/one", "page"),
        ("app://teams_search?nmae=core", "nmae"),
        ("app://teams_page", "uri"),
    ] {
        let error = mcp.read(uri).await.unwrap_err();
        assert_eq!(error["error"]["code"], -32602, "{uri}: {error}");
        assert_eq!(error["error"]["message"], "Invalid arguments");
        assert!(
            error["error"]["data"]["errors"][var].is_array(),
            "{uri}: {error}"
        );
    }
    // The command's own validation: the same answer, with its messages.
    let error = mcp.read("app://teams_top/0").await.unwrap_err();
    assert_eq!(error["error"]["code"], -32602);
    assert_eq!(
        error["error"]["data"]["errors"]["limit"][0],
        "The limit must be at least 1."
    );
    // Something else entirely is still not found.
    let error = mcp.read("app://teams_nothing/1").await.unwrap_err();
    assert_eq!(error["error"]["message"], "Resource not found");
}

#[tokio::test]
async fn a_subscription_follows_what_its_arguments_select() {
    let (app, _) = app().await;
    let mcp = app.mcp();
    let (id, honored) = mcp
        .listen(&["app://teams_search?name=core", "app://teams_page/oops"])
        .await;
    assert_eq!(
        honored,
        ["app://teams_search?name=core"],
        "the invalid one isn't"
    );

    // A team the search doesn't select: re-run, unchanged, nothing said.
    mcp.call("teams_store", json!({ "name": "web" })).await;
    assert!(mcp.next_message().await.is_none());

    // One it does: told, about the URI it asked for.
    mcp.call("teams_store", json!({ "name": "core" })).await;
    let note = mcp.next_message().await.expect("an update");
    assert_eq!(note["params"]["uri"], "app://teams_search?name=core");
    assert_eq!(
        note["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        id
    );
    assert_eq!(
        mcp.read("app://teams_search?name=core").await.unwrap(),
        json!(1)
    );
}

#[tokio::test]
async fn completion_offers_an_enums_values_and_booleans() {
    let (app, _) = app().await;
    let mcp = app.mcp();
    let discover = mcp.request("server/discover", json!({})).await;
    assert!(discover["result"]["capabilities"]["completions"].is_object());

    let complete = |name: &str, value: &str| {
        mcp.request(
            "completion/complete",
            json!({
                "ref": { "type": "ref/resource", "uri": "app://teams_by_kind{?kind,active}" },
                "argument": { "name": name, "value": value },
            }),
        )
    };
    let kinds = complete("kind", "p").await;
    assert_eq!(
        kinds["result"]["completion"],
        json!({ "values": ["product", "platform"], "total": 2, "hasMore": false }),
        "{kinds}"
    );
    assert_eq!(kinds["result"]["resultType"], "complete");
    assert_eq!(
        complete("active", "").await["result"]["completion"]["values"],
        json!(["true", "false"])
    );
    assert_eq!(complete("nope", "").await["error"]["code"], -32602);

    let other = mcp
        .request(
            "completion/complete",
            json!({ "ref": { "type": "ref/prompt", "name": "x" }, "argument": { "name": "a", "value": "" } }),
        )
        .await;
    assert_eq!(other["error"]["code"], -32602);
    let unknown = mcp
        .request(
            "completion/complete",
            json!({ "ref": { "type": "ref/resource", "uri": "app://nope{?x}" }, "argument": { "name": "x", "value": "" } }),
        )
        .await;
    assert_eq!(unknown["error"]["code"], -32602);
}
