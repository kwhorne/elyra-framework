//! [`Page`]: one page of results and the numbers a UI needs to page through
//! them — the shape of Laravel's length-aware paginator, so the same type is
//! what Elyra's `Query::paginate` returns and what a Laravel API answers.

/// One page of results plus the paging metadata a UI needs.
//
// With the `serde` feature it serializes flat, as Laravel's `paginate()` does,
// and deserializes from either of Laravel's shapes: `paginate()`'s flat one, or
// an API Resource collection's `{ data, links, meta }`.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

impl<M> Page<M> {
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
    }

    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Shape<M> {
        /// An API Resource collection: the numbers under `meta`.
        Resource { data: Vec<M>, meta: Numbers },
        /// `paginate()`: the numbers beside `data`.
        Flat {
            data: Vec<M>,
            #[serde(flatten)]
            numbers: Numbers,
        },
    }

    impl<'de, M: serde::Deserialize<'de>> serde::Deserialize<'de> for Page<M> {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let (data, n) = match Shape::deserialize(deserializer)? {
                Shape::Resource { data, meta } => (data, meta),
                Shape::Flat { data, numbers } => (data, numbers),
            };
            Ok(Page {
                data,
                total: n.total,
                per_page: n.per_page,
                current_page: n.current_page,
                last_page: n.last_page,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_range_on_a_page() {
        let page = Page {
            data: vec![1, 2, 3],
            total: 23,
            per_page: 10,
            current_page: 3,
            last_page: 3,
        };
        assert_eq!((page.from(), page.to(), page.has_more()), (21, 23, false));
        let empty: Page<i32> = Page {
            data: vec![],
            total: 0,
            per_page: 10,
            current_page: 1,
            last_page: 1,
        };
        assert_eq!((empty.from(), empty.to()), (0, 0));
    }
}
