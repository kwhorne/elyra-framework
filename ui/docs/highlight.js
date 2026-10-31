// A small highlighter for the reference's code: Svelte, JS, CSS, JSON and
// shell, coloured by the design tokens. Enough to read by, no dependency.

const RULES = {
  comment: /<!--[\s\S]*?-->|\/\*[\s\S]*?\*\/|(?<![:\w])\/\/[^\n]*|^#[^\n]*/,
  string: /"(?:[^"\\\n]|\\.)*"|`(?:[^`\\]|\\.)*`/,
  block: /\{[#:/@](?:snippet|if|else|each|await|then|catch|render|html|const|key)\b/,
  tag: /<\/?[A-Za-z][\w.:-]*|\/?>/,
  rune: /\$(?:state|derived|props|bindable|effect|inspect)\b(?:\.by)?/,
  keyword: /\b(?:import|from|export|default|let|const|function|async|await|return|if|else|new|of|throw|try|catch)\b/,
  literal: /\b(?:true|false|null|undefined)\b|(?<![\w-])\d[\d_.]*\b/,
  attr: /(?<=\s)[\w:-]+(?==)|(?<=\s)(?:bind|on|class|style):[\w-]+/,
  prompt: /^\$ /,
};

const PATTERN = new RegExp(
  Object.entries(RULES)
    .map(([name, rule]) => `(?<${name}>${rule.source})`)
    .join("|"),
  "gm",
);

const escape = (text) => text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** `code` as HTML, its parts in `<span class="hl-…">`. */
export function highlight(code) {
  let out = "";
  let at = 0;
  for (const match of code.matchAll(PATTERN)) {
    if (match[0] === "") continue;
    const kind = Object.keys(RULES).find((name) => match.groups[name] !== undefined);
    out += escape(code.slice(at, match.index));
    out += `<span class="hl-${kind}">${escape(match[0])}</span>`;
    at = match.index + match[0].length;
  }
  return out + escape(code.slice(at));
}
