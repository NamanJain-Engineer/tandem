import { $, browser } from "@wdio/globals";
import { Key } from "webdriverio";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { crash, freshOffline, home, relaunch } from "../support/app";
import { openFromPlus } from "../support/files";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M16-5 Scratchpad (F-SH-12): Mod+B wraps the selection in `**`; and
// typing without a pause, then killing the app (kill -9) within 1 s, keeps
// all but at most the last 300 ms of input. The typing is timestamped: every
// 25 ms the page inserts the time it types, through the editor's real input
// path, so the file on disk says when its last saved input was typed.

const dir = () => join(home(), "scratchpad");

/** The notes on disk, by id. */
function notesOnDisk(): Record<string, string> {
  if (!existsSync(dir())) return {};
  return Object.fromEntries(
    readdirSync(dir())
      .filter((f) => /^note_[0-9A-Z]{26}\.md$/.test(f))
      .map((f) => [f.replace(/\.md$/, ""), readFileSync(join(dir(), f), "utf8")]),
  );
}

async function until(check: () => Promise<boolean> | boolean, timeoutMsg: string, timeout = 10_000): Promise<void> {
  await browser.waitUntil(async () => check(), { timeout, timeoutMsg });
}

/** Opens the Scratchpad and starts a new note, with the cursor in it. */
async function newNote(): Promise<string> {
  const before = Object.keys(notesOnDisk());
  if (!(await $('[data-testid="scratchpad"]').isExisting())) await openFromPlus("Scratchpad");
  await $('[data-testid="scratchpad-new"]').waitForEnabled({ timeout: 10_000 });
  await $('[data-testid="scratchpad-new"]').click();
  let id = "";
  await until(() => {
    id = Object.keys(notesOnDisk()).find((n) => !before.includes(n)) ?? "";
    return id !== "";
  }, "the new note wasn't created on disk");
  await $('[data-testid="note-editor"] .cm-content').waitForDisplayed({ timeout: 10_000 });
  await until(
    async () => browser.execute(() => document.activeElement?.classList.contains("cm-content") ?? false),
    "the new note didn't take the cursor",
  );
  return id;
}

describe("E2E-M16-5 scratchpad", () => {
  before(async function () {
    this.timeout(60_000);
    await freshOffline();
  });

  it("F-SH-12.2 Mod+B wraps the selection in **", async () => {
    const id = await newNote();
    await $('[data-testid="note-editor"] .cm-content').addValue("hello world");
    // Select the last word, then bold it.
    const mod = process.platform === "darwin" ? Key.Alt : Key.Ctrl;
    await browser.keys([Key.Shift, mod, Key.ArrowLeft]);
    await browser.keys([process.platform === "darwin" ? Key.Command : Key.Ctrl, "b"]);
    await until(() => notesOnDisk()[id] === "hello **world**", `on disk: ${JSON.stringify(notesOnDisk()[id])}`);
    // The list names the note after its first line (a narrow sidebar shows
    // the list or the note, so go back to the list).
    if (await $('[data-testid="scratchpad-back"]').isExisting()) await $('[data-testid="scratchpad-back"]').click();
    await until(
      async () =>
        browser.execute(
          (note: string) => document.querySelector(`[data-testid="scratchpad-note"][data-id="${note}"]`)?.textContent?.startsWith("hello world") ?? false,
          id,
        ),
      "the list doesn't name the note",
    );
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("F-SH-12.1 killing the app within 1 s of typing keeps all but the last 300 ms", async function () {
    this.timeout(90_000);
    const id = await newNote();
    // Type a timestamp every 25 ms, without a pause, through the editor's
    // input handling.
    const started = (await browser.execute(() => {
      const w = window as unknown as { __typing?: ReturnType<typeof setInterval> };
      const t0 = Date.now();
      w.__typing = setInterval(() => document.execCommand("insertText", false, `${Date.now()} `), 25);
      return t0;
    })) as number;
    await browser.pause(800);
    const killedAt = Date.now();
    crash();
    expect(killedAt - started).toBeLessThan(1_000);

    const saved = notesOnDisk()[id] ?? "";
    const stamps = saved.trim().split(/\s+/).filter(Boolean).map(Number);
    // Something was saved while the typing never paused, in order, whole.
    expect(stamps.length).toBeGreaterThan(5);
    expect(stamps.every((s, i) => Number.isFinite(s) && (i === 0 || s >= (stamps[i - 1] ?? 0)))).toBe(true);
    expect(saved.endsWith(" ")).toBe(true);
    const lastSaved = stamps.at(-1) ?? 0;
    // At most 300 ms of input is lost; the rest of the allowance is one
    // keystroke (25 ms) and the write itself.
    expect(killedAt - lastSaved).toBeLessThanOrEqual(300 + 25 + 125);

    // The note is there after a restart.
    await relaunch();
    await openFromPlus("Scratchpad");
    await until(
      async () => browser.execute((note: string) => document.querySelector(`[data-testid="scratchpad-note"][data-id="${note}"]`) !== null, id),
      "the note isn't listed after the restart",
      20_000,
    );
  });
});
