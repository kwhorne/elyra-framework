//! `rata make:resource <Model> --view` — the Svelte half of a resource (RFC
//! 0001 step 4): a list, a form for create and edit, and a detail view, over the
//! typed `api.*` that codegen emits for the resource's commands.
//!
//! The views use only what ships — the scaffold's CSS variables and `.btn` /
//! `.card`, the runtime's router, `confirm`, `toast` and `validationErrors` — so
//! they match the theme and need no new dependency. Labels go through `$t` when
//! the project has `lang/en.json`, whose English defaults this merges in.

use std::path::{Component, Path, PathBuf};

use crate::make_resource::{Field, Format, Kind, ModelInfo, Names, ParentView};

/// A reference whose parent has its own resource: the form picks it in a
/// Combobox, the detail view links to it.
fn parent(f: &Field) -> Option<(&ParentView, &str)> {
    let r = f.references.as_ref()?;
    Some((r.view.as_ref()?, r.pk.as_str()))
}

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

    /// In script, a function of one value filling `:param`:
    /// `(v) => $t("ns.key", { param: v })`, or a template literal.
    fn js_fn(&mut self, key: &str, en: &str, param: &str) -> String {
        self.remember(key, en);
        if self.i18n {
            format!("(v) => $t(\"{}.{key}\", {{ {param}: v }})", self.ns)
        } else {
            let template = en
                .replace('\\', "\\\\")
                .replace('`', "\\`")
                .replace(&format!(":{param}"), "${v}");
            format!("(v) => `{template}`")
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

/// The field a record is called by: its first text field that's always
/// there, as the detail view's title.
fn title_field(m: &ModelInfo) -> Option<&Field> {
    m.editable
        .iter()
        .find(|f| f.kind == Kind::Text && !f.nullable && !f.is_file())
}

pub(crate) fn render_index_js(m: &ModelInfo, n: &Names, bindings: &str, gate: bool) -> String {
    let p = &n.plural;
    let route = format!("/{p}");
    // The ⌘K palette finds a record by what the list searches, when it has
    // a name to show it by.
    let (imports, search) = match title_field(m) {
        None => (String::new(), String::new()),
        Some(f) => {
            let can = if gate {
                format!("\n  can: \"{p}.view\",")
            } else {
                String::new()
            };
            (
                format!(
                    "import {{ navigate }} from \"@elyra/runtime\";\nimport {{ api }} from \"{bindings}\";\n\n"
                ),
                format!(
                    r#"
/** The ⌘K palette finds {plural_human} by name, as the list searches. */
export const search = {{
  id: "{p}",
  title: "{title}",{can}
  search: async (query) => {{
    const page = await api.{p}_index({{ search: query, per_page: 5 }});
    return (Array.isArray(page) ? page : (page?.data ?? [])).map((r) => ({{
      title: String(r.{name}),
      subtitle: "{human_cap}",
      action: () => navigate(`/{p}/${{r.{pk}}}`),
    }}));
  }},
}};
"#,
                    plural_human = humanize(p).to_lowercase(),
                    title = humanize(p),
                    name = f.name,
                    human_cap = humanize(&n.human),
                    pk = m.pk,
                ),
            )
        }
    };
    format!(
        r#"// The {human} pages — generated by `rata make:resource {ty} --view`, and
// yours to change. rata lists this folder in ../index.js.

{imports}export const routes = {{
  "{route}": () => import("./Index.svelte"),
  "{route}/new": () => import("./Form.svelte"),
  "{route}/:id": () => import("./Show.svelte"),
  "{route}/:id/edit": () => import("./Form.svelte"),
}};

export const nav = [{{ label: "{title}", path: "{route}" }}];
{search}"#,
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
    let title_js = l.js("title", &humanize(p));
    let search_ph = l.js("search", "Search…");
    let new_label = l.m("new", &format!("New {human}"));
    let failed = l.js(
        "failed",
        &format!("The {} couldn't be loaded", p.replace('_', " ")),
    );
    let no_match = l.js(
        "no_match",
        &format!("No {} match your search.", n.plural.replace('_', " ")),
    );
    let empty = l.js("empty", &format!("No {} yet.", n.plural.replace('_', " ")));
    let delete_m = l.m("delete", "Delete");
    let total = l.js_fn("total", ":total in all", "total");
    let previous = l.js("previous", "Previous");
    let next = l.js("next", "Next");
    let page_label = l.js_fn("page", "Page :page", "page");
    let offline = offline_index(l, n, backend);

    // The columns are the table's: a column sorts by itself, as the command
    // allows; a cell shows the field.
    let columns: String = if shown.is_empty() {
        format!("    {{ key: \"{pk}\", label: \"#\" }},\n")
    } else {
        shown
            .iter()
            .map(|f| {
                let label = l.js(&format!("fields.{}", f.name), &humanize(&f.name));
                format!("    {{ key: \"{}\", label: {label} }},\n", f.column)
            })
            .collect()
    };
    let link = |value: &str| format!("<a href={{href(`/{p}/${{row.{pk}}}`)}}>{value}</a>");
    let cells: String = if shown.is_empty() {
        format!("      {}\n", link(&format!("#{{row.{pk}}}")))
    } else {
        let mut out = String::new();
        for (i, f) in shown.iter().enumerate() {
            let value = display(f, "row", l);
            let value = if i == 0 { link(&value) } else { value };
            let keyword = if i == 0 { "#if" } else { ":else if" };
            out.push_str(&format!(
                "      {{{keyword} col.key === \"{}\"}}{value}\n",
                f.column
            ));
        }
        out.push_str("      {/if}\n");
        out
    };

    format!(
        r#"{header}<script>
  import {{ {can_import}{offline_imports}href, navigate, {offline_outbox}toast{t} }} from "@elyra/runtime";
  import {{ Button, {offline_ui}Empty, Table }} from "@elyra/ui";
  import {{ api, live }} from "{bindings}";

  // What the list asks for: the Table writes the search, the sort and the
  // page here, and the list runs again. It's live too: Rust re-runs it when
  // a write — in this window or any other — changes what it shows.
  let query = $state({{ search: null, sort: "{pk}", direction: "asc", page: 1, per_page: {PER_PAGE} }});
  const list = $derived(live.{p}_index(query));

  const columns = $derived([
{columns}  ]);

  async function remove(row) {{
    try {{
      await api.{p}_destroy(row.{pk});
      toast({deleted}, {{ variant: "success" }});
    }} catch (e) {{
{remove_catch}    }}
  }}

  const message = (e) => e?.detail ?? e?.message ?? String(e);
{offline_script}</script>

<div class="resource">
  <div class="head">
    <h2>{title}</h2>
    <Button variant="primary" href="/{p}/new"{can_create}>{new_label}</Button>
  </div>
{offline_markup}
  <Table
    page={{list}}
    bind:query
    {{columns}}
    sortable
    search
    label={{{title_js}}}
    onrowclick={{(row) => navigate(`/{p}/${{row.{pk}}}`)}}
    labels={{{{ search: {search_ph}, failed: {failed}, total: {total}, previous: {previous}, next: {next}, page: {page_label} }}}}
  >
    {{#snippet cell(row, col)}}
{cells}    {{/snippet}}
    {{#snippet empty()}}
      <Empty title={{query.search ? {no_match} : {empty}}} />
    {{/snippet}}
    {{#snippet actions(row)}}
      {row_delete_open}<Button size="sm" variant="danger" confirm={{{confirm_delete}}} confirmLabel={{{delete_js}}} onclick={{() => remove(row)}}>{delete_m}</Button>{row_delete_close}
    {{/snippet}}
  </Table>
</div>

<style>
  .resource {{ max-width: 960px; display: grid; gap: 14px; }}
  .head {{ display: flex; align-items: center; gap: 12px; }}
  .head h2 {{ flex: 1; margin: 0; font-size: 18px; font-weight: 600; }}
  a {{ color: var(--elyra-accent); text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
{offline_css}</style>
"#,
        header = header("The list", n),
        t = l.import(),
        // What the user may do, by Laravel (RFC 0010): what they can't is hidden.
        can_import = if gate { "can, " } else { "" },
        can_create = if gate {
            format!(" can=\"{p}.create\"")
        } else {
            String::new()
        },
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
        offline_ui = if backend.is_some() { "Callout, " } else { "" },
        offline_script = offline.script,
        offline_markup = offline.markup,
        offline_css = offline.css,
        remove_catch = offline.remove_catch,
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
  function fixPath(w) {{
    const id = w.path.split("?")[0].slice(API.length + 1);
    return id ? `/{p}/${{id}}/edit?write=${{w.id}}` : `/{p}/new?write=${{w.id}}`;
  }}
"#
        ),
        markup: format!(
            r#"
  {{#if $connection.state === "offline"}}
    <Callout>{banner}</Callout>
  {{/if}}
  {{#if waiting.length}}
    <p class="dim">{waiting}</p>
  {{/if}}
  {{#each refused as w (w.id)}}
    <Callout variant="danger">
      <div class="refused">
        <span>{{w.error}}</span>
        {{#if w.reason === "conflict"}}
          <Button size="sm" onclick={{() => outbox.keepMine(w.id)}}>{keep}</Button>
          <Button size="sm" onclick={{() => outbox.takeTheirs(w.id)}}>{take}</Button>
        {{:else if w.method !== "DELETE"}}
          <Button size="sm" href={{fixPath(w)}}>{fix}</Button>
        {{/if}}
        <Button size="sm" variant="ghost" onclick={{() => outbox.discard(w.id)}}>{discard}</Button>
      </div>
    </Callout>
  {{/each}}
"#
        ),
        css: "  .dim { margin: 0; color: var(--elyra-muted); }\n  .refused { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }\n  .refused span { flex: 1; }\n",
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

pub(crate) fn render_form(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    l: &mut Labels,
    backend: Option<&str>,
    presence: bool,
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
    let (file_state, file_current) = if file_fields.is_empty() {
        (String::new(), String::new())
    } else {
        let names = |value: &str| -> String {
            file_fields
                .iter()
                .map(|f| format!("{}: {value}", f.name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        (
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
            let label_js = js_label(&label);
            let name = &f.name;
            if f.is_file() {
                // The first file field also takes a file dropped on the window.
                let drop = if file_fields.first().is_some_and(|first| first.name == f.name) {
                    " acceptsDrop"
                } else {
                    ""
                };
                return format!(
                    "    <Field name=\"{name}\" label={{{label_js}}}><FileField bind:file={{files.{name}}} current={{current.{name}}}{drop} /></Field>\n"
                );
            }
            let required = if crate::make_resource::is_required(f) { " required" } else { "" };
            if let Some((view, pk)) = parent(f) {
                return format!(
                    "    <Field name=\"{name}\" label={{{label_js}}}{required}>\n      <Combobox bind:value={{form.{name}}} bind:label={{names.{name}}} options={{parents.{name}}} optionValue=\"{pk}\" optionLabel=\"{label}\" />\n    </Field>\n",
                    label = view.label,
                );
            }
            match f.kind {
                Kind::Bool => format!(
                    "    <Field name=\"{name}\"><Checkbox bind:checked={{form.{name}}}>{label}</Checkbox></Field>\n"
                ),
                _ if is_textarea(f) => format!(
                    "    <Field name=\"{name}\" label={{{label_js}}}{required}><Textarea rows={{4}} bind:value={{form.{name}}} /></Field>\n"
                ),
                // An ISO date, or null; the form keeps "" for none, as the
                // other text fields do, so a required one says so.
                _ if f.format == Format::Date => format!(
                    "    <Field name=\"{name}\" label={{{label_js}}}{required}><DatePicker bind:value={{() => form.{name} || null, (v) => (form.{name} = v ?? \"\")}} /></Field>\n"
                ),
                kind => {
                    let attrs = match (kind, f.format) {
                        (_, Format::Email) => " type=\"email\"",
                        (Kind::Int, _) => " type=\"number\" step=\"1\"",
                        (Kind::Float, _) => " type=\"number\" step=\"any\"",
                        _ => "",
                    };
                    format!(
                        "    <Field name=\"{name}\" label={{{label_js}}}{required}><Input{attrs} bind:value={{form.{name}}} /></Field>\n"
                    )
                }
            }
        })
        .collect();
    let has = |kind: Kind| m.editable.iter().any(|f| f.kind == kind && !f.is_file());
    let mut controls = Vec::new();
    if has(Kind::Bool) {
        controls.push("Checkbox");
    }
    let is_date = |f: &Field| f.format == Format::Date && f.kind != Kind::Bool && !is_textarea(f);
    if m.editable
        .iter()
        .any(|f| !f.is_file() && is_date(f) && parent(f).is_none())
    {
        controls.push("DatePicker");
    }
    if m.editable.iter().any(|f| {
        !f.is_file()
            && f.kind != Kind::Bool
            && !is_textarea(f)
            && parent(f).is_none()
            && !is_date(f)
    }) {
        controls.push("Input");
    }
    let parents: Vec<(&Field, &ParentView)> = m
        .editable
        .iter()
        .filter_map(|f| parent(f).map(|(view, _)| (f, view)))
        .collect();
    if !parents.is_empty() {
        controls.push("Combobox");
    }
    // The parents' searches, and their names for the record being edited.
    let (parent_state, parent_names) = if parents.is_empty() {
        (String::new(), String::new())
    } else {
        let searches = parents
            .iter()
            .map(|(f, v)| {
                format!(
                    "    {}: (search) => api.{}_index({{ search, per_page: 20 }}),\n",
                    f.name, v.plural
                )
            })
            .collect::<String>();
        let empty = parents
            .iter()
            .map(|(f, _)| format!("{}: \"\"", f.name))
            .collect::<Vec<_>>()
            .join(", ");
        // Its id until its name comes, and the id if it can't be read.
        let ids = parents
            .iter()
            .map(|(f, _)| {
                format!(
                    "{0}: record.{0} == null ? \"\" : `#${{record.{0}}}`",
                    f.name
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let fetches = parents
            .iter()
            .map(|(f, v)| {
                format!(
                    "\n        if (record.{0} != null)\n          api.{1}_show(record.{0}).then((p) => (names.{0} = p.{2})).catch(() => {{}});",
                    f.name, v.plural, v.label
                )
            })
            .collect::<String>();
        (
            format!(
                "\n  // A reference is picked from its parent's own list, searched as it's typed.\n  const parents = {{\n{searches}  }};\n  // The parents' names, shown in place of their ids; a pick sets its own.\n  let names = $state({{ {empty} }});\n"
            ),
            format!("\n        names = {{ {ids} }};{fetches}"),
        )
    };
    if m.editable.iter().any(|f| !f.is_file() && is_textarea(f)) {
        controls.push("Textarea");
    }
    if !file_fields.is_empty() {
        controls.push("FileField");
    }
    let mut ui = vec!["Button", "Callout", "Card", "Field", "Form"];
    if presence {
        ui.push("Presence");
    }
    ui.extend(controls);
    ui.sort_unstable();
    let ui = ui.join(", ");

    let created = l.js("created", &format!("{} created", humanize(human)));
    let saved = l.js("saved", &format!("{} saved", humanize(human)));
    let new_title = l.js("new", &format!("New {human}"));
    let edit_title = l.js("edit", &format!("Edit {human}"));
    let failed_load = l.js("failed_load", &format!("The {human} couldn't be loaded"));
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
  import {{ navigate, {form_imports}toast{t} }} from "@elyra/runtime";
  import {{ {ui} }} from "@elyra/ui";
  import {{ api }} from "{bindings}";

  {form_props}

  let form = $state(blank());{file_state}{parent_state}
  // The fields' messages: the Form fills them from Laravel's 422 (or the
  // command's ValidationErrors), and a field shows its own.
  let errors = $state({{}});
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
        form = fromRecord(record);{file_current}{parent_names}
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
  // The Form runs this, and shows what it throws: a validation failure under
  // its fields, anything else at the top.
  async function save() {{
{json_check}    try {{
{resend}      const record =
        id === null ? await api.{p}_store(input{file_args}) : await api.{p}_update(id, input{file_args});
      toast(id === null ? {created} : {saved}, {{ variant: "success" }});
      navigate(`/{p}/${{record.{pk}}}`);
    }} catch (e) {{
{queued_catch}      throw e;
    }}
  }}

  const message = (e) => e?.detail ?? e?.message ?? String(e);
</script>

<Card title={{id === null ? {new_title} : {edit_title}}}>
{presence_actions}  {{#if loadError}}<Callout variant="danger" title={{{failed_load}}}>{{loadError}}</Callout>{{/if}}
  <Form onsubmit={{save}} bind:errors>
{fields}    <div class="actions">
      <Button type="submit" variant="primary">{save}</Button>
      <Button href={{id === null ? "/{p}" : `/{p}/${{id}}`}}>{cancel}</Button>
    </div>
  </Form>
</Card>

<style>
  .actions {{ display: flex; gap: 8px; }}
</style>
"#,
        header = header("The form, for create and edit", n),
        // Editing: who else has it open, before both save.
        presence_actions = if presence {
            format!("  {{#snippet actions()}}{{#if id !== null}}<Presence channel={{`{p}.${{id}}`}} />{{/if}}{{/snippet}}\n")
        } else {
            String::new()
        },
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
    presence: bool,
) -> String {
    let p = &n.plural;
    let pk = &m.pk;
    let human = &n.human;
    // The card's title, as a JavaScript expression.
    let heading = match title_field(m) {
        Some(f) => format!("record.{}", f.name),
        None => format!("`{} #${{record.{pk}}}`", humanize(human)),
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
            } else if let Some((view, _)) = parent(f) {
                // A parent with its own resource: its name, which opens it.
                format!(
                    "{{#if record.{0} == null}}—{{:else}}<a href={{href(`/{1}/${{record.{0}}}`)}}>{{names.{0} ?? `#${{record.{0}}}`}}</a>{{/if}}",
                    f.name, view.plural
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
            "  .link { background: none; border: 0; padding: 0; color: var(--elyra-accent); cursor: pointer; text-decoration: underline; font: inherit; }\n",
        )
    };
    // Each parent with its own resource, live: a rename there shows here.
    // Followed by its id, so an edit of the record doesn't reopen it, and
    // not while the record itself is gone.
    let parent_stores: String = m
        .editable
        .iter()
        .filter_map(|f| parent(f).map(|(view, _)| (f, view)))
        .map(|(f, view)| {
            format!(
                "  const {0} = $derived(error ? null : (record?.{0} ?? null));\n  $effect(() => {{\n    if ({0} === null) return;\n    return live.{1}_show({0}).subscribe((s) => (names.{0} = s.value?.{2}));\n  }});\n",
                f.name, view.plural, view.label
            )
        })
        .collect();
    let href_import = if parent_stores.is_empty() {
        ""
    } else {
        "href, "
    };
    let parent_stores = if parent_stores.is_empty() {
        String::new()
    } else {
        format!("\n  // The parents' names, from their own live records.\n  let names = $state({{}});\n{parent_stores}")
    };
    let confirm_delete = l.js("confirm_delete", &format!("Delete this {human}?"));
    let delete_js = l.js("delete", "Delete");
    let deleted = l.js("deleted", &format!("{} deleted", humanize(human)));
    let edit = l.m("edit_action", "Edit");
    let delete_m = l.m("delete", "Delete");
    let back = l.m("back", "Back");
    let loading = l.js("loading", "Loading…");
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
  import {{ {can_import}{show_import}{href_import}navigate, toast{t} }} from "@elyra/runtime";
  import {{ Button, Callout, Card, {presence_import}Spinner }} from "@elyra/ui";
  import {{ api, live }} from "{bindings}";

  let {{ params }} = $props();
  const id = $derived(Number(params.id));

  // Live: an edit in another window shows up here, and a delete there turns
  // this into "not found".
  const current = $derived(live.{p}_show(id));
  const record = $derived($current.value ?? null);
  const error = $derived($current.error ? message($current.error) : "");
{parent_stores}
  // The Delete button has asked already.
  async function remove() {{
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
  <div class="missing">
    <Callout variant="danger">{{error}}</Callout>
    <div><Button href="/{p}">{back}</Button></div>
  </div>
{{:else if record}}
  <Card title={{{heading}}}>
{presence_actions}    <dl>
{rows}    </dl>
    {{#snippet footer()}}
      {edit_open}<Button variant="primary" href={{`/{p}/${{id}}/edit`}}>{edit}</Button>{edit_close}
      {delete_open}<Button variant="danger" confirm={{{confirm_delete}}} confirmLabel={{{delete_js}}} onclick={{remove}}>{delete_m}</Button>{delete_close}
      <Button href="/{p}">{back}</Button>
    {{/snippet}}
  </Card>
{{:else}}
  <Spinner label={{{loading}}} />
{{/if}}

<style>
  dl {{ display: grid; grid-template-columns: max-content 1fr; gap: 8px 16px; margin: 0; }}
  dt {{ color: var(--elyra-muted); }}
  dd {{ margin: 0; min-width: 0; overflow-wrap: anywhere; }}
  .missing {{ display: grid; gap: 12px; max-width: 520px; }}
{link_css}</style>
"#,
        header = header("The detail view", n),
        can_import = if gate { "can, " } else { "" },
        presence_import = if presence { "Presence, " } else { "" },
        // Who else has it open, on the web or another desktop.
        presence_actions = if presence {
            format!(
                "    {{#snippet actions()}}<Presence channel={{`{p}.${{id}}`}} />{{/snippet}}\n"
            )
        } else {
            String::new()
        },
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
    render_with(m, n, bindings, i18n, backend, false, false)
}

/// The views; `gate` hides what the signed-in user may not do, by the
/// backend's abilities (RFC 0010), for a resource with abilities over it;
/// `presence` shows who else has a record open (RFC 0015).
pub(crate) fn render_with(
    m: &ModelInfo,
    n: &Names,
    bindings: &str,
    i18n: bool,
    backend: Option<&str>,
    gate: bool,
    presence: bool,
) -> (Vec<(String, String)>, Vec<Label>) {
    let mut l = Labels::new(&n.plural, i18n);
    let files = vec![
        (
            "index.js".to_string(),
            render_index_js(m, n, bindings, gate),
        ),
        (
            "Index.svelte".to_string(),
            render_index(m, n, bindings, &mut l, backend, gate),
        ),
        (
            "Form.svelte".to_string(),
            render_form(m, n, bindings, &mut l, backend, presence),
        ),
        (
            "Show.svelte".to_string(),
            render_show(m, n, bindings, &mut l, backend, gate, presence),
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
    fn the_palette_finds_a_record_by_its_name() {
        let m = customer();
        let n = names("Customer");
        let (files, _) = render(&m, &n, "../../bindings", false, None);
        let index = &files.iter().find(|(f, _)| f == "index.js").unwrap().1;
        for needle in [
            "import { navigate } from \"@elyra/runtime\";\nimport { api } from \"../../bindings\";\n\nexport const routes = {",
            "export const search = {\n  id: \"customers\",\n  title: \"Customers\",\n  search: async (query) => {",
            "const page = await api.customers_index({ search: query, per_page: 5 });",
            "title: String(r.name),",
            "subtitle: \"Customer\",",
            "action: () => navigate(`/customers/${r.id}`),",
        ] {
            assert!(index.contains(needle), "{needle}\n---\n{index}");
        }
        assert!(!index.contains("can:"), "only gated views ask can");

        // Gated by the backend's abilities: only for who may see them.
        let (files, _) = render_with(
            &m,
            &n,
            "../../bindings",
            false,
            Some("/api/customers"),
            true,
            false,
        );
        let index = &files.iter().find(|(f, _)| f == "index.js").unwrap().1;
        assert!(
            index.contains("  title: \"Customers\",\n  can: \"customers.view\",\n"),
            "{index}"
        );

        // Nothing to call a record by: no search.
        let specs: Vec<String> = vec!["amount:float".into(), "note:string?".into()];
        let m = crate::resource_generate::model_from_fields(
            "Payment",
            &specs,
            Path::new("/nowhere/src"),
        )
        .unwrap();
        let (files, _) = render(&m, &names("Payment"), "../../bindings", false, None);
        let index = &files.iter().find(|(f, _)| f == "index.js").unwrap().1;
        assert!(!index.contains("search"), "{index}");
        assert!(!index.contains("import {"), "{index}");
        assert!(
            index.contains("export const nav = [{ label: \"Payments\", path: \"/payments\" }];")
        );
    }

    #[test]
    fn with_presence_the_detail_and_edit_views_show_who_else_is_there() {
        let m = customer();
        let n = names("Customer");
        let (files, _) = render_with(
            &m,
            &n,
            "../../bindings",
            false,
            Some("/api/customers"),
            false,
            true,
        );
        let get = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let show = get("Show.svelte");
        assert!(
            show.contains(
                "import { Button, Callout, Card, Presence, Spinner } from \"@elyra/ui\";"
            ),
            "{show}"
        );
        assert!(
            show.contains("  <Card title={record.name}>\n    {#snippet actions()}<Presence channel={`customers.${id}`} />{/snippet}\n    <dl>"),
            "{show}"
        );
        let form = get("Form.svelte");
        assert!(
            form.contains(", Presence, ") || form.contains("Presence, "),
            "{form}"
        );
        assert!(
            form.contains("{#snippet actions()}{#if id !== null}<Presence channel={`customers.${id}`} />{/if}{/snippet}"),
            "only when editing\n{form}"
        );

        // Without it: neither.
        let (files, _) = render_with(
            &m,
            &n,
            "../../bindings",
            false,
            Some("/api/customers"),
            false,
            false,
        );
        for (_, text) in &files {
            assert!(!text.contains("Presence"), "{text}");
        }
    }

    #[test]
    fn views_follow_the_fields() {
        let m = customer();
        let n = names("Customer");
        let (files, labels) = render(&m, &n, "../../bindings", false, None);
        let get = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;

        let index = get("Index.svelte");
        assert!(index.contains("import { Button, Empty, Table } from \"@elyra/ui\";"));
        assert!(index.contains("const list = $derived(live.customers_index(query));"));
        assert!(index.contains("<Table\n    page={list}\n    bind:query"));
        assert!(
            !index.contains("<style>\n  table"),
            "the table is the component's"
        );
        assert!(
            !index.contains("version"),
            "nothing reloads a live list by hand"
        );
        let show = &files.iter().find(|(f, _)| f == "Show.svelte").unwrap().1;
        assert!(show.contains("const current = $derived(live.customers_show(id));"));
        assert!(
            index.contains("{ key: \"email_address\", label: \"Email\" },"),
            "sorts by column, not field"
        );
        assert!(index.contains("{:else if col.key === \"email_address\"}{row.email}"));
        assert!(index.contains(
            "{#if col.key === \"name\"}<a href={href(`/customers/${row.id}`)}>{row.name}</a>"
        ));
        assert!(index.contains("{:else if col.key === \"phone\"}{row.phone ?? \"—\"}"));
        assert!(index.contains("total: (v) => `${v} in all`"));
        assert!(
            !index.contains("JSON.stringify(row.tags)"),
            "JSON fields stay out of the table"
        );
        assert!(!index.contains("$t("));

        let form = get("Form.svelte");
        assert!(form.contains("import { Button, Callout, Card, Checkbox, Field, Form, Input, Textarea } from \"@elyra/ui\";"));
        assert!(form.contains("<Field name=\"credit\" label={\"Credit\"} required><Input type=\"number\" step=\"any\" bind:value={form.credit} /></Field>"));
        assert!(form.contains("<Field name=\"visits\" label={\"Visits\"} required><Input type=\"number\" step=\"1\" bind:value={form.visits} /></Field>"));
        assert!(form.contains(
            "<Field name=\"phone\" label={\"Phone\"}><Input bind:value={form.phone} /></Field>"
        ));
        assert!(form.contains(
            "<Field name=\"active\"><Checkbox bind:checked={form.active}>Active</Checkbox></Field>"
        ));
        assert!(form.contains("<Textarea rows={4} bind:value={form.tags} />"));
        assert!(form.contains("phone: form.phone === \"\" ? null : form.phone,"));
        assert!(form.contains("tags: json(\"tags\", form.tags),"));
        assert!(form.contains("<Form onsubmit={save} bind:errors>"));
        assert!(
            form.contains("      throw e;\n"),
            "the Form shows what save throws"
        );
        assert!(!form.contains("validationErrors"), "the Form reads the bag");

        let show = get("Show.svelte");
        assert!(show.contains("<Card title={record.name}>"));
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
        assert!(form.contains("import { navigate, toast, t } from \"@elyra/runtime\";"));
        assert!(form
            .contains("<Field name=\"credit\" label={$t(\"customers.fields.credit\")} required>"));
        let index = &files.iter().find(|(f, _)| f == "Index.svelte").unwrap().1;
        assert!(index.contains("total: (v) => $t(\"customers.total\", { total: v })"));
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
            false,
        );
        let view = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let index = view("Index.svelte");
        for needle in [
            "import { can, connection, href, navigate, outbox, toast } from \"@elyra/runtime\";",
            "<Button variant=\"primary\" href=\"/customers/new\" can=\"customers.create\">",
            "{#if row.can?.delete ?? $can(\"customers.delete\")}<Button size=\"sm\" variant=\"danger\"",
        ] {
            assert!(index.contains(needle), "missing {needle:?} in:\n{index}");
        }
        let show = view("Show.svelte");
        for needle in [
            "import { can, navigate, toast } from \"@elyra/runtime\";",
            "{#if record.can?.update ?? $can(\"customers.update\")}<Button variant=\"primary\"",
            "{#if record.can?.delete ?? $can(\"customers.delete\")}<Button variant=\"danger\"",
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
            false,
        );
        assert!(plain
            .iter()
            .all(|(_, text)| !text.contains("$can(") && !text.contains(" can=\"")));
    }

    /// An Invoice with a `team_id` whose Team has a resource (`view`), or not.
    fn invoice_with_team(view: bool) -> ModelInfo {
        let specs: Vec<String> = ["number:string"].iter().map(|s| s.to_string()).collect();
        let mut m = crate::resource_generate::model_from_fields(
            "Invoice",
            &specs,
            Path::new("/nowhere/src"),
        )
        .unwrap();
        m.editable.push(Field {
            name: "team_id".into(),
            column: "team_id".into(),
            kind: Kind::Int,
            nullable: true,
            ty: "i64".into(),
            format: Format::Reference,
            unique: false,
            index: false,
            default: None,
            references: Some(crate::make_resource::Reference {
                model: "Team".into(),
                module: "crate::resources::team".into(),
                table: "teams".into(),
                pk: "id".into(),
                mirror: None,
                view: view.then(|| ParentView {
                    plural: "teams".into(),
                    label: "name".into(),
                }),
            }),
        });
        m
    }

    #[test]
    fn a_reference_with_a_resource_is_picked_and_linked() {
        let (files, _) = render(
            &invoice_with_team(true),
            &names("Invoice"),
            "../../bindings",
            false,
            None,
        );
        let view = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let form = view("Form.svelte");
        for needle in [
            "import { Button, Callout, Card, Combobox, Field, Form, Input } from \"@elyra/ui\";",
            "team_id: (search) => api.teams_index({ search, per_page: 20 }),",
            "<Combobox bind:value={form.team_id} bind:label={names.team_id} options={parents.team_id} optionValue=\"id\" optionLabel=\"name\" />",
            "names = { team_id: record.team_id == null ? \"\" : `#${record.team_id}` };",
            "let names = $state({ team_id: \"\" });",
            "if (record.team_id != null)\n          api.teams_show(record.team_id).then((p) => (names.team_id = p.name)).catch(() => {});",
            "team_id: number(form.team_id),",
        ] {
            assert!(form.contains(needle), "missing {needle:?} in:\n{form}");
        }
        assert!(
            !form.contains("bind:value={form.team_id} />"),
            "no number input for it"
        );
        // With only the reference, the form needs no Input at all.
        let mut only = invoice_with_team(true);
        only.editable.retain(|f| f.name == "team_id");
        let (files, _) = render(&only, &names("Invoice"), "../../bindings", false, None);
        let form = &files.iter().find(|(f, _)| f == "Form.svelte").unwrap().1;
        assert!(
            form.contains(
                "import { Button, Callout, Card, Combobox, Field, Form } from \"@elyra/ui\";"
            ),
            "{form}"
        );
        let show = view("Show.svelte");
        for needle in [
            "import { href, navigate, toast } from \"@elyra/runtime\";",
            "let names = $state({});",
            "const team_id = $derived(error ? null : (record?.team_id ?? null));",
            "    if (team_id === null) return;\n    return live.teams_show(team_id).subscribe((s) => (names.team_id = s.value?.name));",
            "{#if record.team_id == null}—{:else}<a href={href(`/teams/${record.team_id}`)}>{names.team_id ?? `#${record.team_id}`}</a>{/if}",
        ] {
            assert!(show.contains(needle), "missing {needle:?} in:\n{show}");
        }
    }

    #[test]
    fn a_reference_without_a_resource_stays_a_number() {
        let (files, _) = render(
            &invoice_with_team(false),
            &names("Invoice"),
            "../../bindings",
            false,
            None,
        );
        let view = |name: &str| &files.iter().find(|(f, _)| f == name).unwrap().1;
        let form = view("Form.svelte");
        assert!(
            form.contains("<Input type=\"number\" step=\"1\" bind:value={form.team_id} />"),
            "{form}"
        );
        assert!(
            !form.contains("Combobox") && !form.contains("parents"),
            "{form}"
        );
        let show = view("Show.svelte");
        assert!(
            !show.contains("names") && !show.contains("href, "),
            "{show}"
        );
    }

    #[test]
    fn a_date_is_picked_in_a_date_picker_and_kept_as_text() {
        let specs: Vec<String> = ["title:string", "due:date", "born:date?", "notes:text?"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m =
            crate::resource_generate::model_from_fields("Task", &specs, Path::new("/nowhere/src"))
                .unwrap();
        let (files, _) = render(&m, &names("Task"), "../../bindings", false, None);
        let form = &files.iter().find(|(f, _)| f == "Form.svelte").unwrap().1;
        for needle in [
            "import { Button, Callout, Card, DatePicker, Field, Form, Input, Textarea } from \"@elyra/ui\";",
            // Required: none is "", so the server says it's required.
            "<Field name=\"due\" label={\"Due\"} required><DatePicker bind:value={() => form.due || null, (v) => (form.due = v ?? \"\")} /></Field>",
            "<Field name=\"born\" label={\"Born\"}><DatePicker bind:value={() => form.born || null, (v) => (form.born = v ?? \"\")} /></Field>",
            // …and an optional one goes as null, as text does.
            "born: form.born === \"\" ? null : form.born,",
        ] {
            assert!(form.contains(needle), "{needle}\n---\n{form}");
        }
        assert!(!form.contains("type=\"date\""), "{form}");

        // Only dates: no Input to import.
        let specs: Vec<String> = vec!["due:date".into()];
        let m =
            crate::resource_generate::model_from_fields("Task", &specs, Path::new("/nowhere/src"))
                .unwrap();
        let (files, _) = render(&m, &names("Task"), "../../bindings", false, None);
        let form = &files.iter().find(|(f, _)| f == "Form.svelte").unwrap().1;
        assert!(
            form.contains(
                "import { Button, Callout, Card, DatePicker, Field, Form } from \"@elyra/ui\";"
            ),
            "{form}"
        );
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
            "import { Button, Callout, Card, Field, FileField, Form, Input } from \"@elyra/ui\";",
            "let files = $state({ pdf: null, receipt: null });",
            "<Field name=\"pdf\" label={\"Pdf\"}><FileField bind:file={files.pdf} current={current.pdf} acceptsDrop /></Field>",
            "<Field name=\"receipt\" label={\"Receipt\"}><FileField bind:file={files.receipt} current={current.receipt} /></Field>",
            "await api.invoices_store(input, files.pdf ?? null, files.receipt ?? null)",
            "await api.invoices_update(id, input, files.pdf ?? null, files.receipt ?? null)",
            "current = { pdf: record.pdf, receipt: record.receipt };",
        ] {
            assert!(form.contains(needle), "missing {needle:?} in:\n{form}");
        }
        assert!(!form.contains("form.pdf"), "a file isn't in the JSON input");
        let show = view("Show.svelte");
        for needle in [
            "import { dialog, navigate, toast } from \"@elyra/runtime\";",
            "const downloads = { pdf: api.invoices_pdf, receipt: api.invoices_receipt };",
            "onclick={() => download(\"pdf\")}",
            "dialog.save({ defaultName: record[field] ?? undefined })",
        ] {
            assert!(show.contains(needle), "missing {needle:?} in:\n{show}");
        }
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
            "import { connection, href, navigate, outbox, toast } from \"@elyra/runtime\";",
            "import { Button, Callout, Empty, Table } from \"@elyra/ui\";",
            "<Callout>Offline",
            "<Button size=\"sm\" href={fixPath(w)}>Fix</Button>",
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
