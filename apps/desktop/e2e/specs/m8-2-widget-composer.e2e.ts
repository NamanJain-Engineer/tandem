import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, mod } from "../support/app";
import {
  ensureModel,
  fixture,
  hold,
  key,
  mic,
  platform,
  resetPlatform,
  setSetting,
  waitPasted,
  widget,
  words,
} from "../support/dictate";

// E2E-M8-4 Widget (F-DI-07) and E2E-M8-5 Composer (F-DI-12).

describe("E2E-M8-4 Widget", function () {
  this.timeout(300_000);
  before(async function () {
    await freshOffline();
    await setSetting("dictate_enabled", true);
    await ensureModel();
  });

  it("F-DI-07 the widget window is made on the first recording, which still captures from its start (D-519)", async function () {
    await setSetting("dictate_widget_visibility", "while_recording");
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    expect((await widget()).exists).toBe(false);
    await key("hold", "down");
    await browser.waitUntil(async () => (await widget()).visible, {
      timeout: 5_000,
      timeoutMsg: "the first recording didn't make and show the widget",
    });
    await browser.pause(2_500);
    await key("hold", "up");
    const view = await waitPasted();
    expect(words(view.pasted[0] ?? "")).toContain("hello world");
  });

  it("E2E-M8-4 'While recording' shows it when recording starts and hides it 1 s after insertion", async function () {
    await setSetting("dictate_widget_visibility", "while_recording");
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await browser.waitUntil(async () => !(await widget()).visible, {
      timeout: 5_000,
      timeoutMsg: "the widget showed while idle",
    });
    await key("hold", "down");
    await browser.waitUntil(async () => (await widget()).visible, {
      timeout: 2_000,
      timeoutMsg: "the widget didn't show",
    });
    await browser.pause(2_500);
    await key("hold", "up");
    await waitPasted();
    const insertedAt = Date.now();
    await browser.waitUntil(async () => !(await widget()).visible, {
      timeout: 5_000,
      timeoutMsg: "the widget stayed up",
    });
    expect(Date.now() - insertedAt).toBeGreaterThanOrEqual(700);
  });

  it("E2E-M8-4 an error toast carries the full diagnostic in its tooltip", async function () {
    await setSetting("dictate_widget_visibility", "always");
    await mic({ source: "zeros" });
    const main = await browser.getWindowHandle();
    // Zeros: a rebuild at 1 s, then "isn't delivering audio" at about 2 s,
    // while the key is still held (released sooner, it's just silence).
    await hold(3_500);
    let found = false;
    try {
      for (const h of await browser.getWindowHandles()) {
        if (h === main) continue;
        await browser.switchToWindow(h);
        const pill = $('[data-testid="dictate-widget"]');
        if (!(await pill.isExisting())) continue;
        // The state event says "error" first; the toast brings the detail.
        await browser.waitUntil(async () => ((await pill.getAttribute("title")) ?? "").length > 20, {
          timeout: 20_000,
          timeoutMsg: "no error toast with a diagnostic in the widget",
        });
        expect(await pill.getAttribute("data-state")).toBe("error");
        found = true;
      }
    } finally {
      await browser.switchToWindow(main);
    }
    expect(found).toBe(true);
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await setSetting("dictate_widget_visibility", "while_recording");
  });
});

describe("E2E-M8-5 Composer", function () {
  this.timeout(300_000);
  it("E2E-M8-5 dictation goes in at the caret, and the clipboard is unchanged", async function () {
    await resetPlatform();
    const id = (
      await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: null } })
    ).id;
    await browser.keys([mod(), "3"]);
    const row = $(`[data-testid="thread-row"][data-thread-id="${id}"]`);
    await row.waitForClickable({ timeout: 10_000 });
    await row.click();
    const area = $(`[data-testid="thread-main"][data-thread-id="${id}"] textarea`);
    await area.waitForDisplayed({ timeout: 10_000 });
    await area.click();
    await area.setValue("say  please");
    await browser.execute(
      (el: HTMLElement) => (el as HTMLTextAreaElement).setSelectionRange(4, 4),
      await area,
    );
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    await browser.waitUntil(async () => /^say .*hello.*world.* please$/i.test(await area.getValue()), {
      timeout: 60_000,
      timeoutMsg: `not inserted at the caret: ${await area.getValue()}`,
    });
    const view = await platform();
    expect(view.pasted).toEqual([]);
    expect(view.calls.some((c) => c.includes("clipboard"))).toBe(false);
    expect(view.clipboard).toBe("previous clipboard");
  });
});
