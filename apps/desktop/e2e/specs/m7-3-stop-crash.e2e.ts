import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { freshOffline } from "../support/app";
import { assistantTexts, openThreadId, sendMessage, threadMode } from "../support/thread";

/** Engine processes still running for a thread (its MCP config names it). */
function engines(threadId: string): number {
  try {
    return execFileSync("pgrep", ["-f", `${threadId}.json`], { encoding: "utf8" })
      .trim()
      .split("\n")
      .filter(Boolean).length;
  } catch {
    return 0;
  }
}

// E2E-M7-3 Stop and crash (F-TH-03 AC2, AC4): Stop on an engine that ignores
// stdin and SIGINT ends the turn cancelled within 3.5 s with its process tree
// gone; an engine that dies mid-turn leaves an error with Retry, and the app
// carries on.
describe("E2E-M7-3 stop and crash", () => {
  it("F-TH-03.2 Stop on an engine that ignores it is cancelled within 3.5 s, tree gone", async function () {
    this.timeout(90_000);
    await freshOffline();
    await threadMode();
    await sendMessage("/hang");
    await browser.waitUntil(async () => (await assistantTexts()).some((t) => t.includes("Working on it")), {
      timeout: 20_000,
      timeoutMsg: `no streamed text: ${JSON.stringify(await assistantTexts())}`,
    });
    const thread = await openThreadId();
    expect(engines(thread)).toBeGreaterThan(0);
    const stop = await $('[data-testid="composer-stop"]');
    const t0 = Date.now();
    await stop.click();
    await $("p*=Stopped by you").waitForExist({ timeout: 6_000 });
    const took = Date.now() - t0;
    // The core's budget is 3.5 s; the UI adds a render.
    expect(took).toBeLessThan(4_000);
    await browser.waitUntil(async () => engines(thread) === 0, {
      timeout: 2_000,
      timeoutMsg: "the engine tree is still running",
    });
    // The composer is usable again.
    expect(await $('[data-testid="composer-send"]').isExisting()).toBe(true);
  });

  it("F-TH-03.4 an engine that exits mid-turn shows an error with Retry, and the app stays up", async function () {
    this.timeout(60_000);
    await sendMessage("/crash");
    const alerts = () =>
      browser.execute(() =>
        Array.from(document.querySelectorAll<HTMLElement>('[role="alert"]')).map((e) => e.innerText),
      );
    await browser.waitUntil(async () => (await alerts()).some((t) => t.includes("stopped unexpectedly")), {
      timeout: 20_000,
      timeoutMsg: `alerts: ${JSON.stringify(await alerts())}`,
    });
    expect((await alerts()).find((t) => t.includes("stopped unexpectedly"))).toContain("exit 3");
    expect(await $("button=Retry turn").isExisting()).toBe(true);
    // Still responsive: a new message gets an answer from a new session.
    await sendMessage("still there?");
    await browser.waitUntil(
      async () => (await assistantTexts()).some((t) => t.includes("You said: still there?")),
      { timeout: 20_000 },
    );
  });
});
