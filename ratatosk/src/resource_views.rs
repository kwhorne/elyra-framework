//! `rata make:resource <Model> --view` — the Svelte half of a resource (RFC
//! 0001 step 4): a list, a form for create and edit, and a detail view, over the
//! typed `api.*` that codegen emits for the resource's commands.
//!
//! The views use only what ships — the scaffold's CSS variables and `.btn` /
//! `.card`, the runtime's router, `confirm`, `toast` and `validationErrors` — so
//! they match the theme and need no new dependency. Labels go through `$t` when
//! the project has `lang/en.json`, whose English defaults this merges in.

use std::path::{Component, Path, PathBuf};

use crate::make_resource::{Field, Kind, ModelInfo, Names};

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
fn humanize(name: &str) -> String {
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

pub(crate) fn render_index(m: &ModelInfo, n: &Names, bindings: &str, l: &mut Labels) -> String {
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
  import {{ confirm, href, toast{t} }} from "@elyra/runtime";
  import {{ api }} from "{bindings}";

  const PER_PAGE = {PER_PAGE};

  let search = $state("");
  let term = $state("");
  let sort = $state("{pk}");
  let direction = $state("asc");
  let page = $state(1);
  let version = $state(0);
  let result = $state(null);
  let error = $state("");
  let seq = 0;

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

  // Reload whenever the query changes. `seq` drops answers that arrive late.
  $effect(() => {{
    const query = {{ search: term || null, sort, direction, page, per_page: PER_PAGE }};
    void version;
    load(query);
  }});

  async function load(query) {{
    const mine = ++seq;
    try {{
      const next = await api.{p}_index(query);
      if (mine === seq) {{
        result = next;
        error = "";
      }}
    }} catch (e) {{
      if (mine === seq) error = message(e);
    }}
  }}

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
      version++;
    }} catch (e) {{
      toast(message(e), {{ variant: "error" }});
    }}
  }}

  const message = (e) => e?.detail ?? e?.message ?? String(e);
  const from = $derived(result?.data.length ? (result.current_page - 1) * result.per_page + 1 : 0);
  const to = $derived(result?.data.length ? from + result.data.length - 1 : 0);
</script>

<div class="resource">
  <header class="head">
    <h2>{title}</h2>
    <input class="search" type="search" placeholder={{{search_ph}}} bind:value={{search}} />
    <a class="btn primary" href={{href("/{p}/new")}}>{new_label}</a>
  </header>

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
              <button class="btn danger" onclick={{() => remove(row)}}>{delete_m}</button>
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
</style>
"#,
        header = header("The list", n),
        t = l.import(),
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

/// The form's starting value for one field.
fn blank(f: &Field) -> &'static str {
    match f.kind {
        Kind::Text => "\"\"",
        Kind::Int | Kind::Float => "null",
        Kind::Bool => "false",
        Kind::Other => "\"null\"",
    }
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
    if has(Kind::Other) {
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

pub(crate) fn render_form(m: &ModelInfo, n: &Names, bindings: &str, l: &mut Labels) -> String {
    let p = &n.plural;
    let pk = &m.pk;
    let human = &n.human;
    let has_json = m.editable.iter().any(|f| f.kind == Kind::Other);

    let blanks: String = m
        .editable
        .iter()
        .map(|f| format!("      {}: {},\n", f.name, blank(f)))
        .collect();
    let from_record: String = m
        .editable
        .iter()
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

    let fields: String = m
        .editable
        .iter()
        .map(|f| {
            let label = field_label(l, f);
            let name = &f.name;
            let error = format!(
                "    {{#if errors.{name}}}<small class=\"error\">{{errors.{name}[0]}}</small>{{/if}}\n"
            );
            match f.kind {
                Kind::Bool => format!(
                    "  <label class=\"field check\">\n    <input type=\"checkbox\" bind:checked={{form.{name}}} />\n    <span>{label}</span>\n{error}  </label>\n"
                ),
                Kind::Other => format!(
                    "  <label class=\"field\">\n    <span>{label}</span>\n    <textarea rows=\"4\" bind:value={{form.{name}}} aria-invalid={{!!errors.{name}}}></textarea>\n{error}  </label>\n"
                ),
                kind => {
                    let attrs = match kind {
                        Kind::Int => " type=\"number\" step=\"1\"",
                        Kind::Float => " type=\"number\" step=\"any\"",
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

    format!(
        r#"{header}<script>
  import {{ href, navigate, toast, validationErrors{t} }} from "@elyra/runtime";
  import {{ api }} from "{bindings}";

  let {{ params = {{}} }} = $props();
  const id = $derived(params.id ? Number(params.id) : null);

  let form = $state(blank());
  let errors = $state({{}});
  let saving = $state(false);
  let loadError = $state("");

  function blank() {{
    return {{
{blanks}    }};
  }}

  // Editing: start from the record.
  $effect(() => {{
    errors = {{}};
    if (id === null) {{
      form = blank();
      return;
    }}
    api
      .{p}_show(id)
      .then((record) => (form = fromRecord(record)))
      .catch((e) => (loadError = message(e)));
  }});

  function fromRecord(record) {{
    return {{
{from_record}    }};
  }}

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
      const record = id === null ? await api.{p}_store(input) : await api.{p}_update(id, input);
      toast(id === null ? {created} : {saved}, {{ variant: "success" }});
      navigate(`/{p}/${{record.{pk}}}`);
    }} catch (e) {{
      const bag = validationErrors(e);
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

pub(crate) fn render_show(m: &ModelInfo, n: &Names, bindings: &str, l: &mut Labels) -> String {
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
            format!(
                "      <dt>{}</dt>\n      <dd>{}</dd>\n",
                field_label(l, f),
                display(f, "record", l)
            )
        })
        .collect();
    let confirm_delete = l.js("confirm_delete", &format!("Delete this {human}?"));
    let delete_js = l.js("delete", "Delete");
    let deleted = l.js("deleted", &format!("{} deleted", humanize(human)));
    let edit = l.m("edit_action", "Edit");
    let delete_m = l.m("delete", "Delete");
    let back = l.m("back", "Back");
    let loading = l.m("loading", "Loading…");

    format!(
        r#"{header}<script>
  import {{ confirm, href, navigate, toast{t} }} from "@elyra/runtime";
  import {{ api }} from "{bindings}";

  let {{ params }} = $props();
  const id = $derived(Number(params.id));

  let record = $state(null);
  let error = $state("");

  $effect(() => {{
    record = null;
    error = "";
    api
      .{p}_show(id)
      .then((found) => (record = found))
      .catch((e) => (error = message(e)));
  }});

  async function remove() {{
    const ok = await confirm({confirm_delete}, {{ danger: true, confirmLabel: {delete_js} }});
    if (!ok) return;
    try {{
      await api.{p}_destroy(id);
      toast({deleted}, {{ variant: "success" }});
      navigate("/{p}");
    }} catch (e) {{
      toast(message(e), {{ variant: "error" }});
    }}
  }}

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
      <a class="btn primary" href={{href(`/{p}/${{id}}/edit`)}}>{edit}</a>
      <button class="btn danger" onclick={{remove}}>{delete_m}</button>
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
</style>
"#,
        header = header("The detail view", n),
        t = l.import(),
    )
}

/// The view files, rendered, plus the labels they use.
pub(crate) fn render(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    i18n: bool,
) -> (Vec<(String, String)>, Vec<Label>) {
    let mut l = Labels::new(&n.plural, i18n);
    let files = vec![
        ("index.js".to_string(), render_index_js(n)),
        (
            "Index.svelte".to_string(),
            render_index(m, n, bindings, &mut l),
        ),
        (
            "Form.svelte".to_string(),
            render_form(m, n, bindings, &mut l),
        ),
        (
            "Show.svelte".to_string(),
            render_show(m, n, bindings, &mut l),
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
        let (files, labels) = render(&m, &n, "../../bindings", false);
        let get = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;

        let index = get("Index.svelte");
        assert!(index.contains("api.customers_index(query)"));
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
        let (files, labels) = render(&customer(), &names("Customer"), "../../bindings", true);
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
}
