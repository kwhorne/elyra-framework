//! What a list's filters send (RFC 0069, RFC 0071): a range of numbers and a
//! span of days, the same over the app's own database (`elyra::db::filter`'s
//! `Query` helpers) and over a Laravel backend (sent as `filters[amount][min]`,
//! which the package's `elyraList` reads).

/// A number from `min` to `max`, each end optional and inclusive.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Range<T> {
    #[cfg_attr(feature = "serde", serde(default))]
    pub min: Option<T>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub max: Option<T>,
}

impl<T> Default for Range<T> {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
        }
    }
}

/// Days from `from` to `to`, each an ISO day (`2026-11-30`), optional and
/// inclusive: `to` takes in the whole of its day, a time on it too.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Dates {
    #[cfg_attr(feature = "serde", serde(default))]
    pub from: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub to: Option<String>,
}
