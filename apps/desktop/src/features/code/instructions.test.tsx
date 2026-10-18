import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { InstructionsFile, InstructionsPlan, InstructionsReport, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { InstructionsDialog } from "./Instructions";

const WS = { id: "ws_1", name: "shop", kind: "local", root: "/code/shop" } as unknown as WorkspaceSummary;

function file(path: string, extra: Partial<InstructionsFile> = {}): InstructionsFile {
  return {
    path,
    exists: true,
    bytes: 20,
    git: "tracked",
    unsafe: null,
    inWorktrees: null,
    pointsToAgents: false,
    diff: null,
    ...extra,
  };
}

const DIFF = {
  path: "CLAUDE.md",
  change: "modified" as const,
  additions: 1,
  deletions: 1,
  binary: false,
  patch: "diff --git a/CLAUDE.md b/CLAUDE.md\n--- a/CLAUDE.md\n+++ b/CLAUDE.md\n@@ -1,2 +1,2 @@\n # Rules\n-Use npm.\n+Use pnpm.\n",
  truncated: false,
};

function report(over: Partial<InstructionsReport> = {}): InstructionsReport {
  return {
    root: "/code/shop",
    engines: [
      { engineId: "claude", label: "Claude Code", known: true, reads: ["CLAUDE.md"], readsAgents: false },
      { engineId: "codex", label: "Codex", known: true, reads: ["AGENTS.md"], readsAgents: true },
      { engineId: "aider", label: "Aider", known: false, reads: [], readsAgents: false },
    ],
    files: [
      file("AGENTS.md"),
      file("CLAUDE.md", { diff: DIFF }),
      file("GEMINI.md", { exists: false, git: "none" }),
      file("CLAUDE.local.md", { git: "ignored", inWorktrees: false }),
    ],
    ...over,
  };
}

const PLAN: InstructionsPlan = {
  changes: [
    { path: "CLAUDE.md", action: "replace", before: "# Rules\nUse pnpm.\n", after: "@AGENTS.md\n", textLost: true },
    { path: "GEMINI.md", action: "create", before: null, after: "@AGENTS.md\n", textLost: false },
  ],
  skipped: [],
  digest: "d1",
};

let calls: Call[];
function open(handlers: Record<string, (a?: Record<string, unknown>) => unknown>) {
  calls = fakeBackend(handlers);
  render(
    <UiProvider>
      <InstructionsDialog ws={WS} onClose={() => undefined} />
    </UiProvider>,
  );
}

const reads = (engine: string) =>
  within(screen.getAllByTestId("instructions-engine").find((e) => e.dataset.engine === engine) as HTMLElement).getByTestId(
    "instructions-reads",
  ).textContent;

describe("UI-160 Instructions (F-EN-07)", () => {
  beforeEach(() => resetStores());

  it("F-EN-07.1 lists who reads what, and shows CLAUDE.md's difference from AGENTS.md", async () => {
    open({ instructions_get: () => report() });
    await screen.findAllByTestId("instructions-engine");
    expect(reads("claude")).toBe("Reads CLAUDE.md, not AGENTS.md");
    expect(reads("codex")).toBe("Reads AGENTS.md");
    const rows = screen.getAllByTestId("instructions-file").map((r) => r.dataset.path);
    expect(rows).toEqual(["AGENTS.md", "CLAUDE.md", "CLAUDE.local.md"]);
    fireEvent.click(screen.getByTestId("instructions-diff-toggle"));
    expect(await screen.findByText("Use pnpm.", { exact: false })).toBeTruthy();
  });

  it("F-EN-07.2 Use one file for all shows the plan, writes only on confirm, and offers Undo", async () => {
    open({
      instructions_get: () => report(),
      instructions_plan: () => PLAN,
      instructions_apply: () => ({ changeId: "ins_1", written: ["CLAUDE.md", "GEMINI.md"] }),
      instructions_undo: () => null,
    });
    await screen.findAllByTestId("instructions-engine");
    await act(async () => fireEvent.click(screen.getByTestId("instructions-one-file")));
    const changes = await screen.findAllByTestId("instructions-change");
    expect(changes.map((c) => c.dataset.path)).toEqual(["CLAUDE.md", "GEMINI.md"]);
    expect(screen.getByTestId("instructions-text-lost")).toBeTruthy();
    // AGENTS.md exists, so no source is sent; nothing is written yet.
    expect(calls.find((c) => c.cmd === "instructions_plan")?.args).toEqual({ workspaceId: "ws_1", source: null });
    expect(calls.some((c) => c.cmd === "instructions_apply")).toBe(false);
    await act(async () => fireEvent.click(screen.getByTestId("instructions-confirm")));
    expect(calls.find((c) => c.cmd === "instructions_apply")?.args).toEqual({
      workspaceId: "ws_1",
      source: null,
      digest: "d1",
    });
    await act(async () => fireEvent.click(await screen.findByRole("button", { name: "Undo", hidden: true })));
    await waitFor(() => expect(calls.find((c) => c.cmd === "instructions_undo")?.args).toEqual({ changeId: "ins_1" }));
  });

  it("F-EN-07.3 without AGENTS.md the plan starts it from the chosen file", async () => {
    open({
      instructions_get: () =>
        report({ files: [file("AGENTS.md", { exists: false, git: "none" }), file("CLAUDE.md"), file("GEMINI.md")] }),
      instructions_plan: () => PLAN,
    });
    await screen.findAllByTestId("instructions-engine");
    expect(screen.getByText("Start AGENTS.md from")).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByTestId("instructions-one-file")));
    expect(calls.find((c) => c.cmd === "instructions_plan")?.args).toEqual({ workspaceId: "ws_1", source: "CLAUDE.md" });
  });

  it("F-EN-07.4 an ignored CLAUDE.local.md is Not in new worktrees, and Copy into worktrees adds it", async () => {
    open({
      instructions_get: () => report(),
      instructions_copy_to_worktrees: () => ({ file: "/code/shop/.vibezeo/worktree.json", exists: true }),
    });
    const row = await screen.findByTestId("instructions-not-in-worktrees");
    await act(async () => fireEvent.click(within(row).getByRole("button", { name: "Copy into worktrees" })));
    expect(calls.find((c) => c.cmd === "instructions_copy_to_worktrees")?.args).toEqual({
      workspaceId: "ws_1",
      path: "CLAUDE.local.md",
    });
  });

  it("F-EN-07.5 an engine without a table entry is Unknown", async () => {
    open({ instructions_get: () => report() });
    await screen.findAllByTestId("instructions-engine");
    expect(reads("aider")).toBe("Unknown");
  });

  it("F-EN-07.6 a linked CLAUDE.md is marked and the plan says it was skipped", async () => {
    open({
      instructions_get: () => report({ files: [file("AGENTS.md"), file("CLAUDE.md", { unsafe: "symlink" })] }),
      instructions_plan: () => ({ changes: [], skipped: [{ path: "CLAUDE.md", reason: "It's a symbolic link" }], digest: "d0" }),
    });
    expect(await screen.findByText("A link: not read or written")).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByTestId("instructions-one-file")));
    expect((await screen.findByTestId("instructions-skipped")).textContent).toBe(
      "CLAUDE.md is left as it is: It's a symbolic link",
    );
    expect((screen.getByTestId("instructions-confirm") as HTMLButtonElement).disabled).toBe(true);
  });

  it("UI-160 the empty state, and a failed read with Try again", async () => {
    let fail = true;
    open({
      instructions_get: () => {
        if (fail) throw { code: "invalid", message: "Instructions are checked for folders on this computer" };
        return report({ engines: [], files: [file("AGENTS.md", { exists: false, git: "none" })] });
      },
    });
    expect(
      await screen.findByText("Couldn't read the instructions files: Instructions are checked for folders on this computer"),
    ).toBeTruthy();
    fail = false;
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
    expect(await screen.findByText("No instructions files yet")).toBeTruthy();
  });
});
