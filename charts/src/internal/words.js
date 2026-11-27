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
    // WireCharts' families (RFC 0059).
    name: "Name",
    valueHead: "Value",
    value: [":name: :value of :max", "name", "value", "max"],
    flow: [":from to :to: :value", "from", "to", "value"],
    source: "From",
    target: "To",
    link: [":from to :to", "from", "to"],
    items: ["{1} One item|[2,*] :count items", "count"],
    levels: ["{1} on one level|[2,*] over :count levels", "count"],
    highest: ["Highest at :at: :value", "at", "value"],
    open: "Open",
    close: "Close",
    low: "Low",
    high: "High",
    min: "Minimum",
    q1: "Lower quartile",
    median: "Median",
    q3: "Upper quartile",
    max: "Maximum",
    play: "Play the chart as sound",
    stop: "Stop",
    replay: "Replay",
    noGl: "3D isn't available here",
  },
};
