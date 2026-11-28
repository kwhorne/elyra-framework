//! [`Page`]: one page of results and the numbers a UI needs to page through
//! them — the shape of Laravel's length-aware paginator, so the same type is
//! what Elyra's `Query::paginate` returns and what a Laravel API answers.

/// One page of results plus the paging metadata a UI needs.
//
// With the `serde` feature it serializes flat, as Laravel's `paginate()` does,
// and deserializes from either of Laravel's shapes: `paginate()`'s flat one, or
// an API Resource collection's `{ data, links, meta }`.
// `PartialEq` only: the totals are `f64` (RFC 0071).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Page<M> {
    /// The rows on this page.
    pub data: Vec<M>,
    /// Total matching rows (ignoring limit/offset).
    pub total: i64,
    pub per_page: i64,
    pub current_page: i64,
    pub last_page: i64,
    /// The list's totals by column, over every matching row (RFC 0071): what
    /// a Laravel list's `elyraList` answers with `totals`, and a `Table`'s
    /// foot shows. `None` (`null` in the JSON) when there are none.
    pub totals: Option<std::collections::BTreeMap<String, f64>>,
}

impl<M> Page<M> {
    /// A page without totals.
    pub fn new(data: Vec<M>, total: i64, per_page: i64, current_page: i64, last_page: i64) -> Self {
        Page {
            data,
            total,
            per_page,
            current_page,
            last_page,
            totals: None,
        }
    }

    /// Whether another page follows.
    pub fn has_more(&self) -> bool {
        self.current_page < self.last_page
    }

    /// 1-based index of the first row on this page (0 when empty).
    pub fn from(&self) -> i64 {
        if self.data.is_empty() {
            0
        } else {
            (self.current_page - 1) * self.per_page + 1
        }
    }

    /// 1-based index of the last row on this page (0 when empty).
    pub fn to(&self) -> i64 {
        if self.data.is_empty() {
            0
        } else {
            self.from() + self.data.len() as i64 - 1
        }
    }
}

#[cfg(feature = "serde")]
mod de {
    use super::Page;

    #[derive(serde::Deserialize)]
    struct Numbers {
        total: i64,
        per_page: i64,
        current_page: i64,
        last_page: i64,
        #[serde(default)]
        totals: Option<std::collections::BTreeMap<String, f64>>,
    }

    type Totals = Option<std::collections::BTreeMap<String, f64>>;

    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Shape<M> {
        /// An API Resource collection: the numbers under `meta`, the totals
        /// beside `data` (`additional`) or in `meta`.
        Resource {
            data: Vec<M>,
            meta: Numbers,
            #[serde(default)]
            totals: Totals,
        },
        /// `paginate()`: the numbers beside `data`.
        Flat {
            data: Vec<M>,
            #[serde(flatten)]
            numbers: Numbers,
            #[serde(default)]
            totals: Totals,
        },
    }

    impl<'de, M: serde::Deserialize<'de>> serde::Deserialize<'de> for Page<M> {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let (data, n, totals) = match Shape::deserialize(deserializer)? {
                Shape::Resource { data, meta, totals } => (data, meta, totals),
                Shape::Flat {
                    data,
                    numbers,
                    totals,
                } => (data, numbers, totals),
            };
            Ok(Page {
                data,
                total: n.total,
                per_page: n.per_page,
                current_page: n.current_page,
                last_page: n.last_page,
                totals: totals.or(n.totals),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_range_on_a_page() {
        let page = Page::new(vec![1, 2, 3], 23, 10, 3, 3);
        assert_eq!((page.from(), page.to(), page.has_more()), (21, 23, false));
        let empty: Page<i32> = Page::new(vec![], 0, 10, 1, 1);
        assert_eq!((empty.from(), empty.to()), (0, 0));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn totals_from_either_of_laravels_shapes() {
        let flat: Page<i32> = serde_json::from_str(
            r#"{"data":[1],"total":40,"per_page":25,"current_page":1,"last_page":2,"totals":{"amount":128400.5}}"#,
        )
        .unwrap();
        assert_eq!(flat.totals.unwrap()["amount"], 128400.5);
        let resource: Page<i32> = serde_json::from_str(
            r#"{"data":[1],"meta":{"total":40,"per_page":25,"current_page":1,"last_page":2},"totals":{"qty":3}}"#,
        )
        .unwrap();
        assert_eq!(resource.totals.unwrap()["qty"], 3.0);
        let none: Page<i32> = serde_json::from_str(
            r#"{"data":[],"total":0,"per_page":25,"current_page":1,"last_page":1}"#,
        )
        .unwrap();
        assert!(none.totals.is_none());
        // Without totals, `null`: what a `Table` takes as none.
        assert!(serde_json::to_string(&none)
            .unwrap()
            .contains(r#""totals":null"#));
    }
}
