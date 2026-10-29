import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Notification } from "@tandem/protocol";
import { LiveAnnouncer, UiProvider, resetAnnouncer } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { followNotifications, useNotifications } from "../../stores/notifications";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { NotificationsBell } from "./NotificationsPopover";

const note = (id: string, extra: Partial<Notification>): Notification => ({
  id,
  kind: "done",
  title: "Ada finished",
  body: "Claude Code · shop",
  source: { type: "pane", paneId: "pn_1" },
  state: "unread",
  dedupeKey: null,
  createdAt: Date.now() - 60_000,
  readAt: null,
  ...extra,
});

function setup(items: Notification[]): Call[] {
  return fakeBackend({
    notifications_list: () => ({
      items,
      nextCursor: null,
      unread: items.filter((n) => n.state === "unread").length,
    }),
    notifications_mark_read: () => null,
    focus_source: () => null,
    approval_resolve: () => null,
  });
}

describe("notification center (F-DB-02)", () => {
  beforeEach(() => resetStores());

  it("the bell counts unread items; the popover opens on Waiting on you when something needs you", async () => {
    setup([
      note("ntf_1", {
        kind: "approval",
        title: "Bea needs approval",
        dedupeKey: "approval:ap_1",
        body: "Claude Code · shop — Run `pnpm migrate`",
      }),
      note("ntf_2", {}),
      note("ntf_3", { state: "read" }),
    ]);
    render(
      <UiProvider>
        <NotificationsBell />
      </UiProvider>,
    );
    const bell = await screen.findByRole("button", { name: "Notifications, 2 unread" });
    fireEvent.click(bell);
    const pop = await screen.findByTestId("notifications-popover");
    expect(
      within(pop)
        .getAllByTestId("notification")
        .map((n) => n.dataset.kind),
    ).toEqual(["approval"]);
    fireEvent.click(within(pop).getByRole("radio", { name: "All" }));
    expect(within(pop).getAllByTestId("notification")).toHaveLength(3);
  });

  it("clicking an item marks it read and asks the core to focus its source", async () => {
    const calls = setup([note("ntf_1", { source: { type: "pane", paneId: "pn_9" } })]);
    render(
      <UiProvider>
        <NotificationsBell />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Notifications, 1 unread" }));
    fireEvent.click(await screen.findByTestId("notification"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "focus_source")?.args).toEqual({
        source: { type: "pane", paneId: "pn_9" },
      }),
    );
    expect(calls.find((c) => c.cmd === "notifications_mark_read")?.args).toEqual({ ids: ["ntf_1"] });
    expect(useNotifications.getState().unread).toBe(0);
    expect(useNotifications.getState().open).toBe(false);
  });

  it("F-DB-02.1 an approval answered elsewhere resolves its item and leaves Waiting on you", async () => {
    setup([note("ntf_1", { kind: "approval", dedupeKey: "approval:ap_1" })]);
    followNotifications();
    render(
      <UiProvider>
        <NotificationsBell />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Notifications/ }));
    const pop = await screen.findByTestId("notifications-popover");
    expect(within(pop).getByTestId("notification-allow")).toBeTruthy();
    act(() => emitLocal("notifications:changed", { ids: ["ntf_1"], state: "resolved" }));
    await waitFor(() => expect(within(pop).queryByTestId("notification-allow")).toBeNull());
    expect(within(pop).getByText("You're all caught up")).toBeTruthy();
  });

  it("new items arrive live and count", async () => {
    setup([]);
    followNotifications();
    render(
      <UiProvider>
        <NotificationsBell />
      </UiProvider>,
    );
    await screen.findByRole("button", { name: "Notifications" });
    act(() => emitLocal("notifications:new", { notification: note("ntf_7", {}), sound: false }));
    expect(await screen.findByRole("button", { name: "Notifications, 1 unread" })).toBeTruthy();
  });

  it("§5.4 a new notification is read out by the polite live region: its title, not its source line", async () => {
    resetAnnouncer();
    setup([]);
    followNotifications();
    render(
      <UiProvider>
        <NotificationsBell />
        <LiveAnnouncer />
      </UiProvider>,
    );
    await screen.findByRole("button", { name: "Notifications" });
    act(() =>
      emitLocal("notifications:new", {
        notification: note("ntf_8", {
          kind: "approval",
          title: "Bea needs approval",
          body: "Claude Code · shop — Run `pnpm migrate`",
        }),
        sound: false,
      }),
    );
    const region = screen.getByTestId("live-announcer");
    expect(region.textContent).toBe("Bea needs approval");
    expect(region.getAttribute("aria-live")).toBe("polite");
    expect(region.textContent).not.toContain("pnpm migrate");
    resetAnnouncer();
  });

  it("F-TL-12.12 a notification arriving in the frame of a click doesn't slide another Allow once under it", async () => {
    // Rows are 60 high, in page order.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const row = this.closest('[data-testid="notification"]');
      const rows = Array.from(document.querySelectorAll('[data-testid="notification"]'));
      const top = row ? rows.indexOf(row) * 60 : 0;
      return {
        left: 0,
        top,
        width: 300,
        height: 60,
        right: 300,
        bottom: top + 60,
        x: 0,
        y: top,
        toJSON: () => ({}),
      } as DOMRect;
    });
    const calls = setup([
      note("ntf_1", { kind: "approval", dedupeKey: "approval:ap_1", title: "Bea needs approval" }),
    ]);
    followNotifications();
    render(
      <UiProvider>
        <NotificationsBell />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Notifications/ }));
    const pop = await screen.findByTestId("notifications-popover");
    const allowFor = (title: string) =>
      within(
        within(pop)
          .getAllByTestId("notification")
          .find((r) => r.textContent?.includes(title)) as HTMLElement,
      ).getByTestId("notification-allow") as HTMLButtonElement;
    await waitFor(() => expect(allowFor("Bea").disabled).toBe(false));
    // An agent's approval lands on top, pushing Bea's row down, just as the
    // person clicks where Bea's Allow once was.
    act(() =>
      emitLocal("notifications:new", {
        notification: note("ntf_2", {
          kind: "approval",
          dedupeKey: "approval:ap_2",
          title: "Cal needs approval",
        }),
        sound: false,
      }),
    );
    fireEvent.click(allowFor("Bea"));
    fireEvent.click(allowFor("Cal"));
    await new Promise((r) => setTimeout(r, 50));
    expect(calls.some((c) => c.cmd === "approval_resolve")).toBe(false);
    // Once still, Bea's answers.
    await waitFor(() => expect(allowFor("Bea").disabled).toBe(false));
    fireEvent.click(allowFor("Bea"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "approval_resolve")?.args).toMatchObject({ approvalId: "ap_1" }),
    );
    vi.restoreAllMocks();
  });
});
