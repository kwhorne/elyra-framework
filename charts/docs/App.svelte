<!--
  Every chart in @elyra/charts (RFC 0059), drawn from WireCharts' docs
  examples, the fixtures the tests hold the charts to: by WireCharts'
  families, each with its blurb and its code, in dark or light, English or
  Norwegian. First, a year of days with a window on it (RFC 0061).
-->
<script>
  import * as charts from "../src/index.js";
  import * as gl from "../src/gl.js";
  import fixtures from "../src/test/wirecharts/fixtures.json";
  import { exampleOf, nameOf, propsOf } from "./examples.js";
  import { speak } from "./runtime.js";

  const slug = (title) => title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
  const families = [...new Set(fixtures.map((f) => f.family))].map((title) => ({
    title,
    id: slug(title),
    charts: fixtures
      .filter((f) => f.family === title)
      .map((f) => {
        const name = nameOf(f.component);
        const props = propsOf(f);
        // WireCharts' surface takes `series ?? data`, and `series` defaults to []:
        // its docs' example passes `data`, which it never draws.
        if (f.component === "surface") props.series = props.data;
        return {
          name,
          blurb: f.blurb,
          from: gl[name] ? "@elyra/charts/gl" : "@elyra/charts",
          Component: charts[name] ?? gl[name],
          props,
          code: exampleOf(f),
        };
      }),
  }));

  // A year of days, for the zoom: a window the page reads and sets.
  const days = Array.from({ length: 365 }, (_, i) => new Date(Date.UTC(2026, 0, 1 + i)).toISOString().slice(0, 10));
  const visits = [{ name: "Visits", data: days.map((_, i) => Math.round(400 + 180 * Math.sin(i / 29) + 60 * Math.sin(i / 3.1) + i * 0.8)) }];
  let range = $state(["2026-03-01", "2026-03-31"]);
  const names = Array.from({ length: 30 }, (_, i) => `P${String(i + 1).padStart(2, "0")}`);
  const products = [{ name: "Sold", data: names.map((_, i) => 20 + ((i * 37) % 90)) }];

  // Linked (RFC 0063): orders beside the visits, and customers to select.
  const orders = [{ name: "Orders", data: visits[0].data.map((v, i) => Math.round(v / 9 + 12 * Math.sin(i / 5))) }];
  const customers = [
    {
      name: "Customers",
      data: Array.from({ length: 80 }, (_, i) => [Math.round(5 + ((i * 53) % 95)), Math.round(2 + ((i * 29) % 40) + ((i * 53) % 95) / 4)]),
    },
  ];
  let chosen = $state(null);

  let theme = $state("dark");
  let language = $state("en");
  $effect(() => {
    document.documentElement.dataset.theme = theme;
  });
  $effect(() => speak(language));
</script>

<div class="gallery">
  <nav aria-label="Families">
    <strong>@elyra/charts</strong>
    <p>{fixtures.length} charts</p>
    <ul>
      <li><a href="#zoom">Zoom <span>2</span></a></li>
      <li><a href="#linked">Linked <span>3</span></a></li>
      {#each families as family (family.id)}
        <li><a href="#{family.id}">{family.title} <span>{family.charts.length}</span></a></li>
      {/each}
    </ul>
    <div class="switches">
      <button type="button" onclick={() => (theme = theme === "dark" ? "light" : "dark")}>
        {theme === "dark" ? "Light" : "Dark"}
      </button>
      <button type="button" onclick={() => (language = language === "en" ? "nb" : "en")}>
        {language === "en" ? "Norsk" : "English"}
      </button>
    </div>
  </nav>

  <main>
    <section id="zoom" aria-labelledby="zoom-title">
      <h2 id="zoom-title">Zoom</h2>
      <div class="cards">
      <article>
        <h3>LineChart zoom bind:range</h3>
        <p>
          The wheel or a pinch zooms, a drag pans, the slider shows the window. On the chart: + and − zoom,
          Shift+←/→ pan, 0 shows all; the slider's handles take ←/→.
        </p>
        <charts.LineChart series={visits} categories={days} zoom bind:range height={280} />
        <div class="window">
          <output>{range ? `${range[0]} – ${range[1]}` : "All of it"}</output>
          <button type="button" onclick={() => (range = ["2026-03-01", "2026-03-31"])}>March</button>
          <button type="button" onclick={() => (range = ["2026-10-01", "2026-12-31"])}>Q4</button>
          <button type="button" onclick={() => (range = null)}>All</button>
        </div>
      </article>
      <article>
        <h3>BarChart zoom</h3>
        <p>Bars across: the slider stands up beside them.</p>
        <charts.BarChart series={products} categories={names} zoom range={["P05", "P12"]} height={280} />
      </article>
      </div>
    </section>

    <section id="linked" aria-labelledby="linked-title">
      <h2 id="linked-title">Linked</h2>
      <div class="cards">
        <article>
          <h3>LineChart and ColumnChart link="days"</h3>
          <p>Zoom one, the other follows; point at a day in one, the other shows it too.</p>
          <charts.LineChart series={visits} categories={days} zoom link="days" height={200} />
          <charts.ColumnChart series={orders} categories={days} zoom link="days" height={200} />
        </article>
        <article>
          <h3>ScatterChart brush bind:selection</h3>
          <p>Drag a rectangle (Shift for a lasso), or on the chart Space takes the focused point in.</p>
          <charts.ScatterChart series={customers} brush bind:selection={chosen} height={280} />
          <div class="window">
            <output>{chosen?.points ? `${chosen.points.length} selected` : "Nothing selected"}</output>
            <button type="button" onclick={() => (chosen = null)}>Clear</button>
          </div>
        </article>
      </div>
    </section>

    {#each families as family (family.id)}
      <section id={family.id} aria-labelledby="{family.id}-title">
        <h2 id="{family.id}-title">{family.title}</h2>
        <div class="cards">
          {#each family.charts as chart (chart.name)}
            <article>
              <h3>{chart.name}</h3>
              <p>{chart.blurb}</p>
              <chart.Component {...chart.props} height={240} />
              <details>
                <summary>Code</summary>
                <pre><code>{`<script>\n  import { ${chart.name} } from "${chart.from}";\n</script>\n\n${chart.code}`}</code></pre>
              </details>
            </article>
          {/each}
        </div>
      </section>
    {/each}
  </main>
</div>

<style>
  :global(body) { margin: 0; background: var(--elyra-bg); color: var(--elyra-text); font: var(--elyra-font-size) var(--elyra-font); }
  .gallery { display: grid; grid-template-columns: 220px 1fr; min-height: 100vh; }
  nav {
    position: sticky; top: 0; align-self: start; height: 100vh; box-sizing: border-box; padding: 20px 16px;
    background: var(--elyra-surface); border-right: 1px solid var(--elyra-border); overflow: auto;
  }
  nav p { margin: 2px 0 16px; color: var(--elyra-muted); font-size: 12px; }
  nav ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  nav a {
    display: flex; justify-content: space-between; padding: 6px 8px; border-radius: var(--elyra-radius);
    color: var(--elyra-text); text-decoration: none;
  }
  nav a:hover { background: var(--elyra-inset); }
  nav a span { color: var(--elyra-muted); }
  .window { display: flex; align-items: center; gap: 8px; margin-top: 10px; }
  .window output { flex: 1; color: var(--elyra-muted); font-variant-numeric: tabular-nums; }
  .window button, .switches button {
    padding: 6px 10px; border-radius: var(--elyra-radius); border: 1px solid var(--elyra-border);
    background: var(--elyra-raised); color: var(--elyra-text); font: inherit; cursor: pointer;
  }
  .switches { display: flex; gap: 8px; margin-top: 16px; }
  .switches button {
    flex: 1; padding: 6px; border-radius: var(--elyra-radius); border: 1px solid var(--elyra-border);
    background: var(--elyra-raised); color: var(--elyra-text); font: inherit; cursor: pointer;
  }
  main { padding: 24px 28px 64px; }
  h2 { margin: 28px 0 12px; font-size: 18px; }
  section:first-child h2 { margin-top: 0; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 14px; }
  article {
    background: var(--elyra-raised); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg);
    padding: 14px; min-width: 0;
  }
  h3 { margin: 0 0 4px; font-size: 14px; }
  article p { margin: 0 0 10px; color: var(--elyra-muted); font-size: 12px; line-height: 1.4; }
  details { margin-top: 8px; }
  summary { cursor: pointer; color: var(--elyra-muted); font-size: 12px; }
  pre {
    margin: 8px 0 0; padding: 10px; overflow: auto; border-radius: var(--elyra-radius);
    background: var(--elyra-inset); font: 12px/1.5 var(--elyra-font-mono);
  }
  a:focus-visible, button:focus-visible, summary:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
</style>
