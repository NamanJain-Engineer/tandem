import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BrowserState, Device, IpcError, PaneRecord, PickedElement, Rect, SidebarState, Thread } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { followBrowser, useBrowser } from "../../stores/browser";
import { followBrowserTabs, pageTitle, useSidebar } from "../../stores/sidebar";
import { useLayout } from "../../stores/layout";
import { useThreads } from "../../stores/threads";
import { terminalHandles, type TerminalHandle } from "../code/TerminalView";
import { useThreadDrop } from "../thread/drops";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { Sidebar } from "../shell/Sidebar";
import { BrowserView, externalFor } from "./BrowserView";
import { covered, notePage, overlaps } from "./place";

/** Where the page's place sits in the window (jsdom lays nothing out). */
const PLACE: Rect = { x: 900, y: 80, width: 470, height: 700 };
/** Where an open menu sits: over the place, or away from it. */
const AWAY: Rect = { x: 100, y: 60, width: 200, height: 120 };
let menuRect: Rect = AWAY;

const domRect = (r: Rect) => ({ ...r, top: r.y, left: r.x, right: r.x + r.width, bottom: r.y + r.height, toJSON: () => r }) as DOMRect;

function layOut() {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    if (this.dataset.testid === "browser-place") {
      // A hidden tab panel (display: none) has no box.
      const panel = this.closest<HTMLElement>("[role=tabpanel]");
      return domRect(panel && panel.style.display === "none" ? { x: 0, y: 0, width: 0, height: 0 } : PLACE);
    }
    if (this.getAttribute("role") === "menu") return domRect(menuRect);
    return domRect({ x: 0, y: 0, width: 0, height: 0 });
  });
}

/** States grow like the core's: each is newer than the last. */
let seq = 0;
const state = (patch: Partial<BrowserState> = {}): BrowserState => ({
  browserId: "b1",
  url: "",
  title: "",
  canBack: false,
  canForward: false,
  loading: false,
  device: "desktop",
  rotated: false,
  frame: PLACE,
  zoom: 1,
  blocked: null,
  picking: false,
  seq: ++seq,
  ...patch,
});

const ipcError = (code: string, message: string): IpcError => ({ code: code as IpcError["code"], message, stage: "browser", hint: null, retryable: false });

/** A core with one browser page per id. */
function setup(
  opts: { unavailable?: string; sidebar?: Partial<SidebarState>; open?: (a?: Record<string, unknown>) => BrowserState } = {},
): Call[] {
  const pages = new Map<string, BrowserState>();
  const publish = (s: BrowserState) => {
    pages.set(s.browserId, s);
    emitLocal("browser:state", s);
  };
  return fakeBackend({
    sidebar_get: () => ({ open: true, tabs: [], active: null, width: 470, fullscreen: false, pinned: false, ...opts.sidebar }),
    sidebar_save: (a) => a?.sidebar,
    status_list: () => [],
    browser_open: (a) => {
      if (opts.open) return opts.open(a);
      if (opts.unavailable) throw ipcError("unavailable", opts.unavailable);
      const id = String(a?.browserId);
      const url = (a?.url as string | null) ?? "";
      const s = pages.get(id) ?? state({ browserId: id, url, loading: url !== "" });
      pages.set(id, s);
      return s;
    },
    browser_navigate: (a) => {
      const id = String(a?.browserId);
      const input = String(a?.input);
      const s = pages.get(id) ?? state({ browserId: id });
      if (/^(javascript|tandem):/i.test(input)) {
        publish({ ...s, blocked: input, seq: ++seq });
        throw ipcError("invalid", `${input.split(":")[0]}: addresses can't be opened in the browser`);
      }
      publish({ ...s, blocked: null, seq: ++seq });
      return { url: input.includes("://") ? input : `http://${input}/` };
    },
    browser_set_bounds: () => null,
    browser_set_device: (a) => {
      const id = String(a?.browserId);
      const device = a?.device as Device;
      const s = { ...(pages.get(id) ?? state({ browserId: id })), device, rotated: a?.rotated === true, zoom: device === "desktop" ? 1 : 0.5, seq: ++seq };
      pages.set(id, s);
      return s;
    },
    browser_close: () => null,
    browser_clear_data: () => null,
    app_open_external: () => null,
    browser_picker: (a) => {
      const id = String(a?.browserId);
      const s = { ...(pages.get(id) ?? state({ browserId: id })), picking: a?.on === true, seq: ++seq };
      pages.set(id, s);
      return s;
    },
    browser_picker_brief: (a) => `# Picked elements\n\nPage: http://localhost:5173/react\n\nNote: ${String(a?.note)}\n\n## 1. \`#save\``,
  });
}

const pickedEl = (selector: string, component: string | null): PickedElement => ({
  tag: "button",
  selector,
  component: component ? { name: component, framework: "React" } : null,
  rect: { x: 0, y: 0, width: 10, height: 10 },
  styles: { display: "block", position: "static", font: "", color: "", background: "", margin: "0px", padding: "0px" },
  html: "<button></button>",
  truncated: false,
});

const view = (url: string | null = null) =>
  render(
    <UiProvider>
      <BrowserView id="b1" url={url} />
    </UiProvider>,
  );

const address = () => screen.getByTestId("browser-address") as HTMLInputElement;
const bounds = (calls: Call[]) => calls.filter((c) => c.cmd === "browser_set_bounds");
const lastShown = (calls: Call[]) => bounds(calls).at(-1)?.args?.visible;

async function go(text: string) {
  fireEvent.focus(address());
  fireEvent.change(address(), { target: { value: text } });
  fireEvent.keyDown(address(), { key: "Enter" });
}

describe("browser (F-CD-17)", () => {
  beforeEach(() => {
    resetStores();
    followBrowser();
    followBrowserTabs();
    layOut();
    menuRect = AWAY;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    notePage("b1", null);
  });

  it("F-CD-17.1 typing localhost:5173 opens a page and sends it there", async () => {
    const calls = setup();
    view();
    expect(screen.getByText("Open a page")).toBeTruthy();
    expect(address().placeholder).toBe("Search or enter address");
    await go("localhost:5173");
    await waitFor(() => expect(calls.some((c) => c.cmd === "browser_navigate")).toBe(true));
    const open = calls.find((c) => c.cmd === "browser_open");
    // A blank page, hidden until it has something to show.
    expect(open?.args).toMatchObject({ browserId: "b1", url: null, bounds: PLACE, visible: false });
    expect(calls.find((c) => c.cmd === "browser_navigate")?.args).toEqual({ browserId: "b1", input: "localhost:5173" });
    // What was asked for shows until the page commits.
    expect(address().value).toBe("localhost:5173");
    act(() => emitLocal("browser:state", state({ loading: true })));
    await waitFor(() => expect(lastShown(calls)).toBe(true));
    expect(screen.queryByText("Open a page")).toBeNull();
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/", loading: false, title: "Fixtures" })));
    expect(address().value).toBe("http://localhost:5173/");
  });

  it("F-CD-17 a reply that comes after the page's events never undoes them", async () => {
    // The page loads before browser_open's reply arrives (a fast local page).
    const initial = state({ url: "http://localhost:5173/", loading: true });
    setup({
      open: () => {
        emitLocal("browser:state", state({ url: "http://localhost:5173/", loading: false, title: "Fixtures" }));
        return initial;
      },
    });
    view("http://localhost:5173/");
    await waitFor(() => expect(useBrowser.getState().pages.b1?.title).toBe("Fixtures"));
    await new Promise((r) => setTimeout(r, 50));
    expect(useBrowser.getState().pages.b1).toMatchObject({ title: "Fixtures", loading: false });
    expect(address().value).toBe("http://localhost:5173/");
  });

  it("F-CD-17 the address bar shows only the committed URL, whatever the page says", async () => {
    setup();
    view();
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/", title: "https://bank.example/login" })));
    await waitFor(() => expect(address().value).toBe("http://localhost:5173/"));
    // A page loading somewhere else isn't there yet.
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/", loading: true })));
    expect(address().value).toBe("http://localhost:5173/");
    act(() => emitLocal("browser:state", state({ url: "https://example.com/", loading: false })));
    expect(address().value).toBe("https://example.com/");
    // Editing shows the draft; leaving the field shows the page again.
    fireEvent.focus(address());
    fireEvent.change(address(), { target: { value: "something else" } });
    expect(address().value).toBe("something else");
    fireEvent.blur(address());
    expect(address().value).toBe("https://example.com/");
  });

  it("F-CD-17.3 javascript: and tandem: addresses are blocked and say so", async () => {
    const calls = setup();
    view("http://localhost:5173/");
    await waitFor(() => expect(calls.some((c) => c.cmd === "browser_open")).toBe(true));
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/" })));
    for (const input of ["javascript:alert(1)", "tandem://settings"]) {
      await go(input);
      await waitFor(() => expect(screen.getByTestId("browser-blocked").textContent).toContain(`Blocked ${input}.`));
      // The bar keeps the page that's really there.
      await waitFor(() => expect(address().value).toBe("http://localhost:5173/"));
    }
    // A page's own attempt (a link, a redirect) shows the same way.
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/", blocked: "tandem://open" })));
    expect(screen.getByTestId("browser-blocked").textContent).toContain("Blocked tandem://open.");
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByTestId("browser-blocked")).toBeNull();
  });

  it("F-CD-17 the page hides while a dialog is open or a menu covers it", async () => {
    const calls = setup();
    view("http://localhost:5173/");
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/" })));
    await waitFor(() => expect(lastShown(calls)).toBe(true));
    // A menu away from the page leaves it alone…
    fireEvent.keyDown(screen.getByTestId("browser-device"), { key: "Enter" });
    await screen.findByRole("menu");
    await new Promise((r) => setTimeout(r, 300));
    expect(lastShown(calls)).toBe(true);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    // …one over it hides it until it closes.
    menuRect = { x: 1000, y: 100, width: 200, height: 120 };
    fireEvent.keyDown(screen.getByTestId("browser-more"), { key: "Enter" });
    await screen.findByRole("menu");
    await waitFor(() => expect(lastShown(calls)).toBe(false));
    // A dialog anywhere hides it.
    fireEvent.click(screen.getByRole("menuitem", { name: "Clear browsing data…" }));
    const dialog = await screen.findByRole("alertdialog");
    await new Promise((r) => setTimeout(r, 300));
    expect(lastShown(calls)).toBe(false);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(lastShown(calls)).toBe(true));
    expect(calls.some((c) => c.cmd === "browser_clear_data")).toBe(false);
  });

  it("F-CD-17 Clear browsing data asks first, then clears the profile", async () => {
    const calls = setup();
    view();
    fireEvent.keyDown(screen.getByTestId("browser-more"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Clear browsing data…" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Clear browsing data?")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear data" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "browser_clear_data")).toBe(true));
    expect(await screen.findByText("Browsing data cleared")).toBeTruthy();
  });

  it("F-CD-17 device preview: Phone and Tablet show their CSS size and the zoom that fit them", async () => {
    const calls = setup();
    view("http://localhost:5173/");
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/" })));
    await waitFor(() => expect((screen.getByTestId("browser-device") as HTMLButtonElement).disabled).toBe(false));
    fireEvent.keyDown(screen.getByTestId("browser-device"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Phone · 390 × 844" }));
    await waitFor(() => expect(screen.getByTestId("browser-device-label").textContent).toBe("390 × 844 · 50%"));
    expect(calls.find((c) => c.cmd === "browser_set_device")?.args).toEqual({ browserId: "b1", device: "phone", rotated: false });
    fireEvent.keyDown(screen.getByTestId("browser-device"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Rotate" }));
    await waitFor(() => expect(screen.getByTestId("browser-device-label").textContent).toBe("844 × 390 · 50%"));
    fireEvent.keyDown(screen.getByTestId("browser-device"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Tablet · 820 × 1180" }));
    await waitFor(() => expect(screen.getByTestId("browser-device-label").textContent).toBe("1180 × 820 · 50%"));
    fireEvent.keyDown(screen.getByTestId("browser-device"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Desktop" }));
    await waitFor(() => expect(screen.queryByTestId("browser-device-label")).toBeNull());
    expect(calls.filter((c) => c.cmd === "browser_set_device").at(-1)?.args).toEqual({ browserId: "b1", device: "desktop", rotated: false });
  });

  it("F-CD-17 where pages can't be embedded the tab says why and offers the system browser", async () => {
    const calls = setup({ unavailable: "Web pages can't be shown inside the app on this system." });
    view("http://localhost:5173/");
    expect(await screen.findByText("Pages can't open inside Vibezeo here")).toBeTruthy();
    expect(screen.getByText("Web pages can't be shown inside the app on this system.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open in your browser" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "app_open_external")?.args).toEqual({ url: "http://localhost:5173/" }));
    // It doesn't keep asking.
    await new Promise((r) => setTimeout(r, 600));
    expect(calls.filter((c) => c.cmd === "browser_open")).toHaveLength(1);
  });

  it("F-CD-18 the picker lists what's picked; the brief copies, or pastes into an agent pane or the thread without sending", async () => {
    const calls = setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const paste = vi.fn();
    terminalHandles.set("pn_ada", { paste } as unknown as TerminalHandle);
    const agent = { id: "pn_ada", kind: "agent", engineId: "claude", humanName: "Ada", createdAt: 1 } as PaneRecord;
    useLayout.setState({ tabs: { tb_1: { tree: null, active: null, maximized: null, panes: { pn_ada: agent }, keepEmpty: false } } });
    useThreads.setState({ list: [{ id: "th_1", title: "Fix login" } as Thread], selectedId: "th_1" });
    view("http://localhost:5173/react");
    act(() => emitLocal("browser:state", state({ url: "http://localhost:5173/react" })));
    fireEvent.click(await screen.findByTestId("browser-pick"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "browser_picker")?.args).toEqual({ browserId: "b1", on: true }));
    expect(await screen.findByText(/Shift\+click adds one/)).toBeTruthy();
    expect(screen.getByText("Nothing picked yet.")).toBeTruthy();
    act(() => emitLocal("browser:picker", { browserId: "b1", elements: [pickedEl("#save", "SaveButton"), pickedEl("main > ul > li:nth-of-type(2)", null)] }));
    const rows = screen.getAllByTestId("browser-picked");
    expect(rows.map((r) => r.textContent)).toEqual(["1#saveSaveButton (React)", "2main > ul > li:nth-of-type(2)"]);
    fireEvent.change(screen.getByRole("textbox", { name: "Note" }), { target: { value: "bigger" } });
    fireEvent.click(screen.getByTestId("browser-brief-copy"));
    await waitFor(() => expect(writeText).toHaveBeenCalled());
    expect(calls.find((c) => c.cmd === "browser_picker_brief")?.args).toEqual({ browserId: "b1", note: "bigger" });
    expect(writeText.mock.calls[0]?.[0]).toContain("Note: bigger");
    // Send to: the agent pane gets a paste with nothing after the brief.
    fireEvent.keyDown(screen.getByTestId("browser-brief-send"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Ada · claude" }));
    await waitFor(() => expect(paste).toHaveBeenCalledTimes(1));
    const pasted = String(paste.mock.calls[0]?.[0]);
    expect(pasted.startsWith("# Picked elements")).toBe(true);
    expect(pasted.endsWith("`#save`")).toBe(true);
    // …or the open thread's composer.
    fireEvent.keyDown(screen.getByTestId("browser-brief-send"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Thread: Fix login" }));
    await waitFor(() => expect(useThreadDrop.getState().texts.th_1?.[0]).toContain("Note: bigger"));
    terminalHandles.delete("pn_ada");
  });

  it("F-CD-17 New browser opens a tab with its address focused; the page names the pill; closing it closes the page", async () => {
    const calls = setup();
    render(
      <UiProvider>
        <Sidebar />
      </UiProvider>,
    );
    await waitFor(() => expect(useSidebar.getState().loaded).toBe(true));
    fireEvent.keyDown(screen.getByTestId("sidebar-plus"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "New browser" }));
    const pill = await screen.findByTestId("sidebar-pill-browser");
    expect(pill.textContent).toBe("Browser");
    await waitFor(() => expect(document.activeElement).toBe(address()));
    const id = String(pill.dataset.tabId);
    act(() => emitLocal("browser:state", state({ browserId: id, url: "http://localhost:5173/", title: "Fixtures" })));
    await waitFor(() => expect(pill.textContent).toBe("Fixtures"));
    expect(useSidebar.getState().tabs.find((x) => x.id === id)?.url).toBe("http://localhost:5173/");
    act(() => useSidebar.getState().close(id));
    await waitFor(() => expect(calls.find((c) => c.cmd === "browser_close")?.args).toEqual({ browserId: id }));
    expect(useBrowser.getState().pages[id]).toBeUndefined();
  });

  it("F-CD-17 a restored tab reopens its page the first time it shows", async () => {
    const calls = setup({
      sidebar: {
        tabs: [
          { id: "dashboard", kind: "dashboard", title: null, url: null },
          { id: "browser-1", kind: "browser", title: "Fixtures", url: "http://localhost:5173/" },
        ],
        active: "dashboard",
      },
    });
    render(
      <UiProvider>
        <Sidebar />
      </UiProvider>,
    );
    await waitFor(() => expect(screen.getByTestId("sidebar-pill-browser").textContent).toBe("Fixtures"));
    await new Promise((r) => setTimeout(r, 300));
    expect(calls.some((c) => c.cmd === "browser_open")).toBe(false);
    fireEvent.click(screen.getByTestId("sidebar-pill-browser"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "browser_open")?.args).toMatchObject({ browserId: "browser-1", url: "http://localhost:5173/", visible: true }));
  });
});

describe("browser placement (F-CD-17, D-206)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    document.body.innerHTML = "";
    notePage("p", null);
  });

  it("menus cover the pages they overlap; dialogs cover every page", () => {
    const place: Rect = { x: 100, y: 100, width: 300, height: 300 };
    expect(overlaps(place, { x: 390, y: 390, width: 50, height: 50 })).toBe(true);
    expect(overlaps(place, { x: 400, y: 100, width: 50, height: 50 })).toBe(false);
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    document.body.append(menu);
    const at = vi.spyOn(menu, "getBoundingClientRect").mockReturnValue(domRect({ x: 10, y: 10, width: 50, height: 50 }));
    expect(covered(place)).toBe(false);
    at.mockReturnValue(domRect({ x: 350, y: 120, width: 100, height: 100 }));
    expect(covered(place)).toBe(true);
    menu.remove();
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.append(dialog);
    expect(covered(place)).toBe(true);
  });

  it("toasts move bottom-left while a page covers their corner", () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
    notePage("p", { x: 0, y: 40, width: 800, height: 840 });
    expect(document.documentElement.dataset.toasts).toBeUndefined();
    notePage("p", { x: 970, y: 40, width: 470, height: 840 });
    expect(document.documentElement.dataset.toasts).toBe("left");
    notePage("p", null);
    expect(document.documentElement.dataset.toasts).toBeUndefined();
  });

  it("the system browser gets addresses and bare hosts, never searches", () => {
    expect(externalFor("https://vibezeo.com/demo")).toBe("https://vibezeo.com/demo");
    expect(externalFor("localhost:5173")).toBe("http://localhost:5173");
    expect(externalFor("example.com/docs")).toBe("http://example.com/docs");
    expect(externalFor("rust traits")).toBeNull();
    expect(externalFor("javascript:alert(1)")).toBeNull();
  });

  it("a pill names a page by its title, else its host or file", () => {
    const base = { browserId: "b", canBack: false, canForward: false, loading: false, device: "desktop" as const, rotated: false, frame: { x: 0, y: 0, width: 0, height: 0 }, zoom: 1, blocked: null, picking: false, seq: 1 };
    expect(pageTitle({ ...base, url: "http://localhost:5173/x", title: "  Fixtures " })).toBe("Fixtures");
    expect(pageTitle({ ...base, url: "http://localhost:5173/x", title: "" })).toBe("localhost:5173");
    expect(pageTitle({ ...base, url: "file:///work/site/index%20page.html", title: "" })).toBe("index page.html");
    expect(pageTitle({ ...base, url: "", title: "" })).toBeNull();
  });
});
