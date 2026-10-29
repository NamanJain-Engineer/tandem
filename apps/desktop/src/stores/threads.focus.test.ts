import { waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The native window's focus (Tauri): only this path runs in the app, so it's
// pinned here with a fresh store built as if inside Tauri.
type Handler = (e: { payload: boolean }) => void;

describe("F-TH-01 AC2: under Tauri, the native window's focus decides what's read", () => {
  let order: string[];
  let handler: Handler | null;
  let answer: (focused: boolean) => void;
  let fail: { listen?: boolean; query?: boolean; load?: boolean };

  const load = async () => {
    vi.resetModules();
    vi.doMock("@tauri-apps/api/window", () => {
      if (fail.load) throw new Error("no window module");
      return {
        getCurrentWindow: () => ({
          onFocusChanged: async (h: Handler) => {
            order.push("listen");
            if (fail.listen) throw new Error("listen refused");
            handler = h;
            return () => undefined;
          },
          isFocused: () => {
            order.push("query");
            if (fail.query) return Promise.reject(new Error("query refused"));
            return new Promise<boolean>((r) => {
              answer = r;
            });
          },
        }),
      };
    });
    const { useThreads } = await import("./threads");
    return useThreads;
  };

  beforeEach(() => {
    order = [];
    handler = null;
    answer = () => undefined;
    fail = {};
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    vi.doUnmock("@tauri-apps/api/window");
    vi.restoreAllMocks();
    vi.resetModules();
  });

  it("starts unfocused until the window says otherwise", async () => {
    const store = await load();
    expect(store.getState().windowFocused).toBe(false);
    await waitFor(() => expect(order).toEqual(["listen", "query"]));
    expect(store.getState().windowFocused).toBe(false);
    answer(true);
    await waitFor(() => expect(store.getState().windowFocused).toBe(true));
  });

  it("listens before it asks, so a change during the first query isn't missed", async () => {
    await load();
    await waitFor(() => expect(order).toEqual(["listen", "query"]));
  });

  it("an event that arrived during the query wins over the query's older answer", async () => {
    const store = await load();
    await waitFor(() => expect(order).toEqual(["listen", "query"]));
    handler?.({ payload: true });
    expect(store.getState().windowFocused).toBe(true);
    answer(false);
    await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    expect(store.getState().windowFocused).toBe(true);
    handler?.({ payload: false });
    expect(store.getState().windowFocused).toBe(false);
  });

  it("D-3117: wiring that fails stays unfocused and says so in the log, with nothing of the threads", async () => {
    for (const which of ["load", "listen", "query"] as const) {
      order = [];
      fail = { [which]: true };
      const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
      const store = await load();
      await waitFor(() => expect(warn).toHaveBeenCalledTimes(1));
      expect(store.getState().windowFocused).toBe(false);
      const logged = warn.mock.calls[0]?.map(String).join(" ") ?? "";
      expect(logged).toContain("window focus");
      expect(logged).not.toMatch(/refused|no window module/);
      warn.mockRestore();
    }
  });
});
