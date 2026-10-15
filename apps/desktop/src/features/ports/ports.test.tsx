import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { ListeningPort } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useSidebar } from "../../stores/sidebar";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { PortsPanel, REFRESH_MS, urlOf } from "./PortsPanel";

function port(p: number, over: Partial<ListeningPort> = {}): ListeningPort {
  return {
    port: p,
    address: "127.0.0.1",
    allInterfaces: false,
    pid: 4000 + p,
    process: "node",
    startTime: 1700000000,
    paneId: "pn_1",
    paneName: "dev server",
    workspaceId: "ws_1",
    workspaceName: "shop",
    laneBranch: null,
    lanePorts: null,
    url: `http://localhost:${p}`,
    ...over,
  };
}

const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;

function setup(ports: ListeningPort[], stopped = true, visible = true) {
  const calls = fakeBackend({
    ports_list: () => ports,
    ports_stop: () => ({ stopped }),
    sidebar_save: (a) => a?.sidebar,
  });
  const view = render(
    <UiProvider>
      <PortsPanel visible={visible} />
    </UiProvider>,
  );
  return { calls, view };
}

describe("Ports tab (UI-142)", () => {
  beforeEach(() => resetStores());

  it("F-EX-03.1 lists each port with its process and pane; F-EX-03.5 all networks is marked", async () => {
    setup([port(5173), port(8080, { allInterfaces: true, address: "0.0.0.0", process: "python3" })]);
    const rows = await screen.findAllByTestId("port-row");
    expect(rows.map((r) => r.dataset.port)).toEqual(["5173", "8080"]);
    expect(rows[0]?.textContent).toContain("node (9173)");
    expect(rows[0]?.textContent).toContain("dev server · shop");
    expect(rows[0]?.textContent).toContain("localhost");
    expect(within(rows[1]!).getByTestId("port-network").textContent).toContain("All networks");
  });

  it("F-EX-03.2 Open shows the port in a new sidebar browser tab", async () => {
    setup([port(5173)]);
    fireEvent.click(await screen.findByTestId("port-open"));
    const tab = useSidebar.getState().tabs.find((x) => x.kind === "browser");
    expect(tab?.url).toBe("http://localhost:5173");
    expect(urlOf(port(3000))).toBe("http://localhost:3000");
  });

  it("F-EX-03.3 Stop asks first; Cancel keeps it; confirming stops it", async () => {
    const { calls } = setup([port(5173)]);
    fireEvent.click(await screen.findByTestId("port-stop"));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain("Stop node on port 5173?");
    expect(dialog.textContent).toContain("Its pane stays open.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(count(calls, "ports_stop")).toBe(0);
    fireEvent.click(screen.getByTestId("port-stop"));
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Stop process" }));
    await waitFor(() => expect(count(calls, "ports_stop")).toBe(1));
    expect(calls.find((c) => c.cmd === "ports_stop")?.args).toEqual({ paneId: "pn_1", pid: 9173, startTime: 1700000000, force: false });
  });

  it("F-EX-03.3 a process still running after the wait offers Force stop", async () => {
    const { calls } = setup([port(5173)], false);
    fireEvent.click(await screen.findByTestId("port-stop"));
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Stop process" }));
    expect(await screen.findByTestId("port-stuck")).toBeTruthy();
    fireEvent.click(within(screen.getByTestId("port-stuck")).getByRole("button", { name: "Force stop…" }));
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Force stop" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "ports_stop").at(-1)?.args).toMatchObject({ force: true }));
  });

  it("F-EX-03 empty, and no scans while hidden", async () => {
    const { calls, view } = setup([], true, false);
    await new Promise((r) => setTimeout(r, 100));
    expect(count(calls, "ports_list")).toBe(0);
    view.rerender(
      <UiProvider>
        <PortsPanel visible />
      </UiProvider>,
    );
    await waitFor(() => expect(count(calls, "ports_list")).toBe(1));
    expect(await screen.findByText("Servers started from your panes show up here.")).toBeTruthy();
    expect(REFRESH_MS).toBe(2_000);
  });

  it("F-WT-06.4 a worktree's servers are grouped under it, and Open loads its .localhost address", async () => {
    setup([
      port(3000),
      port(41000, {
        paneId: "pn_2",
        workspaceId: "wt_1",
        workspaceName: "fix-auth",
        laneBranch: "tandem/fix-auth",
        lanePorts: { start: 41000, end: 41009 },
        url: "http://fix-auth.localhost:41000",
      }),
    ]);
    const group = await screen.findByTestId("port-group");
    expect(group.dataset.branch).toBe("tandem/fix-auth");
    expect(group.textContent).toBe("tandem/fix-auth · ports 41000–41009");
    const rows = screen.getAllByTestId("port-row");
    // The plain port first, then the worktree's after its header.
    expect(rows.map((r) => r.dataset.port)).toEqual(["3000", "41000"]);
    expect(rows[1]?.dataset.branch).toBe("tandem/fix-auth");
    fireEvent.click(within(rows[1]!).getByTestId("port-open"));
    const tab = useSidebar.getState().tabs.find((x) => x.kind === "browser");
    expect(tab?.url).toBe("http://fix-auth.localhost:41000");
  });
});
