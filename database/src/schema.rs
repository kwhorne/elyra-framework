//! A schema builder — Elyra's `Schema::create`.
//!
//! Migrations used to be raw `.sql` files only, which meant every app wrote
//! driver-specific DDL by hand even though the framework spans SQLite, MySQL and
//! Postgres. This module renders the DDL per driver instead:
//!
//! ```
//! use elyra_db::{schema::Schema, Driver};
//!
//! let sql = Schema::create("users", |t| {
//!     t.id();
//!     t.string("email").unique();
//!     t.string("name").nullable();
//!     t.integer("age").default_value("0");
//!     t.boolean("active").default_value("1");
//!     t.timestamps();
//!     t.index("email");
//! })
//! .to_sql(Driver::Sqlite);
//!
//! assert!(sql[0].starts_with("CREATE TABLE \"users\""));
//! ```
//!
//! Every statement list is ordered: the `CREATE TABLE` first, then its indexes.
//! Feed them to [`Schema`]'s `execute` helpers, or embed them in a
//! [`Migration`](crate::Migration).

use crate::Driver;

/// A column type, mapped to each backend's spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnType {
    /// Auto-incrementing 64-bit primary key.
    Id,
    BigInteger,
    Integer,
    /// `VARCHAR(n)`.
    String(u32),
    Text,
    Boolean,
    Float,
    /// Unix-seconds timestamp (an integer, so the `Any` driver can read it).
    Timestamp,
    Json,
    Blob,
}

impl ColumnType {
    fn sql(&self, driver: Driver) -> String {
        match (self, driver) {
            (ColumnType::Id, Driver::Sqlite) => "INTEGER PRIMARY KEY AUTOINCREMENT".into(),
            (ColumnType::Id, Driver::MySql) => "BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY".into(),
            (ColumnType::Id, Driver::Postgres) => "BIGSERIAL PRIMARY KEY".into(),
            (ColumnType::BigInteger, _) => "BIGINT".into(),
            (ColumnType::Integer, _) => "INTEGER".into(),
            (ColumnType::String(n), _) => format!("VARCHAR({n})"),
            (ColumnType::Text, _) => "TEXT".into(),
            // Stored as 0/1 integers: the sqlx `Any` driver can't read a native
            // BOOLEAN, which is also why `#[derive(Model)]` maps bool that way.
            (ColumnType::Boolean, _) => "INTEGER".into(),
            (ColumnType::Float, Driver::MySql) => "DOUBLE".into(),
            (ColumnType::Float, _) => "DOUBLE PRECISION".into(),
            (ColumnType::Timestamp, _) => "BIGINT".into(),
            (ColumnType::Json, Driver::Postgres) => "JSONB".into(),
            (ColumnType::Json, _) => "TEXT".into(),
            (ColumnType::Blob, Driver::Postgres) => "BYTEA".into(),
            (ColumnType::Blob, Driver::MySql) => "LONGBLOB".into(),
            (ColumnType::Blob, Driver::Sqlite) => "BLOB".into(),
        }
    }

    fn is_id(&self) -> bool {
        matches!(self, ColumnType::Id)
    }
}

/// One column definition.
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    ty: ColumnType,
    nullable: bool,
    unique: bool,
    default: Option<String>,
}

impl Column {
    fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: false,
            unique: false,
            default: None,
        }
    }

    fn to_sql(&self, driver: Driver) -> String {
        let mut sql = format!("{} {}", quote(&self.name, driver), self.ty.sql(driver));
        if self.ty.is_id() {
            return sql; // the type already carries the key + not-null semantics
        }
        if self.nullable {
            sql.push_str(" NULL");
        } else {
            sql.push_str(" NOT NULL");
        }
        if let Some(default) = &self.default {
            sql.push_str(&format!(" DEFAULT {default}"));
        }
        if self.unique {
            sql.push_str(" UNIQUE");
        }
        sql
    }
}

/// The column being defined, so modifiers can be chained
/// (`t.string("email").unique().nullable()`).
pub struct ColumnBuilder<'a> {
    table: &'a mut Table,
    index: usize,
}

impl ColumnBuilder<'_> {
    /// Allow NULL.
    pub fn nullable(self) -> Self {
        self.table.columns[self.index].nullable = true;
        self
    }

    /// Add a UNIQUE constraint.
    pub fn unique(self) -> Self {
        self.table.columns[self.index].unique = true;
        self
    }

    /// A raw SQL default (`"0"`, `"'pending'"`, `"CURRENT_TIMESTAMP"`).
    pub fn default_value(self, value: impl Into<String>) -> Self {
        self.table.columns[self.index].default = Some(value.into());
        self
    }
}

/// A table definition under construction.
#[derive(Debug, Default)]
pub struct Table {
    name: String,
    columns: Vec<Column>,
    indexes: Vec<(Vec<String>, bool)>,
    primary_key: Vec<String>,
    foreign_keys: Vec<(String, String, String, Option<String>)>,
}

impl Table {
    fn column(&mut self, name: impl Into<String>, ty: ColumnType) -> ColumnBuilder<'_> {
        self.columns.push(Column::new(name, ty));
        let index = self.columns.len() - 1;
        ColumnBuilder { table: self, index }
    }

    /// An auto-incrementing `id` primary key.
    pub fn id(&mut self) -> ColumnBuilder<'_> {
        self.column("id", ColumnType::Id)
    }

    /// A named auto-incrementing primary key.
    pub fn id_named(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Id)
    }

    /// `VARCHAR(255)`.
    pub fn string(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::String(255))
    }

    /// `VARCHAR(n)`.
    pub fn string_len(&mut self, name: &str, len: u32) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::String(len))
    }

    pub fn text(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Text)
    }

    pub fn integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Integer)
    }

    pub fn big_integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::BigInteger)
    }

    /// A `0/1` integer column (what `#[derive(Model)]` expects for `bool`).
    pub fn boolean(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Boolean)
    }

    pub fn float(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Float)
    }

    /// A unix-seconds timestamp column.
    pub fn timestamp(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Timestamp)
    }

    pub fn json(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Json)
    }

    pub fn blob(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Blob)
    }

    /// `created_at` + `updated_at`, matching `#[model(timestamps)]`.
    pub fn timestamps(&mut self) {
        self.column("created_at", ColumnType::Timestamp);
        self.column("updated_at", ColumnType::Timestamp);
    }

    /// A nullable `deleted_at`, matching `#[model(soft_deletes)]`.
    pub fn soft_deletes(&mut self) {
        self.column("deleted_at", ColumnType::Timestamp).nullable();
    }

    /// A foreign key column plus its constraint.
    pub fn foreign_id(&mut self, name: &str, references_table: &str) -> &mut Self {
        self.column(name, ColumnType::BigInteger);
        self.foreign_keys.push((
            name.to_string(),
            references_table.to_string(),
            "id".to_string(),
            None,
        ));
        self
    }

    /// `ON DELETE CASCADE` for the most recently declared foreign key.
    pub fn on_delete_cascade(&mut self) -> &mut Self {
        if let Some(last) = self.foreign_keys.last_mut() {
            last.3 = Some("CASCADE".into());
        }
        self
    }

    /// A composite primary key (for tables that don't use `id()`).
    pub fn primary(&mut self, columns: &[&str]) -> &mut Self {
        self.primary_key = columns.iter().map(|c| c.to_string()).collect();
        self
    }

    /// A non-unique index.
    pub fn index(&mut self, column: &str) -> &mut Self {
        self.indexes.push((vec![column.to_string()], false));
        self
    }

    /// A multi-column index.
    pub fn index_on(&mut self, columns: &[&str]) -> &mut Self {
        self.indexes
            .push((columns.iter().map(|c| c.to_string()).collect(), false));
        self
    }

    /// A unique index.
    pub fn unique_index(&mut self, columns: &[&str]) -> &mut Self {
        self.indexes
            .push((columns.iter().map(|c| c.to_string()).collect(), true));
        self
    }
}

/// A pending schema change, renderable to per-driver SQL.
pub enum Schema {
    Create(Table),
    Alter { table: String, changes: Vec<Change> },
    Drop { table: String, if_exists: bool },
    Rename { from: String, to: String },
}

/// One `ALTER TABLE` step.
pub enum Change {
    AddColumn(Column),
    DropColumn(String),
    AddIndex { columns: Vec<String>, unique: bool },
    DropIndex(String),
    RenameColumn { from: String, to: String },
}

/// Builder for `Schema::table` (alterations).
#[derive(Default)]
pub struct Alter {
    table: Table,
    changes: Vec<Change>,
}

impl Alter {
    /// Add a column (`t.add(|c| c.string("nickname"))` style helpers below).
    pub fn add_string(&mut self, name: &str) -> &mut Self {
        self.push(Column::new(name, ColumnType::String(255)))
    }

    pub fn add_text(&mut self, name: &str) -> &mut Self {
        self.push(Column::new(name, ColumnType::Text))
    }

    pub fn add_integer(&mut self, name: &str) -> &mut Self {
        self.push(Column::new(name, ColumnType::Integer))
    }

    pub fn add_boolean(&mut self, name: &str) -> &mut Self {
        self.push(Column::new(name, ColumnType::Boolean))
    }

    pub fn add_timestamp(&mut self, name: &str) -> &mut Self {
        self.push(Column::new(name, ColumnType::Timestamp))
    }

    /// Make the column added last nullable.
    pub fn nullable(&mut self) -> &mut Self {
        if let Some(Change::AddColumn(column)) = self.changes.last_mut() {
            column.nullable = true;
        }
        self
    }

    /// Give the column added last a default.
    pub fn default_value(&mut self, value: impl Into<String>) -> &mut Self {
        if let Some(Change::AddColumn(column)) = self.changes.last_mut() {
            column.default = Some(value.into());
        }
        self
    }

    fn push(&mut self, column: Column) -> &mut Self {
        self.changes.push(Change::AddColumn(column));
        self
    }

    pub fn drop_column(&mut self, name: &str) -> &mut Self {
        self.changes.push(Change::DropColumn(name.to_string()));
        self
    }

    pub fn rename_column(&mut self, from: &str, to: &str) -> &mut Self {
        self.changes.push(Change::RenameColumn {
            from: from.to_string(),
            to: to.to_string(),
        });
        self
    }

    pub fn index(&mut self, columns: &[&str]) -> &mut Self {
        self.changes.push(Change::AddIndex {
            columns: columns.iter().map(|c| c.to_string()).collect(),
            unique: false,
        });
        self
    }

    pub fn unique_index(&mut self, columns: &[&str]) -> &mut Self {
        self.changes.push(Change::AddIndex {
            columns: columns.iter().map(|c| c.to_string()).collect(),
            unique: true,
        });
        self
    }

    pub fn drop_index(&mut self, name: &str) -> &mut Self {
        self.changes.push(Change::DropIndex(name.to_string()));
        self
    }
}

impl Schema {
    /// Define a new table.
    pub fn create(name: &str, define: impl FnOnce(&mut Table)) -> Schema {
        let mut table = Table {
            name: name.to_string(),
            ..Table::default()
        };
        define(&mut table);
        Schema::Create(table)
    }

    /// Alter an existing table.
    pub fn table(name: &str, define: impl FnOnce(&mut Alter)) -> Schema {
        let mut alter = Alter::default();
        alter.table.name = name.to_string();
        define(&mut alter);
        Schema::Alter {
            table: name.to_string(),
            changes: alter.changes,
        }
    }

    /// `DROP TABLE`.
    pub fn drop(name: &str) -> Schema {
        Schema::Drop {
            table: name.to_string(),
            if_exists: false,
        }
    }

    /// `DROP TABLE IF EXISTS`.
    pub fn drop_if_exists(name: &str) -> Schema {
        Schema::Drop {
            table: name.to_string(),
            if_exists: true,
        }
    }

    /// Rename a table.
    pub fn rename(from: &str, to: &str) -> Schema {
        Schema::Rename {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    /// Render the statements for `driver`, in execution order.
    pub fn to_sql(&self, driver: Driver) -> Vec<String> {
        match self {
            Schema::Create(table) => {
                let mut parts: Vec<String> =
                    table.columns.iter().map(|c| c.to_sql(driver)).collect();
                if !table.primary_key.is_empty() {
                    let cols: Vec<String> =
                        table.primary_key.iter().map(|c| quote(c, driver)).collect();
                    parts.push(format!("PRIMARY KEY ({})", cols.join(", ")));
                }
                for (column, ref_table, ref_column, on_delete) in &table.foreign_keys {
                    let mut fk = format!(
                        "FOREIGN KEY ({}) REFERENCES {} ({})",
                        quote(column, driver),
                        quote(ref_table, driver),
                        quote(ref_column, driver)
                    );
                    if let Some(action) = on_delete {
                        fk.push_str(&format!(" ON DELETE {action}"));
                    }
                    parts.push(fk);
                }
                let mut statements = vec![format!(
                    "CREATE TABLE {} ({})",
                    quote(&table.name, driver),
                    parts.join(", ")
                )];
                for (columns, unique) in &table.indexes {
                    statements.push(create_index(&table.name, columns, *unique, driver));
                }
                statements
            }
            Schema::Alter { table, changes } => changes
                .iter()
                .map(|change| match change {
                    Change::AddColumn(column) => format!(
                        "ALTER TABLE {} ADD COLUMN {}",
                        quote(table, driver),
                        column.to_sql(driver)
                    ),
                    Change::DropColumn(name) => format!(
                        "ALTER TABLE {} DROP COLUMN {}",
                        quote(table, driver),
                        quote(name, driver)
                    ),
                    Change::RenameColumn { from, to } => format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        quote(table, driver),
                        quote(from, driver),
                        quote(to, driver)
                    ),
                    Change::AddIndex { columns, unique } => {
                        create_index(table, columns, *unique, driver)
                    }
                    Change::DropIndex(name) => match driver {
                        // MySQL scopes index names to the table.
                        Driver::MySql => format!(
                            "DROP INDEX {} ON {}",
                            quote(name, driver),
                            quote(table, driver)
                        ),
                        _ => format!("DROP INDEX IF EXISTS {}", quote(name, driver)),
                    },
                })
                .collect(),
            Schema::Drop { table, if_exists } => {
                let exists = if *if_exists { "IF EXISTS " } else { "" };
                vec![format!("DROP TABLE {exists}{}", quote(table, driver))]
            }
            Schema::Rename { from, to } => vec![format!(
                "ALTER TABLE {} RENAME TO {}",
                quote(from, driver),
                quote(to, driver)
            )],
        }
    }

    /// Execute the statements against a database, in order.
    pub async fn execute(&self, db: &crate::Database) -> crate::Result<()> {
        for statement in self.to_sql(db.driver()) {
            sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                .execute(db.pool())
                .await?;
        }
        Ok(())
    }
}

/// `CREATE [UNIQUE] INDEX <table>_<cols>_index ON <table> (<cols>)`.
fn create_index(table: &str, columns: &[String], unique: bool, driver: Driver) -> String {
    let name = format!("{table}_{}_index", columns.join("_"));
    let cols: Vec<String> = columns.iter().map(|c| quote(c, driver)).collect();
    format!(
        "CREATE {}INDEX {} ON {} ({})",
        if unique { "UNIQUE " } else { "" },
        quote(&name, driver),
        quote(table, driver),
        cols.join(", ")
    )
}

/// Quote an identifier the way `driver` expects. Identifiers are validated first,
/// so a table/column name can never inject SQL.
pub(crate) fn quote(ident: &str, driver: Driver) -> String {
    let safe: String = ident
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    match driver {
        Driver::MySql => format!("`{safe}`"),
        _ => format!("\"{safe}\""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn users() -> Schema {
        Schema::create("users", |t| {
            t.id();
            t.string("email").unique();
            t.string("name").nullable();
            t.integer("age").default_value("0");
            t.boolean("active").default_value("1");
            t.timestamps();
            t.soft_deletes();
            t.index("email");
        })
    }

    #[test]
    fn sqlite_create_table() {
        let sql = users().to_sql(Driver::Sqlite);
        assert_eq!(sql.len(), 2, "table + index");
        assert!(sql[0].contains("\"id\" INTEGER PRIMARY KEY AUTOINCREMENT"));
        assert!(sql[0].contains("\"email\" VARCHAR(255) NOT NULL UNIQUE"));
        assert!(sql[0].contains("\"name\" VARCHAR(255) NULL"));
        assert!(sql[0].contains("\"age\" INTEGER NOT NULL DEFAULT 0"));
        assert!(sql[0].contains("\"active\" INTEGER NOT NULL DEFAULT 1"));
        assert!(sql[0].contains("\"created_at\" BIGINT NOT NULL"));
        assert!(sql[0].contains("\"deleted_at\" BIGINT NULL"));
        assert_eq!(
            sql[1],
            "CREATE INDEX \"users_email_index\" ON \"users\" (\"email\")"
        );
    }

    #[test]
    fn mysql_uses_backticks_and_auto_increment() {
        let sql = users().to_sql(Driver::MySql);
        assert!(sql[0].starts_with("CREATE TABLE `users`"));
        assert!(sql[0].contains("`id` BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY"));
        assert!(sql[1].contains("CREATE INDEX `users_email_index`"));
    }

    #[test]
    fn postgres_uses_bigserial_and_jsonb() {
        let sql = Schema::create("docs", |t| {
            t.id();
            t.json("body");
            t.blob("raw");
            t.float("score");
        })
        .to_sql(Driver::Postgres);
        assert!(sql[0].contains("\"id\" BIGSERIAL PRIMARY KEY"));
        assert!(sql[0].contains("\"body\" JSONB"));
        assert!(sql[0].contains("\"raw\" BYTEA"));
        assert!(sql[0].contains("\"score\" DOUBLE PRECISION"));
    }

    #[test]
    fn foreign_keys_and_composite_primary_keys() {
        let sql = Schema::create("posts", |t| {
            t.id();
            t.foreign_id("user_id", "users").on_delete_cascade();
            t.string("title");
        })
        .to_sql(Driver::Sqlite);
        assert!(sql[0]
            .contains("FOREIGN KEY (\"user_id\") REFERENCES \"users\" (\"id\") ON DELETE CASCADE"));

        let pivot = Schema::create("role_user", |t| {
            t.big_integer("role_id");
            t.big_integer("user_id");
            t.primary(&["role_id", "user_id"]);
        })
        .to_sql(Driver::Sqlite);
        assert!(pivot[0].contains("PRIMARY KEY (\"role_id\", \"user_id\")"));
    }

    #[test]
    fn alter_table_statements() {
        let sql = Schema::table("users", |t| {
            t.add_string("nickname").nullable();
            t.add_integer("logins").default_value("0");
            t.drop_column("age");
            t.rename_column("name", "full_name");
            t.unique_index(&["email"]);
        })
        .to_sql(Driver::Sqlite);

        assert_eq!(sql.len(), 5);
        assert_eq!(
            sql[0],
            "ALTER TABLE \"users\" ADD COLUMN \"nickname\" VARCHAR(255) NULL"
        );
        assert!(sql[1].contains("\"logins\" INTEGER NOT NULL DEFAULT 0"));
        assert_eq!(sql[2], "ALTER TABLE \"users\" DROP COLUMN \"age\"");
        assert_eq!(
            sql[3],
            "ALTER TABLE \"users\" RENAME COLUMN \"name\" TO \"full_name\""
        );
        assert!(sql[4].starts_with("CREATE UNIQUE INDEX"));
    }

    #[test]
    fn drop_and_rename() {
        assert_eq!(
            Schema::drop("users").to_sql(Driver::Sqlite),
            vec!["DROP TABLE \"users\""]
        );
        assert_eq!(
            Schema::drop_if_exists("users").to_sql(Driver::MySql),
            vec!["DROP TABLE IF EXISTS `users`"]
        );
        assert_eq!(
            Schema::rename("users", "people").to_sql(Driver::Sqlite),
            vec!["ALTER TABLE \"users\" RENAME TO \"people\""]
        );
    }

    #[test]
    fn drop_index_differs_on_mysql() {
        let mysql = Schema::table("users", |t| {
            t.drop_index("users_email_index");
        })
        .to_sql(Driver::MySql);
        assert_eq!(mysql[0], "DROP INDEX `users_email_index` ON `users`");

        let sqlite = Schema::table("users", |t| {
            t.drop_index("users_email_index");
        })
        .to_sql(Driver::Sqlite);
        assert_eq!(sqlite[0], "DROP INDEX IF EXISTS \"users_email_index\"");
    }

    #[test]
    fn identifiers_cannot_inject_sql() {
        let sql = Schema::create("users\"; DROP TABLE admins; --", |t| {
            t.string("email\"; --");
        })
        .to_sql(Driver::Sqlite);
        assert!(!sql[0].contains("DROP TABLE admins"));
        assert!(sql[0].starts_with("CREATE TABLE \"usersDROPTABLEadmins\""));
        assert!(sql[0].contains("\"email\" VARCHAR(255)"));
    }
}
