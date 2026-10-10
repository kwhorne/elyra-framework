//! RFC 0005 against a real Laravel app (scripts/laravel-backend.sh): sign in,
//! the resource verbs, Laravel's own `422`s and paginator, not found, and
//! signing out — over real HTTP. Skipped unless `ELYRA_LARAVEL_URL` says where
//! the app is; CI starts one.
#![cfg(feature = "backend")]

use elyra::auth::Auth;
use elyra::backend::{Backend, BackendError};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Page};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, specta::Type, Debug, Clone, PartialEq)]
struct Customer {
    id: i64,
    name: String,
    email: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type, Default)]
struct CustomerInput {
    name: Option<String>,
    email: Option<String>,
}

#[command]
async fn customers_index(ctx: Ctx, search: String) -> elyra::Result<Page<Customer>> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers")
        .query(&json!({ "search": search, "per_page": 5 }))
        .json()
        .await?)
}

#[command]
async fn customers_show(ctx: Ctx, id: i64) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .resource()
        .await?)
}

#[command]
async fn customers_store(ctx: Ctx, input: CustomerInput) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&input)
        .resource()
        .await?)
}

#[command]
async fn customers_update(ctx: Ctx, id: i64, input: CustomerInput) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .body(&input)
        .resource()
        .await?)
}

#[command]
async fn customers_destroy(ctx: Ctx, id: i64) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .send()
        .await?;
    Ok(())
}

#[tokio::test]
async fn against_a_real_laravel_app() {
    let Ok(url) = std::env::var("ELYRA_LARAVEL_URL") else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL (see scripts/laravel-backend.sh)");
        return;
    };
    let app = TestApp::new(App::new().backend(Backend::new(&url)).commands(commands![
        customers_index,
        customers_show,
        customers_store,
        customers_update,
        customers_destroy
    ]));
    let auth = app.get::<Auth>();

    // Laravel's own 422 for wrong credentials, per field.
    match auth.sign_in("ada@example.com", "wrong").await {
        Err(BackendError::Validation(bag)) => {
            assert_eq!(
                bag.first("email"),
                Some("The provided credentials are incorrect.")
            )
        }
        other => panic!("{other:?}"),
    }
    // Before signing in, the API says 401.
    let err = app.invoke_err("customers_index", ("x".to_string(),)).await;
    assert!(err.contains("sign in again"), "{err}");

    let user = auth.sign_in("ada@example.com", "secret").await.unwrap();
    assert_eq!(user["name"], "Ada");

    // A unique run: the customers table outlives a test.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("Elyra {stamp}");
    let email = format!("elyra{stamp}@example.com");
    let created: Customer = app
        .invoke_ok(
            "customers_store",
            (CustomerInput {
                name: Some(name.clone()),
                email: Some(email.clone()),
            },),
        )
        .await;
    assert_eq!(created.name, name);
    assert!(
        created
            .created_at
            .as_deref()
            .is_some_and(|t| t.contains('T')),
        "Laravel's ISO dates"
    );

    // Laravel's validation, per field — a missing name, a taken email.
    let errors = app
        .invoke_validation_errors(
            "customers_store",
            (CustomerInput {
                name: None,
                email: Some(email.clone()),
            },),
        )
        .await
        .expect("a 422");
    assert!(errors["name"][0].contains("required"), "{errors:?}");
    assert!(errors["email"][0].contains("taken"), "{errors:?}");

    // Laravel's paginator, searched.
    let page: Page<Customer> = app.invoke_ok("customers_index", (name.clone(),)).await;
    assert_eq!((page.total, page.per_page, page.current_page), (1, 5, 1));
    assert_eq!(page.data[0].id, created.id);

    let renamed: Customer = app
        .invoke_ok(
            "customers_update",
            (
                created.id,
                // The whole record, as a form sends it: a `None` goes as
                // `null`, which `sometimes|required` refuses.
                CustomerInput {
                    name: Some(format!("{name} (renamed)")),
                    email: Some(email.clone()),
                },
            ),
        )
        .await;
    assert!(renamed.name.ends_with("(renamed)"));
    let shown: Customer = app.invoke_ok("customers_show", (created.id,)).await;
    assert_eq!(shown, renamed);

    app.invoke_ok::<()>("customers_destroy", (created.id,))
        .await;
    let gone = app.invoke_err("customers_show", (created.id,)).await;
    assert!(gone.contains("not found"), "{gone}");

    // Signing out revokes the token on the server: it no longer works.
    auth.sign_out().await.unwrap();
    assert!(!auth.is_signed_in());
    let err = app.invoke_err("customers_index", ("x".to_string(),)).await;
    assert!(err.contains("sign in again"), "{err}");
    let _: Option<Value> = None;
}
