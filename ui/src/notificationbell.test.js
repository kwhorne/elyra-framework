/** NotificationBell: the count on the bell, the panel, reading one, and the rest of the list. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import NotificationBell from "./NotificationBell.svelte";
import { connection, navigate, notifications } from "./test/runtime.js";

const minutesAgo = (n) => new Date(Date.now() - n * 60_000).toISOString();
const INVOICE = {
  id: "a1", kind: "App\\Notifications\\InvoicePaid", readAt: null, createdAt: minutesAgo(5),
  data: { title: "Invoice 2041 paid", body: "Ada Lovelace AS paid 12 500 kr", url: "/invoices/2041" },
};
const MENTION = {
  id: "b2", kind: "App\\Notifications\\Mentioned", readAt: null, createdAt: minutesAgo(180),
  data: { title: "Linus mentioned you", url: "https://example.com/elsewhere" },
};
const OLD = {
  id: "c3", kind: "App\\Notifications\\Weekly", readAt: "2026-10-01T08:00:00Z", createdAt: "2026-10-01T08:00:00Z",
  data: { summary: "Your week" },
};

const bell = () => screen.getByRole("button", { name: /^Notifications/ });
const panel = () => screen.getByRole("dialog", { name: "Notifications" });

describe("NotificationBell", () => {
  it("shows the unread count on the bell, and says it", async () => {
    render(NotificationBell);
    expect(bell().getAttribute("aria-label")).toBe("Notifications");
    expect(bell().querySelector(".count")).toBeNull();
    notifications.set({ unread: 2, items: [INVOICE, MENTION], hasMore: false });
    await tick();
    expect(bell().getAttribute("aria-label")).toBe("Notifications, 2 unread");
    expect(bell().querySelector(".count").textContent).toBe("2");
    notifications.set({ unread: 140, items: [], hasMore: false });
    await tick();
    expect(bell().querySelector(".count").textContent).toBe("99+");
  });

  it("opens a panel with the newest, unread ones marked, and closes with Esc back on the bell", async () => {
    const user = userEvent.setup();
    notifications.set({ unread: 2, items: [INVOICE, MENTION, OLD], hasMore: false });
    render(NotificationBell);
    await user.click(bell());
    expect(bell().getAttribute("aria-expanded")).toBe("true");
    const items = within(panel()).getAllByRole("listitem");
    expect(items.map((li) => li.querySelector(".title").textContent)).toEqual(["Invoice 2041 paid", "Linus mentioned you", "Notification"]);
    expect(items.map((li) => li.classList.contains("unread"))).toEqual([true, true, false]);
    expect(items[0].querySelector(".body").textContent).toBe("Ada Lovelace AS paid 12 500 kr");
    const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
    expect(items[0].querySelector("time").textContent).toBe(rtf.format(-5, "minute"));
    expect(items[1].querySelector("time").textContent).toBe(rtf.format(-3, "hour"));
    expect(within(panel()).getByText("2 unread")).toBeTruthy();
    // Focus moved in: the first thing to press.
    expect(document.activeElement.textContent).toBe("Mark all read");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(bell());
  });

  it("marks one read and opens its page in the app; a full URL isn't opened", async () => {
    const user = userEvent.setup();
    notifications.set({ unread: 2, items: [INVOICE, MENTION], hasMore: false });
    render(NotificationBell);
    await user.click(bell());
    await user.click(within(panel()).getByText("Invoice 2041 paid"));
    expect(notifications.markRead).toHaveBeenCalledWith("a1");
    expect(navigate).toHaveBeenCalledWith("/invoices/2041");
    expect(screen.queryByRole("dialog")).toBeNull();

    await user.click(bell());
    await user.click(within(panel()).getByText("Linus mentioned you"));
    expect(notifications.markRead).toHaveBeenCalledWith("b2");
    expect(navigate).toHaveBeenCalledTimes(1);
  });

  it("hands a click to onopen, with how it's shown; a read one isn't marked again", async () => {
    const user = userEvent.setup();
    const onopen = vi.fn();
    notifications.set({ unread: 0, items: [OLD], hasMore: false });
    render(NotificationBell, { onopen, present: (n) => ({ title: n.data.summary, body: null, url: "/weekly" }) });
    await user.click(bell());
    await user.click(within(panel()).getByText("Your week"));
    expect(notifications.markRead).not.toHaveBeenCalled();
    expect(onopen).toHaveBeenCalledWith(OLD, { title: "Your week", body: null, url: "/weekly" });
    expect(navigate).not.toHaveBeenCalled();
  });

  it("marks all read, and reads more", async () => {
    const user = userEvent.setup();
    notifications.set({ unread: 1, items: [INVOICE], hasMore: true });
    render(NotificationBell);
    await user.click(bell());
    await user.click(within(panel()).getByRole("button", { name: "Mark all read" }));
    expect(notifications.markAllRead).toHaveBeenCalled();
    await user.click(within(panel()).getByRole("button", { name: "More" }));
    expect(notifications.more).toHaveBeenCalled();
    notifications.set({ unread: 0, items: [{ ...INVOICE, readAt: "2026-11-08T08:00:00Z" }], hasMore: false });
    await tick();
    expect(within(panel()).getByRole("button", { name: "Mark all read" }).disabled).toBe(true);
    expect(within(panel()).queryByRole("button", { name: "More" })).toBeNull();
  });

  it("says what went wrong", async () => {
    const user = userEvent.setup();
    notifications.markAllRead.mockRejectedValueOnce(new Error("The server didn't answer."));
    notifications.set({ unread: 1, items: [INVOICE], hasMore: false });
    render(NotificationBell);
    await user.click(bell());
    await user.click(within(panel()).getByRole("button", { name: "Mark all read" }));
    await waitFor(() => expect(within(panel()).getByRole("alert").textContent).toBe("The server didn't answer."));
  });

  it("says so when there's nothing, and when it's offline", async () => {
    const user = userEvent.setup();
    connection.set({ state: "offline", since: Date.now(), pending: 0, failed: 0 });
    render(NotificationBell);
    await user.click(bell());
    expect(within(panel()).getByText("Nothing yet")).toBeTruthy();
    expect(within(panel()).getByText(/^Offline/)).toBeTruthy();
  });

  it("links to the page with all of them", async () => {
    const user = userEvent.setup();
    render(NotificationBell, { href: "/notifications" });
    await user.click(bell());
    const link = within(panel()).getByRole("link", { name: "See all" });
    expect(link.getAttribute("href")).toBe("#/notifications");
    await user.click(link);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes on a click outside", async () => {
    const user = userEvent.setup();
    render(NotificationBell);
    await user.click(bell());
    await user.click(document.body);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("takes its title and words from props", async () => {
    const user = userEvent.setup();
    notifications.set({ unread: 3, items: [], hasMore: false });
    render(NotificationBell, { title: "Varsler", labels: { unread: (n) => `${n} uleste`, empty: "Ingenting ennå" } });
    const button = screen.getByRole("button", { name: "Varsler, 3 uleste" });
    await user.click(button);
    expect(screen.getByRole("dialog", { name: "Varsler" })).toBeTruthy();
    expect(screen.getByText("Ingenting ennå")).toBeTruthy();
  });
});
