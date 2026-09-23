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

const realFetch = window.fetch.bind(window);
window.fetch = async (url, init = {}) => {
  const u = String(url);
  if (!u.startsWith("elyra://")) return realFetch(url, init);
  // The shell holds the event long-poll open until something happens.
  if (u.includes("/__events")) return sleep(10000).then(() => reply([]));
  const name = u.split("/__cmd/")[1];
  const args = decode(new Uint8Array(await new Response(init.body).arrayBuffer()));
  calls.push({ name, args });
  return commands[name] ? commands[name](...args) : fail(`no fake for ${name}`);
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
