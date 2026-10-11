import { browser } from "@wdio/globals";
import { freshOffline, invoke, waitForPaneText } from "../support/app";
import { PROJECT, ensureConnected, inContainer, openRemoteFolder, resetProject } from "../support/remote";

// E2E-M14-8 Worktrees on a host (F-RM-01, F-CD-15 there, D-1602): the host
// folder's branch reaches the rail once connected, so it offers worktrees;
// a worktree is made by the host's git under
// <repo>/.tandem/worktrees/<slug> on tandem/<slug>, its changes show against
// where it started, it merges into the main checkout, and a clean one is
// removed with its branch kept.

type Summary = {
  id: string;
  root: string;
  branch: string | null;
  parentId: string | null;
  laneBranch: string | null;
};
const asTandem = (cmd: string) => inContainer(`su tandem -c 'cd ${PROJECT} && ${cmd}'`);

describe("E2E-M14-8 worktrees on a host", () => {
  let project = "";

  before(async function () {
    this.timeout(180_000);
    resetProject();
    asTandem("rm -rf .tandem/worktrees; git worktree prune");
    await freshOffline();
    await ensureConnected();
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    // The host's status was read on connect: the rail knows the branch.
    await browser.waitUntil(
      async () =>
        (await invoke<Summary[]>("workspace_list")).some((w) => w.root === PROJECT && w.branch === "main"),
      { timeout: 30_000, timeoutMsg: "the remote folder's branch never reached workspace_list" },
    );
    project = (await invoke<Summary[]>("workspace_list")).find((w) => w.root === PROJECT)?.id ?? "";
  });

  after(() => {
    asTandem("git worktree remove --force .tandem/worktrees/fix-auth 2>/dev/null; git worktree prune");
    resetProject();
  });

  it("F-RM-01 F-CD-15 a worktree is made on the host, diffed, merged and removed", async function () {
    this.timeout(120_000);
    const lane = await invoke<Summary>("lane_create", { workspaceId: project, slugSource: "fix auth" });
    expect(lane.root).toBe(`${PROJECT}/.tandem/worktrees/fix-auth`);
    expect(lane.laneBranch).toBe("tandem/fix-auth");
    expect(lane.parentId).toBe(project);
    expect(asTandem("git -C .tandem/worktrees/fix-auth rev-parse --abbrev-ref HEAD").trim()).toBe(
      "tandem/fix-auth",
    );
    // .tandem/ stays out of the main checkout's status.
    expect(asTandem("git status --porcelain").trim()).toBe("");

    // Work in the worktree, on the host: committed and not.
    asTandem(
      "cd .tandem/worktrees/fix-auth && echo fixed >> README.md && git commit -qam fix && echo note > notes.txt",
    );
    const diffs = await invoke<Array<{ path: string; change: string }>>("lane_diff", { laneId: lane.id });
    expect(diffs.map((d) => `${d.change}:${d.path}`)).toEqual(["modified:README.md", "untracked:notes.txt"]);
    asTandem("rm .tandem/worktrees/fix-auth/notes.txt");

    // Merged into the main checkout by the host's git.
    const report = await invoke<{ merged: boolean; conflicts: string[]; commit: string | null }>(
      "lane_merge",
      {
        laneId: lane.id,
      },
    );
    expect(report.merged).toBe(true);
    expect(report.commit).not.toBeNull();
    expect(asTandem("cat README.md")).toContain("fixed");

    // A clean worktree goes; its branch stays.
    await invoke("lane_remove", { laneId: lane.id, force: false });
    expect(asTandem("test -e .tandem/worktrees/fix-auth && echo yes || echo no").trim()).toBe("no");
    expect(asTandem("git branch --list tandem/fix-auth")).toContain("tandem/fix-auth");
    expect((await invoke<Summary[]>("workspace_list")).some((w) => w.id === lane.id)).toBe(false);
  });

  it("F-RM-01 a dirty worktree isn't removed without the confirmation", async function () {
    this.timeout(60_000);
    const lane = await invoke<Summary>("lane_create", { workspaceId: project, slugSource: "dirty" });
    asTandem("echo wip > .tandem/worktrees/dirty/wip.txt");
    const err = await invoke("lane_remove", { laneId: lane.id, force: false }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(err).toContain("Uncommitted changes");
    expect(asTandem("cat .tandem/worktrees/dirty/wip.txt").trim()).toBe("wip");
    asTandem("rm .tandem/worktrees/dirty/wip.txt");
    await invoke("lane_remove", { laneId: lane.id, force: false });
  });
});
