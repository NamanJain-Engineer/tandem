import { $, browser } from "@wdio/globals";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { checkAgain, fakeEngine, freshOffline, home, invoke, mod, relaunch } from "../support/app";
import { connectTestPlugin, makeTeammate, newChat, sendAndWait, type Teammate } from "../support/agents";

type SpawnLine = { argv: string[]; cwd: string };

/** Clicks the teammate page's tab whose label starts with `name`. */
async function openTab(name: string): Promise<void> {
  const clicked = await browser.execute((n: string) => {
    const tab = Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid="teammate-page"] [role="tab"]'),
    ).find((el) => (el.textContent ?? "").trim().startsWith(n));
    // Radix tabs switch on mousedown, not click.
    tab?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    tab?.click();
    return !!tab;
  }, name);
  expect(clicked).toBe(true);
}

async function openTeammate(id: string): Promise<void> {
  await browser.keys([mod(), "1"]);
  await $(`[data-teammate-id="${id}"]`).click();
  await $('[data-testid="teammate-page"]').waitForDisplayed({ timeout: 10_000 });
}

async function save(): Promise<void> {
  await $('[data-testid="settings-save"]').click();
  // The bar re-renders while it saves: poll the DOM rather than hold an element.
  await browser.waitUntil(
    async () => browser.execute(() => !document.querySelector('[data-testid="save-bar"]')),
    { timeout: 10_000, timeoutMsg: "the Save bar stayed" },
  );
}

async function tabOrder(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid="teammate-page"] [data-testid="chat-tab"]'),
    ).map((el) => el.dataset.chatId ?? ""),
  );
}

// E2E-M10-6 Settings (F-AG-05, F-AG-03): May message off takes the mail
// tool away from the next turn; chat tab order survives a relaunch; the
// plugin nudge's Enable grants the plugin; a new engine runs the next chat
// with memory unchanged.
describe("E2E-M10-6 settings", () => {
  let rhea: Teammate;

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    rhea = await makeTeammate("Rhea");
  });

  after(() => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
  });

  it("F-AG-05 May message off: tandem__send_message is absent from the next turn's tools/list", async function () {
    this.timeout(60_000);
    const chat = await newChat(rhea.id);
    expect((await sendAndWait(chat, "tools")).join(" ")).toContain("tandem__send_message");
    await openTeammate(rhea.id);
    await openTab("Settings");
    await $('[data-testid="teammate-settings"] [aria-label="May message teammates"]').click();
    await save();
    const listed = (await sendAndWait(chat, "tools")).join(" ");
    expect(listed).not.toContain("tandem__send_message");
    expect(listed).not.toContain("tandem__list_teammates");
    expect(listed).toContain("tandem__memory_write");
  });

  it("F-AG-03.1 the chat tab order persists", async function () {
    this.timeout(90_000);
    await newChat(rhea.id);
    await openTeammate(rhea.id);
    await openTab("Chats");
    await browser.waitUntil(async () => (await tabOrder()).length >= 2, { timeout: 10_000 });
    const before = await tabOrder();
    // Drag the last tab onto the first.
    await browser.execute(() => {
      const tabs = Array.from(
        document.querySelectorAll<HTMLElement>('[data-testid="teammate-page"] [data-testid="chat-tab"]'),
      );
      const from = tabs[tabs.length - 1];
      const to = tabs[0];
      if (!from || !to) return;
      const dt = new DataTransfer();
      from.dispatchEvent(new DragEvent("dragstart", { bubbles: true, cancelable: true, dataTransfer: dt }));
      to.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }));
      to.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
    });
    const moved = [before[before.length - 1] ?? "", ...before.slice(0, -1)];
    await browser.waitUntil(async () => JSON.stringify(await tabOrder()) === JSON.stringify(moved), {
      timeout: 10_000,
      timeoutMsg: `tabs didn't move: ${JSON.stringify(await tabOrder())}`,
    });
    await relaunch();
    await openTeammate(rhea.id);
    await browser.waitUntil(async () => (await tabOrder()).length === moved.length, { timeout: 20_000 });
    expect(await tabOrder()).toEqual(moved);
  });

  it("F-PL-05 the nudge's Enable grants test_echo to the teammate", async function () {
    this.timeout(60_000);
    const conn = await connectTestPlugin("test_echo");
    await openTeammate(rhea.id);
    await openTab("Chats");
    const nudge = await $('[data-testid="plugin-nudge"]');
    await nudge.waitForDisplayed({ timeout: 10_000 });
    await nudge.$("button=Enable").click();
    await nudge.waitForExist({ reverse: true, timeout: 10_000 });
    const grants = await invoke<string[]>("grants_list", {
      subject: { type: "teammate", teammateId: rhea.id },
    });
    expect(grants).toContain(conn);
  });

  it("F-AG-05.1 a new engine runs the next chat, with memory unchanged", async function () {
    this.timeout(90_000);
    const notes = join(rhea.home, "memory", "NOTES.md");
    writeFileSync(notes, "- Keeps dependencies current.\n");
    fakeEngine("codex", join(home(), "fakebin"));
    await checkAgain();
    await openTeammate(rhea.id);
    await openTab("Settings");
    const select = $('[data-testid="teammate-settings"] [role="combobox"][aria-label="Engine"]');
    await select.waitForExist({ timeout: 10_000 });
    await select.scrollIntoView();
    await select.waitForClickable({ timeout: 10_000 });
    // Radix Select opens on pointerdown: press and release on the trigger.
    await browser
      .action("pointer")
      .move({ origin: await select.getElement() })
      .down()
      .up()
      .perform();
    const codex = $('//*[@role="option"][.//*[normalize-space()="Codex"]]');
    await codex.waitForClickable({ timeout: 10_000 });
    await codex.click();
    await save();
    const before = (await invoke<SpawnLine[]>("test_spawn_log")).length;
    const chat = await newChat(rhea.id);
    const thread = await invoke<{ thread: { engineId: string } }>("thread_get", { threadId: chat });
    expect(thread.thread.engineId).toBe("codex");
    await sendAndWait(chat, "hello codex");
    const spawned = (await invoke<SpawnLine[]>("test_spawn_log")).slice(before);
    expect(spawned.some((s) => s.argv.includes("app-server"))).toBe(true);
    expect(readFileSync(notes, "utf8")).toBe("- Keeps dependencies current.\n");
  });
});
