import { $, browser } from "@wdio/globals";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  clickAllow,
  closeAllPanes,
  freshOffline,
  home,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitFor,
  waitForPaneText,
} from "../support/app";

// E2E-M18-TL12B `vibezeo run` for agents (F-TL-12 slice 2), with the fake
// keychain and the fake engine's `/sh`: the agent asks the app to run a
// command with the folder's secrets; the pane asks first, naming the command
// and the secret; the output comes back with the value masked, and the
// agent's own environment never has it.

const VALUE = "sk_test_e2e_agent_4242";

/** The style F-TL-12.20 holds the banner to 72 px with, while it runs. */
const SHORT_BANNER_ID = "e2e-tl12-short-banner";

/** Waits for `/sh` after `mark` to report its exit code. */
async function exitAfter(pane: string, mark: string): Promise<number> {
  let code = -2;
  await browser.waitUntil(
    async () => {
      const text = await paneText(pane);
      const at = text.lastIndexOf(mark);
      const m = at >= 0 ? /sh exit: (-?\d+)/.exec(text.slice(at)) : null;
      if (m) code = Number(m[1]);
      return m !== null;
    },
    { timeout: 20_000, timeoutMsg: `no exit code after ${mark}` },
  );
  return code;
}

async function textAfter(pane: string, mark: string): Promise<string> {
  const text = await paneText(pane);
  return text.slice(text.lastIndexOf(mark));
}

describe("E2E-M18-TL12B vibezeo run for agents", function () {
  this.timeout(180_000);
  const folder = `/tmp/tandem-e2e-m18-tl12b-${Date.now()}`;
  let agent = "";
  const banner = () => `[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-approval"]`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, ".env"), "API_KEY=vibezeo-secret://stripe-test\n");
    await freshOffline();
    await invoke("secrets_set", { name: "stripe-test", value: VALUE });
    await addFolder(folder);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
  });

  after(async () => {
    await closeAllPanes();
    for (const s of await invoke<Array<{ name: string }>>("secrets_list"))
      await invoke("secrets_delete", { name: s.name });
    rmSync(folder, { recursive: true, force: true });
  });

  it("F-TL-12.7 the pane asks first by name; Allow once runs it and the value comes back masked", async () => {
    await sendToPane(agent, "/sh echo R1; vibezeo run -- printenv API_KEY\r");
    await $(banner()).waitForDisplayed({ timeout: 15_000 });
    const summary = String(await $(`${banner()} [data-testid="run-line"]`).getProperty("textContent"));
    expect(summary).toContain("printenv API_KEY");
    expect(summary).toContain("API_KEY (stripe-test)");
    expect(summary).not.toContain(VALUE);
    await clickAllow(`${banner()} [data-testid="pane-approval-once"]`);
    expect(await exitAfter(agent, "R1")).toBe(0);
    expect(await textAfter(agent, "R1")).toContain("[REDACTED]");
    expect(await paneText(agent)).not.toContain(VALUE);
    await $(banner()).waitForExist({ reverse: true, timeout: 5_000 });
    // The agent's own environment never had it.
    await sendToPane(agent, '/sh echo R2; echo "own=${API_KEY:-none}"\r');
    expect(await exitAfter(agent, "R2")).toBe(0);
    expect(await textAfter(agent, "R2")).toContain("own=none");
  });

  it("F-TL-12.9 Deny runs nothing; Allow for this agent lets the same command run again without asking", async () => {
    await sendToPane(agent, "/sh echo R3; vibezeo run -- touch ran\r");
    await $(banner()).waitForDisplayed({ timeout: 15_000 });
    await $(`${banner()} [data-testid="pane-approval-deny"]`).click();
    expect(await exitAfter(agent, "R3")).toBe(1);
    expect(await textAfter(agent, "R3")).toContain("Not allowed: you declined");
    expect(existsSync(join(folder, "ran"))).toBe(false);

    await sendToPane(agent, "/sh echo R4; vibezeo run -- echo agent-ok\r");
    await $(banner()).waitForDisplayed({ timeout: 15_000 });
    await clickAllow(`${banner()} [data-testid="pane-approval-agent"]`);
    expect(await exitAfter(agent, "R4")).toBe(0);
    await sendToPane(agent, "/sh echo R5; vibezeo run -- echo agent-ok\r");
    expect(await exitAfter(agent, "R5")).toBe(0);
    expect(await textAfter(agent, "R5")).toContain("agent-ok");
    expect(await $(banner()).isExisting()).toBe(false);
  });

  it("F-TL-12.20 in a short banner Allow waits for the command's own last line, held on screen, not the lines below it", async () => {
    // The marker sat over the variables line, and a jump to the banner's
    // end turned Allow on with the command's tail never on screen (the
    // review of b3ff3696, M-1). A real webview lays it out: the banner is
    // held to 72 px, as in a 120 px pane, from its first frame. Shortened
    // after it showed, it had been on screen whole, the command's end with
    // it (the review of 8f35e5d0, S-2).
    const once = `${banner()} [data-testid="pane-approval-once"]`;
    await browser.execute((css: string) => {
      const style = document.createElement("style");
      style.id = SHORT_BANNER_ID;
      style.textContent = css;
      document.head.append(style);
    }, `[data-testid="pane-approval"]{max-height:72px!important}`);
    try {
      // 80 words: 4 lines or more at any width the E2E window gives a pane.
      const words = Array.from({ length: 80 }, (_, i) => `segment${i}`).join(" ");
      await sendToPane(agent, `/sh echo R7; vibezeo run -- echo ${words} TAIL-END\r`);
      await $(banner()).waitForDisplayed({ timeout: 15_000 });
      const shape = await browser.execute((sel: string) => {
        const b = document.querySelector<HTMLElement>(sel);
        const pre = b?.querySelector<HTMLElement>('[data-testid="run-line-command"]');
        if (!b || !pre) return { height: -1, lines: 0 };
        // The command's lines: its height less the padding and borders
        // above and below, in lines of its own height.
        const cs = getComputedStyle(pre);
        const inside =
          pre.getBoundingClientRect().height -
          parseFloat(cs.paddingTop) -
          parseFloat(cs.paddingBottom) -
          parseFloat(cs.borderTopWidth) -
          parseFloat(cs.borderBottomWidth);
        return {
          height: b.getBoundingClientRect().height,
          lines: Math.round(inside / parseFloat(cs.lineHeight)),
        };
      }, banner());
      expect(shape.height).toBeGreaterThan(0);
      expect(shape.height).toBeLessThanOrEqual(72);
      // With 3 lines or more, the last one is below the view at the top.
      expect(shape.lines).toBeGreaterThanOrEqual(3);
      // At the top: Allow is off, and says to scroll to the end.
      await browser.pause(900);
      expect(await $(once).getAttribute("aria-disabled")).toBe("true");
      expect(await $(once).getAttribute("title")).toBe("Scroll to the end of the command first.");
      // End, with the focus in the banner (on Allow once, without scrolling
      // to it), so the key scrolls the banner and not the terminal. The view
      // lands at the banner's end, past the command's last line: Allow stays
      // off and says to scroll up a little. Where scrolling is smooth, End
      // animates and the line passes through the view for a frame or two,
      // which doesn't count either (the review of 8f35e5d0, S-1).
      await browser.execute((sel: string) => {
        document.querySelector<HTMLElement>(sel)?.focus({ preventScroll: true });
      }, once);
      await browser.keys("End");
      await waitFor(
        () =>
          browser.execute((sel: string) => {
            const b = document.querySelector<HTMLElement>(sel);
            return b ? Math.round(b.scrollHeight - b.clientHeight - b.scrollTop) : -1;
          }, banner()),
        (left) => left >= 0 && left <= 1,
        "the banner's scroll left after End",
        { timeout: 5_000 },
      );
      await browser.pause(900);
      expect(await $(once).getAttribute("aria-disabled")).toBe("true");
      expect(await $(once).getAttribute("title")).toBe("Scroll up a little to see the end of the command.");
      // The command's last line wholly in view, held there past the dwell:
      // the end is seen. Allow then waits only for its own place, below the
      // view, which clickAllow brings into view and gives its half second.
      await browser.execute((sel: string) => {
        document.querySelector(`${sel} [data-testid="run-line-end"]`)?.scrollIntoView({ block: "center" });
      }, banner());
      await browser.pause(900);
      expect(String(await $(once).getAttribute("title"))).not.toContain("end of the command");
      await clickAllow(once);
      expect(await exitAfter(agent, "R7")).toBe(0);
      expect(await textAfter(agent, "R7")).toContain("TAIL-END");
    } finally {
      await browser.execute((id: string) => document.getElementById(id)?.remove(), SHORT_BANNER_ID);
    }
  });

  it("F-TL-12.10 outside an agent pane it says why and exits 1", async () => {
    const plain = (await paneIds()).find((p) => p !== agent) ?? "";
    const vibezeo = join(home(), "run", "bin", "vibezeo");
    await sendToPane(plain, `'${vibezeo}' run -- echo hi; echo "code=$?"\r`);
    await waitForPaneText(plain, "code=1");
    expect(await paneText(plain)).toContain("vibezeo run works in agent panes in Vibezeo");
  });
});
