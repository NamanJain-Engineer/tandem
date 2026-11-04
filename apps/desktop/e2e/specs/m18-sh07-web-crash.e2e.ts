import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import {
  addFolder,
  freshOffline,
  home,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  waitFor,
  waitForPaneText,
} from "../support/app";

const pids = async (pane: string) => [...(await paneText(pane)).matchAll(/PID-(\d+)/g)].map((m) => m[1]);
const log = () => readFileSync(join(home(), "logs", "tandem.log"), "utf8");
const count = (code: string) => log().split(`code="${code}"`).length - 1;
/** The recovery the last stop chose, from its log line. */
const lastRecovery = () => [...log().matchAll(/code="window\.web_ended".*recovery=(\w+)/g)].pop()?.[1];
/** The native "keeps stopping" question answers from this file (TANDEM_FAKE_CONFIRM). */
const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

/** Marks the page, so a new one is told apart. */
const mark = () =>
  browser.execute(() => {
    (window as unknown as { __beforeStop?: boolean }).__beforeStop = true;
  });

/** Waits for a new page, ready again, with `pane` back in its layout. */
async function reloaded(what: string, pane: string): Promise<void> {
  await browser.waitUntil(
    async () => {
      try {
        return await browser.execute(() => {
          const w = window as unknown as { __beforeStop?: boolean; __tandemTest?: unknown };
          return !w.__beforeStop && w.__tandemTest !== undefined;
        });
      } catch {
        return false;
      }
    },
    { timeout: 30_000, timeoutMsg: `the window never came back (${what})` },
  );
  // The page is up before its layout has loaded.
  await browser.waitUntil(async () => (await paneIds()).join() === pane, {
    timeout: 20_000,
    timeoutMsg: `the pane never came back (${what})`,
  });
}

// E2E-M18-SH-07 The window's own crash (F-SH-07.8). A shell pane runs a
// command while the window's web content stops: first through the path a
// crash runs (`test_web_ended`), which keeps the WebDriver session, so the
// reloaded page is read, and the third stop within 5 minutes asks; then a
// real kill of the web process. WebKitWebDriver ends its session when the
// page's process dies, so that one comes last and is read from outside the
// page: the app's log and a file the shell writes. Linux only (WebKitGTK).
describe("E2E-M18-SH-07 the window reloads when its web content crashes", function () {
  this.timeout(240_000);
  let pane = "";
  let shell = "";
  let appPid = 0;

  before(async function () {
    if (process.platform !== "linux") this.skip();
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-sh07-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    pane = (await paneIds())[0] ?? "";
    await sendToPane(pane, "echo PID-$$\r");
    await browser.waitUntil(async () => (await pids(pane)).length === 1, { timeout: 15_000 });
    shell = (await pids(pane))[0] ?? "";
  });

  after(() => {
    // The killed page's session is gone and its app still runs.
    if (appPid) process.kill(appPid, "SIGTERM");
  });

  it("F-SH-07.8 a stop reloads the window: the pane keeps its shell and shows what it printed meanwhile; the third within 5 minutes asks", async () => {
    // It prints while the window reloads.
    await sendToPane(pane, "sleep 1; echo AFTER-RELOAD-$((6*7))\r");
    await mark();
    await invoke("test_web_ended", {});
    await reloaded("the first stop", pane);
    expect(lastRecovery()).toBe("Reload");
    await waitForPaneText(pane, "AFTER-RELOAD-42");
    await sendToPane(pane, "echo PID-$$\r");
    await browser.waitUntil(async () => (await pids(pane)).length === 2, { timeout: 15_000 });
    expect((await pids(pane))[1]).toBe(shell);

    // The second reloads by itself too; the third asks, and Reload reloads.
    await mark();
    await invoke("test_web_ended", {});
    await reloaded("the second stop", pane);
    answerConfirm("yes");
    await mark();
    await invoke("test_web_ended", {});
    await reloaded("Reload, asked after the third stop", pane);
    expect(lastRecovery()).toBe("Ask");
  });

  it("F-SH-07.8 a killed web process reloads the window by itself, and the pane's shell carries on", async () => {
    const out = join(home(), "after-crash.txt");
    await sendToPane(pane, `sleep 2; echo $$ > ${out}\r`);
    const [ended, readyBefore] = [count("window.web_ended"), count("app.ready")];
    const [n, pid] = await invoke<[number, number]>("test_web_process_kill", {});
    appPid = pid;
    expect(n).toBeGreaterThan(0);
    await waitFor(
      () => count("window.web_ended") > ended && lastRecovery() === "Reload",
      (done) => done,
      "the window's web content never stopped",
      { timeout: 15_000 },
    );
    await waitFor(
      () => count("app.ready") > readyBefore,
      (done) => done,
      "the window never came back",
      { timeout: 30_000 },
    );
    await waitFor(
      () => (existsSync(out) ? readFileSync(out, "utf8").trim() : ""),
      (written) => written === shell,
      "the pane's shell didn't carry on",
      { timeout: 15_000 },
    );
  });
});
