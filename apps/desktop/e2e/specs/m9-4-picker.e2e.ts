import { $, browser } from "@wdio/globals";
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  fakeLog,
  freshOffline,
  home,
  invoke,
  mod,
  paneIds,
  pickTile,
  split,
  waitForPaneText,
} from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M9-4 Picker (F-CD-18): a browser pane on the React fixture page
// (scripts/serve-fixtures.ts `/react`) picks two elements, a click then a
// Shift+click, and copies the brief: it names both, each by a selector that
// matches exactly that element in the page, with the React component that
// rendered it. Send to… pastes the same brief into an agent pane, bracketed
// and with nothing after it, so nothing is submitted. The clicks are made in
// the page by the test harness (`test_browser_eval`), which also checks the
// selectors there.

const PASTE_START = "\u001b[200~";
const PASTE_END = "\u001b[201~";
const hex = (s: string) => Buffer.from(s, "utf8").toString("hex");

async function until(check: () => Promise<boolean>, timeoutMsg: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(check, { timeout, timeoutMsg });
}

/** Runs `script` in browser page `id` and returns its value. */
async function inPage<T>(id: string, script: string): Promise<T | null> {
  const json = await invoke<string>("test_browser_eval", { browserId: id, script });
  return json === "" ? null : (JSON.parse(json) as T);
}

const inPane = (pane: string, testid: string) => `[data-testid="pane"][data-pane-id="${pane}"] [data-testid="${testid}"]`;

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

/** Each picked element's test, as the page sees it: the second item's Save
 * button (clicked) and the third item's text (Shift+clicked). */
const PICKS = [`document.querySelectorAll("li.todo")[1].querySelector("button")`, `document.querySelectorAll("li.todo")[2].querySelector("span")`];

/** Clicks `target` in the page the way a user's click arrives; whether the
 * page's own handlers may still act on it (false: the picker took it). */
const click = (target: string, shift: boolean) =>
  `${target}.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window, shiftKey: ${shift} }))`;

/** The agent's input since `from` log lines, as one hex string. */
const typed = (from: number) =>
  fakeLog()
    .slice(from)
    .filter((l) => l.startsWith("raw: "))
    .map((l) => l.slice(5))
    .join("");

describe("E2E-M9-4 picker", () => {
  const folder = `/tmp/tandem-e2e-m9-4-${Date.now()}`;
  let agent = "";
  let pane = "";
  let page = "";
  let brief = "";

  before(async function () {
    this.timeout(120_000);
    mkdirSync(folder, { recursive: true });
    await freshOffline();
    // The agent is the fake Codex, which runs no hooks.
    fakeEngine("codex", join(home(), "fakebin"));
    await checkAgain();
    await addFolder(folder);
    await until(async () => (await paneIds()).length === 1, "the folder's shell didn't open", 20_000);
    agent = await split("right", "codex");
    await waitForPaneText(agent, "Fake Codex (test engine)");
    pane = await browserSplit();
    const field = await $(inPane(pane, "browser-address"));
    await field.click();
    await field.setValue("localhost:5173/react");
    await browser.keys("Enter");
    // The fixture server bundles the page on its first visit.
    await until(
      async () =>
        (await browser.execute((p: string) => document.querySelector(`[data-testid="pane"][data-pane-id="${p}"] [data-testid="pane-title"]`)?.textContent ?? "", pane)) ===
        "React fixture",
      "the React page didn't load",
      40_000,
    );
    page = (await $(inPane(pane, "browser-view")).getAttribute("data-browser-id")) ?? "";
    await until(async () => (await inPage<number>(page, `document.querySelectorAll("li.todo").length`)) === 3, "React didn't render the list");
  });

  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    await checkAgain();
  });

  it("F-CD-18.1 two picked elements and the brief: two selectors, each matching exactly that one element", async () => {
    await $(inPane(pane, "browser-pick")).click();
    await until(async () => (await $(inPane(pane, "browser-pick")).getAttribute("aria-pressed")) === "true", "the picker didn't turn on");
    await until(async () => (await inPage<boolean>(page, `document.querySelector("tandem-picker-layer") !== null`)) === true, "the picker isn't in the page");

    // The page's own handlers never get a picking click.
    expect(await inPage<boolean>(page, click(PICKS[0] ?? "", false))).toBe(false);
    await until(async () => (await $$picked()).length === 1, "the click didn't pick");
    expect(await inPage<boolean>(page, click(PICKS[1] ?? "", true))).toBe(false);
    await until(async () => (await $$picked()).length === 2, "Shift+click didn't add the second element");

    await $(inPane(pane, "browser-pick-note")).setValue("Make the second Save button the primary one");
    await $(inPane(pane, "browser-brief-copy")).click();
    await until(async () => {
      brief = String(await invoke("plugin:clipboard-manager|read_text").catch(() => ""));
      return brief.startsWith("# Picked elements");
    }, "the brief wasn't copied");

    expect(brief).toContain("\nPage: http://localhost:5173/react\n");
    expect(brief).toContain("\nNote: Make the second Save button the primary one\n");
    const selectors = Array.from(brief.matchAll(/^## \d+\. `(.+)`$/gm), (m) => m[1] ?? "");
    expect(selectors).toHaveLength(2);
    const found = await inPage<Array<{ count: number; same: boolean }>>(
      page,
      `(() => { const want = [${PICKS.join(", ")}]; return ${JSON.stringify(selectors)}.map((s, i) => { const all = document.querySelectorAll(s); return { count: all.length, same: all[0] === want[i] }; }); })()`,
    );
    expect(found).toEqual([
      { count: 1, same: true },
      { count: 1, same: true },
    ]);
    // The components that rendered them, from React's own records.
    expect(brief).toMatch(/^## 1\. `.+`\n- Component: `SaveButton` \(React\)\n- Size and position: \d+ × \d+ at \(\d+, \d+\)\n- Styles: display: inline-block; position: static; /m);
    expect(brief).toMatch(/^## 2\. `.+`\n- Component: `TodoItem` \(React\)\n/m);
    expect(brief).toContain('```html\n<button type="button" class="save">Save</button>\n```');
    expect(brief).toContain("```html\n<span>Ship it</span>\n```");
    expect(brief.endsWith("```")).toBe(true);
  });

  it("the picker panel has no serious a11y violations in either theme", async () => {
    // "Brief copied" goes first: a toast fading out mid-check reads as low
    // contrast.
    await until(
      async () => browser.execute(() => document.querySelector("[data-toast-region] [role=status], [data-toast-region] [role=alert]") === null),
      "the toast didn't go",
      10_000,
    );
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("F-CD-18.2 Send to… pastes the brief into an agent pane with no newline after it", async () => {
    const from = fakeLog().length;
    await $(inPane(pane, "browser-brief-send")).click();
    await until(
      async () =>
        browser.execute(() => {
          const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((el) => el.textContent?.trim().endsWith("Codex"));
          item?.click();
          return item !== undefined;
        }),
      "the agent pane isn't a target",
      5_000,
    );
    const pasted = hex(`${PASTE_START}${brief}${PASTE_END}`);
    await until(async () => typed(from).length >= pasted.length, `the agent got ${typed(from).length / 2} of ${pasted.length / 2} bytes`);
    // Nothing follows the paste: no Enter, so nothing is submitted.
    await browser.pause(1_000);
    expect(typed(from)).toBe(pasted);
  });

  it("F-CD-18 Esc in the page stops picking and keeps what's picked", async () => {
    await inPage(page, `document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }))`);
    await until(async () => (await $(inPane(pane, "browser-pick")).getAttribute("aria-pressed")) === "false", "Esc didn't stop the picker");
    expect(await inPage<boolean>(page, `document.querySelector("tandem-picker-layer") === null`)).toBe(true);
    expect(await $$picked()).toHaveLength(2);
    // Clicks are the page's own again.
    expect(await inPage<boolean>(page, click(PICKS[0] ?? "", false))).toBe(true);
  });

  /** The picked elements the panel lists. */
  async function $$picked(): Promise<string[]> {
    return browser.execute(
      (p: string) => Array.from(document.querySelectorAll(`[data-testid="pane"][data-pane-id="${p}"] [data-testid="browser-picked"] code`), (el) => el.textContent ?? ""),
      pane,
    );
  }
});
