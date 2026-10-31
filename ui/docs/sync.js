// The examples in the reference are files (ui/docs/examples/<page>/<name>.svelte):
// the docs site runs them, the tests compile and mount them. Each one's code
// is also in its page, under a marker, so the Markdown reads on its own:
//
//   <!-- example: button/variants -->
//   ```svelte
//   …the file…
//   ```
//
// `npm run docs:sync` writes the files into the pages; the tests fail when
// a page and its example differ.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { MARKER, PAGES, fileOf } from "./pages.js";

export { MARKER };

const docs = new URL("../../docs/", import.meta.url);
const examples = new URL("./examples/", import.meta.url);

export const pagePath = (page) => fileURLToPath(new URL(fileOf(page), docs));
export const examplePath = (name) => fileURLToPath(new URL(`${name}.svelte`, examples));

/** `markdown` with each marked example's code replaced by what `read(name)` gives. */
export function sync(markdown, read) {
  return markdown.replace(MARKER, (_, name) => {
    const code = read(name);
    return `<!-- example: ${name} -->\n\`\`\`svelte\n${code.endsWith("\n") ? code : `${code}\n`}\`\`\``;
  });
}

/** The examples a page marks, in order. */
export function examplesIn(markdown) {
  return [...markdown.matchAll(MARKER)].map((m) => m[1]);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes("--check");
  let stale = 0;
  for (const page of PAGES) {
    const path = pagePath(page);
    const before = readFileSync(path, "utf8");
    const after = sync(before, (name) => readFileSync(examplePath(name), "utf8"));
    if (before === after) continue;
    stale++;
    if (check) console.error(`out of date: docs/${fileOf(page)}`);
    else {
      writeFileSync(path, after);
      console.log(`wrote docs/${fileOf(page)}`);
    }
  }
  if (check && stale) process.exit(1);
}
