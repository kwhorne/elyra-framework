<!--
  Every chart in @elyra/charts (RFC 0059), drawn from WireCharts' docs
  examples, the fixtures the tests hold the charts to: by WireCharts'
  families, each with its blurb and its code, in dark or light, English or
  Norwegian.
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
