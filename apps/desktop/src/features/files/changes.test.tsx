import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type {
  Branch,
  FileStatus,
  FsChange,
  GitPanel,
  IpcError,
  StatusRecord,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useFiles } from "../../stores/files";
import { resetGit, useGit } from "../../stores/git";
import { useSidebar } from "../../stores/sidebar";
import { useStatus } from "../../stores/status";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { StatusBar } from "../shell/StatusBar";
import { FilesPanel } from "./FilesPanel";

const WS = {
  id: "ws_1",
  root: "/work/shop",
  name: "shop",
  tabs: [{ id: "tab_1" }],
  branch: "main",
} as unknown as WorkspaceSummary;

type Repo = {
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  staged: FileStatus[];
  unstaged: FileStatus[];
  noCommits: boolean;
};

const ipcError = (code: string, message: string): IpcError => ({
  code: code as IpcError["code"],
  message,
  stage: "git",
  hint: null,
  retryable: false,
});

/** A repository behind the git_* commands. `repo: null` is a plain folder. */
let remoteHost: string | null = null;

function setup(repo: Repo | null, opts: { commitFails?: string } = {}) {
  const channels: Array<{ onmessage: (c: FsChange) => void }> = [];
  const branches: Branch[] = [
    {
      name: repo?.branch ?? "main",
      current: true,
      remote: false,
      upstream: repo?.upstream ?? null,
      ahead: 0,
      behind: 0,
    },
    { name: "feature", current: false, remote: false, upstream: null, ahead: 0, behind: 0 },
  ];
  const move = (paths: string[], from: FileStatus[], to: FileStatus[]) => {
    for (const p of paths) {
      const at = from.findIndex((f) => f.path === p);
      if (at < 0) continue;
      const [f] = from.splice(at, 1);
      if (f) to.push({ ...f, change: f.change === "untracked" ? "added" : f.change });
    }
  };
  const calls = fakeBackend({
    workspace_list: () => [WS],
    editor_tabs_get: () => ({
      tabs: [],
      active: null,
      expanded: [],
      showIgnored: false,
      divider: null,
      view: null,
    }),
    editor_tabs_save: () => null,
    fs_watch: (a) => {
      channels.push(a?.channel as { onmessage: (c: FsChange) => void });
      return `fsw_${channels.length}`;
    },
    fs_unwatch: () => null,
    fs_list: () => [],
    git_status: (): GitPanel | null =>
      repo
        ? {
            root: "/work/shop",
            folder: "",
            status: { ...repo, staged: [...repo.staged], unstaged: [...repo.unstaged] },
            ...(remoteHost ? { host: remoteHost } : {}),
          }
        : null,
    git_diff: (a) => ({
      path: String(a?.path),
      change: "modified",
      additions: 1,
      deletions: 1,
      binary: false,
      truncated: false,
      patch: a?.staged ? "@@ -1 +1 @@\n-old staged\n+new staged\n" : "@@ -1 +1 @@\n-one\n+ONE\n",
    }),
    git_stage: (a) => {
      if (repo) move(a?.paths as string[], repo.unstaged, repo.staged);
      return null;
    },
    git_unstage: (a) => {
      if (repo) move(a?.paths as string[], repo.staged, repo.unstaged);
      return null;
    },
    git_discard: (a) => {
      if (repo) repo.unstaged = repo.unstaged.filter((f) => !(a?.paths as string[]).includes(f.path));
      return null;
    },
    git_commit: () => {
      if (opts.commitFails) throw ipcError("invalid", opts.commitFails);
      if (!repo) throw ipcError("not_found", "Not a git repository");
      repo.staged = [];
      repo.noCommits = false;
      repo.ahead += 1;
      return { sha: "a".repeat(40) };
    },
    git_push: (a) => {
      if (!repo) return null;
      if (a?.publish) repo.upstream = `origin/${repo.branch}`;
      repo.ahead = 0;
      return null;
    },
    git_pull: () => {
      if (repo) repo.behind = 0;
      return null;
    },
    git_branches: () => branches,
    git_switch: (a) => {
      if (repo) repo.branch = String(a?.branch);
      return null;
    },
    git_create_branch: (a) => {
      if (repo) {
        repo.branch = String(a?.name);
        repo.upstream = null;
      }
      return null;
    },
  });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  render(
    <UiProvider>
      <FilesPanel />
      <StatusBar />
    </UiProvider>,
  );
  const batch = () =>
    act(async () => {
      for (const ch of channels) ch.onmessage({ paths: ["a.txt"], git: false, overflow: false });
      await Promise.resolve();
    });
  return { calls, batch };
}

const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;
const changeRows = () =>
  screen.queryAllByTestId("git-change").map((r) => `${r.dataset.staged ? "S" : "U"}:${r.dataset.path}`);

async function showChanges() {
  fireEvent.click(await screen.findByRole("radio", { name: /^Changes/ }));
  await waitFor(() => expect(screen.getByTestId("files-panel").dataset.view).toBe("changes"));
}

function chip() {
  return screen.getByTestId("git-branch-chip");
}

async function chipMenu(item: RegExp) {
  fireEvent.keyDown(chip(), { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: item }));
}

const modified = (path: string): FileStatus => ({ path, change: "modified" });

describe("Changes view (F-CD-20)", () => {
  beforeEach(() => {
    resetStores();
    resetGit();
    remoteHost = null;
  });

  it("F-RM-01 a remote folder's branch chip says push and pull run on the host", async () => {
    remoteHost = "Build box";
    const repo: Repo = {
      branch: "main",
      upstream: "origin/main",
      ahead: 1,
      behind: 0,
      staged: [],
      unstaged: [modified("a.txt")],
      noCommits: false,
    };
    const { calls } = setup(repo);
    await showChanges();
    await waitFor(() => expect(chip().dataset.branch).toBe("main"));
    fireEvent.keyDown(chip(), { key: "Enter" });
    const item = await screen.findByRole("menuitem", {
      name: "Push, pull and branches: run git in a terminal on Build box",
    });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(screen.queryByRole("menuitem", { name: /^Push$/ })).toBeNull();
    expect(count(calls, "git_branches")).toBe(0);
  });

  it("F-CD-20.2 stage, commit and push: the branch chip comes back to 0/0", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: "origin/main",
      ahead: 0,
      behind: 0,
      staged: [],
      unstaged: [modified("a.txt"), modified("b.txt")],
      noCommits: false,
    };
    const { calls } = setup(repo);
    await showChanges();
    await waitFor(() => expect(changeRows()).toEqual(["U:a.txt", "U:b.txt"]));
    expect(screen.getByRole("radio", { name: "Changes (2)" })).toBeTruthy();
    fireEvent.click(screen.getByTestId("git-stage-all"));
    await waitFor(() => expect(changeRows()).toEqual(["S:a.txt", "S:b.txt"]));
    expect(calls.find((c) => c.cmd === "git_stage")?.args).toMatchObject({
      paths: ["a.txt", "b.txt"],
      context: { type: "workspace", workspaceId: "ws_1" },
    });
    const commit = screen.getByTestId("git-commit") as HTMLButtonElement;
    expect(commit.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Commit message"), {
      target: { value: "Fix the second line\n\nAnd add b." },
    });
    expect(commit.disabled).toBe(false);
    fireEvent.click(commit);
    await waitFor(() => expect(chip().dataset.ahead).toBe("1"));
    expect(calls.find((c) => c.cmd === "git_commit")?.args?.message).toBe(
      "Fix the second line\n\nAnd add b.",
    );
    expect((screen.getByLabelText("Commit message") as HTMLTextAreaElement).value).toBe("");
    expect(chip().textContent).toContain("↑1 ↓0");
    await chipMenu(/^Push$/);
    await waitFor(() => expect(chip().dataset.ahead).toBe("0"));
    expect(chip().dataset.behind).toBe("0");
    expect(chip().textContent).toContain("↑0 ↓0");
    expect(calls.find((c) => c.cmd === "git_push")?.args).toMatchObject({ publish: false });
    expect(screen.getByText("No changes")).toBeTruthy();
  });

  it("F-CD-20.3 a repository with no commits lists the new files and takes the first commit", async () => {
    const repo: Repo = {
      branch: "trunk",
      upstream: null,
      ahead: 0,
      behind: 0,
      staged: [],
      unstaged: [
        { path: "README.md", change: "untracked" },
        { path: "src/lib.rs", change: "untracked" },
      ],
      noCommits: true,
    };
    const { calls } = setup(repo);
    await showChanges();
    await waitFor(() => expect(changeRows()).toEqual(["U:README.md", "U:src/lib.rs"]));
    expect(screen.getByRole("button", { name: "README.md, untracked" })).toBeTruthy();
    expect(chip().textContent).toContain("Not published");
    fireEvent.click(screen.getByTestId("git-stage-all"));
    await waitFor(() => expect(changeRows()).toEqual(["S:README.md", "S:src/lib.rs"]));
    fireEvent.change(screen.getByLabelText("Commit message"), { target: { value: "First commit" } });
    fireEvent.keyDown(screen.getByLabelText("Commit message"), {
      key: "Enter",
      ctrlKey: true,
      metaKey: true,
    });
    await waitFor(() => expect(count(calls, "git_commit")).toBe(1));
    await waitFor(() => expect(screen.getByText("No changes")).toBeTruthy());
    // Publishing gives the new branch its upstream.
    await chipMenu(/^Publish branch$/);
    await waitFor(() => expect(chip().textContent).toContain("↑0 ↓0"));
    expect(calls.find((c) => c.cmd === "git_push")?.args).toMatchObject({ publish: true });
  });

  it("F-CD-20 discarding asks first, and Cancel keeps the change", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: null,
      ahead: 0,
      behind: 0,
      staged: [],
      unstaged: [modified("a.txt")],
      noCommits: false,
    };
    const { calls } = setup(repo);
    await showChanges();
    await waitFor(() => expect(changeRows()).toEqual(["U:a.txt"]));
    fireEvent.click(screen.getByTestId("git-discard"));
    const ask = await screen.findByRole("alertdialog");
    expect(within(ask).getByText("Discard changes to a.txt?")).toBeTruthy();
    fireEvent.click(within(ask).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(count(calls, "git_discard")).toBe(0);
    fireEvent.click(screen.getByTestId("git-discard"));
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(changeRows()).toEqual([]));
    expect(calls.find((c) => c.cmd === "git_discard")?.args).toMatchObject({ paths: ["a.txt"] });
  });

  it("F-CD-20 picking a change shows its diff, staged or not", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: null,
      ahead: 0,
      behind: 0,
      staged: [modified("s.txt")],
      unstaged: [modified("a.txt")],
      noCommits: false,
    };
    const { calls } = setup(repo);
    await showChanges();
    await waitFor(() => expect(changeRows()).toEqual(["S:s.txt", "U:a.txt"]));
    fireEvent.click(screen.getByRole("button", { name: "a.txt, modified" }));
    const diff = await screen.findByTestId("git-diff");
    expect(diff.dataset.path).toBe("a.txt");
    expect(diff.textContent).toContain("ONE");
    expect(calls.find((c) => c.cmd === "git_diff")?.args).toMatchObject({ path: "a.txt", staged: false });
    fireEvent.click(screen.getByRole("button", { name: "Back to changes" }));
    fireEvent.click(await screen.findByRole("button", { name: "s.txt, modified" }));
    await waitFor(() => expect(screen.getByTestId("git-diff").textContent).toContain("new staged"));
    expect(calls.filter((c) => c.cmd === "git_diff").pop()?.args).toMatchObject({
      path: "s.txt",
      staged: true,
    });
  });

  it("F-CD-20 outside a repository the view says so", async () => {
    setup(null);
    await showChanges();
    expect(await screen.findByText("Not a git repository")).toBeTruthy();
    expect(screen.queryByTestId("git-branch-chip")).toBeNull();
    expect(screen.getByTestId("status-branch").textContent).toContain("No branch");
  });

  it("F-CD-20 a commit git refuses keeps the message and says why", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: null,
      ahead: 0,
      behind: 0,
      staged: [modified("a.txt")],
      unstaged: [],
      noCommits: false,
    };
    setup(repo, { commitFails: "git commit failed: the pre-commit hook said no" });
    await showChanges();
    await waitFor(() => expect(changeRows()).toEqual(["S:a.txt"]));
    fireEvent.change(screen.getByLabelText("Commit message"), { target: { value: "Try" } });
    fireEvent.click(screen.getByTestId("git-commit"));
    expect(await screen.findByText("git commit failed: the pre-commit hook said no")).toBeTruthy();
    expect((screen.getByLabelText("Commit message") as HTMLTextAreaElement).value).toBe("Try");
    expect(changeRows()).toEqual(["S:a.txt"]);
  });

  it("F-CD-20 the status is read again on a watch batch and when an agent finishes", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: null,
      ahead: 0,
      behind: 0,
      staged: [],
      unstaged: [],
      noCommits: false,
    };
    const { calls, batch } = setup(repo);
    await waitFor(() => expect(count(calls, "git_status")).toBeGreaterThan(0));
    const { followGit } = await import("../../stores/git");
    followGit();
    await waitFor(() => expect(useGit.getState().byKey["ws:ws_1"]?.at).toBe("ready"));
    const before = count(calls, "git_status");
    repo.unstaged = [modified("a.txt")];
    await batch();
    await waitFor(() => expect(count(calls, "git_status")).toBeGreaterThan(before));
    await waitFor(() => expect(useGit.getState().byKey["ws:ws_1"]?.panel?.status.unstaged).toHaveLength(1));
    const afterBatch = count(calls, "git_status");
    const done: StatusRecord = { key: "pane:pn_1", status: "done", ts: 2 } as unknown as StatusRecord;
    act(() => useStatus.getState().apply([{ ...done, status: "working", ts: 1 } as StatusRecord], []));
    act(() => useStatus.getState().apply([done], []));
    await waitFor(() => expect(count(calls, "git_status")).toBeGreaterThan(afterBatch));
  });

  it("F-CD-20 the status bar shows the branch with ahead/behind and opens Changes", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: "origin/main",
      ahead: 2,
      behind: 1,
      staged: [],
      unstaged: [],
      noCommits: false,
    };
    setup(repo);
    const segment = await screen.findByTestId("status-branch");
    await waitFor(() => expect(segment.textContent).toContain("main↑2 ↓1"));
    expect(segment.getAttribute("aria-label")).toBe("Branch main, 2 to push, 1 to pull. Open changes");
    act(() => useSidebar.getState().setOpen(false));
    fireEvent.click(segment);
    expect(useSidebar.getState().open).toBe(true);
    expect(useSidebar.getState().tabs.find((x) => x.id === useSidebar.getState().active)?.kind).toBe("files");
    await waitFor(() => expect(useFiles.getState().byKey["ws:ws_1"]?.view).toBe("changes"));
  });

  it("F-CD-20 New branch creates and switches; Switch branch lists the others", async () => {
    const repo: Repo = {
      branch: "main",
      upstream: "origin/main",
      ahead: 0,
      behind: 0,
      staged: [],
      unstaged: [],
      noCommits: false,
    };
    const { calls } = setup(repo);
    await showChanges();
    await waitFor(() => expect(chip().dataset.branch).toBe("main"));
    await chipMenu(/^New branch/);
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Branch name"), { target: { value: "panel-work" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create and switch" }));
    await waitFor(() => expect(chip().dataset.branch).toBe("panel-work"));
    expect(calls.find((c) => c.cmd === "git_create_branch")?.args).toMatchObject({ name: "panel-work" });
    fireEvent.keyDown(chip(), { key: "Enter" });
    const sub = await screen.findByRole("menuitem", { name: /Switch branch/ });
    await waitFor(() => expect(count(calls, "git_branches")).toBeGreaterThan(0));
    fireEvent.keyDown(sub, { key: "ArrowRight" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "feature" }));
    await waitFor(() => expect(chip().dataset.branch).toBe("feature"));
    expect(calls.find((c) => c.cmd === "git_switch")?.args).toMatchObject({ branch: "feature" });
  });
});
