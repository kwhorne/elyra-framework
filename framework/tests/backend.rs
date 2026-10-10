//! `Backend` (RFC 0005 step 2): the Laravel API through the app's `Http`, and
//! Laravel's answers as Elyra's errors. Answered by an `HttpFake`, no network.
#![cfg(feature = "backend")]

use elyra::backend::{Backend, BackendError};
use elyra::http::{Http, HttpFake, Method};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Page, ValidationErrors};
use serde::{Deserialize, Serialize};
use serde_json::json;

const BASE: &str = "https://crm.test";

#[derive(Serialize, Deserialize, specta::Type, Debug, PartialEq, Clone)]
struct Customer {
    id: i64,
    name: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
struct CustomerInput {
    name: Option<String>,
}

#[command]
async fn customers_index(ctx: Ctx, page: i64) -> elyra::Result<Page<Customer>> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers")
        .query(&json!({ "page": page }))
        .json()
        .await?)
}

#[command]
async fn customers_store(ctx: Ctx, input: CustomerInput) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&input)
        .json()
        .await?)
}

#[command]
async fn customers_destroy(ctx: Ctx, id: i64) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .json::<()>()
        .await?;
    Ok(())
}

fn app(fake: &HttpFake) -> TestApp {
    TestApp::new(
        App::new()
            .backend(Backend::new(format!("{BASE}/")))
            .commands(commands![
                customers_index,
                customers_store,
                customers_destroy
            ])
            .swap(Http::fake(fake.clone())),
    )
}

#[tokio::test]
async fn both_of_laravels_paginator_shapes_are_a_page() {
    let flat = HttpFake::new().get(
        format!("{BASE}/api/customers"),
        200,
        json!({ "current_page": 2, "data": [{ "id": 3, "name": "Ada" }], "from": 3,
                "last_page": 4, "per_page": 2, "to": 3, "total": 7,
                "first_page_url": "…", "path": "…", "links": [] }),
    );
    let page: Page<Customer> = app(&flat).invoke_ok("customers_index", (2,)).await;
    assert_eq!(
        (page.total, page.per_page, page.current_page, page.last_page),
        (7, 2, 2, 4)
    );
    assert_eq!(
        page.data,
        [Customer {
            id: 3,
            name: "Ada".into()
        }]
    );
    let sent = flat.assert_sent("GET", &format!("{BASE}/api/customers"));
    assert_eq!(
        sent.url,
        format!("{BASE}/api/customers?page=2"),
        "the base's slash isn't doubled"
    );

    let resource = HttpFake::new().get(
        format!("{BASE}/api/customers"),
        200,
        json!({ "data": [{ "id": 3, "name": "Ada" }],
                "links": { "first": "…", "next": null },
                "meta": { "current_page": 1, "last_page": 1, "per_page": 15, "total": 1, "path": "…" } }),
    );
    let page: Page<Customer> = app(&resource).invoke_ok("customers_index", (1,)).await;
    assert_eq!((page.total, page.per_page, page.last_page), (1, 15, 1));
}

#[tokio::test]
async fn a_422_is_the_same_bag_a_local_command_returns() {
    let fake = HttpFake::new().post(
        format!("{BASE}/api/customers"),
        422,
        json!({ "message": "The name field is required. (and 1 more error)",
                "errors": { "name": ["The name field is required."],
                            "items.0.qty": ["The items.0.qty must be at least 1."] } }),
    );
    let app = app(&fake);
    let errors = app
        .invoke_validation_errors("customers_store", (CustomerInput { name: None },))
        .await
        .expect("a validation bag, as from a local command");
    assert_eq!(errors["name"], ["The name field is required."]);
    assert_eq!(
        errors["items.0.qty"],
        ["The items.0.qty must be at least 1."]
    );
    let sent = fake.assert_sent("POST", &format!("{BASE}/api/customers"));
    assert_eq!(sent.body, Some(json!({ "name": null })));
    assert_eq!(
        sent.header("accept"),
        Some("application/json"),
        "else Laravel redirects"
    );
}

#[tokio::test]
async fn the_token_is_sent_and_a_401_forgets_it() {
    let fake = HttpFake::new().get(
        format!("{BASE}/api/customers"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    let app = app(&fake);
    let backend = app.get::<Backend>();
    backend.use_token(Some("1|abc".into()));
    let err = app.invoke_err("customers_index", (1,)).await;
    assert!(err.contains("sign in again"), "{err}");
    let sent = fake.assert_sent("GET", &format!("{BASE}/api/customers"));
    assert_eq!(sent.header("authorization"), Some("Bearer 1|abc"));
    assert!(!backend.has_token(), "a 401 means the token is gone");
}

#[tokio::test]
async fn a_204_decodes_as_nothing() {
    let fake = HttpFake::new().on(
        Method::Delete,
        format!("{BASE}/api/customers/9"),
        204,
        json!(null),
    );
    let app = app(&fake);
    app.invoke_ok::<()>("customers_destroy", (9,)).await;
}

/// The error for one Laravel answer, through a real `Backend` and fake.
async fn error_for(status: u16, headers: &[(&str, &str)], body: serde_json::Value) -> BackendError {
    let fake =
        HttpFake::new().on_with_headers(Method::Get, format!("{BASE}/x"), status, headers, body);
    Backend::new(BASE)
        .with_fake_for_tests(&fake)
        .get("/x")
        .send()
        .await
        .unwrap_err()
}

#[tokio::test]
async fn each_answer_has_its_kind() {
    let cases: Vec<(BackendError, Option<&str>, &str)> = vec![
        (
            error_for(401, &[], json!({})).await,
            Some("unauthenticated"),
            "sign in again",
        ),
        (
            error_for(419, &[], json!({})).await,
            Some("unauthenticated"),
            "sign in again",
        ),
        (
            error_for(403, &[], json!({ "message": "You don't own this server." })).await,
            Some("denied"),
            "You don't own this server.",
        ),
        (
            error_for(404, &[], json!({})).await,
            Some("not-found"),
            "not found",
        ),
        (
            error_for(429, &[("Retry-After", "30")], json!({})).await,
            Some("too-many-requests"),
            "try again in 30 s",
        ),
        (
            error_for(503, &[], json!({})).await,
            Some("server"),
            "the server failed (503)",
        ),
        (
            error_for(418, &[], json!({ "message": "teapot" })).await,
            None,
            "418: teapot",
        ),
    ];
    for (error, kind, text) in cases {
        let error: elyra::Error = error.into();
        assert_eq!(error.kind(), kind, "{error}");
        assert!(error.to_string().contains(text), "{error} / {text}");
    }

    let offline = HttpFake::new().unreachable(Method::Get, format!("{BASE}/x"));
    let error: elyra::Error = Backend::new(BASE)
        .with_fake_for_tests(&offline)
        .get("/x")
        .send()
        .await
        .unwrap_err()
        .into();
    assert_eq!(error.kind(), Some("offline"));

    // A 422 with no field errors still says what Laravel said.
    match error_for(422, &[], json!({ "message": "Nope." })).await {
        BackendError::Other {
            status: 422,
            message,
        } => assert_eq!(message, "Nope."),
        other => panic!("{other:?}"),
    }
    match error_for(422, &[], json!({ "message": "Nope.", "errors": {} })).await {
        BackendError::Validation(bag) => assert_eq!(bag.first("_"), Some("Nope.")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_path_is_relative_to_the_backend() {
    let err = Backend::new(BASE)
        .with_fake_for_tests(&HttpFake::new())
        .get("https://elsewhere.test/steal")
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, BackendError::Http(_)), "{err:?}");
    let _ = ValidationErrors::new();
}

/// Through a command, as the shell dispatches it: the error keeps its kind,
/// so the frontend reads `offline` — not just the message.
#[tokio::test]
async fn a_commands_backend_error_keeps_its_kind() {
    let fake = HttpFake::new()
        .unreachable(Method::Get, format!("{BASE}/api/customers"))
        .post(format!("{BASE}/api/customers"), 401, json!({}));
    let app = app(&fake);
    let body = rmp_serde::to_vec(&(1,)).unwrap();
    let offline = app
        .registry()
        .dispatch(app.ctx().clone(), "customers_index", &body)
        .await
        .unwrap_err();
    assert_eq!(offline.kind(), Some("offline"), "{offline}");
    let body = rmp_serde::to_vec(&(CustomerInput { name: None },)).unwrap();
    let signed_out = app
        .registry()
        .dispatch(app.ctx().clone(), "customers_store", &body)
        .await
        .unwrap_err();
    assert_eq!(signed_out.kind(), Some("unauthenticated"));
}
