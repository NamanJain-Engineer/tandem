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

// A build without Vibezeo accounts (TANDEM_HOSTED=0) is always Free and
// can't sign in: nothing it shows may promise either (D-909).
const NO_ACCOUNTS = { ...SIGNED_IN, signedIn: false, email: null, hosted: false };
const EARLY = "Pro and Team start with Vibezeo accounts. This build runs the Free plan.";

const META: TasksMeta = {
  version: 1,
  next_number: 1,
  projects: [],
  labels: [],
  statuses: ["backlog", "todo", "in_progress", "in_review", "done", "canceled"],
  actors: [{ id: "me", kind: "human", name: "You" }],
  teams: [{ id: "local", name: "Local" }],
};

function pro() {
  useSession.setState({
    entitlement: { ...FREE_VIEW, plan: "pro", limits: { ...FREE, routines: true, taskExecution: true } },
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
  it("F-TK-04 Execute says the build runs the Free plan, not sign in, and offers no Upgrade", () => {
    useSession.setState({ auth: NO_ACCOUNTS });
    const { result } = renderHook(() => useExecuteBlock());
    expect(result.current?.reason).toContain(EARLY);
    expect(result.current?.reason).not.toMatch(/sign in/i);
    expect(result.current?.upgrade).toBe(false);
    // A hosted build still asks for sign-in.
    useSession.setState({ auth: { ...SIGNED_IN, signedIn: false } });
    const hosted = renderHook(() => useExecuteBlock());
    expect(hosted.result.current?.reason).toMatch(/^Sign in to run tasks/);
  });

  it("F-RT-04 the Routines upsell says the build runs the Free plan and offers no Upgrade", async () => {
    useSession.setState({ auth: NO_ACCOUNTS });
    render(
      <UiProvider>
        <RoutinesPage />
      </UiProvider>,
    );
    expect(await screen.findByText(/This build runs the Free plan\./)).toBeTruthy();
    expect(screen.queryByText(/Sign in/)).toBeNull();
    expect(screen.queryByTestId("routines-upgrade")).toBeNull();
  });

  it("F-TK-03 the empty Tasks page only says Execute runs work where it can", async () => {
    const { unmount } = render(
      <UiProvider>
        <TasksPanel />
      </UiProvider>,
    );
    expect(
      await screen.findByText(/Agents in your threads and teammate chats can read and update/),
    ).toBeTruthy();
    expect(screen.queryByText(/choose Execute/)).toBeNull();
    unmount();
    pro();
    render(
      <UiProvider>
        <TasksPanel />
      </UiProvider>,
    );
    expect(await screen.findByText(/Nothing runs until you choose Execute/)).toBeTruthy();
  });
});

describe("Pause routines without routines (F-RT-04)", () => {
  it("F-RT-04 the palette command and the Settings row are absent on a plan without routines", () => {
    const pause = COMMANDS.find((c) => c.id === "routines.pause")!;
    expect(isEnabled(pause)).toBe(false);
    const rows = (routines: boolean) =>
      settingsPages("linux", routines)
        .flatMap((p) => p.rows)
        .map((r) => r.id);
    expect(rows(false)).not.toContain("pause_all_routines");
    pro();
    expect(isEnabled(pause)).toBe(true);
    expect(rows(true)).toContain("pause_all_routines");
  });
});
