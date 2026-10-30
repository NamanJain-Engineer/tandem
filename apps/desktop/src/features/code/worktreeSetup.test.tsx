import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  IpcError,
  LaneSetup,
  OverlapReport,
  PreviewState,
  SetupState,
  Tab,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import {
  patternLines,
  pollPreviews,
  prepareWorktree,
  startPreview,
  teardownText,
  useWorktrees,
} from "../../stores/worktrees";
import { useSidebar } from "../../stores/sidebar";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { waiting } from "../../test/waiting";
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
      config: {
        copy: [".env*", "config/local.yml"],
        setup: "pnpm install",
        teardown: null,
        ports: null,
        env: {},
      },
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
      lane_setup: (): LaneSetup => ({ state: state(), copy: null, copyError: null, runtime: null }),
      worktree_setup_trust: () => state({ trusted: true }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    expect(named("lane_setup_run")).toHaveLength(0);
    await screen.findByTestId("worktree-review");
    expect(screen.getByText("pnpm install")).toBeTruthy();
    // S5: the review opens on its own after Launch, Retry or New worktree:
    // Trust and run waits a moment, so a click aimed at the new panes
    // trusts nothing, and Tab still reaches it.
    const trust = screen.getByTestId("worktree-review-trust") as HTMLButtonElement;
    expect(trust.hasAttribute("data-grant")).toBe(true);
    expect(waiting(trust)).toBe(true);
    expect(trust.disabled).toBe(false);
    expect(trust.title).toBe("Trust and run turns on once this has been still on screen for a moment.");
    fireEvent.click(trust);
    expect(named("worktree_setup_trust")).toHaveLength(0);
    await waitFor(() => expect(waiting(trust)).toBe(false));
    fireEvent.click(screen.getByTestId("worktree-review-trust"));
    await waitFor(() => expect(useWorktrees.getState().ask).toBeNull());
    expect(named("worktree_setup_trust")[0]?.args).toEqual({ workspaceId: "wt_1", digest: "d1" });
    expect(named("lane_setup_run")[0]?.args).toEqual({ laneId: "wt_1", tabId: "tb_wt_1" });
    expect(useLayout.getState().tabs.tb_wt_1?.panes.pn_setup).toBeTruthy();
  });

  it("F-WT-01.3 Not now runs nothing, and several new worktrees share one review", async () => {
    const WT2 = ws("wt_2", { ...WT, id: "wt_2", laneBranch: "tandem/fix-auth-2" });
    useWorkspaces.setState({ list: [REPO, WT, WT2] });
    backend({
      lane_setup: (): LaneSetup => ({ state: state(), copy: null, copyError: null, runtime: null }),
    });
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
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({ trusted: true }),
        copy: null,
        copyError: null,
        runtime: null,
      }),
    });
    await prepareWorktree("wt_1");
    expect(useWorktrees.getState().ask).toBeNull();
    expect(named("lane_setup_run")).toHaveLength(1);
  });

  it("F-WT-01.1 the copy is reported, and a copy-only setup runs nothing", async () => {
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({ config: { copy: [".env*"], setup: null, teardown: null }, digest: null }),
        copy: {
          copied: [".env", ".env.local", "config/local.yml"],
          skipped: [],
          bytes: 30,
          partial: false,
          cloned: 0,
        },
        copyError: null,
        runtime: null,
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
        runtime: null,
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
    siblings: [{ laneId: "wt_2", branch: "tandem/fix-auth-2", files: ["src/a.ts"], symbols: [] }],
    forecast: { kind: "conflicts", files: ["README.md"] },
    uncommitted: 1,
    unparsed: 0,
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

  it("F-WT-10.6 F-WT-10.8 a rename reads as one; Claim these files claims on the click; a watched change reloads", async () => {
    let loads = 0;
    backend({
      lane_overlap: () => {
        loads += 1;
        return report({
          siblings: [
            {
              laneId: "wt_2",
              branch: "tandem/user",
              files: [],
              symbols: [
                { name: "parse", qualified: "parse", here: "renamed", there: "used", renamedTo: "parseLine" },
              ],
            },
          ],
        });
      },
      lane_radar_claim: () => ["src/parse.ts"],
      lane_radar: () => [],
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    const sym = await screen.findByTestId("overlap-symbol");
    expect(sym.textContent).toContain("renamed to parseLine here, used there");
    // Nothing is claimed without the click.
    expect(named("lane_radar_claim")).toHaveLength(0);
    fireEvent.click(screen.getByTestId("overlap-claim"));
    await waitFor(() =>
      expect(named("lane_radar_claim")[0]?.args).toEqual({ laneId: "wt_1", otherId: "wt_2" }),
    );
    expect(await screen.findByText("Claimed 1 file for this worktree")).toBeTruthy();
    // A watched change in another repository's worktree: nothing to read.
    useWorkspaces.setState({
      list: [REPO, WT, ws("wt_2", { ...WT, id: "wt_2", laneBranch: "tandem/user" })],
    });
    const before = loads;
    act(() => emitLocal("radar:changed", { laneIds: ["elsewhere"] }));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(loads).toBe(before);
    // A sibling's symbols changed: the check reads them again, keeping the
    // last report on screen meanwhile.
    act(() => emitLocal("radar:changed", { laneIds: ["wt_2"] }));
    expect(screen.queryByTestId("overlap-symbol")).not.toBeNull();
    await waitFor(() => expect(loads).toBe(before + 1));
  });

  it("F-WT-10.3 an older reload that answers last never replaces a newer report", async () => {
    // A watched change's reload on its way when the person asks again: the
    // first reload's reply came last and the dialog showed the older report
    // (the review of 9a4e93ab, S3).
    const replies: Array<(r: OverlapReport) => void> = [];
    const on = (branch: string) =>
      report({ siblings: [{ laneId: "wt_2", branch, files: ["src/a.ts"], symbols: [] }] });
    backend({
      lane_overlap: () => {
        if (named("lane_overlap").length === 1) {
          throw { code: "io", message: "disk busy", stage: null, hint: null, retryable: true };
        }
        return new Promise<OverlapReport>((resolve) => replies.push(resolve));
      },
      lane_radar: () => [],
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    expect(await screen.findByText("Couldn't check overlap: disk busy")).toBeTruthy();
    act(() => emitLocal("radar:changed", { laneIds: ["wt_1"] }));
    await waitFor(() => expect(replies).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(replies).toHaveLength(2));
    const branch = async () => (await screen.findByTestId("overlap-sibling")).getAttribute("data-branch");
    const [older, newer] = replies;
    await act(async () => newer?.(on("tandem/newer")));
    await waitFor(async () => expect(await branch()).toBe("tandem/newer"));
    await act(async () => older?.(on("tandem/older")));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(await branch()).toBe("tandem/newer");
  });

  it("F-WT-10.3 changes told while a check runs bring one more, not one each", async () => {
    // The check parses the siblings, each parse that changes what's shared
    // tells a change, and each change started a whole check again (the
    // review of d06b9e3d, SF-3).
    const replies: Array<(r: OverlapReport) => void> = [];
    const on = (branch: string) =>
      report({ siblings: [{ laneId: "wt_2", branch, files: ["src/a.ts"], symbols: [] }] });
    backend({
      lane_overlap: () => new Promise<OverlapReport>((resolve) => replies.push(resolve)),
      lane_radar: () => [],
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    await waitFor(() => expect(replies).toHaveLength(1));
    for (let n = 0; n < 3; n++) act(() => emitLocal("radar:changed", { laneIds: ["wt_1"] }));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(named("lane_overlap")).toHaveLength(1);
    await act(async () => replies[0]?.(on("tandem/first")));
    await waitFor(() => expect(named("lane_overlap")).toHaveLength(2));
    await act(async () => replies[1]?.(on("tandem/second")));
    const branch = async () => (await screen.findByTestId("overlap-sibling")).getAttribute("data-branch");
    await waitFor(async () => expect(await branch()).toBe("tandem/second"));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(named("lane_overlap")).toHaveLength(2);
    // Nothing on its way: a change reads again at once.
    act(() => emitLocal("radar:changed", { laneIds: ["wt_1"] }));
    await waitFor(() => expect(named("lane_overlap")).toHaveLength(3));
  });

  it("F-WT-10.8 a worktree that only uses what it shares has nothing to claim", async () => {
    backend({
      lane_overlap: () =>
        report({
          siblings: [
            {
              laneId: "wt_2",
              branch: "tandem/renamer",
              files: [],
              symbols: [
                { name: "parse", qualified: "parse", here: "used", there: "renamed", renamedTo: "parseLine" },
              ],
            },
          ],
        }),
      lane_radar: () => [],
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    const sym = await screen.findByTestId("overlap-symbol");
    expect(sym.textContent).toContain("used here, renamed to parseLine there");
    expect(screen.queryByTestId("overlap-claim")).toBeNull();
  });

  it("F-WT-10.8 a claim that fails says why", async () => {
    backend({
      lane_overlap: () =>
        report({
          siblings: [
            {
              laneId: "wt_2",
              branch: "tandem/user",
              files: [],
              symbols: [{ name: "parse", qualified: "parse", here: "changed", there: "used" }],
            },
          ],
        }),
      lane_radar_claim: () => {
        throw {
          code: "unavailable",
          message: "Claims aren't running, so nothing was claimed",
          stage: null,
          hint: null,
          retryable: false,
        };
      },
      lane_radar: () => [],
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    fireEvent.click(await screen.findByTestId("overlap-claim"));
    expect(
      await screen.findByText("Couldn't claim the files: Claims aren't running, so nothing was claimed"),
    ).toBeTruthy();
  });

  it("F-WT-10.1 lists the symbols a sibling shares, with what each side did, and the unparsed files", async () => {
    backend({
      lane_overlap: () =>
        report({
          unparsed: 2,
          siblings: [
            {
              laneId: "wt_2",
              branch: "tandem/caller",
              files: [],
              symbols: [{ name: "Session", qualified: "Session", here: "removed", there: "used" }],
            },
          ],
        }),
    });
    useWorktrees.getState().open({ kind: "overlap", worktreeId: "wt_1" });
    dialogs();
    const sibling = await screen.findByTestId("overlap-sibling");
    expect(sibling.textContent).toContain("tandem/caller touches the same symbols");
    const sym = screen.getByTestId("overlap-symbol");
    expect(sym.dataset.symbol).toBe("Session");
    expect(sym.textContent).toContain("renamed or removed here, used there");
    expect(screen.getByTestId("overlap-unparsed").textContent).toBe(
      "2 changed files weren't checked for symbols (other languages, too large, or unreadable).",
    );
  });

  it("F-WT-03.2 the merge confirm says it will conflict, and names the file, before anything merges", async () => {
    backend({ lane_overlap: () => report({ siblings: [] }) });
    useWorktrees.getState().open({ kind: "merge", worktreeId: "wt_1" });
    dialogs();
    const verdict = await screen.findByTestId("merge-forecast");
    expect(verdict.getAttribute("data-verdict")).toBe("conflicts");
    expect(verdict.textContent).toContain("Merging will conflict in 1 file");
    expect(verdict.textContent).toContain("README.md");
    expect(verdict.textContent).toContain(
      "1 uncommitted change isn't in the forecast. Commit it to include it.",
    );
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

describe("worktrees on SSH hosts (D-2209)", () => {
  const HOSTED = ws("wt_h", { ...WT, id: "wt_h", kind: "ssh", hostId: "hst_1" });

  it("F-WT-01 a host's worktree gets no setup prompt or error after it's made", async () => {
    backend({ lane_setup: () => null });
    useWorkspaces.setState({ list: [REPO, HOSTED] });
    await prepareWorktree("wt_h");
    expect(useWorktrees.getState().notices).toEqual([]);
    expect(useWorktrees.getState().ask).toBeNull();
  });

  it("F-WT-03 Check overlap and Worktree setup are off for a host's worktree, and its merge has no forecast", async () => {
    backend({});
    const entries = worktreeMenu(HOSTED, "main", () => 0);
    const off = (id: string) =>
      entries.find((e) => "id" in e && e.id === id && "disabled" in e && e.disabled);
    expect(off("wt-overlap")).toBeTruthy();
    expect(off("wt-setup")).toBeTruthy();
    useWorkspaces.setState({ list: [REPO, HOSTED] });
    useWorktrees.getState().open({ kind: "merge", worktreeId: "wt_h" });
    dialogs();
    await screen.findByText("Merge");
    expect(screen.queryByTestId("merge-forecast")).toBeNull();
    expect(named("lane_overlap")).toHaveLength(0);
  });
});

describe("worktree runtime (F-WT-06)", () => {
  it("F-WT-06.1 the dialog saves ports and environment with the rest", async () => {
    backend({
      worktree_setup_get: () =>
        state({ config: { copy: [], setup: null, teardown: null, ports: 10, env: { A: "1" } } }),
      worktree_env_check: () => [],
    });
    useWorktrees.getState().open({ kind: "setup", workspaceId: "shop" });
    dialogs();
    const portsInput = (await screen.findByTestId("worktree-setup-ports")) as HTMLInputElement;
    expect(portsInput.value).toBe("10");
    expect((screen.getByTestId("worktree-setup-env") as HTMLTextAreaElement).value).toBe("A=1");
    fireEvent.change(screen.getByTestId("worktree-setup-env"), {
      target: { value: "DATABASE_URL=postgres://localhost/app_{{slug}}\n\nPORT = {{port}}" },
    });
    fireEvent.click(screen.getByTestId("worktree-setup-save"));
    await waitFor(() => expect(named("worktree_setup_save")).toHaveLength(1));
    expect(named("worktree_setup_save")[0]?.args).toEqual({
      workspaceId: "shop",
      config: {
        copy: [],
        setup: null,
        teardown: null,
        ports: 10,
        env: { DATABASE_URL: "postgres://localhost/app_{{slug}}", PORT: "{{port}}" },
      },
    });
  });

  it("F-WT-09.1 saving the dialog keeps the file's preview, which it doesn't edit (D-861)", async () => {
    const preview = { command: "pnpm dev --port {{port}}", port: 0, path: "/" };
    backend({
      worktree_setup_get: () =>
        state({ config: { copy: [], setup: "pnpm install", teardown: null, ports: 10, env: {}, preview } }),
      worktree_env_check: () => [],
    });
    useWorktrees.getState().open({ kind: "setup", workspaceId: "shop" });
    dialogs();
    await screen.findByTestId("worktree-setup-ports");
    fireEvent.change(screen.getByTestId("worktree-setup-command"), { target: { value: "pnpm i" } });
    fireEvent.click(screen.getByTestId("worktree-setup-save"));
    await waitFor(() => expect(named("worktree_setup_save")).toHaveLength(1));
    expect(named("worktree_setup_save")[0]?.args).toEqual({
      workspaceId: "shop",
      config: { copy: [], setup: "pnpm i", teardown: null, ports: 10, env: {}, preview },
    });
  });

  it("F-WT-06.6 a secret-looking value is flagged as it's typed; a bad line or port count blocks Save", async () => {
    backend({
      worktree_setup_get: () => state({ exists: false, config: null, digest: null }),
      worktree_env_check: (a) =>
        Object.keys((a?.env as Record<string, string>) ?? {}).filter((k) => k.endsWith("_KEY")),
    });
    useWorktrees.getState().open({ kind: "setup", workspaceId: "shop" });
    dialogs();
    await screen.findByTestId("worktree-setup-env");
    fireEvent.change(screen.getByTestId("worktree-setup-env"), {
      target: { value: "STRIPE_KEY=sk_live_x\nDB=app" },
    });
    expect((await screen.findByTestId("worktree-setup-secret", {}, { timeout: 2000 })).textContent).toBe(
      "This looks like a secret. Don't commit it in worktree.json: STRIPE_KEY",
    );
    fireEvent.change(screen.getByTestId("worktree-setup-env"), { target: { value: "no equals sign" } });
    expect(await screen.findByText("Line 1 needs the form NAME=value.")).toBeTruthy();
    expect((screen.getByTestId("worktree-setup-save") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByTestId("worktree-setup-env"), { target: { value: "" } });
    fireEvent.change(screen.getByTestId("worktree-setup-ports"), { target: { value: "500" } });
    expect(screen.getByText("Use a number from 1 to 100, or leave it empty.")).toBeTruthy();
    expect((screen.getByTestId("worktree-setup-save") as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-WT-06.3 a block moved past busy ports says so after the worktree is made", async () => {
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({ config: { copy: [], setup: null, teardown: null, ports: 10 }, digest: null }),
        copy: null,
        copyError: null,
        runtime: { index: 1, ports: { start: 41020, end: 41029 }, movedFrom: { start: 41010, end: 41019 } },
      }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    expect(
      await screen.findByText("Ports 41010–41019 were busy; tandem/fix-auth uses 41020–41029."),
    ).toBeTruthy();
  });

  it("F-WT-06.2 an untrusted env-only file asks for review, and trusting it runs nothing", async () => {
    const envOnly = state({ config: { copy: [], setup: null, teardown: null, env: { DB: "app_{{slug}}" } } });
    backend({
      lane_setup: (): LaneSetup => ({ state: envOnly, copy: null, copyError: null, runtime: null }),
      worktree_setup_trust: () => ({ ...envOnly, trusted: true }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    const review = await screen.findByTestId("worktree-review");
    expect(review.textContent).toContain("DB=app_{{slug}}");
    await waitFor(() => expect(waiting(screen.getByTestId("worktree-review-trust"))).toBe(false));
    fireEvent.click(screen.getByTestId("worktree-review-trust"));
    await waitFor(() => expect(useWorktrees.getState().ask).toBeNull());
    expect(named("worktree_setup_trust")).toHaveLength(1);
    expect(named("lane_setup_run")).toHaveLength(0);
  });

  it("F-WT-06.1 the worktree's row tooltip names its port block", async () => {
    backend({ lane_runtime: () => ({ start: 41010, end: 41019 }), lane_forge: () => "github" });
    useWorktrees.setState({ runtimes: {}, forges: {} });
    render(
      <UiProvider>
        <WorktreeRow wt={WT} base="main" />
      </UiProvider>,
    );
    await waitFor(() => expect(useWorktrees.getState().runtimes.wt_1).toEqual({ start: 41010, end: 41019 }));
    expect(named("lane_runtime")[0]?.args).toEqual({ laneId: "wt_1" });
  });
});

describe("previews (F-WT-09)", () => {
  it("F-WT-09.1 a worktree whose preview answers shows Preview, which opens it in a browser tab", async () => {
    backend({
      lane_previews: (): PreviewState[] => [
        {
          laneId: "wt_1",
          status: "ready",
          url: "http://fix-auth.localhost:41000/",
          port: 41000,
          trusted: true,
        },
      ],
    });
    useWorktrees.setState({ previews: {} });
    await pollPreviews(["wt_1"]);
    render(
      <UiProvider>
        <WorktreeRow wt={WT} base="main" />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByTestId("worktree-preview"));
    expect(useSidebar.getState().tabs.find((x) => x.kind === "browser")?.url).toBe(
      "http://fix-auth.localhost:41000/",
    );
    // It goes when the port stops answering.
    useWorktrees.setState({
      previews: { wt_1: { laneId: "wt_1", status: "down", url: "u", port: 41000, trusted: true } },
    });
    await waitFor(() => expect(screen.queryByTestId("worktree-preview")).toBeNull());
  });

  it("F-WT-09.1 Start preview opens a Preview pane in the worktree", async () => {
    backend({ lane_preview_start: () => ({ ...pane, id: "pn_prev" }) });
    const out = await startPreview("wt_1");
    expect(out.ok).toBe(true);
    expect(named("lane_preview_start")[0]?.args).toEqual({ laneId: "wt_1", tabId: "tb_wt_1" });
    expect(useLayout.getState().tabs.tb_wt_1?.panes.pn_prev).toBeTruthy();
  });

  it("F-WT-09.2 an untrusted preview opens the review, and trusting it starts the preview, not the setup", async () => {
    const withPreview = state({
      config: {
        copy: [],
        setup: "pnpm install",
        teardown: null,
        ports: 2,
        preview: { command: "serve {{port}}", port: 0, path: "/" },
      },
    });
    let trusted = false;
    backend({
      lane_preview_start: () => {
        if (!trusted) {
          const err: IpcError = {
            code: "permission_denied",
            message: "Review this repository's worktree setup first",
            stage: "setup",
            hint: null,
            retryable: false,
          };
          throw err;
        }
        return { ...pane, id: "pn_prev" };
      },
      worktree_setup_get: () => withPreview,
      worktree_setup_trust: () => {
        trusted = true;
        return { ...withPreview, trusted: true };
      },
    });
    dialogs();
    await startPreview("wt_1");
    const review = await screen.findByTestId("worktree-review");
    expect(review.textContent).toContain("serve {{port}}");
    await waitFor(() => expect(waiting(screen.getByTestId("worktree-review-trust"))).toBe(false));
    fireEvent.click(screen.getByTestId("worktree-review-trust"));
    await waitFor(() => expect(named("lane_preview_start")).toHaveLength(2));
    expect(named("lane_setup_run")).toHaveLength(0);
  });

  it("F-WT-09.1 a new worktree with a trusted preview and no setup starts the preview", async () => {
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({
          trusted: true,
          config: {
            copy: [],
            setup: null,
            teardown: null,
            ports: 2,
            preview: { command: "serve", port: 0, path: "/" },
          },
        }),
        copy: null,
        copyError: null,
        runtime: null,
      }),
      lane_preview_start: () => ({ ...pane, id: "pn_prev" }),
    });
    await prepareWorktree("wt_1");
    expect(named("lane_preview_start")).toHaveLength(1);
    expect(named("lane_setup_run")).toHaveLength(0);
  });

  /** New worktree… in the shop repository, then what the new tab's grid
   * does once it shows: load the layout, and start a shell if it's empty. */
  async function newWorktreeShown() {
    dialogs();
    act(() => useWorktrees.getState().open({ kind: "new", workspaceId: "shop" }));
    fireEvent.change(await screen.findByTestId("worktree-name"), { target: { value: "fix-auth" } });
    fireEvent.click(screen.getByTestId("worktree-create"));
    await waitFor(() => expect(useWorkspaces.getState().activeId).toBe("wt_1"));
    await useLayout.getState().load("wt_1");
    await useLayout.getState().ensurePane("tb_wt_1");
  }

  it("F-WT-09.1 a new worktree's trusted preview is its tab's only pane, with no shell beside it (D-863)", async () => {
    backend({
      lane_create: () => WT,
      lane_setup: (): LaneSetup => ({
        state: state({
          trusted: true,
          config: {
            copy: [],
            setup: null,
            teardown: null,
            ports: 2,
            preview: { command: "serve", port: 0, path: "/" },
          },
        }),
        copy: null,
        copyError: null,
        runtime: null,
      }),
      // The Preview pane arrives after the tab has loaded empty, as in
      // E2E-M18-WT09-1's failure: a shell there took the last of Free's 4
      // live panes, and the preview never started.
      lane_preview_start: () => new Promise((r) => setTimeout(() => r({ ...pane, id: "pn_prev" }), 30)),
      pane_create: () => ({ ...pane, id: "pn_shell" }),
    });
    await newWorktreeShown();
    await waitFor(() => expect(useLayout.getState().tabs.tb_wt_1?.panes.pn_prev).toBeTruthy());
    expect(named("pane_create")).toHaveLength(0);
    expect(Object.keys(useLayout.getState().tabs.tb_wt_1?.panes ?? {})).toEqual(["pn_prev"]);
  });

  it("F-WT-09.2 a new worktree waiting for its review gets a shell meanwhile (D-863)", async () => {
    backend({
      lane_create: () => WT,
      lane_setup: (): LaneSetup => ({
        state: state({
          config: {
            copy: [],
            setup: null,
            teardown: null,
            ports: 2,
            preview: { command: "serve", port: 0, path: "/" },
          },
        }),
        copy: null,
        copyError: null,
        runtime: null,
      }),
      pane_create: () => ({ ...pane, id: "pn_shell" }),
    });
    await newWorktreeShown();
    expect(named("pane_create")).toHaveLength(1);
    expect(Object.keys(useLayout.getState().tabs.tb_wt_1?.panes ?? {})).toEqual(["pn_shell"]);
    await screen.findByTestId("worktree-review");
    expect(named("lane_preview_start")).toHaveLength(0);
  });
});

describe("copy-on-write copies (F-WT-09)", () => {
  it("F-WT-09.6 the copy toast says how many files were cloned", async () => {
    backend({
      lane_setup: (): LaneSetup => ({
        state: state({ config: { copy: ["node_modules/"], setup: null, teardown: null }, digest: null }),
        copy: {
          copied: ["node_modules/a.js", "node_modules/b.js"],
          skipped: [],
          bytes: 20,
          partial: false,
          cloned: 2,
        },
        copyError: null,
        runtime: null,
      }),
    });
    dialogs();
    await prepareWorktree("wt_1");
    expect(await screen.findByText("Copied 2 files into the worktree")).toBeTruthy();
    expect(
      screen.getByText("2 files were cloned, sharing their data with your checkout until either changes."),
    ).toBeTruthy();
  });
});
