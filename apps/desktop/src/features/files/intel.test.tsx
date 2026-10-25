import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { FileContent, FsEntry, LspDocument, LspLocation, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useFiles } from "../../stores/files";
import { diffNames, nameAt } from "../../stores/intel";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { intelStatus } from "./FileEditor";
import { FilesPanel } from "./FilesPanel";

const WS = {
  id: "ws_1",
  root: "/work/py",
  name: "py",
  tabs: [{ id: "tab_1" }],
} as unknown as WorkspaceSummary;

const DISK: Record<string, string> = {
  "util.py": "def helper():\n    return 1\n",
  "main.py": "import util\nutil.helper()\nERROR\n",
  "lib.rs": "fn a() {}\n",
  "app.ts": "export const a = 1;\n",
};

let trusted = false;

const loc = (path: string, line: number, character: number, preview: string): LspLocation => ({
  path,
  inFolder: true,
  line,
  character,
  endLine: line,
  endCharacter: character + 6,
  preview,
});

function doc(path: string, text: string): LspDocument {
  if (path.endsWith(".ts"))
    return trusted
      ? {
          language: "TypeScript",
          server: "typescript-language-server",
          state: "ready",
          message: null,
          diagnostics: [],
          sandboxed: false,
        }
      : {
          language: "TypeScript",
          server: "typescript-language-server",
          state: "untrusted",
          message: "typescript-language-server is off until you trust this repository",
          diagnostics: [],
          sandboxed: false,
        };
  if (path.endsWith(".rs"))
    return {
      language: "Rust",
      server: "rust-analyzer",
      state: "none",
      message: "No language server for Rust",
      diagnostics: [],
      sandboxed: false,
    };
  return {
    language: "Python",
    server: "pyright",
    state: "ready",
    message: null,
    diagnostics: text.split("\n").flatMap((l, line) =>
      l.includes("ERROR")
        ? [
            {
              line,
              character: 0,
              endLine: line,
              endCharacter: l.length,
              severity: "error" as const,
              message: "fake: error here",
            },
          ]
        : [],
    ),
    sandboxed: false,
  };
}

function setup(): Call[] {
  const entry = (path: string): FsEntry => ({
    name: path,
    path,
    kind: "file",
    symlink: false,
    ignored: false,
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
    }),
    editor_tabs_save: () => null,
    fs_watch: () => "fsw_1",
    fs_unwatch: () => null,
    fs_list: (a) => (String(a?.dir ?? "") === "" ? Object.keys(DISK).map(entry) : []),
    fs_read: (a) => {
      const path = String(a?.path);
      const text = DISK[path] ?? "";
      return {
        path,
        kind: "text",
        text,
        size: text.length,
        readonly: false,
        readonlyReason: null,
        truncated: false,
        hash: `h${text.length}`,
        mtime: 1,
        dataUrl: null,
        hex: null,
      } satisfies FileContent;
    },
    lsp_sync: (a) => doc(String(a?.path), String(a?.text)),
    lsp_definition: () => [loc("util.py", 0, 4, "def helper():")],
    lsp_references: () => [loc("util.py", 0, 4, "def helper():"), loc("main.py", 1, 5, "util.helper()")],
    lsp_hover: () => "fake hover: helper",
    lsp_trust: () => {
      trusted = true;
      return null;
    },
  });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  render(
    <UiProvider>
      <FilesPanel />
    </UiProvider>,
  );
  return calls;
}

const ctxState = () => {
  const s = useFiles.getState();
  return s.key ? s.byKey[s.key] : undefined;
};

async function openFile(path: string, at?: { line: number; col: number }) {
  await waitFor(() => expect(ctxState()?.restored).toBe(true));
  await act(async () => {
    await useFiles.getState().open(path, at);
  });
  await waitFor(() => expect(ctxState()?.tabs.find((f) => f.path === path)?.status).toBe("ready"));
}

/** Presses a key in the editor (the cursor is where `openFile` put it). */
function press(key: string, shiftKey = false) {
  const content = screen.getByTestId("code-editor").querySelector(".cm-content");
  if (!content) throw new Error("no editor");
  act(() => {
    content.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true }));
  });
}

const status = () => screen.queryByTestId("intel-status")?.textContent ?? null;

describe("Code intelligence in the Files editor (F-CD-23)", () => {
  beforeEach(() => {
    resetStores();
    trusted = false;
  });

  it("F-CD-23.1 F12 opens the definition in the other file, in a new tab at its line", async () => {
    const calls = setup();
    await openFile("main.py", { line: 2, col: 7 });
    await waitFor(() => expect(status()).toBe("1 error"));
    press("F12");
    await waitFor(() => expect(ctxState()?.active).toBe("util.py"));
    expect(ctxState()?.tabs.map((f) => f.path)).toEqual(["main.py", "util.py"]);
    expect(ctxState()?.tabs.find((f) => f.path === "util.py")?.goto).toMatchObject({ line: 1, col: 5 });
    const asked = calls.find((c) => c.cmd === "lsp_definition");
    expect(asked?.args).toMatchObject({ path: "main.py", line: 1, character: 6 });
    // The editor's text goes with it, unsaved edits included.
    expect(asked?.args?.text).toBe(DISK["main.py"]);
  });

  it("F-CD-23.3 the server's diagnostics underline their range and the status line counts them", async () => {
    setup();
    await openFile("main.py");
    await waitFor(() => expect(status()).toBe("1 error"));
    const marks = Array.from(screen.getByTestId("code-editor").querySelectorAll(".cm-diag-error"));
    expect(marks.map((m) => m.textContent)).toEqual(["ERROR"]);
  });

  it("F-CD-23.2 Shift+F12 lists the references; choosing one opens it", async () => {
    setup();
    await openFile("main.py", { line: 2, col: 7 });
    await waitFor(() => expect(status()).toBe("1 error"));
    press("F12", true);
    await waitFor(() =>
      expect(screen.getByTestId("references-title").textContent).toBe("2 references to helper"),
    );
    const rows = screen.getAllByTestId("reference-row");
    expect(rows.map((r) => r.textContent)).toEqual(["util.py:1def helper():", "main.py:2util.helper()"]);
    fireEvent.click(rows[0] as HTMLElement);
    await waitFor(() => expect(ctxState()?.active).toBe("util.py"));
    // Escape closes the panel.
    fireEvent.keyDown(screen.getByTestId("references-panel"), { key: "Escape" });
    expect(screen.queryByTestId("references-panel")).toBeNull();
  });

  it("F-CD-23.2 a diff's Find references asks in the worktree and fills the panel", async () => {
    const calls = setup();
    await waitFor(() => expect(ctxState()?.restored).toBe(true));
    const names = diffNames({ type: "workspace", workspaceId: "ws_1" });
    await act(async () => {
      names.onReferences({ path: "main.py", line: 2, character: 5, name: "helper" });
      await new Promise((r) => setTimeout(r, 0));
    });
    await waitFor(() =>
      expect(screen.getByTestId("references-title").textContent).toBe("2 references to helper"),
    );
    expect(calls.find((c) => c.cmd === "lsp_references")?.args).toMatchObject({
      context: { type: "workspace", workspaceId: "ws_1" },
      path: "main.py",
      line: 1,
      character: 5,
      text: DISK["main.py"],
    });
    await waitFor(() => expect(ctxState()?.active).toBe("main.py"));
  });

  it("F-CD-23.4 without a server the status line says so and names what to install", async () => {
    setup();
    await openFile("lib.rs");
    await waitFor(() => expect(status()).toBe("No language server for Rust"));
    expect(screen.getByTestId("intel-status").getAttribute("title")).toBe(
      "Install rust-analyzer to get definitions and references",
    );
  });

  it("F-CD-23 a server that runs the repository's code waits for Trust repository…", async () => {
    const calls = setup();
    await openFile("app.ts");
    await waitFor(() =>
      expect(status()).toContain("typescript-language-server is off until you trust this repository"),
    );
    fireEvent.click(screen.getByTestId("intel-trust"));
    await waitFor(() => expect(status()).toBe("typescript-language-server"));
    expect(calls.find((c) => c.cmd === "lsp_trust")?.args).toMatchObject({ path: "app.ts" });
    expect(screen.queryByTestId("intel-trust")).toBeNull();
  });

  it("F-CD-23 a sandboxed worktree is trusted on its own, and the status line says so", () => {
    const doc: LspDocument = {
      language: "Rust",
      server: "rust-analyzer",
      state: "untrusted",
      message: "rust-analyzer is off until you trust this worktree",
      diagnostics: [],
      sandboxed: true,
    };
    expect(intelStatus(doc, false)?.text).toBe("rust-analyzer is off until you trust this worktree");
  });

  it("F-CD-23 closing a tab tells the language server", async () => {
    const calls = setup();
    await openFile("main.py");
    act(() => useFiles.getState().close("main.py"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "lsp_close")?.args).toMatchObject({ path: "main.py" }),
    );
  });

  it("F-CD-23 the status line's words", () => {
    const ready = doc("a.py", "");
    expect(intelStatus(undefined, true)?.text).toBe("Starting the language server…");
    expect(intelStatus(undefined, false)).toBeNull();
    expect(intelStatus(ready, false)?.text).toBe("pyright");
    const d = (severity: "error" | "warning") => ({
      line: 0,
      character: 0,
      endLine: 0,
      endCharacter: 1,
      severity,
      message: "m",
    });
    expect(intelStatus({ ...ready, diagnostics: [d("error"), d("error"), d("warning")] }, false)?.text).toBe(
      "2 errors, 1 warning",
    );
    expect(
      intelStatus({ ...ready, state: "failed", message: "rust-analyzer stopped: it exited" }, false)?.text,
    ).toBe("rust-analyzer stopped: it exited");
    expect(nameAt("util.helper()", { line: 0, character: 6 })).toBe("helper");
  });
});
