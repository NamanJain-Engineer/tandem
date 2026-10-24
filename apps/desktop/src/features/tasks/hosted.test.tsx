import { render, renderHook, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { TasksMeta } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { FREE, FREE_VIEW, SIGNED_IN, fakeBackend, resetStores } from "../../test/fakeBackend";
import { COMMANDS, isEnabled } from "../commands/commands";
import { RoutinesPage } from "../routines/RoutinesPage";
import { settingsPages } from "../settings/pages";
import { useExecuteBlock } from "./Execute";
import { TasksPanel } from "./TasksPanel";

// A build without Vibezeo accounts (TANDEM_HOSTED=0) can't sign in, and
// everything local runs free and unlimited there (D-909, D-869, D-2900):
// nothing it shows may promise a plan or ask for sign-in.
const NO_ACCOUNTS = { ...SIGNED_IN, signedIn: false, email: null, hosted: false };

const META: TasksMeta = {
  version: 1,
  next_number: 1,
  projects: [],
  labels: [],
  statuses: ["backlog", "todo", "in_progress", "in_review", "done", "canceled"],
  actors: [{ id: "me", kind: "human", name: "You" }],
  teams: [{ id: "local", name: "Local" }],
};

/** Limits that turn routines and task runs off (a signed token still can). */
function off() {
  useSession.setState({
    entitlement: {
      ...FREE_VIEW,
      limits: { ...FREE, routines: false, backgroundRoutines: false, taskExecution: false },
    },
  });
}

beforeEach(() => {
  resetStores();
  fakeBackend({
    tasks_meta: () => META,
    tasks_list: () => ({ items: [], next_cursor: null }),
    routines_list: () => [],
    routines_background_status: () => ({ installed: false, kind: "systemd", path: null }),
    teammate_list: () => [],
  });
});

describe("builds without accounts (D-909)", () => {
  it("F-TK-04 with no account Execute runs: no block and no sign-in (D-2902)", () => {
    useSession.setState({ auth: NO_ACCOUNTS });
    expect(renderHook(() => useExecuteBlock()).result.current).toBeNull();
    // Limits that turn runs off say so, and sell nothing.
    off();
    const blocked = renderHook(() => useExecuteBlock()).result.current;
    expect(blocked?.reason).toBe("Running tasks is turned off for this account. You can still track tasks.");
    expect(blocked?.reason).not.toMatch(/sign in|Pro|plan/i);
  });

  it("F-RT-04 with no account Routines offers New routine; limits that turn them off say so, with no Upgrade", async () => {
    useSession.setState({ auth: NO_ACCOUNTS });
    const { unmount } = render(
      <UiProvider>
        <RoutinesPage />
      </UiProvider>,
    );
    expect(await screen.findByTestId("routine-new")).toBeTruthy();
    expect(screen.queryByText(/Sign in|Pro\b|plan/)).toBeNull();
    unmount();
    off();
    render(
      <UiProvider>
        <RoutinesPage />
      </UiProvider>,
    );
    expect(await screen.findByText("Routines are turned off")).toBeTruthy();
    expect(screen.queryByTestId("routine-new")).toBeNull();
    expect(screen.queryByTestId("routines-upgrade")).toBeNull();
  });

  it("F-TK-03 the empty Tasks page only says Execute runs work where it can", async () => {
    const { unmount } = render(
      <UiProvider>
        <TasksPanel />
      </UiProvider>,
    );
    expect(await screen.findByText(/Nothing runs until you choose Execute/)).toBeTruthy();
    unmount();
    off();
    render(
      <UiProvider>
        <TasksPanel />
      </UiProvider>,
    );
    expect(
      await screen.findByText(/Agents in your threads and teammate chats can read and update/),
    ).toBeTruthy();
    expect(screen.queryByText(/choose Execute/)).toBeNull();
  });
});

describe("Pause routines without routines (F-RT-04)", () => {
  it("F-RT-04 the palette command and the Settings row are there with no account, and absent when limits turn routines off", () => {
    const pause = COMMANDS.find((c) => c.id === "routines.pause")!;
    const rows = (routines: boolean) =>
      settingsPages("linux", routines)
        .flatMap((p) => p.rows)
        .map((r) => r.id);
    expect(isEnabled(pause)).toBe(true);
    expect(rows(true)).toContain("pause_all_routines");
    off();
    expect(isEnabled(pause)).toBe(false);
    expect(rows(false)).not.toContain("pause_all_routines");
  });
});

describe("Conductor's mute and call log without Conductor (F-CN-01)", () => {
  it("F-CN-01 mute and the call log are absent without Conductor; the call command stays to say it comes with accounts (D-833, D-2903)", () => {
    const cmd = (id: string) => COMMANDS.find((c) => c.id === id)!;
    expect(isEnabled(cmd("conductor.mute"))).toBe(false);
    expect(isEnabled(cmd("conductor.log"))).toBe(false);
    expect(isEnabled(cmd("conductor.call"))).toBe(true);
    useSession.setState({ entitlement: { ...FREE_VIEW, limits: { ...FREE, conductor: true } } });
    expect(isEnabled(cmd("conductor.mute"))).toBe(true);
    expect(isEnabled(cmd("conductor.log"))).toBe(true);
  });
});

describe("Find work's hosted ranking without Vibezeo accounts (F-CN-05)", () => {
  it("F-CN-05 the Settings › Privacy row is absent without accounts (D-833)", () => {
    const rows = (accounts: boolean) =>
      settingsPages("linux", false, accounts)
        .flatMap((p) => p.rows)
        .map((r) => r.id);
    expect(rows(false)).not.toContain("find_work_hosted");
    expect(rows(true)).toContain("find_work_hosted");
  });
});
