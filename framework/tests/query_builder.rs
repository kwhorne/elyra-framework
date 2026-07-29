//! Eloquent-flavoured query-builder coverage: filtering, ordering, pagination,
//! aggregates, joins, soft deletes, bulk update/delete, chunking, transactions —
//! against real (temp-file) SQLite.
#![cfg(feature = "database")]

use elyra::db::schema::Schema;
use elyra::db::sqlx;
use elyra::{Database, Model};

#[derive(Model, Debug, PartialEq)]
#[model(table = "products")]
struct Product {
    id: i64,
    name: String,
    category: String,
    price: f64,
    stock: i64,
}

#[derive(Model, Debug, PartialEq)]
#[model(table = "accounts", soft_deletes)]
struct Account {
    id: i64,
    email: String,
    deleted_at: Option<i64>,
}

#[derive(Model, Debug, PartialEq)]
#[model(table = "orders")]
struct Order {
    id: i64,
    product_id: i64,
    quantity: i64,
}

async fn db(tag: &str) -> (std::path::PathBuf, Database) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("elyra-qb-{tag}-{nanos}.db"));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    (path, Database::connect(&url).await.unwrap())
}

/// Create the schema with the *schema builder* (so it's covered end to end) and
/// insert a small fixture set.
async fn seeded(tag: &str) -> (std::path::PathBuf, Database) {
    let (path, db) = db(tag).await;

    Schema::create("products", |t| {
        t.id();
        t.string("name");
        t.string("category");
        t.float("price");
        t.integer("stock");
        t.index("category");
    })
    .execute(&db)
    .await
    .unwrap();

    Schema::create("accounts", |t| {
        t.id();
        t.string("email").unique();
        t.soft_deletes();
    })
    .execute(&db)
    .await
    .unwrap();

    Schema::create("orders", |t| {
        t.id();
        t.foreign_id("product_id", "products");
        t.integer("quantity");
    })
    .execute(&db)
    .await
    .unwrap();

    for (name, category, price, stock) in [
        ("Keyboard", "input", 89.0, 12),
        ("Mouse", "input", 39.0, 40),
        ("Monitor", "display", 329.0, 3),
        ("Lamp", "misc", 19.0, 0),
        ("Dock", "misc", 149.0, 7),
    ] {
        sqlx::query("INSERT INTO products (name, category, price, stock) VALUES (?, ?, ?, ?)")
            .bind(name)
            .bind(category)
            .bind(price)
            .bind(stock)
            .execute(db.pool())
            .await
            .unwrap();
    }
    for (product_id, quantity) in [(1i64, 2i64), (1, 1), (3, 5)] {
        sqlx::query("INSERT INTO orders (product_id, quantity) VALUES (?, ?)")
            .bind(product_id)
            .bind(quantity)
            .execute(db.pool())
            .await
            .unwrap();
    }
    (path, db)
}

fn cleanup(path: std::path::PathBuf) {
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn where_like_null_and_between() {
    let (path, db) = seeded("where").await;

    let matches = Product::query()
        .where_like("name", "M%")
        .order_by("name")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(
        matches.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["Monitor", "Mouse"]
    );

    let mid = Product::query()
        .where_between("price", 30, 150)
        .order_by("price")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(
        mid.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["Mouse", "Keyboard", "Dock"]
    );

    // NULL handling via the soft-delete column of another table.
    sqlx::query("INSERT INTO accounts (email, deleted_at) VALUES ('a@x.io', NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    assert_eq!(
        Account::query()
            .where_null("deleted_at")
            .count(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Account::query()
            .where_not_null("deleted_at")
            .with_trashed()
            .count(&db)
            .await
            .unwrap(),
        0
    );
    cleanup(path);
}

#[tokio::test]
async fn or_groups_combine_with_and() {
    let (path, db) = seeded("or").await;
    // (category = input OR category = misc) AND stock > 5
    let found = Product::query()
        .where_gt("stock", 5)
        .or_where_eq(&[("category", "input".into()), ("category", "misc".into())])
        .order_by("name")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(
        found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["Dock", "Keyboard", "Mouse"]
    );
    cleanup(path);
}

#[tokio::test]
async fn multiple_order_by_clauses_chain() {
    let (path, db) = seeded("order").await;
    let rows = Product::query()
        .order_by("category")
        .order_by_desc("price")
        .get(&db)
        .await
        .unwrap();
    let names: Vec<&str> = rows.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Monitor", "Keyboard", "Mouse", "Dock", "Lamp"],
        "category ASC, then price DESC within a category"
    );
    cleanup(path);
}

#[tokio::test]
async fn counts_and_aggregates() {
    let (path, db) = seeded("agg").await;

    assert_eq!(Product::query().count(&db).await.unwrap(), 5);
    assert_eq!(
        Product::query()
            .where_eq("category", "misc")
            .count(&db)
            .await
            .unwrap(),
        2
    );
    assert!(Product::query()
        .where_eq("category", "input")
        .exists(&db)
        .await
        .unwrap());
    assert!(!Product::query()
        .where_eq("category", "nope")
        .exists(&db)
        .await
        .unwrap());

    let sum = Product::query().sum(&db, "stock").await.unwrap().unwrap();
    assert_eq!(sum as i64, 62);
    let avg = Product::query()
        .where_eq("category", "input")
        .avg(&db, "stock")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(avg as i64, 26);
    assert_eq!(Product::query().min(&db, "stock").await.unwrap(), Some(0));
    assert_eq!(Product::query().max(&db, "stock").await.unwrap(), Some(40));

    // An aggregate must ignore limit/offset but honour the filters.
    assert_eq!(
        Product::query().limit(2).count(&db).await.unwrap(),
        5,
        "limit must not constrain COUNT"
    );
    cleanup(path);
}

#[tokio::test]
async fn pagination_reports_totals_and_pages() {
    let (path, db) = seeded("page").await;

    let first = Product::query()
        .order_by("id")
        .paginate(&db, 1, 2)
        .await
        .unwrap();
    assert_eq!(first.data.len(), 2);
    assert_eq!(first.total, 5);
    assert_eq!(first.per_page, 2);
    assert_eq!(first.current_page, 1);
    assert_eq!(first.last_page, 3);
    assert!(first.has_more());
    assert_eq!((first.from(), first.to()), (1, 2));

    let last = Product::query()
        .order_by("id")
        .paginate(&db, 3, 2)
        .await
        .unwrap();
    assert_eq!(last.data.len(), 1);
    assert!(!last.has_more());
    assert_eq!((last.from(), last.to()), (5, 5));

    let past_the_end = Product::query()
        .order_by("id")
        .paginate(&db, 9, 2)
        .await
        .unwrap();
    assert!(past_the_end.data.is_empty());
    assert_eq!((past_the_end.from(), past_the_end.to()), (0, 0));

    // An empty result set still yields a sane page.
    let none = Product::query()
        .where_eq("category", "nope")
        .paginate(&db, 1, 10)
        .await
        .unwrap();
    assert_eq!(none.total, 0);
    assert_eq!(none.last_page, 1);
    cleanup(path);
}

#[tokio::test]
async fn limit_and_offset_page_manually() {
    let (path, db) = seeded("offset").await;
    let page2 = Product::query()
        .order_by("id")
        .limit(2)
        .offset(2)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(page2.iter().map(|p| p.id).collect::<Vec<_>>(), vec![3, 4]);
    cleanup(path);
}

#[tokio::test]
async fn joins_filter_by_another_table() {
    let (path, db) = seeded("join").await;
    // Products that have at least one order of 2+ units.
    let rows = Product::query()
        .join("orders", "orders.product_id", "products.id")
        .where_gte("orders.quantity", 2)
        .order_by("products.id")
        .get(&db)
        .await
        .unwrap();
    let names: Vec<&str> = rows.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Keyboard", "Monitor"]);

    // A left join keeps rows without a match.
    let all = Order::query()
        .left_join("products", "products.id", "orders.product_id")
        .count(&db)
        .await
        .unwrap();
    assert_eq!(all, 3);
    cleanup(path);
}

#[tokio::test]
async fn a_join_cannot_inject_sql() {
    let (path, db) = seeded("join-safety").await;
    let result = Product::query()
        .join(
            "orders; DROP TABLE products",
            "orders.product_id",
            "products.id",
        )
        .get(&db)
        .await;
    assert!(result.is_err(), "a bad identifier must fail the query");
    // The table is still there.
    assert_eq!(Product::query().count(&db).await.unwrap(), 5);
    cleanup(path);
}

#[tokio::test]
async fn soft_deletes_hide_rows_until_asked_for() {
    let (path, db) = seeded("soft").await;
    for email in ["a@x.io", "b@x.io", "c@x.io"] {
        sqlx::query("INSERT INTO accounts (email, deleted_at) VALUES (?, NULL)")
            .bind(email)
            .execute(db.pool())
            .await
            .unwrap();
    }

    // Soft-delete one account.
    let affected = Account::query()
        .where_eq("email", "b@x.io")
        .soft_delete(&db)
        .await
        .unwrap();
    assert_eq!(affected, 1);

    // Default scope excludes it…
    assert_eq!(Account::query().count(&db).await.unwrap(), 2);
    // …with_trashed includes it…
    assert_eq!(Account::query().with_trashed().count(&db).await.unwrap(), 3);
    // …only_trashed isolates it.
    let trashed = Account::query().only_trashed().get(&db).await.unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(trashed[0].email, "b@x.io");

    // Restore brings it back into the default scope.
    assert_eq!(Account::query().restore(&db).await.unwrap(), 1);
    assert_eq!(Account::query().count(&db).await.unwrap(), 3);

    // A hard delete really removes it.
    assert_eq!(
        Account::query()
            .where_eq("email", "c@x.io")
            .delete(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(Account::query().with_trashed().count(&db).await.unwrap(), 2);
    cleanup(path);
}

#[tokio::test]
async fn soft_delete_on_a_plain_model_is_an_error() {
    let (path, db) = seeded("soft-missing").await;
    let err = Product::query().soft_delete(&db).await.unwrap_err();
    assert!(err.to_string().contains("soft_deletes"), "{err}");
    cleanup(path);
}

#[tokio::test]
async fn bulk_update_and_delete() {
    let (path, db) = seeded("bulk").await;

    let updated = Product::query()
        .where_eq("category", "input")
        .update(
            &db,
            &[("stock", 99.into()), ("category", "peripherals".into())],
        )
        .await
        .unwrap();
    assert_eq!(updated, 2);

    let rows = Product::query()
        .where_eq("category", "peripherals")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|p| p.stock == 99));

    let deleted = Product::query()
        .where_lt("price", 50)
        .delete(&db)
        .await
        .unwrap();
    assert_eq!(deleted, 2, "Mouse (39) and Lamp (19)");
    assert_eq!(Product::query().count(&db).await.unwrap(), 3);
    cleanup(path);
}

#[tokio::test]
async fn an_update_with_no_values_is_a_no_op() {
    let (path, db) = seeded("bulk-empty").await;
    assert_eq!(Product::query().update(&db, &[]).await.unwrap(), 0);
    cleanup(path);
}

#[tokio::test]
async fn chunking_walks_every_row() {
    let (path, db) = seeded("chunk").await;
    let mut seen = Vec::new();
    let mut batches = 0;
    let processed = Product::query()
        .order_by("id")
        .chunk(&db, 2, |batch| {
            batches += 1;
            seen.extend(batch.into_iter().map(|p| p.id));
            Ok(())
        })
        .await
        .unwrap();

    assert_eq!(processed, 5);
    assert_eq!(seen, vec![1, 2, 3, 4, 5]);
    assert_eq!(batches, 3, "2 + 2 + 1");
    cleanup(path);
}

#[tokio::test]
async fn transactions_commit_and_roll_back() {
    let (path, db) = seeded("tx").await;

    // Commit.
    db.transaction(|tx| {
        Box::pin(async move {
            sqlx::query("UPDATE products SET stock = 1000 WHERE id = 1")
                .execute(&mut **tx)
                .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(Product::find(&db, 1).await.unwrap().unwrap().stock, 1000);

    // Roll back: the error propagates and the write is undone.
    let result: elyra::db::Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                sqlx::query("UPDATE products SET stock = 7 WHERE id = 1")
                    .execute(&mut **tx)
                    .await?;
                Err(elyra::db::Error::Query("nope".into()))
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(
        Product::find(&db, 1).await.unwrap().unwrap().stock,
        1000,
        "the failed transaction must not have changed anything"
    );
    cleanup(path);
}
