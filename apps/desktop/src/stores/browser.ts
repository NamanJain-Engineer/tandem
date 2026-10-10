// The browser (F-CD-17): each page's state as the core reports it
// (`browser:state`, with the committed URL), and the calls the views make.
// A page lives in the core until its tab or pane closes; views come and go
// (a reload, a move between sidebar and pane) and find it again by id.
import { create } from "zustand";
import type { BrowserCmd, BrowserPicked, BrowserState, Device, IpcError, Navigated, PickedElement, Rect } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type BrowserStore = {
  pages: Record<string, BrowserState>;
  /** What the element picker has picked in each page (F-CD-18). */
  picked: Record<string, PickedElement[]>;
  /** Pages the core couldn't show, with why. */
  unavailable: Record<string, string>;
  open: (id: string, url: string | null, bounds: Rect, visible: boolean) => Promise<IpcError | null>;
  navigate: (id: string, input: string) => Promise<IpcError | null>;
  command: (id: string, cmd: BrowserCmd) => void;
  place: (id: string, bounds: Rect, visible: boolean) => void;
  setDevice: (id: string, device: Device, rotated: boolean) => void;
  close: (id: string) => void;
  /** The page goes on under `to` (a pane opened in the sidebar, a sidebar
   * tab moved to a pane). */
  move: (from: string, to: string) => Promise<IpcError | null>;
  /** Drops what's known of a page the core closed itself (a pane's). */
  forget: (id: string) => void;
  clearData: () => Promise<IpcError | null>;
  /** The element picker on or off (F-CD-18). */
  pick: (id: string, on: boolean) => Promise<IpcError | null>;
  /** The Markdown brief of the picked elements and `note`. */
  brief: (id: string, note: string) => Promise<string | IpcError>;
};

function asError(e: unknown): IpcError {
  if (e && typeof e === "object" && "message" in e) return e as IpcError;
  return { code: "internal", message: String(e), stage: "browser", hint: null, retryable: false };
}

/** Called with every page state, e.g. to keep a sidebar tab's title. */
type Listener = (state: BrowserState) => void;
const listeners = new Set<Listener>();

export function onBrowserState(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** Takes a page's state unless the one held is as new (`seq`): a command's
 * reply can come after the events that followed it. */
function apply(state: BrowserState): void {
  const held = useBrowser.getState().pages[state.browserId];
  if (held && held.seq >= state.seq) return;
  useBrowser.setState((s) => ({ pages: { ...s.pages, [state.browserId]: state } }));
  for (const fn of listeners) fn(state);
}

export const useBrowser = create<BrowserStore>((set) => {
  return {
    pages: {},
    picked: {},
    unavailable: {},

    open: async (id, url, bounds, visible) => {
      // A page's events start once it exists: be listening first, or a
      // page restored at launch could load before anyone hears it.
      await listening;
      try {
        apply(await invoke<BrowserState>("browser_open", { browserId: id, url, bounds, visible }));
        set((s) => {
          const { [id]: _, ...rest } = s.unavailable;
          return { unavailable: rest };
        });
        return null;
      } catch (e) {
        const err = asError(e);
        if (err.code === "unavailable") set((s) => ({ unavailable: { ...s.unavailable, [id]: err.message } }));
        return err;
      }
    },

    navigate: async (id, input) => {
      try {
        await invoke<Navigated>("browser_navigate", { browserId: id, input });
        return null;
      } catch (e) {
        return asError(e);
      }
    },

    command: (id, cmd) => void invoke("browser_command", { browserId: id, cmd }).catch(() => undefined),

    place: (id, bounds, visible) =>
      void invoke("browser_set_bounds", { browserId: id, bounds, visible }).catch(() => undefined),

    setDevice: (id, device, rotated) =>
      void invoke<BrowserState>("browser_set_device", { browserId: id, device, rotated })
        .then(apply)
        .catch(() => undefined),

    close: (id) => {
      void invoke("browser_close", { browserId: id }).catch(() => undefined);
      set((s) => {
        const { [id]: _, ...pages } = s.pages;
        return { pages };
      });
    },

    move: async (from, to) => {
      try {
        const state = await invoke<BrowserState>("browser_move", { from, to });
        set((s) => {
          const { [from]: _, ...pages } = s.pages;
          const { [from]: elements, ...picked } = s.picked;
          return { pages, picked: elements ? { ...picked, [to]: elements } : picked };
        });
        apply(state);
        return null;
      } catch (e) {
        return asError(e);
      }
    },

    forget: (id) =>
      set((s) => {
        const { [id]: _, ...pages } = s.pages;
        const { [id]: __, ...unavailable } = s.unavailable;
        const { [id]: ___, ...picked } = s.picked;
        return { pages, unavailable, picked };
      }),

    pick: async (id, on) => {
      try {
        apply(await invoke<BrowserState>("browser_picker", { browserId: id, on }));
        return null;
      } catch (e) {
        return asError(e);
      }
    },

    brief: async (id, note) => {
      try {
        return await invoke<string>("browser_picker_brief", { browserId: id, note });
      } catch (e) {
        return asError(e);
      }
    },

    clearData: async () => {
      try {
        await invoke("browser_clear_data");
        return null;
      } catch (e) {
        return asError(e);
      }
    },
  };
});

/** Pages whose address field takes focus when their view mounts (a tab
 * just made with "New browser"). */
const wantsFocus = new Set<string>();

export function focusAddressOnMount(id: string): void {
  wantsFocus.add(id);
}

export function takeAddressFocus(id: string): boolean {
  return wantsFocus.delete(id);
}

/** A page's name for its tab or pane: its title, else its host or file name. */
export function pageTitle(state: BrowserState): string | null {
  const title = state.title.trim();
  if (title) return title.slice(0, 120);
  try {
    const url = new URL(state.url);
    return url.protocol === "file:" ? (decodeURIComponent(url.pathname.split("/").pop() ?? "") || null) : url.host || null;
  } catch {
    return null;
  }
}

let following = false;
/** Resolves once `browser:state` is listened to. */
let listening: Promise<unknown> = Promise.resolve();

/** Follows `browser:state` for every page. */
export function followBrowser(): void {
  if (following) return;
  following = true;
  listening = Promise.all([
    listen<BrowserState>("browser:state", apply),
    listen<BrowserPicked>("browser:picker", (p) =>
      useBrowser.setState((s) => ({ picked: { ...s.picked, [p.browserId]: p.elements } })),
    ),
  ]).catch(() => undefined);
}

/** Tests: forget everything. */
export function resetBrowser(): void {
  useBrowser.setState({ pages: {}, picked: {}, unavailable: {} });
  wantsFocus.clear();
}
