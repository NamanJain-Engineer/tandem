import { $, browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, mod, paneIds, paneText, sendToPane, waitForPaneText } from "../support/app";

const GOLDENS = new URL("../goldens", import.meta.url).pathname;

type Hook = { paneScreen: (id: string) => string; paneSize: (id: string) => { cols: number; rows: number } | null };

/** The screen as rows, from the serialize addon (the alternate screen when a TUI has it). */
async function screen(pane: string): Promise<string[]> {
  const raw = String(await browser.execute((id: string) => (window as unknown as { __tandemTest: Hook }).__tandemTest.paneScreen(id), pane));
  const parts = raw.split("\x1b[?1049h\x1b[H");
  return (parts[parts.length - 1] ?? "").split("\r\n");
}

/** Waits until the screen stops changing, then returns it. */
async function stable(pane: string): Promise<string[]> {
  let last = "";
  let rows: string[] = [];
  await browser.waitUntil(
    async () => {
      rows = await screen(pane);
      const now = JSON.stringify(rows);
      const same = now === last;
      last = now;
      return same;
    },
    { timeout: 10_000, interval: 300, timeoutMsg: "the screen never settled" },
  );
  return rows;
}

async function size(pane: string): Promise<string> {
  const s = (await browser.execute((id: string) => (window as unknown as { __tandemTest: Hook }).__tandemTest.paneSize(id), pane)) as {
    cols: number;
    rows: number;
  } | null;
  return s ? `${s.cols}x${s.rows}` : "unknown";
}

/** Compares with `goldens/<name>.<cols>x<rows>.json`; UPDATE_GOLDENS=1 writes it. */
async function golden(name: string, pane: string, rows: string[]): Promise<void> {
  const file = join(GOLDENS, `${name}.${await size(pane)}.json`);
  const body = `${JSON.stringify(rows, null, 1)}\n`;
  if (process.env.UPDATE_GOLDENS === "1") {
    mkdirSync(GOLDENS, { recursive: true });
    writeFileSync(file, body);
    return;
  }
  if (!existsSync(file)) throw new Error(`no golden ${file} (run with UPDATE_GOLDENS=1 and review it)`);
  expect(JSON.parse(readFileSync(file, "utf8"))).toEqual(rows);
}

/** Collapses or restores the rail (Mod+\ outside a terminal) and waits for
 * the pane's terminal to take its new width. */
async function toggleRail(pane?: string): Promise<void> {
  const before = pane ? await size(pane) : "";
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), "\\"]);
  if (pane) await browser.waitUntil(async () => (await size(pane)) !== before, { timeout: 5_000, timeoutMsg: `still ${before}` });
  await browser.pause(300);
}

/** Rows as text: cursor-forward runs (the serializer's blanks) become spaces, other sequences go. */
const plain = (rows: string[]) =>
  rows.map((r) =>
    r
      .replace(/\x1b\[(\d*)C/g, (_, n: string) => " ".repeat(Number(n || "1"))) // eslint-disable-line no-control-regex
      .replace(/\x1b\[[0-9;?]*[A-Za-z]/g, ""), // eslint-disable-line no-control-regex
  );

// E2E-M3-2 TUIs (F-CD-03, F-CD-04): vim, htop and less render and redraw
// after a resize; serialized screens match the goldens.
describe("E2E-M3-2 TUIs", () => {
  const root = `/tmp/tandem-e2e-m3-2-${Date.now()}`;
  let pane = "";

  before(async () => {
    mkdirSync(root, { recursive: true });
    writeFileSync(join(root, "notes.txt"), Array.from({ length: 60 }, (_, i) => `line ${String(i + 1).padStart(2, "0")} the quick brown fox`).join("\n") + "\n");
    await freshOffline();
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    pane = (await paneIds())[0] ?? "";
    await waitForPaneText(pane, "#");
    // The rail's state persists between runs; goldens are taken with it open.
    if ((await $('nav[aria-label="Workspaces"]').getSize("width")) < 100) await toggleRail(pane);
    // The program and the view agree on the size (the view measures its
    // cell once it is in the document, then resizes the PTY to match).
    const want = await size(pane);
    await sendToPane(pane, "clear; stty size\n");
    await browser.waitUntil(async () => /^\d+ \d+$/m.test(await paneText(pane)), { timeout: 5_000 });
    const [rows, cols] = (/^(\d+) (\d+)$/m.exec(await paneText(pane)) ?? []).slice(1);
    expect(`${cols}x${rows}`).toBe(want);
  });

  it("vim: :e a file, resize, :q", async () => {
    await sendToPane(pane, "vim -u NONE -N -i NONE --cmd 'set shortmess+=I'\n");
    await browser.waitUntil(async () => plain(await screen(pane)).some((r) => r.startsWith("~")), { timeout: 10_000 });
    // vim reads its terminal-query replies right after drawing; keys typed
    // then can be taken as part of a reply.
    await browser.pause(500);
    await sendToPane(pane, ":e notes.txt\r");
    await browser.waitUntil(async () => plain(await screen(pane)).some((r) => r.includes('"notes.txt" 60L')), {
      timeout: 10_000,
      timeoutMsg: `vim's file message: ${JSON.stringify((await screen(pane)).slice(-2))}`,
    });
    await golden("vim-edit", pane, await stable(pane));
    await toggleRail(pane);
    const resized = await stable(pane);
    expect(plain(resized)[0]).toContain("line 01");
    await golden("vim-resized", pane, resized);
    await sendToPane(pane, ":q\r");
    await waitForPaneText(pane, "vim -u NONE");
    await browser.waitUntil(async () => !(await screen(pane)).join("").includes("line 01 the quick"), { timeout: 5_000, timeoutMsg: "vim's screen is still up" });
    await toggleRail(pane);
  });

  it("htop: resize", async () => {
    await sendToPane(pane, "clear; htop\n");
    await browser.waitUntil(async () => plain(await screen(pane)).some((r) => r.includes("F10Quit")), { timeout: 10_000 });
    await toggleRail(pane);
    // htop's numbers move; its column header and key bar don't.
    await browser.waitUntil(async () => plain(await screen(pane)).some((r) => r.includes("F10Quit")), { timeout: 10_000 });
    const rows = plain(await screen(pane));
    const header = rows.find((r) => /\bPID\b.*\bCommand\b/.test(r))?.trim().replace(/\s+/g, " ") ?? "";
    const keys = rows.find((r) => r.includes("F10Quit"))?.trim().replace(/\s+/g, " ") ?? "";
    await golden("htop-resized", pane, [header, keys]);
    await sendToPane(pane, "q");
    await toggleRail(pane);
  });

  it("less: open a file and page through it", async () => {
    await sendToPane(pane, "clear; less notes.txt\n");
    await browser.waitUntil(async () => plain(await screen(pane)).some((r) => r.includes("line 01")), { timeout: 10_000 });
    await sendToPane(pane, " ");
    await browser.waitUntil(async () => !plain(await screen(pane)).some((r) => r.includes("line 01 ")), { timeout: 5_000 });
    await golden("less-page2", pane, await stable(pane));
    await sendToPane(pane, "q");
  });
});
