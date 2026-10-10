import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { PaneRecord, PromptInfo, PromptList, Rendered, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { terminalHandles, type TerminalHandle } from "../code/TerminalView";
import { PromptPicker, usePrompts } from "./PromptPicker";

const WS = { id: "ws_1", root: "/work/shop", name: "shop", tabs: [{ id: "tab_1" }], branch: "main" } as unknown as WorkspaceSummary;

const REVIEW: PromptInfo = { path: "/work/shop/.tandem/prompts/review.md", title: "Review", description: "Risky diffs", source: "repo", variables: ["branch"] };
const TICKET: PromptInfo = { path: "/data/prompts/ticket.md", title: "Ticket", description: null, source: "profile", variables: ["ticket", "selection"] };

function pane(): PaneRecord {
  return { id: "pn_1", tabId: "tab_1", workspaceId: "ws_1", kind: "agent", cwd: null, engineId: "claude", accountId: null, humanName: "Claude Code · 1", title: null, restoreState: "none", createdAt: 0, browserUrl: null } as PaneRecord;
}

let pasted: string[] = [];

function setup(list: Partial<PromptList> = {}) {
  const calls = fakeBackend({
    workspace_list: () => [WS],
    prompts_list: () => ({ items: [REVIEW, TICKET], skipped: [], excludeHidesRepo: false, ...list }),
    prompts_render: (a): Rendered => {
      const values = (a?.values ?? {}) as Record<string, string>;
      if (a?.path === REVIEW.path) return { text: "Review the diff on main", missing: [], values: { branch: "main" } };
      const missing = ["ticket", "selection"].filter((k) => !values[k]);
      return { text: `Ticket ${values.ticket ?? "{{ticket}}"}: ${values.selection ?? ""}`, missing, values };
    },
    prompts_create: () => ({ path: "/data/prompts/new.md" }),
    prompts_share_repo: () => null,
  });
  useUi.setState({ mode: "code" });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  useLayout.setState({ tabs: { tab_1: { tree: null, active: "pn_1", maximized: null, panes: { pn_1: pane() }, keepEmpty: false } } });
  pasted = [];
  terminalHandles.set("pn_1", { paste: (t: string) => pasted.push(t), focus: () => undefined, selection: () => "" } as unknown as TerminalHandle);
  render(
    <UiProvider>
      <PromptPicker />
    </UiProvider>,
  );
  act(() => usePrompts.getState().show());
  return calls;
}

const input = () => screen.getByPlaceholderText("Search saved prompts");
const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;

describe("Prompt picker (UI-145)", () => {
  beforeEach(() => resetStores());
  afterEach(() => terminalHandles.clear());

  it("F-EX-05.1 lists the workspace's and the profile's prompts with their source", async () => {
    const calls = setup();
    const rows = await screen.findAllByTestId("prompt-row");
    expect(rows.map((r) => `${r.dataset.title}|${r.dataset.source}`)).toEqual(["Review|repo", "Ticket|profile"]);
    expect(rows[0]?.textContent).toContain("This repository");
    expect(rows[1]?.textContent).toContain("Yours");
    expect(calls.find((c) => c.cmd === "prompts_list")?.args).toEqual({ context: { type: "workspace", workspaceId: "ws_1" } });
    fireEvent.change(input(), { target: { value: "risky" } });
    await waitFor(() => expect(screen.getAllByTestId("prompt-row").map((r) => r.dataset.title)).toEqual(["Review"]));
  });

  it("F-EX-05.3 a filled prompt goes into the pane as a paste and is never sent", async () => {
    setup();
    await screen.findAllByTestId("prompt-row");
    fireEvent.keyDown(input(), { key: "Enter" });
    await waitFor(() => expect(pasted).toEqual(["Review the diff on main"]));
    expect(pasted[0]?.endsWith("\r")).toBe(false);
  });

  it("F-EX-05.2 variables without a value are asked for before inserting", async () => {
    const calls = setup();
    await screen.findAllByTestId("prompt-row");
    fireEvent.change(input(), { target: { value: "ticket" } });
    await waitFor(() => expect(screen.getAllByTestId("prompt-row").length).toBe(1));
    fireEvent.keyDown(input(), { key: "Enter" });
    const dialog = await screen.findByRole("dialog", { name: "Fill in the prompt" });
    const fields = within(dialog).getAllByTestId("prompt-var");
    expect(fields.map((f) => f.dataset.name)).toEqual(["ticket", "selection"]);
    fireEvent.change(fields[0]!, { target: { value: "T-12" } });
    fireEvent.change(fields[1]!, { target: { value: "boom" } });
    await waitFor(() => expect((within(dialog).getByRole("textbox", { name: "Preview" }) as HTMLTextAreaElement).value).toBe("Ticket T-12: boom"));
    fireEvent.click(within(dialog).getByTestId("prompt-insert"));
    await waitFor(() => expect(pasted).toEqual(["Ticket T-12: boom"]));
    expect(count(calls, "prompts_render")).toBeGreaterThan(1);
  });

  it("F-EX-05.2 Insert right after typing uses what was typed, not a stale preview", async () => {
    setup();
    await screen.findAllByTestId("prompt-row");
    fireEvent.change(input(), { target: { value: "ticket" } });
    await waitFor(() => expect(screen.getAllByTestId("prompt-row").length).toBe(1));
    fireEvent.keyDown(input(), { key: "Enter" });
    const dialog = await screen.findByRole("dialog", { name: "Fill in the prompt" });
    const fields = within(dialog).getAllByTestId("prompt-var");
    fireEvent.change(fields[0]!, { target: { value: "T-7" } });
    fireEvent.change(fields[1]!, { target: { value: "x" } });
    fireEvent.click(within(dialog).getByTestId("prompt-insert"));
    await waitFor(() => expect(pasted).toEqual(["Ticket T-7: x"]));
  });

  it("F-EX-05.4 new prompt and sharing the repository's prompts", async () => {
    const calls = setup({ excludeHidesRepo: true, items: [] });
    expect(await screen.findByTestId("prompts-empty")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Share with the repository" }));
    await waitFor(() => expect(count(calls, "prompts_share_repo")).toBe(1));
    fireEvent.keyDown(input(), { key: "Enter" });
    const dialog = await screen.findByRole("dialog", { name: "New prompt" });
    fireEvent.change(within(dialog).getByTestId("prompt-name"), { target: { value: "Write tests" } });
    fireEvent.click(within(dialog).getByTestId("prompt-create"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "prompts_create")?.args).toMatchObject({ scope: "repo", name: "Write tests" }));
  });
});
