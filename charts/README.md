# @elyra/charts

Charts for [Elyra Framework](https://github.com/kwhorne/elyra-framework) apps,
in Svelte 5, on [ECharts](https://echarts.apache.org)
([RFC 0058](https://github.com/kwhorne/elyra-framework/blob/main/docs/proposals/0058-elyra-charts.md)).

```svelte
<script>
  import { LineChart, PieChart } from "@elyra/charts";
  import { live } from "./bindings";
</script>

<LineChart series={[{ name: "Revenue", data: [120, 180, 150, 260] }]} categories={["Jan", "Feb", "Mar", "Apr"]} smooth />
<PieChart live={live.cases_by_status()} donut />
```

- WireCharts' data: `series` (`[{ name, data }]`) and `categories`.
- `LineChart`, `AreaChart`, `ColumnChart`, `BarChart`, `PieChart`,
  `ScatterChart`, and `Chart` for any ECharts option. Each imports only its
  own part of ECharts.
- The design tokens (`--elyra-chart-1` … `-8` for the series), in every
  theme; live queries with their states; a summary and the data as a table
  for the screen reader and the assistant; the app's language.

It's attached to each release as `elyra-charts-<version>.tgz`, beside
`@elyra/runtime` and `@elyra/ui`, which it needs at the same version. See
[the reference](https://github.com/kwhorne/elyra-framework/blob/main/docs/charts.md).
