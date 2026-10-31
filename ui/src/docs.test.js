// @vitest-environment node
//
// The reference (docs/ui/*.md, run by ui/docs/) says what each component
// takes; these hold it to the components themselves, so it can't drift.

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { compile, parse } from "svelte/compiler";
import { PAGES, fileOf } from "../docs/pages.js";
import { examplePath, examplesIn, pagePath, sync } from "../docs/sync.js";

const src = (file) => readFileSync(fileURLToPath(new URL(file, import.meta.url)), "utf8");
const read = (path) => readFileSync(path, "utf8");

/** The components the package exports, from index.js. */
const EXPORTED = [...src("./index.js").matchAll(/export \{ default as (\w+) \}/g)].map((m) => m[1]);

/** A component's props, from its `$props()`: name → { default, bindable }, and whether it has a rest. */
function propsOf(source) {
  const ast = parse(source, { modern: true });
  const props = new Map();
  let rest = false;
  const walk = (node) => {
    if (!node || typeof node !== "object") return;
    if (node.type === "VariableDeclarator" && node.init?.callee?.name === "$props") {
      for (const p of node.id.properties) {
        if (p.type === "RestElement") {
          rest = true;
          continue;
        }
        let value = p.value.type === "AssignmentPattern" ? source.slice(p.value.right.start, p.value.right.end) : null;
        const bindable = value?.startsWith("$bindable(") ?? false;
        if (bindable) value = value.slice("$bindable(".length, -1) || null;
        if (value === "undefined") value = null;
        props.set(p.key.name, { default: value, bindable });
      }
    }
    for (const key in node) {
      const child = node[key];
      if (Array.isArray(child)) child.forEach(walk);
      else if (child && typeof child === "object" && key !== "parent") walk(child);
    }
  };
  walk(ast.instance?.content);
  return { props, rest };
}

/** Split a Markdown table row on the pipes that aren't escaped or in code. */
function cells(row) {
  const out = [];
  let cell = "";
  let ticks = 0;
  for (let i = 0; i < row.length; i++) {
    const ch = row[i];
    if (ch === "\\" && row[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (ch === "`") {
      let run = 1;
      while (row[i + run] === "`") run++;
      ticks = ticks === run ? 0 : ticks || run;
      cell += "`".repeat(run);
      i += run - 1;
    } else if (ch === "|" && !ticks) {
      out.push(cell.trim());
      cell = "";
    } else cell += ch;
  }
  out.push(cell.trim());
  return out.slice(1, -1);
}

/** What's inside a code span: `` `x` `` or ``` `` x `` ```. */
const code = (cell) => cell.match(/^(`+)\s?([\s\S]*?)\s?\1$/)?.[2] ?? null;

/** A page's `### Name` API sections: name → { rows, text }. */
function apiOf(markdown) {
  const api = markdown.split(/^## API$/m)[1];
  if (!api) return new Map();
  const sections = new Map();
  for (const part of api.split(/^### /m).slice(1)) {
    const [heading, ...lines] = part.split("\n");
    const body = lines.join("\n").split(/^## /m)[0];
    const rows = body
      .split("\n")
      .filter((l) => l.startsWith("|"))
      .slice(2)
      .map(cells);
    sections.set(heading.trim(), { rows, text: body });
  }
  return sections;
}

const markdownOf = (page) => read(pagePath(page));

/**
 * A component's snippet props: the ones it renders (`{@render footer()}`),
 * and the ones it hands to a component that does (`<Modal {footer}>`).
 */
const snippetCache = new Map();
function snippetsOf(name) {
  if (snippetCache.has(name)) return snippetCache.get(name);
  const source = src(`./${name}.svelte`);
  const { props } = propsOf(source);
  const out = new Set([...source.matchAll(/\{@render (\w+)\??\.?\(/g)].map((m) => m[1]).filter((n) => props.has(n)));
  snippetCache.set(name, out);
  for (const [, child, attributes] of source.matchAll(/<([A-Z]\w*)\b([^>]*)>/g)) {
    if (!EXPORTED.includes(child) || child === name) continue;
    for (const prop of props.keys()) {
      const handed = new RegExp(`\\{${prop}\\}|\\b${prop}=\\{${prop}\\}`).test(attributes);
      if (handed && snippetsOf(child).has(prop)) out.add(prop);
    }
  }
  return out;
}

describe("the reference", () => {
  it("lists every page it has, and has every page it lists", () => {
    const listed = PAGES.filter((p) => !p.file).map((p) => `${p.slug}.md`).sort();
    const present = readdirSync(fileURLToPath(new URL("../../docs/ui/", import.meta.url))).filter((f) => f.endsWith(".md")).sort();
    expect(present).toEqual(listed);
  });

  it("documents every component the package exports, on one page", () => {
    const documented = PAGES.flatMap((p) => p.components ?? []);
    expect([...documented].sort()).toEqual([...EXPORTED].sort());
    expect(new Set(documented).size).toBe(documented.length);
  });

  for (const page of PAGES.filter((p) => p.components)) {
    for (const name of page.components) {
      it(`${name}: its props table is its props (docs/${fileOf(page)})`, () => {
        const section = apiOf(markdownOf(page)).get(name);
        expect(section, `docs/${fileOf(page)} has no "### ${name}" under "## API"`).toBeDefined();

        const source = src(`./${name}.svelte`);
        const { props, rest } = propsOf(source);
        const snippets = snippetsOf(name);
        const rows = new Map(
          section.rows.map((r) => {
            const prop = r[0].match(/^`([\w]+)`/)?.[1];
            return [prop, { cell: r[0], type: r[1], default: r[2] }];
          }),
        );
        // `class` is renamed in the source (`class: className`); the key is `class`.
        expect([...rows.keys()].sort()).toEqual([...props.keys()].sort());

        for (const [prop, { default: value, bindable }] of props) {
          const row = rows.get(prop);
          const shown = row.default === "—" ? null : code(row.default);
          expect(shown, `${name}'s ${prop} defaults to ${value ?? "nothing"}`).toBe(value);
          expect(row.cell.includes("bindable"), `${name}'s ${prop} is${bindable ? "" : " not"} bindable`).toBe(bindable);
          expect(/Snippet/.test(row.type), `${name}'s ${prop} is${snippets.has(prop) ? "" : " not"} a snippet`).toBe(snippets.has(prop));
        }
        if (rest) expect(section.text, `${name} passes other attributes on: say where`).toMatch(/Other attributes/);
      });
    }
  }

  it("names every design token", () => {
    const tokens = [...new Set([...src("./tokens.css").matchAll(/(--elyra-[\w-]+):/g)].map((m) => m[1]))];
    const page = markdownOf(PAGES.find((p) => p.slug === "tokens"));
    const missing = tokens.filter((t) => !page.includes(`\`${t}\``));
    expect(missing).toEqual([]);
  });
});

describe("the examples", () => {
  const dir = fileURLToPath(new URL("../docs/examples/", import.meta.url));
  const files = readdirSync(dir, { recursive: true })
    .filter((f) => f.endsWith(".svelte"))
    .map((f) => f.replace(/\.svelte$/, "").split("\\").join("/"))
    .sort();

  it("are in their pages as they are in their files (npm run docs:sync)", () => {
    for (const page of PAGES) {
      const markdown = markdownOf(page);
      expect(sync(markdown, (name) => read(examplePath(name))), `docs/${fileOf(page)}`).toBe(markdown);
    }
  });

  it("are each on one page, and every page's examples exist", () => {
    const used = PAGES.flatMap((page) => examplesIn(markdownOf(page)));
    expect([...used].sort()).toEqual(files);
  });

  for (const name of files) {
    it(`${name} compiles without a warning`, () => {
      const { warnings } = compile(read(examplePath(name)), { filename: `${name}.svelte`, generate: "client" });
      expect(warnings.map((w) => w.message)).toEqual([]);
    });
  }
});
