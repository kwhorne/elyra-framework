/** SplitPane: the two panes, dragging the divider, the keyboard, the limits, and keeping the size. */
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { store } from "./test/runtime.js";
import SplitPaneFixture from "./test/SplitPaneFixture.svelte";

const divider = () => screen.getByRole("separator");
const shown = () => Number(screen.getByTestId("size").textContent);
const first = () => document.getElementById(divider().getAttribute("aria-controls"));

/** Drag the divider by `by` pixels along `axis`. */
async function drag(by, axis = "clientX") {
  await fireEvent.pointerDown(divider(), { button: 0, pointerId: 1, [axis]: 300 });
  await fireEvent.pointerMove(divider(), { pointerId: 1, [axis]: 300 + by / 2 });
  await fireEvent.pointerMove(divider(), { pointerId: 1, [axis]: 300 + by });
  await fireEvent.pointerUp(divider(), { pointerId: 1, [axis]: 300 + by });
}

describe("SplitPane", () => {
  it("has both panes, the first at its size, and a labelled separator between them", () => {
    render(SplitPaneFixture, { size: 240 });
    expect(screen.getByText("Customers")).toBeTruthy();
    expect(screen.getByText("Ada Lovelace")).toBeTruthy();
    const sep = screen.getByRole("separator", { name: "Resize" });
    expect(sep.tabIndex).toBe(0);
    expect(sep.getAttribute("aria-orientation")).toBe("vertical");
    expect(sep.getAttribute("aria-valuenow")).toBe("240");
    expect(sep.getAttribute("aria-valuemin")).toBe("120");
    expect(sep.hasAttribute("aria-valuemax")).toBe(false);
    expect(first().textContent).toContain("Customers");
    expect(first().style.flexBasis).toBe("240px");
  });

  it("stacks the panes with direction=\"vertical\"", () => {
    render(SplitPaneFixture, { direction: "vertical", label: "Resize the preview" });
    expect(screen.getByRole("separator", { name: "Resize the preview" }).getAttribute("aria-orientation")).toBe("horizontal");
  });

  it("follows a drag of the divider", async () => {
    render(SplitPaneFixture, { size: 240 });
    await drag(60);
    expect(shown()).toBe(300);
    expect(first().style.flexBasis).toBe("300px");
    await drag(-100);
    expect(shown()).toBe(200);
  });

  it("follows a vertical drag up and down", async () => {
    render(SplitPaneFixture, { size: 200, direction: "vertical" });
    await drag(50, "clientY");
    expect(shown()).toBe(250);
    await drag(50, "clientX");
    expect(shown()).toBe(250);
  });

  it("ignores a move without a press, and another button", async () => {
    render(SplitPaneFixture, { size: 240 });
    await fireEvent.pointerMove(divider(), { clientX: 500 });
    expect(shown()).toBe(240);
    await fireEvent.pointerDown(divider(), { button: 2, clientX: 300 });
    await fireEvent.pointerMove(divider(), { clientX: 400 });
    expect(shown()).toBe(240);
    // A drag ends with the release.
    await drag(10);
    await fireEvent.pointerMove(divider(), { pointerId: 1, clientX: 600 });
    expect(shown()).toBe(250);
  });

  it("stops at min and max", async () => {
    render(SplitPaneFixture, { size: 240, min: 200, max: 320 });
    await drag(-500);
    expect(shown()).toBe(200);
    await drag(500);
    expect(shown()).toBe(320);
    expect(divider().getAttribute("aria-valuemax")).toBe("320");
  });

  it("moves with the keyboard", async () => {
    render(SplitPaneFixture, { size: 240, min: 100, max: 600, step: 10 });
    divider().focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(shown()).toBe(250);
    await userEvent.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(shown()).toBe(230);
    await userEvent.keyboard("{Shift>}{ArrowRight}{/Shift}");
    expect(shown()).toBe(270);
    // The other axis's keys aren't this divider's.
    await userEvent.keyboard("{ArrowDown}");
    expect(shown()).toBe(270);
    await userEvent.keyboard("{Home}");
    expect(shown()).toBe(100);
    await userEvent.keyboard("{End}");
    expect(shown()).toBe(600);
    await userEvent.keyboard("{Enter}");
    expect(shown()).toBe(240);
  });

  it("moves a vertical divider with ↑ and ↓", async () => {
    render(SplitPaneFixture, { size: 200, direction: "vertical", step: 20 });
    divider().focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(shown()).toBe(220);
    await userEvent.keyboard("{ArrowUp}{ArrowUp}");
    expect(shown()).toBe(180);
    await userEvent.keyboard("{ArrowRight}");
    expect(shown()).toBe(180);
  });

  it("goes back to its first size on a double-click", async () => {
    render(SplitPaneFixture, { size: 240 });
    await drag(100);
    expect(shown()).toBe(340);
    await fireEvent.dblClick(divider());
    expect(shown()).toBe(240);
  });

  it("leaves the second pane its room", async () => {
    render(SplitPaneFixture, { size: 240, minSecond: 150 });
    const root = divider().parentElement;
    // happy-dom has no layout: give the pane its width.
    Object.defineProperty(root, "clientWidth", { configurable: true, value: 500 });
    await drag(400);
    expect(shown()).toBe(500 - 150);
    expect(divider().getAttribute("aria-valuemax")).toBe("350");
    divider().focus();
    await userEvent.keyboard("{Home}{End}");
    expect(shown()).toBe(350);
  });

  it("leaves the second pane its room from the keyboard too", async () => {
    render(SplitPaneFixture, { size: 240, minSecond: 150 });
    Object.defineProperty(divider().parentElement, "clientWidth", { configurable: true, value: 500 });
    divider().focus();
    await userEvent.keyboard("{End}");
    expect(shown()).toBe(350);
  });

  it("squeezes the first pane when the window gets too small for both", async () => {
    const observers = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(run) {
          this.run = run;
          observers.push(this);
        }
        observe() {}
        disconnect() {}
      },
    );
    try {
      render(SplitPaneFixture, { size: 400, minSecond: 150 });
      Object.defineProperty(divider().parentElement, "clientWidth", { configurable: true, value: 450 });
      observers.forEach((o) => o.run());
      await vi.waitFor(() => expect(shown()).toBe(300));
    } finally {
      vi.unstubAllGlobals();
    }
  });

  describe("persist", () => {
    it("keeps the size in the settings store, and starts from it", async () => {
      const { unmount } = render(SplitPaneFixture, { size: 240, persist: "customers" });
      await vi.waitFor(() => expect(store.get).toHaveBeenCalledWith("elyra-ui.split.customers"));
      await drag(80);
      await vi.waitFor(() => expect(store.set).toHaveBeenCalledWith("elyra-ui.split.customers", 320));
      unmount();
      render(SplitPaneFixture, { size: 240, persist: "customers" });
      await vi.waitFor(() => expect(shown()).toBe(320));
    });

    it("saves a keyboard change once the keys stop", async () => {
      render(SplitPaneFixture, { size: 240, persist: "invoices" });
      divider().focus();
      await userEvent.keyboard("{ArrowRight}{ArrowRight}");
      expect(store.set).not.toHaveBeenCalled();
      await vi.waitFor(() => expect(store.set).toHaveBeenCalledTimes(1));
      expect(store.set).toHaveBeenCalledWith("elyra-ui.split.invoices", 272);
    });

    it("keeps a saved size within the limits, and ignores what isn't a number", async () => {
      await store.set("elyra-ui.split.a", 9000);
      await store.set("elyra-ui.split.b", "wide");
      store.set.mockClear();
      render(SplitPaneFixture, { size: 240, max: 500, persist: "a" });
      await vi.waitFor(() => expect(shown()).toBe(500));
      document.body.innerHTML = "";
      render(SplitPaneFixture, { size: 240, persist: "b" });
      await vi.waitFor(() => expect(store.get).toHaveBeenCalledWith("elyra-ui.split.b"));
      expect(shown()).toBe(240);
    });

    it("works without a store to keep it in", async () => {
      store.get.mockRejectedValueOnce(new Error("no shell"));
      store.set.mockRejectedValueOnce(new Error("no shell"));
      render(SplitPaneFixture, { size: 240, persist: "docs" });
      await drag(20);
      await vi.waitFor(() => expect(store.set).toHaveBeenCalled());
      expect(shown()).toBe(260);
    });

    it("doesn't touch the store without persist", async () => {
      render(SplitPaneFixture, { size: 240 });
      await drag(20);
      await new Promise((r) => setTimeout(r, 10));
      expect(store.get).not.toHaveBeenCalled();
      expect(store.set).not.toHaveBeenCalled();
    });
  });
});
