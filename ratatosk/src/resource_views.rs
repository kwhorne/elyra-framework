//! `rata make:resource <Model> --view` — the Svelte half of a resource (RFC
//! 0001 step 4): a list, a form for create and edit, and a detail view, over the
//! typed `api.*` that codegen emits for the resource's commands.
//!
//! The views use only what ships — the scaffold's CSS variables and `.btn` /
//! `.card`, the runtime's router, `confirm`, `toast` and `validationErrors` — so
//! they match the theme and need no new dependency. Labels go through `$t` when
//! the project has `lang/en.json`, whose English defaults this merges in.

use std::path::{Component, Path, PathBuf};

use crate::make_resource::{Field, Format, Kind, ModelInfo, Names};

/// Rows per page the list asks for.
const PER_PAGE: i64 = 25;
/// Columns the list shows; the detail view shows every field.
const MAX_COLUMNS: usize = 5;

/// A label key and its English default.
pub(crate) type Label = (String, String);

/// A label, either translated (`$t("customers.save")`) or plain English. The
/// keys used are collected so the English defaults can be merged into the
/// project's `lang/en.json`.
pub(crate) struct Labels {
    ns: String,
    i18n: bool,
    used: Vec<(String, String)>,
}

impl Labels {
    pub(crate) fn new(ns: &str, i18n: bool) -> Self {
        Labels {
            ns: ns.to_string(),
            i18n,
            used: Vec::new(),
        }
    }

    fn remember(&mut self, key: &str, en: &str) {
        if !self.used.iter().any(|(k, _)| k == key) {
            self.used.push((key.to_string(), en.to_string()));
        }
    }

    /// In markup: `{$t("ns.key")}` or the text.
    fn m(&mut self, key: &str, en: &str) -> String {
        self.remember(key, en);
        if self.i18n {
            format!("{{$t(\"{}.{key}\")}}", self.ns)
        } else {
            en.to_string()
        }
    }

    /// In script: `$t("ns.key")` or a string literal.
    fn js(&mut self, key: &str, en: &str) -> String {
        self.remember(key, en);
        if self.i18n {
            format!("$t(\"{}.{key}\")", self.ns)
        } else {
            js_str(en)
        }
    }

    /// In markup, with `:name` placeholders filled from JS expressions.
    fn mp(&mut self, key: &str, en: &str, params: &[(&str, &str)]) -> String {
        self.remember(key, en);
        if self.i18n {
            let args: Vec<String> = params.iter().map(|(n, e)| format!("{n}: {e}")).collect();
            format!("{{$t(\"{}.{key}\", {{ {} }})}}", self.ns, args.join(", "))
        } else {
            // Longest name first, so `:to` doesn't eat the start of `:total`.
            let mut ordered = params.to_vec();
            ordered.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
            let mut out = en.to_string();
            for (name, expr) in ordered {
                out = out.replace(&format!(":{name}"), &format!("{{{expr}}}"));
            }
            out
        }
    }

    /// The `import` for `t`, when labels are translated.
    fn import(&self) -> &'static str {
        if self.i18n {
            ", t"
        } else {
            ""
        }
    }

    pub(crate) fn used(&self) -> &[(String, String)] {
        &self.used
    }
}

fn js_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `credit_limit` -> `Credit limit`.
pub(crate) fn humanize(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn field_label(l: &mut Labels, f: &Field) -> String {
    l.m(&format!("fields.{}", f.name), &humanize(&f.name))
}

/// How a value reads in the list and the detail view.
fn display(f: &Field, var: &str, l: &mut Labels) -> String {
    let v = format!("{var}.{}", f.name);
    if f.is_file() {
        return format!("{{{v} ?? \"—\"}}");
    }
    match f.kind {
        Kind::Bool => {
            let yes = l.js("yes", "Yes");
            let no = l.js("no", "No");
            if f.nullable {
                format!("{{{v} == null ? \"—\" : {v} ? {yes} : {no}}}")
            } else {
                format!("{{{v} ? {yes} : {no}}}")
            }
        }
        Kind::Other => format!("<code>{{JSON.stringify({v})}}</code>"),
        _ if f.nullable => format!("{{{v} ?? \"—\"}}"),
        _ => format!("{{{v}}}"),
    }
}

/// A label as a JavaScript expression inside `{…}`: `$t(…)` as it is, a
/// plain label quoted.
fn js_label(label: &str) -> String {
    if label.starts_with("{") && label.ends_with("}") {
        label[1..label.len() - 1].to_owned()
    } else {
        js_str(label)
    }
}

/// `FileField.svelte`, next to the resources' views: the file field the
/// forms with a `file` field use (RFC 0009). Written once; the app's to
/// change.
pub(crate) const FILE_FIELD: &str = r#"<!--
  A file field — written by `rata make:resource` for a `file` field (RFC 0009),
  and yours to change. The user picks a file, or drops one on the window while
  the form is open (`acceptsDrop`); the form sends it beside its fields.
-->
<script>
  import { dialog, onFileDrop } from "@elyra/runtime";

  let {
    label,
    file = $bindable(null),
    current = null,
    error = null,
    filters = [],
    acceptsDrop = false,
  } = $props();

  async function choose() {
    const [picked] = await dialog.open({ filters });
    if (picked) file = picked;
  }

  $effect(() => {
    if (!acceptsDrop) return;
    return onFileDrop((files) => {
      const dropped = files.find((f) => f.kind === "open");
      if (dropped) file = dropped;
    });
  });
</script>

<div class="field">
  <span>{label}</span>
  <div class="pick" class:invalid={!!error}>
    <button type="button" class="btn" onclick={choose}>Choose a file…</button>
    <span class="name">{file?.name ?? current ?? "No file"}</span>
    {#if file}<button type="button" class="btn" onclick={() => (file = null)}>Keep the current one</button>{/if}
  </div>
  {#if error}<small class="error">{error}</small>{/if}
</div>

<style>
  .field { display: grid; gap: 6px; }
  .field > span { color: var(--text-dim); font-size: 13px; }
  .pick { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .pick.invalid .name { color: var(--red); }
  .name { color: var(--text); overflow-wrap: anywhere; }
  .error { color: var(--red); }
</style>
"#;

/// `import … from "<this>"`: the generated bindings, relative to the views.
/// Both paths are relative to the project root.
pub(crate) fn bindings_import(view_dir: &Path, bindings: &Path) -> String {
    let parts = |p: &Path| -> Vec<String> {
        p.components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    let from = parts(view_dir);
    let mut to = parts(&bindings.with_extension(""));
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut rel: Vec<String> = vec!["..".to_string(); from.len() - common];
    if rel.is_empty() {
        rel.push(".".into());
    }
    rel.extend(to.drain(common..));
    rel.join("/")
}

fn header(what: &str, n: &Names) -> String {
    format!(
        "<!--\n  {what} — generated by `rata make:resource {} --view`, and yours to change.\n-->\n",
        n.ty
    )
}

pub(crate) fn render_index_js(n: &Names) -> String {
    let p = &n.plural;
    let route = format!("/{p}");
    format!(
        r#"// The {human} pages — generated by `rata make:resource {ty} --view`, and
// yours to change. rata lists this folder in ../index.js.

export const routes = {{
  "{route}": () => import("./Index.svelte"),
  "{route}/new": () => import("./Form.svelte"),
  "{route}/:id": () => import("./Show.svelte"),
  "{route}/:id/edit": () => import("./Form.svelte"),
}};

export const nav = [{{ label: "{title}", path: "{route}" }}];
"#,
        human = n.human,
        ty = n.ty,
        title = humanize(&n.plural),
    )
}

pub(crate) fn render_index(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    l: &mut Labels,
    backend: Option<&str>,
    gate: bool,
) -> String {
    let p = &n.plural;
    let pk = &m.pk;
    let human = &n.human;
    let shown: Vec<&Field> = m
        .editable
        .iter()
        .filter(|f| f.kind != Kind::Other)
        .take(MAX_COLUMNS)
        .collect();

    let confirm_delete = l.js("confirm_delete", &format!("Delete this {human}?"));
    let delete_js = l.js("delete", "Delete");
    let deleted = l.js("deleted", &format!("{} deleted", humanize(human)));
    let title = l.m("title", &humanize(p));
    let search_ph = l.js("search", "Search…");
    let new_label = l.m("new", &format!("New {human}"));
    let loading = l.m("loading", "Loading…");
    let no_match = l.js(
        "no_match",
        &format!("No {} match your search.", n.plural.replace('_', " ")),
    );
    let empty = l.js("empty", &format!("No {} yet.", n.plural.replace('_', " ")));
    let delete_m = l.m("delete", "Delete");
    let range = l.mp(
        "range",
        ":from–:to of :total",
        &[("from", "from"), ("to", "to"), ("total", "result.total")],
    );
    let previous = l.m("previous", "Previous");
    let next = l.m("next", "Next");
    let offline = offline_index(l, n, backend);

    let heads: String = shown
        .iter()
        .map(|f| {
            format!(
                "          <th>\n            <button class=\"sort\" class:sorted={{sort === \"{col}\"}} onclick={{() => sortBy(\"{col}\")}}>\n              {label} {{arrow(\"{col}\")}}\n            </button>\n          </th>\n",
                col = f.column,
                label = field_label(l, f),
            )
        })
        .collect();
    let cells: String = shown
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let value = display(f, "row", l);
            if i == 0 {
                format!("            <td><a href={{href(`/{p}/${{row.{pk}}}`)}}>{value}</a></td>\n")
            } else {
                format!("            <td>{value}</td>\n")
            }
        })
        .collect();
    let first_cell_link = if shown.is_empty() {
        format!("            <td><a href={{href(`/{p}/${{row.{pk}}}`)}}>#{{row.{pk}}}</a></td>\n")
    } else {
        String::new()
    };

    format!(
        r#"{header}<script>
  import {{ {can_import}confirm, {offline_imports}href, {offline_outbox}toast{t} }} from "@elyra/runtime";
  import {{ api, live }} from "{bindings}";

  const PER_PAGE = {PER_PAGE};

  let search = $state("");
  let term = $state("");
  let sort = $state("{pk}");
  let direction = $state("asc");
  let page = $state(1);

  // Search once typing pauses.
  $effect(() => {{
    const value = search.trim();
    const timer = setTimeout(() => {{
      if (value !== term) {{
        term = value;
        page = 1;
      }}
    }}, 250);
    return () => clearTimeout(timer);
  }});

  // The list is live: Rust re-runs it when a write — in this window or any
  // other — changes what it shows, so nothing here ever reloads it. A new
  // query opens a new subscription and closes the old one.
  const list = $derived(
    live.{p}_index({{ search: term || null, sort, direction, page, per_page: PER_PAGE }}),
  );
  const result = $derived($list.value ?? null);
  const error = $derived($list.error && !$list.value ? message($list.error) : "");

  function sortBy(column) {{
    direction = sort === column && direction === "asc" ? "desc" : "asc";
    sort = column;
    page = 1;
  }}

  function arrow(column) {{
    return sort === column ? (direction === "asc" ? "↑" : "↓") : "";
  }}

  async function remove(row) {{
    const ok = await confirm({confirm_delete}, {{ danger: true, confirmLabel: {delete_js} }});
    if (!ok) return;
    try {{
      await api.{p}_destroy(row.{pk});
      toast({deleted}, {{ variant: "success" }});
    }} catch (e) {{
{remove_catch}    }}
  }}

  const message = (e) => e?.detail ?? e?.message ?? String(e);
  const from = $derived(result?.data.length ? (result.current_page - 1) * result.per_page + 1 : 0);
  const to = $derived(result?.data.length ? from + result.data.length - 1 : 0);
{offline_script}</script>

<div class="resource">
  <header class="head">
    <h2>{title}</h2>
    <input class="search" type="search" placeholder={{{search_ph}}} bind:value={{search}} />
    {new_open}<a class="btn primary" href={{href("/{p}/new")}}>{new_label}</a>{new_close}
  </header>
{offline_markup}
  {{#if error}}
    <p class="error">{{error}}</p>
  {{:else if !result}}
    <p class="dim">{loading}</p>
  {{:else if result.data.length === 0}}
    <p class="dim">{{term ? {no_match} : {empty}}}</p>
  {{:else}}
    <table>
      <thead>
        <tr>
{first_head}{heads}          <th></th>
        </tr>
      </thead>
      <tbody>
        {{#each result.data as row (row.{pk})}}
          <tr>
{first_cell_link}{cells}            <td class="actions">
              {row_delete_open}<button class="btn danger" onclick={{() => remove(row)}}>{delete_m}</button>{row_delete_close}
            </td>
          </tr>
        {{/each}}
      </tbody>
    </table>
    <footer class="pager">
      <span class="dim">{range}</span>
      <button class="btn" disabled={{page <= 1}} onclick={{() => page--}}>{previous}</button>
      <button class="btn" disabled={{page >= result.last_page}} onclick={{() => page++}}>{next}</button>
    </footer>
  {{/if}}
</div>

<style>
  .resource {{ max-width: 960px; }}
  .head {{ display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }}
  .head h2 {{ margin: 0; font-size: 18px; font-weight: 600; }}
  .search {{ max-width: 280px; margin-left: auto; }}
  table {{ width: 100%; border-collapse: collapse; background: var(--panel); border: 1px solid var(--border); border-radius: 10px; overflow: hidden; }}
  th, td {{ text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--border); }}
  tbody tr:last-child td {{ border-bottom: none; }}
  tbody tr:hover {{ background: var(--bg-3); }}
  td a {{ color: var(--accent); text-decoration: none; }}
{sort_css}  .actions {{ text-align: right; width: 1%; white-space: nowrap; }}
  .btn.danger {{ color: var(--red); }}
  .btn.danger:hover {{ border-color: var(--red); }}
  .btn:disabled {{ opacity: 0.5; cursor: default; }}
  .pager {{ display: flex; align-items: center; gap: 8px; justify-content: flex-end; margin-top: 12px; }}
  .pager .dim {{ margin-right: auto; }}
  .dim {{ color: var(--text-dim); }}
  .error {{ color: var(--red); }}
{offline_css}</style>
"#,
        header = header("The list", n),
        t = l.import(),
        // What the user may do, by Laravel (RFC 0010): what they can't is hidden.
        can_import = if gate { "can, " } else { "" },
        new_open = if gate {
            format!("{{#if $can(\"{p}.create\")}}")
        } else {
            String::new()
        },
        new_close = if gate { "{/if}" } else { "" },
        row_delete_open = if gate {
            format!("{{#if row.can?.delete ?? $can(\"{p}.delete\")}}")
        } else {
            String::new()
        },
        row_delete_close = if gate { "{/if}" } else { "" },
        offline_imports = if backend.is_some() {
            "connection, "
        } else {
            ""
        },
        offline_outbox = if backend.is_some() { "outbox, " } else { "" },
        offline_script = offline.script,
        offline_markup = offline.markup,
        offline_css = offline.css,
        remove_catch = offline.remove_catch,
        sort_css = if shown.is_empty() {
            ""
        } else {
            "  .sort { background: none; border: none; padding: 0; color: var(--text-dim); font: inherit; font-weight: 600; }\n  .sort.sorted, .sort:hover { color: var(--text); }\n"
        },
        first_head = if shown.is_empty() {
            "          <th>#</th>\n"
        } else {
            ""
        },
    )
}

/// The list's part in working offline (RFC 0007) — empty for a local resource.
struct OfflineIndex {
    script: String,
    markup: String,
    css: &'static str,
    remove_catch: String,
}

fn offline_index(l: &mut Labels, n: &Names, backend: Option<&str>) -> OfflineIndex {
    let p = &n.plural;
    let Some(api) = backend else {
        return OfflineIndex {
            script: String::new(),
            markup: String::new(),
            css: "",
            remove_catch: "      toast(message(e), { variant: \"error\" });\n".into(),
        };
    };
    let deleted_offline = l.js(
        "deleted_offline",
        "Deleted — it will be sent when you're back online",
    );
    let banner = l.m(
        "offline",
        "Offline — this is what was last read. Changes are saved, and sent when you're back.",
    );
    let waiting = l.mp(
        "waiting",
        ":count change(s) waiting to be sent",
        &[("count", "waiting.length")],
    );
    let keep = l.m("keep_mine", "Keep mine");
    let take = l.m("take_theirs", "Take theirs");
    let fix = l.m("fix", "Fix");
    let discard = l.m("discard", "Discard");
    OfflineIndex {
        script: format!(
            r#"
  // Working offline (RFC 0007): this resource's writes that wait for the
  // network, and those the server refused — to fix, or to drop.
  const API = "{api}";
  const writes = $derived(
    $outbox.filter((w) => {{
      const path = w.path.split("?")[0];
      return path === API || path.startsWith(`${{API}}/`);
    }}),
  );
  const waiting = $derived(writes.filter((w) => w.status === "pending"));
  const refused = $derived(writes.filter((w) => w.status === "failed"));

  // A refused create opens the empty form, an update the record's, with the
  // write's values and the server's errors.
  function fixHref(w) {{
    const id = w.path.split("?")[0].slice(API.length + 1);
    return href(id ? `/{p}/${{id}}/edit?write=${{w.id}}` : `/{p}/new?write=${{w.id}}`);
  }}
"#
        ),
        markup: format!(
            r#"
  {{#if $connection.state === "offline"}}
    <p class="banner">{banner}</p>
  {{/if}}
  {{#if waiting.length}}
    <p class="dim">{waiting}</p>
  {{/if}}
  {{#each refused as w (w.id)}}
    <div class="refused">
      <span class="error">{{w.error}}</span>
      {{#if w.reason === "conflict"}}
        <button class="btn" onclick={{() => outbox.keepMine(w.id)}}>{keep}</button>
        <button class="btn" onclick={{() => outbox.takeTheirs(w.id)}}>{take}</button>
      {{:else if w.method !== "DELETE"}}
        <a class="btn" href={{fixHref(w)}}>{fix}</a>
      {{/if}}
      <button class="btn" onclick={{() => outbox.discard(w.id)}}>{discard}</button>
    </div>
  {{/each}}
"#
        ),
        css: "  .banner { padding: 8px 12px; border-radius: 8px; background: var(--bg-3); color: var(--text-dim); }\n  .refused { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }\n  .refused .error { margin-right: auto; }\n",
        remove_catch: format!(
            "      if (e?.kind === \"queued\") toast({deleted_offline}, {{ variant: \"info\" }});\n      else toast(message(e), {{ variant: \"error\" }});\n"
        ),
    }
}

/// The form's starting value for one field.
/// The field's `=default` when `--generate` gave one.
fn blank(f: &Field) -> String {
    match (f.kind, &f.default) {
        (Kind::Text, Some(d)) => js_str(d),
        (Kind::Int | Kind::Float | Kind::Bool, Some(d)) => d.clone(),
        (Kind::Text, None) => "\"\"".into(),
        (Kind::Int | Kind::Float, None) => "null".into(),
        (Kind::Bool, None) => "false".into(),
        (Kind::Other, _) => "\"null\"".into(),
    }
}

/// A textarea: JSON, and long text.
fn is_textarea(f: &Field) -> bool {
    f.kind == Kind::Other || f.format == Format::Long
}

/// Only the rules a form's fields use — Svelte warns about the rest.
fn form_css(m: &ModelInfo) -> String {
    let has = |kind: Kind| m.editable.iter().any(|f| f.kind == kind);
    let mut css = String::new();
    if has(Kind::Bool) {
        css.push_str(
            "  .check { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }\n",
        );
        css.push_str("  .check input { flex: none; }\n");
        css.push_str("  .check small { flex-basis: 100%; }\n");
    }
    if m.editable.iter().any(is_textarea) {
        css.push_str(
            "  textarea { background: var(--bg-3); color: var(--text); border: 1px solid var(--border); \
             border-radius: 7px; padding: 7px 10px; font-family: var(--font-mono); }\n",
        );
    }
    if m.editable.iter().any(|f| f.kind != Kind::Bool) {
        css.push_str("  [aria-invalid=\"true\"] { border-color: var(--red); }\n");
    }
    css
}

pub(crate) fn render_form(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    l: &mut Labels,
    backend: Option<&str>,
) -> String {
    let p = &n.plural;
    let pk = &m.pk;
    let human = &n.human;
    let has_json = m.editable.iter().any(|f| f.kind == Kind::Other);

    let blanks: String = m
        .editable
        .iter()
        .filter(|f| !f.is_file())
        .map(|f| format!("      {}: {},\n", f.name, blank(f)))
        .collect();
    let from_record: String = m
        .editable
        .iter()
        .filter(|f| !f.is_file())
        .map(|f| {
            let v = format!("record.{}", f.name);
            let value = match f.kind {
                Kind::Text if f.nullable => format!("{v} ?? \"\""),
                Kind::Bool if f.nullable => format!("{v} ?? false"),
                Kind::Other => format!("JSON.stringify({v} ?? null, null, 2)"),
                _ => v,
            };
            format!("      {}: {value},\n", f.name)
        })
        .collect();
    let payload: String = m
        .editable
        .iter()
        .filter(|f| !f.is_file())
        .map(|f| {
            let v = format!("form.{}", f.name);
            let value = match f.kind {
                Kind::Text if f.nullable => format!("{v} === \"\" ? null : {v}"),
                Kind::Int | Kind::Float => format!("number({v})"),
                Kind::Other => format!("json(\"{}\", {v})", f.name),
                _ => v,
            };
            format!("      {}: {value},\n", f.name)
        })
        .collect();

    let invalid_json = if has_json {
        l.js("invalid_json", "Not valid JSON.")
    } else {
        String::new()
    };
    let json_helper = if has_json {
        r#"
  // A JSON field is edited as text; bad JSON is a field error, not a request.
  let bad = [];
  function json(field, text) {
    try {
      return text.trim() === "" ? null : JSON.parse(text);
    } catch {
      bad.push(field);
      return null;
    }
  }
"#
        .to_string()
    } else {
        String::new()
    };
    let json_check = if has_json {
        format!(
            r#"    bad = [];
    const input = payload();
    if (bad.length) {{
      errors = Object.fromEntries(bad.map((field) => [field, [{invalid_json}]]));
      return;
    }}
"#
        )
    } else {
        "    const input = payload();\n".into()
    };

    // File fields (RFC 0009): the file the user chose goes beside the input.
    let file_fields: Vec<&Field> = m.editable.iter().filter(|f| f.is_file()).collect();
    let file_args: String = file_fields
        .iter()
        .map(|f| format!(", files.{} ?? null", f.name))
        .collect();
    let (file_import, file_state, file_current) = if file_fields.is_empty() {
        (String::new(), String::new(), String::new())
    } else {
        let names = |value: &str| -> String {
            file_fields
                .iter()
                .map(|f| format!("{}: {value}", f.name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        (
            "  import FileField from \"../FileField.svelte\";\n".to_string(),
            format!(
                "\n  // The files chosen here, and the names of those the record has.\n  let files = $state({{ {} }});\n  let current = $state({{}});\n",
                names("null")
            ),
            format!(
                " current = {{ {} }};",
                file_fields
                    .iter()
                    .map(|f| format!("{0}: record.{0}", f.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    };
    let fields: String = m
        .editable
        .iter()
        .map(|f| {
            let label = field_label(l, f);
            let name = &f.name;
            let error = format!(
                "    {{#if errors.{name}}}<small class=\"error\">{{errors.{name}[0]}}</small>{{/if}}\n"
            );
            if f.is_file() {
                // The first file field also takes a file dropped on the window.
                let drop = if file_fields.first().is_some_and(|first| first.name == f.name) {
                    " acceptsDrop"
                } else {
                    ""
                };
                return format!(
                    "  <FileField label={{{label_js}}} bind:file={{files.{name}}} current={{current.{name}}} error={{errors.{name}?.[0]}}{drop} />\n",
                    label_js = js_label(&label)
                );
            }
            match f.kind {
                Kind::Bool => format!(
                    "  <label class=\"field check\">\n    <input type=\"checkbox\" bind:checked={{form.{name}}} />\n    <span>{label}</span>\n{error}  </label>\n"
                ),
                _ if is_textarea(f) => format!(
                    "  <label class=\"field\">\n    <span>{label}</span>\n    <textarea rows=\"4\" bind:value={{form.{name}}} aria-invalid={{!!errors.{name}}}></textarea>\n{error}  </label>\n"
                ),
                kind => {
                    let attrs = match (kind, f.format) {
                        (_, Format::Email) => " type=\"email\"",
                        (_, Format::Date) => " type=\"date\"",
                        (Kind::Int, _) => " type=\"number\" step=\"1\"",
                        (Kind::Float, _) => " type=\"number\" step=\"any\"",
                        _ => "",
                    };
                    format!(
                        "  <label class=\"field\">\n    <span>{label}</span>\n    <input{attrs} bind:value={{form.{name}}} aria-invalid={{!!errors.{name}}} />\n{error}  </label>\n"
                    )
                }
            }
        })
        .collect();

    let created = l.js("created", &format!("{} created", humanize(human)));
    let saved = l.js("saved", &format!("{} saved", humanize(human)));
    let new_title = l.js("new", &format!("New {human}"));
    let edit_title = l.js("edit", &format!("Edit {human}"));
    let save = l.m("save", "Save");
    let cancel = l.m("cancel", "Cancel");
    // Over the backend (RFC 0007): a save without an answer waits in the
    // outbox, and a refused one is fixed here and sent again.
    let (form_imports, form_props, write_effect, skip_load, resend, queued_catch) = match backend {
        Some(_) => {
            let queued = l.js("queued", "Saved — it will be sent when you're back online");
            let resent = l.js("resent", "Sent again");
            (
                "outbox, ",
                "let { params = {}, query = {} } = $props();\n  const id = $derived(params.id ? Number(params.id) : null);\n  // Fixing a write the server refused: its values, and the server's errors.\n  const write = $derived(query.write ? Number(query.write) : null);",
                "\n  let fixing = null;\n  $effect(() => {\n    const w = write === null ? null : $outbox.find((w) => w.id === write);\n    if (w && fixing !== w.id) {\n      fixing = w.id;\n      form = fromRecord({ ...blank(), ...w.body });\n      errors = w.errors ?? {};\n    }\n  });\n".to_string(),
                "    if (write !== null) return;\n",
                format!("      if (write !== null) {{\n        await outbox.retry(write, input);\n        toast({resent}, {{ variant: \"success\" }});\n        navigate(\"/{p}\");\n        return;\n      }}\n"),
                format!("      if (e?.kind === \"queued\") {{\n        toast({queued}, {{ variant: \"info\" }});\n        navigate(\"/{p}\");\n        return;\n      }}\n"),
            )
        }
        None => (
            "",
            "let { params = {} } = $props();\n  const id = $derived(params.id ? Number(params.id) : null);",
            String::new(),
            "",
            String::new(),
            String::new(),
        ),
    };

    format!(
        r#"{header}<script>
  import {{ href, navigate, {form_imports}toast, validationErrors{t} }} from "@elyra/runtime";
  import {{ api }} from "{bindings}";
{file_import}
  {form_props}

  let form = $state(blank());{file_state}
  let errors = $state({{}});
  let saving = $state(false);
  let loadError = $state("");

  function blank() {{
    return {{
{blanks}    }};
  }}

  // Editing: start from the record.
  $effect(() => {{
{skip_load}    errors = {{}};
    if (id === null) {{
      form = blank();
      return;
    }}
    api
      .{p}_show(id)
      .then((record) => {{
        form = fromRecord(record);{file_current}
      }})
      .catch((e) => (loadError = message(e)));
  }});

  function fromRecord(record) {{
    return {{
{from_record}    }};
  }}
{write_effect}
  // The form as the command's input: empty optional text is `null`.
  function payload() {{
    return {{
{payload}    }};
  }}

  const number = (value) =>
    value === "" || value === null || value === undefined || Number.isNaN(Number(value))
      ? null
      : Number(value);
{json_helper}
  async function submit(event) {{
    event.preventDefault();
{json_check}    errors = {{}};
    saving = true;
    try {{
{resend}      const record =
        id === null ? await api.{p}_store(input{file_args}) : await api.{p}_update(id, input{file_args});
      toast(id === null ? {created} : {saved}, {{ variant: "success" }});
      navigate(`/{p}/${{record.{pk}}}`);
    }} catch (e) {{
{queued_catch}      const bag = validationErrors(e);
      if (bag) errors = bag;
      else toast(message(e), {{ variant: "error" }});
    }} finally {{
      saving = false;
    }}
  }}

  const message = (e) => e?.detail ?? e?.message ?? String(e);
</script>

<form class="card" onsubmit={{submit}}>
  <h2>{{id === null ? {new_title} : {edit_title}}}</h2>
  {{#if loadError}}<p class="error">{{loadError}}</p>{{/if}}
{fields}  <div class="row">
    <button class="btn primary" type="submit" disabled={{saving}}>{save}</button>
    <a class="btn" href={{href(id === null ? "/{p}" : `/{p}/${{id}}`)}}>{cancel}</a>
  </div>
</form>

<style>
  form {{ display: grid; gap: 14px; }}
  h2 {{ margin: 0; }}
  .field {{ display: grid; gap: 6px; }}
  .field > span {{ color: var(--text-dim); font-size: 13px; }}
{kind_css}  .error {{ color: var(--red); }}
  .btn:disabled {{ opacity: 0.5; cursor: default; }}
</style>
"#,
        header = header("The form, for create and edit", n),
        kind_css = form_css(m),
        t = l.import(),
    )
}

pub(crate) fn render_show(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    l: &mut Labels,
    backend: Option<&str>,
    gate: bool,
) -> String {
    let p = &n.plural;
    let pk = &m.pk;
    let human = &n.human;
    let heading = match m
        .editable
        .iter()
        .find(|f| f.kind == Kind::Text && !f.nullable)
    {
        Some(f) => format!("{{record.{}}}", f.name),
        None => format!("{} #{{record.{pk}}}", humanize(human)),
    };
    let rows: String = m
        .editable
        .iter()
        .map(|f| {
            let value = if f.is_file() {
                // A file (RFC 0009): its name, which downloads it.
                format!(
                    "{{#if record.{0}}}<button class=\"link\" onclick={{() => download(\"{0}\")}}>{{record.{0}}}</button>{{:else}}—{{/if}}",
                    f.name
                )
            } else {
                display(f, "record", l)
            };
            format!(
                "      <dt>{}</dt>\n      <dd>{value}</dd>\n",
                field_label(l, f)
            )
        })
        .collect();
    let file_fields: Vec<&Field> = m.editable.iter().filter(|f| f.is_file()).collect();
    let (show_import, download_fn, link_css) = if file_fields.is_empty() {
        ("", String::new(), "")
    } else {
        let saved_js = l.js("downloaded", "Saved");
        let map = file_fields
            .iter()
            .map(|f| format!("{0}: api.{p}_{0}", f.name))
            .collect::<Vec<_>>()
            .join(", ");
        (
            "dialog, ",
            format!(
                r#"
  // A file: saved where the user chooses, under its name.
  const downloads = {{ {map} }};
  async function download(field) {{
    const to = await dialog.save({{ defaultName: record[field] ?? undefined }});
    if (!to) return;
    try {{
      await downloads[field](id, to);
      toast({saved_js}, {{ variant: "success" }});
    }} catch (e) {{
      toast(message(e), {{ variant: "error" }});
    }}
  }}
"#
            ),
            "  .link { background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; text-decoration: underline; font: inherit; }\n",
        )
    };
    let confirm_delete = l.js("confirm_delete", &format!("Delete this {human}?"));
    let delete_js = l.js("delete", "Delete");
    let deleted = l.js("deleted", &format!("{} deleted", humanize(human)));
    let edit = l.m("edit_action", "Edit");
    let delete_m = l.m("delete", "Delete");
    let back = l.m("back", "Back");
    let loading = l.m("loading", "Loading…");
    // Over the backend (RFC 0007): a delete without an answer waits too.
    let remove_catch = match backend {
        Some(_) => {
            let deleted_offline = l.js(
                "deleted_offline",
                "Deleted — it will be sent when you're back online",
            );
            format!(
                "      if (e?.kind === \"queued\") {{\n        toast({deleted_offline}, {{ variant: \"info\" }});\n        navigate(\"/{p}\");\n      }} else toast(message(e), {{ variant: \"error\" }});\n"
            )
        }
        None => "      toast(message(e), { variant: \"error\" });\n".into(),
    };

    format!(
        r#"{header}<script>
  import {{ {can_import}confirm, {show_import}href, navigate, toast{t} }} from "@elyra/runtime";
  import {{ api, live }} from "{bindings}";

  let {{ params }} = $props();
  const id = $derived(Number(params.id));

  // Live: an edit in another window shows up here, and a delete there turns
  // this into "not found".
  const current = $derived(live.{p}_show(id));
  const record = $derived($current.value ?? null);
  const error = $derived($current.error ? message($current.error) : "");

  async function remove() {{
    const ok = await confirm({confirm_delete}, {{ danger: true, confirmLabel: {delete_js} }});
    if (!ok) return;
    try {{
      await api.{p}_destroy(id);
      toast({deleted}, {{ variant: "success" }});
      navigate("/{p}");
    }} catch (e) {{
{remove_catch}    }}
  }}

{download_fn}
  const message = (e) => e?.detail ?? e?.message ?? String(e);
</script>

{{#if error}}
  <div class="card">
    <p class="error">{{error}}</p>
    <a class="btn" href={{href("/{p}")}}>{back}</a>
  </div>
{{:else if record}}
  <div class="card">
    <h2>{heading}</h2>
    <dl>
{rows}    </dl>
    <div class="row">
      {edit_open}<a class="btn primary" href={{href(`/{p}/${{id}}/edit`)}}>{edit}</a>{edit_close}
      {delete_open}<button class="btn danger" onclick={{remove}}>{delete_m}</button>{delete_close}
      <a class="btn" href={{href("/{p}")}}>{back}</a>
    </div>
  </div>
{{:else}}
  <p class="dim">{loading}</p>
{{/if}}

<style>
  dl {{ display: grid; grid-template-columns: max-content 1fr; gap: 8px 16px; margin: 16px 0; }}
  dt {{ color: var(--text-dim); }}
  dd {{ margin: 0; }}
  .btn.danger {{ color: var(--red); }}
  .btn.danger:hover {{ border-color: var(--red); }}
  .dim {{ color: var(--text-dim); }}
  .error {{ color: var(--red); }}
{link_css}</style>
"#,
        header = header("The detail view", n),
        can_import = if gate { "can, " } else { "" },
        edit_open = if gate {
            format!("{{#if record.can?.update ?? $can(\"{p}.update\")}}")
        } else {
            String::new()
        },
        edit_close = if gate { "{/if}" } else { "" },
        delete_open = if gate {
            format!("{{#if record.can?.delete ?? $can(\"{p}.delete\")}}")
        } else {
            String::new()
        },
        delete_close = if gate { "{/if}" } else { "" },
        t = l.import(),
    )
}

/// The view files, rendered, plus the labels they use.
#[cfg(test)]
pub(crate) fn render(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    i18n: bool,
    backend: Option<&str>,
) -> (Vec<(String, String)>, Vec<Label>) {
    render_with(m, n, bindings, i18n, backend, false)
}

/// The views; `gate` hides what the signed-in user may not do, by the
/// backend's abilities (RFC 0010), for a resource with abilities over it.
pub(crate) fn render_with(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    i18n: bool,
    backend: Option<&str>,
    gate: bool,
) -> (Vec<(String, String)>, Vec<Label>) {
    let mut l = Labels::new(&n.plural, i18n);
    let files = vec![
        ("index.js".to_string(), render_index_js(n)),
        (
            "Index.svelte".to_string(),
            render_index(m, n, bindings, &mut l, backend, gate),
        ),
        (
            "Form.svelte".to_string(),
            render_form(m, n, bindings, &mut l, backend),
        ),
        (
            "Show.svelte".to_string(),
            render_show(m, n, bindings, &mut l, backend, gate),
        ),
    ];
    (files, l.used().to_vec())
}

// ---------------------------------------------------------------------------
// lang/en.json
// ---------------------------------------------------------------------------

/// The labels as a JSON object, nested on `.`, indented for depth 1.
fn labels_json(labels: &[(String, String)]) -> String {
    let q = |s: &str| serde_json::to_string(s).expect("a string");
    let mut top: Vec<String> = Vec::new();
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for (key, en) in labels {
        match key.split_once('.') {
            Some((group, rest)) => {
                let line = format!("      {}: {}", q(rest), q(en));
                match groups.iter_mut().find(|(g, _)| g == group) {
                    Some((_, lines)) => lines.push(line),
                    None => groups.push((group.to_string(), vec![line])),
                }
            }
            None => top.push(format!("    {}: {}", q(key), q(en))),
        }
    }
    for (group, lines) in groups {
        top.push(format!(
            "    {}: {{\n{}\n    }}",
            q(&group),
            lines.join(",\n")
        ));
    }
    format!("{{\n{}\n  }}", top.join(",\n"))
}

/// Add `"<ns>": { … }` to a translation file, textually, so the rest of the
/// file keeps its formatting. Returns the new text, or `None` when `ns` is
/// already there (it's the user's then — nothing is changed).
pub(crate) fn merge_labels(
    text: &str,
    ns: &str,
    labels: &[(String, String)],
) -> Result<Option<String>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("isn't valid JSON ({e})"))?;
    let Some(object) = parsed.as_object() else {
        return Err("isn't a JSON object".into());
    };
    if object.contains_key(ns) {
        return Ok(None);
    }
    let end = text.rfind('}').expect("an object ends with }");
    let before = text[..end].trim_end();
    let comma = if object.is_empty() { "" } else { "," };
    let block = format!(
        "{before}{comma}\n  {}: {}\n}}\n",
        serde_json::to_string(ns).expect("a string"),
        labels_json(labels)
    );
    serde_json::from_str::<serde_json::Value>(&block)
        .map_err(|e| format!("merging would produce invalid JSON ({e})"))?;
    Ok(Some(block))
}

/// The folder the views go in, relative to the project root.
pub(crate) fn view_dir(frontend_dir: &Path, n: &Names) -> PathBuf {
    frontend_dir.join("src").join("resources").join(&n.plural)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_resource::{names, tests::customer};

    #[test]
    fn bindings_are_imported_relative_to_the_views() {
        let views = Path::new("app/src/resources/customers");
        assert_eq!(
            bindings_import(views, Path::new("app/src/bindings.ts")),
            "../../bindings"
        );
        assert_eq!(
            bindings_import(views, Path::new("app/src/lib/api.ts")),
            "../../lib/api"
        );
        assert_eq!(
            bindings_import(
                Path::new("example/app/src/resources/x"),
                Path::new("example/app/src/bindings.ts")
            ),
            "../../bindings"
        );
    }

    #[test]
    fn labels_are_plain_or_translated() {
        let mut plain = Labels::new("customers", false);
        assert_eq!(plain.m("save", "Save"), "Save");
        assert_eq!(plain.js("save", "Save"), "\"Save\"");
        assert_eq!(
            plain.mp(
                "range",
                ":from–:to of :total",
                &[("from", "a"), ("to", "b"), ("total", "c")]
            ),
            "{a}–{b} of {c}"
        );
        let mut t = Labels::new("customers", true);
        assert_eq!(t.m("save", "Save"), "{$t(\"customers.save\")}");
        assert_eq!(t.js("save", "Save"), "$t(\"customers.save\")");
        assert_eq!(
            t.mp("range", ":from of :total", &[("from", "a"), ("total", "c")]),
            "{$t(\"customers.range\", { from: a, total: c })}"
        );
        assert_eq!(t.used().len(), 2, "a key is recorded once");
    }

    #[test]
    fn views_follow_the_fields() {
        let m = customer();
        let n = names("Customer");
        let (files, labels) = render(&m, &n, "../../bindings", false, None);
        let get = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;

        let index = get("Index.svelte");
        assert!(index.contains("live.customers_index({ search: term || null"));
        assert!(
            !index.contains("version"),
            "nothing reloads a live list by hand"
        );
        let show = &files.iter().find(|(f, _)| f == "Show.svelte").unwrap().1;
        assert!(show.contains("const current = $derived(live.customers_show(id));"));
        assert!(
            index.contains("sortBy(\"email_address\")"),
            "sorts by column, not field"
        );
        assert!(index.contains("{row.phone ?? \"—\"}"));
        assert!(
            !index.contains("JSON.stringify(row.tags)"),
            "JSON fields stay out of the table"
        );
        assert!(!index.contains("$t("));

        let form = get("Form.svelte");
        assert!(form.contains("<input type=\"number\" step=\"any\" bind:value={form.credit}"));
        assert!(form.contains("<input type=\"number\" step=\"1\" bind:value={form.visits}"));
        assert!(form.contains("bind:checked={form.active}"));
        assert!(form.contains("<textarea rows=\"4\" bind:value={form.tags}"));
        assert!(form.contains("phone: form.phone === \"\" ? null : form.phone,"));
        assert!(form.contains("tags: json(\"tags\", form.tags),"));
        assert!(form.contains("const bag = validationErrors(e);"));

        let show = get("Show.svelte");
        assert!(show.contains("<h2>{record.name}</h2>"));
        assert!(show.contains("<code>{JSON.stringify(record.tags)}</code>"));

        let routes = get("index.js");
        assert!(routes.contains("\"/customers/:id/edit\": () => import(\"./Form.svelte\")"));
        assert!(labels
            .iter()
            .any(|(k, v)| k == "fields.email" && v == "Email"));
    }

    #[test]
    fn with_lang_every_label_is_a_key() {
        let (files, labels) = render(
            &customer(),
            &names("Customer"),
            "../../bindings",
            true,
            None,
        );
        let form = &files.iter().find(|(f, _)| f == "Form.svelte").unwrap().1;
        assert!(form.contains("validationErrors, t }"));
        assert!(form.contains("{$t(\"customers.fields.credit\")}"));
        // Every key the views use has an English default.
        for (_, src) in &files {
            for part in src.split("$t(\"customers.").skip(1) {
                let key = &part[..part.find('"').unwrap()];
                assert!(labels.iter().any(|(k, _)| k == key), "no default for {key}");
            }
        }
    }

    #[test]
    fn merging_labels_keeps_the_file_and_never_overwrites() {
        let labels = vec![
            ("title".to_string(), "Customers".to_string()),
            ("fields.name".to_string(), "Name".to_string()),
            ("fields.email".to_string(), "Email \"work\"".to_string()),
        ];
        let text = "{\n    \"welcome\": \"Hi\",\n    \"nav\": { \"home\": \"Home\" }\n}\n";
        let merged = merge_labels(text, "customers", &labels).unwrap().unwrap();
        assert!(merged.starts_with(
            "{\n    \"welcome\": \"Hi\",\n    \"nav\": { \"home\": \"Home\" },\n  \"customers\""
        ));
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["customers"]["fields"]["email"], "Email \"work\"");
        assert_eq!(v["welcome"], "Hi");

        assert_eq!(
            merge_labels(&merged, "customers", &labels).unwrap(),
            None,
            "already there"
        );
        let empty = merge_labels("{}", "customers", &labels).unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&empty).unwrap()["customers"]["title"],
            "Customers"
        );
        assert!(merge_labels("[1]", "customers", &labels).is_err());
        assert!(merge_labels("{ nope", "customers", &labels).is_err());
    }

    #[test]
    fn what_the_user_may_not_do_is_hidden() {
        let (files, _) = render_with(
            &customer(),
            &names("Customer"),
            "../../bindings",
            false,
            Some("/api/customers"),
            true,
        );
        let view = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let index = view("Index.svelte");
        for needle in [
            "import { can, confirm, connection, href, outbox, toast } from \"@elyra/runtime\";",
            "{#if $can(\"customers.create\")}<a class=\"btn primary\" href={href(\"/customers/new\")}>",
            "{#if row.can?.delete ?? $can(\"customers.delete\")}<button class=\"btn danger\"",
        ] {
            assert!(index.contains(needle), "missing {needle:?} in:\n{index}");
        }
        let show = view("Show.svelte");
        for needle in [
            "import { can, confirm, href, navigate, toast } from \"@elyra/runtime\";",
            "{#if record.can?.update ?? $can(\"customers.update\")}<a class=\"btn primary\"",
            "{#if record.can?.delete ?? $can(\"customers.delete\")}<button class=\"btn danger\"",
        ] {
            assert!(show.contains(needle), "missing {needle:?} in:\n{show}");
        }
        // Without abilities, or without a backend, nothing is hidden.
        let (plain, _) = render_with(
            &customer(),
            &names("Customer"),
            "../../bindings",
            false,
            Some("/api/customers"),
            false,
        );
        assert!(plain.iter().all(|(_, text)| !text.contains("$can(")));
    }

    #[test]
    fn a_file_field_has_a_picker_and_a_download() {
        let specs: Vec<String> = ["number:string", "pdf:file?", "receipt:file?"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m = crate::resource_generate::model_from_fields(
            "Invoice",
            &specs,
            Path::new("/nowhere/src"),
        )
        .unwrap();
        let (files, _) = render(
            &m,
            &names("Invoice"),
            "../../bindings",
            false,
            Some("/api/invoices"),
        );
        let view = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let form = view("Form.svelte");
        for needle in [
            "import FileField from \"../FileField.svelte\";",
            "let files = $state({ pdf: null, receipt: null });",
            "<FileField label={\"Pdf\"} bind:file={files.pdf} current={current.pdf} error={errors.pdf?.[0]} acceptsDrop />",
            "<FileField label={\"Receipt\"} bind:file={files.receipt} current={current.receipt} error={errors.receipt?.[0]} />",
            "await api.invoices_store(input, files.pdf ?? null, files.receipt ?? null)",
            "await api.invoices_update(id, input, files.pdf ?? null, files.receipt ?? null)",
            "current = { pdf: record.pdf, receipt: record.receipt };",
        ] {
            assert!(form.contains(needle), "missing {needle:?} in:\n{form}");
        }
        assert!(!form.contains("form.pdf"), "a file isn't in the JSON input");
        let show = view("Show.svelte");
        for needle in [
            "import { confirm, dialog, href, navigate, toast } from \"@elyra/runtime\";",
            "const downloads = { pdf: api.invoices_pdf, receipt: api.invoices_receipt };",
            "onclick={() => download(\"pdf\")}",
            "dialog.save({ defaultName: record[field] ?? undefined })",
        ] {
            assert!(show.contains(needle), "missing {needle:?} in:\n{show}");
        }
        assert!(FILE_FIELD.contains("onFileDrop") && FILE_FIELD.contains("dialog.open"));
    }

    #[test]
    fn views_over_the_backend_work_offline() {
        let (files, labels) = render(
            &customer(),
            &names("Customer"),
            "../../bindings",
            false,
            Some("/api/customers"),
        );
        let view = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let index = view("Index.svelte");
        for needle in [
            "import { confirm, connection, href, outbox, toast } from \"@elyra/runtime\";",
            "const API = \"/api/customers\";",
            "{#if $connection.state === \"offline\"}",
            "outbox.keepMine(w.id)",
            "outbox.takeTheirs(w.id)",
            "outbox.discard(w.id)",
            "if (e?.kind === \"queued\")",
            "{waiting.length} change(s) waiting to be sent",
        ] {
            assert!(index.contains(needle), "missing {needle:?} in:\n{index}");
        }
        let form = view("Form.svelte");
        for needle in [
            "let { params = {}, query = {} } = $props();",
            "const write = $derived(query.write ? Number(query.write) : null);",
            "form = fromRecord({ ...blank(), ...w.body });",
            "errors = w.errors ?? {};",
            "await outbox.retry(write, input);",
            "if (e?.kind === \"queued\") {",
        ] {
            assert!(form.contains(needle), "missing {needle:?} in:\n{form}");
        }
        assert!(view("Show.svelte").contains("if (e?.kind === \"queued\") {"));
        assert!(labels.iter().any(|(k, _)| k == "keep_mine"));

        // A local resource has none of it.
        let (local, _) = render(
            &customer(),
            &names("Customer"),
            "../../bindings",
            false,
            None,
        );
        assert!(local
            .iter()
            .all(|(_, text)| !text.contains("outbox") && !text.contains("queued")));
    }
}
