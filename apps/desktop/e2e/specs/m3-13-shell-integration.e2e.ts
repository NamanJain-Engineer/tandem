import { $, browser } from "@wdio/globals";
import { mkdirSync, writeFileSync } from "node:fs";
import { addFolder, freshOffline, invoke, mod, paneIds, sendToPane, split, typeKeys, waitForPaneText } from "../support/app";

type PaneRow = { id: string; cwd: string | null };

async function tabOf(paneId: string): Promise<string> {
  for (const w of await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list")) {
    for (const tab of w.tabs) {
      if ((await invoke<PaneRow[]>("pane_list", { tabId: tab.id })).some((p) => p.id === paneId)) return tab.id;
    }
  }
  throw new Error(`no tab holds ${paneId}`);
}

async function storedCwd(tabId: string, paneId: string): Promise<string | null> {
  return (await invoke<PaneRow[]>("pane_list", { tabId })).find((p) => p.id === paneId)?.cwd ?? null;
}

/** Types `cd <dir>` and polls the stored cwd from inside the page, so the
 * time measured is the app's, not WebDriver round trips. */
async function cdAndTime(paneId: string, tabId: string, dir: string): Promise<number> {
  const ms = await browser.executeAsync(
    (pane: string, tab: string, target: string, done: (ms: number) => void) => {
      const ipc = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const t0 = performance.now();
      void ipc.invoke("pty_write", { paneId: pane, data: `cd ${target}\r` }).then(() => {
        const poll = () => {
          void ipc.invoke("pane_list", { tabId: tab }).then((rows) => {
            const row = (rows as Array<{ id: string; cwd: string | null }>).find((p) => p.id === pane);
            if (row?.cwd === target) done(performance.now() - t0);
            else if (performance.now() - t0 > 5_000) done(-1);
            else setTimeout(poll, 5);
          });
        };
        poll();
      });
    },
    paneId,
    tabId,
    dir,
  );
  return Number(ms);
}

// E2E-M3-13 Shell integration (F-CD-10).
describe("E2E-M3-13 shell integration", () => {
  const root = `/tmp/tandem-e2e-m3-13-${Date.now()}`;
  /** The bash pane from the first test (integration on). */
  let bash = "";

  before(async () => {
    mkdirSync(`${root}/listing`, { recursive: true });
    for (const f of ["alpha.txt", "beta.txt", "gamma.txt"]) writeFileSync(`${root}/listing/${f}`, "x");
    await freshOffline();
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  after(async () => {
    await invoke("settings_set", { key: "default_shell", value: null });
  });

  for (const shell of ["/bin/bash", "/usr/bin/zsh"]) {
    it(`F-CD-10.1 cd /tmp in ${shell} stores the cwd within 200 ms`, async () => {
      await invoke("settings_set", { key: "default_shell", value: shell });
      const pane = await split("right");
      const tab = await tabOf(pane);
      // The first prompt reports the starting folder.
      await browser.waitUntil(async () => (await storedCwd(tab, pane)) === root, {
        timeout: 10_000,
        timeoutMsg: `${shell} never reported its cwd: ${await storedCwd(tab, pane)}`,
      });
      if (shell === "/bin/bash") bash = pane;
      const ms = await cdAndTime(pane, tab, "/tmp");
      expect(ms).toBeGreaterThan(0);
      expect(ms).toBeLessThan(200);
    });
  }

  it("F-CD-10.2 a program's own OSC 133 marks are ignored; the shell's are not", async () => {
    const pane = bash;
    const failed = () => browser.execute((id: string) => document.querySelectorAll(`[data-pane-id="${id}"] .tn-cmd-mark[data-ok="false"]`).length, pane);
    // A spoofed command that "failed": no ✗ appears.
    await sendToPane(pane, "printf '\\033]133;A\\007\\033]133;C\\007\\033]133;D;1\\007spoofed\\n'\n");
    await waitForPaneText(pane, "spoofed");
    await browser.pause(300);
    expect(await failed()).toBe(0);
    // The shell's own marks for a failing command do.
    await sendToPane(pane, "false\n");
    await browser.waitUntil(async () => (await failed()) === 1, { timeout: 5_000, timeoutMsg: "no ✗ for false" });
  });

  it("F-CD-10.3 Copy last command output after ls is exactly ls's output", async () => {
    const pane = bash;
    await sendToPane(pane, `cd ${root}/listing\n`);
    await sendToPane(pane, "ls\n");
    await waitForPaneText(pane, "gamma.txt");
    const last = async () => (await invoke<{ text: string } | null>("term_last_output", { paneId: pane }))?.text ?? "";
    await browser.waitUntil(async () => (await last()).includes("gamma.txt"), {
      timeout: 5_000,
      timeoutMsg: `last output: ${await last()}`,
    });
    const expected = await last();
    // The palette command copies it.
    await $(`[data-testid="terminal"][data-pane-id="${pane}"]`).click();
    await browser.keys([mod(), "k"]);
    await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 3_000 });
    await typeKeys("Copy last command output");
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await invoke<string>("plugin:clipboard-manager|read_text")) === expected, {
      timeout: 5_000,
      timeoutMsg: `clipboard: ${JSON.stringify(await invoke("plugin:clipboard-manager|read_text").catch((e: unknown) => String(e)))}`,
    });
    // Exactly ls's output: the names, nothing from the prompt or the command line.
    expect(expected.split(/\s+/)).toEqual(["alpha.txt", "beta.txt", "gamma.txt"]);
  });
});
