//! Active-Record model tests against real (temp-file) SQLite.
//! Only compiled with `--features database`.
#![cfg(feature = "database")]

use elyra::db::sqlx;
use elyra::{Database, Model};

// bool field (<-> INTEGER 0/1) + a column-name override (`name` -> `label`).
#[derive(Model, Debug, PartialEq)]
#[model(table = "widgets")]
struct Widget {
    id: i64,
    #[model(column = "label")]
    name: String,
    qty: i64,
    price: f64,
    active: bool,
}

// timestamps auto-managed (unix seconds).
#[derive(Model, Debug)]
#[model(table = "notes", timestamps)]
struct Note {
    id: i64,
    body: String,
    created_at: i64,
    updated_at: i64,
}

async fn db(tag: &str) -> (std::path::PathBuf, Database) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("elyra-model-{tag}-{nanos}.db"));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    (path, Database::connect(&url).await.unwrap())
}

#[tokio::test]
async fn crud_query_bool_and_column_override() {
    let (path, db) = db("crud").await;
    sqlx::raw_sql(
        "CREATE TABLE widgets (\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            label TEXT NOT NULL, \
            qty INTEGER NOT NULL, \
            price REAL NOT NULL, \
            active INTEGER NOT NULL)",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let mut bolt = Widget {
        id: 0,
        name: "bolt".into(),
        qty: 10,
        price: 1.5,
        active: true,
    };
    bolt.insert(&db).await.unwrap();
    assert!(bolt.id > 0);

    let mut nut = Widget {
        id: 0,
        name: "nut".into(),
        qty: 5,
        price: 0.25,
        active: false,
    };
    nut.insert(&db).await.unwrap();

    // The column override + bool roundtrip through find().
    let found = Widget::find(&db, bolt.id).await.unwrap().unwrap();
    assert_eq!(found.name, "bolt"); // read from the `label` column
    assert!(found.active);

    // Query builder filters on the bool (bound as 0/1) and on price.
    let active = Widget::query()
        .where_eq("active", true)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "bolt");

    let cheap = Widget::query()
        .where_lt("price", 1.0)
        .order_by("id")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(cheap.len(), 1);
    assert_eq!(cheap[0].name, "nut");
    assert!(!cheap[0].active);

    // update + save + delete
    bolt.active = false;
    bolt.update(&db).await.unwrap();
    assert!(!Widget::find(&db, bolt.id).await.unwrap().unwrap().active);

    nut.delete(&db).await.unwrap();
    assert_eq!(Widget::all(&db).await.unwrap().len(), 1);

    // Column identifiers are validated, not injected.
    assert!(Widget::query()
        .where_eq("label; DROP TABLE widgets", "x")
        .get(&db)
        .await
        .is_err());

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn timestamps_are_managed() {
    let (path, db) = db("ts").await;
    sqlx::raw_sql(
        "CREATE TABLE notes (\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            body TEXT NOT NULL, \
            created_at INTEGER NOT NULL, \
            updated_at INTEGER NOT NULL)",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let mut note = Note {
        id: 0,
        body: "hello".into(),
        created_at: 0,
        updated_at: 0,
    };
    note.insert(&db).await.unwrap();

    // insert stamped both.
    let stored = Note::find(&db, note.id).await.unwrap().unwrap();
    assert!(stored.created_at > 0);
    assert!(stored.updated_at > 0);
    let created = stored.created_at;

    // update refreshes updated_at (prove it by zeroing then updating).
    note.updated_at = 0;
    note.body = "world".into();
    note.update(&db).await.unwrap();
    let after = Note::find(&db, note.id).await.unwrap().unwrap();
    assert!(after.updated_at > 0);
    assert_eq!(after.created_at, created); // created_at untouched on update
    assert_eq!(after.body, "world");

    let _ = std::fs::remove_file(&path);
}
