<!--
  One page of the reference: its Markdown, with each marked example running
  above its code. Links to other pages stay in the site; links to the rest of
  the docs go to GitHub.
-->
<script>
  import { Marked } from "marked";
  import Example from "./Example.svelte";
  import { highlight } from "./highlight.js";
  import { MARKER } from "./pages.js";

  let { markdown, file, examples } = $props();

  const REPO = "https://github.com/kwhorne/elyra-framework/blob/main/";

  /** Where a link in `file` (relative to docs/) goes from the site. */
  function target(href) {
    if (/^[a-z]+:/i.test(href) || href.startsWith("#")) return href;
    const url = new URL(href, `https://docs.invalid/docs/${file}`);
    const page = url.pathname.match(/^\/docs\/ui\/([\w-]+)\.md$/);
    if (page) return `#${page[1]}`;
    if (url.pathname === "/docs/ui.md") return "#overview";
    return `${REPO}${url.pathname.slice(1)}${url.hash}`;
  }

  const marked = new Marked({
    gfm: true,
    walkTokens(token) {
      if (token.type === "link") token.href = target(token.href);
    },
    renderer: {
      code({ text, lang }) {
        return `<pre class="block" data-lang="${lang ?? ""}"><code>${highlight(text)}</code></pre>`;
      },
      link({ href, tokens }) {
        const text = this.parser.parseInline(tokens);
        const away = /^https?:/.test(href) ? ` target="_blank" rel="noopener"` : "";
        return `<a href="${href}"${away}>${text}</a>`;
      },
    },
  });

  /** The page in parts: Markdown, and the examples between it. */
  const parts = $derived.by(() => {
    const out = [];
    let at = 0;
    for (const match of markdown.matchAll(MARKER)) {
      out.push({ html: marked.parse(markdown.slice(at, match.index)) });
      out.push({ example: match[1], code: match[2] });
      at = match.index + match[0].length;
    }
    out.push({ html: marked.parse(markdown.slice(at)) });
    return out;
  });
</script>

<article class="page">
  {#each parts as part, i (i)}
    {#if part.example}
      <Example name={part.example} component={examples[part.example]} code={part.code} />
    {:else}
      <div class="prose">{@html part.html}</div>
    {/if}
  {/each}
</article>

<style>
  .page { min-width: 0; }
  .prose { min-width: 0; }

  .prose :global(h1) {
    margin: 0 0 10px;
    font-size: 30px; line-height: 1.15; letter-spacing: -0.015em; font-weight: 650;
    text-wrap: balance;
  }
  .prose :global(h1 + p) { font-size: 16px; color: var(--elyra-muted); max-width: 64ch; }
  .prose :global(h2) {
    margin: 44px 0 10px; padding-top: 20px;
    border-top: 1px solid var(--elyra-border);
    font-size: 19px; line-height: 1.3; font-weight: 620; letter-spacing: -0.005em;
    text-wrap: balance;
  }
  .prose :global(h3) { margin: 28px 0 8px; font-size: 15px; font-weight: 620; }
  .prose :global(h3 code) { font-size: 14px; }
  .prose :global(p), .prose :global(li) { max-width: 70ch; line-height: 1.65; }
  .prose :global(ul) { padding-left: 20px; }
  .prose :global(li + li) { margin-top: 4px; }
  .prose :global(a) { color: var(--elyra-accent); text-decoration: none; }
  .prose :global(a:hover) { text-decoration: underline; text-underline-offset: 3px; }
  .prose :global(a:focus-visible) { outline: 2px solid var(--elyra-focus); outline-offset: 2px; border-radius: 3px; }
  .prose :global(:not(pre) > code) {
    font: 0.88em var(--elyra-font-mono);
    padding: 1px 5px; border-radius: 5px;
    background: var(--elyra-inset);
    overflow-wrap: anywhere;
  }
  .prose :global(pre.block) {
    margin: 16px 0;
    padding: 14px 16px;
    overflow-x: auto;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg);
    background: var(--elyra-surface);
    font: 12.5px/1.65 var(--elyra-font-mono);
    tab-size: 2;
  }
  .prose :global(blockquote) {
    margin: 16px 0; padding: 10px 16px;
    border-left: 3px solid var(--elyra-accent);
    background: color-mix(in srgb, var(--elyra-accent) 7%, transparent);
    border-radius: 0 var(--elyra-radius) var(--elyra-radius) 0;
  }
  .prose :global(blockquote p) { margin: 4px 0; }

  /* The API tables, and the keyboard's. */
  .prose :global(table) {
    display: block; overflow-x: auto;
    width: 100%; margin: 12px 0 20px;
    border-collapse: collapse;
    font-size: 13px;
  }
  .prose :global(th) {
    text-align: left; font-weight: 600; font-size: 11.5px;
    letter-spacing: 0.04em; text-transform: uppercase;
    color: var(--elyra-muted);
    padding: 8px 14px 8px 0; border-bottom: 1px solid var(--elyra-border);
  }
  .prose :global(td) {
    vertical-align: top; line-height: 1.55;
    padding: 9px 14px 9px 0; border-bottom: 1px solid color-mix(in srgb, var(--elyra-border) 60%, transparent);
  }
  .prose :global(td:first-child) { white-space: nowrap; }
  .prose :global(td:first-child code) { color: var(--elyra-accent); background: var(--elyra-accent-soft); }
  .prose :global(td:nth-child(2) code), .prose :global(td:nth-child(3) code) { white-space: nowrap; }
  .prose :global(kbd) {
    font: 12px var(--elyra-font-mono);
    padding: 1px 6px; border-radius: 5px;
    border: 1px solid var(--elyra-border); border-bottom-width: 2px;
    background: var(--elyra-raised);
  }

  /* The highlighter's colours. */
  .page :global(.hl-comment) { color: var(--elyra-muted); font-style: italic; }
  .page :global(.hl-string) { color: var(--elyra-success); }
  .page :global(.hl-tag) { color: var(--elyra-danger); }
  .page :global(.hl-attr) { color: var(--elyra-warning); }
  .page :global(.hl-keyword), .page :global(.hl-block) { color: var(--hl-keyword); }
  .page :global(.hl-rune) { color: var(--hl-rune); }
  .page :global(.hl-literal) { color: var(--hl-literal); }
  .page :global(.hl-prompt) { color: var(--elyra-muted); user-select: none; }
</style>
