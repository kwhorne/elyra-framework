//! Casts — how a model field is stored when its Rust type isn't a column type
//! the driver reads natively. Laravel's `$casts`.
//!
//! The `Any` driver behind every model only decodes SQL scalars (integers,
//! floats, text, blobs), so a `Vec<String>`, a `serde_json::Value` or an enum
//! can't be a plain field. A cast bridges it:
//!
//! ```ignore
//! #[derive(Model)]
//! struct Post {
//!     id: i64,
//!     #[model(cast = "json")] tags: Vec<String>,          // TEXT column, JSON
//!     #[model(cast = "text")] status: Status,             // TEXT column, Display/FromStr
//!     #[model(cast = Cents)]  price: Money,               // your own `impl Cast<Money>`
//! }
//! ```
//!
//! `"json"` and `"text"` are shorthands for [`Json`] and [`Text`]; any other
//! value is a path to a type implementing [`Cast`] for the field's type.

use std::fmt::Display;
use std::str::FromStr;

use sqlx::any::AnyRow;
use sqlx::Row;

use crate::error::{Error, Result};
use crate::model::Value;

/// Converts a field of type `T` to and from its column.
///
/// Implement it on a marker type to add your own cast:
///
/// ```ignore
/// struct Cents;
/// impl Cast<Money> for Cents {
///     fn encode(value: &Money) -> Result<Value> { Ok(Value::Int(value.cents())) }
///     fn decode(row: &AnyRow, column: &str) -> Result<Money> {
///         Ok(Money::from_cents(row.try_get::<i64, _>(column)?))
///     }
/// }
/// ```
pub trait Cast<T> {
    /// The column value to write for `value`.
    fn encode(value: &T) -> Result<Value>;

    /// Read the field back out of `column`.
    fn decode(row: &AnyRow, column: &str) -> Result<T>;
}

/// Store any serde type as JSON text. Use a `TEXT` column.
///
/// `NULL` round-trips as `None` for an `Option<T>` field (and is an error for a
/// non-optional one). On Postgres, don't use the schema builder's `json()`
/// column: it creates `JSONB`, which the `Any` driver can't read — use `text()`.
pub struct Json;

impl<T> Cast<T> for Json
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn encode(value: &T) -> Result<Value> {
        let json = serde_json::to_string(value)
            .map_err(|e| Error::Query(format!("json cast: cannot encode: {e}")))?;
        // `None` serializes to `null`; store a real NULL rather than the text.
        Ok(if json == "null" {
            Value::Null
        } else {
            Value::Text(json)
        })
    }

    fn decode(row: &AnyRow, column: &str) -> Result<T> {
        let raw: Option<String> = row.try_get(column)?;
        let text = raw.as_deref().unwrap_or("null");
        serde_json::from_str(text)
            .map_err(|e| Error::Query(format!("json cast on `{column}`: {e}")))
    }
}

/// Store a type through its string form: [`Display`] to write, [`FromStr`] to
/// read. Suits enums with a fixed spelling, identifiers, and anything with a
/// canonical text representation. Use a `TEXT` column.
pub struct Text;

impl<T> Cast<T> for Text
where
    T: Display + FromStr,
    T::Err: Display,
{
    fn encode(value: &T) -> Result<Value> {
        Ok(Value::Text(value.to_string()))
    }

    fn decode(row: &AnyRow, column: &str) -> Result<T> {
        let raw: String = row.try_get(column)?;
        raw.parse().map_err(|e| {
            Error::Query(format!(
                "text cast on `{column}`: cannot parse {raw:?}: {e}"
            ))
        })
    }
}
