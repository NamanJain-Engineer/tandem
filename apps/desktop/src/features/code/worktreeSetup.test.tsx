import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { IpcError, LaneSetup, OverlapReport, SetupState, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { patternLines, prepareWorktree, teardownText, useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { WorktreeDialogs, WorktreeRow, worktreeMenu } from "./Worktrees";

function ws(id: string, over: Partial<WorkspaceSummary> = {}): WorkspaceSummary {
  const tab: Tab = { id: `tb_${id}`, workspaceId: id, name: "Tab 1", sortKey: "a0", createdAt: 0 };
  return {
    id,
    kind: "local",
    hostId: "local",
    root: `/code/${id}`,
    wslDistro: null,
    name: id,
    parentId: null,
    laneBranch: null,
    laneBase: null,
    sortKey: "a0",
    pinned: false,
    collapsed: false,
    sessionsCollapsed: false,
    createdAt: 0,
    missing: false,
    branch: null,
    liveCount: 0,
    tabs: [tab],
    ...over,
  };
}

const REPO = ws("shop", { branch: "main" });
const WT = ws("wt_1", {
  parentId: "shop",
  laneBranch: "tandem/fix-auth",
  laneBase: "abc",
  root: "/code/shop/.tandem/worktrees/fix-auth",
  name: "fix-auth",
});

const state = (over: Partial<SetupState> = {}): SetupState => ({
  file: "/code/shop/.vibezeo/worktree.json",
  exists: true,
  config: { copy: [".env*"], setup: "pnpm install", teardown: null },
  error: null,
  digest: "d1",
  trusted: false,
  ...over,
});

const pane = { id: "pn_setup", tabId: "tb_wt_1", kind: "terminal", cwd: WT.root, engineId: null };

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function backend(handlers: Parameters<typeof fakeBackend>[0] = {}) {
  calls = fakeBackend({
    workspace_list: () => [REPO, WT],
    layout_get: () => [],
    lane_setup_run: () => pane,
    ...handlers,
  });
}

function dialogs() {
  return render(
    <UiProvider>
      <WorktreeDialogs />
    </UiProvider>,
  );
}

beforeEach(() => {
  resetStores();
  useWorktrees.setState({ ask: null, notices: [] });
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
  useWorkspaces.setState({ list: [REPO, WT], activeId: "shop", activeTab: {} });
});
afterEach(() => setInvokeBackend(null));

describe("worktree setup (F-WT-01)", () => {
  it("F-WT-01.4 the setup dialog saves patterns and commands for the repository", async () => {
    backend({ worktree_setup_get: () => state({ exists: false, config: null, digest: null }) });
    useWorktrees.getState().open({ kind: "setup", workspaceId: "shop" });
    dialogs();
    await screen.findByTestId("worktree-setup-empty");
    fireEvent.change(screen.getByTestId("worktree-setup-copy"), {
      target: { value: ".env*\n\n  config/local.yml \n" },
    });
    fireEvent.change(screen.getByTestId("worktree-setup-command"), { target: { value: "pnpm install" } });
    fireEvent.click(screen.getByTestId("worktree-setup-save"));
    await waitFor(() => expect(useWorktrees.getState().ask).toBeNull());
    expect(named("worktree_setup_save")[0]?.args).toEqual({
      workspaceId: "shop",
      config: { copy: [".env*", "config/local.yml"], setup: "pnpm install", teardown: null },
    });
    expect(screen.getByText("Saved the worktree setup")).toBeTruthy();
  });

  it("F-WT-01.4 the dialog shows a saved file's values, and a read failure offers Retry", async () => {
    let fail = true;
    backend({
      worktree_setup_get: () => {
        if (fail) {
          fail = false;
          const err: IpcError = { code: "io", message: "disk", stage: null, hint: null, retryable: true };
          throw err;
        }
        return state({ config: { copy: [".env", "secrets/"], setup: "make deps", teardown: "make down" } });
      },
    });
    useWorktrees.getState().open({ kind: "setup", workspaceId: "wt_1" });
    dialogs();
    expect(await screen.findByText("Couldn't read the worktree setup: disk")).toBeTruthy();
    fireEvent.click(screen.getByText("Retry"));
    await waitFor(() =>
      expect((screen.getByTestId("worktree-setup-copy") as HTMLTextAreaElement).value).toBe(".env\nsecrets/"),
    );
    expect((screen.getByTestId("worktree-setup-teardown") as HTMLInputElement).value).toBe("make down");
  });

  it("F-WT-01.6 a malformed file is explained, and saving replaces it", async () => {
    backend({
      worktree_setup_get: () =>
        state({ config: null, digest: null, error: ".vibezeo/worktree.json isn't valid: line 1, column 3" }),
    });
    useWorktrees.getState().open({ kind: "setup", workspaceId: "shop" });
    dialogs();
    expect(await screen.findByText(/Saving replaces it/)).toBeTruthy();
  });

  it("F-WT-01.3 an untrusted setup asks first; Trust and run trusts the reviewed digest, then runs it", async () => {
    backend({
      lane_setup: (): LaneSetup => ({ state: state(), copy: null, copyError: null }),
      worktree_setup_trust: () => state({ trusted: true }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    expect(named("lane_setup_run")).toHaveLength(0);
    await screen.findByTestId("worktree-review");
    expect(screen.getByText("pnpm install")).toBeTruthy();
    fireEvent.click(screen.getByTestId("worktree-review-trust"));
    await waitFor(() => expect(useWorktrees.getState().ask).toBeNull());
    expect(named("worktree_setup_trust")[0]?.args).toEqual({ workspaceId: "wt_1", digest: "d1" });
    expect(named("lane_setup_run")[0]?.args).toEqual({ laneId: "wt_1", tabId: "tb_wt_1" });
    expect(useLayout.getState().tabs.tb_wt_1?.panes.pn_setup).toBeTruthy();
  });

  it("F-WT-01.3 Not now runs nothing, and several new worktrees share one review", async () => {
    const WT2 = ws("wt_2", { ...WT, id: "wt_2", laneBranch: "tandem/fix-auth-2" });
    useWorkspaces.setState({ list: [REPO, WT, WT2] });
    backend({ lane_setup: (): LaneSetup => ({ state: state(), copy: null, copyError: null }) });
    dialogs();
    await prepareWorktree("wt_1");
    await prepareWorktree("wt_2");
    const ask = useWorktrees.getState().ask;
    expect(ask?.kind === "review" && ask.worktreeIds).toEqual(["wt_1", "wt_2"]);
    expect(await screen.findByText(/in 2 new worktrees/)).toBeTruthy();
    fireEvent.click(screen.getByTestId("worktree-review-later"));
    await waitFor(() => expect(useWorktrees.getState().ask).toBeNull());
    expect(named("worktree_setup_trust")).toHaveLength(0);
    expect(named("lane_setup_run")).toHaveLength(0);
  });

  it("F-WT-01.4 a trusted setup runs without asking", async () => {
    backend({ lane_setup: (): LaneSetup => ({ state: state({ trusted: true }), copy: null, copyError: null }) });
    await prepareWorktree("wt_1");
    expect(useWorktrees.getState().ask).toBeNull();
    expect(named("lane_setup_run")).toHaveLength(1);
  });

  it("F-WT-01.1 the copy is reported, and a copy-only setup runs nothing", async () => {
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({ config: { copy: [".env*"], setup: null, teardown: null }, digest: null }),
        copy: { copied: [".env", ".env.local", "config/local.yml"], skipped: [], bytes: 30, partial: false },
        copyError: null,
      }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    expect(await screen.findByText("Copied 3 files into the worktree")).toBeTruthy();
    expect(useWorktrees.getState().ask).toBeNull();
    expect(named("lane_setup_run")).toHaveLength(0);
  });

  it("F-WT-01.6 a malformed file never runs anything and says why", async () => {
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({ config: null, digest: null, error: "bad json" }),
        copy: null,
        copyError: null,
      }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    expect(await screen.findByText("The worktree setup can't be used: bad json")).toBeTruthy();
    expect(named("lane_setup_run")).toHaveLength(0);
  });

  it("F-WT-01.5 the teardown's outcome reads plainly", () => {
    expect(teardownText(null)).toBeUndefined();
    expect(teardownText({ kind: "ran", code: 0 })).toBe("Teardown finished");
    expect(teardownText({ kind: "ran", code: 3 })).toBe("Teardown exited with code 3");
    expect(teardownText({ kind: "timed_out" })).toBe("Teardown was stopped after 2 minutes");
    expect(teardownText({ kind: "untrusted" })).toMatch(/isn't trusted/);
    expect(teardownText({ kind: "failed", message: "no sh" })).toBe("Teardown couldn't start: no sh");
    expect(patternLines(" .env \n\n#x\n")).toEqual([".env", "#x"]);
  });
});

describe("overlap check (F-WT-03)", () => {
  const report = (over: Partial<OverlapReport> = {}): OverlapReport => ({
    files: 2,
    siblings: [{ laneId: "wt_2", branch: "tandem/fix-auth-2", files: ["src/a.ts"] }],
    forecast: { kind: "conflicts", files: ["README.md"] },
    uncommitted: 1,
    ...over,
  });

  it("F-WT-03.1 lists the sibling worktrees that changed the same files", async () => {
    backend({ lane_overlap: () => report() });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    const sibling = await screen.findByTestId("overlap-sibling");
    expect(sibling.getAttribute("data-branch")).toBe("tandem/fix-auth-2");
    expect(sibling.textContent).toContain("tandem/fix-auth-2 also changed 1 file");
    expect(sibling.textContent).toContain("src/a.ts");
    expect(named("lane_overlap")[0]?.args).toEqual({ laneId: "wt_1" });
  });

  it("F-WT-03.2 the merge confirm says it will conflict, and names the file, before anything merges", async () => {
    backend({ lane_overlap: () => report({ siblings: [] }) });
    useWorktrees.getState().open({ kind: "merge", worktreeId: "wt_1" });
    dialogs();
    const verdict = await screen.findByTestId("merge-forecast");
    expect(verdict.getAttribute("data-verdict")).toBe("conflicts");
    expect(verdict.textContent).toContain("Merging will conflict in 1 file");
    expect(verdict.textContent).toContain("README.md");
    expect(verdict.textContent).toContain("1 uncommitted change isn't in the forecast");
    expect(named("lane_merge")).toHaveLength(0);
  });

  it("F-WT-03.3 no shared files and a clean forecast read as such", async () => {
    backend({
      lane_overlap: () => report({ siblings: [], forecast: { kind: "clean" }, uncommitted: 0 }),
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    expect(await screen.findByText("No overlap with other worktrees")).toBeTruthy();
    expect(screen.getByText("No conflicts expected")).toBeTruthy();
  });

  it("F-WT-03.2 an old git says why there's no forecast, and a failure offers Retry", async () => {
    let fail = true;
    backend({
      lane_overlap: () => {
        if (fail) {
          fail = false;
          const err: IpcError = { code: "io", message: "boom", stage: null, hint: null, retryable: true };
          throw err;
        }
        return report({ forecast: { kind: "unavailable", reason: "The forecast needs git 2.38 or later" } });
      },
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    expect(await screen.findByText("Couldn't check overlap: boom")).toBeTruthy();
    fireEvent.click(screen.getByText("Retry"));
    expect(await screen.findByText("No forecast: The forecast needs git 2.38 or later")).toBeTruthy();
  });
});

describe("GitLab merge requests (F-WT-04)", () => {
  async function menuLabels(): Promise<string[]> {
    useWorktrees.setState({ forges: {} });
    render(
      <UiProvider>
        <WorktreeRow wt={WT} base="main" />
      </UiProvider>,
    );
    await waitFor(() => expect(named("lane_forge")).toHaveLength(1));
    await waitFor(() => expect(useWorktrees.getState().forges.wt_1).toBeTruthy());
    return worktreeMenu(WT, "main", () => 0).flatMap((e) => ("label" in e && e.label ? [e.label] : []));
  }

  it("F-WT-04.1 a GitLab origin makes the menu say Create merge request", async () => {
    backend({ lane_forge: () => "gitlab" });
    const labels = await menuLabels();
    expect(labels).toContain("Create merge request");
    expect(labels).not.toContain("Create pull request");
  });

  it("F-WT-04.2 a GitHub origin keeps Create pull request", async () => {
    backend({ lane_forge: () => "github" });
    const labels = await menuLabels();
    expect(labels).toContain("Create pull request");
    expect(labels).not.toContain("Create merge request");
  });
});
