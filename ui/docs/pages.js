// The reference's pages, in the order the docs site lists them. Each page is
// a Markdown file in the repository's docs/ (`docs/ui/<slug>.md`, or `file`),
// and documents the components named here: its `## API` section has a
// `### <Component>` table for each (src/docs.test.js holds it to that).

export const GROUPS = [
  {
    title: "Start",
    pages: [
      { slug: "overview", title: "Overview", file: "ui.md" },
      { slug: "tokens", title: "Design tokens" },
    ],
  },
  {
    title: "Actions",
    pages: [
      { slug: "button", title: "Button", components: ["Button", "ButtonGroup"] },
      { slug: "dropdown", title: "Dropdown", components: ["Dropdown", "MenuItem"] },
    ],
  },
  {
    title: "Forms",
    pages: [
      { slug: "form", title: "Form and Field", components: ["Form", "Field"] },
      { slug: "input", title: "Input and Textarea", components: ["Input", "Textarea"] },
      { slug: "select", title: "Select", components: ["Select"] },
      { slug: "checkbox", title: "Checkbox", components: ["Checkbox"] },
      { slug: "switch", title: "Switch", components: ["Switch"] },
      { slug: "radio-group", title: "RadioGroup", components: ["RadioGroup"] },
    ],
  },
  {
    title: "Overlays",
    pages: [
      { slug: "modal", title: "Modal and Drawer", components: ["Modal", "Drawer"] },
      { slug: "popover", title: "Popover", components: ["Popover"] },
      { slug: "tooltip", title: "Tooltip", components: ["Tooltip"] },
    ],
  },
  {
    title: "Data",
    pages: [
      { slug: "table", title: "Table and Pagination", components: ["Table", "Pagination"] },
      { slug: "badge", title: "Badge", components: ["Badge"] },
    ],
  },
  {
    title: "Layout",
    pages: [
      { slug: "app-shell", title: "AppShell", components: ["AppShell", "Sidebar", "NavItem", "Toolbar"] },
      { slug: "status-bar", title: "StatusBar", components: ["StatusBar"] },
      { slug: "tabs", title: "Tabs", components: ["Tabs"] },
      { slug: "card", title: "Card", components: ["Card"] },
      { slug: "separator", title: "Separator", components: ["Separator"] },
    ],
  },
  {
    title: "Feedback",
    pages: [
      { slug: "callout", title: "Callout", components: ["Callout"] },
      { slug: "outbox-drawer", title: "OutboxDrawer", components: ["OutboxDrawer"] },
      { slug: "empty", title: "Empty", components: ["Empty"] },
      { slug: "spinner", title: "Spinner and Skeleton", components: ["Spinner", "Skeleton"] },
    ],
  },
];

export const PAGES = GROUPS.flatMap((group) => group.pages.map((page) => ({ ...page, group: group.title })));

/** The page's Markdown file, relative to the repository's docs/. */
export const fileOf = (page) => page.file ?? `ui/${page.slug}.md`;

/**
 * An example in a page: its name, then its code, which `npm run docs:sync`
 * keeps the same as ui/docs/examples/<name>.svelte.
 */
export const MARKER = /<!-- example: ([\w/-]+) -->\n```svelte\n([\s\S]*?)```/g;
