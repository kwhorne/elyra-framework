/** Combobox: type to filter, the keyboard, leaving the field, searching through a function. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import ComboboxFixture from "./test/ComboboxFixture.svelte";

const NAMES = ["Aker", "Bergen Bryggeri", "Equinor", "Hydro", "Kongsberg"];
const CUSTOMERS = [
  { id: 1, name: "Aker" },
  { id: 2, name: "Equinor" },
  { id: 3, name: "Hydro", disabled: true },
  { id: 4, name: "Kongsberg" },
];

const box = () => screen.getByRole("combobox", { name: "Customer" });
const names = () => screen.getAllByRole("option").map((o) => o.textContent.trim());
const active = () => document.getElementById(box().getAttribute("aria-activedescendant"))?.textContent.trim();

/** A promise to settle by hand. */
function deferred() {
  let resolve, reject;
  const promise = new Promise((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}

describe("Combobox", () => {
  it("is a labelled combobox whose typing filters the list", async () => {
    render(ComboboxFixture, { options: NAMES });
    expect(box().getAttribute("aria-expanded")).toBe("false");
    expect(box().getAttribute("aria-autocomplete")).toBe("list");
    await userEvent.type(box(), "er");
    expect(box().getAttribute("aria-expanded")).toBe("true");
    expect(box().getAttribute("aria-controls")).toBe(screen.getByRole("listbox").id);
    expect(names()).toEqual(["Aker", "Bergen Bryggeri", "Kongsberg"]);
    expect(active()).toBe("Aker");
  });

  it("picks with the keyboard, and the input shows the choice", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, { options: CUSTOMERS, optionValue: "id", optionLabel: "name", onchange });
    await userEvent.type(box(), "o");
    expect(names()).toEqual(["Equinor", "Hydro", "Kongsberg"]);
    // Hydro is disabled: ↓ goes past it.
    await userEvent.keyboard("{ArrowDown}");
    expect(active()).toBe("Kongsberg");
    await userEvent.keyboard("{ArrowDown}");
    expect(active()).toBe("Kongsberg");
    await userEvent.keyboard("{ArrowUp}");
    expect(active()).toBe("Equinor");
    await userEvent.keyboard("{Enter}");
    expect(onchange).toHaveBeenCalledWith(2, CUSTOMERS[1]);
    expect(box().value).toBe("Equinor");
    expect(screen.getByTestId("label").textContent).toBe("Equinor");
    expect(screen.getByTestId("value").textContent).toBe("2");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(box());
  });

  it("takes functions for the value and the label", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, {
      options: CUSTOMERS,
      optionValue: (c) => `c${c.id}`,
      optionLabel: (c) => c.name.toUpperCase(),
      onchange,
    });
    await userEvent.type(box(), "kong{Enter}");
    expect(onchange).toHaveBeenCalledWith("c4", CUSTOMERS[3]);
    expect(box().value).toBe("KONGSBERG");
  });

  it("opens on ↓ with the whole list, at the chosen option, and Esc puts the text back", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, { options: NAMES, value: "Hydro", onchange });
    expect(box().value).toBe("Hydro");
    box().focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(names()).toEqual(NAMES);
    expect(active()).toBe("Hydro");
    expect(screen.getByRole("option", { name: "Hydro" }).getAttribute("aria-selected")).toBe("true");
    await userEvent.keyboard("x{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(box().value).toBe("Hydro");
    expect(onchange).not.toHaveBeenCalled();
  });

  it("opens on a click, and a click picks", async () => {
    render(ComboboxFixture, { options: NAMES });
    await userEvent.click(box());
    await userEvent.click(screen.getByRole("option", { name: "Equinor" }));
    expect(box().value).toBe("Equinor");
    expect(document.activeElement).toBe(box());
  });

  it("doesn't pick a disabled option on a click", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, { options: CUSTOMERS, optionValue: "id", optionLabel: "name", onchange });
    await userEvent.type(box(), "hy");
    await userEvent.click(screen.getByRole("option", { name: "Hydro" }));
    expect(onchange).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("says when nothing matches", async () => {
    render(ComboboxFixture, { options: NAMES, emptyText: "No customer" });
    await userEvent.type(box(), "zzz");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.getByText("No customer")).toBeTruthy();
  });

  it("puts the choice back when focus leaves half-typed text", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, { options: NAMES, value: "Aker", onchange });
    await userEvent.clear(box());
    await userEvent.type(box(), "Hyd");
    await userEvent.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(box().value).toBe("Aker");
    expect(onchange).not.toHaveBeenCalled();
    await userEvent.type(box(), "x");
    await userEvent.tab();
    expect(box().value).toBe("Aker");
  });

  it("clears the choice when the text is emptied and focus leaves", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, { options: NAMES, value: "Aker", label: "Aker", onchange });
    await userEvent.clear(box());
    await userEvent.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(onchange).toHaveBeenCalledWith(null, null);
    expect(screen.getByTestId("value").textContent).toBe("null");
    expect(box().value).toBe("");
  });

  it("has a clear button while something is chosen", async () => {
    const onchange = vi.fn();
    render(ComboboxFixture, { options: NAMES, value: "Hydro", onchange });
    const clear = screen.getByRole("button", { name: "Clear" });
    expect(clear.tabIndex).toBe(-1);
    box().focus();
    await userEvent.click(clear);
    expect(onchange).toHaveBeenCalledTimes(1);
    expect(onchange).toHaveBeenCalledWith(null, null);
    expect(box().value).toBe("");
    expect(document.activeElement).toBe(box());
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
  });

  it("has no clear button when it isn't clearable", () => {
    render(ComboboxFixture, { options: NAMES, value: "Hydro", clearable: false });
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
  });

  it("follows a value set from outside", async () => {
    const { rerender } = render(ComboboxFixture, { options: NAMES, value: "Aker" });
    expect(box().value).toBe("Aker");
    await rerender({ value: "Equinor" });
    expect(box().value).toBe("Equinor");
  });

  it("keeps Enter from submitting the form only while the list is open", async () => {
    const submit = vi.fn((e) => e.preventDefault());
    document.addEventListener("submit", submit);
    try {
      render(ComboboxFixture, { options: NAMES });
      // The fixture has no form; wrap the input in one for this test.
      const form = document.createElement("form");
      box().closest(".elyra-field").before(form);
      form.append(box().closest(".elyra-field"));
      await userEvent.type(box(), "aker{Enter}");
      expect(submit).not.toHaveBeenCalled();
      expect(box().value).toBe("Aker");
    } finally {
      document.removeEventListener("submit", submit);
    }
  });

  it("is disabled", () => {
    render(ComboboxFixture, { options: NAMES, value: "Aker", disabled: true });
    expect(box().disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
  });

  describe("searching through a function", () => {
    it("asks it for what's typed, with a signal, and takes a Page", async () => {
      const search = vi.fn(async (q) => ({
        data: CUSTOMERS.filter((c) => c.name.toLowerCase().includes(q.toLowerCase())),
        total: 9,
        current_page: 1,
        last_page: 1,
        per_page: 20,
      }));
      const onchange = vi.fn();
      render(ComboboxFixture, { options: search, optionValue: "id", optionLabel: "name", debounce: 0, onchange });
      await userEvent.type(box(), "kong");
      await waitFor(() => expect(names()).toEqual(["Kongsberg"]));
      const [query, { signal }] = search.mock.calls.at(-1);
      expect(query).toBe("kong");
      expect(signal).toBeInstanceOf(AbortSignal);
      await userEvent.keyboard("{Enter}");
      expect(onchange).toHaveBeenCalledWith(4, CUSTOMERS[3]);
      expect(box().value).toBe("Kongsberg");
      expect(screen.getByTestId("value").textContent).toBe("4");
    });

    it("waits for the typing to pause", async () => {
      const search = vi.fn(async () => NAMES);
      render(ComboboxFixture, { options: search, debounce: 40 });
      box().focus();
      await userEvent.keyboard("hyd");
      expect(search).not.toHaveBeenCalled();
      await waitFor(() => expect(search).toHaveBeenCalledTimes(1));
      expect(search.mock.calls[0][0]).toBe("hyd");
    });

    it("lists on a click, before anything is typed", async () => {
      const search = vi.fn(async () => NAMES);
      render(ComboboxFixture, { options: search });
      await userEvent.click(box());
      await waitFor(() => expect(names()).toEqual(NAMES));
      expect(search).toHaveBeenCalledTimes(1);
      expect(search.mock.calls[0][0]).toBe("");
    });

    it("lists from an empty query when opened without typing", async () => {
      const search = vi.fn(async () => NAMES);
      render(ComboboxFixture, { options: search });
      box().focus();
      await userEvent.keyboard("{ArrowDown}");
      await waitFor(() => expect(names()).toEqual(NAMES));
      expect(search.mock.calls[0][0]).toBe("");
    });

    it("shows that it's searching, and keeps only the latest answer", async () => {
      const answers = [deferred(), deferred()];
      const search = vi.fn(() => answers[search.mock.calls.length - 1].promise);
      render(ComboboxFixture, { options: search, debounce: 0 });
      box().focus();
      await userEvent.keyboard("a");
      await waitFor(() => expect(search).toHaveBeenCalledTimes(1));
      expect(screen.getByRole("status", { name: "Searching" })).toBeTruthy();
      expect(screen.getByText("Searching…")).toBeTruthy();
      const first = search.mock.calls[0][1].signal;
      await userEvent.keyboard("k");
      await waitFor(() => expect(search).toHaveBeenCalledTimes(2));
      expect(first.aborted).toBe(true);
      answers[1].resolve(["Aker"]);
      await waitFor(() => expect(names()).toEqual(["Aker"]));
      answers[0].resolve(["Stale"]);
      await Promise.resolve();
      await new Promise((r) => setTimeout(r, 0));
      expect(names()).toEqual(["Aker"]);
      expect(screen.queryByRole("status", { name: "Searching" })).toBeNull();
    });

    it("says what went wrong", async () => {
      const search = vi.fn(async () => {
        throw { detail: "The server is down" };
      });
      render(ComboboxFixture, { options: search, debounce: 0 });
      await userEvent.type(box(), "a");
      expect((await screen.findByRole("alert")).textContent).toBe("The server is down");
    });

    it("waits for minChars", async () => {
      const search = vi.fn(async () => NAMES);
      render(ComboboxFixture, { options: search, debounce: 0, minChars: 2 });
      await userEvent.type(box(), "a");
      await new Promise((r) => setTimeout(r, 10));
      expect(search).not.toHaveBeenCalled();
      expect(box().getAttribute("aria-expanded")).toBe("false");
      await userEvent.type(box(), "k");
      await waitFor(() => expect(search).toHaveBeenCalledWith("ak", expect.anything()));
      expect(box().getAttribute("aria-expanded")).toBe("true");
    });

    it("shows the label it's given for the chosen value", () => {
      render(ComboboxFixture, { options: async () => [], value: 7, label: "Statkraft" });
      expect(box().value).toBe("Statkraft");
    });
  });
});
