import { create } from "zustand";
import type { Notification, NotificationPage, NotificationState } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { playSound } from "../features/notifications/sound";
import { useSettings } from "./settings";

type NotificationsStore = {
  /** The bell's popover (Mod+Shift+N opens it too). */
  open: boolean;
  setOpen: (open: boolean) => void;
  items: Notification[];
  unread: number;
  nextCursor: string | null;
  loaded: boolean;
  load: () => Promise<void>;
  loadMore: () => Promise<void>;
  markRead: (ids: string[]) => Promise<void>;
  markAllRead: () => Promise<void>;
};

const countUnread = (items: Notification[]) => items.filter((n) => n.state === "unread").length;

export const useNotifications = create<NotificationsStore>((set, get) => ({
  open: false,
  setOpen: (open) => set({ open }),
  items: [],
  unread: 0,
  nextCursor: null,
  loaded: false,
  load: async () => {
    const page = await invoke<NotificationPage>("notifications_list", {}).catch(() => null);
    if (page) set({ items: page.items, unread: page.unread, nextCursor: page.nextCursor, loaded: true });
    else set({ loaded: true });
  },
  loadMore: async () => {
    const cursor = get().nextCursor;
    if (!cursor) return;
    const page = await invoke<NotificationPage>("notifications_list", { cursor }).catch(() => null);
    if (!page) return;
    const known = new Set(get().items.map((n) => n.id));
    set({ items: [...get().items, ...page.items.filter((n) => !known.has(n.id))], nextCursor: page.nextCursor, unread: page.unread });
  },
  markRead: async (ids) => {
    apply(ids, "read");
    await invoke("notifications_mark_read", { ids }).catch(() => undefined);
  },
  markAllRead: async () => {
    apply(
      get()
        .items.filter((n) => n.state === "unread")
        .map((n) => n.id),
      "read",
    );
    await invoke("notifications_mark_read", { all: true }).catch(() => undefined);
  },
}));

/** Reads only move unread → read; resolving wins over both. */
function apply(ids: string[], state: NotificationState): void {
  const set = new Set(ids);
  const items = useNotifications.getState().items.map((n) => {
    if (!set.has(n.id)) return n;
    if (state === "read" && n.state !== "unread") return n;
    return { ...n, state };
  });
  const before = useNotifications.getState();
  const unreadDelta = countUnread(before.items) - countUnread(items);
  useNotifications.setState({ items, unread: Math.max(0, before.unread - unreadDelta) });
}

let following = false;

export function followNotifications(): void {
  if (following) return;
  following = true;
  void listen<{ notification: Notification; sound: boolean }>("notifications:new", ({ notification, sound }) => {
    const s = useNotifications.getState();
    if (s.items.some((n) => n.id === notification.id)) return;
    useNotifications.setState({ items: [notification, ...s.items], unread: s.unread + 1 });
    if (sound) {
      const name = useSettings.getState().values.notify_sound_name;
      playSound(typeof name === "string" ? name : "chime");
    }
  });
  void listen<{ ids: string[]; state: NotificationState }>("notifications:changed", ({ ids, state }) => apply(ids, state));
}

/** Tests. */
export function resetNotifications(): void {
  useNotifications.setState({ open: false, items: [], unread: 0, nextCursor: null, loaded: false });
}
