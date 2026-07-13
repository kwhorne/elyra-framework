//! Model relations (has_many / has_one / belongs_to) + `where_in`, on SQLite.
#![cfg(feature = "database")]

use elyra::db::sqlx;
use elyra::{Database, Model};

#[derive(Model, Debug)]
#[model(table = "users", has_many(Post, fk = "user_id", as = "posts"))]
struct User {
    id: i64,
    name: String,
}

#[derive(Model, Debug)]
#[model(table = "posts", belongs_to(User, fk = "user_id"))]
struct Post {
    id: i64,
    user_id: i64,
    title: String,
}

async fn setup() -> (std::path::PathBuf, Database) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-rel-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let db = Database::connect(&url).await.unwrap();
    sqlx::raw_sql(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE posts (id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER NOT NULL, title TEXT NOT NULL);",
    )
    .execute(db.pool())
    .await
    .unwrap();
    (path, db)
}

#[tokio::test]
async fn has_many_belongs_to_and_where_in() {
    let (path, db) = setup().await;

    let mut alice = User {
        id: 0,
        name: "alice".into(),
    };
    alice.insert(&db).await.unwrap();
    let mut bob = User {
        id: 0,
        name: "bob".into(),
    };
    bob.insert(&db).await.unwrap();

    for title in ["hello", "world"] {
        let mut p = Post {
            id: 0,
            user_id: alice.id,
            title: title.into(),
        };
        p.insert(&db).await.unwrap();
    }
    let mut bobs = Post {
        id: 0,
        user_id: bob.id,
        title: "solo".into(),
    };
    bobs.insert(&db).await.unwrap();

    // has_many: alice.posts()
    let posts = alice.posts(&db).await.unwrap();
    assert_eq!(posts.len(), 2);
    assert!(posts.iter().all(|p| p.user_id == alice.id));

    // belongs_to: post.user()
    let owner = posts[0].user(&db).await.unwrap().unwrap();
    assert_eq!(owner.id, alice.id);
    assert_eq!(owner.name, "alice");

    // where_in: eager-load posts for a set of user ids, then group.
    let all = Post::query()
        .where_in("user_id", [alice.id, bob.id])
        .order_by("id")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);

    // empty IN matches nothing.
    let none = Post::query()
        .where_in("user_id", Vec::<i64>::new())
        .get(&db)
        .await
        .unwrap();
    assert!(none.is_empty());

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn eager_loading_avoids_n_plus_1() {
    let (path, db) = setup().await;

    let mut alice = User {
        id: 0,
        name: "alice".into(),
    };
    alice.insert(&db).await.unwrap();
    let mut bob = User {
        id: 0,
        name: "bob".into(),
    };
    bob.insert(&db).await.unwrap();

    for (uid, title) in [(alice.id, "a1"), (alice.id, "a2"), (bob.id, "b1")] {
        let mut p = Post {
            id: 0,
            user_id: uid,
            title: title.into(),
        };
        p.insert(&db).await.unwrap();
    }

    // has_many eager load: one query for all parents, grouped by parent id.
    let users = User::all(&db).await.unwrap();
    let by_user = User::load_posts(&db, &users).await.unwrap();
    assert_eq!(by_user.get(&alice.id).map(|v| v.len()), Some(2));
    assert_eq!(by_user.get(&bob.id).map(|v| v.len()), Some(1));

    // belongs_to eager load: one query for all owners, keyed by owner id.
    let posts = Post::all(&db).await.unwrap();
    let owners = Post::load_user(&db, &posts).await.unwrap();
    assert_eq!(owners.len(), 2); // alice + bob, deduped
                                 // join a child to its eager-loaded owner:
    let first = &posts[0];
    assert_eq!(
        owners.get(&first.user_id).map(|u| u.name.as_str()),
        Some("alice")
    );

    let _ = std::fs::remove_file(&path);
}

// --- Field-based auto-hydration -------------------------------------------

#[derive(Model, Debug)]
#[model(table = "h_authors")]
struct Author {
    id: i64,
    name: String,
    #[model(has_many(Book, fk = "author_id"))]
    books: Vec<Book>,
}

#[derive(Model, Debug)]
#[model(table = "h_books")]
struct Book {
    id: i64,
    author_id: i64,
    title: String,
}

#[derive(Model, Debug, Clone)]
#[model(table = "h_owners")]
struct Owner {
    id: i64,
    label: String,
}

#[derive(Model, Debug)]
#[model(table = "h_items")]
struct Item {
    id: i64,
    owner_id: i64,
    #[model(belongs_to(Owner, fk = "owner_id"))]
    owner: Option<Owner>,
}

#[tokio::test]
async fn field_relations_hydrate_into_the_struct() {
    let (path, db) = setup().await;
    sqlx::raw_sql(
        "CREATE TABLE h_authors (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE h_books (id INTEGER PRIMARY KEY AUTOINCREMENT, author_id INTEGER NOT NULL, title TEXT NOT NULL);\
         CREATE TABLE h_owners (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL);\
         CREATE TABLE h_items (id INTEGER PRIMARY KEY AUTOINCREMENT, owner_id INTEGER NOT NULL);",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let mut a = Author {
        id: 0,
        name: "asimov".into(),
        books: vec![],
    };
    a.insert(&db).await.unwrap();
    let mut b = Author {
        id: 0,
        name: "le guin".into(),
        books: vec![],
    };
    b.insert(&db).await.unwrap();
    for (aid, title) in [(a.id, "foundation"), (a.id, "i, robot"), (b.id, "earthsea")] {
        let mut book = Book {
            id: 0,
            author_id: aid,
            title: title.into(),
        };
        book.insert(&db).await.unwrap();
    }

    // has_many: books hydrate straight into author.books.
    let mut authors = Author::query().order_by("id").get(&db).await.unwrap();
    assert!(authors.iter().all(|x| x.books.is_empty()));
    Author::with_books(&db, &mut authors).await.unwrap();
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[1].books.len(), 1);
    assert_eq!(authors[1].books[0].title, "earthsea");

    // belongs_to: the shared owner hydrates into each item.owner.
    let mut o = Owner {
        id: 0,
        label: "acme".into(),
    };
    o.insert(&db).await.unwrap();
    for _ in 0..2 {
        let mut it = Item {
            id: 0,
            owner_id: o.id,
            owner: None,
        };
        it.insert(&db).await.unwrap();
    }
    let mut items = Item::all(&db).await.unwrap();
    assert!(items.iter().all(|x| x.owner.is_none()));
    Item::with_owner(&db, &mut items).await.unwrap();
    assert!(items
        .iter()
        .all(|x| x.owner.as_ref().map(|ow| ow.label.as_str()) == Some("acme")));

    let _ = std::fs::remove_file(&path);
}
