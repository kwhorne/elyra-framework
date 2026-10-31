/** Select: a combobox over a listbox, the keyboard, search, disabled options. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import SelectFixture from "./test/SelectFixture.svelte";

const NORDIC = ["Denmark", "Finland", { value: "is", label: "Iceland", disabled: true }, "Norway", "Sweden"];
const MANY = ["Austria", "Belgium", "Croatia", "Denmark", "Estonia", "Finland", "Germany", "Hungary", "Ireland", "Norway"];

describe("Select", () => {
  it("is a labelled combobox that opens a listbox", async () => {
    render(SelectFixture, { options: NORDIC });
    const box = screen.getByRole("combobox", { name: "Country" });
    expect(box.textContent).toContain("Choose…");
    expect(box.getAttribute("aria-expanded")).toBe("false");
    await userEvent.click(box);
    expect(box.getAttribute("aria-expanded")).toBe("true");
    const list = screen.getByRole("listbox");
    expect(box.getAttribute("aria-controls")).toBe(list.id);
    expect(screen.getAllByRole("option").map((o) => o.textContent.trim())).toEqual(
      ["Denmark", "Finland", "Iceland", "Norway", "Sweden"],
    );
  });

  it("is driven by the keyboard, past a disabled option", async () => {
    const onchange = vi.fn();
    render(SelectFixture, { options: NORDIC, onchange });
    const box = screen.getByRole("combobox", { name: "Country" });
    box.focus();
    await userEvent.keyboard("{ArrowDown}");
    const list = screen.getByRole("listbox");
    expect(document.activeElement).toBe(list);
    const active = () => document.getElementById(list.getAttribute("aria-activedescendant")).textContent.trim();
    expect(active()).toBe("Denmark");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(active()).toBe("Norway");
    await userEvent.keyboard("{End}");
    expect(active()).toBe("Sweden");
    await userEvent.keyboard("{Home}{ArrowDown}{Enter}");
    expect(onchange).toHaveBeenCalledWith("Finland");
    expect(box.textContent).toContain("Finland");
    expect(document.activeElement).toBe(box);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("opens on the chosen option, and Esc closes without choosing", async () => {
    const onchange = vi.fn();
    render(SelectFixture, { options: NORDIC, value: "Norway", onchange });
    const box = screen.getByRole("combobox", { name: "Country" });
    await userEvent.click(box);
    const list = screen.getByRole("listbox");
    expect(document.getElementById(list.getAttribute("aria-activedescendant")).textContent.trim()).toBe("Norway");
    expect(screen.getByRole("option", { name: "Norway" }).getAttribute("aria-selected")).toBe("true");
    await userEvent.keyboard("{ArrowDown}{Escape}");
    expect(onchange).not.toHaveBeenCalled();
    expect(box.textContent).toContain("Norway");
    expect(document.activeElement).toBe(box);
  });

  it("jumps to what's typed", async () => {
    render(SelectFixture, { options: NORDIC });
    await userEvent.click(screen.getByRole("combobox", { name: "Country" }));
    await userEvent.keyboard("sw");
    const list = screen.getByRole("listbox");
    expect(document.getElementById(list.getAttribute("aria-activedescendant")).textContent.trim()).toBe("Sweden");
  });

  it("searches a long list", async () => {
    const onchange = vi.fn();
    render(SelectFixture, { options: MANY, onchange });
    await userEvent.click(screen.getByRole("combobox", { name: "Country" }));
    const search = screen.getByRole("textbox", { name: "Search the options" });
    expect(document.activeElement).toBe(search);
    await userEvent.type(search, "land");
    expect(screen.getAllByRole("option").map((o) => o.textContent.trim())).toEqual(["Finland", "Ireland"]);
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(onchange).toHaveBeenCalledWith("Ireland");
    await userEvent.click(screen.getByRole("combobox", { name: "Country" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Search the options" }), "xyz");
    expect(screen.getByText("No matches")).toBeTruthy();
  });

  it("closes on a click elsewhere", async () => {
    render(SelectFixture, { options: NORDIC });
    await userEvent.click(screen.getByRole("combobox", { name: "Country" }));
    expect(screen.getByRole("listbox")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
