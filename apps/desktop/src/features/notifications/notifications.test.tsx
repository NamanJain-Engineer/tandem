import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Notification } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
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
});
