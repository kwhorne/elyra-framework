/** Button: variants, links into the app, abilities, the confirm question, busy. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import ButtonFixture from "./test/ButtonFixture.svelte";
import { confirm, grant } from "./test/runtime.js";

describe("Button", () => {
  it("is a button that runs onclick", async () => {
    const onclick = vi.fn();
    render(ButtonFixture, { variant: "primary", onclick });
    const button = screen.getByRole("button", { name: "Save" });
    expect(button.getAttribute("type")).toBe("button");
    expect(button.className).toContain("primary");
    await userEvent.click(button);
    expect(onclick).toHaveBeenCalledOnce();
  });

  it("is a link into the app with an app path", () => {
    render(ButtonFixture, { href: "/customers/7", label: "Open" });
    expect(screen.getByRole("link", { name: "Open" }).getAttribute("href")).toBe("#/customers/7");
  });

  it("is hidden, or disabled, without the ability, and follows the list", async () => {
    grant(["customers.view"]);
    const hidden = render(ButtonFixture, { can: "customers.delete", label: "Delete" });
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
    grant(["customers.*"]);
    await vi.waitFor(() => expect(screen.getByRole("button", { name: "Delete" })).toBeTruthy());
    hidden.unmount();

    grant([]);
    render(ButtonFixture, { can: "customers.delete", canMode: "disable", label: "Remove", onclick: vi.fn() });
    expect(screen.getByRole("button", { name: "Remove" }).disabled).toBe(true);
  });

  it("asks first, and runs only on yes", async () => {
    const onclick = vi.fn();
    render(ButtonFixture, { variant: "danger", confirm: "Delete Ada?", confirmLabel: "Delete", onclick });
    const button = screen.getByRole("button", { name: "Save" });
    confirm.mockResolvedValueOnce(false);
    await userEvent.click(button);
    expect(confirm).toHaveBeenCalledWith("Delete Ada?", { danger: true, confirmLabel: "Delete" });
    expect(onclick).not.toHaveBeenCalled();
    await userEvent.click(button);
    await vi.waitFor(() => expect(onclick).toHaveBeenCalledOnce());
  });

  it("is busy while loading: disabled, and says so", async () => {
    const onclick = vi.fn();
    render(ButtonFixture, { loading: true, onclick });
    const button = screen.getByRole("button", { name: "Save" });
    expect(button.disabled).toBe(true);
    expect(button.getAttribute("aria-busy")).toBe("true");
    await userEvent.click(button);
    expect(onclick).not.toHaveBeenCalled();
  });
});
