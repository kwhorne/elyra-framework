# Models

`#[derive(Model)]` is Elyra's Eloquent — a thin Active-Record layer over the
[`Database`](database.md) pool (not a second ORM engine). Feature-gated behind
`database`.

```rust
use elyra::Model;

#[derive(Model, serde::Serialize, serde::Deserialize, specta::Type)]
#[model(table = "todos", timestamps)]
struct Todo {
    #[model(id)] id: i64,
    title: String,
    done: bool,                          // <-> INTEGER 0/1 column
    #[model(column = "body")] text: String,
    created_at: i64,
    updated_at: i64,
}
```

Because models are plain structs, a command returning `Vec<Todo>` becomes
`Promise<Todo[]>` through [codegen](codegen.md).

## Generated methods

```rust
Todo::all(&db).await?;                        // Vec<Todo>
Todo::find(&db, 1).await?;                     // Option<Todo>

let mut t = Todo { id: 0, title: "milk".into(), done: false, text: "".into(),
                   created_at: 0, updated_at: 0 };
t.insert(&db).await?;                          // sets t.id (+ timestamps)
t.done = true;
t.save(&db).await?;                            // insert-if-new (id == 0) else update
t.update(&db).await?;                          // UPDATE ... WHERE id = ?
t.delete(&db).await?;                          // DELETE ... WHERE id = ?
```

- `insert` sets the primary key from the database (`RETURNING` on sqlite/postgres,
  `last_insert_id()` on MySQL).
- `save` inserts when `id == 0`, otherwise updates.
- `all` and `find` go through the query builder, so they respect
  [soft deletes](#soft-deletes) and [global scopes](#global-scopes) like every
  other read.

## Query builder

```rust
Todo::query()
    .where_eq("done", false)
    .where_gt("id", 10)
    .where_in("id", [1, 2, 3])
    .order_by("id")            // or .order_by_desc("id")
    .limit(50)
    .get(&db).await?;          // Vec<Todo>

Todo::query().where_eq("title", "milk").first(&db).await?;   // Option<Todo>
```

Comparisons: `where_eq`, `where_ne`, `where_lt`, `where_gt`, `where_lte`,
`where_gte`, plus `where_in`. Values implement `Into<Value>` (`i64`, `i32`,
`i16`, `bool`, `f64`, `f32`, `&str`, `String`, `&String`, and `Option<T>` of any
of them — `None` binds as `NULL`).

Placeholders are rendered per driver; **column identifiers are validated** (they
can't be bound), so `where_eq("a; DROP TABLE", ..)` is rejected, not executed.
An empty `where_in([])` matches nothing.

## Attributes

| Attribute | On | Meaning |
|---|---|---|
| `#[model(table = "..")]` | struct | Table name (default: lowercased struct name) |
| `#[model(timestamps)]` | struct | Auto-manage `created_at` / `updated_at` (unix seconds) |
| `#[model(soft_deletes)]` | struct | Hide rows whose `deleted_at` is set ([soft deletes](#soft-deletes)) |
| `#[model(global_scope = path)]` | struct | Constrain every query ([global scopes](#global-scopes)); repeatable |
| `#[model(id)]` | field | Mark the primary key (default: a field/column named `id`) |
| `#[model(column = "..")]` | field | Map the field to a differently-named column |
| `#[model(cast = "json" \| "text" \| Path)]` | field | Store a non-scalar type ([casts](#casts)) |
| `#[model(has_many(T, fk="..", as=".."))]` | struct | Relation (accessor + `load_*` map) |
| `#[model(has_one(T, fk="..", as=".."))]` | struct | Relation (accessor + `load_*` map) |
| `#[model(belongs_to(T, fk="..", as=".."))]` | struct | Relation (accessor + `load_*` map) |
| `#[model(belongs_to_many(T, pivot="..", fk="..", related_fk="..", as=".."))]` | struct | [Many-to-many](#many-to-many-belongs_to_many) through a pivot |
| `#[model(has_many(T, fk=".."))]` | field | Relation hydrated into the field (`with_*`) |
| `#[model(has_one(T, fk=".."))]` | field | Relation hydrated into the field (`with_*`) |
| `#[model(belongs_to(T, fk=".."))]` | field | Relation hydrated into the field (`with_*`) |
| `#[model(belongs_to_many(T, pivot="..", fk="..", related_fk=".."))]` | field | Many-to-many hydrated into the field (`with_*`) |

**Unknown options are compile errors**, and table/column names must be bare SQL
identifiers. The derive used to ignore what it didn't recognise — a misspelt
`global_scope` would have compiled into a model that silently leaked across
tenants.

## Relations

```rust
#[derive(Model)]
#[model(table = "users", has_many(Post, fk = "user_id", as = "posts"))]
struct User { id: i64, name: String }

#[derive(Model)]
#[model(table = "posts", belongs_to(User, fk = "user_id"))]
struct Post { id: i64, user_id: i64, title: String }

let posts = user.posts(&db).await?;        // has_many -> Vec<Post>
let owner = post.user(&db).await?;         // belongs_to -> Option<User>
```

- `has_many` → `Vec<T>`; `has_one` → `Option<T>`; `belongs_to` → `Option<T>`.
- `fk` and `as` (the method name) are optional; defaults are derived from the
  type names (`{type}_id`, pluralized/singular lowercase name). Multi-word type
  names need explicit `fk`/`as`.

### Eager loading

Relation accessors (`user.posts(&db)`) are lazy — one query each, so calling them
in a loop is N+1. For a batch of parents, each relation also generates a
`load_<name>` method that runs **one** query and returns a `HashMap` for joining:

```rust
let users = User::all(&db).await?;

// has_many: keyed by parent PK -> Vec<child>
let by_user = User::load_posts(&db, &users).await?;   // HashMap<i64, Vec<Post>>
for user in &users {
    let posts = by_user.get(&user.id).cloned().unwrap_or_default();
}

// belongs_to: keyed by owner PK -> owner
let posts = Post::all(&db).await?;
let owners = Post::load_user(&db, &posts).await?;      // HashMap<i64, User>
let owner = owners.get(&posts[0].user_id);
```

`has_one` generates `load_<name>` returning `HashMap<i64, T>` (first child per
parent). Under the hood these use `where_in` + grouping; you can also drop to the
primitive directly:

```rust
let posts = Post::query().where_in("user_id", ids).get(&db).await?;
```

### Auto-hydration (relation fields)

Instead of joining a `HashMap` yourself, declare the relation **on a field** and
let the data hydrate straight into the struct. The field is not a column (it is
skipped by `COLUMNS`, `insert`, and `from_row`, and defaults to empty):

```rust
#[derive(Model, Debug)]
#[model(table = "authors")]
struct Author {
    id: i64,
    name: String,
    #[model(has_many(Book, fk = "author_id"))]
    books: Vec<Book>,          // filled by `with_books`, empty otherwise
}

#[derive(Model, Debug, Clone)] // belongs_to targets must be `Clone`
#[model(table = "books")]
struct Book {
    id: i64,
    author_id: i64,
    title: String,
    #[model(belongs_to(Author, fk = "author_id"))]
    author: Option<Author>,
}
```

Each relation field generates a `with_<field>` batch hydrator that runs **one**
query and assigns the result into every element:

```rust
let mut authors = Author::all(&db).await?;
Author::with_books(&db, &mut authors).await?;   // authors[i].books now populated

let mut books = Book::all(&db).await?;
Book::with_author(&db, &mut books).await?;      // books[i].author == Some(..)
```

- `has_many` → `Vec<T>`, `has_one` / `belongs_to` → `Option<T>`.
- `fk` defaults as for struct-level relations (`{self}_id`, or `{target}_id` for
  `belongs_to`).
- `belongs_to` clones the shared owner into each child, so the target type must
  derive `Clone`.

### Many-to-many (`belongs_to_many`)

```rust
#[derive(Model)]
#[model(table = "users", belongs_to_many(Role))]
struct User { id: i64, name: String }

#[derive(Model)]
#[model(table = "roles")]
struct Role { id: i64, name: String }
```

The pivot follows Laravel's conventions unless overridden: the two model names in
alphabetical order (`role_user`), `user_id` pointing at the declaring model and
`role_id` at the other. Override with `pivot = ".."`, `fk = ".."`,
`related_fk = ".."`; name the methods with `as = ".."` (default: `roles`).

```rust
user.roles(&db).await?;                                  // Vec<Role>
user.roles_query().where_like("name", "a%").paginate(&db, 1, 20).await?;

user.attach_roles(&db, [admin.id, editor.id]).await?;    // rows inserted
user.detach_roles(&db, [editor.id]).await?;
user.detach_all_roles(&db).await?;

let changes = user.sync_roles(&db, ids).await?;          // exactly these, in one transaction
changes.attached; changes.detached;                      // what actually changed
user.sync_roles_without_detaching(&db, [viewer.id]).await?;  // the idempotent attach

let by_user = User::load_roles(&db, &users).await?;      // HashMap<i64, Vec<Role>>, one query
```

- **`attach` of an already-attached id is an error**, as in Laravel, when the
  pivot's primary key covers both columns (it should). Duplicates *within one
  call* are collapsed. Use `sync_roles_without_detaching` for "make sure these
  are attached".
- **Eager loading is one query:** the pivot's parent key is selected alongside
  the related columns, so each row already says which parent it belongs to. The
  related model therefore doesn't need `Clone`, even when parents share a row.
- On a field (`#[model(belongs_to_many(Role, pivot = "role_user", fk = "user_id"))]
  roles: Vec<Role>`), `with_roles` hydrates the field, and the write methods are
  named by the field.
- Both keys are `i64`; a model with another primary-key type can't declare the
  relation (compile error). Pivot columns beyond the two keys (Laravel's
  `withPivot`, pivot timestamps) aren't supported yet.

A pivot migration:

```rust
Schema::create("role_user", |t| {
    t.foreign_id("role_id", "roles").on_delete_cascade();
    t.foreign_id("user_id", "users").on_delete_cascade();
    t.primary(&["role_id", "user_id"]);
})
```

## Casts

The `Any` driver behind every model decodes SQL scalars only, so a `Vec<String>`,
a struct or an enum can't be a plain field. A cast bridges it — Laravel's
`$casts`:

```rust
#[derive(Model)]
struct Post {
    id: i64,
    #[model(cast = "json")] tags: Vec<String>,        // TEXT, JSON-encoded
    #[model(cast = "json")] meta: Option<Meta>,       // NULL <-> None
    #[model(cast = "text")] status: Status,           // TEXT, via Display / FromStr
    #[model(cast = Cents)]  price: Money,             // your own caster
}
```

| Cast | Field type | Column |
|---|---|---|
| `"json"` | any `Serialize + DeserializeOwned` | `TEXT` (`NULL` ⇄ `None`) |
| `"text"` | any `Display + FromStr` | `TEXT` |
| a path | whatever it implements `Cast<T>` for | whatever it encodes |

A custom cast is a marker type implementing
[`elyra::db::cast::Cast<T>`](https://docs.rs/elyra-db/latest/elyra_db/cast/trait.Cast.html):

```rust
use elyra::db::cast::Cast;
use elyra::db::sqlx::{any::AnyRow, Row};

struct Cents;
impl Cast<Money> for Cents {
    fn encode(v: &Money) -> elyra::db::Result<Value> { Ok(Value::Int(v.cents())) }
    fn decode(row: &AnyRow, col: &str) -> elyra::db::Result<Money> {
        Ok(Money::from_cents(row.try_get::<i64, _>(col)?))
    }
}
```

A value that won't decode (an unknown enum spelling, malformed JSON) is an error
naming the column, not a panic. **On Postgres use `t.text()` for a JSON cast
column**, not `t.json()`: the latter creates `JSONB`, which the `Any` driver can't
read. In a bulk `Query::update`, pass the encoded value yourself —
`("tags", Json::encode(&tags)?)`.

## Scopes

### Local scopes

A local scope is any `fn(Query<M>) -> Query<M>` — Laravel's `scopeActive`,
without the naming magic:

```rust
fn active(q: Query<User>) -> Query<User> { q.where_eq("active", true) }
fn admins(q: Query<User>) -> Query<User> { q.where_eq("role", "admin") }

User::query().scope(active).scope(admins).get(&db).await?;
```

`when` and `when_some` apply a clause only when there is something to filter on —
the shape a search form produces:

```rust
User::query()
    .when(filter.only_active, active)
    .when_some(filter.name, |q, name| q.where_like("name", format!("%{name}%")))
    .paginate(&db, filter.page, 25).await?;
```

### Global scopes

A global scope constrains **every** query for the model — `query()`, `find`,
`all`, counts, bulk `update` / `delete`, and relation queries *into* the model:

```rust
fn current_workspace(q: Query<Doc>) -> Query<Doc> {
    q.where_eq("workspace_id", workspace::current())
}

#[derive(Model)]
#[model(table = "docs", global_scope = current_workspace)]
struct Doc { id: i64, workspace_id: i64, title: String }

Doc::find(&db, id).await?;                                // None if it's another workspace's
Doc::query().without_global_scopes().count(&db).await?;   // an explicit, greppable opt-out
```

A scope is a plain function with no `Ctx`, so a runtime value like the current
workspace comes from state your app owns — an `AtomicI64` or `OnceLock` it sets
on sign-in, the same way Laravel's scopes read `auth()->user()`.

Only the scope's `where` constraints apply; ordering, limits or joins it sets are
ignored rather than silently changing every query. Soft deletes are independent —
`without_global_scopes` doesn't include trashed rows; `with_trashed` does.

## Factories

A factory is a recipe for a valid row — Laravel's `User::factory()`:

```rust
use elyra::Factory;

impl Factory for User {
    fn definition(n: u64) -> Self {
        User { id: 0, name: format!("User {n}"), email: format!("user{n}@example.test"), admin: false }
    }
}

let users = User::factory().count(3).create(&db).await?;              // inserted, ids set
let admin = User::factory().state(|u| u.admin = true).create_one(&db).await?;
let drafts = User::factory().count(10).make();                        // not inserted
User::factory().count(4).sequence(|u, i| u.admin = i == 0).create(&db).await?;

// Related rows: the child's factory, pointed at the parent.
Post::factory().count(5).state(move |p| p.user_id = admin.id).create(&db).await?;
```

`n` is unique for the life of the process — across models and builders — so it is
safe for columns with a unique constraint. States apply in the order they were
added, after the definition. Factories work in tests and in
[seeders](migrations.md) alike.

## `bool` columns

`bool` fields map to an **INTEGER `0/1`** column: bind `0/1`, decode `!= 0`.
This is portable across all three drivers — the `Any` driver can't read SQLite's
native `BOOLEAN` type, so models never use one. Declare such columns `INTEGER`
in your migration.

## Primary keys

The default is a single **`i64` autoincrement** column: the database assigns it
and `insert` reads it back (`RETURNING` on SQLite/Postgres, `last_insert_id` on
MySQL); `save()` treats `0` as unsaved.

A **non-`i64` single-column key** (e.g. `String`) is also supported — mark it
with `#[model(id)]`:

```rust
#[derive(Model)]
#[model(table = "settings")]
struct Setting {
    #[model(id)]
    key: String,   // app-supplied; included in INSERT, not read back
    value: String,
}

Setting { key: "theme".into(), value: "dark".into() }.insert(&db).await?;
let s = Setting::find(&db, "theme".to_string()).await?;
```

For an app-supplied key the value is inserted as-is (no key retrieval), and
`find` takes that key type. `save()` uses the type's `Default` as the "unsaved"
sentinel (`""` for `String`), so prefer explicit `insert` / `update` when the
key is always set. Relation eager-loading still assumes `i64` keys.

## Query builder

```rust
// Filtering
let users = User::query()
    .where_eq("active", true)
    .where_like("email", "%@example.com")
    .where_between("age", 18, 65)
    .where_not_null("verified_at")
    .or_where_eq(&[("role", "admin".into()), ("role", "owner".into())])
    .order_by("name")
    .order_by_desc("created_at")        // chains: ORDER BY name ASC, created_at DESC
    .limit(20)
    .offset(40)
    .get(&db)
    .await?;

// Aggregates (limit/offset are ignored, filters are not)
let total   = User::query().count(&db).await?;
let any     = User::query().where_eq("active", true).exists(&db).await?;
let sum     = Order::query().sum(&db, "total").await?;      // Option<f64>
let average = Order::query().avg(&db, "total").await?;
let oldest  = User::query().min(&db, "created_at").await?;  // Option<i64>

// Pagination
let page = User::query().order_by("id").paginate(&db, 2, 25).await?;
page.data;         // Vec<User>
page.total;        // matching rows
page.last_page;    // page count
page.has_more();   // bool
(page.from(), page.to());

// Joins (identifiers may be table-qualified)
let recent = Product::query()
    .join("orders", "orders.product_id", "products.id")
    .where_gte("orders.quantity", 2)
    .get(&db)
    .await?;

// Bulk writes
let updated = User::query()
    .where_eq("active", false)
    .update(&db, &[("active", true.into()), ("note", "reactivated".into())])
    .await?;
let removed = User::query().where_lt("age", 13).delete(&db).await?;

// Batching without loading everything at once
User::query().chunk(&db, 500, |batch| {
    for user in batch { /* … */ }
    Ok(())
}).await?;
```

## Soft deletes

```rust
#[derive(Model)]
#[model(table = "accounts", soft_deletes)]
struct Account {
    id: i64,
    email: String,
    deleted_at: Option<i64>,
}
```

`deleted_at` (unix seconds, nullable) makes every query skip trashed rows —
including `find` and `all`:

```rust
Account::query().count(&db).await?;                        // live rows only
Account::query().with_trashed().count(&db).await?;         // include trashed
Account::query().only_trashed().get(&db).await?;           // just the trashed

Account::query().where_eq("email", &email).soft_delete(&db).await?;  // set deleted_at
Account::query().where_eq("email", &email).restore(&db).await?;      // clear it
Account::query().where_eq("email", &email).delete(&db).await?;       // hard delete

Account::query().with_trashed().find(&db, id).await?;                // reach a trashed row by id
```

The instance method `account.delete(&db)` is a **hard** delete; soft-delete a
single row through the builder, as above.

Add the column with the [schema builder](migrations.md)'s `t.soft_deletes()`.

## v1 assumptions

- Composite (multi-column) primary keys are not supported (a pivot's composite
  key is fine — it isn't a model).
- `group_by`/`having`, `first_or_create`/`update_or_create`/`upsert`, polymorphic
  relations, `has_many_through`, model events/observers and accessors/mutators
  are not implemented yet.
- Column name equals field name unless overridden with `#[model(column)]`.
- SQLite is test-covered; MySQL/Postgres run in CI against real servers.

## Related

- [Database](database.md) · [Migrations](migrations.md) · [Codegen](codegen.md)
