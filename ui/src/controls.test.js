/** Textarea, Checkbox, Switch and RadioGroup. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import ControlsFixture from "./test/ControlsFixture.svelte";

describe("form controls", () => {
  it("a textarea is labelled and takes text", async () => {
    render(ControlsFixture);
    const notes = screen.getByLabelText("Notes");
    expect(notes.tagName).toBe("TEXTAREA");
    await userEvent.type(notes, "Call back");
    expect(notes.value).toBe("Call back");
  });

  it("a checkbox toggles from its label", async () => {
    render(ControlsFixture);
    const box = screen.getByRole("checkbox", { name: "I agree" });
    expect(box.checked).toBe(false);
    await userEvent.click(screen.getByText("I agree"));
    expect(box.checked).toBe(true);
  });

  it("a switch is a switch, toggled by click, keyboard and its label", async () => {
    render(ControlsFixture);
    const toggle = screen.getByRole("switch", { name: "Go live" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await userEvent.click(toggle);
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    toggle.focus();
    await userEvent.keyboard(" ");
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await userEvent.click(screen.getByText("Go live"));
    expect(toggle.getAttribute("aria-checked")).toBe("true");
  });

  it("a radio group is labelled by its field, and picks one", async () => {
    render(ControlsFixture);
    const group = screen.getByRole("radiogroup", { name: "Plan" });
    expect(group).toBeTruthy();
    const yearly = screen.getByRole("radio", { name: "Yearly" });
    expect(screen.getByRole("radio", { name: "monthly" }).checked).toBe(true);
    await userEvent.click(yearly);
    expect(yearly.checked).toBe(true);
    expect(group.getAttribute("aria-invalid")).toBeNull();
    await userEvent.click(screen.getByRole("radio", { name: "None" }));
    expect(group.getAttribute("aria-invalid")).toBe("true");
    expect(document.getElementById(group.getAttribute("aria-describedby")).textContent).toBe("Choose a plan.");
  });
});
