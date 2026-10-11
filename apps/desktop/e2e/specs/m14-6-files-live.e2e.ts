import { browser } from "@wdio/globals";
import { freshOffline, waitForPaneText } from "../support/app";
import { openFiles, showChanges, treeRows, waitForChanges, waitForRow } from "../support/files";
import { PROJECT, ensureConnected, inContainer, openRemoteFolder, resetProject } from "../support/remote";

// E2E-M14-6 Live remote files (F-RM-01, D-1600): files made, changed and
// staged on the host by something other than Vibezeo (here `docker exec`)
// show in the Files tab and the Changes view without Refresh or any click:
// the helper watches the folder and relays each batch.

const asHost = (cmd: string) => inContainer(`su tandem -c 'cd ${PROJECT} && ${cmd}'`);

describe("E2E-M14-6 live remote files", () => {
  before(async function () {
    this.timeout(180_000);
    resetProject();
    await freshOffline();
    await ensureConnected();
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    await openFiles();
    await waitForRow("README.md", 30_000);
  });

  it("F-RM-01 a file made on the host appears in the tree, and goes when it's deleted", async function () {
    this.timeout(60_000);
    asHost("echo from the host > made-on-host.txt");
    await waitForRow("made-on-host.txt", 15_000);
    asHost("rm made-on-host.txt");
    await browser.waitUntil(async () => !(await treeRows()).includes("made-on-host.txt"), {
      timeout: 15_000,
      timeoutMsg: `made-on-host.txt is still listed: ${JSON.stringify(await treeRows())}`,
    });
  });

  it("F-RM-01 the Changes view follows the host's own git: edit, then stage", async function () {
    this.timeout(60_000);
    await showChanges();
    asHost("echo more >> README.md");
    await waitForChanges(["U:README.md"]);
    // A git command on the host (not a file in the folder) changes the index.
    asHost("git add README.md");
    await waitForChanges(["S:README.md"]);
    asHost("git reset -q README.md && git checkout -q -- README.md");
    await waitForChanges([]);
  });
});
