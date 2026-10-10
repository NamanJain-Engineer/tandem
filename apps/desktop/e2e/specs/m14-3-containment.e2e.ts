import { freshOffline, invoke, waitForPaneText } from "../support/app";
import { PROJECT, ensureConnected, openRemoteFolder, resetProject } from "../support/remote";

// E2E-M14-3 Containment (F-RM-01 AC3): through a remote workspace,
// `../../etc/passwd` is refused by tandem-host (`..` out of the root), and
// so is an absolute path outside it; files inside read, write and list.

type Ctx = { type: "workspace"; workspaceId: string };

describe("E2E-M14-3 containment", () => {
  let context: Ctx;

  before(async function () {
    this.timeout(180_000);
    resetProject();
    await freshOffline();
    await ensureConnected();
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    const ws = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find(
      (w) => w.root === PROJECT,
    );
    context = { type: "workspace", workspaceId: ws?.id ?? "" };
  });

  it("F-RM-01.3 reading ../../etc/passwd relative to the root is refused", async () => {
    const err = await invoke("fs_read", { context, path: "../../etc/passwd" }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(err).toContain("path_escape");
    const abs = await invoke("fs_read", { context, path: "/etc/passwd" }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(abs).toContain("path_escape");
  });

  it("F-RM-01 files inside the remote root list, read and save", async () => {
    const rows = await invoke<Array<{ path: string }>>("fs_list", { context, dir: "", showIgnored: false });
    expect(rows.map((r) => r.path)).toEqual(expect.arrayContaining(["README.md", "main.rs"]));
    const file = await invoke<{ text: string; hash: string; readonly: boolean }>("fs_read", {
      context,
      path: "README.md",
    });
    expect(file.text).toBe("# project\n");
    expect(file.readonly).toBe(false);
    await invoke("fs_write", {
      context,
      path: "README.md",
      text: "# project\nedited\n",
      expectedHash: file.hash,
    });
    const again = await invoke<{ text: string }>("fs_read", { context, path: "README.md" });
    expect(again.text).toBe("# project\nedited\n");
    // The host's git sees it.
    const panel = await invoke<{ status: { unstaged: Array<{ path: string; change: string }> } } | null>(
      "git_status",
      { context },
    );
    expect(panel?.status.unstaged).toContainEqual({ path: "README.md", change: "modified" });
    await invoke("git_discard", { context, paths: ["README.md"] });
  });
});
