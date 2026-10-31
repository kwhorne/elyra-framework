<!--
  The reference: the pages down the side, one page at a time (`#button`), in
  the theme the reader chose. Built on the tokens it documents.
-->
<script>
  import { onMount, tick } from "svelte";
  import { route } from "@elyra/runtime";
  import pkg from "../package.json";
  import Page from "./Page.svelte";
  import { GROUPS, PAGES, fileOf } from "./pages.js";

  const sources = import.meta.glob(["../../docs/ui.md", "../../docs/ui/*.md"], {
    query: "?raw",
    import: "default",
    eager: true,
  });
  const markdownOf = (page) => sources[`../../docs/${fileOf(page)}`] ?? `# ${page.title}\n\nMissing: docs/${fileOf(page)}.`;

  const modules = import.meta.glob("./examples/**/*.svelte", { import: "default", eager: true });
  const examples = Object.fromEntries(
    Object.entries(modules).map(([path, component]) => [path.slice("./examples/".length, -".svelte".length), component]),
  );

  const slugFrom = (hash) => {
    const slug = hash.replace(/^#/, "");
    return PAGES.some((p) => p.slug === slug) ? slug : null;
  };

  let slug = $state(slugFrom(location.hash) ?? "overview");
  let menuOpen = $state(false);
  let main = $state();

  const page = $derived(PAGES.find((p) => p.slug === slug));
  const index = $derived(PAGES.indexOf(page));
  const previous = $derived(PAGES[index - 1]);
  const next = $derived(PAGES[index + 1]);

  // The theme: what the host or the reader set, else the system's.
  const stored = () => {
    try {
      return localStorage.getItem("elyra-ui-theme");
    } catch {
      return null;
    }
  };
  const root = document.documentElement;
  let theme = $state(
    root.dataset.theme ?? stored() ?? (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark"),
  );
  $effect(() => {
    root.dataset.theme = theme;
  });

  function setTheme(to) {
    theme = to;
    try {
      localStorage.setItem("elyra-ui-theme", to);
    } catch {}
  }

  onMount(() => {
    const onHash = async () => {
      // `#!/customers` is a link inside an example (runtime.js): it moves the
      // example's route, not the page.
      if (location.hash.startsWith("#!")) return;
      const to = slugFrom(location.hash);
      if (!to || to === slug) return;
      slug = to;
      menuOpen = false;
      await tick();
      main?.scrollTo?.(0, 0);
      window.scrollTo(0, 0);
    };
    addEventListener("hashchange", onHash);

    const follow = matchMedia("(prefers-color-scheme: light)");
    const onScheme = (event) => stored() || (theme = event.matches ? "light" : "dark");
    follow.addEventListener?.("change", onScheme);

    return () => {
      removeEventListener("hashchange", onHash);
      follow.removeEventListener?.("change", onScheme);
    };
  });

  // A link into the app, in an example, goes to the example's route.
  function onClick(event) {
    const link = event.target.closest?.("a[href^='#!']");
    if (!link) return;
    event.preventDefault();
    route.set({ path: link.getAttribute("href").slice(2), params: {}, query: {} });
  }

  $effect(() => {
    document.title = page && page.slug !== "overview" ? `${page.title} · Elyra UI` : "Elyra UI";
  });
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && menuOpen && (menuOpen = false)} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="site" onclick={onClick}>
  <header class="top">
    <button type="button" class="menu" aria-expanded={menuOpen} aria-controls="pages" onclick={() => (menuOpen = !menuOpen)}>
      <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
      <span>{page?.title}</span>
    </button>
  </header>

  <aside id="pages" class="side" class:open={menuOpen}>
    <a class="brand" href="#overview">
      <span class="mark" aria-hidden="true"></span>
      <span class="name">Elyra <b>UI</b></span>
      <span class="version">{pkg.version}</span>
    </a>

    <nav aria-label="The reference">
      {#each GROUPS as group (group.title)}
        <p class="group">{group.title}</p>
        <ul>
          {#each group.pages as item (item.slug)}
            <li>
              <a href="#{item.slug}" aria-current={item.slug === slug ? "page" : undefined}>{item.title}</a>
            </li>
          {/each}
        </ul>
      {/each}
    </nav>

    <div class="theme" role="group" aria-label="Theme">
      <button type="button" aria-pressed={theme === "dark"} onclick={() => setTheme("dark")}>Dark</button>
      <button type="button" aria-pressed={theme === "light"} onclick={() => setTheme("light")}>Light</button>
    </div>
  </aside>

  <main bind:this={main} class="main">
    <p class="crumb">{page?.group}</p>
    {#key slug}
      <Page markdown={markdownOf(page)} file={fileOf(page)} {examples} />
    {/key}

    <nav class="pager" aria-label="Previous and next">
      {#if previous}<a class="prev" href="#{previous.slug}"><small>Previous</small>{previous.title}</a>{:else}<span></span>{/if}
      {#if next}<a class="next" href="#{next.slug}"><small>Next</small>{next.title}</a>{/if}
    </nav>
    <p class="foot">
      @elyra/ui {pkg.version} · MIT · The pages are <code>docs/ui/</code> in
      <a href="https://github.com/kwhorne/elyra-framework/tree/main/docs/ui" target="_blank" rel="noopener">the repository</a>.
      In these docs the user may do everything but <code>customers.delete</code>.
    </p>
  </main>
</div>

<style>
  :global(:root) {
    --hl-keyword: #bb9af7; --hl-rune: #7dcfff; --hl-literal: #ff9e64;
    --side-width: 248px;
  }
  :global(:root[data-theme="light"]) {
    --hl-keyword: #7847bd; --hl-rune: #166775; --hl-literal: #965027;
  }
  :global(:root[data-theme="dark"]) { color-scheme: dark; }
  :global(:root[data-theme="light"]) { color-scheme: light; }
  :global(body) {
    margin: 0;
    background: var(--elyra-bg); color: var(--elyra-text);
    font: var(--elyra-font-size)/1.55 var(--elyra-font);
    -webkit-font-smoothing: antialiased;
  }
  :global(body::before) {
    content: ""; position: fixed; inset: 0 0 auto; z-index: 10;
    height: env(safe-area-inset-top, 0px); background: var(--elyra-surface);
  }

  .site { min-height: 100vh; }

  .side {
    position: fixed; top: 0; bottom: 0; left: 0; z-index: 20;
    width: var(--side-width); box-sizing: border-box;
    display: flex; flex-direction: column;
    padding-block: calc(18px + env(safe-area-inset-top, 0px)) 16px;
    padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: 16px;
    background: var(--elyra-surface);
    border-right: 1px solid var(--elyra-border);
    overflow-y: auto;
  }
  .brand {
    display: flex; align-items: center; gap: 9px;
    padding: 2px 8px 18px; color: var(--elyra-text); text-decoration: none;
  }
  .mark {
    width: 18px; height: 18px; border-radius: 5px;
    background: linear-gradient(135deg, var(--elyra-accent), color-mix(in srgb, var(--elyra-accent) 40%, var(--elyra-success)));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--elyra-on-accent) 18%, transparent);
  }
  .name { font-size: 15px; font-weight: 500; letter-spacing: -0.01em; }
  .name b { font-weight: 700; }
  .version {
    margin-left: auto; font: 11px var(--elyra-font-mono); color: var(--elyra-muted);
    padding: 1px 6px; border: 1px solid var(--elyra-border); border-radius: 999px;
  }
  .group {
    margin: 18px 8px 6px;
    font-size: 11px; font-weight: 600; letter-spacing: 0.07em; text-transform: uppercase;
    color: var(--elyra-muted);
  }
  nav ul { list-style: none; margin: 0; padding: 0; }
  nav li a {
    display: block; padding: 5px 8px; border-radius: 6px;
    color: var(--elyra-text); text-decoration: none; font-size: 13.5px;
  }
  nav li a:hover { background: var(--elyra-inset); }
  nav li a[aria-current="page"] {
    background: var(--elyra-accent-soft); color: var(--elyra-accent); font-weight: 550;
  }
  nav li a:focus-visible, .brand:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; }

  .theme {
    display: flex; gap: 2px; margin-top: auto; padding: 3px;
    border: 1px solid var(--elyra-border); border-radius: 8px; background: var(--elyra-bg);
  }
  .theme button {
    flex: 1; font: inherit; font-size: 12.5px; padding: 5px 0; cursor: pointer;
    border: 0; border-radius: 6px; background: none; color: var(--elyra-muted);
  }
  .theme button[aria-pressed="true"] { background: var(--elyra-raised); color: var(--elyra-text); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.18); }
  .theme button:focus-visible { outline: 2px solid var(--elyra-focus); }

  .main {
    box-sizing: border-box;
    margin-left: var(--side-width);
    max-width: calc(var(--side-width) + 900px);
    padding-block: 44px 64px;
    padding-left: max(40px, env(safe-area-inset-left, 0px));
    padding-right: max(40px, env(safe-area-inset-right, 0px));
  }
  .crumb {
    margin: 0 0 8px;
    font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase;
    color: var(--elyra-accent);
  }

  .pager { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-top: 56px; }
  .pager a {
    display: flex; flex-direction: column; gap: 2px;
    padding: 12px 16px; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg);
    color: var(--elyra-text); text-decoration: none; font-weight: 550;
  }
  .pager a:hover { border-color: var(--elyra-accent); }
  .pager a:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .pager small { font-weight: 400; font-size: 12px; color: var(--elyra-muted); }
  .pager .next { text-align: right; grid-column: 2; }
  .foot { margin-top: 28px; font-size: 12.5px; color: var(--elyra-muted); }
  .foot a { color: inherit; }
  .foot code { font: 0.92em var(--elyra-font-mono); }

  .top { display: none; }

  @media (max-width: 860px) {
    .top {
      display: flex; position: sticky; top: env(safe-area-inset-top, 0px); z-index: 15;
      padding-block: 8px;
      padding-left: max(16px, env(safe-area-inset-left, 0px));
      padding-right: max(16px, env(safe-area-inset-right, 0px));
      background: var(--elyra-surface); border-bottom: 1px solid var(--elyra-border);
    }
    .menu {
      display: flex; align-items: center; gap: 8px;
      font: inherit; font-weight: 550; color: var(--elyra-text);
      background: none; border: 0; padding: 6px 4px; cursor: pointer;
    }
    .menu:focus-visible { outline: 2px solid var(--elyra-focus); border-radius: 6px; }
    .side { transform: translateX(-100%); transition: transform 0.18s ease; box-shadow: var(--elyra-shadow); }
    .side.open { transform: none; }
    .main {
      margin-left: 0;
      padding-block: 28px 48px;
      padding-left: max(16px, env(safe-area-inset-left, 0px));
      padding-right: max(16px, env(safe-area-inset-right, 0px));
    }
  }
</style>
