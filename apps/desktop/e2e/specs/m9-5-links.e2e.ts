import { browser } from "@wdio/globals";
import { Key } from "webdriverio";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, paneIds, paneText, sendToPane, waitForPaneText } from "../support/app";

// E2E-M9-5 Links (F-CD-09): a shell prints `src/main.rs:42:7` and Ctrl+Click
// on it (real WebDriver input) opens src/main.rs in the Files tab with the
// cursor on line 42; `/etc/hosts` opens read-only; after `cd sub` the same
// relative path opens sub's file, because links follow the shell's OSC 7;
// a URL opens in the app's browser. Each link is printed from pieces, so the
// typed command line holds none.

async function until(check: () => Promise<boolean>, timeoutMsg: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(check, { timeout, timeoutMsg });
}

/** Where `needle` shows in pane `pane` (client pixels), once it does. */
async function pointOf(pane: string, needle: string): Promise<{ x: number; y: number }> {
  let point: { x: number; y: number } | null = null;
  await until(async () => {
    point = (await browser.execute(
      (id: string, text: string) =>
        (window as unknown as { __tandemTest?: { panePoint: (id: string, text: string) => { x: number; y: number } | null } }).__tandemTest?.panePoint(id, text) ?? null,
      pane,
      needle,
    )) as { x: number; y: number } | null;
    return point !== null;
  }, `${needle} isn't on screen: ${await paneText(pane)}`);
  return point as unknown as { x: number; y: number };
}

/** Clicks `needle` in pane `pane`, with Ctrl (Cmd on macOS) held when
 * `mod`. The pointer comes from a few rows below, so xterm looks for a link
 * where it lands, and rests there while the core resolves it. */
async function clickText(pane: string, needle: string, mod: boolean): Promise<void> {
  const { x, y } = await pointOf(pane, needle);
  const at = { x: Math.round(x), y: Math.round(y), origin: "viewport" as const };
  const pointer = browser
    .action("pointer", { parameters: { pointerType: "mouse" } })
    .move({ ...at, y: at.y + 48 })
    .move(at)
    .pause(1_000)
    .down({ button: 0 })
    .up({ button: 0 })
    .pause(0);
  if (!mod) {
    await pointer.perform();
    return;
  }
  const key = process.platform === "darwin" ? Key.Command : Key.Ctrl;
  await browser.actions([browser.action("key").down(key).pause(0).pause(0).pause(0).pause(0).up(key), pointer]);
}

const modClick = (pane: string, needle: string) => clickText(pane, needle, true);

/** Whether xterm shows a link under the pointer in pane `pane`. */
const onLink = (pane: string) =>
  browser.execute((id: string) => document.querySelector(`[data-testid="pane"][data-pane-id="${id}"] .xterm-cursor-pointer`) !== null, pane);

/** The Files tab's active editor tab and the line its cursor is on. */
const editor = () =>
  browser.execute(() => {
    const tab = document.querySelector<HTMLElement>('[data-testid="editor-tab"][aria-selected="true"]');
    const view = document.querySelector<HTMLElement>('[data-testid="code-editor"]');
    return {
      path: tab?.dataset.path ?? "",
      line: view?.querySelector(".cm-activeLineGutter")?.textContent ?? "",
      text: view?.querySelector(".cm-activeLine")?.textContent ?? "",
      readonly: view?.dataset.readonly === "true",
    };
  });

describe("E2E-M9-5 links", () => {
  const folder = `/tmp/tandem-e2e-m9-5-${Date.now()}`;
  let shell = "";

  before(async function () {
    this.timeout(60_000);
    mkdirSync(join(folder, "src"), { recursive: true });
    mkdirSync(join(folder, "sub/src"), { recursive: true });
    writeFileSync(join(folder, "src/main.rs"), Array.from({ length: 60 }, (_, i) => `// main line ${i + 1}\n`).join(""));
    writeFileSync(join(folder, "sub/src/main.rs"), Array.from({ length: 10 }, (_, i) => `// sub line ${i + 1}\n`).join(""));
    await freshOffline();
    await addFolder(folder);
    await until(async () => (await paneIds()).length === 1, "the folder's shell didn't open", 20_000);
    shell = (await paneIds())[0] ?? "";
    await waitForPaneText(shell, "#");
  });

  it("F-CD-09.1 Mod+Click on src/main.rs:42:7 opens it with the cursor on line 42", async () => {
    await sendToPane(shell, 'clear; echo "src""/main.rs:42:7"\n');
    await modClick(shell, "src/main.rs:42:7");
    await until(async () => (await editor()).line === "42", `the editor isn't on line 42: ${JSON.stringify(await editor())}`);
    expect(await editor()).toEqual({ path: "src/main.rs", line: "42", text: "// main line 42", readonly: false });
  });

  it("F-CD-09 a plain click on a link opens nothing", async () => {
    await sendToPane(shell, 'clear; echo "src""/main.rs:7"\n');
    await clickText(shell, "src/main.rs:7", false);
    // The link was there under the pointer, and the click left it be.
    await until(() => onLink(shell), "src/main.rs:7 isn't a link");
    await browser.pause(1_000);
    expect((await editor()).line).toBe("42");
  });

  it("F-CD-09.2 Mod+Click on /etc/hosts opens it read-only", async () => {
    await sendToPane(shell, 'clear; echo "/etc""/hosts"\n');
    await modClick(shell, "/etc/hosts");
    await until(async () => (await editor()).path === "/etc/hosts", `/etc/hosts didn't open: ${JSON.stringify(await editor())}`);
    expect((await editor()).readonly).toBe(true);
    expect(await browser.execute(() => document.body.textContent?.includes("This file is outside your workspaces, so it's read-only."))).toBe(true);
  });

  it("F-CD-09.3 after cd sub, a relative path resolves against sub", async () => {
    await sendToPane(shell, 'cd sub; clear; echo "src""/main.rs:3"\n');
    await modClick(shell, "src/main.rs:3");
    await until(async () => (await editor()).path === "sub/src/main.rs", `sub's file didn't open: ${JSON.stringify(await editor())}`);
    await until(async () => (await editor()).line === "3", `the editor isn't on line 3: ${JSON.stringify(await editor())}`);
    expect((await editor()).text).toBe("// sub line 3");
  });

  it("F-CD-09 Mod+Click on a URL opens it in the app's browser", async () => {
    await sendToPane(shell, 'clear; echo "http://local""host:5173/"\n');
    await modClick(shell, "http://localhost:5173/");
    await until(
      async () =>
        browser.execute(() =>
          Array.from(document.querySelectorAll('[data-testid="sidebar-pill-browser"]')).some((el) => el.textContent === "Fixtures"),
        ),
      "the page didn't open in a browser tab",
    );
  });
});
