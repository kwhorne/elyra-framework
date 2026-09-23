//! `unique` / `exists` validation against a real SQLite database.
#![cfg(feature = "database")]

use elyra::db::sqlx;
use elyra::validation::Validator;
use elyra::Database;
use serde_json::json;

async fn db() -> (std::path::PathBuf, Database) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "elyra-validation-db-{}-{}.db",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL, handle TEXT);\
         CREATE TABLE roles (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
         INSERT INTO users (id, email, handle) VALUES (1, 'ada@example.test', 'ada'), (2, 'grace@example.test', 'grace');\
         INSERT INTO roles (id, name) VALUES (10, 'admin'), (11, 'editor');",
    )
    .execute(db.pool())
    .await
    .unwrap();
    (path, db)
}

#[tokio::test]
async fn unique_rejects_a_taken_value_and_accepts_a_free_one() {
    let (path, db) = db().await;
    let taken = json!({ "email": "ada@example.test" });
    let e = Validator::new(&taken)
        .rule("email", "required|email|unique:users")
        .errors_with(&db)
        .await;
    assert_eq!(e.first("email"), Some("The email has already been taken."));

    let free = json!({ "email": "new@example.test" });
    assert!(Validator::new(&free)
        .rule("email", "unique:users")
        .validate_with(&db)
        .await
        .is_ok());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unique_can_ignore_the_row_being_updated() {
    let (path, db) = db().await;
    // Ada keeps her own email while editing her profile…
    let own = json!({ "email": "ada@example.test" });
    assert!(Validator::new(&own)
        .rule("email", "unique:users,email,1")
        .validate_with(&db)
        .await
        .is_ok());
    // …but can't take Grace's.
    let other = json!({ "email": "grace@example.test" });
    assert!(Validator::new(&other)
        .rule("email", "unique:users,email,1")
        .errors_with(&db)
        .await
        .has("email"));
    // A custom key column.
    assert!(Validator::new(&own)
        .rule("email", "unique:users,email,ada,handle")
        .validate_with(&db)
        .await
        .is_ok());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn exists_accepts_known_ids_and_names_the_bad_ones() {
    let (path, db) = db().await;
    let input = json!({ "role_id": 10, "items": [{ "role_id": 11 }, { "role_id": 99 }] });
    let e = Validator::new(&input)
        .rules(&[
            ("role_id", "exists:roles,id"),
            ("items.*.role_id", "exists:roles,id"),
        ])
        .errors_with(&db)
        .await;
    assert!(!e.has("role_id"));
    assert!(!e.has("items.0.role_id"));
    assert_eq!(
        e.first("items.1.role_id"),
        Some("The selected items.1.role id is invalid.")
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn the_column_defaults_to_the_field_name_and_other_rules_still_run() {
    let (path, db) = db().await;
    let input = json!({ "handle": "ada", "email": "not-an-email" });
    let e = Validator::new(&input)
        .rules(&[("handle", "unique:users"), ("email", "email|unique:users")])
        .errors_with(&db)
        .await;
    assert_eq!(
        e.first("handle"),
        Some("The handle has already been taken.")
    );
    assert_eq!(
        e.first("email"),
        Some("The email must be a valid email address.")
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn an_absent_optional_field_skips_the_query() {
    let (path, db) = db().await;
    assert!(Validator::new(&json!({}))
        .rule("role_id", "nullable|exists:roles,id")
        .validate_with(&db)
        .await
        .is_ok());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
#[should_panic(expected = "invalid identifier `users; DROP TABLE users` in `unique` on `email`")]
async fn a_malformed_table_name_is_refused_before_any_sql_runs() {
    let (_path, db) = db().await;
    let _ = Validator::new(&json!({ "email": "x@y.z" }))
        .rule("email", "unique:users; DROP TABLE users")
        .errors_with(&db)
        .await;
}
