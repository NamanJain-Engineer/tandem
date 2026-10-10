import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Checkpoint, CheckpointTimeline, FileDiff, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { useWorkspaces } from "../../stores/workspaces";
import { useUi } from "../../stores/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { CheckpointsPanel } from "./CheckpointsPanel";
import { formatBytes } from "./CheckpointsSettings";

const WS = { id: "ws_1", root: "/work/shop", name: "shop", tabs: [{ id: "tab_1" }], branch: "main" } as unknown as WorkspaceSummary;

function cp(id: string, over: Partial<Checkpoint> = {}): Checkpoint {
  return {
    id,
    folder: "/work/shop",
    workspaceId: "ws_1",
    threadId: null,
    paneId: null,
    turnId: null,
    reason: "turn",
    label: "Fix login",
    files: 3,
    skipped: [],
    partial: false,
    late: false,
    createdAt: Date.now() - 120_000,
    differs: 2,
    ...over,
  };
}

const DIFF: FileDiff[] = [
  { path: "src/a.ts", oldPath: undefined, change: "modified", additions: 1, deletions: 2, binary: false, patch: "@@ -1 +1 @@\n-old\n+new\n", truncated: false },
  { path: "src/new.ts", oldPath: undefined, change: "added", additions: 1, deletions: 0, binary: false, patch: "@@ -0,0 +1 @@\n+x\n", truncated: false },
];

function setup(timeline: Partial<CheckpointTimeline> = {}) {
  const tl: CheckpointTimeline = { folder: "/work/shop", store: "repo", enabled: true, items: [cp("cp_1"), cp("cp_0", { reason: "prompt", label: "Claude Code · 2", differs: 0 })], ...timeline };
  const calls = fakeBackend({
    workspace_list: () => [WS],
    checkpoints_list: () => tl,
    checkpoint_diff: () => DIFF,
    checkpoint_restore: (a) => ({ restored: (a?.paths as string[] | null) ?? DIFF.map((d) => d.path), failed: [], undoId: "cp_undo" }),
    checkpoint_create: () => cp("cp_2", { reason: "manual" }),
    settings_set: () => null,
  });
  useUi.setState({ mode: "code" });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  render(
    <UiProvider>
      <CheckpointsPanel />
    </UiProvider>,
  );
  return { calls, tl };
}

const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;

describe("Checkpoints tab (UI-140)", () => {
  beforeEach(() => resetStores());

  it("F-EX-01 lists the folder's checkpoints with what differs from now", async () => {
    const { calls } = setup();
    const rows = await screen.findAllByTestId("checkpoint-row");
    expect(rows.map((r) => r.dataset.id)).toEqual(["cp_1", "cp_0"]);
    expect(rows[0]?.textContent).toContain("Before a turn");
    expect(rows[0]?.textContent).toContain("2 files differ");
    expect(rows[1]?.textContent).toContain("Before a prompt");
    expect(rows[1]?.textContent).toContain("Same as now");
    expect(calls.find((c) => c.cmd === "checkpoints_list")?.args).toEqual({ context: { type: "workspace", workspaceId: "ws_1" } });
  });

  it("F-EX-01.2 restore all asks first, then restores every differing file and offers Undo", async () => {
    const { calls } = setup();
    fireEvent.click((await screen.findAllByTestId("checkpoint-row"))[0]!);
    expect(await screen.findAllByTestId("checkpoint-file")).toHaveLength(2);
    fireEvent.click(screen.getByTestId("checkpoint-restore-all"));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog?.textContent).toContain("Restore 2 files to Before a turn?");
    expect(dialog?.textContent).toContain("A checkpoint of how things are now is saved first");
    expect(count(calls, "checkpoint_restore")).toBe(0);
    fireEvent.click(within(dialog).getByRole("button", { name: "Restore files" }));
    await waitFor(() => expect(count(calls, "checkpoint_restore")).toBe(1));
    expect(calls.find((c) => c.cmd === "checkpoint_restore")?.args).toMatchObject({ checkpointId: "cp_1", paths: null });
    expect(await screen.findByText("Restored 2 files")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(count(calls, "checkpoint_restore")).toBe(2));
    expect(calls.filter((c) => c.cmd === "checkpoint_restore")[1]?.args).toMatchObject({ checkpointId: "cp_undo" });
  });

  it("F-EX-01.3 restoring chosen files sends only those", async () => {
    const { calls } = setup();
    fireEvent.click((await screen.findAllByTestId("checkpoint-row"))[0]!);
    const files = await screen.findAllByTestId("checkpoint-file");
    fireEvent.click(within(files[1]!).getByRole("checkbox"));
    fireEvent.click(screen.getByTestId("checkpoint-restore-some"));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog?.textContent).toContain("Restore 1 file to Before a turn?");
    fireEvent.click(within(dialog).getByRole("button", { name: "Restore files" }));
    await waitFor(() => expect(count(calls, "checkpoint_restore")).toBe(1));
    expect(calls.find((c) => c.cmd === "checkpoint_restore")?.args).toMatchObject({ paths: ["src/a.ts"] });
  });

  it("F-EX-01.6 a folder without git says what checkpoints cover", async () => {
    setup({ store: "folder" });
    expect((await screen.findByTestId("checkpoints-folder-note")).textContent).toContain("Not a git repository. Checkpoints skip what .gitignore lists and stop at 20,000 files.");
  });

  it("F-EX-01.10 off shows the off state with Turn on; no git says so", async () => {
    const { calls } = setup({ enabled: false, items: [] });
    fireEvent.click(await screen.findByRole("button", { name: "Turn on" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "settings_set" && c.args?.key === "checkpoints_enabled")).toBe(true));
  });

  it("F-EX-01 empty, git missing and new checkpoints refresh the list", async () => {
    const { calls } = setup({ items: [] });
    expect(await screen.findByText("No checkpoints yet")).toBeTruthy();
    const before = count(calls, "checkpoints_list");
    await act(async () => emitLocal("checkpoint:created", { folder: "/work/shop" }));
    await waitFor(() => expect(count(calls, "checkpoints_list")).toBe(before + 1));
    await act(async () => emitLocal("checkpoint:created", { folder: "/elsewhere" }));
    expect(count(calls, "checkpoints_list")).toBe(before + 1);
  });

  it("F-EX-01 git missing shows the honest state", async () => {
    setup({ store: "no_git", items: [] });
    expect(await screen.findByText("Checkpoints need git. Install git to turn them on.")).toBeTruthy();
    expect((screen.getByTestId("checkpoint-now") as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-EX-01 sizes read plainly", () => {
    expect(formatBytes(0)).toBe("0 KB");
    expect(formatBytes(412 * 1024 * 1024)).toBe("412 MB");
    expect(formatBytes(3.1 * 1024 ** 3)).toBe("3.1 GB");
  });
});
