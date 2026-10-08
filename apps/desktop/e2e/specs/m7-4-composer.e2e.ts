import { $, browser } from "@wdio/globals";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { addFolder, freshOffline, home, invoke, mod, paneIds } from "../support/app";
import { assistantTexts, composer, openThreadId, png, sendMessage, threadMode, userTexts, waitForAssistant } from "../support/thread";

type Thread = { id: string; access: string; workspaceId: string | null };

const value = async () => String(await composer().getProperty("value"));

/** Empties the composer the way a user would (clearValue skips React's onChange). */
async function clearComposer(): Promise<void> {
  await composer().click();
  await browser.keys([mod(), "a"]);
  await browser.keys("Backspace");
}

// E2E-M7-4 Composer (F-TH-07): Shift+Enter and Enter, IME-safe Enter, a failed
// send restores its text, a pasted screenshot becomes a PNG chip in
// $DATA/attachments/<thread>/, `@src/` lists only files that aren't ignored,
// and a new thread takes the remembered defaults.
describe("E2E-M7-4 composer", () => {
  const root = `/tmp/tandem-e2e-m7-4-${Date.now()}`;
  let threadId = "";

  before(async function () {
    this.timeout(60_000);
    mkdirSync(`${root}/src/gen`, { recursive: true });
    writeFileSync(`${root}/src/app.ts`, "export {};\n");
    writeFileSync(`${root}/src/gen/big.ts`, "export {};\n");
    writeFileSync(`${root}/.gitignore`, "src/gen/\n");
    await freshOffline();
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await threadMode();
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    threadId = (await invoke<Thread>("thread_create", { args: { kind: "thread", workspaceId: ws?.id } })).id;
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
  });

  it("F-TH-07.1 Shift+Enter inserts a newline and Enter sends", async function () {
    this.timeout(60_000);
    const box = composer();
    await box.click();
    await box.setValue("line one");
    await browser.keys(["Shift", "Enter"]);
    await browser.keys("line two".split(""));
    expect(await value()).toBe("line one\nline two");
    await browser.keys("Enter");
    await waitForAssistant("You said: line one");
    expect(await userTexts()).toContain("line one\nline two");
    expect(await value()).toBe("");
  });

  it("F-TH-07.2 Enter during IME composition doesn't send", async function () {
    this.timeout(30_000);
    const box = composer();
    await box.click();
    await box.setValue("かな");
    const before = (await userTexts()).length;
    await browser.execute(() => {
      const el = document.querySelector('[data-testid="composer"] textarea');
      // The Enter that confirms a composition (keyCode 229 can't be set on a
      // synthetic event; isComposing can).
      el?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true, isComposing: true }));
    });
    await browser.pause(500);
    expect((await userTexts()).length).toBe(before);
    expect(await value()).toBe("かな");
    await clearComposer();
  });

  it("F-TH-07.3 a failed send puts the text back ahead of anything typed since", async function () {
    this.timeout(60_000);
    // A running turn with a queued follow-up: a third message can't be sent.
    await sendMessage("/hang");
    await browser.waitUntil(async () => (await assistantTexts()).some((t) => t.includes("Working on it")), { timeout: 20_000 });
    await sendMessage("queued one");
    await $("span*=Queued · sends when this turn finishes").waitForExist({ timeout: 5_000 });
    await sendMessage("will fail");
    await browser.waitUntil(async () => (await value()).startsWith("will fail"), {
      timeout: 5_000,
      timeoutMsg: `composer: ${JSON.stringify(await value())}`,
    });
    // Stop with Esc (the toast for the failed send covers the Stop button's
    // corner); the queued message goes back to the draft, not sent.
    await composer().click();
    await browser.keys("Escape");
    await $("p*=Stopped by you").waitForExist({ timeout: 6_000 });
    await clearComposer();
  });

  it("F-TH-07.5 a pasted screenshot becomes a PNG chip saved under $DATA/attachments", async function () {
    this.timeout(30_000);
    const file = `${root}/shot.png`;
    writeFileSync(file, png(4, 3));
    const owner = spawn("xclip", ["-selection", "clipboard", "-t", "image/png", "-i", file], { stdio: "ignore", detached: true });
    owner.unref();
    await browser.pause(500);
    await composer().click();
    await browser.keys(["Control", "v"]);
    // (A selector ending in ".png" reads as an image file to WebdriverIO.)
    const chips = () => browser.execute(() => document.querySelector('[data-testid="composer"]')?.textContent ?? "");
    await browser.waitUntil(async () => (await chips()).includes("Pasted image.png"), {
      timeout: 5_000,
      timeoutMsg: `composer: ${JSON.stringify(await chips())}`,
    });
    const dir = `${home()}/attachments/${threadId}`;
    const saved = existsSync(dir) ? execFileSync("ls", [dir], { encoding: "utf8" }).trim().split("\n") : [];
    expect(saved.length).toBe(1);
    const bytes = readFileSync(`${dir}/${saved[0]}`);
    expect(bytes.subarray(1, 4).toString()).toBe("PNG");
    try {
      execFileSync("pkill", ["-f", `xclip -selection clipboard -t image/png -i ${file}`]);
    } catch {
      // already gone
    }
  });

  it("F-TH-07.6 @src/ lists files under src/ and leaves out ignored ones", async function () {
    this.timeout(30_000);
    const box = composer();
    await box.click();
    await box.setValue("look at @src/");
    const list = await $('[data-testid="composer"] [role="listbox"]');
    await list.waitForExist({ timeout: 5_000 });
    await browser.waitUntil(async () => (await list.$$('[role="option"]').length) > 0, { timeout: 5_000 });
    const options = await browser.execute(() =>
      Array.from(document.querySelectorAll('[data-testid="composer"] [role="option"]')).map((o) => o.textContent ?? ""),
    );
    expect(options).toEqual(["src/app.ts"]);
    await browser.keys("Escape");
    await clearComposer();
  });

  it("F-TH-07.4 a new thread takes the engine's remembered access", async function () {
    this.timeout(30_000);
    expect(await openThreadId()).toBe(threadId);
    await invoke("thread_update", { threadId, access: "read" });
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const next = await invoke<Thread>("thread_create", { args: { kind: "thread", workspaceId: ws?.id } });
    expect(next.access).toBe("read");
  });
});
