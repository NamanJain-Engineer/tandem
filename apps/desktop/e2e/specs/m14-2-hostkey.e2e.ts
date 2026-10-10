import { browser } from "@wdio/globals";
import { freshOffline, settled } from "../support/app";
import {
  hostFingerprint,
  hostKeyShown,
  hostRows,
  openRemotePage,
  regenerateHostKeys,
  waitForState,
  approveHostKey,
} from "../support/remote";

// E2E-M14-2 Host key (F-RM-01): the container gets a new host key and the
// app reconnects. Expected: refused, with the warning ("Host key changed
// … won't connect until you verify it"). Then "Trust new key…" (a danger
// confirm) forgets the old key and the new fingerprint is shown for
// approval before connecting again.

describe("E2E-M14-2 host key", () => {
  before(async function () {
    this.timeout(120_000);
    await freshOffline();
    await openRemotePage();
    // E2E-M14-1 added the host and trusted its key.
    await browser.waitUntil(async () => (await hostRows()).length === 1, {
      timeout: 10_000,
      timeoutMsg: "no fixture host",
    });
    if ((await hostRows())[0]?.state === "connected") {
      await $("button=Disconnect").click();
      await waitForState("down");
    }
  });

  it("F-RM-01.2 a changed host key refuses the connection with a warning", async function () {
    this.timeout(120_000);
    const before = hostFingerprint();
    regenerateHostKeys();
    expect(hostFingerprint()).not.toBe(before);
    await $("button=Connect").click();
    await browser.waitUntil(async () => (await hostRows())[0]?.stage === "host_key", {
      timeout: 60_000,
      timeoutMsg: `not refused: ${JSON.stringify(await hostRows())}`,
    });
    expect((await hostRows())[0]?.state).toBe("down");
    expect(
      await browser.execute(() =>
        document.body.textContent?.includes("Host key changed for tandem@127.0.0.1:2222"),
      ),
    ).toBe(true);
    expect((await hostRows())[0]?.error).toContain("won't connect until you verify it");
    // Nothing was asked: a changed key is never offered for approval.
    expect(await $('[data-testid="remote-hostkey"]').isExisting()).toBe(false);
  });

  it("F-RM-01 Trust new key… shows the new fingerprint before connecting", async function () {
    this.timeout(180_000);
    await $("button=Trust new key…").click();
    await settled();
    await $("button=Forget the old key").click();
    expect(await hostKeyShown()).toBe(hostFingerprint());
    await approveHostKey();
    await waitForState("connected", 120_000);
  });
});
