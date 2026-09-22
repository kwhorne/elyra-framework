//! The Eloquent-depth features on SQLite: casts, local + global scopes,
//! `belongs_to_many`, factories — and `find` / `all` respecting soft deletes.
#![cfg(feature = "database")]

use std::fmt;
use std::str::FromStr;

use elyra::db::cast::{Cast, Json};
use elyra::db::model::SyncChanges;
use elyra::db::sqlx::{self, any::AnyRow, Row};
use elyra::{Database, Factory, Model, Query, Value};

// --- casts --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Address {
    city: String,
    zip: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status {
    Draft,
    Published,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Status::Draft => "draft",
            Status::Published => "published",
        })
    }
}

impl FromStr for Status {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "draft" => Ok(Status::Draft),
            "published" => Ok(Status::Published),
            other => Err(format!("not a status: {other}")),
        }
    }
}

/// Money in the app, integer cents in the column — a hand-written cast.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Money(f64);

struct Cents;

impl Cast<Money> for Cents {
    fn encode(value: &Money) -> elyra::db::Result<Value> {
        Ok(Value::Int((value.0 * 100.0).round() as i64))
    }
    fn decode(row: &AnyRow, column: &str) -> elyra::db::Result<Money> {
        Ok(Money(row.try_get::<i64, _>(column)? as f64 / 100.0))
    }
}

#[derive(Model, Debug)]
#[model(table = "articles")]
struct Article {
    id: i64,
    title: String,
    #[model(cast = "json")]
    tags: Vec<String>,
    #[model(cast = "json")]
    address: Option<Address>,
    #[model(cast = "text")]
    status: Status,
    #[model(cast = Cents)]
    price: Money,
}

// --- scopes -------------------------------------------------------------------

fn in_workspace_one(q: Query<Doc>) -> Query<Doc> {
    q.where_eq("workspace_id", 1)
}

#[derive(Model, Debug)]
#[model(table = "docs", global_scope = in_workspace_one)]
struct Doc {
    id: i64,
    workspace_id: i64,
    title: String,
    archived: bool,
}

#[derive(Model, Debug)]
#[model(table = "workspaces", has_many(Doc, fk = "workspace_id", as = "docs"))]
struct Workspace {
    id: i64,
    name: String,
}

fn unarchived(q: Query<Doc>) -> Query<Doc> {
    q.where_eq("archived", false)
}

#[derive(Model, Debug)]
#[model(table = "notes", soft_deletes)]
struct Note {
    id: i64,
    body: String,
    deleted_at: Option<i64>,
}

// --- belongs_to_many ----------------------------------------------------------

#[derive(Model, Debug)]
#[model(table = "users", belongs_to_many(Role))]
struct User {
    id: i64,
    name: String,
}

/// Deliberately **not** `Clone`: eager loading a role shared by two users must
/// not need to copy it.
#[derive(Model, Debug, PartialEq)]
#[model(table = "roles")]
struct Role {
    id: i64,
    name: String,
}

#[derive(Model, Debug)]
#[model(table = "users")]
struct UserWithRoles {
    id: i64,
    name: String,
    #[model(belongs_to_many(Role, pivot = "role_user", fk = "user_id"))]
    roles: Vec<Role>,
}

impl Factory for User {
    fn definition(n: u64) -> Self {
        User {
            id: 0,
            name: format!("User {n}"),
        }
    }
}

impl Factory for Role {
    fn definition(n: u64) -> Self {
        Role {
            id: 0,
            name: format!("role-{n}"),
        }
    }
}

impl Factory for Doc {
    fn definition(n: u64) -> Self {
        Doc {
            id: 0,
            workspace_id: 1,
            title: format!("Doc {n}"),
            archived: false,
        }
    }
}

// --- harness ------------------------------------------------------------------

async fn setup(tag: &str) -> (std::path::PathBuf, Database) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "elyra-eloquent-{tag}-{}-{n}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE articles (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, \
             tags TEXT NOT NULL, address TEXT, status TEXT NOT NULL, price INTEGER NOT NULL);\
         CREATE TABLE workspaces (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE docs (id INTEGER PRIMARY KEY AUTOINCREMENT, workspace_id INTEGER NOT NULL, \
             title TEXT NOT NULL, archived INTEGER NOT NULL);\
         CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL, deleted_at INTEGER);\
         CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE roles (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);\
         CREATE TABLE role_user (role_id INTEGER NOT NULL, user_id INTEGER NOT NULL, \
             PRIMARY KEY (role_id, user_id));",
    )
    .execute(db.pool())
    .await
    .unwrap();
    (path, db)
}

fn cleanup(path: std::path::PathBuf) {
    let _ = std::fs::remove_file(path);
}

// --- casts --------------------------------------------------------------------

#[tokio::test]
async fn cast_fields_round_trip_through_their_columns() {
    let (path, db) = setup("casts").await;
    let mut article = Article {
        id: 0,
        title: "Hello".into(),
        tags: vec!["rust".into(), "desktop".into()],
        address: Some(Address {
            city: "Oslo".into(),
            zip: "0150".into(),
        }),
        status: Status::Published,
        price: Money(19.99),
    };
    article.insert(&db).await.unwrap();

    let loaded = Article::find(&db, article.id).await.unwrap().unwrap();
    assert_eq!(loaded.tags, vec!["rust", "desktop"]);
    assert_eq!(loaded.address, article.address);
    assert_eq!(loaded.status, Status::Published);
    assert_eq!(loaded.price, Money(19.99));

    // The columns hold what the casts say they hold.
    let row = sqlx::query("SELECT tags, status, price FROM articles")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("tags"), r#"["rust","desktop"]"#);
    assert_eq!(row.get::<String, _>("status"), "published");
    assert_eq!(row.get::<i64, _>("price"), 1999);
    cleanup(path);
}

#[tokio::test]
async fn a_json_none_is_stored_as_null_and_updates_go_through_the_cast() {
    let (path, db) = setup("casts-null").await;
    let mut article = Article {
        id: 0,
        title: "Draft".into(),
        tags: Vec::new(),
        address: None,
        status: Status::Draft,
        price: Money(0.0),
    };
    article.insert(&db).await.unwrap();
    let null: Option<String> = sqlx::query("SELECT address FROM articles")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get("address");
    assert_eq!(null, None, "None must be a real NULL, not the text `null`");

    article.status = Status::Published;
    article.tags.push("late".into());
    article.update(&db).await.unwrap();
    let loaded = Article::find(&db, article.id).await.unwrap().unwrap();
    assert_eq!(loaded.status, Status::Published);
    assert_eq!(loaded.tags, vec!["late"]);
    assert_eq!(loaded.address, None);
    cleanup(path);
}

#[tokio::test]
async fn a_bulk_update_can_write_a_cast_column_through_the_cast() {
    let (path, db) = setup("casts-bulk").await;
    let mut article = Article {
        id: 0,
        title: "Bulk".into(),
        tags: vec!["old".into()],
        address: None,
        status: Status::Draft,
        price: Money(1.0),
    };
    article.insert(&db).await.unwrap();

    let tags = vec!["new".to_string(), "tags".to_string()];
    Article::query()
        .where_eq("id", article.id)
        .update(&db, &[("tags", Json::encode(&tags).unwrap())])
        .await
        .unwrap();
    let loaded = Article::find(&db, article.id).await.unwrap().unwrap();
    assert_eq!(loaded.tags, tags);
    cleanup(path);
}

#[tokio::test]
async fn an_unparseable_text_cast_is_an_error_naming_the_column() {
    let (path, db) = setup("casts-bad").await;
    sqlx::query("INSERT INTO articles (title, tags, status, price) VALUES ('x', '[]', 'bogus', 0)")
        .execute(db.pool())
        .await
        .unwrap();
    let err = Article::all(&db).await.unwrap_err().to_string();
    assert!(err.contains("status"), "{err}");
    assert!(err.contains("bogus"), "{err}");
    cleanup(path);
}

// --- local scopes -------------------------------------------------------------

#[tokio::test]
async fn local_scopes_and_conditional_clauses_compose() {
    let (path, db) = setup("scopes").await;
    Doc::factory()
        .count(4)
        .sequence(|d, i| d.archived = i % 2 == 1)
        .create(&db)
        .await
        .unwrap();

    assert_eq!(Doc::query().scope(unarchived).count(&db).await.unwrap(), 2);

    // `when` applies only when asked…
    let only_live = true;
    assert_eq!(
        Doc::query()
            .when(only_live, unarchived)
            .count(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Doc::query()
            .when(false, unarchived)
            .count(&db)
            .await
            .unwrap(),
        4
    );

    // …and `when_some` threads an optional filter value through.
    let search: Option<&str> = None;
    assert_eq!(
        Doc::query()
            .when_some(search, |q, s| q.where_like("title", format!("%{s}%")))
            .count(&db)
            .await
            .unwrap(),
        4
    );
    let docs = Doc::query().get(&db).await.unwrap();
    let wanted = docs[0].title.clone();
    assert_eq!(
        Doc::query()
            .when_some(Some(wanted.as_str()), |q, s| q.where_eq("title", s))
            .count(&db)
            .await
            .unwrap(),
        1
    );
    cleanup(path);
}

// --- global scopes ------------------------------------------------------------

#[tokio::test]
async fn a_global_scope_constrains_every_query_path() {
    let (path, db) = setup("global").await;
    let ours = Doc::factory().count(2).create(&db).await.unwrap();
    let theirs = Doc::factory()
        .state(|d| d.workspace_id = 2)
        .create_one(&db)
        .await
        .unwrap();

    // query / count / all / find all carry it.
    assert_eq!(Doc::query().count(&db).await.unwrap(), 2);
    assert_eq!(Doc::all(&db).await.unwrap().len(), 2);
    assert!(Doc::find(&db, ours[0].id).await.unwrap().is_some());
    assert!(
        Doc::find(&db, theirs.id).await.unwrap().is_none(),
        "a row outside the scope must not be reachable by id"
    );

    // So do relation queries into the scoped model.
    let mut first = Workspace {
        id: 0,
        name: "first".into(),
    };
    first.insert(&db).await.unwrap();
    assert_eq!(first.id, 1);
    let mut second = Workspace {
        id: 0,
        name: "second".into(),
    };
    second.insert(&db).await.unwrap();
    // `theirs` has workspace_id = 2, so without the scope this would find it.
    assert!(second.docs(&db).await.unwrap().is_empty());
    assert_eq!(first.docs(&db).await.unwrap().len(), 2);

    // Opting out is explicit.
    assert_eq!(
        Doc::query()
            .without_global_scopes()
            .count(&db)
            .await
            .unwrap(),
        3
    );
    assert!(Doc::query()
        .without_global_scopes()
        .find(&db, theirs.id)
        .await
        .unwrap()
        .is_some());
    cleanup(path);
}

#[tokio::test]
async fn bulk_writes_respect_the_global_scope() {
    // An UPDATE through the builder must not escape the scope either — this is
    // where the old re-binding path would have mis-ordered the placeholders.
    let (path, db) = setup("global-write").await;
    Doc::factory().count(2).create(&db).await.unwrap();
    Doc::factory()
        .state(|d| d.workspace_id = 2)
        .create_one(&db)
        .await
        .unwrap();

    let touched = Doc::query()
        .where_eq("archived", false)
        .update(&db, &[("title", "renamed".into())])
        .await
        .unwrap();
    assert_eq!(touched, 2, "only this workspace's rows");
    let untouched: String = sqlx::query("SELECT title FROM docs WHERE workspace_id = 2")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get("title");
    assert_ne!(untouched, "renamed");

    assert_eq!(Doc::query().delete(&db).await.unwrap(), 2);
    assert_eq!(
        Doc::query()
            .without_global_scopes()
            .count(&db)
            .await
            .unwrap(),
        1
    );
    cleanup(path);
}

// --- soft deletes through find / all -------------------------------------------

#[tokio::test]
async fn find_and_all_skip_soft_deleted_rows() {
    // Regression: both issued raw SQL and returned trashed rows.
    let (path, db) = setup("soft").await;
    for body in ["kept", "trashed"] {
        let mut note = Note {
            id: 0,
            body: body.into(),
            deleted_at: None,
        };
        note.insert(&db).await.unwrap();
    }
    Note::query()
        .where_eq("body", "trashed")
        .soft_delete(&db)
        .await
        .unwrap();

    let all = Note::all(&db).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].body, "kept");
    assert!(Note::find(&db, 2).await.unwrap().is_none());
    let trashed = Note::query().with_trashed().find(&db, 2).await.unwrap();
    assert_eq!(trashed.unwrap().body, "trashed");
    cleanup(path);
}

// --- belongs_to_many ----------------------------------------------------------

#[tokio::test]
async fn attach_reads_and_detach_through_the_pivot() {
    let (path, db) = setup("pivot").await;
    let [admin, editor, viewer]: [Role; 3] = Role::factory()
        .count(3)
        .create(&db)
        .await
        .unwrap()
        .try_into()
        .unwrap();
    let alice = User::factory().create_one(&db).await.unwrap();

    // Duplicates within one call collapse instead of hitting the pivot's key.
    assert_eq!(
        alice
            .attach_roles(&db, [admin.id, editor.id, admin.id])
            .await
            .unwrap(),
        2
    );
    let mut roles = alice.roles(&db).await.unwrap();
    roles.sort_by_key(|r| r.id);
    assert_eq!(roles, vec![admin, editor]);

    // The relation is a query: narrow it, count it, page it.
    assert_eq!(alice.roles_query().count(&db).await.unwrap(), 2);
    let first = alice
        .roles_query()
        .order_by_desc("roles.id")
        .first(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.name, roles[1].name);
    let page = alice.roles_query().paginate(&db, 1, 1).await.unwrap();
    assert_eq!((page.total, page.last_page), (2, 2));

    // Attaching one that's already there is an error, as in Laravel…
    assert!(alice.attach_roles(&db, [roles[0].id]).await.is_err());
    // …which is what `sync_without_detaching` is for.
    let changes = alice
        .sync_roles_without_detaching(&db, [roles[0].id, viewer.id])
        .await
        .unwrap();
    assert_eq!(
        changes,
        SyncChanges {
            attached: vec![viewer.id],
            detached: vec![],
        }
    );

    assert_eq!(alice.detach_roles(&db, [viewer.id]).await.unwrap(), 1);
    assert_eq!(alice.detach_all_roles(&db).await.unwrap(), 2);
    assert!(alice.roles(&db).await.unwrap().is_empty());
    cleanup(path);
}

#[tokio::test]
async fn sync_makes_exactly_the_given_set_attached() {
    let (path, db) = setup("sync").await;
    let roles = Role::factory().count(4).create(&db).await.unwrap();
    let ids: Vec<i64> = roles.iter().map(|r| r.id).collect();
    let bob = User::factory().create_one(&db).await.unwrap();
    bob.attach_roles(&db, [ids[0], ids[1]]).await.unwrap();

    let changes = bob.sync_roles(&db, [ids[1], ids[2], ids[3]]).await.unwrap();
    assert_eq!(changes.detached, vec![ids[0]]);
    assert_eq!(changes.attached, vec![ids[2], ids[3]]);

    let mut now: Vec<i64> = bob.roles(&db).await.unwrap().iter().map(|r| r.id).collect();
    now.sort_unstable();
    assert_eq!(now, vec![ids[1], ids[2], ids[3]]);

    // Syncing to the same set is a no-op; to nothing detaches everything.
    let same = bob.sync_roles(&db, [ids[3], ids[2], ids[1]]).await.unwrap();
    assert_eq!(same, SyncChanges::default());
    let cleared = bob.sync_roles(&db, []).await.unwrap();
    assert_eq!(cleared.detached.len(), 3);
    assert!(bob.roles(&db).await.unwrap().is_empty());
    cleanup(path);
}

#[tokio::test]
async fn eager_loading_through_a_pivot_shares_rows_without_clone() {
    let (path, db) = setup("pivot-eager").await;
    let roles = Role::factory().count(2).create(&db).await.unwrap();
    let users = User::factory().count(3).create(&db).await.unwrap();
    // users[0] and users[1] share roles[0]; users[2] has none.
    users[0]
        .attach_roles(&db, [roles[0].id, roles[1].id])
        .await
        .unwrap();
    users[1].attach_roles(&db, [roles[0].id]).await.unwrap();

    let map = User::load_roles(&db, &users).await.unwrap();
    assert_eq!(map[&users[0].id].len(), 2);
    assert_eq!(map[&users[1].id].len(), 1);
    assert_eq!(map[&users[1].id][0].name, roles[0].name);
    assert!(!map.contains_key(&users[2].id));

    // The field-level form hydrates straight into the struct.
    let mut hydrated = UserWithRoles::query()
        .order_by("id")
        .get(&db)
        .await
        .unwrap();
    UserWithRoles::with_roles(&db, &mut hydrated).await.unwrap();
    let counts: Vec<usize> = hydrated.iter().map(|u| u.roles.len()).collect();
    assert_eq!(counts, vec![2, 1, 0]);
    cleanup(path);
}

// --- factories ----------------------------------------------------------------

#[tokio::test]
async fn factories_make_and_create_distinct_rows() {
    let (path, db) = setup("factory").await;

    // `make` never touches the database.
    let drafts = Role::factory().count(3).make();
    assert_eq!(drafts.len(), 3);
    assert!(drafts.iter().all(|r| r.id == 0));
    assert_eq!(Role::query().count(&db).await.unwrap(), 0);

    // `n` is unique, so a UNIQUE column survives repeated builders.
    let a = Role::factory().count(2).create(&db).await.unwrap();
    let b = Role::factory().count(2).create(&db).await.unwrap();
    assert!(a.iter().chain(&b).all(|r| r.id > 0));
    assert_eq!(Role::query().count(&db).await.unwrap(), 4);

    // States apply in order, after the definition.
    let named = Role::factory()
        .state(|r| r.name = "first".into())
        .state(|r| r.name.push_str("-then-second"))
        .create_one(&db)
        .await
        .unwrap();
    assert_eq!(named.name, "first-then-second");

    // `sequence` sees each instance's index in the batch.
    let seq = Role::factory()
        .count(3)
        .sequence(|r, i| r.name = format!("seq-{i}"))
        .make();
    let names: Vec<&str> = seq.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["seq-0", "seq-1", "seq-2"]);
    cleanup(path);
}
