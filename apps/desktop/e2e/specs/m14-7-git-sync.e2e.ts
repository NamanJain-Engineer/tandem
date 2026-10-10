import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, waitForPaneText } from "../support/app";
import {
  branchChip,
  branchMenu,
  commitStaged,
  openFiles,
  showChanges,
  waitForChanges,
} from "../support/files";
import { PROJECT, ensureConnected, inContainer, openRemoteFolder, resetProject } from "../support/remote";

// E2E-M14-7 Push, pull and branches on a host (F-RM-01, D-1601): the
// Changes view's branch menu runs the host's own git. Its `origin` is a
// bare repository on the host, so nothing of this computer's (no key, no
// credential) could take part: commit and push land there, a commit pushed
// from another clone on the host comes back with Pull, and a branch that
// exists only on the remote is switched to by its tracking branch.

const ORIGIN = "/home/tandem/origin.git";
const OTHER = "/home/tandem/other";
const asTandem = (cmd: string) => inContainer(`su tandem -c '${cmd}'`);
const inProject = (cmd: string) => asTandem(`cd ${PROJECT} && ${cmd}`);

describe("E2E-M14-7 push, pull and branches on a host", () => {
  before(async function () {
    this.timeout(180_000);
    resetProject();
    asTandem(`rm -rf ${ORIGIN} ${OTHER} && git init -q --bare -b main ${ORIGIN}`);
    inProject(
      `git checkout -q main && (git remote remove origin 2>/dev/null; true) && git remote add origin ${ORIGIN} && git push -q -u origin main`,
    );
    await freshOffline();
    await ensureConnected();
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    await openFiles();
    await showChanges();
    await browser.waitUntil(async () => (await branchChip())?.upstream === "origin/main", {
      timeout: 15_000,
      timeoutMsg: `the branch chip: ${JSON.stringify(await branchChip())}`,
    });
  });

  after(() => {
    // The commits made here go too: the next spec expects the fixture as made.
    resetProject();
    asTandem(`rm -rf ${ORIGIN} ${OTHER}`);
  });

  it("F-RM-01 a commit made in the Changes view is pushed by the host's git", async function () {
    this.timeout(90_000);
    inProject("echo pushed from vibezeo >> README.md");
    await waitForChanges(["U:README.md"]);
    await $('[data-testid="git-stage-all"]').click();
    await waitForChanges(["S:README.md"]);
    await commitStaged("from the panel");
    await browser.waitUntil(async () => (await branchChip())?.ahead === "1", { timeout: 15_000 });
    await branchMenu("Push");
    await browser.waitUntil(async () => (await branchChip())?.ahead === "0", {
      timeout: 60_000,
      timeoutMsg: `still ahead: ${JSON.stringify(await branchChip())}`,
    });
    expect(asTandem(`git --git-dir=${ORIGIN} log -1 --format=%s main`).trim()).toBe("from the panel");
  });

  it("F-RM-01 Pull brings a commit pushed from elsewhere", async function () {
    this.timeout(90_000);
    asTandem(
      `git clone -q ${ORIGIN} ${OTHER} && cd ${OTHER} && git config user.name Other && git config user.email o@example.com && echo elsewhere > from-other.txt && git add from-other.txt && git commit -qm other && git push -q origin HEAD:main HEAD:refs/heads/feature`,
    );
    inProject("git fetch -q");
    await browser.waitUntil(async () => (await branchChip())?.behind === "1", {
      timeout: 30_000,
      timeoutMsg: `not behind: ${JSON.stringify(await branchChip())}`,
    });
    await branchMenu("Pull");
    await browser.waitUntil(async () => (await branchChip())?.behind === "0", { timeout: 60_000 });
    expect(inProject("cat from-other.txt").trim()).toBe("elsewhere");
  });

  it("F-RM-01 a branch only on the remote is listed, and switching tracks it", async function () {
    this.timeout(90_000);
    // The same commands the branch menu's Switch branch submenu sends.
    const ws = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find(
      (w) => w.root === PROJECT,
    );
    const context = { type: "workspace", workspaceId: ws?.id ?? "" };
    const branches = await invoke<Array<{ name: string; remote: boolean; current: boolean }>>(
      "git_branches",
      {
        context,
      },
    );
    expect(branches).toContainEqual(expect.objectContaining({ name: "origin/feature", remote: true }));
    expect(branches).toContainEqual(expect.objectContaining({ name: "main", current: true }));
    await invoke("git_switch", { context, branch: "origin/feature" });
    await browser.waitUntil(async () => (await branchChip())?.branch === "feature", {
      timeout: 30_000,
      timeoutMsg: `the chip: ${JSON.stringify(await branchChip())}`,
    });
    expect(inProject("git rev-parse --abbrev-ref --symbolic-full-name @{u}").trim()).toBe("origin/feature");
  });
});
