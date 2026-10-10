import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  EditorTabs,
  FileContent,
  FsChange,
  FsEntry,
  IpcError,
  Thread,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { AUTOSAVE_MS, useFiles } from "../../stores/files";
import { useSidebar } from "../../stores/sidebar";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { runCommand } from "../commands/commands";
import { useThreadDrop } from "../thread/drops";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { FilesPanel } from "./FilesPanel";

const WS = {
  id: "ws_1",
  root: "/work/shop",
  name: "shop",
  tabs: [{ id: "tab_1" }],
} as unknown as WorkspaceSummary;

type Channel = { onmessage: (c: FsChange) => void };

const hash = (text: string) => `h${text.length}:${text}`;

function entry(path: string, kind: "file" | "dir", extra: Partial<FsEntry> = {}): FsEntry {
  return { name: path.split("/").pop() ?? path, path, kind, symlink: false, ignored: false, ...extra };
}

/** A fake disk behind the fs_* commands. */
function setup(opts: { saved?: Partial<EditorTabs>; outside?: boolean } = {}) {
  const disk = new Map<string, string>([
    ["src/main.rs", "fn main() {}\n"],
    ["src/lib.rs", "pub fn lib() {}\n"],
    ["README.md", "# shop\n"],
  ]);
  const channels: Channel[] = [];
  const err = (code: string, message: string): IpcError => ({
    code: code as IpcError["code"],
    message,
    stage: "files",
    hint: null,
    retryable: false,
  });
  const calls = fakeBackend({
    workspace_list: () => [WS],
    editor_tabs_get: () => ({
      tabs: [],
      active: null,
      expanded: [],
      showIgnored: false,
      divider: null,
      view: null,
      ...opts.saved,
    }),
    editor_tabs_save: () => null,
    fs_watch: (a) => {
      channels.push(a?.channel as Channel);
      return `fsw_${channels.length}`;
    },
    fs_unwatch: () => null,
    fs_list: (a) => {
      const dir = String(a?.dir ?? "");
      if (dir === "") {
        const list = [entry("src", "dir", { gitStatus: "modified" }), entry("README.md", "file")];
        return a?.showIgnored ? [entry("node_modules", "dir", { ignored: true }), ...list] : list;
      }
      if (dir === "src")
        return [entry("src/lib.rs", "file"), entry("src/main.rs", "file", { gitStatus: "modified" })];
      return [];
    },
    fs_read: (a) => {
      const path = String(a?.path);
      if (path === "/etc/hosts") {
        return {
          path,
          kind: "text",
          text: "127.0.0.1 localhost\n",
          size: 20,
          readonly: true,
          readonlyReason: "outside",
          truncated: false,
          hash: "x",
          mtime: 1,
          dataUrl: null,
          hex: null,
        } satisfies FileContent;
      }
      const text = disk.get(path);
      if (text === undefined) throw err("not_found", "That file or folder doesn't exist");
      return {
        path,
        kind: "text",
        text,
        size: text.length,
        readonly: false,
        readonlyReason: null,
        truncated: false,
        hash: hash(text),
        mtime: 1,
        dataUrl: null,
        hex: null,
      } satisfies FileContent;
    },
    fs_write: (a) => {
      const path = String(a?.path);
      const now = disk.get(path);
      if (now !== undefined && a?.expectedHash !== hash(now)) throw err("conflict", "File changed on disk");
      disk.set(path, String(a?.text));
      return { hash: hash(String(a?.text)), mtime: 2 };
    },
    fs_compare: (a) => ({
      path: String(a?.path),
      oldPath: null,
      change: "modified",
      additions: 1,
      deletions: 1,
      binary: false,
      truncated: false,
      patch: `@@ -1 +1 @@\n-${(disk.get(String(a?.path)) ?? "").trimEnd()}\n+${String(a?.text).trimEnd()}\n`,
    }),
    term_quote_paths: (a) => `'${(a?.paths as string[])[0]}'`,
    pty_write: () => null,
    thread_attach_paths: (a) =>
      (a?.paths as string[]).map((path, i) => ({
        id: `att_${i + 1}`,
        threadId: String(a?.threadId),
        kind: "file",
        path,
        name: path.split("/").pop() ?? path,
        mime: null,
        bytes: 7,
        sent: false,
        error: null,
      })),
  });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  render(
    <UiProvider>
      <FilesPanel />
    </UiProvider>,
  );
  const watch = (change: Partial<FsChange>) =>
    act(async () => {
      for (const ch of channels) ch.onmessage({ paths: [], git: false, overflow: false, ...change });
      await Promise.resolve();
    });
  return { calls, disk, watch };
}

const rows = () => screen.queryAllByTestId("file-row").map((r) => r.dataset.path);
const writes = (calls: Call[]) => calls.filter((c) => c.cmd === "fs_write");
const ctxState = () => {
  const s = useFiles.getState();
  return s.key ? s.byKey[s.key] : undefined;
};

async function openMain() {
  await waitFor(() => expect(rows()).toContain("src"));
  fireEvent.click(screen.getByRole("treeitem", { name: /^src/ }));
  await waitFor(() => expect(rows()).toContain("src/main.rs"));
  fireEvent.click(screen.getByRole("treeitem", { name: /^main\.rs/ }));
  await waitFor(() => expect(ctxState()?.tabs.find((f) => f.path === "src/main.rs")?.status).toBe("ready"));
}

describe("Files tab (F-CD-19)", () => {
  beforeEach(() => resetStores());
  afterEach(() => vi.useRealTimers());

  it("F-CD-19.3 the tree hides a gitignored node_modules until Show ignored", async () => {
    const { calls } = setup();
    await waitFor(() => expect(rows()).toEqual(["src", "README.md"]));
    fireEvent.click(screen.getByTestId("files-show-ignored"));
    await waitFor(() => expect(rows()).toEqual(["node_modules", "src", "README.md"]));
    expect(screen.getByRole("treeitem", { name: "node_modules, ignored" })).toBeTruthy();
    expect(calls.some((c) => c.cmd === "fs_list" && c.args?.showIgnored === true)).toBe(true);
  });

  it("F-CD-19.1 saving a file an agent changed meanwhile shows the conflict prompt, never a silent overwrite", async () => {
    const { disk, calls } = setup();
    await openMain();
    disk.set("src/main.rs", "fn main() { agent(); }\n");
    act(() => useFiles.getState().edit("src/main.rs", "fn main() { mine(); }\n"));
    await act(async () => {
      await useFiles.getState().save("src/main.rs");
    });
    const dialog = await screen.findByRole("dialog", { name: "File changed on disk" });
    expect(disk.get("src/main.rs")).toBe("fn main() { agent(); }\n");
    expect(within(dialog).getByRole("button", { name: "Reload" })).toBeTruthy();
    // Compare shows both versions.
    fireEvent.click(within(dialog).getByRole("button", { name: "Compare" }));
    await waitFor(() => expect(screen.getByTestId("conflict-diff").textContent).toContain("mine"));
    expect(screen.getByTestId("conflict-diff").textContent).toContain("agent");
    // Overwrite writes mine over the agent's version, knowingly.
    fireEvent.click(screen.getByRole("button", { name: "Overwrite" }));
    await waitFor(() => expect(disk.get("src/main.rs")).toBe("fn main() { mine(); }\n"));
    expect(writes(calls).at(-1)?.args?.expectedHash).toBe(hash("fn main() { agent(); }\n"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("F-CD-19.1 Reload drops the unsaved edits for the version on disk", async () => {
    const { disk } = setup();
    await openMain();
    disk.set("src/main.rs", "agent\n");
    act(() => useFiles.getState().edit("src/main.rs", "mine\n"));
    await act(async () => {
      await useFiles.getState().save("src/main.rs");
    });
    fireEvent.click(await screen.findByRole("button", { name: "Reload" }));
    await waitFor(() => expect(ctxState()?.tabs[0]?.text).toBe("agent\n"));
    expect(ctxState()?.tabs[0]?.dirty).toBe(false);
    expect(disk.get("src/main.rs")).toBe("agent\n");
  });

  it("F-CD-19.2 auto-save writes within 1 s of the last keystroke", async () => {
    const { calls, disk } = setup();
    await openMain();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    act(() => useFiles.getState().edit("src/main.rs", "fn main() { 1 }\n"));
    act(() => vi.advanceTimersByTime(300));
    act(() => useFiles.getState().edit("src/main.rs", "fn main() { 12 }\n"));
    act(() => vi.advanceTimersByTime(AUTOSAVE_MS - 1));
    expect(writes(calls)).toHaveLength(0);
    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    vi.useRealTimers();
    await waitFor(() => expect(disk.get("src/main.rs")).toBe("fn main() { 12 }\n"));
    expect(writes(calls)).toHaveLength(1);
    expect(AUTOSAVE_MS).toBeLessThan(1000);
  });

  it("auto-save off: edits wait for Mod+S", async () => {
    const { calls } = setup();
    await act(async () => {
      await (await import("../../stores/settings")).useSettings.getState().set("auto_save_files", false);
    });
    await openMain();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    act(() => useFiles.getState().edit("src/main.rs", "x\n"));
    act(() => vi.advanceTimersByTime(5_000));
    vi.useRealTimers();
    expect(writes(calls)).toHaveLength(0);
    expect(screen.getByTestId("editor-tab").dataset.dirty).toBe("true");
  });

  it("F-CD-19.4 the same tabs reopen for the workspace, and changes are saved per context", async () => {
    const { calls } = setup({
      saved: {
        tabs: [{ path: "README.md" }, { path: "src/main.rs" }],
        active: "src/main.rs",
        expanded: ["src"],
      },
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("editor-tab").map((t) => t.dataset.path)).toEqual([
        "README.md",
        "src/main.rs",
      ]),
    );
    expect(calls.find((c) => c.cmd === "editor_tabs_get")?.args?.contextKey).toBe("ws:ws_1");
    // The expanded folder comes back too (listed, though the narrow layout shows the editor).
    await waitFor(() => expect(calls.some((c) => c.cmd === "fs_list" && c.args?.dir === "src")).toBe(true));
    fireEvent.click(screen.getByTitle("Close main.rs"));
    await waitFor(() => {
      const save = calls.filter((c) => c.cmd === "editor_tabs_save").at(-1);
      expect((save?.args?.state as EditorTabs | undefined)?.tabs).toEqual([{ path: "README.md" }]);
    });
  });

  it("closing a tab selects its left neighbour", async () => {
    setup({
      saved: {
        tabs: [{ path: "README.md" }, { path: "src/lib.rs" }, { path: "src/main.rs" }],
        active: "src/lib.rs",
      },
    });
    await waitFor(() => expect(screen.getAllByTestId("editor-tab")).toHaveLength(3));
    act(() => useFiles.getState().close("src/lib.rs"));
    expect(ctxState()?.active).toBe("README.md");
    act(() => useFiles.getState().close("README.md"));
    expect(ctxState()?.active).toBe("src/main.rs");
  });

  it("Delete on a focused editor tab closes it (the keyboard's ×)", async () => {
    setup({ saved: { tabs: [{ path: "README.md" }, { path: "src/main.rs" }], active: "src/main.rs" } });
    await waitFor(() => expect(screen.getAllByTestId("editor-tab")).toHaveLength(2));
    const tab = screen.getByRole("tab", { name: "main.rs" });
    expect(tab.getAttribute("aria-keyshortcuts")).toBe("Delete");
    fireEvent.keyDown(tab, { key: "Delete" });
    await waitFor(() =>
      expect(screen.getAllByTestId("editor-tab").map((t) => t.dataset.path)).toEqual(["README.md"]),
    );
  });

  it("a clean open file follows the disk; an edited one keeps its text and says the file changed", async () => {
    const { disk, watch } = setup({
      saved: { tabs: [{ path: "src/main.rs" }, { path: "README.md" }], active: "src/main.rs" },
    });
    await waitFor(() => expect(ctxState()?.tabs.every((f) => f.status === "ready")).toBe(true));
    act(() => useFiles.getState().edit("src/main.rs", "mine\n"));
    disk.set("src/main.rs", "agent\n");
    disk.set("README.md", "# changed\n");
    await watch({ paths: ["src/main.rs", "README.md"] });
    await waitFor(() =>
      expect(ctxState()?.tabs.find((f) => f.path === "README.md")?.text).toBe("# changed\n"),
    );
    await waitFor(() =>
      expect(ctxState()?.tabs.find((f) => f.path === "src/main.rs")?.changedOnDisk).toBe(true),
    );
    expect(ctxState()?.tabs.find((f) => f.path === "src/main.rs")?.text).toBe("mine\n");
    expect(screen.getByText("This file changed on disk while you were editing it.")).toBeTruthy();
  });

  it("F-CD-09.2 a file outside every workspace opens read-only with a banner", async () => {
    setup();
    await waitFor(() => expect(rows()).toContain("src"));
    await act(async () => {
      await useFiles.getState().open("/etc/hosts", { line: 1 });
    });
    expect(await screen.findByText("This file is outside your workspaces, so it's read-only.")).toBeTruthy();
    expect(screen.getByTestId("code-editor").dataset.readonly).toBe("true");
    act(() => useFiles.getState().edit("/etc/hosts", "evil\n"));
    expect(ctxState()?.tabs.find((f) => f.path === "/etc/hosts")?.dirty).toBe(false);
  });

  it("follows the thread's folder in Thread mode; a folderless thread says it has no folder", async () => {
    setup();
    await waitFor(() => expect(rows()).toContain("src"));
    const thread = (id: string, workspaceId: string | null) =>
      ({ id, workspaceId, cwd: "/work/shop", title: id }) as unknown as Thread;
    act(() => {
      useThreads.setState({ list: [thread("th_1", "ws_1"), thread("th_2", null)], selectedId: "th_1" });
      useUi.setState({ mode: "thread" });
    });
    await waitFor(() => expect(useFiles.getState().key).toBe("th:th_1"));
    act(() => useThreads.setState({ selectedId: "th_2" }));
    expect(await screen.findByText("This thread has no folder")).toBeTruthy();
    act(() => useUi.setState({ mode: "code" }));
    await waitFor(() => expect(useFiles.getState().key).toBe("ws:ws_1"));
  });

  it("F-CD-06 dragging a file from the tree onto a pane types its quoted path, never Enter", async () => {
    const { calls } = setup();
    await waitFor(() => expect(rows()).toContain("README.md"));
    const pane = document.createElement("div");
    pane.dataset.testid = "pane";
    pane.dataset.paneId = "pane_3";
    document.body.appendChild(pane);
    // jsdom has no layout: every point is over the pane.
    document.elementFromPoint = () => pane;
    const row = screen.getByRole("treeitem", { name: "README.md" });
    // jsdom has no PointerEvent: mouse events of the pointer types carry the
    // button and coordinates the same way.
    const pointer = (target: EventTarget, type: string, x: number, y: number) =>
      act(() => {
        target.dispatchEvent(
          new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y }),
        );
      });
    pointer(row, "pointerdown", 10, 10);
    pointer(window, "pointermove", 200, 200);
    expect(document.querySelector('[data-testid="file-drag-ghost"]')?.textContent).toBe("README.md");
    pointer(window, "pointerup", 200, 200);
    await waitFor(() => expect(calls.some((c) => c.cmd === "pty_write")).toBe(true));
    expect(calls.find((c) => c.cmd === "term_quote_paths")?.args).toMatchObject({
      paneId: "pane_3",
      paths: ["/work/shop/README.md"],
    });
    expect(calls.find((c) => c.cmd === "pty_write")?.args?.data).toBe("'/work/shop/README.md'");
    expect(document.querySelector('[data-testid="file-drag-ghost"]')).toBeNull();
    delete (document as { elementFromPoint?: unknown }).elementFromPoint;
    pane.remove();
  });

  it("F-TH-07 dragging a file from the tree onto the open thread attaches it to the composer", async () => {
    const { calls } = setup();
    useThreadDrop.setState({ over: false, dropped: {} });
    act(() => useThreads.setState({ selectedId: "th_1" }));
    await waitFor(() => expect(rows()).toContain("README.md"));
    const thread = document.createElement("div");
    thread.dataset.testid = "thread-main";
    document.body.appendChild(thread);
    document.elementFromPoint = () => thread;
    const row = screen.getByRole("treeitem", { name: "README.md" });
    const pointer = (target: EventTarget, type: string, x: number, y: number) =>
      act(() => {
        target.dispatchEvent(
          new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y }),
        );
      });
    pointer(row, "pointerdown", 10, 10);
    pointer(window, "pointermove", 300, 120);
    expect(useThreadDrop.getState().over).toBe(true);
    pointer(window, "pointerup", 300, 120);
    await waitFor(() =>
      expect(useThreadDrop.getState().dropped.th_1?.map((a) => a.path)).toEqual(["/work/shop/README.md"]),
    );
    expect(calls.find((c) => c.cmd === "thread_attach_paths")?.args).toMatchObject({
      threadId: "th_1",
      paths: ["/work/shop/README.md"],
    });
    expect(useThreadDrop.getState().over).toBe(false);
    expect(calls.some((c) => c.cmd === "pty_write")).toBe(false);
    delete (document as { elementFromPoint?: unknown }).elementFromPoint;
    thread.remove();
  });

  it("Mod+Alt+F's command opens the Files tab", () => {
    fakeBackend({});
    expect(runCommand("sidebar.files")).toBe(true);
    const s = useSidebar.getState();
    expect(s.open).toBe(true);
    expect(s.tabs.find((x) => x.id === s.active)?.kind).toBe("files");
  });
});
