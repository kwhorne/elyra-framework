/** OutboxDrawer, and the StatusBar's counts that open it. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import OutboxDrawer from "./OutboxDrawer.svelte";
import StatusBar from "./StatusBar.svelte";
import { confirm, outbox } from "./test/runtime.js";

const SAVED = Date.UTC(2026, 9, 31, 9, 42);

const CONFLICT = {
  id: 1, method: "PUT", path: "/api/customers/42", body: { name: "Ada Lovelace", credit_limit: 50000 },
  status: "failed", attempts: 1, error: "Someone changed this customer meanwhile.", errors: null,
  reason: "conflict", createdAt: SAVED,
};
const INVALID = {
  id: 2, method: "POST", path: "/api/customers", body: { name: "Grace", email: "ada@example.com", contract: { file: "contract.pdf" } },
  status: "failed", attempts: 1, error: "The given data was invalid.",
  errors: { email: ["The email has already been taken."], vat: ["The vat field is required."] },
  reason: "validation", createdAt: SAVED,
};
const WAITING = {
  id: 3, method: "DELETE", path: "/api/customers/7", body: null,
  status: "pending", attempts: 3, error: null, errors: null, reason: null, createdAt: SAVED,
};

const item = (name) => screen.getByRole("listitem", { name });

describe("OutboxDrawer", () => {
  it("says so when nothing is waiting", () => {
    render(OutboxDrawer, { open: true });
    const dialog = screen.getByRole("dialog", { name: "Changes" });
    expect(dialog.textContent).toContain("Nothing waiting");
  });

  it("lists the refused writes first, then the waiting ones", () => {
    outbox.set([WAITING, CONFLICT, INVALID]);
    render(OutboxDrawer, { open: true });
    const refused = screen.getByRole("region", { name: /Refused/ });
    const waiting = screen.getByRole("region", { name: /Waiting/ });
    expect(refused.compareDocumentPosition(waiting) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(refused).getAllByRole("listitem").map((li) => li.getAttribute("aria-label"))).toEqual([
      "Update /api/customers/42",
      "Create /api/customers",
    ]);
    expect(within(waiting).getByRole("listitem").getAttribute("aria-label")).toBe("Delete /api/customers/7");
    expect(item("Delete /api/customers/7").textContent).toContain("tried 3 times");
  });

  it("shows what the server said, per field, and a file by its name", () => {
    outbox.set([INVALID]);
    render(OutboxDrawer, { open: true });
    const write = item("Create /api/customers");
    expect(write.textContent).toContain("Not valid");
    // Laravel's summary repeats a field's message; the fields say it.
    expect(write.textContent).not.toContain("The given data was invalid.");
    expect(within(write).getByText("contract.pdf").tagName).toBe("SPAN");
    expect(write.textContent).toContain("The email has already been taken.");
    expect(within(write).getAllByText("The email has already been taken.")).toHaveLength(1);
    // An error for a field the write didn't have still shows.
    expect(write.textContent).toContain("The vat field is required.");
  });

  it("shows what the server said when it gave no fields", () => {
    outbox.set([{ ...INVALID, errors: null, reason: "denied", error: "This action is unauthorized." }]);
    render(OutboxDrawer, { open: true });
    const write = item("Create /api/customers");
    expect(write.textContent).toContain("Not allowed");
    expect(write.textContent).toContain("This action is unauthorized.");
  });

  it("resolves a conflict: keep mine, or take theirs", async () => {
    outbox.set([CONFLICT, { ...CONFLICT, id: 4, path: "/api/customers/43" }]);
    render(OutboxDrawer, { open: true });
    const mine = item("Update /api/customers/42");
    expect(mine.textContent).toContain("Changed meanwhile");
    expect(within(mine).queryByRole("button", { name: "Send again" })).toBeNull();
    await userEvent.click(within(mine).getByRole("button", { name: "Keep mine" }));
    expect(outbox.keepMine).toHaveBeenCalledWith(1);
    await userEvent.click(within(item("Update /api/customers/43")).getByRole("button", { name: "Take theirs" }));
    expect(outbox.takeTheirs).toHaveBeenCalledWith(4);
    expect(screen.getByRole("dialog").textContent).toContain("Nothing waiting");
  });

  it("sends a refused write again, or discards it after asking", async () => {
    outbox.set([INVALID, { ...INVALID, id: 5, path: "/api/invoices" }]);
    render(OutboxDrawer, { open: true });
    await userEvent.click(within(item("Create /api/customers")).getByRole("button", { name: "Send again" }));
    expect(outbox.retry).toHaveBeenCalledWith(2);

    const invoice = item("Create /api/invoices");
    confirm.mockResolvedValueOnce(false);
    await userEvent.click(within(invoice).getByRole("button", { name: "Discard" }));
    expect(confirm).toHaveBeenCalledWith("Discard this change? It won't be sent.", { danger: true, confirmLabel: "Discard" });
    expect(outbox.discard).not.toHaveBeenCalled();
    await userEvent.click(within(invoice).getByRole("button", { name: "Discard" }));
    expect(outbox.discard).toHaveBeenCalledWith(5);
  });

  it("discards a waiting write", async () => {
    outbox.set([WAITING]);
    render(OutboxDrawer, { open: true });
    await userEvent.click(within(item("Delete /api/customers/7")).getByRole("button", { name: "Discard" }));
    expect(outbox.discard).toHaveBeenCalledWith(3);
  });

  it("says when a way out fails, and keeps the write", async () => {
    outbox.set([CONFLICT]);
    outbox.keepMine.mockRejectedValueOnce(new Error("The backend can't be reached."));
    render(OutboxDrawer, { open: true });
    const write = item("Update /api/customers/42");
    await userEvent.click(within(write).getByRole("button", { name: "Keep mine" }));
    expect((await within(write).findByRole("alert")).textContent).toBe("The backend can't be reached.");
    expect(within(write).getByRole("button", { name: "Keep mine" }).disabled).toBe(false);
  });

  it("names a write the app's way", () => {
    outbox.set([CONFLICT]);
    render(OutboxDrawer, { open: true, describe: (w) => `Customer ${w.path.split("/").pop()}` });
    expect(item("Customer 42")).toBeTruthy();
  });
});

describe("StatusBar's outbox", () => {
  it("opens the drawer from its counts, and Esc closes it", async () => {
    outbox.set([CONFLICT, WAITING]);
    render(StatusBar, { assistant: false });
    const bar = screen.getByRole("contentinfo", { name: "Status" });
    expect(screen.queryByRole("dialog")).toBeNull();
    const refused = within(bar).getByRole("button", { name: "1 change refused" });
    await userEvent.click(refused);
    const dialog = screen.getByRole("dialog", { name: "Changes" });
    expect(within(dialog).getByRole("listitem", { name: "Update /api/customers/42" })).toBeTruthy();
    await userEvent.keyboard("{Escape}");
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(refused);
    await userEvent.click(within(bar).getByRole("button", { name: "1 change waiting" }));
    expect(screen.getByRole("dialog", { name: "Changes" })).toBeTruthy();
  });

  it("has no drawer with the outbox turned off", () => {
    outbox.set([CONFLICT]);
    render(StatusBar, { assistant: false, outbox: false });
    expect(screen.queryByRole("button", { name: /refused/ })).toBeNull();
  });
});
