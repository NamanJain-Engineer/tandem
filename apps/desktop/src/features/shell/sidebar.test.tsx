import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SidebarState } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useSidebar } from "../../stores/sidebar";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { Sidebar } from "./Sidebar";

function setup(saved: Partial<SidebarState> = {}): Call[] {
  const stored: SidebarState = {
    open: true,
    tabs: [
      { id: "dashboard", kind: "dashboard", title: null, url: null },
      { id: "files", kind: "files", title: null, url: null },
    ],
    active: "dashboard",
    width: 470,
    fullscreen: false,
    pinned: false,
    ...saved,
  };
  return fakeBackend({
    sidebar_get: () => stored,
    sidebar_save: (a) => a?.sidebar,
    status_list: () => [],
  });
}

const pills = () => screen.getAllByRole("tab").map((el) => el.dataset.tabId);

describe("right sidebar (F-SH-10)", () => {
  beforeEach(() => {
    resetStores();
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
  });
  afterEach(() => vi.useRealTimers());

  it("F-SH-10.4 opening Dashboard twice leaves one Dashboard tab and focuses it", async () => {
    setup();
    render(<UiProvider><Sidebar /></UiProvider>);
    await waitFor(() => expect(pills()).toEqual(["dashboard", "files"]));
    act(() => useSidebar.getState().activate("files"));
    act(() => useSidebar.getState().openTab("dashboard"));
    act(() => useSidebar.getState().openTab("dashboard"));
    expect(pills()).toEqual(["dashboard", "files"]);
    expect(screen.getByRole("tab", { selected: true }).dataset.tabId).toBe("dashboard");
    // Browser tabs aren't singletons.
    act(() => useSidebar.getState().openTab("browser"));
    act(() => useSidebar.getState().openTab("browser"));
    expect(pills()).toHaveLength(4);
  });

  it("F-SH-10.3 order, active tab and width are saved (debounced) as one state", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const calls = setup();
    render(<UiProvider><Sidebar /></UiProvider>);
    await waitFor(() => expect(pills()).toEqual(["dashboard", "files"]));
    act(() => useSidebar.getState().reorder(["files", "dashboard"]));
    fireEvent.click(screen.getAllByRole("tab")[1] as HTMLElement);
    act(() => useSidebar.getState().setWidth(2_000));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    const saves = calls.filter((c) => c.cmd === "sidebar_save");
    expect(saves).toHaveLength(1);
    expect(saves[0]?.args?.sidebar).toMatchObject({
      tabs: [{ id: "files" }, { id: "dashboard" }],
      active: "dashboard",
      width: 900,
    });
  });

  it("a pill closes with × or Delete; Alt+Shift+Arrow moves it", async () => {
    setup();
    render(<UiProvider><Sidebar /></UiProvider>);
    await waitFor(() => expect(pills()).toEqual(["dashboard", "files"]));
    fireEvent.keyDown(screen.getByRole("tab", { name: /Files/ }), { key: "ArrowLeft", altKey: true, shiftKey: true });
    expect(pills()).toEqual(["files", "dashboard"]);
    fireEvent.click(screen.getByRole("button", { name: "Close Files" }));
    expect(pills()).toEqual(["dashboard"]);
    fireEvent.keyDown(screen.getByRole("tab", { name: /Dashboard/ }), { key: "Delete" });
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
    expect(useSidebar.getState().open).toBe(false);
  });

  it("F-SH-10.2 full-screen overlays the content and keeps the docked width in the flow; Esc leaves it", async () => {
    setup();
    const { container } = render(<UiProvider><Sidebar /></UiProvider>);
    await waitFor(() => expect(pills()).toHaveLength(2));
    const aside = screen.getByTestId("sidebar");
    expect(aside.dataset.placement).toBe("docked");
    fireEvent.click(within(aside).getByTestId("sidebar-fullscreen"));
    expect(aside.dataset.placement).toBe("fullscreen");
    // The spacer the main area keeps instead of growing into the gap.
    const spacer = container.querySelector('[aria-hidden="true"][style*="width: 470px"]');
    expect(spacer).not.toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(aside.dataset.placement).toBe("docked");
  });

  it("below 1,100 px it floats, and an outside click closes it unless pinned", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1_000 });
    setup();
    render(
      <UiProvider>
        <button type="button">outside</button>
        <Sidebar />
      </UiProvider>,
    );
    await waitFor(() => expect(pills()).toHaveLength(2));
    expect(screen.getByTestId("sidebar").dataset.placement).toBe("floating");
    fireEvent.click(screen.getByRole("button", { name: "Keep open" }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "outside" }));
    expect(useSidebar.getState().open).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Close on outside click" }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "outside" }));
    expect(useSidebar.getState().open).toBe(false);
  });
});
