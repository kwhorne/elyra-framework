/** Table: a live page, sorting and searching through the query, paging, its states. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { writable } from "svelte/store";
import TableFixture from "./test/TableFixture.svelte";

const ADA = { id: 1, name: "Ada", email: "ada@example.com", credit: 1200 };
const GRACE = { id: 2, name: "Grace", email: "grace@example.com", credit: 50 };
const LINUS = { id: 3, name: "Linus", email: "linus@example.com", credit: 300 };

const page = (data, current = 1, last = 1) => ({ data, total: 5, per_page: 2, current_page: current, last_page: last });
const live = (value, extra = {}) => writable({ value, error: null, loading: false, ...extra });
const query = () => JSON.parse(screen.getByTestId("query").textContent);
const cells = () => screen.getAllByRole("row").slice(1).map((r) => within(r).getAllByRole("cell")[0].textContent.trim());

describe("Table over a live page", () => {
  it("shows the rows, and sorts through the query", async () => {
    render(TableFixture, { page: live(page([ADA, GRACE])), columns: ["name", "email"], sortable: true, label: "Customers" });
    expect(screen.getByRole("table", { name: "Customers" })).toBeTruthy();
    expect(cells()).toEqual(["Ada", "Grace"]);
    const name = screen.getByRole("columnheader", { name: /Name/ });
    expect(name.getAttribute("aria-sort")).toBe("none");
    await userEvent.click(within(name).getByRole("button"));
    expect(query()).toEqual({ sort: "name", direction: "asc", page: 1 });
    expect(name.getAttribute("aria-sort")).toBe("ascending");
    await userEvent.click(within(name).getByRole("button"));
    expect(query()).toMatchObject({ sort: "name", direction: "desc" });
    expect(name.getAttribute("aria-sort")).toBe("descending");
  });

  it("speaks the app's language", async () => {
    render(TableFixture, {
      page: live(page([ADA, GRACE], 1, 3)),
      columns: ["name"],
      search: true,
      labels: { search: "Søk…", total: (n) => `${n} i alt`, previous: "Forrige", next: "Neste", page: (n) => `Side ${n}` },
    });
    expect(screen.getByRole("searchbox").getAttribute("placeholder")).toBe("Søk…");
    expect(screen.getByText("5 i alt")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Forrige" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Side 3" })).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Neste" }));
    expect(query()).toMatchObject({ page: 2 });
  });

  it("searches and pages through the query", async () => {
    render(TableFixture, { page: live(page([ADA, GRACE], 1, 3)), columns: ["name"], search: true });
    await userEvent.type(screen.getByRole("searchbox", { name: "Search" }), "gr");
    await vi.waitFor(() => expect(query()).toEqual({ search: "gr", page: 1 }));
    expect(screen.getByText("5 in all")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(query()).toMatchObject({ search: "gr", page: 2 });
  });

  it("follows the store as the query re-runs", async () => {
    const store = live(page([ADA, GRACE]));
    render(TableFixture, { page: store, columns: ["name"] });
    store.set({ value: page([LINUS]), error: null, loading: false });
    await vi.waitFor(() => expect(cells()).toEqual(["Linus"]));
  });

  it("shows loading, a failure, and nothing", async () => {
    const store = live(undefined, { loading: true });
    render(TableFixture, { page: store, columns: ["name", "email"] });
    expect(screen.getByRole("table").getAttribute("aria-busy")).toBe("true");
    expect(screen.getAllByRole("row")).toHaveLength(4);

    store.set({ value: undefined, error: { message: "The server is down." }, loading: false });
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toContain("The server is down."));
    expect(screen.queryByText("Nothing here yet")).toBeNull();

    store.set({ value: page([]), error: null, loading: false });
    await vi.waitFor(() => expect(screen.getByText("Nothing here yet")).toBeTruthy());
  });
});

describe("Table over plain rows", () => {
  it("searches, sorts and pages them itself", async () => {
    render(TableFixture, {
      rows: [ADA, GRACE, LINUS],
      columns: ["name", { key: "credit", label: "Credit", align: "end", format: (v) => `${v} kr` }],
      sortable: ["credit"],
      search: true,
      perPage: 2,
    });
    expect(cells()).toEqual(["Ada", "Grace"]);
    expect(screen.getByText("1200 kr")).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "Name" }).getAttribute("aria-sort")).toBeNull();
    const credit = () => within(screen.getByRole("columnheader", { name: /Credit/ })).getByRole("button");
    await userEvent.click(credit());
    expect(cells()).toEqual(["Grace", "Linus"]);
    await userEvent.click(credit());
    expect(cells()).toEqual(["Ada", "Linus"]);
    await userEvent.click(credit());
    await userEvent.click(screen.getByRole("button", { name: "Page 2" }));
    expect(cells()).toEqual(["Ada"]);
    await userEvent.type(screen.getByRole("searchbox", { name: "Search" }), "lin");
    await vi.waitFor(() => expect(cells()).toEqual(["Linus"]));
    await userEvent.clear(screen.getByRole("searchbox", { name: "Search" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Search" }), "zz");
    await vi.waitFor(() => expect(screen.getByText("Nothing matches “zz”")).toBeTruthy());
  });

  it("opens a row by click or keyboard, and ↓/↑ move between rows", async () => {
    const onopen = vi.fn();
    render(TableFixture, { rows: [ADA, GRACE, LINUS], columns: ["name"], onopen, withActions: true });
    const [, first, second] = screen.getAllByRole("row");
    first.focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(second);
    await userEvent.keyboard("{Enter}");
    expect(onopen).toHaveBeenLastCalledWith(GRACE);
    await userEvent.click(within(first).getAllByRole("cell")[0]);
    expect(onopen).toHaveBeenLastCalledWith(ADA);
    // An action in the row runs once, without also opening the row.
    onopen.mockClear();
    await userEvent.click(screen.getByRole("button", { name: "Open Linus" }));
    expect(onopen).toHaveBeenCalledOnce();
  });
});
