import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ListeningPort, StatusRecord } from "@tandem/protocol";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { useDashboard } from "../../stores/dashboard";
import { useSession } from "../../stores/session";
import { useSidebar } from "../../stores/sidebar";
import { useStatus } from "../../stores/status";
import { BOOT, FREE_VIEW, SIGNED_IN, fakeBackend, resetStores } from "../../test/fakeBackend";
import { StatusBar } from "./StatusBar";
import { useBoot } from "./useBoot";

const show = () =>
  render(
    <UiProvider>
      <StatusBar />
    </UiProvider>,
  );

describe("status bar connection (UI §3.6)", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend();
    useBoot.setState({ boot: { ...BOOT, version: "0.4.0" } });
  });

  it("an early-access build (no hosted services) claims no connection, only its version", () => {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: false },
      entitlement: { ...FREE_VIEW, online: true },
    });
    show();
    expect(screen.queryByTestId("connection-state")).toBeNull();
    expect(screen.getByText("v0.4.0")).toBeTruthy();
  });

  it("a hosted build says Online, and Offline after a failed refresh", () => {
    useSession.setState({ auth: SIGNED_IN, entitlement: { ...FREE_VIEW, online: true } });
    const { unmount } = show();
    expect(screen.getByTestId("connection-state").textContent).toBe("Online");
    unmount();
    useSession.setState({ auth: SIGNED_IN, entitlement: { ...FREE_VIEW, online: false } });
    show();
    expect(screen.getByTestId("connection-state").textContent).toBe("Offline · local features still work");
  });
});

const rec = (key: string, status: StatusRecord["status"]): StatusRecord => ({
  key,
  status,
  reason: null,
  since: 1,
  ts: 1,
  tool: null,
  title: null,
  preview: null,
});

const listening = (port: number): ListeningPort => ({
  port,
  address: "127.0.0.1",
  allInterfaces: false,
  pid: 4000 + port,
  process: "node",
  startTime: 1700000000,
  paneId: "pn_1",
  paneName: "dev server",
  workspaceId: "ws_1",
  workspaceName: "shop",
  laneBranch: null,
  lanePorts: null,
  url: `http://localhost:${port}`,
});

describe("D-2603 status bar at a glance (UI §3.6)", () => {
  beforeEach(() => {
    resetStores();
    useStatus.setState({ records: {}, unread: {} });
    useBoot.setState({ boot: { ...BOOT, version: "0.9.0" } });
  });

  it("counts agents working and waiting, and each opens the Dashboard filtered to it", async () => {
    fakeBackend({ ports_list: () => [] });
    useStatus.setState({
      records: {
        "pane:a": rec("pane:a", "working"),
        "pane:b": rec("pane:b", "working"),
        "thread:c": rec("thread:c", "waiting"),
        "pane:d": rec("pane:d", "idle"),
      },
    });
    show();
    expect(screen.getByTestId("status-working").textContent).toBe("2 working");
    expect(screen.getByTestId("status-waiting").textContent).toBe("1 waiting");
    fireEvent.click(screen.getByTestId("status-waiting"));
    expect(useDashboard.getState().filter).toBe("waiting");
    expect(useSidebar.getState().open).toBe(true);
    expect(useSidebar.getState().active).toBe("dashboard");
  });

  it("shows nothing for agents when none is busy", () => {
    fakeBackend({ ports_list: () => [] });
    useStatus.setState({ records: { "pane:d": rec("pane:d", "idle") } });
    show();
    expect(screen.queryByTestId("status-working")).toBeNull();
    expect(screen.queryByTestId("status-waiting")).toBeNull();
  });

  it("names one listening port and counts several", async () => {
    fakeBackend({ ports_list: () => [listening(5173)] });
    const { unmount } = show();
    await waitFor(() => expect(screen.getByTestId("status-ports").textContent).toBe(":5173"));
    unmount();
    fakeBackend({ ports_list: () => [listening(5173), listening(8080), listening(9229)] });
    show();
    await waitFor(() => expect(screen.getByTestId("status-ports").textContent).toBe("3 ports"));
  });
});
