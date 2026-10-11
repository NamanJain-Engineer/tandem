import { $, $$, browser } from "@wdio/globals";
import { existsSync, readFileSync, rmSync } from "node:fs";
import {
  addFolder,
  freshOffline,
  paneIds,
  paneText,
  sendToPane,
  waitForPaneText,
  waitForShell,
} from "../support/app";

// E2E-M15-7 Parking (§5.1 memory, D-139): a pane hidden for a while gives
// its xterm to the core and gets it back, with the same screen, when shown.
type Hook = { __tandemTest: { parkedPanes: () => string[]; parkAfter: (ms: number | null) => void } };
const parked = async () =>
  (await browser.execute(() => (window as unknown as Hook).__tandemTest.parkedPanes())) as string[];

describe("E2E-M15-7 parked panes", () => {
  let pane = "";

  before(async () => {
    await freshOffline();
    await browser.execute(() => (window as unknown as Hook).__tandemTest.parkAfter(500));
    await addFolder(`/tmp/tandem-e2e-m15-7-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    pane = (await paneIds())[0] ?? "";
    await waitForShell(pane);
  });

  after(async () => {
    await browser.execute(() => (window as unknown as Hook).__tandemTest.parkAfter(null));
  });

  const hide = async () => {
    await $('button[aria-label="New tab"]').click();
    await browser.waitUntil(async () => !(await paneIds()).includes(pane), {
      timeout: 10_000,
      timeoutMsg: "the pane is still visible",
    });
    await browser.waitUntil(async () => (await parked()).includes(pane), {
      timeout: 10_000,
      timeoutMsg: "the hidden pane never parked",
    });
  };
  const show = async () => {
    await $$('[data-testid="tab"]')[0]?.click();
    await browser.waitUntil(
      async () => (await paneIds()).includes(pane) && !(await parked()).includes(pane),
      {
        timeout: 10_000,
        timeoutMsg: "the pane never came back",
      },
    );
  };

  it("§5.1 a parked shell keeps its scrollback and the output it made while parked", async () => {
    await sendToPane(pane, "for i in $(seq 1 40); do echo before-$i; done\n");
    await waitForPaneText(pane, "before-40");
    await hide();
    // The program runs on with no view; its output lands in the ring.
    await sendToPane(pane, "echo while-$((6 * 7))\n");
    await browser.pause(500);
    await show();
    await waitForPaneText(pane, "while-42");
    expect(await paneText(pane)).toContain("before-40");
    // Still a live shell.
    await sendToPane(pane, "echo after-$((5 + 5))\n");
    await waitForPaneText(pane, "after-10");
  });

  it("§5.1 a full-screen program comes back on its alternate screen and leaves it cleanly", async () => {
    await sendToPane(
      pane,
      "printf '\\033[?1049h\\033[2J\\033[5;3Halt-screen-up'; read -r _; printf '\\033[?1049l'; echo left-alt\n",
    );
    await waitForPaneText(pane, "alt-screen-up");
    await hide();
    await show();
    await waitForPaneText(pane, "alt-screen-up");
    // Only the alternate screen shows; the normal one waits under it.
    expect(await paneText(pane)).not.toContain("after-10");
    await sendToPane(pane, "\n");
    await waitForPaneText(pane, "left-alt");
    expect(await paneText(pane)).toContain("after-10");
  });

  it("F-CD-03.5 a parked pane's ESC[c is still answered within 100 ms", async () => {
    await hide();
    const out = `/tmp/tandem-e2e-park-da-${Date.now()}`;
    // The reply, ESC [ ? 6 2 ; 2 2 c, is 9 bytes.
    await sendToPane(
      pane,
      `stty raw -echo; t0=$(date +%s%N); printf '\\033[c'; r=$(dd bs=1 count=9 2>/dev/null | od -An -c | tr -d ' \\n'); t1=$(date +%s%N); stty sane; echo "$(( (t1 - t0) / 1000000 )) $r" > ${out}\n`,
    );
    await browser.waitUntil(() => existsSync(out), { timeout: 10_000, timeoutMsg: "no reply recorded" });
    const [ms, reply] = readFileSync(out, "utf8").trim().split(" ");
    rmSync(out);
    expect(reply).toContain("?62;22c");
    expect(Number(ms)).toBeLessThan(100);
    await show();
  });
});
