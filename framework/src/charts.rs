//! What a chart draws (RFC 0058): WireCharts' data contract, as a type, so a
//! live command, `rata codegen` and a Laravel endpoint agree on its shape.
//!
//! ```
//! use elyra::charts::ChartData;
//!
//! let revenue = ChartData::new(["Jan", "Feb", "Mar"])
//!     .series("Revenue", [120.0, 180.0, 150.0])
//!     .series("Costs", [90.0, 110.0, 130.0]);
//! assert_eq!(revenue.series.len(), 2);
//! ```
//!
//! ```ignore
//! #[command(live, can = "reports.view")]
//! async fn revenue_by_month(ctx: Ctx) -> Result<ChartData> {
//!     Ok(ChartData::new(["Jan", "Feb"]).series("Revenue", [120.0, 180.0]))
//! }
//! ```
//!
//! `@elyra/charts` draws it: `<ColumnChart live={live.revenue_by_month()} />`.

use serde::{Deserialize, Serialize};

/// A chart's series and the categories along its x axis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ChartData {
    pub series: Vec<Series>,
    /// The x axis: names, or ISO dates (a time axis). Empty for a scatter.
    #[serde(default)]
    pub categories: Vec<String>,
}

/// One series: its name (in the legend, the tooltip and the summary) and
/// its points.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Series {
    /// `null` for a series without one (a pie's single series).
    #[serde(default)]
    pub name: Option<String>,
    pub data: Vec<Point>,
}

/// A point: a value (`null` is a gap), a named value (a pie's slice), or
/// coordinates (`[x, y]`, or `[x, y, size]` for a bubble).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum Point {
    Value(Option<f64>),
    Named { name: String, value: f64 },
    At(Vec<f64>),
}

impl From<f64> for Point {
    fn from(value: f64) -> Self {
        Point::Value(Some(value))
    }
}

impl From<Option<f64>> for Point {
    fn from(value: Option<f64>) -> Self {
        Point::Value(value)
    }
}

impl ChartData {
    /// Data along these categories, with no series yet.
    pub fn new<S: Into<String>>(categories: impl IntoIterator<Item = S>) -> Self {
        Self {
            series: Vec::new(),
            categories: categories.into_iter().map(Into::into).collect(),
        }
    }

    /// Add a series of values, one per category (`None` for a gap).
    pub fn series<P: Into<Point>>(
        mut self,
        name: impl Into<String>,
        data: impl IntoIterator<Item = P>,
    ) -> Self {
        self.series.push(Series {
            name: Some(name.into()),
            data: data.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// A pie's slices: one series of named values.
    pub fn slices<S: Into<String>>(slices: impl IntoIterator<Item = (S, f64)>) -> Self {
        Self {
            series: vec![Series {
                name: None,
                data: slices
                    .into_iter()
                    .map(|(name, value)| Point::Named {
                        name: name.into(),
                        value,
                    })
                    .collect(),
            }],
            categories: Vec::new(),
        }
    }

    /// Add a series of points, `[x, y]` or `[x, y, size]`: a scatter's.
    pub fn points(
        mut self,
        name: impl Into<String>,
        points: impl IntoIterator<Item = Vec<f64>>,
    ) -> Self {
        self.series.push(Series {
            name: Some(name.into()),
            data: points.into_iter().map(Point::At).collect(),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn it_is_wirecharts_contract_on_the_wire() {
        let data = ChartData::new(["Jan", "Feb"]).series("Revenue", [Some(120.0), None]);
        assert_eq!(
            serde_json::to_value(&data).unwrap(),
            json!({ "series": [{ "name": "Revenue", "data": [120.0, null] }], "categories": ["Jan", "Feb"] })
        );
        let pie = ChartData::slices([("Open", 3.0), ("Closed", 1.0)]);
        assert_eq!(
            serde_json::to_value(&pie).unwrap(),
            json!({ "series": [{ "name": null, "data": [{ "name": "Open", "value": 3.0 }, { "name": "Closed", "value": 1.0 }] }], "categories": [] })
        );
    }

    #[test]
    fn what_a_laravel_endpoint_sends_decodes() {
        let sent = json!({
            "series": [
                { "name": "Revenue", "data": [120, 180, null] },
                { "name": "Deals", "data": [[10, 20, 4], [30, 50]] },
                { "data": [{ "name": "Open", "value": 3 }] }
            ],
            "categories": ["Jan", "Feb", "Mar"]
        });
        let data: ChartData = serde_json::from_value(sent).unwrap();
        assert_eq!(data.series[0].data[2], Point::Value(None));
        assert_eq!(data.series[1].data[0], Point::At(vec![10.0, 20.0, 4.0]));
        assert_eq!(
            data.series[2].data[0],
            Point::Named {
                name: "Open".into(),
                value: 3.0
            }
        );
        // Categories are optional, as a scatter has none.
        let bare: ChartData = serde_json::from_value(json!({ "series": [] })).unwrap();
        assert!(bare.categories.is_empty());
    }
}
