/** TreeView: the tree's roles, the WAI-ARIA keyboard, choosing, children read on opening, `can`, and keeping what's open. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import TreeView from "./TreeView.svelte";
import { grant, store } from "./test/runtime.js";

const NODES = [
  {
    id: 1,
    label: "Sales",
    children: [
      { id: 11, label: "Norway", children: [{ id: 111, label: "Oslo" }, { id: 112, label: "Bergen" }] },
      { id: 12, label: "Sweden" },
    ],
  },
  { id: 2, label: "Support", children: [{ id: 21, label: "Tier 1" }] },
  { id: 3, label: "Finance", can: "finance.view" },
  { id: 4, label: "Archive", disabled: true },
];

const item = (name) => screen.getByRole("treeitem", { name });
const shown = () => screen.getAllByRole("treeitem").map((el) => el.getAttribute("aria-labelledby") && document.getElementById(el.getAttribute("aria-labelledby")).textContent.trim());
const focused = () => document.getElementById(document.activeElement.getAttribute("aria-labelledby"))?.textContent.trim();

describe("TreeView", () => {
  it("is a named tree of rows, each with its level, place and state; closed branches hide their children", () => {
    render(TreeView, { nodes: NODES, label: "Departments" });
    const tree = screen.getByRole("tree", { name: "Departments" });
    expect(shown()).toEqual(["Sales", "Support", "Finance", "Archive"]);
    const sales = item("Sales");
    expect(tree.contains(sales)).toBe(true);
    expect(sales.getAttribute("aria-level")).toBe("1");
    expect(sales.getAttribute("aria-setsize")).toBe("4");
    expect(sales.getAttribute("aria-posinset")).toBe("1");
    expect(sales.getAttribute("aria-expanded")).toBe("false");
    expect(item("Finance").hasAttribute("aria-expanded")).toBe(false);
    expect(item("Archive").getAttribute("aria-disabled")).toBe("true");
    // One row is in the Tab order: the first, with nothing chosen.
    expect(screen.getAllByRole("treeitem").filter((el) => el.tabIndex === 0)).toEqual([sales]);
  });

  it("opens with what `expanded` says, in groups, a treeitem named by its own label", () => {
    render(TreeView, { nodes: NODES, expanded: [1, 11] });
    expect(shown()).toEqual(["Sales", "Norway", "Oslo", "Bergen", "Sweden", "Support", "Finance", "Archive"]);
    const oslo = item("Oslo");
    expect(oslo.getAttribute("aria-level")).toBe("3");
    expect(oslo.getAttribute("aria-setsize")).toBe("2");
    expect(oslo.parentElement.getAttribute("role")).toBe("group");
    expect(item("Norway").getAttribute("aria-expanded")).toBe("true");
  });

  it("moves with ↓ ↑ Home End, through the rows that show", async () => {
    const user = userEvent.setup();
    render(TreeView, { nodes: NODES, expanded: [1] });
    await user.tab();
    expect(focused()).toBe("Sales");
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}");
    expect(focused()).toBe("Support");
    await user.keyboard("{ArrowUp}");
    expect(focused()).toBe("Sweden");
    await user.keyboard("{End}");
    expect(focused()).toBe("Archive");
    await user.keyboard("{ArrowDown}");
    expect(focused()).toBe("Archive");
    await user.keyboard("{Home}");
    expect(focused()).toBe("Sales");
    // The focused row is the one Tab comes back to.
    expect(document.activeElement.tabIndex).toBe(0);
  });

  it("→ opens a branch, then moves to its first child; ← closes it, then moves to the parent", async () => {
    const user = userEvent.setup();
    const { component } = render(TreeView, { nodes: NODES });
    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(item("Sales").getAttribute("aria-expanded")).toBe("true");
    expect(focused()).toBe("Sales");
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe("Norway");
    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(focused()).toBe("Oslo");
    // A leaf: → does nothing, ← goes to the parent.
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe("Oslo");
    await user.keyboard("{ArrowLeft}");
    expect(focused()).toBe("Norway");
    await user.keyboard("{ArrowLeft}");
    expect(item("Norway").getAttribute("aria-expanded")).toBe("false");
    expect(focused()).toBe("Norway");
    await user.keyboard("{ArrowLeft}");
    expect(focused()).toBe("Sales");
    expect(component).toBeTruthy();
  });

  it("Enter and Space choose a row, and a click does; a disabled row isn't chosen", async () => {
    const user = userEvent.setup();
    const onselect = vi.fn();
    render(TreeView, { nodes: NODES, onselect });
    await user.tab();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onselect).toHaveBeenLastCalledWith(NODES[1]);
    expect(item("Support").getAttribute("aria-selected")).toBe("true");
    expect(item("Sales").getAttribute("aria-selected")).toBe("false");
    await user.keyboard("{ArrowUp} ");
    expect(onselect).toHaveBeenLastCalledWith(NODES[0]);
    await user.click(screen.getByText("Finance"));
    expect(onselect).toHaveBeenLastCalledWith(NODES[2]);
    expect(document.activeElement).toBe(item("Finance"));
    await user.click(screen.getByText("Archive"));
    expect(onselect).toHaveBeenCalledTimes(3);
    expect(item("Archive").getAttribute("aria-selected")).toBe("false");
  });

  it("starts the Tab order on the chosen row", () => {
    render(TreeView, { nodes: NODES, selected: 2 });
    expect(item("Support").tabIndex).toBe(0);
    expect(item("Sales").tabIndex).toBe(-1);
  });

  it("opens and closes a branch from its chevron, or a double click, choosing a child once", async () => {
    const user = userEvent.setup();
    const onselect = vi.fn();
    render(TreeView, { nodes: NODES, onselect });
    await user.click(item("Sales").querySelector(".toggle"));
    expect(item("Sales").getAttribute("aria-expanded")).toBe("true");
    expect(onselect).not.toHaveBeenCalled();
    // A child's click is the child's alone.
    await user.click(screen.getByText("Sweden"));
    expect(onselect.mock.calls).toEqual([[NODES[0].children[1]]]);
    await user.dblClick(screen.getByText("Support"));
    expect(item("Support").getAttribute("aria-expanded")).toBe("true");
    await user.click(item("Sales").querySelector(".toggle"));
    expect(shown()).toEqual(["Sales", "Support", "Tier 1", "Finance", "Archive"]);
  });

  it("* opens every branch beside the row", async () => {
    const user = userEvent.setup();
    render(TreeView, { nodes: NODES });
    await user.tab();
    await user.keyboard("*");
    expect(item("Sales").getAttribute("aria-expanded")).toBe("true");
    expect(item("Support").getAttribute("aria-expanded")).toBe("true");
    expect(item("Norway").getAttribute("aria-expanded")).toBe("false");
  });

  it("moves to the next row starting with what's typed", async () => {
    const user = userEvent.setup();
    // A pause between keys starts a new word.
    let now = 1_000_000;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const type = async (keys, pause = 1000) => {
      now += pause;
      await user.keyboard(keys);
    };
    render(TreeView, { nodes: [...NODES, { id: 5, label: "Strategy" }] });
    await user.tab();
    await type("s");
    expect(focused()).toBe("Support");
    // The same letter again moves on, round to the top.
    await type("s", 0);
    expect(focused()).toBe("Strategy");
    await type("s", 0);
    expect(focused()).toBe("Sales");
    await type("f");
    expect(focused()).toBe("Finance");
    // A word: the row it starts.
    await type("{Home}");
    await type("str");
    expect(focused()).toBe("Strategy");
    vi.restoreAllMocks();
  });

  it("hides a node the user may not see, with its children", async () => {
    grant(["customers.*"]);
    render(TreeView, { nodes: NODES });
    expect(shown()).toEqual(["Sales", "Support", "Archive"]);
    expect(item("Archive").getAttribute("aria-setsize")).toBe("3");
    grant(["*"]);
    await tick();
    expect(shown()).toContain("Finance");
  });

  it("reads a branch's children with `load` the first time it opens, an array or a page", async () => {
    const user = userEvent.setup();
    let finish;
    const load = vi.fn((node) =>
      node.id === "docs"
        ? new Promise((resolve) => (finish = resolve))
        : Promise.resolve({ data: [{ id: "x", label: "Nested" }], total: 1 }),
    );
    render(TreeView, {
      nodes: [{ id: "docs", label: "Documents", hasChildren: true }, { id: "pics", label: "Pictures", hasChildren: true }],
      load,
    });
    expect(item("Documents").getAttribute("aria-expanded")).toBe("false");
    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(load).toHaveBeenCalledWith({ id: "docs", label: "Documents", hasChildren: true });
    expect(item("Documents").getAttribute("aria-busy")).toBe("true");
    expect(screen.getByText("Loading…")).toBeTruthy();
    finish([{ id: "a", label: "Contract.pdf" }]);
    await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
    expect(item("Contract.pdf").getAttribute("aria-level")).toBe("2");
    // Closed and opened again: read once.
    await user.keyboard("{ArrowLeft}{ArrowRight}");
    expect(load).toHaveBeenCalledTimes(1);
    // A Page<T>'s rows.
    await user.click(item("Pictures").querySelector(".toggle"));
    await waitFor(() => expect(item("Nested")).toBeTruthy());
  });

  it("says when children couldn't be read, and tries again", async () => {
    const user = userEvent.setup();
    const load = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce([{ id: "a", label: "Contract.pdf" }]);
    render(TreeView, { nodes: [{ id: "docs", label: "Documents", hasChildren: true }], load });
    await user.click(item("Documents").querySelector(".toggle"));
    await waitFor(() => expect(screen.getByText("Couldn't load this.")).toBeTruthy());
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(item("Contract.pdf")).toBeTruthy());
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("isn't a branch with hasChildren and no load", () => {
    render(TreeView, { nodes: [{ id: "docs", label: "Documents", hasChildren: true }] });
    expect(item("Documents").hasAttribute("aria-expanded")).toBe(false);
  });

  it("keeps what's open in the settings store with persist, and opens it so again", async () => {
    const user = userEvent.setup();
    const first = render(TreeView, { nodes: NODES, persist: "departments" });
    await user.tab();
    await user.keyboard("{ArrowRight}{ArrowDown}{ArrowRight}");
    await waitFor(() => expect(store.set).toHaveBeenLastCalledWith("elyra-ui.tree.departments", [1, 11]));
    first.unmount();

    render(TreeView, { nodes: NODES, persist: "departments" });
    await waitFor(() => expect(shown()).toEqual(["Sales", "Norway", "Oslo", "Bergen", "Sweden", "Support", "Finance", "Archive"]));
    expect(store.get).toHaveBeenCalledWith("elyra-ui.tree.departments");
  });

  it("takes its words from labels", async () => {
    const user = userEvent.setup();
    render(TreeView, {
      nodes: [{ id: "docs", label: "Dokumenter", hasChildren: true }],
      load: () => new Promise(() => {}),
      labels: { loading: "Laster …" },
    });
    await user.click(item("Dokumenter").querySelector(".toggle"));
    expect(screen.getByText("Laster …")).toBeTruthy();
  });
});
