//! RFC 0071 against a real Laravel app (scripts/laravel-backend.sh): a list's
//! search, filters, sort and page sent through `Backend` as PHP reads them,
//! answered by the package's `elyraList`, with its totals over every match.
//! Skipped unless `ELYRA_LARAVEL_URL` says where the app is; CI starts one.
#![cfg(feature = "backend")]

use elyra::auth::Auth;
use elyra::backend::Backend;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Page};
use serde_json::{json, Value};

#[command]
async fn customers_list(ctx: Ctx, query: Value) -> elyra::Result<Page<Value>> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers")
        .query(&query)
        .json()
        .await?)
}

#[command]
async fn customers_store(ctx: Ctx, input: Value) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&input)
        .resource()
        .await?)
}

#[tokio::test]
async fn a_list_over_laravel() {
    let Ok(url) = std::env::var("ELYRA_LARAVEL_URL") else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL (see scripts/laravel-backend.sh)");
        return;
    };
    let app = TestApp::new(
        App::new()
            .backend(Backend::new(&url))
            .commands(commands![customers_list, customers_store]),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();

    // A unique run: the customers table outlives a test, so every query
    // below is narrowed to this run's customers by the search.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let run = format!("Lists{stamp}");
    let mut ids = Vec::new();
    for (name, credit, since) in [
        ("A", 100, "2026-11-05"),
        ("B", 860, "2026-11-30"),
        ("C", 3150, "2026-12-01"),
        ("100%_off", 22000, "2026-10-31"),
    ] {
        let made: Value = app
            .invoke_ok(
                "customers_store",
                (json!({
                    "name": format!("{run} {name}"),
                    "email": format!("{}{stamp}@example.com", name.to_lowercase().replace(['%', '_'], "")),
                    "credit": credit,
                    "since": since,
                }),),
            )
            .await;
        ids.push(made["id"].as_i64().unwrap());
    }
    let list = |query: Value| {
        let app = &app;
        async move {
            let page: Page<Value> = app.invoke_ok("customers_list", (query,)).await;
            page
        }
    };
    let names = |page: &Page<Value>| {
        let mut names: Vec<String> = page
            .data
            .iter()
            .map(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches(&format!("{run} "))
                    .to_string()
            })
            .collect();
        names.sort();
        names
    };
    let search = run.to_lowercase();

    // The search ignores case; the totals are over every match, not the page.
    let page = list(json!({ "search": search, "per_page": 2 })).await;
    assert_eq!((page.data.len(), page.total), (2, 4));
    assert_eq!(page.totals.as_ref().unwrap()["credit"], 26110.0);

    // A range, each end inclusive; the totals follow the filter.
    let page = list(json!({ "search": search, "filters": { "credit": { "min": 3150 } } })).await;
    assert_eq!(names(&page), ["100%_off", "C"]);
    assert_eq!(page.totals.as_ref().unwrap()["credit"], 25150.0);

    // Days: `to` is the whole of its day.
    let page = list(json!({ "search": search, "filters": { "since": { "from": "2026-11-01", "to": "2026-11-30" } } })).await;
    assert_eq!(names(&page), ["A", "B"]);

    // Options: one of them.
    let page = list(json!({ "search": search, "filters": { "id": [ids[0], ids[2]] } })).await;
    assert_eq!(names(&page), ["A", "C"]);

    // Text: contains, `%` and `_` as themselves, not wildcards.
    let page = list(json!({ "search": search, "filters": { "name": "0%_o" } })).await;
    assert_eq!(names(&page), ["100%_off"]);
    let page = list(json!({ "search": search, "filters": { "name": "%" } })).await;
    assert_eq!(names(&page), ["100%_off"]);

    // A key the controller doesn't allow, and a kind not the column's, are ignored.
    let page =
        list(json!({ "search": search, "filters": { "contract": "x", "credit": ["1", "2"] } }))
            .await;
    assert_eq!(page.total, 4);

    // The sort: an allowed column, else the key.
    let page = list(json!({ "search": search, "sort": "credit", "direction": "desc" })).await;
    assert!(page.data[0]["name"].as_str().unwrap().ends_with("100%_off"));
    let page = list(json!({ "search": search, "sort": "contract_path" })).await;
    let order: Vec<i64> = page
        .data
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(order, ids);

    // `per_page` is capped.
    let page = list(json!({ "search": search, "per_page": 1000 })).await;
    assert_eq!(page.per_page, 100);

    // Nothing matches: a sum of nothing is 0.
    let page = list(json!({ "search": search, "filters": { "credit": { "min": 1000000 } } })).await;
    assert_eq!((page.total, page.totals.unwrap()["credit"]), (0, 0.0));
}
