import { $, browser } from "@wdio/globals";
import { freshOffline } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { openFromPlus } from "../support/files";

// E2E-M9-3 Browser (F-CD-17): `localhost:5173` loads http://localhost:5173;
// a page probing for the app finds no way in (`window.__TAURI__` and every
// other bridge come up empty); `javascript:` and `tandem:` addresses are
// blocked, typed or page-made. The pages come from scripts/serve-fixtures.ts,
// which run.ts starts on :5173 and which records what the browser asked for.

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
