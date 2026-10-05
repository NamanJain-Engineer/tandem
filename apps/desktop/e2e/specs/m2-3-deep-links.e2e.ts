import { mkdirSync } from "node:fs";
import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, relaunch, textOf, workspaceRoots } from "../support/app";

// E2E-M2-3 Deep links (MILESTONES M2, F-SH-13 AC1–2).
describe("E2E-M2-3 deep links", () => {
  const target = `/tmp/tandem-e2e-link-${Date.now()}`;

  it("tandem-dev://open?path= launches the app and adds the workspace", async () => {
    await freshOffline();
    mkdirSync(target, { recursive: true });
    await relaunch([`tandem-dev://open?path=${encodeURIComponent(target)}`]);
    await browser.waitUntil(async () => (await workspaceRoots()).includes(target), { timeout: 15_000, timeoutMsg: `roots: ${await workspaceRoots()}` });
  });

  for (const [name, link] of [
    ["traversal", "tandem-dev://../../etc"],
    ["5 KB link", `tandem-dev://open?path=/${"a".repeat(5 * 1024)}`],
  ] as const) {
    it(`rejects a ${name} with "Unrecognized link" and no state change`, async () => {
      const before = await invoke<Array<{ id: string }>>("workspace_list");
      await relaunch([link]);
      await browser.waitUntil(async () => (await textOf('[role="region"][aria-label="Notifications"]')).includes("Unrecognized link"), { timeout: 15_000 });
      const after = await invoke<Array<{ id: string }>>("workspace_list");
      expect(after.map((w) => w.id)).toEqual(before.map((w) => w.id));
      await $('header[role="banner"]').waitForExist();
    });
  }
});
