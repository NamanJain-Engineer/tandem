import { browser } from "@wdio/globals";
import { freshOffline, invoke, paneText, sendToPane, waitForPaneText } from "../support/app";
import {
  PROJECT,
  addFixtureHost,
  hostFingerprint,
  hostKeyShown,
  hostRows,
  inContainer,
  openRemoteFolder,
  waitForState,
  approveHostKey,
  resetProject,
} from "../support/remote";

// E2E-M14-1 Connect (F-RM-01): add 127.0.0.1:2222 with the fixture's key,
// approve the fingerprint the dialog shows (it's the container's own), pick
// /home/tandem/project, and run `echo hi` in the remote shell. The remote
// spawn's environment holds no `.tandem/providers` path.

describe("E2E-M14-1 connect", () => {
  let shell = "";

  before(async function () {
    this.timeout(180_000);
    // A clean host: the helper is uploaded and checked again.
    inContainer("rm -rf /home/tandem/.tandem-host/1");
    resetProject();
    await freshOffline();
  });

  it("F-RM-01 an unknown host shows its fingerprint for approval, then connects", async function () {
    this.timeout(180_000);
    await addFixtureHost();
    const shown = await hostKeyShown();
    expect(shown).toMatch(/^SHA256:/);
    expect(shown).toBe(hostFingerprint());
    await approveHostKey();
    await waitForState("connected", 120_000);
    expect((await hostRows())[0]?.text).toBe("Connected");
    // The helper went to ~/.tandem-host/<protocol>/ and is this build's.
    expect(inContainer("ls /home/tandem/.tandem-host").split("\n")).toContain("1");
  });

  it("F-RM-01.1 a remote folder opens with a shell on the host: echo hi", async () => {
    shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    await sendToPane(shell, "echo h''i; pwd\n");
    await browser.waitUntil(async () => (await paneText(shell)).split("\n").some((l) => l.trim() === "hi"), {
      timeout: 15_000,
      timeoutMsg: `no hi: ${await paneText(shell)}`,
    });
    expect(await paneText(shell)).toContain(PROJECT);
    // The rail marks it remote.
    const tooltip = await browser.execute(
      (root: string) =>
        document.querySelector<HTMLElement>(
          `[data-testid="workspace-row"][data-root="${root}"] [aria-label^="Remote"]`,
        ) !== null,
      PROJECT,
    );
    expect(tooltip).toBe(true);
  });

  it("F-RM-01.4 the remote spawn env contains no local provider-home path", async () => {
    await sendToPane(shell, "clear; env | grep -c 'tandem/providers'; echo env-done-$((1+1))\n");
    await waitForPaneText(shell, "env-done-2");
    const lines = (await paneText(shell)).split("\n").map((l) => l.trim());
    expect(lines).toContain("0");
    // And what the app sent was only the allowlist.
    const log =
      await invoke<Array<{ cwd: string; envKeys: string[]; providerHome: string | null }>>("test_spawn_log");
    const spawn = log.find((s) => s.cwd === PROJECT);
    expect(spawn).toBeDefined();
    expect(
      spawn?.envKeys.filter(
        (k) => !["TERM", "COLORTERM", "TERM_PROGRAM", "TANDEM_PANE_ID", "LANG"].includes(k),
      ),
    ).toEqual([]);
    expect(spawn?.providerHome).toBeNull();
  });
});
