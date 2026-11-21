//! Composite keys (RFC 0044): a model whose row is named by more than one
//! column — find, insert, update, delete, `save()` as an upsert (timestamps
//! included), a trashed row, a factory, and `belongs_to` from it. On SQLite
//! here; `model_servers.rs` runs the same on MySQL and Postgres.
#![cfg(feature = "database")]

use elyra::db::sqlx;
use elyra::{Database, Factory, Model};

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "orders")]
pub struct Order {
    id: i64,
    customer: String,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[model(
    table = "order_lines",
    timestamps,
    soft_deletes,
    belongs_to(Order, fk = "order_id")
)]
pub struct OrderLine {
    #[model(id)]
    order_id: i64,
    #[model(id)]
    line_no: i64,
    product: String,
    qty: i64,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
}

impl Factory for OrderLine {
    fn definition(n: u64) -> Self {
        OrderLine {
            order_id: 1,
            line_no: 100 + n as i64,
            product: format!("p{n}"),
            qty: 1,
            created_at: 0,
            updated_at: 0,
            deleted_at: None,
        }
    }
}

/// Every column is the key's: a tag on a post, say.
#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "post_tags")]
pub struct PostTag {
    #[model(id)]
    post_id: i64,
    #[model(id)]
    tag: String,
}

async fn db() -> (std::path::PathBuf, Database) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-keys-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE orders (id INTEGER PRIMARY KEY AUTOINCREMENT, customer TEXT NOT NULL);\
         CREATE TABLE order_lines (order_id INTEGER NOT NULL, line_no INTEGER NOT NULL, \
             product TEXT NOT NULL, qty INTEGER NOT NULL, created_at INTEGER NOT NULL, \
             updated_at INTEGER NOT NULL, deleted_at INTEGER, PRIMARY KEY (order_id, line_no));\
         CREATE TABLE post_tags (post_id INTEGER NOT NULL, tag TEXT NOT NULL, \
             PRIMARY KEY (post_id, tag));",
    )
    .execute(db.pool())
    .await
    .unwrap();
    (path, db)
}

fn line(order_id: i64, line_no: i64, product: &str, qty: i64) -> OrderLine {
    OrderLine {
        order_id,
        line_no,
        product: product.into(),
        qty,
        created_at: 0,
        updated_at: 0,
        deleted_at: None,
    }
}

#[tokio::test]
async fn a_row_is_found_written_and_deleted_by_its_whole_key() {
    let (path, db) = db().await;
    assert_eq!(<OrderLine as Model>::KEY, &["order_id", "line_no"]);
    assert_eq!(<OrderLine as Model>::PK, "order_id");
    assert_eq!(<Order as Model>::KEY, &["id"]);

    let mut order = Order {
        id: 0,
        customer: "Ada".into(),
    };
    order.insert(&db).await.unwrap();
    // Inserted as written: the key isn't read back.
    for (n, product) in [(1, "bolt"), (2, "nut")] {
        line(order.id, n, product, n).insert(&db).await.unwrap();
    }

    let found = OrderLine::find(&db, (order.id, 2)).await.unwrap().unwrap();
    assert_eq!(found.product, "nut");
    assert!(OrderLine::find(&db, (order.id, 3)).await.unwrap().is_none());
    // The builder's find and where_key take it too.
    let first = OrderLine::query()
        .find(&db, (order.id, 1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.product, "bolt");
    assert_eq!(
        OrderLine::query()
            .where_key((order.id, 1))
            .count(&db)
            .await
            .unwrap(),
        1
    );

    // update touches only the row its whole key names.
    let mut nut = found;
    nut.qty = 9;
    nut.update(&db).await.unwrap();
    let bolt = OrderLine::find(&db, (order.id, 1)).await.unwrap().unwrap();
    assert_eq!(bolt.qty, 1);
    assert_eq!(
        OrderLine::find(&db, (order.id, 2))
            .await
            .unwrap()
            .unwrap()
            .qty,
        9
    );

    // belongs_to from it.
    assert_eq!(bolt.order(&db).await.unwrap().unwrap().customer, "Ada");

    // delete, likewise; a trashed row is reached with with_trashed().
    nut.delete(&db).await.unwrap();
    assert!(OrderLine::find(&db, (order.id, 2)).await.unwrap().is_none());
    assert!(OrderLine::find(&db, (order.id, 1)).await.unwrap().is_some());
    OrderLine::query()
        .where_key((order.id, 1))
        .soft_delete(&db)
        .await
        .unwrap();
    assert!(OrderLine::find(&db, (order.id, 1)).await.unwrap().is_none());
    assert!(OrderLine::query()
        .with_trashed()
        .find(&db, (order.id, 1))
        .await
        .unwrap()
        .is_some());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn save_is_an_upsert_on_the_key_keeping_created_at() {
    let (path, db) = db().await;
    let mut a = line(7, 1, "bolt", 1);
    a.save(&db).await.unwrap();
    assert!(a.created_at > 0 && a.updated_at > 0);
    let created = a.created_at;

    // Saved again with another created_at in hand: the row's stays.
    let mut again = line(7, 1, "bolt", 5);
    again.created_at = 1;
    again.save(&db).await.unwrap();
    let row = OrderLine::find(&db, (7, 1)).await.unwrap().unwrap();
    assert_eq!(row.qty, 5);
    assert_eq!(row.created_at, created);
    assert_eq!(OrderLine::query().count(&db).await.unwrap(), 1);

    // Another key: a new row.
    line(7, 2, "nut", 1).save(&db).await.unwrap();
    assert_eq!(OrderLine::query().count(&db).await.unwrap(), 2);

    // Every column the key's: saving twice is one row, and update has nothing to set.
    let mut tag = PostTag {
        post_id: 1,
        tag: "rust".into(),
    };
    tag.save(&db).await.unwrap();
    tag.save(&db).await.unwrap();
    tag.update(&db).await.unwrap();
    assert_eq!(PostTag::query().count(&db).await.unwrap(), 1);
    assert!(PostTag::find(&db, (1, "rust".to_string()))
        .await
        .unwrap()
        .is_some());
    tag.delete(&db).await.unwrap();
    assert_eq!(PostTag::query().count(&db).await.unwrap(), 0);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_factory_inserts_them() {
    let (path, db) = db().await;
    let made = OrderLine::factory().count(3).create(&db).await.unwrap();
    assert_eq!(made.len(), 3);
    assert_eq!(OrderLine::query().count(&db).await.unwrap(), 3);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_single_key_still_takes_what_it_took() {
    let (path, db) = db().await;
    let mut order = Order {
        id: 0,
        customer: "Grete".into(),
    };
    order.insert(&db).await.unwrap();
    // An i32 for an i64 key, through Into<M::Key>.
    let id32 = order.id as i32;
    assert!(Order::query().find(&db, id32).await.unwrap().is_some());
    assert!(Order::find(&db, order.id).await.unwrap().is_some());
    let _ = std::fs::remove_file(path);
}
