import { $, browser } from "@wdio/globals";
import { mkdirSync } from "node:fs";
import type { SidebarState, TabLayout } from "@tandem/protocol";
import { addFolder, freshOffline, invoke, mod, paneIds, pickTile, relaunch } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { openFromPlus } from "../support/files";

// E2E-M9-3 Browser (F-CD-17): `localhost:5173` loads http://localhost:5173;
// a page probing for the app finds no way in (`window.__TAURI__` and every
// other bridge come up empty); `javascript:` and `tandem:` addresses are
// blocked, typed or page-made. Then browser panes: one moves to the sidebar
// and back without reloading, comes back on its page after a relaunch, and
// a browser seat opens with a lineup (F-CD-14). The pages come from
// scripts/serve-fixtures.ts, which run.ts starts on :5173 and which records
// what the browser asked for.

const FIXTURES = "http://127.0.0.1:5173";

async function fixtures<T>(path: string): Promise<T> {
  return (await (await fetch(`${FIXTURES}${path}`)).json()) as T;
}

const read = (testid: string, what: "value" | "text" | "visible") =>
  browser.execute(
    (id: string, w: string) => {
      const el = document.querySelector<HTMLInputElement>(`[data-testid="${id}"]`);
      if (!el) return "";
      return w === "value" ? el.value : w === "text" ? (el.textContent ?? "") : (el.getAttribute("data-visible") ?? "");
    },
    testid,
    what,
  );

const address = () => read("browser-address", "value");
const pill = () => read("sidebar-pill-browser", "text");
const blocked = () => read("browser-blocked", "text");

async function until(check: () => Promise<boolean>, timeoutMsg: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(check, { timeout, timeoutMsg });
}

/** Types into the address field and presses Enter. */
async function go(text: string): Promise<void> {
  const field = await $('[data-testid="browser-address"]');
  await field.click();
  await field.setValue(text);
  await browser.keys("Enter");
}

describe("E2E-M9-3 browser", () => {
  before(async function () {
    this.timeout(60_000);
    await fetch(`${FIXTURES}/__reset`, { method: "POST" });
    await freshOffline();
    await openFromPlus("New browser");
    await $('[data-testid="browser-address"]').waitForDisplayed({ timeout: 10_000 });
  });

  it("F-CD-17.1 typing localhost:5173 loads http://localhost:5173", async () => {
    // A new tab's address field has the focus.
    await until(
      async () => (await browser.execute(() => document.activeElement?.getAttribute("data-testid"))) === "browser-address",
      "the address field isn't focused",
      5_000,
    );
    await go("localhost:5173");
    await until(async () => (await address()) === "http://localhost:5173/", `the address bar shows ${await address()}`);
    await until(async () => (await pill()) === "Fixtures", "the tab isn't named after the page");
    expect(await fixtures<string[]>("/__requests")).toContain("/");
    expect(await read("browser-place", "visible")).toBe("true");
  });

  it("F-CD-17.2 a page probing window.__TAURI__ gets undefined", async () => {
    await go("localhost:5173/probe");
    await until(async () => (await pill()).startsWith("Probe:"), "the probe didn't finish");
    expect(await address()).toBe("http://localhost:5173/probe");
    const reports = await fixtures<Array<Record<string, string>>>("/__reports");
    expect(reports).toEqual([
      {
        tauri: "undefined",
        internals: "undefined",
        handler: "undefined",
        post: "failed",
        "ipc://localhost/app_boot": "failed",
        "tauri://localhost/": "failed",
      },
    ]);
    expect(await pill()).toBe("Probe: no app bridge");
  });

  it("F-CD-17.3 javascript: and tandem: addresses are blocked, typed or page-made", async () => {
    for (const input of ["javascript:document.title='pwned'", "tandem://settings"]) {
      await go(input);
      await until(async () => (await blocked()).includes(`Blocked ${input}.`), `${input} wasn't blocked`);
      // The bar keeps the page that's really there, and the page is untouched.
      expect(await address()).toBe("http://localhost:5173/probe");
    }
    expect(await pill()).toBe("Probe: no app bridge");
    // A page sending itself to tandem: is refused the same way.
    await go("localhost:5173/to-tandem");
    await until(async () => (await blocked()).includes("Blocked tandem://open?from=page."), "the page's tandem: link wasn't blocked");
    expect(await fixtures<string[]>("/__requests")).toContain("/to-tandem");
    expect(await address()).toBe("http://localhost:5173/to-tandem");
  });

  it("the browser tab has no serious a11y violations in either theme", async () => {
    await expectNoSeriousA11yViolationsInBothThemes();
  });
});

/** How often the fixture server served `path`. */
async function served(path: string): Promise<number> {
  return (await fixtures<string[]>("/__requests")).filter((p) => p === path).length;
}

/** A pane's title, by id. */
const paneTitle = (id: string) =>
  browser.execute(
    (pane: string) => document.querySelector(`[data-testid="pane"][data-pane-id="${pane}"] [data-testid="pane-title"]`)?.textContent ?? "",
    id,
  );

/** The visible browser panes' ids. */
const browserPanes = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="pane"][data-kind="browser"]'))
      .filter((el) => el.offsetParent !== null)
      .map((el) => el.dataset.paneId ?? ""),
  );

/** Picks `label` in a pane's ⋯ menu. */
async function paneMenu(id: string, label: string): Promise<void> {
  await $(`[data-testid="pane"][data-pane-id="${id}"] button[aria-label^="Actions for"]`).click();
  await until(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((el) => el.textContent?.trim() === l);
        item?.click();
        return item !== undefined;
      }, label),
    `no menu item ${label}`,
    5_000,
  );
}

/** Splits the focused pane right with a Browser pane; returns its id. */
async function browserSplit(): Promise<string> {
  const before = await paneIds();
  await browser.execute((mac: boolean) => {
    (document.activeElement as HTMLElement | null)?.blur();
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyD", key: "d", ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true }));
  }, mod() === "Meta");
  await pickTile("browser");
  let added = "";
  await until(async () => {
    added = (await paneIds()).find((id) => !before.includes(id)) ?? "";
    return added !== "";
  }, "no browser pane was added");
  return added;
}

describe("E2E-M9-3 browser panes", () => {
  const folder = `/tmp/tandem-e2e-m9-3-${Date.now()}`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(folder, { recursive: true });
    // The first tests' sidebar tab goes, so the sidebar's browser is the moved page.
    await browser.execute(() => {
      for (const pill of Array.from(document.querySelectorAll<HTMLElement>('[data-testid="sidebar-pill-browser"]')))
        pill.parentElement?.querySelector<HTMLElement>('[data-testid="sidebar-pill-close"]')?.click();
    });
    await until(
      async () => browser.execute(() => document.querySelector('[data-testid="sidebar-pill-browser"]') === null),
      "a browser tab is still open",
      5_000,
    );
    await addFolder(folder);
    await until(async () => (await paneIds()).length === 1, "the folder's shell didn't open", 20_000);
  });

  it("F-CD-17 a pane opens a page, moves to the sidebar with it and back, never reloading it", async () => {
    const pane = await browserSplit();
    expect(await paneTitle(pane)).toBe("Browser");
    const field = await $(`[data-testid="pane"][data-pane-id="${pane}"] [data-testid="browser-address"]`);
    await field.click();
    await field.setValue("localhost:5173");
    await browser.keys("Enter");
    await until(async () => (await paneTitle(pane)) === "Fixtures", "the pane didn't load the page");
    const loads = await served("/");

    await paneMenu(pane, "Open in sidebar");
    await until(async () => !(await paneIds()).includes(pane), "the pane didn't close");
    await until(async () => (await pill()) === "Fixtures", "the sidebar tab isn't the page");
    expect(await address()).toBe("http://localhost:5173/");
    expect(await read("browser-place", "visible")).toBe("true");

    await $('[data-testid="sidebar-panel-browser"] [data-testid="browser-more"]').click();
    await until(
      async () =>
        browser.execute(() => {
          const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((el) => el.textContent?.trim() === "Move to a pane");
          item?.click();
          return item !== undefined;
        }),
      "no Move to a pane",
      5_000,
    );
    await until(async () => (await browserPanes()).length === 1, "the page didn't come back to a pane");
    const [back] = await browserPanes();
    await until(async () => (await paneTitle(back ?? "")) === "Fixtures", "the pane isn't on the page");
    expect(await browser.execute(() => document.querySelector('[data-testid="sidebar-pill-browser"]') === null)).toBe(true);
    // The same page all along: the server served it once.
    expect(await served("/")).toBe(loads);
  });

  it("F-CD-17 a browser pane is back on its page after a relaunch", async () => {
    const [pane] = await browserPanes();
    // The sidebar and the layout save shortly after a change: let both land.
    await until(
      async () => !(await invoke<SidebarState>("sidebar_get")).tabs.some((tab) => tab.kind === "browser"),
      "the sidebar still lists the moved tab",
      5_000,
    );
    const workspaceId = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === folder)?.id;
    await until(
      async () => JSON.stringify(await invoke<TabLayout[]>("layout_get", { workspaceId })).includes(pane ?? "?"),
      "the layout doesn't hold the browser pane yet",
      5_000,
    );
    const loads = await served("/");
    await relaunch();
    await until(async () => (await browserPanes()).includes(pane ?? "?"), "the browser pane wasn't restored", 30_000);
    await until(async () => (await paneTitle(pane ?? "")) === "Fixtures", "the restored pane didn't open its page", 20_000);
    expect(await served("/")).toBe(loads + 1);
  });

  it("F-CD-14 a browser seat opens its page tiled with the lineup", async () => {
    await browser.keys([mod(), "Shift", "l"]);
    await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
    await $('[data-testid="launch-add-seat"]').click();
    await until(
      async () =>
        browser.execute(() => {
          const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((el) => el.textContent?.trim() === "Browser URL");
          item?.click();
          return item !== undefined;
        }),
      "no Browser URL seat",
      5_000,
    );
    const url = await $('[data-testid="launch-seat"][data-kind="browser"] input');
    await url.waitForExist({ timeout: 5_000 });
    await url.setValue("localhost:5173/probe");
    await until(async () => String(await $('[data-testid="launch-go"]').getProperty("textContent")).includes("1 browser"), "the button doesn't count the browser");
    await $('[data-testid="launch-go"]').click();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 20_000 });
    await until(async () => (await browserPanes()).length === 1 && (await paneIds()).length === 2, "the lineup didn't open beside its browser");
    const [pane] = await browserPanes();
    await until(async () => (await paneTitle(pane ?? "")) === "Probe: no app bridge", "the seat's page didn't load");
  });
});
