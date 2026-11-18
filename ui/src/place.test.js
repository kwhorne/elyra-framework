/** Where a popup goes: beside what opened it, flipped when there's more room, inside the window. */
import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { MARGIN, place, position } from "./internal/place.js";
import MenuFixture from "./test/MenuFixture.svelte";

const rect = (left, top, width, height) => ({
  left,
  top,
  width,
  height,
  right: left + width,
  bottom: top + height,
  x: left,
  y: top,
});
const WINDOW = { width: 1000, height: 700 };

describe("position", () => {
  it("opens below, its left edge on the anchor's", () => {
    const at = position(rect(100, 100, 80, 32), { width: 200, height: 150 }, WINDOW);
    expect(at).toMatchObject({ top: 136, left: 100, side: "bottom", clipped: false, width: null });
  });

  it("opens above near the window's bottom, where there's more room", () => {
    const at = position(rect(100, 640, 80, 32), { width: 200, height: 150 }, WINDOW, { gap: 6 });
    expect(at.side).toBe("top");
    expect(at.top).toBe(640 - 6 - 150);
    expect(at.clipped).toBe(false);
  });

  it("stays below when above has no more room, and scrolls in what there is", () => {
    const at = position(rect(100, 300, 80, 32), { width: 200, height: 600 }, WINDOW);
    expect(at.side).toBe("bottom");
    expect(at.clipped).toBe(true);
    expect(at.maxHeight).toBe(700 - 332 - 4 - MARGIN);
  });

  it("a tooltip asked for on top goes below when the top is too close", () => {
    const at = position(rect(100, 10, 80, 20), { width: 60, height: 24 }, WINDOW, {
      side: "top",
      align: "center",
      gap: 6,
    });
    expect(at.side).toBe("bottom");
    expect(at.left).toBe(100 + (80 - 60) / 2);
  });

  it("keeps inside the window's edges", () => {
    expect(position(rect(950, 100, 40, 32), { width: 200, height: 100 }, WINDOW).left).toBe(1000 - 200 - MARGIN);
    expect(position(rect(2, 100, 40, 32), { width: 200, height: 100 }, WINDOW, { align: "end" }).left).toBe(MARGIN);
  });

  it("lines up the right edges with align end, and matches the anchor's width", () => {
    expect(position(rect(500, 100, 80, 32), { width: 200, height: 100 }, WINDOW, { align: "end" }).left).toBe(380);
    const list = position(rect(100, 100, 260, 32), { width: 120, height: 100 }, WINDOW, { match: true });
    expect(list.width).toBe(260);
  });
});

describe("place", () => {
  afterEach(() => vi.restoreAllMocks());

  /**
   * An anchor at `anchorRect`, and a popup of `size` (its layout's, as
   * `offsetWidth`/`offsetHeight` give it) whose `fixed` 0,0 is `origin`:
   * what a probe beside it measures there.
   */
  function measured(anchorRect, size, origin = { left: 0, top: 0 }) {
    const anchor = document.createElement("button");
    const popup = document.createElement("div");
    document.body.append(anchor, popup);
    Object.defineProperty(popup, "offsetWidth", { get: () => size.width });
    Object.defineProperty(popup, "offsetHeight", { get: () => size.height });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
      return this === anchor ? anchor.at ?? anchorRect : rect(origin.left, origin.top, 0, 0);
    });
    return { anchor, popup };
  }

  it("puts the popup in fixed coordinates, flipped above near the bottom", () => {
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(700);
    const { anchor, popup } = measured(rect(100, 640, 80, 32), { width: 200, height: 150 });
    const placed = place(popup, { anchor });
    expect(popup.style.position).toBe("fixed");
    expect(popup.style.top).toBe(`${640 - 4 - 150}px`);
    expect(popup.style.left).toBe("100px");
    expect(popup.dataset.side).toBe("top");
    placed.destroy();
  });

  it("inside a transformed ancestor, counts from where fixed really starts", () => {
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(700);
    // `fixed` 0,0 lands at 50,40: a Modal's backdrop is the containing block.
    const { anchor, popup } = measured(rect(100, 100, 80, 32), { width: 200, height: 150 }, { left: 50, top: 40 });
    place(popup, { anchor }).destroy();
    expect(popup.style.top).toBe(`${136 - 40}px`);
    expect(popup.style.left).toBe(`${100 - 50}px`);
  });

  it("scrolls when neither side has room, and follows a scroll", () => {
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(700);
    const { anchor, popup } = measured(rect(100, 300, 80, 32), { width: 200, height: 900 });
    const placed = place(popup, { anchor });
    expect(popup.style.overflowY).toBe("auto");
    expect(popup.style.maxHeight).toBe(`${700 - 332 - 4 - MARGIN}px`);

    anchor.at = rect(100, 200, 80, 32);
    document.body.dispatchEvent(new Event("scroll"));
    window.dispatchEvent(new Event("scroll"));
    expect(popup.style.top).toBe("236px");
    placed.destroy();
  });

  it("measures its layout, not an opening animation's slide or scale", () => {
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(700);
    const { anchor, popup } = measured(rect(100, 100, 260, 32), { width: 260, height: 150 });
    // Drawn mid-animation: 4 px up and a little smaller. Not what's measured.
    popup.getBoundingClientRect = () => rect(103, -4, 255, 147);
    place(popup, { anchor, match: true }).destroy();
    expect(popup.style.top).toBe("136px");
    expect(popup.style.left).toBe("100px");
    expect(popup.style.width).toBe("260px");
    expect(popup.style.boxSizing).toBe("border-box");
  });

  it("leaves a hidden popup alone until it's shown", () => {
    const { anchor, popup } = measured(rect(100, 100, 80, 32), { width: 60, height: 24 });
    popup.hidden = true;
    const placed = place(popup, { anchor, shown: false });
    expect(popup.style.position).toBe("");
    popup.hidden = false;
    placed.update({ anchor, shown: true });
    expect(popup.style.position).toBe("fixed");
    placed.destroy();
  });

  it("a Dropdown's menu is placed by it", async () => {
    render(MenuFixture);
    await userEvent.click(screen.getByRole("button", { name: /actions/i }));
    const menu = screen.getByRole("menu");
    expect(menu.style.position).toBe("fixed");
    expect(menu.dataset.side).toBeDefined();
  });
});
