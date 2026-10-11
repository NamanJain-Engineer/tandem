import { $, browser } from "@wdio/globals";
import { mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, home, invoke, paneIds, paneText, sendToPane, split, waitForPaneText } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M17-2 Command recall (F-EX-02): a bash pane's commands are recorded
// with their exit codes (a space-prefixed one isn't); Insert puts a command at
// the prompt and runs nothing; Run runs it once; a token is hidden and can't
// be run.

type Entry = { id: number; command: string; exitCode: number | null; runs: number; secret: boolean };

async function history(query = ""): Promise<Entry[]> {
  return invoke<Entry[]>("history_search", { query, workspaceId: null, limit: 50 });
}

/** Opens the recall palette with its shortcut (Ctrl+Alt+J). */
async function openRecall(): Promise<void> {
  await browser.execute(() => {
    const mac = navigator.platform.toLowerCase().includes("mac");
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyJ", key: "j", ctrlKey: !mac, metaKey: mac, altKey: true, bubbles: true, cancelable: true }));
  });
  await $('[data-testid="quick-picker"]').waitForDisplayed({ timeout: 10_000 });
}

async function rows(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="recall-row"]')).map((r) => `${r.dataset.command}|${r.textContent ?? ""}`),
  );
}

async function search(text: string): Promise<void> {
  const input = await $('[data-testid="quick-picker"] input');
  await input.setValue(text);
  await browser.waitUntil(async () => (await rows()).length === 1, { timeout: 10_000, timeoutMsg: `rows: ${JSON.stringify(await rows())}` });
}

describe("E2E-M17-2 command recall", () => {
  const dir = `/tmp/tandem-e2e-m17-2-${Date.now()}`;
  let shell = "";

  before(async function () {
    this.timeout(60_000);
    mkdirSync(dir, { recursive: true });
    await freshOffline();
    await addFolder(dir);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    // bash with shell integration (the run's own login shell may be sh).
    await invoke("settings_set", { key: "default_shell", value: "/bin/bash" });
    shell = await split("right");
    // Ready once its first prompt reported the folder (OSC 7).
    await browser.waitUntil(
      async () => {
        for (const w of await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list"))
          for (const tab of w.tabs)
            if ((await invoke<Array<{ id: string; cwd: string | null }>>("pane_list", { tabId: tab.id })).some((p) => p.id === shell && p.cwd === dir))
              return true;
        return false;
      },
      { timeout: 20_000, timeoutMsg: "bash never reported its folder" },
    );
  });

  after(async () => {
    await invoke("settings_set", { key: "default_shell", value: null });
  });

  it("F-EX-02.1 commands are recorded with their exit codes; F-EX-02.3 a space-prefixed one isn't", async function () {
    this.timeout(60_000);
    await sendToPane(shell, "echo one\r");
    await waitForPaneText(shell, "one");
    await sendToPane(shell, "false\r");
    await sendToPane(shell, " echo hidden\r");
    await waitForPaneText(shell, "hidden");
    await browser.waitUntil(async () => (await history()).length === 2, { timeout: 10_000, timeoutMsg: `history: ${JSON.stringify(await history())}` });
    const got = await history();
    expect(got.map((e) => [e.command, e.exitCode])).toEqual([
      ["false", 1],
      ["echo one", 0],
    ]);
    // The command line never shows in the terminal as a mark.
    expect(await paneText(shell)).not.toContain("cmdline_url");
    await openRecall();
    // The list loads after the picker opens.
    await browser.waitUntil(async () => (await rows()).length === 2, { timeout: 10_000, timeoutMsg: `rows: ${JSON.stringify(await rows())}` });
    const shown = await rows();
    expect(shown.map((r) => r.split("|")[0])).toEqual(["false", "echo one"]);
    expect(shown[0]).toContain("✗ 1");
    await expectNoSeriousA11yViolationsInBothThemes();
    await browser.keys("Escape");
  });

  it("F-EX-02.4 Insert puts the command at the prompt and runs nothing; Run runs it once", async function () {
    this.timeout(60_000);
    await openRecall();
    await search("one");
    await browser.keys("Enter");
    await $('[data-testid="quick-picker"]').waitForExist({ timeout: 5_000, reverse: true });
    // The command line now reads "echo one" with nothing run.
    await browser.waitUntil(async () => (await paneText(shell)).trimEnd().endsWith("echo one"), {
      timeout: 5_000,
      timeoutMsg: `screen: ${await paneText(shell)}`,
    });
    await browser.pause(800);
    expect((await history("echo one"))[0]?.runs).toBe(1);
    // Clear the inserted line (Ctrl+U), then Run from the palette.
    await sendToPane(shell, "\u0015");
    await openRecall();
    await search("one");
    await browser.keys(["Control", "Enter"]);
    await browser.waitUntil(async () => (await history("echo one"))[0]?.runs === 2, {
      timeout: 10_000,
      timeoutMsg: `runs: ${JSON.stringify(await history("echo one"))}`,
    });
  });

  it("F-EX-02.2 a token is hidden before it's stored and can't be run", async function () {
    this.timeout(60_000);
    const token = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    await sendToPane(shell, `export GITHUB_TOKEN=${token}\r`);
    await browser.waitUntil(async () => (await history("GITHUB")).length === 1, { timeout: 10_000, timeoutMsg: "not recorded" });
    const [e] = await history("GITHUB");
    expect(e?.secret).toBe(true);
    expect(e?.command).toContain("[REDACTED]");
    expect(e?.command).not.toContain(token);
    // Nowhere in the profile's database files.
    const files = readdirSync(home()).filter((f) => f.startsWith("tandem.db"));
    for (const f of files) {
      const p = join(home(), f);
      if (statSync(p).isFile()) expect(readFileSync(p).includes(token)).toBe(false);
    }
    await openRecall();
    await search("GITHUB");
    expect((await rows())[0]).toContain("[REDACTED]");
    await browser.keys(["Control", "Enter"]);
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.querySelector('[role="region"][aria-label="Notifications"]')?.textContent ?? "")).includes(
          "hidden secret",
        ),
      { timeout: 5_000, timeoutMsg: "no secret notice" },
    );
    await browser.keys("Escape");
  });
});
