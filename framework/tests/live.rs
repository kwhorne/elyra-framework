//! Live queries, the data side (RFC 0002 step 1): every model-layer read is
//! recorded inside `live::track`, and every write reports its table to the
//! database's change hub — once it's durable, and only if something changed.
#![cfg(feature = "database")]

use std::collections::BTreeSet;

use elyra::db::live::{self, track};
use elyra::db::sqlx;
use elyra::{Database, Factory, Model};
use tokio::sync::broadcast::Receiver;

#[derive(Model, Debug, Clone)]
#[model(table = "teams", has_many(Member, fk = "team_id", as = "members"))]
struct Team {
    id: i64,
    name: String,
}

#[derive(Model, Debug, Clone)]
#[model(table = "members", soft_deletes, belongs_to_many(Role))]
struct Member {
    id: i64,
    team_id: i64,
    name: String,
    deleted_at: Option<i64>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "roles")]
struct Role {
    id: i64,
    name: String,
}

impl Factory for Role {
    fn definition(n: u64) -> Self {
        Role {
            id: 0,
            name: format!("role-{n}"),
        }
    }
}

async fn setup() -> (std::path::PathBuf, Database) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-live-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE teams (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE members (id INTEGER PRIMARY KEY AUTOINCREMENT, team_id INTEGER NOT NULL, \
             name TEXT NOT NULL, deleted_at INTEGER);\
         CREATE TABLE roles (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE member_role (member_id INTEGER NOT NULL, role_id INTEGER NOT NULL, \
             PRIMARY KEY (member_id, role_id));",
    )
    .execute(db.pool())
    .await
    .unwrap();
    (path, db)
}

/// Every key reported so far, without waiting.
fn drain(rx: &mut Receiver<std::sync::Arc<str>>) -> Vec<String> {
    let mut keys = Vec::new();
    while let Ok(key) = rx.try_recv() {
        keys.push(key.to_string());
    }
    keys
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

#[tokio::test]
async fn reads_record_every_table_they_touch() {
    let (path, db) = setup().await;
    let mut team = Team {
        id: 0,
        name: "core".into(),
    };
    team.insert(&db).await.unwrap();

    let (_, reads) = track(None, Team::query().get(&db)).await;
    assert_eq!(reads, set(&["teams"]));

    // A join reads both tables; an aggregate and a page read theirs.
    let (_, reads) = track(None, async {
        Member::query()
            .join("teams", "teams.id", "members.team_id")
            .count(&db)
            .await
    })
    .await;
    assert_eq!(reads, set(&["members", "teams"]));
    let (_, reads) = track(None, Member::query().paginate(&db, 1, 10)).await;
    assert_eq!(reads, set(&["members"]));

    // Relations: has_many and belongs_to_many (through the pivot).
    let (_, reads) = track(None, team.members(&db)).await;
    assert_eq!(reads, set(&["members"]));
    let mut member = Member {
        id: 0,
        team_id: team.id,
        name: "ada".into(),
        deleted_at: None,
    };
    member.insert(&db).await.unwrap();
    let (_, reads) = track(None, member.roles(&db)).await;
    assert!(
        reads.contains("roles") && reads.contains("member_role"),
        "{reads:?}"
    );

    // Raw SQL declares what it reads; reads outside `track` record nothing.
    let (_, reads) = track(None, async {
        live::depends_on("settings:theme");
        sqlx::query("SELECT 1").execute(db.pool()).await.unwrap();
    })
    .await;
    assert_eq!(reads, set(&["settings:theme"]));
    Team::query().get(&db).await.unwrap();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn writes_report_their_table_once_durable() {
    let (path, db) = setup().await;
    let mut rx = db.changes().subscribe();

    let mut team = Team {
        id: 0,
        name: "core".into(),
    };
    team.insert(&db).await.unwrap();
    team.name = "platform".into();
    team.save(&db).await.unwrap();
    assert_eq!(
        drain(&mut rx),
        ["teams", "teams"],
        "insert, then update via save"
    );

    let mut member = Member {
        id: 0,
        team_id: team.id,
        name: "ada".into(),
        deleted_at: None,
    };
    member.insert(&db).await.unwrap();
    drain(&mut rx);

    // Bulk writes report only when a row changed.
    Member::query()
        .where_eq("name", "nobody")
        .update(&db, &[("name", "x".into())])
        .await
        .unwrap();
    assert!(drain(&mut rx).is_empty(), "no row matched, nothing changed");
    Member::query()
        .where_eq("id", member.id)
        .soft_delete(&db)
        .await
        .unwrap();
    Member::query()
        .where_eq("id", member.id)
        .restore(&db)
        .await
        .unwrap();
    assert_eq!(drain(&mut rx), ["members", "members"]);

    // Pivot writes report the pivot table; factories report through insert.
    let roles = Role::factory().count(2).create(&db).await.unwrap();
    assert_eq!(drain(&mut rx), ["roles", "roles"]);
    member.attach_roles(&db, [roles[0].id]).await.unwrap();
    member.sync_roles(&db, [roles[1].id]).await.unwrap();
    member.sync_roles(&db, [roles[1].id]).await.unwrap(); // no change
    member.detach_all_roles(&db).await.unwrap();
    assert_eq!(
        drain(&mut rx),
        ["member_role", "member_role", "member_role"]
    );

    // A delete, and raw SQL reporting by hand.
    team.delete(&db).await.unwrap();
    db.touch("settings:theme");
    assert_eq!(drain(&mut rx), ["teams", "settings:theme"]);

    // A failed write reports nothing.
    let mut broken = Team {
        id: 0,
        name: "x".into(),
    };
    sqlx::raw_sql("DROP TABLE teams")
        .execute(db.pool())
        .await
        .unwrap();
    assert!(broken.insert(&db).await.is_err());
    assert!(drain(&mut rx).is_empty());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn every_clone_shares_one_hub() {
    let (path, db) = setup().await;
    let mut rx = db.changes().subscribe();
    let other = db.clone();
    other.touch("teams");
    assert_eq!(drain(&mut rx), ["teams"]);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_transaction_reports_on_commit_and_never_on_rollback() {
    let (path, db) = setup().await;
    let mut rx = db.changes().subscribe();

    db.transaction(|tx| {
        Box::pin(async move {
            sqlx::query("INSERT INTO teams (name) VALUES ('a')")
                .execute(&mut **tx)
                .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert!(
        drain(&mut rx).is_empty(),
        "raw SQL in a transaction reports nothing by itself"
    );

    let db2 = db.clone();
    db.transaction(move |tx| {
        let db = db2.clone();
        Box::pin(async move {
            sqlx::query("INSERT INTO teams (name) VALUES ('b')")
                .execute(&mut **tx)
                .await?;
            db.touch("teams");
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(drain(&mut rx), ["teams"], "touch inside, sent on commit");

    let db3 = db.clone();
    let rolled: elyra::db::Result<()> = db
        .transaction(move |_tx| {
            let db = db3.clone();
            Box::pin(async move {
                db.touch("teams");
                Err(elyra::db::Error::Query("abort".into()))
            })
        })
        .await;
    assert!(rolled.is_err());
    assert!(drain(&mut rx).is_empty(), "a rollback sends nothing");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_live_run_cannot_write() {
    let (path, db) = setup().await;
    let mut rx = db.changes().subscribe();
    let (result, _) = track(Some("teams_index"), async {
        let mut team = Team {
            id: 0,
            name: "sneaky".into(),
        };
        team.insert(&db).await
    })
    .await;
    let err = result.unwrap_err().to_string();
    assert!(err.contains("`teams_index` is a live command"), "{err}");
    assert_eq!(
        Team::query().count(&db).await.unwrap(),
        0,
        "nothing was written"
    );
    assert!(drain(&mut rx).is_empty());

    // Bulk writes and pivot writes are refused the same way.
    let (bulk, _) = track(Some("teams_index"), Team::query().delete(&db)).await;
    assert!(bulk.is_err());
    let _ = std::fs::remove_file(path);
}
