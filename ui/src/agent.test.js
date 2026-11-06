/**
 * The coding agent's view of a window (RFC 0026): `framework/src/shell/agent.js`,
 * which a debug build adds to each page, read and driven against @elyra/ui's
 * own components.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import LayoutFixture from "./test/LayoutFixture.svelte";
import { route } from "./test/runtime.js";
import FormFixture from "./test/FormFixture.svelte";
import SelectFixture from "./test/SelectFixture.svelte";

const SCRIPT = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../framework/src/shell/agent.js"), "utf8");

const NAV = [
  { label: "Customers", path: "/customers" },
  { label: "Reports", path: "/reports" },
];

beforeEach(() => {
  delete window.__elyraAgent;
  new Function(SCRIPT)();
});

const agent = () => window.__elyraAgent;
/** The ref the snapshot gave the line that matches `pattern`. */
const refOf = (snapshot, pattern) => {
  const line = snapshot.split("\n").find((l) => pattern.test(l));
  expect(line, `no line matches ${pattern} in\n${snapshot}`).toBeDefined();
  return line.match(/\[ref=(e\d+)\]/)?.[1];
};

describe("the snapshot", () => {
  it("is the page's accessibility tree: roles, names, states, refs", () => {
    window.location.hash = "#/customers";
    route.set({ path: "/customers", query: {} });
    render(LayoutFixture, { nav: NAV });
    const snapshot = agent().snapshot();
    expect(snapshot).toMatch(/^route: \/customers\n/);
    expect(snapshot).toContain("- banner");
    expect(snapshot).toMatch(/- navigation "Main"/);
    expect(snapshot).toMatch(/- link "Customers" \[current\] \[ref=e\d+\]/);
    expect(snapshot).toMatch(/- button "New customer" \[ref=e\d+\]/);
    expect(snapshot).toMatch(/- tablist "Customer"/);
    expect(snapshot).toMatch(/- tab "Details" \[selected\] \[ref=e\d+\]/);
    expect(snapshot).toMatch(/- tab "Notes" \[disabled\] \[ref=e\d+\]/);
    expect(snapshot).toMatch(/- tabpanel "Details"/);
    expect(snapshot).toContain('- text "Panel: details"');
    // A callout named by its words, shown below it, doesn't say them twice.
    expect(snapshot).toMatch(/- alert\n\s+- text "Overdue"/);
  });

  it("leaves out what's hidden, and masks a password", () => {
    document.body.innerHTML = `
      <main>
        <button>Shown</button>
        <button hidden>Hidden</button>
        <div aria-hidden="true"><button>Decorative</button></div>
        <div style="display: none"><button>Collapsed</button></div>
        <label>Password <input type="password" value="hunter2"></label>
        <label>Code <input autocomplete="one-time-code" value="123456"></label>
        <label>Name <input value="Ada"></label>
      </main>`;
    const snapshot = agent().snapshot();
    expect(snapshot).toContain('- button "Shown"');
    for (const gone of ["Hidden", "Decorative", "Collapsed"]) expect(snapshot).not.toContain(gone);
    expect(snapshot).toMatch(/- textbox "Password" value="•••"/);
    expect(snapshot).toMatch(/- textbox "Code" value="•••"/);
    expect(snapshot).not.toContain("hunter2");
    expect(snapshot).not.toContain("123456");
    expect(snapshot).toMatch(/- textbox "Name" value="Ada"/);
  });

  it("names a row by its cells, a space between them", () => {
    document.body.innerHTML = `
      <table aria-label="Todos"><tbody>
        <tr><td>1</td><td>Write the RFC</td><td><button>Delete</button></td></tr>
      </tbody></table>`;
    const snapshot = agent().snapshot();
    expect(snapshot).toMatch(/- row "1 Write the RFC Delete" \[ref=e\d+\]/);
    expect(snapshot).toMatch(/- button "Delete" \[ref=e\d+\]/);
  });

  it("is bounded: a long list shows its first rows and how many more", () => {
    const items = Array.from({ length: 70 }, (_, i) => `<li>Item ${i}</li>`).join("");
    document.body.innerHTML = `<ul aria-label="Many">${items}</ul><p>${"x".repeat(500)}</p>`;
    const snapshot = agent().snapshot();
    expect(snapshot).toMatch(/- list "Many" \(70 items\)/);
    expect(snapshot).toContain('- listitem "Item 49"');
    expect(snapshot).not.toContain('"Item 50"');
    expect(snapshot).toContain("- … 20 more");
    expect(snapshot).toContain(`- text "${"x".repeat(200)}…"`);
  });

  it("reports the page's errors since the last snapshot, once", () => {
    document.body.innerHTML = "<main></main>";
    const quiet = vi.spyOn(console, "log").mockImplementation(() => {});
    console.error("customers_index failed: offline");
    const first = agent().snapshot();
    expect(first).toContain("errors since the last snapshot:\n- customers_index failed: offline");
    expect(agent().snapshot()).not.toContain("errors since");
    quiet.mockRestore();
  });
});

describe("acting", () => {
  it("types into a field and clicks a button, then answers with the page after", async () => {
    const onsubmit = vi.fn();
    render(FormFixture, { onsubmit });
    let snapshot = agent().snapshot();
    expect(snapshot).toMatch(/- textbox "Name.*" \[required\] \[ref=e\d+\]/);
    const name = refOf(snapshot, /textbox "Name/);
    // The page hears it as typing: `input` and `change`, as bind:value needs.
    const heard = [];
    for (const kind of ["input", "change"]) {
      screen.getByLabelText(/Name/).addEventListener(kind, () => heard.push(kind));
    }
    snapshot = await agent().act({ action: "type", ref: name, text: "Ada Lovelace" });
    expect(heard).toEqual(["input", "change"]);
    expect(screen.getByLabelText(/Name/).value).toBe("Ada Lovelace");
    expect(snapshot).toMatch(/- textbox "Name.*" value="Ada Lovelace"/);
    snapshot = await agent().act({ action: "type", ref: refOf(snapshot, /textbox "Name/), text: " (1815)", append: true });
    expect(screen.getByLabelText(/Name/).value).toBe("Ada Lovelace (1815)");
    await agent().act({ action: "click", ref: refOf(snapshot, /button "Save"/) });
    expect(onsubmit).toHaveBeenCalledTimes(1);
  });

  it("presses a key where the focus is, or on a ref", async () => {
    render(LayoutFixture, { nav: NAV });
    let snapshot = agent().snapshot();
    snapshot = await agent().act({ action: "press", ref: refOf(snapshot, /tab "Details"/), key: "ArrowRight" });
    expect(snapshot).toMatch(/- tab "Invoices" \[selected\]/);
    snapshot = await agent().act({ action: "press", key: "End" });
    expect(snapshot).toMatch(/- tab "Files" \[selected\] \[focused\]/);
  });

  it("selects an option, in a native select and in Select's list", async () => {
    document.body.innerHTML = `<label>Plan <select><option value="free">Free</option><option value="pro">Pro</option></select></label>`;
    let snapshot = agent().snapshot();
    snapshot = await agent().act({ action: "select", ref: refOf(snapshot, /combobox "Plan/), option: "Pro" });
    expect(document.querySelector("select").value).toBe("pro");
    expect(snapshot).toMatch(/- combobox "Plan" value="Pro" \[ref=e\d+\]/);
    // The label's words are the select's name, not a line of their own.
    expect(snapshot).not.toContain('- text "Plan"');

    document.body.innerHTML = "";
    const onchange = vi.fn();
    render(SelectFixture, { options: ["Denmark", "Norway", "Sweden"], onchange });
    snapshot = agent().snapshot();
    snapshot = await agent().act({ action: "click", ref: refOf(snapshot, /combobox "Country"/) });
    expect(snapshot).toMatch(/- option "Norway"/);
    await agent().act({ action: "select", ref: refOf(snapshot, /combobox "Country"/), option: "Norway" });
    expect(onchange).toHaveBeenCalled();
    expect(screen.getByRole("combobox", { name: "Country" }).textContent).toContain("Norway");
  });

  it("navigates to a route", async () => {
    document.body.innerHTML = "<main></main>";
    const snapshot = await agent().act({ action: "navigate", route: "/invoices?status=open" });
    expect(window.location.hash).toBe("#/invoices?status=open");
    expect(snapshot).toMatch(/^route: \/invoices\?status=open/);
  });

  it("refuses a ref it didn't give, or one that's gone, and says what to do", async () => {
    document.body.innerHTML = "<button>Once</button>";
    await expect(agent().act({ action: "click", ref: "e99" })).rejects.toThrow(/take a snapshot first/);
    const ref = refOf(agent().snapshot(), /button "Once"/);
    document.body.innerHTML = "<button>Again</button>";
    await expect(agent().act({ action: "click", ref })).rejects.toThrow(/no longer on the page/);
    await expect(agent().act({ action: "hover", ref })).rejects.toThrow(/unknown action "hover"/);
  });
});

describe("jobs", () => {
  it("are started, then taken once they're done, and only once", async () => {
    document.body.innerHTML = "<button>Go</button>";
    expect(agent().start("j1", "snapshot")).toBe(true);
    await vi.waitFor(() => expect(agent().take("j1")).toBeTruthy());
    agent().start("j2", "snapshot");
    let job;
    await vi.waitFor(() => expect((job = agent().take("j2"))).toBeTruthy());
    expect(job.done).toBe(true);
    expect(job.value).toContain('- button "Go"');
    expect(agent().take("j2")).toBeNull();
    // A failing action's error is the job's answer.
    agent().start("j3", "act", { action: "click", ref: "e42" });
    await vi.waitFor(() => expect((job = agent().take("j3"))).toBeTruthy());
    expect(job.error).toMatch(/take a snapshot first/);
  });
});
