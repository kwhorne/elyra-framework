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
`bool`, `f64`, `&str`, `String`).

Placeholders are rendered per driver; **column identifiers are validated** (they
can't be bound), so `where_eq("a; DROP TABLE", ..)` is rejected, not executed.
An empty `where_in([])` matches nothing.

## Attributes

| Attribute | On | Meaning |
|---|---|---|
| `#[model(table = "..")]` | struct | Table name (default: lowercased struct name) |
| `#[model(timestamps)]` | struct | Auto-manage `created_at` / `updated_at` (unix seconds) |
| `#[model(id)]` | field | Mark the primary key (default: a field/column named `id`) |
| `#[model(column = "..")]` | field | Map the field to a differently-named column |
| `#[model(has_many(T, fk="..", as=".."))]` | struct | Relation (accessor + `load_*` map) |
| `#[model(has_one(T, fk="..", as=".."))]` | struct | Relation (accessor + `load_*` map) |
| `#[model(belongs_to(T, fk="..", as=".."))]` | struct | Relation (accessor + `load_*` map) |
| `#[model(has_many(T, fk=".."))]` | field | Relation hydrated into the field (`with_*`) |
| `#[model(has_one(T, fk=".."))]` | field | Relation hydrated into the field (`with_*`) |
| `#[model(belongs_to(T, fk=".."))]` | field | Relation hydrated into the field (`with_*`) |

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

## `bool` columns

`bool` fields map to an **INTEGER `0/1`** column: bind `0/1`, decode `!= 0`.
This is portable across all three drivers — the `Any` driver can't read SQLite's
native `BOOLEAN` type, so models never use one. Declare such columns `INTEGER`
in your migration.

## v1 assumptions

- Primary key is an `i64` autoincrement column (`0` = unsaved).
- Column name equals field name unless overridden with `#[model(column)]`.
- SQLite is test-covered; MySQL/Postgres are compile-verified.

## Related

- [Database](database.md) · [Migrations](migrations.md) · [Codegen](codegen.md)
