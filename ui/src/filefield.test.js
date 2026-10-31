/** FileField: the native dialog, the record's file, dropping on the window, the field's label and error. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { dialog, dropFiles, dropListeners } from "./test/runtime.js";
import FileFieldFixture from "./test/FileFieldFixture.svelte";

const PDF = { id: "f1", path: "/Users/ada/receipt.pdf", name: "receipt.pdf", kind: "open" };
const PHOTO = { id: "f2", path: "/Users/ada/photo.jpg", name: "photo.jpg", kind: "open" };
const FILTERS = [{ name: "Receipts", extensions: ["pdf", "jpg"] }];

const choose = () => screen.getByRole("button", { name: "Receipt" });

describe("FileField", () => {
  it("is the field's labelled button, described by the file's name and the field's help", () => {
    render(FileFieldFixture);
    const button = choose();
    expect(button.textContent.trim()).toBe("Choose a file…");
    const described = button.getAttribute("aria-describedby").split(" ").map((id) => document.getElementById(id).textContent.trim());
    expect(described).toEqual(["No file", "A PDF or a photo."]);
  });

  it("opens the native dialog with the filters, and takes the first file chosen", async () => {
    const onchange = vi.fn();
    dialog.open.mockResolvedValue([PDF, PHOTO]);
    render(FileFieldFixture, { filters: FILTERS, onchange });
    await userEvent.click(choose());
    expect(dialog.open).toHaveBeenCalledWith({ filters: FILTERS });
    expect(screen.getByTestId("file").textContent).toBe("receipt.pdf");
    expect(onchange).toHaveBeenCalledWith(PDF);
    expect(choose().textContent.trim()).toBe("Replace…");
    expect(screen.getByText("receipt.pdf", { selector: ".name" })).toBeTruthy();
  });

  it("keeps what it had when the dialog is cancelled", async () => {
    const onchange = vi.fn();
    render(FileFieldFixture, { file: PDF, onchange });
    await userEvent.click(choose());
    expect(screen.getByTestId("file").textContent).toBe("receipt.pdf");
    expect(onchange).not.toHaveBeenCalled();
  });

  it("is busy while the dialog is open", async () => {
    let answer;
    dialog.open.mockImplementation(() => new Promise((resolve) => (answer = resolve)));
    render(FileFieldFixture);
    await userEvent.click(choose());
    expect(choose().disabled).toBe(true);
    expect(choose().getAttribute("aria-busy")).toBe("true");
    answer([PHOTO]);
    await vi.waitFor(() => expect(choose().disabled).toBe(false));
    expect(screen.getByTestId("file").textContent).toBe("photo.jpg");
  });

  it("says when the dialog fails", async () => {
    dialog.open.mockRejectedValue({ detail: "The app has no `system` feature" });
    render(FileFieldFixture);
    await userEvent.click(choose());
    expect(screen.getByRole("alert").textContent).toBe("The app has no `system` feature");
    dialog.open.mockResolvedValue([PDF]);
    await userEvent.click(choose());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows the record's file, and a new one can be taken back", async () => {
    const onchange = vi.fn();
    dialog.open.mockResolvedValue([PDF]);
    render(FileFieldFixture, { current: "2026-10-contract.pdf", onchange });
    expect(screen.getByText("2026-10-contract.pdf")).toBeTruthy();
    expect(choose().textContent.trim()).toBe("Replace…");
    expect(screen.queryByRole("button", { name: "Keep the current one" })).toBeNull();
    await userEvent.click(choose());
    expect(screen.getByText("receipt.pdf", { selector: ".name" })).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Keep the current one" }));
    expect(onchange).toHaveBeenLastCalledWith(null);
    expect(screen.getByTestId("file").textContent).toBe("none");
    expect(screen.getByText("2026-10-contract.pdf")).toBeTruthy();
  });

  it("removes a chosen file when the record has none", async () => {
    render(FileFieldFixture, { file: PDF });
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(screen.getByTestId("file").textContent).toBe("none");
    expect(screen.getByText("No file")).toBeTruthy();
  });

  it("doesn't submit the form", async () => {
    const onsubmit = vi.fn(async () => {});
    render(FileFieldFixture, { onsubmit, file: PDF });
    await userEvent.click(choose());
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onsubmit).not.toHaveBeenCalled();
  });

  it("takes a file dropped on the window, with acceptsDrop, while it's shown", async () => {
    const { rerender } = render(FileFieldFixture, { acceptsDrop: true });
    expect(screen.getByText("or drop one on the window")).toBeTruthy();
    dropFiles([{ ...PHOTO, kind: "folder", name: "Photos" }, PDF]);
    await vi.waitFor(() => expect(screen.getByTestId("file").textContent).toBe("receipt.pdf"));
    await rerender({ shown: false });
    expect(dropListeners()).toBe(0);
  });

  it("ignores a drop without acceptsDrop", async () => {
    render(FileFieldFixture, { acceptsDrop: false });
    expect(dropListeners()).toBe(0);
    expect(screen.queryByText("or drop one on the window")).toBeNull();
    dropFiles([PDF]);
    expect(screen.getByTestId("file").textContent).toBe("none");
  });

  it("ignores a dropped folder", async () => {
    const onchange = vi.fn();
    render(FileFieldFixture, { acceptsDrop: true, file: PDF, onchange });
    dropFiles([{ ...PHOTO, kind: "folder" }]);
    expect(screen.getByTestId("file").textContent).toBe("receipt.pdf");
    expect(onchange).not.toHaveBeenCalled();
  });

  it("shows the field's error", () => {
    render(FileFieldFixture, { error: "The receipt must be a PDF." });
    expect(choose().getAttribute("aria-invalid")).toBe("true");
    const ids = choose().getAttribute("aria-describedby").split(" ");
    expect(document.getElementById(ids[1]).textContent).toBe("The receipt must be a PDF.");
  });

  it("is disabled: no dialog, no undo, no drop", async () => {
    render(FileFieldFixture, { disabled: true, file: PDF, acceptsDrop: true });
    expect(choose().disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
    expect(dropListeners()).toBe(0);
  });
});
