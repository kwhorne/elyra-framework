/** AppShell, NavItem, StatusBar, Tabs, and the small parts. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { writable } from "svelte/store";
import LayoutFixture from "./test/LayoutFixture.svelte";
import { abilities, assistant, connection, grant, inspector, openAssistant, openInspector, outbox, route, transfers } from "./test/runtime.js";
import Pagination from "./Pagination.svelte";
import Spinner from "./Spinner.svelte";
import Separator from "./Separator.svelte";

const NAV = [
  { label: "Customers", path: "/customers" },
  { label: "Notifications", path: "/notifications", count: writable(3) },
  { label: "Reports", path: "/reports", can: "reports.view" },
];

describe("AppShell and NavItem", () => {
  it("lay out the window: toolbar, navigation, page, status bar", () => {
    render(LayoutFixture, { nav: NAV });
    expect(screen.getByRole("banner").textContent).toContain("CRM");
    expect(screen.getByRole("button", { name: "New customer" })).toBeTruthy();
    const navigation = screen.getByRole("navigation", { name: "Main" });
    expect(within(navigation).getByRole("link", { name: "Customers" }).getAttribute("href")).toBe("#/customers");
    expect(screen.getByRole("main")).toBeTruthy();
    expect(screen.getByRole("contentinfo", { name: "Status" })).toBeTruthy();
  });

  it("mark the current page, under it too, and follow the route", async () => {
    route.set({ path: "/customers/7", query: {} });
    render(LayoutFixture, { nav: NAV });
    const customers = screen.getByRole("link", { name: "Customers" });
    expect(customers.getAttribute("aria-current")).toBe("page");
    route.set({ path: "/notifications", query: {} });
    await vi.waitFor(() => expect(customers.getAttribute("aria-current")).toBeNull());
    expect(screen.getByRole("link", { name: /Notifications/ }).getAttribute("aria-current")).toBe("page");
  });

  it("show a count, and hide what the user may not see", () => {
    grant(["customers.*"]);
    render(LayoutFixture, { nav: NAV });
    expect(screen.getByRole("link", { name: /Notifications/ }).textContent).toContain("3");
    expect(screen.queryByRole("link", { name: "Reports" })).toBeNull();
  });

  it("put the navigation across the top", () => {
    render(LayoutFixture, { nav: NAV, layout: "topbar" });
    expect(within(screen.getByRole("banner")).getByRole("navigation", { name: "Main" })).toBeTruthy();
    expect(screen.queryByRole("complementary")).toBeNull();
  });
});

describe("StatusBar", () => {
  it("offers the inspector in a debug build, and says when it's simulating", async () => {
    inspector.available.mockResolvedValue(true);
    render(LayoutFixture);
    const bar = screen.getByRole("contentinfo", { name: "Status" });
    const button = await within(bar).findByRole("button", { name: "Inspector" });
    await userEvent.click(button);
    expect(openInspector).toHaveBeenCalledOnce();
    abilities.set({ loaded: true, abilities: ["*"], masked: ["customers.delete", "invoices.*"] });
    const simulating = await within(bar).findByRole("button", { name: "Simulating −2" });
    expect(simulating.title).toBe("Simulating without customers.delete, invoices.*");
  });

  it("has no inspector button in a release build, or when turned off", async () => {
    render(LayoutFixture);
    const bar = screen.getByRole("contentinfo", { name: "Status" });
    await within(bar).findByRole("button", { name: "Assistant" });
    expect(within(bar).queryByRole("button", { name: "Inspector" })).toBeNull();
  });

  it("says nothing while all is well, and offers the assistant", async () => {
    render(LayoutFixture);
    const bar = screen.getByRole("contentinfo", { name: "Status" });
    const button = await within(bar).findByRole("button", { name: "Assistant" });
    expect(bar.textContent.replace("Assistant", "").trim()).toBe("");
    await userEvent.click(button);
    expect(openAssistant).toHaveBeenCalledOnce();
  });

  it("shows offline, the outbox and the transfers", async () => {
    connection.set({ state: "offline", since: Date.UTC(2026, 9, 31, 9, 42), pending: 2, failed: 1 });
    outbox.set([{ status: "pending" }, { status: "pending" }, { status: "failed" }]);
    transfers.set([
      { id: "t1", direction: "upload", name: "contract.pdf", sent: 40, total: 100, state: "running" },
      { id: "t2", direction: "download", name: "old.pdf", sent: 10, total: 10, state: "done" },
    ]);
    render(LayoutFixture);
    const bar = screen.getByRole("contentinfo", { name: "Status" });
    expect(within(bar).getByText(/^Offline since \d/).getAttribute("role")).toBe("status");
    expect(bar.textContent).toContain("2 changes waiting");
    expect(bar.textContent).toContain("1 change refused");
    expect(bar.textContent).toContain("Uploading contract.pdf 40 %");
    expect(bar.textContent).not.toContain("old.pdf");
  });

  it("has no assistant button in an app without one", async () => {
    assistant.source.mockRejectedValue(new Error("this app has no assistant"));
    render(LayoutFixture);
    await vi.waitFor(() => expect(assistant.source).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 10));
    expect(screen.queryByRole("button", { name: "Assistant" })).toBeNull();
  });
});

describe("Tabs", () => {
  it("are a tablist the arrow keys move through, past a disabled tab", async () => {
    render(LayoutFixture);
    const list = screen.getByRole("tablist", { name: "Customer" });
    const tabs = within(list).getAllByRole("tab").map((t) => t.textContent.trim());
    expect(tabs).toEqual(["Details", "Invoices", "Notes", "Audit", "Files"]);
    const details = screen.getByRole("tab", { name: "Details" });
    expect(details.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tabpanel").textContent).toContain("Panel: details");
    expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(details.id);
    details.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(document.activeElement.textContent.trim()).toBe("Invoices");
    expect(screen.getByRole("tabpanel").textContent).toContain("Panel: invoices");
    await userEvent.keyboard("{ArrowRight}");
    expect(document.activeElement.textContent.trim()).toBe("Audit");
    await userEvent.keyboard("{End}");
    expect(document.activeElement.textContent.trim()).toBe("Files");
    await userEvent.keyboard("{ArrowRight}");
    expect(document.activeElement.textContent.trim()).toBe("Details");
    expect(screen.getByRole("tab", { name: "Files" }).tabIndex).toBe(-1);
  });

  it("hide a tab the user may not see", () => {
    grant(["customers.view"]);
    render(LayoutFixture);
    expect(screen.queryByRole("tab", { name: "Audit" })).toBeNull();
  });
});

describe("the small parts", () => {
  it("a card is labelled by its title, a danger callout is an alert and can be dismissed", async () => {
    const ondismiss = vi.fn();
    render(LayoutFixture, { ondismiss });
    expect(screen.getByRole("region", { name: "Ada Lovelace" }).textContent).toContain("Customer since 2021");
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Two invoices are overdue.");
    await userEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(ondismiss).toHaveBeenCalledOnce();
  });

  it("pagination shows the pages around the current one", async () => {
    const onchange = vi.fn();
    render(Pagination, { page: 5, lastPage: 9, onchange });
    const nav = screen.getByRole("navigation", { name: "Pagination" });
    expect([...nav.children].map((el) => el.textContent.trim())).toEqual(
      ["Previous", "1", "…", "4", "5", "6", "…", "9", "Next"],
    );
    expect(screen.getByRole("button", { name: "Page 5" }).getAttribute("aria-current")).toBe("page");
    await userEvent.click(screen.getByRole("button", { name: "Page 9" }));
    expect(onchange).toHaveBeenCalledWith(9);
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("button", { name: "Next" }).disabled).toBe(true);
  });

  it("a spinner and a separator say what they are", () => {
    render(Spinner, { label: "Saving" });
    expect(screen.getByRole("status", { name: "Saving" })).toBeTruthy();
    render(Separator, { orientation: "vertical" });
    expect(screen.getByRole("separator").getAttribute("aria-orientation")).toBe("vertical");
  });
});
