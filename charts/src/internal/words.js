// What @elyra/charts says (RFC 0056 and 0058), in English. Each is also
// `elyra.charts.<word>` in `framework/lang/en.json`, translated by the app's
// catalog, and a chart's `labels` win over both.

export const WORDS = {
  charts: {
    empty: "No data",
    failed: "Couldn't load this chart",
    caption: "The chart's data",
    category: "Category",
    series: [
      "{1} :name: :first|[2,*] :name: from :first to :last, between :min and :max, over :count points",
      "name",
      "first",
      "last",
      "min",
      "max",
      "count",
    ],
    slice: [":name :share", "name", "share"],
  },
};
