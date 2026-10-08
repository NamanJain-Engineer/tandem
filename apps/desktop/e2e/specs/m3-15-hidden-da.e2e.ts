import { $, browser } from "@wdio/globals";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { addFolder, freshOffline, paneIds, sendToPane, waitForShell } from "../support/app";

// E2E-M3-15 Hidden DA (F-CD-03 AC5): a pane in a background tab still gets
// its terminal queries answered.
describe("E2E-M3-15 hidden DA", () => {
  it("F-CD-03.5 a hidden pane's ESC[c is answered within 100 ms", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-15-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const pane = (await paneIds())[0] ?? "";
    await waitForShell(pane);
    await $('button[aria-label="New tab"]').click();
    await browser.waitUntil(async () => !(await paneIds()).includes(pane), {
      timeout: 10_000,
      timeoutMsg: "the first pane is still visible",
    });
    const out = `/tmp/tandem-e2e-da-${Date.now()}`;
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
  });
});
