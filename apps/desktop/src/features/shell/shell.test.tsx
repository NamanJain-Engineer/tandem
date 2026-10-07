import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { emitLocal } from "../../ipc/events";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { DeepLinks } from "./DeepLinks";
import { TitleBar } from "./TitleBar";
import { WindowControls } from "./WindowControls";

const ws: WorkspaceSummary = {
  id: "ws_01JTESTTESTTESTTESTTESTTEST",
  kind: "local",
  hostId: "local",
  root: "/code/shop",
  wslDistro: null,
  name: "shop",
  parentId: null,
  laneBranch: null,
  laneBase: null,
  sortKey: "a0",
  pinned: false,
  collapsed: false,
  sessionsCollapsed: false,
  createdAt: 0,
  missing: false,
  branch: "main",
  liveCount: 0,
  tabs: [],
};

const wrap = (ui: React.ReactNode) => render(<UiProvider>{ui}</UiProvider>);

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

describe("title bar (F-SH-01)", () => {
  it("F-SH-01.5 every control has an accessible name, in visual order", () => {
    fakeBackend();
    wrap(<TitleBar />);
    const bar = screen.getByRole("banner");
    const controls = Array.from(bar.querySelectorAll<HTMLElement>("button, [role=radio]"));
    expect(controls.length).toBeGreaterThan(8);
    for (const c of controls) expect(c.getAttribute("aria-label") ?? c.textContent?.trim()).toBeTruthy();
    const names = controls.map((c) => c.getAttribute("aria-label") ?? c.textContent?.trim());
    expect(names.indexOf("Tandem menu")).toBeLessThan(names.indexOf("Toggle left sidebar"));
    expect(names.indexOf("Toggle left sidebar")).toBeLessThan(names.indexOf("Account"));
    expect(names.at(-1)).toBe("Close");
  });

  it("F-SH-01.6 the Workspace menu shows only in Code mode", () => {
    fakeBackend();
    useWorkspaces.setState({ list: [ws], activeId: ws.id });
    wrap(<TitleBar />);
    expect(screen.getByRole("button", { name: /Workspace menu: shop/ })).toBeTruthy();
    act(() => useUi.setState({ mode: "agent" }));
    expect(screen.queryByRole("button", { name: /Workspace menu/ })).toBeNull();
  });

  it("F-SH-01.4 the maximize button toggles Maximize / Restore", async () => {
    let maximized = false;
    fakeBackend({ window_toggle_maximize: () => ({ maximized: (maximized = !maximized) }) });
    wrap(<WindowControls />);
    fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Restore" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Maximize" })).toBeTruthy());
  });
});

describe("deep links (F-SH-13)", () => {
  it("F-SH-13.2 an unrecognized link shows a toast and changes nothing", async () => {
    const calls = fakeBackend();
    wrap(<DeepLinks />);
    await waitFor(() => expect(calls.some((c) => c.cmd === "deeplink_pending")).toBe(true));
    act(() => emitLocal("app:deep-link", { route: "unrecognized", params: {} }));
    await waitFor(() => expect(screen.getByText("Unrecognized link")).toBeTruthy());
    expect(calls.some((c) => c.cmd === "workspace_add")).toBe(false);
  });

  it("F-SH-13.1 open?path= adds the workspace, including links queued before the UI was ready", async () => {
    const calls = fakeBackend({
      deeplink_pending: () => [{ route: "open", params: { path: "/tmp/x" } }],
      workspace_add: () => ({ workspace: { ...ws, root: "/tmp/x", name: "x" }, created: true }),
    });
    wrap(<DeepLinks />);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "workspace_add")?.args).toEqual({ root: "/tmp/x" }),
    );
    expect(useUi.getState().mode).toBe("code");
  });
});
