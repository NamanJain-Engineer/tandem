import { browser } from "@wdio/globals";
import { freshOffline, paneText, shellPid, waitForPaneText } from "../support/app";
import { PROJECT, dropConnection, hostRows, openRemoteFolder, openRemotePage, waitForState } from "../support/remote";

// E2E-M14-4 Re-attach (F-RM-01 AC5): `ssh -O exit` drops the connection.
// The remote pane says "Reconnecting…", then the same remote shell (the
// same PID) is back and takes input.

describe("E2E-M14-4 re-attach", () => {
  let shell = "";

  before(async function () {
    this.timeout(180_000);
    await freshOffline();
    await openRemotePage();
    await browser.waitUntil(async () => (await hostRows()).length === 1, { timeout: 10_000, timeoutMsg: "no fixture host" });
    if ((await hostRows())[0]?.state !== "connected") {
      await $("button=Connect").click();
      await waitForState("connected", 120_000);
    }
    shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
  });

  it("F-RM-01.5 after ssh -O exit the same remote PTY re-attaches", async () => {
    const pid = await shellPid(shell);
    dropConnection();
    await waitForPaneText(shell, "Reconnecting…", 30_000);
    await waitForPaneText(shell, "Reconnected", 60_000);
    expect(await shellPid(shell)).toBe(pid);
    // Nothing printed before the drop shows twice.
    const text = await paneText(shell);
    expect(text.split("Reconnecting…").length).toBe(2);
  });
});
