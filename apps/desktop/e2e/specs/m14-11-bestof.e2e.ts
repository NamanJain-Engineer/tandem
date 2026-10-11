import { browser } from "@wdio/globals";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { freshOffline, home, invoke, sendToPane, waitForPaneText } from "../support/app";
import {
  PROJECT,
  copyIn,
  ensureConnected,
  hostRows,
  inContainer,
  openRemoteFolder,
  resetProject,
} from "../support/remote";

// E2E-M14-11 Best of N on a host (F-RM-01, F-EX-04, D-1605): two (fake)
// claude seats in a folder on the fixture. Their worktrees are made by the
// host's git from one commit, Compare shows each lane's changes there, the
// check runs on the host in each worktree, and Keep this one commits the
// winner on the host, merges it into the main checkout and removes the
// other worktree; both branches stay. A dirty base refuses, Cancel changes
// nothing.

type Lane = {
  seat: number;
  laneId: string | null;
  branch: string | null;
  paneId: string | null;
  error: string | null;
  check: string;
  checkExit: number | null;
  checkOutput: string | null;
};
type Run = { id: string; state: string; base: string; lanes: Lane[] };
type Detail = { run: Run; diffs: Array<{ seat: number; files: Array<{ path: string }>; error: string | null }> };

const asTandem = (cmd: string) => inContainer(`su tandem -c 'cd ${PROJECT} && ${cmd}'`);
/** The native confirm answers from this file (TANDEM_FAKE_CONFIRM). */
const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

describe("E2E-M14-11 Best of N on a host", () => {
  let wsId = "";
  let run: Run = { id: "", state: "", base: "", lanes: [] };
  const rootOf = (l: Lane) => `${PROJECT}/.tandem/worktrees/${(l.branch ?? "").replace(/^tandem\//, "")}`;
  const get = () => invoke<Detail>("bestof_get", { runId: run.id });

  before(async function () {
    this.timeout(240_000);
    copyIn(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", "/usr/local/bin/claude");
    inContainer("chmod 755 /usr/local/bin/claude");
    resetProject();
    await freshOffline();
    await ensureConnected();
    await invoke("engines_detect", { hostId: (await hostRows())[0]?.id ?? "" });
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    wsId =
      (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === PROJECT)?.id ?? "";
  });

  after(() => resetProject());

  it("F-RM-01 F-EX-04.1 two seats get two worktrees on the host from one commit", async function () {
    this.timeout(120_000);
    run = await invoke<Run>("bestof_start", {
      workspaceId: wsId,
      prompt: "Fix login redirect",
      seats: [{ engineId: "claude" }, { engineId: "claude" }],
      checkCommand: "pwd; test -f done.txt",
    });
    expect(run.lanes.map((l) => l.error)).toEqual([null, null]);
    const branches = run.lanes.map((l) => l.branch ?? "");
    expect(new Set(branches).size).toBe(2);
    for (const b of branches) expect(b.startsWith("tandem/")).toBe(true);
    const head = asTandem("git rev-parse HEAD").trim();
    expect(run.base).toBe(head);
    for (const b of branches) expect(asTandem(`git rev-parse ${b}`).trim()).toBe(head);
    for (const l of run.lanes) expect(asTandem(`test -d ${rootOf(l)} && echo yes`).trim()).toBe("yes");
  });

  it("F-RM-01 F-EX-04.2 Compare shows each lane's changes on the host", async function () {
    this.timeout(60_000);
    const [one] = run.lanes;
    await sendToPane(one?.paneId ?? "", "/write done.txt=yes");
    await sendToPane(one?.paneId ?? "", "\r");
    await browser.waitUntil(async () => asTandem(`cat ${rootOf(one!)}/done.txt 2>/dev/null || true`).trim() === "yes", {
      timeout: 30_000,
      timeoutMsg: "the agent on the host didn't write done.txt",
    });
    const d = await get();
    expect(d.diffs.map((x) => x.error)).toEqual([null, null]);
    expect(d.diffs.find((x) => x.seat === 0)?.files.map((f) => f.path)).toEqual(["done.txt"]);
    expect(d.diffs.find((x) => x.seat === 1)?.files).toEqual([]);
  });

  it("F-RM-01 F-EX-04.3 the check runs on the host in each worktree", async function () {
    this.timeout(120_000);
    await invoke("bestof_check", { runId: run.id });
    await browser.waitUntil(async () => (await get()).run.lanes.every((l) => l.check !== "running"), {
      timeout: 90_000,
      timeoutMsg: "the checks never finished",
    });
    const lanes = (await get()).run.lanes;
    const [one, two] = [lanes.find((l) => l.seat === 0)!, lanes.find((l) => l.seat === 1)!];
    expect([one.check, one.checkExit]).toEqual(["passed", 0]);
    expect([two.check, two.checkExit]).toEqual(["failed", 1]);
    // `pwd` printed the worktree's folder on the host, which isn't on this machine.
    expect(one.checkOutput ?? "").toContain(rootOf(one));
    expect(two.checkOutput ?? "").toContain(rootOf(two));
  });

  it("F-RM-01 F-EX-04.4 Keep commits the winner on the host, merges it and removes the other", async function () {
    this.timeout(120_000);
    // A dirty base refuses before anything changes.
    asTandem("echo local >> README.md");
    const dirty = await invoke("bestof_keep", { runId: run.id, seat: 0 }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(dirty).toContain("Commit or stash your changes");
    asTandem("git checkout -q -- README.md");
    // Cancel changes nothing.
    answerConfirm("no");
    const cancelled = await invoke("bestof_keep", { runId: run.id, seat: 0 }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(cancelled).toContain('"code":"cancelled"');
    const [one, two] = run.lanes;
    for (const l of [one!, two!]) expect(asTandem(`test -d ${rootOf(l)} && echo yes`).trim()).toBe("yes");

    answerConfirm("yes");
    const report = await invoke<{ merged: boolean; conflicts: string[] }>("bestof_keep", { runId: run.id, seat: 0 });
    expect(report.merged).toBe(true);
    // The winner's uncommitted file was committed on its branch and merged.
    expect(asTandem("cat done.txt").trim()).toBe("yes");
    expect(asTandem(`git log -1 --format=%s ${one!.branch}`)).toContain("Best of N: ");
    expect(asTandem(`test -d ${rootOf(two!)} && echo yes || echo no`).trim()).toBe("no");
    for (const l of [one!, two!]) expect(asTandem(`git branch --list ${l.branch}`)).toContain(l.branch ?? "");
    expect((await get()).run.state).toBe("merged");
  });
});
