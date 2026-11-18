/** Form and Field: labels, descriptions, Laravel's errors per field, busy. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { CommandError, ValidationError } from "@elyra/runtime";
import FormFixture from "./test/FormFixture.svelte";
import DirtyFormFixture from "./test/DirtyFormFixture.svelte";

describe("Form and Field", () => {
  it("label the control, and describe it", () => {
    render(FormFixture, { onsubmit: vi.fn() });
    const name = screen.getByLabelText(/Name/);
    expect(name.tagName).toBe("INPUT");
    expect(name.required).toBe(true);
    const described = document.getElementById(name.getAttribute("aria-describedby"));
    expect(described.textContent).toBe("As on the invoice.");
    expect(name.getAttribute("aria-invalid")).toBeNull();
  });

  it("put each of Laravel's messages under its field, and the rest at the top", async () => {
    const onsubmit = vi.fn(async () => {
      throw new ValidationError(
        "customers_store",
        { name: ["The name field is required."], team_id: ["The team is gone."] },
        "{}",
      );
    });
    render(FormFixture, { onsubmit });
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    const name = screen.getByLabelText(/Name/);
    await vi.waitFor(() => expect(name.getAttribute("aria-invalid")).toBe("true"));
    expect(document.getElementById(name.getAttribute("aria-describedby")).textContent).toBe(
      "The name field is required.",
    );
    expect(screen.getByLabelText("Email").getAttribute("aria-invalid")).toBeNull();
    expect(screen.getByRole("alert").textContent).toBe("The team is gone.");

    // Saved again: the old messages go.
    onsubmit.mockResolvedValueOnce(undefined);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(name.getAttribute("aria-invalid")).toBeNull());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("show any other failure at the top", async () => {
    render(FormFixture, {
      onsubmit: async () => {
        throw new CommandError("customers_store", "The server is down.", "server");
      },
    });
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toBe("The server is down."));
  });

  it("are busy while the save runs", async () => {
    let finish;
    render(FormFixture, { onsubmit: () => new Promise((r) => (finish = r)) });
    const save = screen.getByRole("button", { name: "Save" });
    await userEvent.click(save);
    await vi.waitFor(() => expect(save.getAttribute("aria-busy")).toBe("true"));
    expect(save.disabled).toBe(true);
    expect(document.querySelector("form").getAttribute("aria-busy")).toBe("true");
    finish();
    await vi.waitFor(() => expect(save.getAttribute("aria-busy")).toBeNull());
  });

  it("bind what's typed", async () => {
    const onsubmit = vi.fn();
    const { component } = render(FormFixture, { onsubmit });
    await userEvent.type(screen.getByLabelText(/Name/), "Ada");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(onsubmit).toHaveBeenCalledOnce());
    expect(component).toBeTruthy();
    expect(screen.getByLabelText(/Name/).value).toBe("Ada");
  });
});

describe("Form: after a 422, and unsaved changes", () => {
  it("focuses the first field with a message, in the form's order", async () => {
    const onsubmit = vi.fn(async () => {
      throw new ValidationError("customers_store", { email: ["Not an email."] }, "{}");
    });
    render(FormFixture, { onsubmit });
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Email")));
  });

  it("is dirty once something's typed, and clean again once it's saved", async () => {
    const oninput = vi.fn();
    const onsubmit = vi.fn(async () => {});
    render(DirtyFormFixture, { onsubmit, oninput });
    const dirty = () => screen.getByTestId("dirty").textContent;
    expect(dirty()).toBe("false");
    await userEvent.type(screen.getByLabelText("Name"), "Ada");
    expect(dirty()).toBe("true");
    expect(oninput).toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(dirty()).toBe("false"));
  });

  it("stays dirty when the save fails", async () => {
    const onsubmit = vi.fn(async () => {
      throw new CommandError("customers_update", "unreachable", "No answer.");
    });
    render(DirtyFormFixture, { onsubmit });
    await userEvent.type(screen.getByLabelText("Name"), "Ada");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getByTestId("dirty").textContent).toBe("true");
  });

  it("isn't dirty from a Select's search, only from what's picked", async () => {
    render(DirtyFormFixture, { onsubmit: vi.fn() });
    const dirty = () => screen.getByTestId("dirty").textContent;
    await userEvent.click(screen.getByLabelText("Country"));
    await userEvent.type(screen.getByLabelText("Search the options"), "Swe");
    expect(dirty()).toBe("false");
    await userEvent.click(screen.getByRole("option", { name: /Sweden/ }));
    await vi.waitFor(() => expect(dirty()).toBe("true"));
  });

  it("a Switch makes it dirty", async () => {
    render(DirtyFormFixture, { onsubmit: vi.fn() });
    await userEvent.click(screen.getByRole("switch"));
    expect(screen.getByTestId("dirty").textContent).toBe("true");
  });
});
