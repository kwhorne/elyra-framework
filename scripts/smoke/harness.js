// Browser smoke test for the views `rata make:resource Customer --generate`
// writes — built into the app by scripts/smoke-resource.sh, never shipped.
//
// It stands in a fake `elyra://` backend with the generated commands'
// semantics (validation bag, paging, not-found), walks the screens the way a
// user would, and reports PASS/FAIL lines in <pre id="harness">.
import { encode, decode } from "@msgpack/msgpack";

// What the smoke script generates; the fake validates like the Rust side.
const REQUIRED = ["name", "email", "active", "credit", "visits", "team_id"];

const rows = [];
let nextId = 1;
const calls = [];
const results = [];
const ok = (name, cond, detail = "") =>
  results.push(`${cond ? "PASS" : "FAIL"} ${name}${detail && !cond ? " — " + detail : ""}`);

const reply = (value) =>
  new Response(encode(value ?? null), { status: 200, headers: { "x-elyra-status": "ok" } });
const fail = (body, kind = "command") =>
  new Response(body, { status: 500, headers: { "x-elyra-status": "error", "x-elyra-error-kind": kind } });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function validate(input) {
  const bag = {};
  for (const f of REQUIRED) {
    const v = input[f];
    if (v === null || v === undefined || (typeof v === "string" && v.trim() === "")) {
      bag[f] = [`The ${f} field is required.`];
    }
  }
  return Object.keys(bag).length ? bag : null;
}

const commands = {
  customers_index(q) {
    let list = rows;
    if (q.search) list = list.filter((r) => r.name.includes(q.search));
    const per = Math.min(Math.max(q.per_page ?? 25, 1), 100);
    const page = Math.max(q.page ?? 1, 1);
    return reply({
      data: list.slice((page - 1) * per, page * per),
      total: list.length,
      per_page: per,
      current_page: page,
      last_page: Math.max(1, Math.ceil(list.length / per)),
    });
  },
  customers_show(id) {
    const row = rows.find((r) => r.id === id);
    return row ? reply(row) : fail(`customer ${id} not found`);
  },
  customers_store(input) {
    const bag = validate(input);
    if (bag) return fail(JSON.stringify(bag), "validation");
    const row = { id: nextId++, ...input, created_at: 1, updated_at: 1 };
    rows.push(row);
    return reply(row);
  },
  customers_update(id, input) {
    const row = rows.find((r) => r.id === id);
    if (!row) return fail(`customer ${id} not found`);
    const bag = validate(input);
    if (bag) return fail(JSON.stringify(bag), "validation");
    Object.assign(row, input);
    return reply(row);
  },
  customers_destroy(id) {
    rows.splice(rows.findIndex((r) => r.id === id), 1);
    return reply(null);
  },
};

// Live queries, as the registry does them: a subscription re-runs after a
// write and its window gets the result on `elyra:live:<id>` if it changed.
const subs = new Map();
const queue = [];
let nextSub = 1;

async function outcome(res) {
  if (res.headers.get("x-elyra-status") === "ok") {
    return { value: decode(new Uint8Array(await res.arrayBuffer())) };
  }
  return { error: { message: await res.text(), kind: res.headers.get("x-elyra-error-kind") } };
}

async function refresh() {
  for (const [id, sub] of subs) {
    const next = await outcome(commands[sub.name](...sub.args));
    const seen = JSON.stringify(next);
    if (seen !== sub.last) {
      sub.last = seen;
      queue.push([`elyra:live:${id}`, next]);
    }
  }
}

/** Another window writing: the fake changes, and the live views must follow. */
async function elsewhere(write) {
  write();
  await refresh();
}

const WRITES = new Set(["customers_store", "customers_update", "customers_destroy"]);

const realFetch = window.fetch.bind(window);
window.fetch = async (url, init = {}) => {
  const u = String(url);
  if (!u.startsWith("elyra://")) return realFetch(url, init);
  // The shell holds the event long-poll open until something happens.
  if (u.includes("/__events")) {
    for (let i = 0; i < 500 && queue.length === 0; i++) await sleep(20);
    return reply(queue.splice(0));
  }
  const body = decode(new Uint8Array(await new Response(init.body).arrayBuffer()));
  if (u.endsWith("/__live-stop")) {
    return reply(subs.delete(body));
  }
  const live = u.split("/__live/")[1];
  if (live) {
    if (!commands[live]) return fail(`no fake for ${live}`);
    const first = await outcome(commands[live](...body));
    if (first.error) return fail(first.error.message, first.error.kind);
    const id = `s${nextSub++}`;
    subs.set(id, { name: live, args: body, last: JSON.stringify(first) });
    calls.push({ name: `live:${live}`, args: body });
    return reply({ id, value: first.value });
  }
  const name = u.split("/__cmd/")[1];
  calls.push({ name, args: body });
  if (!commands[name]) return fail(`no fake for ${name}`);
  const res = commands[name](...body);
  if (WRITES.has(name)) await refresh();
  return res;
};

const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const text = () => $(".content")?.innerText ?? "";
const control = (label) =>
  $$("label.field")
    .find((l) => l.querySelector("span")?.textContent.trim() === label)
    ?.querySelector("input, textarea");
const button = (label) => $$("button, a").find((b) => b.textContent.trim() === label);
const last = (name) => [...calls].reverse().find((c) => c.name === name);
function type(label, value) {
  const el = control(label);
  el.value = value;
  el.dispatchEvent(new Event("input", { bubbles: true }));
}
async function waitFor(fn, what) {
  for (let i = 0; i < 80; i++) {
    if (fn()) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${what}: ${text().replace(/\s+/g, " ").slice(0, 200)}`);
}

async function scenario() {
  location.hash = "#/customers";
  await waitFor(() => text().includes("No customers yet."), "the empty list");
  ok("empty state", true);
  const nav = $$(".nav a").map((a) => a.textContent);
  ok("both resources in the nav", nav.includes("Customers") && nav.includes("Teams"), nav.join(","));

  button("New customer").click();
  await waitFor(() => $("form h2")?.textContent === "New customer", "the form");
  ok("input types follow the fields", control("Email")?.type === "email" && control("Born")?.type === "date" && control("Bio")?.tagName === "TEXTAREA");
  ok("defaults start the form", $("label.check input")?.checked === true && control("Credit")?.value === "0");
  button("Save").click();
  await waitFor(() => $$("small.error").length > 0, "field errors");
  const errors = $$("small.error").map((e) => e.textContent);
  ok("the validation bag lands per field", errors.includes("The name field is required."), errors.join(" | "));
  const empty = last("customers_store").args[0];
  ok("an empty form sends typed nulls", empty.name === "" && empty.phone === null && empty.born === null && empty.meta === null && empty.visits === null, JSON.stringify(empty));

  type("Name", "Ada");
  type("Email", "ada@example.com");
  type("Visits", "3");
  type("Team id", "1");
  type("Born", "2026-01-02");
  type("Meta", '{"vip": true}');
  button("Save").click();
  await waitFor(() => $(".card h2")?.textContent === "Ada", "the detail view");
  const sent = last("customers_store").args[0];
  ok("the payload is typed", sent.visits === 3 && sent.team_id === 1 && sent.credit === 0 && sent.meta?.vip === true && sent.born === "2026-01-02", JSON.stringify(sent));

  button("Edit").click();
  await waitFor(() => control("Name")?.value === "Ada", "the edit form, filled");
  type("Name", "Ada Lovelace");
  button("Save").click();
  await waitFor(() => $(".card h2")?.textContent === "Ada Lovelace", "the update");
  ok("update", last("customers_update")?.args[0] === 1);

  button("Back").click();
  await waitFor(() => $$("tbody tr").length === 1, "the list");
  $("tbody tr button.danger").click();
  await waitFor(() => $(".elyra-modal-overlay"), "the confirm dialog");
  $$(".elyra-modal-actions button").find((b) => b.textContent === "Delete").click();
  await waitFor(() => text().includes("No customers yet."), "the row gone");
  ok("delete behind confirm", last("customers_destroy")?.args[0] === 1);

  // Live: another window adds a customer — the list shows it, no reload.
  await elsewhere(() =>
    rows.push({ id: nextId++, name: "Grace", email: "grace@example.com", phone: null, bio: null,
      active: true, credit: 0, visits: 1, born: null, meta: null, team_id: 1, created_at: 1, updated_at: 1 }),
  );
  await waitFor(() => $$("tbody tr").length === 1 && text().includes("Grace"), "a row from elsewhere");
  ok("another window's write reaches the list", true);

  // …and the detail view follows an edit made elsewhere, then a delete.
  const grace = rows.find((r) => r.name === "Grace");
  location.hash = `#/customers/${grace.id}`;
  await waitFor(() => $(".card h2")?.textContent === "Grace", "the detail view");
  await elsewhere(() => (grace.name = "Grace Hopper"));
  await waitFor(() => $(".card h2")?.textContent === "Grace Hopper", "an edit from elsewhere");
  ok("the detail view follows an edit elsewhere", true);
  await elsewhere(() => rows.splice(rows.indexOf(grace), 1));
  await waitFor(() => text().includes(`customer ${grace.id} not found`), "a delete from elsewhere");
  ok("…and a delete", true);
  ok("subscriptions close with their view", subs.size === 1, `${subs.size} open`);

  location.hash = "#/customers/999";
  await waitFor(() => text().includes("customer 999 not found"), "the not-found error");
  ok("a missing record shows its error", true);
}

scenario()
  .catch((e) => ok("scenario", false, e.message))
  .finally(() => {
    const pre = document.createElement("pre");
    pre.id = "harness";
    pre.textContent = results.join("\n");
    document.body.appendChild(pre);
  });
